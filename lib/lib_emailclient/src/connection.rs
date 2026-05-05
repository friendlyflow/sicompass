//! Shared IMAP connection helpers used by both `net.rs` (RealImap) and
//! `idle.rs` (IdleController background thread).

use crate::EmailClientConfig;
use native_tls::TlsConnector;

pub type ImapSession = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

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
        let default_port = if url.starts_with("imaps://") { 993 } else { 143 };
        Some((rest.to_owned(), default_port))
    }
}

// ---------------------------------------------------------------------------
// XOAUTH2 authenticator
// ---------------------------------------------------------------------------

/// IMAP Authenticator implementing the XOAUTH2 SASL mechanism.
///
/// The `process` method returns the raw SASL initial response (the imap crate
/// base64-encodes it automatically before sending).
pub struct XOAuth2Auth {
    pub user: String,
    pub token: String,
}

impl imap::Authenticator for XOAuth2Auth {
    type Response = String;
    fn process(&self, _challenge: &[u8]) -> Self::Response {
        format!("user={}\x01auth=Bearer {}\x01\x01", self.user, self.token)
    }
}

// ---------------------------------------------------------------------------
// Session factory
// ---------------------------------------------------------------------------

/// Open an authenticated IMAP session from `config`.
///
/// Uses XOAUTH2 when an access token is present, LOGIN otherwise.
pub fn connect_imap(config: &EmailClientConfig) -> Result<ImapSession, String> {
    let (host, port) = parse_imap_url(&config.imap_url)
        .ok_or_else(|| format!("cannot parse IMAP URL: {}", config.imap_url))?;

    let tls = TlsConnector::new().map_err(|e| e.to_string())?;
    let client = imap::connect((host.as_str(), port), &host, &tls)
        .map_err(|e| e.to_string())?;

    if config.oauth_access_token.is_empty() {
        client
            .login(&config.username, &config.password)
            .map_err(|(e, _)| e.to_string())
    } else {
        let auth = XOAuth2Auth {
            user: config.username.clone(),
            token: config.oauth_access_token.clone(),
        };
        client
            .authenticate("XOAUTH2", &auth)
            .map_err(|(e, _)| e.to_string())
    }
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
}
