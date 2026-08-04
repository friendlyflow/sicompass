//! Wire-level tests for `RealImap` against a fake in-process IMAP server.
//!
//! These tests exist to pin the **observable IMAP wire behaviour** of
//! `RealImap`: the exact commands it sends and how it decodes the responses.
//! The 68 `MockImap` tests in `lib.rs` sit *above* the `ImapBackend` trait and
//! would keep passing even if the wire layer were completely broken, and the
//! only other coverage (`real_imap_smoke`) is `#[ignore]`d behind live
//! credentials.
//!
//! They are written against the current `imap` 2.x backend and **must pass
//! unchanged after the `async-imap` migration** — that is the behaviour
//! preservation proof for the port.
//!
//! The server speaks just enough IMAP to drive `RealImap`: it is a scripted
//! responder, not a real mailbox. It listens on `127.0.0.1:0` and is reached
//! over plaintext `imap://`, which `connection::open_stream` permits only for
//! loopback addresses.
//!
//! # Bugs this suite found on first run
//!
//! 1. **IDLE never raised the refresh flag.** `run_idle_session` looked for the
//!    EXISTS in `session.unsolicited_responses`, but the idle handle consumes
//!    that line into its own buffer and the DONE handshake does not route
//!    untagged responses to that channel, so the drain was always empty. Fixed
//!    in `idle.rs` by treating `WaitOutcome::MailboxChanged` as the signal.
//! 2. **`fetch_threads` could never succeed.** No released imap-proto parses a
//!    `* THREAD` response (checked: 0.10.2 and 0.16.7, so the `async-imap`
//!    migration would not have fixed it either), and the command errored out
//!    before `parse_thread_response` saw anything. Fixed by running THREAD over
//!    `connection::RawImap`, which reads the response without imap-proto.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use base64::Engine as _;
use sicompass_emailclient::idle::IdleController;
use sicompass_emailclient::net::RealImap;
use sicompass_emailclient::{EmailClientConfig, ImapBackend, MailBody};

// ---------------------------------------------------------------------------
// Test isolation
// ---------------------------------------------------------------------------

/// Point the SQLite envelope cache at a scratch directory.
///
/// `EnvelopeCache::open` resolves its DB path through
/// `sicompass_sdk::platform::cache_home()`. Without this the tests would read
/// and write the developer's real email cache, and results would depend on
/// previous runs.
///
/// The DB file is keyed on the username, so each test additionally uses a
/// unique one (see [`unique_user`]) and therefore its own cache file.
///
/// Which variable to set is platform-specific, because `cache_home()` only
/// consults `XDG_CACHE_HOME` on Linux: Windows resolves `%LOCALAPPDATA%` and
/// macOS derives `~/Library/Caches` from `$HOME`. Setting the XDG one alone
/// isolated Linux and left the other two writing to the real cache, which is
/// both the pollution this function exists to prevent and a source of failures
/// that look like flakes: a warm cache lets `list_messages` answer from SQLite
/// without issuing the FETCH the tests assert on, so a suite that passed on a
/// clean machine failed on the second run.
fn isolate_cache() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("sicompass-fake-imap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch cache dir");
        // Safety: single-threaded initialisation guarded by `Once`, run before
        // any test touches the cache.
        unsafe {
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            std::env::set_var("XDG_CACHE_HOME", &dir);
            #[cfg(target_os = "windows")]
            std::env::set_var("LOCALAPPDATA", &dir);
            #[cfg(target_os = "macos")]
            std::env::set_var("HOME", &dir);
        }
    });
}

/// A username no other test shares, so no test can see another's cached
/// envelopes.
fn unique_user(tag: &str) -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    format!("{tag}-{}@example.com", N.fetch_add(1, Ordering::Relaxed))
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// The two messages the fake INBOX contains.
///
/// UID 1 is plain and `\Seen`; UID 2 is multipart with an attachment and
/// `\Flagged`, so a single mailbox exercises both the envelope path and the
/// MIME parser.
const ENVELOPE_1: &str = "\"Mon, 1 Jan 2025 00:00:00 +0000\" \"First subject\" \
     ((\"Alice\" NIL \"alice\" \"example.com\")) \
     ((\"Alice\" NIL \"alice\" \"example.com\")) \
     ((\"Alice\" NIL \"alice\" \"example.com\")) \
     ((\"Bob\" NIL \"bob\" \"example.com\")) \
     NIL NIL NIL \"<msg1@example.com>\"";

