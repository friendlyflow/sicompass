//! Google OAuth2 authorization flow — port of `oauth2.c`.
//!
//! Uses a local HTTP server on a random port to receive the redirect code,
//! then exchanges it for tokens via the Google token endpoint.

use reqwest::blocking::Client;
use serde::Deserialize;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const OAUTH2_SCOPE: &str = "https://mail.google.com/ email profile";

/// Result of an OAuth2 token operation — mirrors `OAuth2TokenResult` from C.
#[derive(Debug, Clone, Default)]
pub struct OAuth2TokenResult {
    pub success: bool,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub error: String,
}

// ---------------------------------------------------------------------------
// Non-blocking handle
// ---------------------------------------------------------------------------

/// An in-flight OAuth2 authorization request. Created by [`start`]; poll it
/// each frame with [`PendingAuthorize::poll`] until it returns `Some`.
pub struct PendingAuthorize {
    rx: std::sync::mpsc::Receiver<OAuth2TokenResult>,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
}

impl std::fmt::Debug for PendingAuthorize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingAuthorize")
            .field("cancelled", &self.cancel.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl PendingAuthorize {
    /// Non-blocking check. Returns `Some(result)` once the worker finishes
    /// (success, error, or timeout), `None` while still waiting.
    pub fn poll(&self) -> Option<OAuth2TokenResult> {
        // Check deadline before try_recv so callers don't have to track time.
        if Instant::now() >= self.deadline {
            self.cancel.store(true, Ordering::Relaxed);
            // Drain whatever the worker might have sent right at the deadline.
            if let Ok(r) = self.rx.try_recv() {
                return Some(r);
            }
            return Some(OAuth2TokenResult {
                error: "timed out waiting for Google authorization".to_owned(),
                ..Default::default()
            });
        }
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(OAuth2TokenResult {
                error: "authorization worker exited unexpectedly".to_owned(),
                ..Default::default()
            }),
        }
    }

    /// Signal the worker thread to stop and return immediately on next poll.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Start the OAuth2 authorization flow asynchronously.
///
/// Binds a local HTTP listener, opens the browser, spawns a worker thread
/// that waits for the redirect, and returns a [`PendingAuthorize`] handle
/// immediately. Call [`PendingAuthorize::poll`] each frame until it returns
/// `Some`.
pub fn start(
    client_id: &str,
    client_secret: &str,
    timeout_secs: u64,
) -> Result<PendingAuthorize, OAuth2TokenResult> {
    if client_id.is_empty() || client_secret.is_empty() {
        return Err(OAuth2TokenResult {
            error: "client ID and client secret are required".to_owned(),
            ..Default::default()
        });
    }

    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| OAuth2TokenResult {
        error: format!("failed to start local server: {e}"),
        ..Default::default()
    })?;
    listener.set_nonblocking(true).map_err(|e| OAuth2TokenResult {
        error: format!("failed to set listener non-blocking: {e}"),
        ..Default::default()
    })?;

    let port = listener.local_addr().unwrap().port();
    let redirect_uri = format!("http://localhost:{port}");

    let auth_url = format!(
        "{GOOGLE_AUTH_URL}?client_id={client_id}&redirect_uri={redir}&\
         response_type=code&scope={scope}&access_type=offline&prompt=consent",
        client_id = percent_encode(client_id),
        redir = percent_encode(&redirect_uri),
        scope = percent_encode(OAUTH2_SCOPE),
    );
    sicompass_sdk::platform::open_with_default(&auth_url);

    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel::<OAuth2TokenResult>();
    let cancel_worker = Arc::clone(&cancel);
    let client_id = client_id.to_owned();
    let client_secret = client_secret.to_owned();

    std::thread::spawn(move || {
        let result = accept_and_exchange(
            listener,
            &cancel_worker,
            &client_id,
            &client_secret,
            &redirect_uri,
        );
        let _ = tx.send(result);
    });

    Ok(PendingAuthorize {
        rx,
        cancel,
        deadline: Instant::now() + Duration::from_secs(timeout_secs),
    })
}

/// Start the OAuth2 authorization flow and block until completion or timeout.
///
/// Convenience wrapper for callers (and tests) that can afford to block.
pub fn authorize(client_id: &str, client_secret: &str, timeout_secs: u64) -> OAuth2TokenResult {
    match start(client_id, client_secret, timeout_secs) {
        Err(e) => e,
        Ok(handle) => {
            let sleep = Duration::from_millis(50);
            loop {
                if let Some(result) = handle.poll() {
                    return result;
                }
                std::thread::sleep(sleep);
            }
        }
    }
}

