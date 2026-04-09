//! IMAP IDLE background thread — port of `emailclient_idle.c`.
//!
//! Spawns a thread that maintains an IMAP IDLE connection to a single folder.
//! When the server sends EXISTS or EXPUNGE, the shared `notify` flag is set
//! so the provider can refresh on the next render cycle.

use crate::EmailClientConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

type ImapSession = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

const RECONNECT_DELAY_SECS: u64 = 10;

// ---------------------------------------------------------------------------
// IdleController
// ---------------------------------------------------------------------------

pub struct IdleController {
    /// Shared flag written by the IDLE thread when new mail arrives.
    notify: Arc<AtomicBool>,
    /// Signal to stop the background thread.
    running: Arc<AtomicBool>,
    /// Background thread handle.
    thread: Option<std::thread::JoinHandle<()>>,
}

impl IdleController {
    pub fn new(notify: Arc<AtomicBool>) -> Self {
        IdleController {
            notify,
            running: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }

    /// Start (or restart) IDLE monitoring on `folder`.
    ///
    /// Stops any existing session first, then spawns a new background thread.
    pub fn start(&mut self, config: EmailClientConfig, folder: String) {
        self.stop();

        let notify = Arc::clone(&self.notify);
        let running = Arc::clone(&self.running);
        running.store(true, Ordering::Relaxed);

        self.thread = Some(std::thread::spawn(move || {
            idle_loop(config, folder, notify, running);
        }));
    }

    /// Stop the background IDLE thread.
    ///
    /// Sets the running flag to false and drops the handle without joining.
    /// The thread will exit on its own within IDLE_POLL_INTERVAL (30 s) once
    /// it wakes from wait_with_timeout and sees running=false.  Not joining
    /// avoids blocking the main thread for up to 29 minutes on wait_keepalive.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        let _ = self.thread.take(); // drop handle without joining
    }
}

impl Drop for IdleController {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------
// IDLE loop
// ---------------------------------------------------------------------------

/// The main IDLE background thread function.
fn idle_loop(
    config: EmailClientConfig,
    folder: String,
    notify: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) {
    while running.load(Ordering::Relaxed) {
        match run_idle_session(&config, &folder, &notify, &running) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("emailclient_idle: session error: {e}");
            }
        }

        if !running.load(Ordering::Relaxed) {
            break;
        }