/// UID 2's From has no display name, covering `format_address`'s second branch.
const ENVELOPE_2: &str = "\"Tue, 2 Jan 2025 09:30:00 +0000\" \"Second subject\" \
     ((NIL NIL \"carol\" \"example.com\")) \
     ((NIL NIL \"carol\" \"example.com\")) \
     ((NIL NIL \"carol\" \"example.com\")) \
     ((\"Bob\" NIL \"bob\" \"example.com\")) \
     NIL NIL \"<msg1@example.com>\" \"<msg2@example.com>\"";

/// Raw RFC 2822 source returned for `UID FETCH 2 BODY[]`.
fn message_2_source() -> String {
    // `aGVsbG8sIHdvcmxk` is "hello, world".
    concat!(
        "From: carol@example.com\r\n",
        "To: Bob <bob@example.com>\r\n",
        "Subject: Second subject\r\n",
        "Date: Tue, 2 Jan 2025 09:30:00 +0000\r\n",
        "Message-ID: <msg2@example.com>\r\n",
        "In-Reply-To: <msg1@example.com>\r\n",
        "References: <msg1@example.com>\r\n",
        "Content-Type: multipart/mixed; boundary=\"bnd42\"\r\n",
        "\r\n",
        "--bnd42\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "Body of the second message.\r\n",
        "--bnd42\r\n",
        "Content-Type: application/octet-stream\r\n",
        "Content-Disposition: attachment; filename=\"notes.txt\"\r\n",
        "Content-Transfer-Encoding: base64\r\n",
        "\r\n",
        "aGVsbG8sIHdvcmxk\r\n",
        "--bnd42--\r\n",
    )
    .to_owned()
}

// ---------------------------------------------------------------------------
// Fake server
// ---------------------------------------------------------------------------

/// How the fake server should behave for a given test.
#[derive(Clone)]
struct Options {
    /// Advertised capabilities, after `IMAP4rev1`.
    capabilities: String,
    /// Answer `UID MOVE` with `NO`, forcing the COPY + `\Deleted` + EXPUNGE
    /// fallback in `move_message`.
    reject_move: bool,
    /// Emit `* {n} EXISTS` while idling, then hang up so the IDLE worker's
    /// reconnect back-off (not a 30 s poll) is what the test waits on.
    idle_exists: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            capabilities: "UIDPLUS MOVE".to_owned(),
            reject_move: false,
            idle_exists: false,
        }
    }
}

/// A scripted IMAP server on loopback.
///
/// Accepts connections until dropped, and records every client command line so
/// tests can assert on the exact wire traffic.
struct FakeImap {
    addr: SocketAddr,
    log: Arc<Mutex<Vec<String>>>,
    shutdown: Arc<AtomicBool>,
}