/// Fetch the authenticated user's email address from Google's userinfo endpoint.
/// Returns `None` on any network or parse error (caller treats it as optional).
pub fn fetch_email(access_token: &str) -> Option<String> {
    let response = Client::new()
        .get(GOOGLE_USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .ok()?;
    let json: serde_json::Value = response.json().ok()?;
    json.get("email")?.as_str().map(|s| s.to_owned())
}

/// Refresh an expired access token using a refresh token.
pub fn refresh_token(client_id: &str, client_secret: &str, refresh_tok: &str) -> OAuth2TokenResult {
    if client_id.is_empty() || client_secret.is_empty() || refresh_tok.is_empty() {
        return OAuth2TokenResult {
            error: "client ID, client secret, and refresh token are required".to_owned(),
            ..Default::default()
        };
    }

    let client = match Client::new().post(GOOGLE_TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_tok),
            ("grant_type", "refresh_token"),
        ])
        .send()
    {
        Ok(r) => r,
        Err(e) => {
            return OAuth2TokenResult {
                error: format!("token refresh failed: {e}"),
                ..Default::default()
            }
        }
    };

    parse_token_response(client)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Worker: non-blocking accept loop that checks `cancel` between attempts.
/// On a successful connection it reads the request, sends the success page,
/// and exchanges the auth code — all on this worker thread.
fn accept_and_exchange(
    listener: TcpListener,
    cancel: &AtomicBool,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
) -> OAuth2TokenResult {
    let sleep = Duration::from_millis(50);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return OAuth2TokenResult {
                error: "login cancelled".to_owned(),
                ..Default::default()
            };
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

                let mut buf = [0u8; 4096];
                let n = match stream.read(&mut buf) {
                    Ok(n) => n,
                    Err(_) => return OAuth2TokenResult {
                        error: "failed to read redirect request".to_owned(),
                        ..Default::default()
                    },
                };
                let request = match std::str::from_utf8(&buf[..n]) {
                    Ok(s) => s,
                    Err(_) => return OAuth2TokenResult {
                        error: "invalid UTF-8 in redirect request".to_owned(),
                        ..Default::default()
                    },
                };

                let first_line = request.lines().next().unwrap_or("");

                // Always send a response so the browser tab closes cleanly.
                let (status, body) = if first_line.contains("error=") || !first_line.contains("code=") {
                    (
                        "400 Bad Request",
                        "<html><body><h2>Authentication failed</h2>\
                         <p>You can close this tab and return to sicompass.</p>\
                         </body></html>",
                    )
                } else {
                    (
                        "200 OK",
                        "<html><body><h2>Authentication successful</h2>\
                         <p>You can close this tab and return to sicompass.</p>\
                         </body></html>",
                    )
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/html\r\n\r\n{body}"
                );
                let _ = stream.write_all(response.as_bytes());

                if first_line.contains("error=") {
                    return OAuth2TokenResult {
                        error: "Google returned an error response".to_owned(),
                        ..Default::default()
                    };
                }

                let code = match extract_query_param(first_line, "code") {
                    Some(c) => c,
                    None => return OAuth2TokenResult {
                        error: "no authorization code in redirect".to_owned(),
                        ..Default::default()
                    },
                };

                return exchange_code(&code, client_id, client_secret, redirect_uri);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(sleep);
            }
            Err(e) => {
                return OAuth2TokenResult {
                    error: format!("accept error: {e}"),
                    ..Default::default()
                };
            }
        }
    }
}

fn extract_query_param(get_line: &str, param: &str) -> Option<String> {
    // GET /?code=XXXX&... HTTP/1.1
    let query_start = get_line.find('?')?;
    let query_end = get_line[query_start..].find(' ').map(|i| query_start + i).unwrap_or(get_line.len());
    let query = &get_line[query_start + 1..query_end];
    for part in query.split('&') {
        if let Some(value) = part.strip_prefix(&format!("{param}=")) {
            return Some(value.to_owned());
        }
    }
    None
}

