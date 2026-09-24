//! A fake IMAP mailbox on loopback, for the tests that drive the email
//! plugin (`tests/fixtures/plugins/emailclient`) the way the app runs it: in
//! the sandbox, its IMAP connection a host socket granted by name, its work
//! in host tasks.
//!
//! Just enough IMAP4rev1 for the plugin's worker and its IDLE task: LOGIN and
//! XOAUTH2, LIST, SELECT, FETCH of envelopes and bodies, STORE, UID MOVE,
//! EXPUNGE and IDLE. Unlike the scripted server in the plugin's own wire tests,
//! it keeps state: a moved message leaves INBOX, and [`FakeImap::deliver`]
//! puts a new one in, announced to a client that is idling.
//!
//! It speaks plain `imap://`, which the plugin allows for loopback only.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One message in the fake INBOX.
#[derive(Clone)]
struct Message {
    uid: u32,
    subject: String,
    from: String,
    seen: bool,
}

#[derive(Default)]
struct State {
    inbox: Vec<Message>,
    next_uid: u32,
    /// Every command line received, for assertions.
    log: Vec<String>,
}

pub struct FakeImap {
    port: u16,
    state: Arc<Mutex<State>>,
    shutdown: Arc<AtomicBool>,
}

impl FakeImap {
    /// A server whose INBOX holds `messages`, as `(subject, from address)`,
    /// all read, UIDs from 1.
    pub fn start(messages: &[(&str, &str)]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let state = Arc::new(Mutex::new(State {
            inbox: messages
                .iter()
                .enumerate()
                .map(|(i, (subject, from))| Message {
                    uid: i as u32 + 1,
                    subject: (*subject).to_owned(),
                    from: (*from).to_owned(),
                    seen: true,
                })
                .collect(),
            next_uid: messages.len() as u32 + 1,
            log: Vec::new(),
        }));
        let shutdown = Arc::new(AtomicBool::new(false));
        let (s, stop) = (Arc::clone(&state), Arc::clone(&shutdown));
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let (s, stop) = (Arc::clone(&s), Arc::clone(&stop));
                        std::thread::spawn(move || serve(stream, &s, &stop));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => return,
                }
            }
        });
        FakeImap {
            port,
            state,
            shutdown,
        }
    }

    pub fn url(&self) -> String {
        format!("imap://127.0.0.1:{}", self.port)
    }

    /// The socket grant the plugin needs for this server.
    pub fn endpoint(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// New mail: a message added to INBOX, which an idling client hears of.
    pub fn deliver(&self, subject: &str, from: &str) {
        let mut st = self.state.lock().unwrap();
        let uid = st.next_uid;
        st.next_uid += 1;
        st.inbox.push(Message {
            uid,
            subject: subject.to_owned(),
            from: from.to_owned(),
            seen: false,
        });
    }

    /// The subjects in INBOX now.
    pub fn inbox(&self) -> Vec<String> {
        let st = self.state.lock().unwrap();
        st.inbox.iter().map(|m| m.subject.clone()).collect()
    }

    /// Every command line received so far.
    pub fn commands(&self) -> Vec<String> {
        self.state.lock().unwrap().log.clone()
    }
}