impl FakeImap {
    fn start(opts: Options) -> Self {
        isolate_cache();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local_addr");
        listener.set_nonblocking(true).expect("set_nonblocking");

        let log = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let thread_log = Arc::clone(&log);
        let thread_shutdown = Arc::clone(&shutdown);
        std::thread::spawn(move || {
            while !thread_shutdown.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // `accept` on a non-blocking listener yields a blocking
                        // socket on Linux, but do not rely on that.
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                        let conn_log = Arc::clone(&thread_log);
                        let conn_opts = opts.clone();
                        std::thread::spawn(move || {
                            serve(stream, conn_opts, conn_log);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        FakeImap { addr, log, shutdown }
    }

    fn url(&self) -> String {
        format!("imap://127.0.0.1:{}", self.addr.port())
    }

    /// Config pointed at this server, authenticating with LOGIN.
    fn config(&self, user: &str) -> EmailClientConfig {
        EmailClientConfig {
            imap_url: self.url(),
            username: user.to_owned(),
            password: "hunter2".to_owned(),
            ..Default::default()
        }
    }

    /// Every command line the server has received so far.
    fn commands(&self) -> Vec<String> {
        self.log.lock().expect("log mutex").clone()
    }

    /// The first logged command containing `needle`, panicking with the full
    /// transcript when there is none — a bare `assert!(any(...))` failure would
    /// not say what the client actually sent.
    fn expect_command(&self, needle: &str) -> String {
        let cmds = self.commands();
        cmds.iter()
            .find(|c| c.contains(needle))
            .unwrap_or_else(|| {
                panic!("no command containing {needle:?}; transcript:\n  {}", cmds.join("\n  "))
            })
            .clone()
    }

    fn assert_no_command(&self, needle: &str) {
        let cmds = self.commands();
        assert!(
            !cmds.iter().any(|c| c.contains(needle)),
            "unexpected command containing {needle:?}; transcript:\n  {}",
            cmds.join("\n  ")
        );
    }

    /// Block until `pred` holds over the transcript, up to `timeout`.
    fn wait_for<F: Fn(&[String]) -> bool>(&self, timeout: Duration, pred: F) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if pred(&self.commands()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }
}

impl Drop for FakeImap {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

/// Serve one client connection.
fn serve(stream: TcpStream, opts: Options, log: Arc<Mutex<Vec<String>>>) {
    let reader_half = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut reader = BufReader::new(reader_half);
    let mut w = stream;

    if write!(w, "* OK [CAPABILITY IMAP4rev1] fake IMAP ready\r\n").is_err() {
        return;
    }
    let _ = w.flush();

    loop {
        let line = match read_line(&mut reader) {
            Some(l) => l,
            None => return,
        };
        log.lock().expect("log mutex").push(line.clone());

        let (tag, rest) = match line.split_once(' ') {
            Some((t, r)) => (t.to_owned(), r.to_owned()),
            None => (line.clone(), String::new()),
        };
        let upper = rest.to_uppercase();

        let keep_going = dispatch(&mut w, &mut reader, &opts, &log, &tag, &rest, &upper);
        let _ = w.flush();
        if !keep_going {
            return;
        }
    }
}

/// Handle one command. Returns `false` when the connection should close.
fn dispatch(
    w: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    opts: &Options,
    log: &Arc<Mutex<Vec<String>>>,
    tag: &str,
    rest: &str,
    upper: &str,
) -> bool {
    // `UID ...` variants must be matched before their bare counterparts, since
    // "UID FETCH" also contains "FETCH".
    if upper.starts_with("LOGIN") {
        send(w, &format!("{tag} OK LOGIN completed\r\n"));
    } else if upper.starts_with("AUTHENTICATE") {
        // Empty continuation, exactly as Gmail sends for XOAUTH2.
        send(w, "+ \r\n");
        let payload = read_line(reader).unwrap_or_default();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload.trim())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_else(|_| format!("<undecodable: {payload}>"));
        // Recorded with a synthetic prefix so tests can assert on the decoded
        // SASL body rather than its base64 envelope.
        log.lock().expect("log mutex").push(format!("SASL-PAYLOAD {decoded}"));
        send(w, &format!("{tag} OK AUTHENTICATE completed\r\n"));
    } else if upper.starts_with("CAPABILITY") {
        send(w, &format!("* CAPABILITY IMAP4rev1 {}\r\n", opts.capabilities));
        send(w, &format!("{tag} OK CAPABILITY completed\r\n"));
    } else if upper.starts_with("LIST") {
        // `[Gmail]` is \Noselect and must be filtered out by `list_folders`.
        send(w, "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n");
        send(w, "* LIST (\\HasChildren \\Noselect) \"/\" \"[Gmail]\"\r\n");
        send(w, "* LIST (\\HasNoChildren \\Trash) \"/\" \"[Gmail]/Trash\"\r\n");
        send(w, "* LIST (\\HasNoChildren \\Sent) \"/\" \"[Gmail]/Sent Mail\"\r\n");
        send(w, "* LIST (\\HasNoChildren \\All) \"/\" \"[Gmail]/All Mail\"\r\n");
        send(w, &format!("{tag} OK LIST completed\r\n"));
    } else if upper.starts_with("SELECT") || upper.starts_with("EXAMINE") {
        send(w, "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n");
        send(w, "* 2 EXISTS\r\n");
        send(w, "* 0 RECENT\r\n");
        send(w, "* OK [UIDVALIDITY 4242] UIDs valid\r\n");
        send(w, "* OK [UIDNEXT 3] Predicted next UID\r\n");
        send(w, &format!("{tag} OK [READ-WRITE] SELECT completed\r\n"));
    } else if upper.starts_with("UID SEARCH") || upper.starts_with("SEARCH") {
        // Every fixture search resolves to UID 2.
        send(w, "* SEARCH 2\r\n");
        send(w, &format!("{tag} OK SEARCH completed\r\n"));
    } else if upper.starts_with("UID THREAD") || upper.starts_with("THREAD") {
        send(w, "* THREAD (1 2)(3)\r\n");
        send(w, &format!("{tag} OK THREAD completed\r\n"));
    } else if upper.starts_with("UID FETCH") || upper.starts_with("FETCH") {
        if upper.contains("BODY[]") {
            fetch_body(w, tag, rest);
        } else {
            send(w, &format!("* 1 FETCH (UID 1 ENVELOPE ({ENVELOPE_1}) FLAGS (\\Seen))\r\n"));
            send(w, &format!("* 2 FETCH (UID 2 ENVELOPE ({ENVELOPE_2}) FLAGS (\\Flagged))\r\n"));
            send(w, &format!("{tag} OK FETCH completed\r\n"));
        }
    } else if upper.starts_with("UID STORE") || upper.starts_with("STORE") {
        send(w, "* 2 FETCH (UID 2 FLAGS (\\Seen))\r\n");
        send(w, &format!("{tag} OK STORE completed\r\n"));
    } else if upper.starts_with("UID COPY") || upper.starts_with("COPY") {
        send(w, &format!("{tag} OK [COPYUID 4242 2 9] COPY completed\r\n"));
    } else if upper.starts_with("UID MOVE") || upper.starts_with("MOVE") {
        if opts.reject_move {
            send(w, &format!("{tag} NO [CANNOT] MOVE not supported\r\n"));
        } else {
            send(w, &format!("{tag} OK [COPYUID 4242 2 9] MOVE completed\r\n"));
        }
    } else if upper.starts_with("UID EXPUNGE") || upper.starts_with("EXPUNGE") {
        send(w, "* 2 EXPUNGE\r\n");
        send(w, &format!("{tag} OK EXPUNGE completed\r\n"));
    } else if upper.starts_with("APPEND") {
        return append(w, reader, log, tag, rest);
    } else if upper.starts_with("IDLE") {
        return idle(w, reader, opts, log, tag);
    } else if upper.starts_with("CLOSE") {
        send(w, &format!("{tag} OK CLOSE completed\r\n"));
    } else if upper.starts_with("LOGOUT") {
        send(w, "* BYE fake server logging out\r\n");
        send(w, &format!("{tag} OK LOGOUT completed\r\n"));
        return false;
    } else {
        send(w, &format!("{tag} BAD unrecognised command\r\n"));
    }
    true
}

/// `UID FETCH <uid> BODY[]` — reply with a literal, or nothing when the UID is
/// unknown, which is how `fetch_message` learns to return `Ok(None)`.
fn fetch_body(w: &mut TcpStream, tag: &str, rest: &str) {
    // "UID FETCH 2 BODY[]" → 2
    let uid: u32 = rest
        .split_whitespace()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if uid == 2 {
        let body = message_2_source();
        send(w, &format!("* 2 FETCH (UID 2 BODY[] {{{}}}\r\n", body.len()));
        send(w, &body);
        send(w, ")\r\n");
    }
    send(w, &format!("{tag} OK FETCH completed\r\n"));
}

/// `APPEND <folder> {<len>}` — continuation, then the literal, then OK.
fn append(
    w: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    log: &Arc<Mutex<Vec<String>>>,
    tag: &str,
    rest: &str,
) -> bool {
    let len: usize = match rest
        .rsplit_once('{')
        .and_then(|(_, n)| n.trim_end_matches(['}', '+']).trim_end_matches('}').parse().ok())
    {
        Some(n) => n,
        None => {
            send(w, &format!("{tag} BAD APPEND needs a literal\r\n"));
            return true;
        }
    };

    send(w, "+ Ready for literal data\r\n");
    let _ = w.flush();

    let mut buf = vec![0u8; len];
    if reader.read_exact(&mut buf).is_err() {
        return false;
    }
    // Trailing CRLF after the literal.
    let _ = read_line(reader);

    log.lock()
        .expect("log mutex")
        .push(format!("APPEND-LITERAL {}", String::from_utf8_lossy(&buf)));
    send(w, &format!("{tag} OK [APPENDUID 4242 9] APPEND completed\r\n"));
    true
}

/// `IDLE` — continuation, an optional unsolicited EXISTS, then wait for DONE.
fn idle(
    w: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    opts: &Options,
    log: &Arc<Mutex<Vec<String>>>,
    tag: &str,
) -> bool {
    send(w, "+ idling\r\n");
    let _ = w.flush();

    if opts.idle_exists {
        // New mail arrives while the client is idling.
        std::thread::sleep(Duration::from_millis(20));
        send(w, "* 3 EXISTS\r\n");
        let _ = w.flush();
    }

    // The client answers DONE once it stops idling.
    match read_line(reader) {
        Some(done) => {
            log.lock().expect("log mutex").push(done);
            send(w, &format!("{tag} OK IDLE terminated\r\n"));
        }
        None => return false,
    }

    // Hang up rather than serving a second IDLE. The worker then fails fast on
    // reconnect instead of blocking the test for a full 30 s poll interval.
    !opts.idle_exists
}

fn send(w: &mut TcpStream, s: &str) {
    let _ = w.write_all(s.as_bytes());
}

/// Read one CRLF-terminated line, without the terminator. `None` on EOF.
fn read_line(reader: &mut BufReader<TcpStream>) -> Option<String> {
    let mut buf = Vec::new();
    match reader.read_until(b'\n', &mut buf) {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            while matches!(buf.last(), Some(b'\r') | Some(b'\n')) {
                buf.pop();
            }
            Some(String::from_utf8_lossy(&buf).into_owned())
        }
    }
}

// ---------------------------------------------------------------------------
// Connection and authentication
// ---------------------------------------------------------------------------

#[tokio::test]
async fn login_sends_credentials_after_greeting() {
    let server = FakeImap::start(Options::default());
    let user = unique_user("login");
    let mut imap = RealImap::from_config(&server.config(&user));

    imap.list_folders().await.expect("list_folders");

    let login = server.expect_command("LOGIN");
    assert!(login.contains(&user), "LOGIN should carry the username: {login}");
    assert!(login.contains("hunter2"), "LOGIN should carry the password: {login}");
    server.assert_no_command("AUTHENTICATE");
}

#[tokio::test]
async fn xoauth2_sends_the_exact_sasl_payload() {
    let server = FakeImap::start(Options::default());
    let user = unique_user("xoauth2");
    let mut config = server.config(&user);
    config.oauth_access_token = "ya29.token".to_owned();
    let mut imap = RealImap::from_config(&config);

    imap.list_folders().await.expect("list_folders");

    server.expect_command("AUTHENTICATE XOAUTH2");
    let payload = server.expect_command("SASL-PAYLOAD");
    assert_eq!(
        payload,
        format!("SASL-PAYLOAD user={user}\u{1}auth=Bearer ya29.token\u{1}\u{1}"),
        "the XOAUTH2 SASL body is what Gmail accepts or rejects; it must not drift"
    );
    server.assert_no_command("LOGIN");
}

// ---------------------------------------------------------------------------
// Folders and envelopes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_folders_keeps_special_use_and_drops_noselect() {
    let server = FakeImap::start(Options::default());
    let mut imap = RealImap::from_config(&server.config(&unique_user("folders")));

    let folders = imap.list_folders().await.expect("list_folders");

    let names: Vec<&str> = folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["INBOX", "[Gmail]/Trash", "[Gmail]/Sent Mail", "[Gmail]/All Mail"],
        "\\Noselect containers must not appear as selectable folders"
    );

    let trash = folders.iter().find(|f| f.name == "[Gmail]/Trash").expect("Trash");
    assert!(
        trash.attributes.iter().any(|a| a == "\\Trash"),
        "SPECIAL-USE attributes drive trash/archive routing: {:?}",
        trash.attributes
    );
}

#[tokio::test]
async fn list_messages_decodes_envelopes_flags_and_orders_newest_first() {
    let server = FakeImap::start(Options::default());
    let mut imap = RealImap::from_config(&server.config(&unique_user("headers")));

    let headers = imap.list_messages("INBOX", 50).await.expect("list_messages");

    assert_eq!(headers.len(), 2);
    // Most-recent-first.
    assert_eq!(headers[0].uid, 2);
    assert_eq!(headers[1].uid, 1);

    assert_eq!(headers[1].from, "Alice <alice@example.com>");
    assert_eq!(headers[1].subject, "First subject");
    assert_eq!(headers[1].date, "Mon, 1 Jan 2025 00:00:00 +0000");
    assert!(headers[1].seen);
    assert!(!headers[1].flagged);

    // No display name — `format_address` falls back to mailbox@host.
    assert_eq!(headers[0].from, "carol@example.com");
    assert!(!headers[0].seen);
    assert!(headers[0].flagged);

    server.expect_command("SELECT");
    let fetch = server.expect_command("FETCH");
    assert!(
        fetch.contains("(UID ENVELOPE FLAGS)"),
        "header listing must not pull bodies: {fetch}"
    );
}

// ---------------------------------------------------------------------------
// Message bodies
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_message_parses_literal_body_and_attachment() {
    let server = FakeImap::start(Options::default());
    let mut imap = RealImap::from_config(&server.config(&unique_user("body")));

    let msg = imap
        .fetch_message("INBOX", 2).await
        .expect("fetch_message")
        .expect("UID 2 exists");

    assert_eq!(msg.uid, 2);
    assert_eq!(msg.subject, "Second subject");
    assert_eq!(msg.message_id, "<msg2@example.com>");
    assert_eq!(msg.in_reply_to, "<msg1@example.com>");
    match &msg.body {
        MailBody::Text(t) => assert!(
            t.contains("Body of the second message."),
            "multipart/mixed text part should surface as the body: {t:?}"
        ),
        other => panic!("expected a text body, got {other:?}"),
    }

    let att = msg.attachments.first().expect("one attachment");
    assert_eq!(att.filename, "notes.txt");
    assert_eq!(att.data, b"hello, world", "base64 attachment must be decoded");

    let fetch = server.expect_command("BODY[]");
    assert!(fetch.contains("UID FETCH 2"), "must fetch by UID, not sequence: {fetch}");
}

#[tokio::test]
async fn fetch_message_returns_none_for_unknown_uid() {
    let server = FakeImap::start(Options::default());
    let mut imap = RealImap::from_config(&server.config(&unique_user("missing")));

    // The fake server answers UID 99 with a bare OK and no FETCH data.
    let msg = imap.fetch_message("INBOX", 99).await.expect("fetch_message");

    assert!(msg.is_none(), "a missing UID is Ok(None), not an error");
    server.expect_command("UID FETCH 99");
}

#[tokio::test]
async fn fetch_message_by_message_id_searches_then_fetches() {
    let server = FakeImap::start(Options::default());
    let mut imap = RealImap::from_config(&server.config(&unique_user("byid")));

    let msg = imap
        .fetch_message_by_message_id("INBOX", "<msg2@example.com>").await
        .expect("fetch_message_by_message_id")
        .expect("search resolves to UID 2");

    assert_eq!(msg.uid, 2);
    let search = server.expect_command("UID SEARCH");
    assert!(
        search.contains("HEADER Message-ID <msg2@example.com>"),
        "must search on the Message-ID header: {search}"
    );
    server.expect_command("UID FETCH 2");
}

// ---------------------------------------------------------------------------
// Mutations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn set_flags_issues_separate_add_and_remove_stores() {
    let server = FakeImap::start(Options::default());
    let mut imap = RealImap::from_config(&server.config(&unique_user("flags")));

