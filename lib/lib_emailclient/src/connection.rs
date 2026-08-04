//! Shared IMAP connection helpers used by both `net.rs` (RealImap) and
//! `idle.rs` (IdleController background task).
//!
//! Everything here is async on top of `async-imap` and a process-wide tokio
//! runtime. The synchronous `Provider` trait is bridged at the `ImapBackend`
//! call sites via [`block_on`], not here.

use crate::EmailClientConfig;
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

/// The process-wide runtime that carries all email I/O.
///
/// Multi-threaded on purpose: the folder-list and INBOX-prefetch tasks run
/// concurrently on it, and [`block_on`] relies on `block_in_place`, which a
/// current-thread runtime does not support. Mirrors `chromium_runtime()` in
/// `lib_webbrowser`.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("sicompass-email")
            .build()
            .expect("failed to build the email runtime")
    })
}

/// Drive `fut` to completion from synchronous code.
///
/// `Runtime::block_on` panics when called from inside a runtime context, which
/// the tasks spawned in `lib.rs` do create (the same hazard is documented in
/// `lib_updater/src/github.rs` and `plugin.rs`). Detect that case and hand the
/// work to `block_in_place` instead of panicking.
pub fn block_on<F: Future>(fut: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => runtime().block_on(fut),
    }
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

/// Transport carrying an IMAP session.
///
/// Production traffic is always `Tls`. `Plain` exists so the test suite can
/// drive `RealImap` against a local fake IMAP server; [`open_stream`] refuses
/// it for anything that is not a loopback address, so credentials can never
/// leave the machine in the clear.
#[derive(Debug)]
pub enum ImapStream {
    Tls(Box<async_native_tls::TlsStream<TcpStream>>),
    Plain(TcpStream),
}

impl AsyncRead for ImapStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ImapStream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
            ImapStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ImapStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            ImapStream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
            ImapStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ImapStream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
            ImapStream::Plain(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ImapStream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
            ImapStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

pub type ImapSession = async_imap::Session<ImapStream>;

// ---------------------------------------------------------------------------
// URL parser
// ---------------------------------------------------------------------------

/// Parse `imaps://host` or `imaps://host:port` into `(host, port)`.
pub fn parse_imap_url(url: &str) -> Option<(String, u16)> {
    let rest = url
        .strip_prefix("imaps://")
        .or_else(|| url.strip_prefix("imap://"))?;
    if let Some(colon) = rest.rfind(':') {
        let host = rest[..colon].to_owned();
        let port: u16 = rest[colon + 1..].parse().ok()?;
        Some((host, port))
    } else {
        let default_port = if url.starts_with("imaps://") {
            993
        } else {
            143
        };
        Some((rest.to_owned(), default_port))
    }
}

// ---------------------------------------------------------------------------
// XOAUTH2 authenticator
// ---------------------------------------------------------------------------

/// IMAP Authenticator implementing the XOAUTH2 SASL mechanism.
///
/// The `process` method returns the raw SASL initial response (async-imap
/// base64-encodes it automatically before sending).
pub struct XOAuth2Auth {
    pub user: String,
    pub token: String,
}

impl async_imap::Authenticator for XOAuth2Auth {
    type Response = String;
    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        xoauth2_payload(&self.user, &self.token)
    }
}

/// The raw XOAUTH2 SASL initial response.
///
/// Kept in one place because two callers send it: the `Authenticator` above
/// (which base64-encodes it for us) and [`RawImap`] (which encodes it itself).
/// Servers reject any deviation, so the two must not drift apart.
pub fn xoauth2_payload(user: &str, token: &str) -> String {
    format!("user={user}\x01auth=Bearer {token}\x01\x01")
}

// ---------------------------------------------------------------------------
// Session factory
// ---------------------------------------------------------------------------

/// Open the transport for `url`.
///
/// `imaps://` performs a TLS handshake. `imap://` stays in the clear and is
/// therefore only permitted when the resolved address is loopback — the fake
/// IMAP server in the test suite is the only intended user.
pub async fn open_stream(url: &str, host: &str, port: u16) -> Result<ImapStream, String> {
    let addr = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| e.to_string())?
        .next()
        .ok_or_else(|| format!("cannot resolve {host}:{port}"))?;

    let use_tls = url.starts_with("imaps://");

    // Decide before opening the socket, so a misconfigured plaintext URL fails
    // fast instead of hanging on a connect to a remote host.
    if !use_tls && !addr.ip().is_loopback() {
        return Err(format!(
            "refusing to send IMAP credentials in the clear to {host}; use imaps://"
        ));
    }

    let tcp = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;

    if use_tls {
        let stream = async_native_tls::TlsConnector::new()
            .connect(host, tcp)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(ImapStream::Tls(Box::new(stream)));
    }

    Ok(ImapStream::Plain(tcp))
}

