//! Google OAuth2 authorization flow — port of `oauth2.c`.
//!
//! Uses a local HTTP server on a random port to receive the redirect code,
//! then exchanges it for tokens via the Google token endpoint.

use reqwest::blocking::Client;
use serde::Deserialize;
use std::io::{Read, Write};
use std::net::TcpListener;

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const OAUTH2_SCOPE: &str = "https://mail.google.com/";

/// Result of an OAuth2 token operation — mirrors `OAuth2TokenResult` from C.
#[derive(Debug, Clone, Default)]
pub struct OAuth2TokenResult {
    pub success: bool,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub error: String,
}

/// Start the OAuth2 authorization flow for Google.
///
/// Opens the browser for Google login, waits for the redirect on a local HTTP
/// server, then exchanges the authorization code for tokens.
pub fn authorize(client_id: &str, client_secret: &str, timeout_secs: u64) -> OAuth2TokenResult {
    if client_id.is_empty() || client_secret.is_empty() {
        return OAuth2TokenResult {
            error: "client ID and client secret are required".to_owned(),
            ..Default::default()
        };
    }

    // Bind to a random free port on loopback.
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => {
            return OAuth2TokenResult {
                error: format!("failed to start local server: {e}"),
                ..Default::default()
            }
        }
    };
    let port = listener.local_addr().unwrap().port();
    let redirect_uri = format!("http://localhost:{port}");

    let auth_url = format!(
        "{GOOGLE_AUTH_URL}?client_id={client_id}&redirect_uri={redir}&\
         response_type=code&scope={scope}&access_type=offline&prompt=consent",
        redir = percent_encode(&redirect_uri),
        scope = percent_encode(OAUTH2_SCOPE),
    );
    sicompass_sdk::platform::open_with_default(&auth_url);

    let code = match wait_for_auth_code(listener, timeout_secs) {
        Some(c) => c,
        None => {
            return OAuth2TokenResult {
                error: "timed out waiting for authorization".to_owned(),
                ..Default::default()
            }
        }
    };

    exchange_code(&code, client_id, client_secret, &redirect_uri)
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

fn wait_for_auth_code(listener: TcpListener, timeout_secs: u64) -> Option<String> {
    use std::sync::mpsc;
    use std::time::Duration;

    // TcpListener has no set_read_timeout, so spawn an accept thread and use
    // a channel with a timeout to bound the wait.
    let (tx, rx) = mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        let result = (|| {
            let (mut stream, _) = listener.accept().ok()?;
            stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;

            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).ok()?;
            let request = std::str::from_utf8(&buf[..n]).ok()?;

            // Send a success HTML response.
            let response = concat!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n",
                "<html><body><h2>Authentication successful</h2>",
                "<p>You can close this tab and return to sicompass.</p>",
                "</body></html>"
            );
            let _ = stream.write_all(response.as_bytes());

            // Check for error in query string.
            let first_line = request.lines().next().unwrap_or("");
            if first_line.contains("error=") {
                return None;
            }

            extract_query_param(first_line, "code")
        })();
        let _ = tx.send(result);
    });

    rx.recv_timeout(Duration::from_secs(timeout_secs)).ok().flatten()
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
}