fn exchange_code(
    code: &str,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
) -> OAuth2TokenResult {
    let response = match Client::new()
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("code", code),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
    {
        Ok(r) => r,
        Err(e) => {
            return OAuth2TokenResult {
                error: format!("token exchange failed: {e}"),
                ..Default::default()
            }
        }
    };

    parse_token_response(response)
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

fn parse_token_response(response: reqwest::blocking::Response) -> OAuth2TokenResult {
    let text = match response.text() {
        Ok(t) => t,
        Err(e) => {
            return OAuth2TokenResult {
                error: format!("failed to read response: {e}"),
                ..Default::default()
            }
        }
    };

    let parsed: TokenResponse = match serde_json::from_str(&text) {
        Ok(p) => p,
        Err(e) => {
            return OAuth2TokenResult {
                error: format!("invalid JSON response: {e}"),
                ..Default::default()
            }
        }
    };

    if let Some(err) = parsed.error {
        let desc = parsed.error_description.unwrap_or_default();
        return OAuth2TokenResult {
            error: format!("{err}: {desc}"),
            ..Default::default()
        };
    }

    let access_token = parsed.access_token.unwrap_or_default();
    if access_token.is_empty() {
        return OAuth2TokenResult {
            error: "no access_token in response".to_owned(),
            ..Default::default()
        };
    }

    OAuth2TokenResult {
        success: true,
        access_token,
        refresh_token: parsed.refresh_token.unwrap_or_default(),
        expires_in: parsed.expires_in.unwrap_or(3600),
        ..Default::default()
    }
}

/// Minimal percent-encoder for URL components (spaces, slashes, colons, etc.).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
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
    fn test_authorize_empty_client_id_fails() {
        let r = authorize("", "secret", 1);
        assert!(!r.success);
        assert!(!r.error.is_empty());
    }

    #[test]
    fn test_authorize_empty_client_secret_fails() {
        let r = authorize("id", "", 1);
        assert!(!r.success);
        assert!(!r.error.is_empty());
    }

    #[test]
    fn test_refresh_empty_client_id_fails() {
        let r = refresh_token("", "secret", "refresh");
        assert!(!r.success);
        assert!(!r.error.is_empty());
    }

    #[test]
    fn test_refresh_empty_refresh_token_fails() {
        let r = refresh_token("id", "secret", "");
        assert!(!r.success);
        assert!(!r.error.is_empty());
    }

    #[test]
    fn test_percent_encode_special_chars() {
        assert_eq!(percent_encode("hello world"), "hello%20world");
        assert_eq!(percent_encode("a/b"), "a%2Fb");
        assert_eq!(percent_encode("a:b"), "a%3Ab");
    }

    #[test]
    fn test_percent_encode_unreserved_unchanged() {
        assert_eq!(percent_encode("abc-123_ABC.~"), "abc-123_ABC.~");
    }

    #[test]
    fn test_extract_query_param() {
        let line = "GET /?code=abc123&state=xyz HTTP/1.1";
        assert_eq!(extract_query_param(line, "code"), Some("abc123".to_owned()));
        assert_eq!(extract_query_param(line, "state"), Some("xyz".to_owned()));
        assert_eq!(extract_query_param(line, "missing"), None);
    }

    #[test]
    fn test_extract_query_param_no_query() {
        let line = "GET / HTTP/1.1";
        assert_eq!(extract_query_param(line, "code"), None);
    }

    #[test]
    fn test_start_empty_client_id_fails() {
        let r = start("", "secret", 5);
        assert!(r.is_err());
        assert!(!r.unwrap_err().error.is_empty());
    }

    #[test]
    fn test_pending_authorize_cancel_unblocks() {
        // start() with real credentials would open a browser — use a dummy
        // client_id/secret pair and cancel immediately.  We can't call start()
        // without the browser opening, so we test cancel on a PendingAuthorize
        // constructed manually from a channel pair.
        let (tx, rx) = std::sync::mpsc::channel::<OAuth2TokenResult>();
        let cancel = Arc::new(AtomicBool::new(false));
        let handle = PendingAuthorize {
            rx,
            cancel: Arc::clone(&cancel),
            deadline: Instant::now() + Duration::from_secs(60),
        };
        // Nothing sent yet — should be None.
        assert!(handle.poll().is_none());
        // Send a cancellation result on the channel (simulates the worker responding).
        tx.send(OAuth2TokenResult {
            error: "login cancelled".to_owned(),
            ..Default::default()
        }).unwrap();
        // Now poll should return Some.
        let result = handle.poll().unwrap();
        assert!(!result.success);
        assert!(!result.error.is_empty());
    }

    #[test]
    fn test_pending_authorize_timeout_returns_error() {
        let (_tx, rx) = std::sync::mpsc::channel::<OAuth2TokenResult>();
        let cancel = Arc::new(AtomicBool::new(false));
        let handle = PendingAuthorize {
            rx,
            cancel,
            // Already past the deadline.
            deadline: Instant::now() - Duration::from_secs(1),
        };
        let result = handle.poll().unwrap();
        assert!(!result.success);
        assert!(result.error.contains("timed out"));
    }
}