/// Open an authenticated IMAP session from `config`.
///
/// Uses XOAUTH2 when an access token is present, LOGIN otherwise.
pub async fn connect_imap(config: &EmailClientConfig) -> Result<ImapSession, String> {
    let (host, port) = parse_imap_url(&config.imap_url)
        .ok_or_else(|| format!("cannot parse IMAP URL: {}", config.imap_url))?;

    let stream = open_stream(&config.imap_url, &host, port).await?;

    // async-imap does not consume the server greeting; unlike the old blocking
    // `imap::connect` we have to read it ourselves before issuing any command.
    let mut client = async_imap::Client::new(stream);
    client
        .read_response()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "server closed the connection before greeting".to_owned())?;

    if config.oauth_access_token.is_empty() {
        client
            .login(&config.username, &config.password)
            .await
            .map_err(|(e, _)| e.to_string())
    } else {
        let auth = XOAuth2Auth {
            user: config.username.clone(),
            token: config.oauth_access_token.clone(),
        };
        client
            .authenticate("XOAUTH2", auth)
            .await
            .map_err(|(e, _)| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// RawImap — hand-rolled client for commands async-imap cannot decode
// ---------------------------------------------------------------------------

/// A minimal IMAP client that returns server responses verbatim.
///
/// This exists for exactly one reason: every response line goes through
/// `imap_proto::parse_response`, and **no released imap-proto understands the
/// THREAD extension** (verified against 0.10.2, which the old `imap` 2.x crate
/// used, and 0.16.7, which async-imap uses). `UID THREAD` therefore fails with
/// a parse error before its payload can be read, no matter which wire crate is
/// underneath, and the crates' raw readers are private.
///
/// It speaks only what THREAD needs: authenticate, `CAPABILITY`, `SELECT`,
/// `UID THREAD`. Responses are read line by line, which is safe because none of
/// those commands can return a literal (`{n}`) — do not extend this to `FETCH`,
/// which can.
///
/// Running THREAD on its own connection has a second benefit: `fetch_threads`
/// no longer issues a `SELECT` on the main session, so it cannot disturb the
/// mailbox that session has selected.
pub struct RawImap {
    io: tokio::io::BufReader<ImapStream>,
    tag: u32,
    /// Capabilities from the post-authentication `CAPABILITY`, fetched once.
    caps: Option<Vec<String>>,
}

impl RawImap {
    /// Connect and authenticate, using XOAUTH2 when a token is present and
    /// LOGIN otherwise — the same choice `connect_imap` makes.
    pub async fn connect(config: &EmailClientConfig) -> Result<Self, String> {
        let (host, port) = parse_imap_url(&config.imap_url)
            .ok_or_else(|| format!("cannot parse IMAP URL: {}", config.imap_url))?;
        let stream = open_stream(&config.imap_url, &host, port).await?;

        let mut raw = RawImap {
            io: tokio::io::BufReader::new(stream),
            tag: 0,
            caps: None,
        };

        let greeting = raw.read_line().await?;
        if !greeting.starts_with("* OK") {
            return Err(format!("unexpected IMAP greeting: {greeting}"));
        }

        if config.oauth_access_token.is_empty() {
            raw.login(&config.username, &config.password).await?;
        } else {
            raw.authenticate_xoauth2(&config.username, &config.oauth_access_token)
                .await?;
        }
        Ok(raw)
    }

    /// Cached `CAPABILITY` keywords, upper-cased.
    pub async fn capabilities(&mut self) -> Result<&[String], String> {
        if self.caps.is_none() {
            let lines = self.command("CAPABILITY").await?;
            let caps = lines
                .iter()
                .find_map(|l| l.strip_prefix("* CAPABILITY "))
                .map(|l| l.split_whitespace().map(|c| c.to_uppercase()).collect())
                .unwrap_or_default();
            self.caps = Some(caps);
        }
        Ok(self.caps.as_deref().unwrap_or(&[]))
    }

    /// `SELECT` then `UID THREAD <algo> UTF-8 ALL`, returning the raw response
    /// lines for [`crate::net::parse_thread_response`].
    pub async fn uid_thread(&mut self, folder: &str, algo: &str) -> Result<String, String> {
        self.command(&format!("SELECT {}", quote(folder))).await?;
        let lines = self
            .command(&format!("UID THREAD {algo} UTF-8 ALL"))
            .await?;
        Ok(lines.join("\r\n"))
    }

    async fn login(&mut self, user: &str, password: &str) -> Result<(), String> {
        self.command(&format!("LOGIN {} {}", quote(user), quote(password)))
            .await?;
        Ok(())
    }

    async fn authenticate_xoauth2(&mut self, user: &str, token: &str) -> Result<(), String> {
        let tag = self.next_tag();
        self.write(&format!("{tag} AUTHENTICATE XOAUTH2\r\n"))
            .await?;

        let cont = self.read_line().await?;
        if !cont.starts_with('+') {
            return Err(format!("server refused XOAUTH2: {cont}"));
        }

        let payload = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            xoauth2_payload(user, token),
        );
        self.write(&format!("{payload}\r\n")).await?;
        self.read_tagged(&tag).await?;
        Ok(())
    }

    /// Run one command and return its untagged (`*`) response lines.
    async fn command(&mut self, cmd: &str) -> Result<Vec<String>, String> {
        let tag = self.next_tag();
        self.write(&format!("{tag} {cmd}\r\n")).await?;
        self.read_tagged(&tag).await
    }

    /// Collect untagged lines until the tagged completion for `tag`.
    async fn read_tagged(&mut self, tag: &str) -> Result<Vec<String>, String> {
        let mut untagged = Vec::new();
        loop {
            let line = self.read_line().await?;
            match line.strip_prefix(&format!("{tag} ")) {
                Some(status) if status.starts_with("OK") => return Ok(untagged),
                Some(status) => return Err(status.to_owned()),
                None => untagged.push(line),
            }
        }
    }

    fn next_tag(&mut self) -> String {
        self.tag += 1;
        // A distinct prefix from async-imap's, so a stray response is obvious
        // in a packet capture.
        format!("t{}", self.tag)
    }

    async fn write(&mut self, s: &str) -> Result<(), String> {
        let stream = self.io.get_mut();
        stream
            .write_all(s.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stream.flush().await.map_err(|e| e.to_string())
    }

    async fn read_line(&mut self) -> Result<String, String> {
        let mut buf = Vec::new();
        match self.io.read_until(b'\n', &mut buf).await {
            Ok(0) => Err("connection closed by server".to_owned()),
            Ok(_) => {
                while matches!(buf.last(), Some(b'\r') | Some(b'\n')) {
                    buf.pop();
                }
                Ok(String::from_utf8_lossy(&buf).into_owned())
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Quote an IMAP astring, escaping `\` and `"`.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_imap::Authenticator;

    #[test]
    fn test_parse_imap_url_with_port() {
        assert_eq!(
            parse_imap_url("imaps://imap.gmail.com:993"),
            Some(("imap.gmail.com".to_owned(), 993))
        );
    }

    #[test]
    fn test_parse_imap_url_without_port_defaults_993() {
        assert_eq!(
            parse_imap_url("imaps://imap.gmail.com"),
            Some(("imap.gmail.com".to_owned(), 993))
        );
    }

    #[test]
    fn test_parse_imap_url_plain_defaults_143() {
        assert_eq!(
            parse_imap_url("imap://mail.example.com"),
            Some(("mail.example.com".to_owned(), 143))
        );
    }

    #[test]
    fn test_parse_imap_url_invalid_returns_none() {
        assert_eq!(parse_imap_url("http://example.com"), None);
    }

    #[test]
    fn test_xoauth2_process_builds_sasl_payload() {
        let mut auth = XOAuth2Auth {
            user: "user@example.com".to_owned(),
            token: "tok123".to_owned(),
        };
        assert_eq!(
            auth.process(b""),
            "user=user@example.com\x01auth=Bearer tok123\x01\x01"
        );
    }

    /// Plaintext IMAP must never be attempted against a remote host, or the
    /// login credentials would go out in the clear. The refusal happens before
    /// any socket is opened, so this test never touches the network.
    #[tokio::test]
    async fn test_plaintext_to_non_loopback_is_refused() {
        // 198.51.100.0/24 is TEST-NET-2 (RFC 5737): reserved for documentation
        // and never routable, so `lookup_host` resolves it locally.
        let err = open_stream("imap://198.51.100.7", "198.51.100.7", 143)
            .await
            .expect_err("plaintext to a remote host must be refused");
        assert!(
            err.contains("in the clear"),
            "expected a plaintext refusal, got: {err}"
        );
    }

    /// The loopback carve-out must not weaken `imaps://` — TLS is used for
    /// loopback too, so a local fake server cannot downgrade a secure config.
    #[tokio::test]
    async fn test_imaps_to_loopback_still_attempts_tls() {
        // Nothing is listening, so this fails at connect; the point is that it
        // never reports the plaintext refusal, i.e. it took the TLS branch.
        let err = open_stream("imaps://127.0.0.1", "127.0.0.1", 1)
            .await
            .expect_err("nothing listens on port 1");
        assert!(
            !err.contains("in the clear"),
            "imaps:// must not take the plaintext branch, got: {err}"
        );
    }
}