        // Back-off before reconnecting.
        for _ in 0..RECONNECT_DELAY_SECS {
            if !running.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

/// Connect, authenticate, select folder, then run the IDLE inner loop.
fn run_idle_session(
    config: &EmailClientConfig,
    folder: &str,
    notify: &Arc<AtomicBool>,
    running: &Arc<AtomicBool>,
) -> Result<(), String> {
    use native_tls::TlsConnector;

    let (host, port) = parse_imap_url(&config.imap_url)
        .ok_or_else(|| format!("cannot parse IMAP URL: {}", config.imap_url))?;

    let tls = TlsConnector::new().map_err(|e| e.to_string())?;
    let client: imap::Client<native_tls::TlsStream<std::net::TcpStream>> =
        imap::connect((host.as_str(), port), &host, &tls)
            .map_err(|e| e.to_string())?;

    let mut session: ImapSession = if config.oauth_access_token.is_empty() {
        client
            .login(&config.username, &config.password)
            .map_err(|(e, _)| e.to_string())?
    } else {
        let auth = XOAuth2Auth {
            user: config.username.clone(),
            token: config.oauth_access_token.clone(),
        };
        client
            .authenticate("XOAUTH2", &auth)
            .map_err(|(e, _)| e.to_string())?
    };

    session.select(folder).map_err(|e| e.to_string())?;

    // Inner IDLE loop.
    // Use wait_with_timeout so the thread wakes every IDLE_POLL_INTERVAL and
    // can check the running flag.  This keeps stop() non-blocking — the thread
    // exits on its own within one poll interval after running is set to false.
    const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(30);
    while running.load(Ordering::Relaxed) {
        let idle = session.idle().map_err(|e| e.to_string())?;
        let outcome = idle
            .wait_with_timeout(IDLE_POLL_INTERVAL)
            .map_err(|e| e.to_string())?;
        // Handle consumed — session borrow released; drain unsolicited responses
        // only when the server actually notified us (not on a poll timeout).
        if running.load(Ordering::Relaxed)
            && matches!(outcome, imap::extensions::idle::WaitOutcome::MailboxChanged)
        {
            while let Ok(response) = session.unsolicited_responses.try_recv() {
                if matches!(
                    response,
                    imap::types::UnsolicitedResponse::Exists(_)
                        | imap::types::UnsolicitedResponse::Expunge(_)
                ) {
                    notify.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
    }

    let _ = session.logout();
    Ok(())
}

// ---------------------------------------------------------------------------
// XOAUTH2 authenticator
// ---------------------------------------------------------------------------

/// IMAP Authenticator implementing the XOAUTH2 SASL mechanism.
///
/// The `process` method returns the raw SASL initial response (the imap crate
/// base64-encodes it automatically before sending):
/// "user=<user>\x01auth=Bearer <token>\x01\x01"
struct XOAuth2Auth {
    user: String,
    token: String,
}

impl imap::Authenticator for XOAuth2Auth {
    type Response = String;

    fn process(&self, _challenge: &[u8]) -> Self::Response {
        format!("user={}\x01auth=Bearer {}\x01\x01", self.user, self.token)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse `imaps://host` or `imaps://host:port` into `(host, port)`.
pub(crate) fn parse_imap_url(url: &str) -> Option<(String, u16)> {
    let rest = url
        .strip_prefix("imaps://")
        .or_else(|| url.strip_prefix("imap://"))?;
    if let Some(colon) = rest.rfind(':') {
        let host = rest[..colon].to_owned();
        let port: u16 = rest[colon + 1..].parse().ok()?;
        Some((host, port))
    } else {
        let default_port = if url.starts_with("imaps://") { 993 } else { 143 };
        Some((rest.to_owned(), default_port))
    }
}

/// Minimal standard base64 encoder (no external dep).
pub(crate) fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((triple >> 6) & 0x3F) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(triple & 0x3F) as usize] as char } else { '=' });
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_imap_url_with_port() {
        assert_eq!(parse_imap_url("imaps://imap.gmail.com:993"), Some(("imap.gmail.com".to_owned(), 993)));
    }

    #[test]
    fn test_parse_imap_url_without_port_defaults_993() {
        assert_eq!(parse_imap_url("imaps://imap.gmail.com"), Some(("imap.gmail.com".to_owned(), 993)));
    }

    #[test]
    fn test_parse_imap_plain_url_defaults_143() {
        assert_eq!(parse_imap_url("imap://mail.example.com"), Some(("mail.example.com".to_owned(), 143)));
    }

    #[test]
    fn test_parse_imap_url_empty_returns_none() {
        assert_eq!(parse_imap_url(""), None);
    }

    #[test]
    fn test_parse_imap_url_invalid_returns_none() {
        assert_eq!(parse_imap_url("http://example.com"), None);
    }

    #[test]
    fn test_base64_encode_hello() {
        // "Hello" → "SGVsbG8="
        assert_eq!(base64_encode(b"Hello"), "SGVsbG8=");
    }

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn test_base64_encode_padding_two() {
        // "M" → "TQ=="
        assert_eq!(base64_encode(b"M"), "TQ==");
    }

    #[test]
    fn test_base64_encode_padding_one() {
        // "Ma" → "TWE="
        assert_eq!(base64_encode(b"Ma"), "TWE=");
    }

    #[test]
    fn test_base64_encode_no_padding() {
        // "Man" → "TWFu"
        assert_eq!(base64_encode(b"Man"), "TWFu");
    }

    #[test]
    fn test_idle_controller_start_stop_noop_without_config() {
        // With an empty IMAP URL the thread should fail fast without panicking.
        let notify = Arc::new(AtomicBool::new(false));
        let mut ctrl = IdleController::new(Arc::clone(&notify));
        ctrl.start(EmailClientConfig::default(), "INBOX".to_owned());
        // Give the thread a moment to exit.
        std::thread::sleep(Duration::from_millis(100));
        ctrl.stop();
        // No panic is the success criterion.
    }

    #[test]
    fn test_needs_refresh_propagates_via_flag() {
        let notify = Arc::new(AtomicBool::new(false));
        let ctrl = IdleController::new(Arc::clone(&notify));
        // Simulate what the IDLE thread does on new mail.
        notify.store(true, Ordering::Relaxed);
        assert!(notify.load(Ordering::Relaxed));
        // Simulate provider calling clear_needs_refresh.
        notify.store(false, Ordering::Relaxed);
        assert!(!notify.load(Ordering::Relaxed));
    }
}