    imap.set_flags("INBOX", 2, &["\\Seen"], &["\\Flagged"]).await
        .expect("set_flags");

    let cmds = server.commands();
    let add = cmds
        .iter()
        .find(|c| c.contains("UID STORE") && c.contains("+FLAGS"))
        .unwrap_or_else(|| panic!("no +FLAGS store; transcript:\n  {}", cmds.join("\n  ")));
    let remove = cmds
        .iter()
        .find(|c| c.contains("UID STORE") && c.contains("-FLAGS"))
        .unwrap_or_else(|| panic!("no -FLAGS store; transcript:\n  {}", cmds.join("\n  ")));

    assert!(add.contains("2 +FLAGS (\\Seen)"), "{add}");
    assert!(remove.contains("2 -FLAGS (\\Flagged)"), "{remove}");
}

#[tokio::test]
async fn set_flags_skips_the_store_when_a_side_is_empty() {
    let server = FakeImap::start(Options::default());
    let mut imap = RealImap::from_config(&server.config(&unique_user("flags-one")));

    imap.set_flags("INBOX", 2, &["\\Seen"], &[]).await.expect("set_flags");

    server.expect_command("+FLAGS");
    server.assert_no_command("-FLAGS");
}

#[tokio::test]
async fn copy_message_issues_uid_copy() {
    let server = FakeImap::start(Options::default());
    let mut imap = RealImap::from_config(&server.config(&unique_user("copy")));

    imap.copy_message("INBOX", 2, "[Gmail]/All Mail").await.expect("copy_message");

    let copy = server.expect_command("UID COPY");
    assert!(copy.contains('2') && copy.contains("All Mail"), "{copy}");
}