impl Drop for FakeImap {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

fn envelope(m: &Message) -> String {
    let (mailbox, host) = m.from.split_once('@').unwrap_or((&m.from, "x.com"));
    format!(
        "(\"Mon, 1 Jan 2025 00:00:00 +0000\" \"{subject}\" \
         ((NIL NIL \"{mailbox}\" \"{host}\")) ((NIL NIL \"{mailbox}\" \"{host}\")) \
         ((NIL NIL \"{mailbox}\" \"{host}\")) ((NIL NIL \"me\" \"x.com\")) \
         NIL NIL NIL \"<{uid}@x.com>\")",
        subject = m.subject,
        uid = m.uid,
    )
}

fn fetch_line(seq: usize, m: &Message) -> String {
    let flags = if m.seen { "\\Seen" } else { "" };
    format!(
        "* {seq} FETCH (UID {} ENVELOPE {} FLAGS ({flags}))\r\n",
        m.uid,
        envelope(m)
    )
}

/// `a:b`, `a:*`, `a` → the inclusive range, `*` being `max`.
fn range(spec: &str, max: u32) -> (u32, u32) {
    let num = |s: &str| if s == "*" { max } else { s.parse().unwrap_or(0) };
    match spec.split_once(':') {
        Some((a, b)) => (num(a), num(b)),
        None => (num(spec), num(spec)),
    }
}

fn serve(stream: TcpStream, state: &Mutex<State>, stop: &AtomicBool) {
    let mut w = stream.try_clone().unwrap();
    let mut r = BufReader::new(stream);
    let send = |w: &mut TcpStream, s: &str| {
        let _ = w.write_all(s.as_bytes());
    };
    send(&mut w, "* OK [CAPABILITY IMAP4rev1] fake IMAP ready\r\n");
    let mut selected = String::new();
    while !stop.load(Ordering::Relaxed) {
        let Some(line) = read_line(&mut r) else {
            return;
        };
        state.lock().unwrap().log.push(line.clone());
        let (tag, rest) = line.split_once(' ').unwrap_or((&line, ""));
        let upper = rest.to_uppercase();
        let words: Vec<&str> = rest.split_whitespace().collect();
        if upper.starts_with("LOGIN") {
            send(&mut w, &format!("{tag} OK LOGIN completed\r\n"));
        } else if upper.starts_with("AUTHENTICATE") {
            send(&mut w, "+ \r\n");
            let _ = read_line(&mut r);
            send(&mut w, &format!("{tag} OK AUTHENTICATE completed\r\n"));
        } else if upper.starts_with("CAPABILITY") {
            send(&mut w, "* CAPABILITY IMAP4rev1 UIDPLUS MOVE IDLE\r\n");
            send(&mut w, &format!("{tag} OK CAPABILITY completed\r\n"));
        } else if upper.starts_with("LIST") {
            send(&mut w, "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n");
            send(&mut w, "* LIST (\\HasNoChildren \\Trash) \"/\" \"[Gmail]/Trash\"\r\n");
            send(&mut w, &format!("{tag} OK LIST completed\r\n"));
        } else if upper.starts_with("SELECT") || upper.starts_with("EXAMINE") {
            selected = words.get(1).unwrap_or(&"").trim_matches('"').to_owned();
            let exists = if selected == "INBOX" {
                state.lock().unwrap().inbox.len()
            } else {
                0
            };
            send(&mut w, "* FLAGS (\\Seen \\Flagged \\Deleted)\r\n");
            send(&mut w, &format!("* {exists} EXISTS\r\n"));
            send(&mut w, "* OK [UIDVALIDITY 7] UIDs valid\r\n");
            send(&mut w, &format!("{tag} OK [READ-WRITE] SELECT completed\r\n"));
        } else if upper.starts_with("UID FETCH") || upper.starts_with("FETCH") {
            let by_uid = upper.starts_with("UID");
            let spec = words.get(if by_uid { 2 } else { 1 }).unwrap_or(&"");
            let st = state.lock().unwrap();
            let inbox: &[Message] = if selected == "INBOX" { &st.inbox } else { &[] };
            for (i, m) in inbox.iter().enumerate() {
                let seq = i + 1;
                let max = if by_uid {
                    inbox.last().map_or(0, |m| m.uid)
                } else {
                    inbox.len() as u32
                };
                let (lo, hi) = range(spec, max);
                let key = if by_uid { m.uid } else { seq as u32 };
                if key < lo || key > hi {
                    continue;
                }
                if upper.contains("BODY[]") {
                    let body = format!(
                        "From: {}\r\nSubject: {}\r\nMessage-ID: <{}@x.com>\r\n\r\nbody of {}\r\n",
                        m.from, m.subject, m.uid, m.subject
                    );
                    send(
                        &mut w,
                        &format!("* {seq} FETCH (UID {} BODY[] {{{}}}\r\n", m.uid, body.len()),
                    );
                    send(&mut w, &body);
                    send(&mut w, ")\r\n");
                } else {
                    send(&mut w, &fetch_line(seq, m));
                }
            }
            send(&mut w, &format!("{tag} OK FETCH completed\r\n"));
        } else if upper.starts_with("UID STORE") {
            send(&mut w, &format!("{tag} OK STORE completed\r\n"));
        } else if upper.starts_with("UID MOVE") {
            let uid: u32 = words.get(2).and_then(|u| u.parse().ok()).unwrap_or(0);
            state.lock().unwrap().inbox.retain(|m| m.uid != uid);
            send(&mut w, &format!("{tag} OK MOVE completed\r\n"));
        } else if upper.starts_with("UID EXPUNGE") || upper.starts_with("EXPUNGE") {
            send(&mut w, &format!("{tag} OK EXPUNGE completed\r\n"));
        } else if upper.starts_with("IDLE") {
            if !idle(&mut w, &mut r, state, stop, tag) {
                return;
            }
        } else if upper.starts_with("LOGOUT") {
            send(&mut w, "* BYE logging out\r\n");
            send(&mut w, &format!("{tag} OK LOGOUT completed\r\n"));
            return;
        } else if upper.starts_with("NOOP") || upper.starts_with("CLOSE") {
            send(&mut w, &format!("{tag} OK completed\r\n"));
        } else {
            send(&mut w, &format!("{tag} BAD not in this fake\r\n"));
        }
    }
}

/// Idle until the client says DONE, announcing new mail as it arrives.
fn idle(
    w: &mut TcpStream,
    r: &mut BufReader<TcpStream>,
    state: &Mutex<State>,
    stop: &AtomicBool,
    tag: &str,
) -> bool {
    let _ = w.write_all(b"+ idling\r\n");
    let mut told = state.lock().unwrap().inbox.len();
    let _ = r.get_ref().set_read_timeout(Some(Duration::from_millis(50)));
    let mut buf = Vec::new();
    let done = loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let now = state.lock().unwrap().inbox.len();
        if now > told {
            told = now;
            let _ = w.write_all(format!("* {now} EXISTS\r\n").as_bytes());
        }
        match r.read_until(b'\n', &mut buf) {
            Ok(0) => return false,
            Ok(_) => break String::from_utf8_lossy(&buf).trim_end().to_owned(),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return false,
        }
    };
    let _ = r.get_ref().set_read_timeout(None);
    state.lock().unwrap().log.push(done);
    let _ = w.write_all(format!("{tag} OK IDLE terminated\r\n").as_bytes());
    true
}

fn read_line(r: &mut BufReader<TcpStream>) -> Option<String> {
    let mut buf = Vec::new();
    match r.read_until(b'\n', &mut buf) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(String::from_utf8_lossy(&buf).trim_end().to_owned()),
    }
}