#[tokio::test]
async fn move_message_prefers_the_move_extension() {
    let server = FakeImap::start(Options::default());
    let mut imap = RealImap::from_config(&server.config(&unique_user("move")));

    imap.move_message("INBOX", 2, "[Gmail]/Trash").await.expect("move_message");

    server.expect_command("UID MOVE");
    server.assert_no_command("UID EXPUNGE");
}

#[tokio::test]
async fn move_message_falls_back_to_copy_delete_expunge() {
    let server = FakeImap::start(Options {
        reject_move: true,
        ..Default::default()
    });
    let mut imap = RealImap::from_config(&server.config(&unique_user("move-fallback")));

    imap.move_message("INBOX", 2, "[Gmail]/Trash").await
        .expect("fallback should still succeed");

    // Order matters: COPY must precede the \Deleted flag and the expunge, or a
    // failed copy would destroy the message.
    let cmds = server.commands();
    let pos = |needle: &str| {
        cmds.iter()
            .position(|c| c.contains(needle))
            .unwrap_or_else(|| panic!("no {needle:?}; transcript:\n  {}", cmds.join("\n  ")))
    };
    let (mv, copy, store, expunge) = (
        pos("UID MOVE"),
        pos("UID COPY"),
        pos("+FLAGS (\\Deleted)"),
        pos("UID EXPUNGE"),
    );
    assert!(mv < copy, "MOVE is tried first");
    assert!(copy < store, "COPY before marking \\Deleted");
    assert!(store < expunge, "mark \\Deleted before expunging");
}

#[tokio::test]
async fn expunge_uid_targets_a_single_uid() {
    let server = FakeImap::start(Options::default());
    let mut imap = RealImap::from_config(&server.config(&unique_user("expunge")));

    imap.expunge_uid("INBOX", 2).await.expect("expunge_uid");

    let expunge = server.expect_command("UID EXPUNGE");
    assert!(expunge.ends_with('2'), "must scope the expunge to UID 2: {expunge}");
}

#[tokio::test]
async fn append_transfers_the_message_as_a_literal() {
    let server = FakeImap::start(Options::default());
    let mut imap = RealImap::from_config(&server.config(&unique_user("append")));

    let raw = b"From: a@b.com\r\nSubject: Saved\r\n\r\nSent copy.\r\n";
    imap.append("[Gmail]/Sent Mail", raw).await.expect("append");

    let cmd = server.expect_command("APPEND");
    assert!(cmd.contains("Sent Mail"), "{cmd}");
    assert!(cmd.contains(&format!("{{{}}}", raw.len())), "literal length: {cmd}");

    let literal = server.expect_command("APPEND-LITERAL");
    assert!(
        literal.contains("Subject: Saved") && literal.contains("Sent copy."),
        "the appended bytes must arrive intact: {literal}"
    );
}

// ---------------------------------------------------------------------------
// Threading
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_threads_parses_the_uid_thread_response() {
    let server = FakeImap::start(Options {
        capabilities: "UIDPLUS MOVE THREAD=REFERENCES".to_owned(),
        ..Default::default()
    });
    let mut imap = RealImap::from_config(&server.config(&unique_user("threads")));

    let threads = imap
        .fetch_threads("INBOX").await
        .expect("fetch_threads")
        .expect("server advertises THREAD=REFERENCES");

    assert_eq!(threads, vec![vec![1, 2], vec![3]]);
    let thread = server.expect_command("UID THREAD");
    assert!(thread.contains("REFERENCES UTF-8 ALL"), "{thread}");
}

#[tokio::test]
async fn fetch_threads_returns_none_without_the_capability() {
    let server = FakeImap::start(Options::default()); // no THREAD=* advertised
    let mut imap = RealImap::from_config(&server.config(&unique_user("nothreads")));

    let threads = imap.fetch_threads("INBOX").await.expect("fetch_threads");

    assert!(
        threads.is_none(),
        "without THREAD support the caller must fall back to the SEARCH path"
    );
    server.assert_no_command("UID THREAD");
}

#[tokio::test]
async fn fetch_threads_falls_back_to_orderedsubject() {
    let server = FakeImap::start(Options {
        capabilities: "UIDPLUS THREAD=ORDEREDSUBJECT".to_owned(),
        ..Default::default()
    });
    let mut imap = RealImap::from_config(&server.config(&unique_user("ordsubj")));

    imap.fetch_threads("INBOX").await
        .expect("fetch_threads")
        .expect("ORDEREDSUBJECT is also threadable");

    let thread = server.expect_command("UID THREAD");
    assert!(thread.contains("ORDEREDSUBJECT"), "{thread}");
}

// ---------------------------------------------------------------------------
// IDLE
// ---------------------------------------------------------------------------

#[test]
fn idle_sets_the_notify_flag_on_unsolicited_exists() {
    let server = FakeImap::start(Options {
        idle_exists: true,
        ..Default::default()
    });
    let notify = Arc::new(AtomicBool::new(false));
    let mut ctrl = IdleController::new(Arc::clone(&notify));

    ctrl.start(server.config(&unique_user("idle")), "INBOX".to_owned());

    let deadline = Instant::now() + Duration::from_secs(10);
    while !notify.load(Ordering::Relaxed) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let fired = notify.load(Ordering::Relaxed);
    ctrl.stop();

    assert!(fired, "an unsolicited EXISTS must raise the refresh flag");
    assert!(
        server.wait_for(Duration::from_secs(1), |c| c.iter().any(|l| l.contains("IDLE"))),
        "the worker must actually issue IDLE"
    );
    server.expect_command("DONE");
}
