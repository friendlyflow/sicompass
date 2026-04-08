//! Production IMAP and SMTP backends — Step 2.
//!
//! `RealImap` implements `ImapBackend` using the `imap` crate (native-tls).
//! `RealSmtp` implements `SmtpBackend` using the `lettre` crate.
//!
//! Both are instantiated lazily from `EmailClientConfig` inside `init()`.

use crate::{EmailClientConfig, EmailMessage, ImapBackend, MessageHeader, SmtpBackend};
use crate::idle::parse_imap_url;

use imap_proto::types::Address;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{Message, SmtpTransport, Transport};
use native_tls::TlsConnector;

// ---------------------------------------------------------------------------
// RealImap
// ---------------------------------------------------------------------------

type ImapSession = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

pub struct RealImap {
    config: EmailClientConfig,
    session: Option<ImapSession>,
}

impl RealImap {
    pub fn from_config(config: &EmailClientConfig) -> Self {
        RealImap {
            config: config.clone(),
            session: None,
        }
    }

    /// Get (or create) a live IMAP session.
    fn session(&mut self) -> Result<&mut ImapSession, String> {
        if self.session.is_none() {
            self.session = Some(connect(&self.config)?);
        }
        Ok(self.session.as_mut().unwrap())
    }

    /// Invalidate the cached session (called after errors).
    fn reset_session(&mut self) {
        if let Some(mut s) = self.session.take() {
            let _ = s.logout();
        }
    }
}

fn connect(config: &EmailClientConfig) -> Result<ImapSession, String> {
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

impl ImapBackend for RealImap {
    fn list_folders(&mut self) -> Result<Vec<String>, String> {
        // Avoid borrow conflict: get error first, reset session, then unwrap.
        if let Err(e) = self.session() {
            self.reset_session();
            return Err(e);
        }
        let session = self.session.as_mut().unwrap();
        let names = session
            .list(None, Some("*"))
            .map_err(|e| { e.to_string() })?;
        let mut folders: Vec<String> = names
            .iter()
            .filter_map(|n| {
                // Skip \\Noselect folders (containers).
                if n.attributes().iter().any(|a| {
                    matches!(a, imap::types::NameAttribute::NoSelect)
                }) {
                    return None;
                }
                Some(n.name().to_owned())
            })
            .collect();
        Ok(folders)
    }

    fn list_messages(&mut self, folder: &str, limit: usize) -> Result<Vec<MessageHeader>, String> {
        let session = match self.session() {
            Ok(s) => s,
            Err(e) => { self.reset_session(); return Err(e); }
        };

        let mailbox = session.select(folder).map_err(|e| e.to_string())?;
        let total = mailbox.exists as usize;
        if total == 0 {
            return Ok(vec![]);
        }

        let start = if total > limit { total - limit + 1 } else { 1 };
        let fetch_range = format!("{start}:{total}");

        let messages = session
            .fetch(&fetch_range, "UID ENVELOPE")
            .map_err(|e| e.to_string())?;

        let mut headers: Vec<MessageHeader> = messages
            .iter()
            .filter_map(|m| {
                let uid = m.uid?;
                let env = m.envelope()?;
                let subject = env
                    .subject
                    .as_deref()
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or("")
                    .to_owned();
                let from = env
                    .from
                    .as_deref()
                    .and_then(|addrs| addrs.first())
                    .map(|a| format_address(a))
                    .unwrap_or_default();
                let date = env
                    .date
                    .as_deref()
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or("")
                    .to_owned();
                Some(MessageHeader { uid, from, subject, date })
            })
            .collect();

        // Most-recent-first order (mirror the C FETCH 1:N reverse).
        headers.reverse();
        Ok(headers)
    }

    fn fetch_message(&mut self, folder: &str, uid: u32) -> Result<Option<EmailMessage>, String> {
        let session = match self.session() {
            Ok(s) => s,
            Err(e) => { self.reset_session(); return Err(e); }
        };

        session.select(folder).map_err(|e| e.to_string())?;
        let uid_str = uid.to_string();
        let messages = session
            .uid_fetch(&uid_str, "BODY[]")
            .map_err(|e| e.to_string())?;

        let raw = messages
            .iter()
            .find(|m| m.uid == Some(uid))
            .and_then(|m| m.body())
            .map(|b| b.to_vec());

        match raw {
            None => Ok(None),
            Some(bytes) => Ok(Some(parse_rfc2822(uid, &bytes))),
        }
    }

    fn fetch_message_by_message_id(
        &mut self,
        folder: &str,
        message_id: &str,
    ) -> Result<Option<EmailMessage>, String> {
        let session = match self.session() {
            Ok(s) => s,
            Err(e) => { self.reset_session(); return Err(e); }
        };

        session.select(folder).map_err(|e| e.to_string())?;
        let search = format!("HEADER Message-ID {message_id}");
        let uids = session
            .uid_search(&search)
            .map_err(|e| e.to_string())?;

        let uid = match uids.iter().next() {
            Some(&u) => u,
            None => return Ok(None),
        };

        // Reuse the normal fetch path.
        self.fetch_message(folder, uid)
    }
}

// ---------------------------------------------------------------------------
// RealSmtp
// ---------------------------------------------------------------------------

pub struct RealSmtp {
    config: EmailClientConfig,
}

impl RealSmtp {
    pub fn from_config(config: &EmailClientConfig) -> Self {
        RealSmtp { config: config.clone() }
    }
}

/// Parse `smtps://host` or `smtps://host:port` → `(host, port)`.
fn parse_smtp_url(url: &str) -> Option<(String, u16)> {
    let rest = url
        .strip_prefix("smtps://")
        .or_else(|| url.strip_prefix("smtp://"))?;
    if let Some(colon) = rest.rfind(':') {
        let host = rest[..colon].to_owned();
        let port: u16 = rest[colon + 1..].parse().ok()?;
        Some((host, port))
    } else {
        let port = if url.starts_with("smtps://") { 465 } else { 587 };
        Some((rest.to_owned(), port))
    }
}

impl SmtpBackend for RealSmtp {
    fn send(&mut self, from: &str, to: &str, subject: &str, body: &str) -> Result<(), String> {
        let (host, port) = parse_smtp_url(&self.config.smtp_url)
            .ok_or_else(|| format!("cannot parse SMTP URL: {}", self.config.smtp_url))?;

        let email = Message::builder()
            .from(from.parse().map_err(|e: lettre::address::AddressError| e.to_string())?)
            .to(to.parse().map_err(|e: lettre::address::AddressError| e.to_string())?)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_owned())
            .map_err(|e| e.to_string())?;

        let transport = if self.config.oauth_access_token.is_empty() {
            let creds = Credentials::new(
                self.config.username.clone(),
                self.config.password.clone(),
            );
            SmtpTransport::relay(&host)
                .map_err(|e| e.to_string())?
                .port(port)
                .credentials(creds)
                .build()
        } else {
            // XOAUTH2: lettre accepts the raw access token as password with Xoauth2 mechanism.
            let creds = Credentials::new(
                self.config.username.clone(),
                self.config.oauth_access_token.clone(),
            );
            SmtpTransport::relay(&host)
                .map_err(|e| e.to_string())?
                .port(port)
                .credentials(creds)
                .authentication(vec![Mechanism::Xoauth2])
                .build()
        };

        transport.send(&email).map(|_| ()).map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// XOAUTH2 IMAP authenticator
// ---------------------------------------------------------------------------

struct XOAuth2Auth {
    user: String,
    token: String,
}

impl imap::Authenticator for XOAuth2Auth {
    type Response = String;
    fn process(&self, _challenge: &[u8]) -> Self::Response {
        // imap crate base64-encodes the response automatically.
        format!("user={}\x01auth=Bearer {}\x01\x01", self.user, self.token)
    }
}

// ---------------------------------------------------------------------------
// RFC 2822 raw-message parser
// ---------------------------------------------------------------------------

/// Parse a raw RFC 2822 email (BODY[] response) into an `EmailMessage`.
fn parse_rfc2822(uid: u32, raw: &[u8]) -> EmailMessage {
    let text = String::from_utf8_lossy(raw);

    // Split headers from body at the first blank line.
    let (header_block, body) = if let Some(pos) = text.find("\r\n\r\n") {
        (&text[..pos], text[pos + 4..].to_string())
    } else if let Some(pos) = text.find("\n\n") {
        (&text[..pos], text[pos + 2..].to_string())
    } else {
        (text.as_ref(), String::new())
    };

    let mut from = String::new();
    let mut to = String::new();
    let mut subject = String::new();
    let mut date = String::new();
    let mut message_id = String::new();
    let mut in_reply_to = String::new();
    let mut references = String::new();

    for line in header_block.lines() {
        if let Some(v) = line.strip_prefix("From: ").or_else(|| line.strip_prefix("from: ")) {
            from = v.to_owned();
        } else if let Some(v) = line.strip_prefix("To: ").or_else(|| line.strip_prefix("to: ")) {
            to = v.to_owned();
        } else if let Some(v) = line.strip_prefix("Subject: ").or_else(|| line.strip_prefix("subject: ")) {
            subject = v.to_owned();
        } else if let Some(v) = line.strip_prefix("Date: ").or_else(|| line.strip_prefix("date: ")) {
            date = v.to_owned();
        } else if let Some(v) = line.strip_prefix("Message-ID: ").or_else(|| line.strip_prefix("Message-Id: ")) {
            message_id = v.to_owned();
        } else if let Some(v) = line.strip_prefix("In-Reply-To: ").or_else(|| line.strip_prefix("in-reply-to: ")) {
            in_reply_to = v.to_owned();
        } else if let Some(v) = line.strip_prefix("References: ").or_else(|| line.strip_prefix("references: ")) {
            references = v.to_owned();
        }
    }

    EmailMessage {
        uid,
        from,
        to,
        subject,
        date,
        body,
        message_id,
        in_reply_to,
        references,
    }
}

/// Format an IMAP address struct as "Name <mailbox@host>" or "mailbox@host".
fn format_address(addr: &Address<'_>) -> String {
    let name = addr
        .name
        .as_deref()
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("")
        .to_owned();
    let mailbox = addr
        .mailbox
        .as_deref()
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("");
    let host = addr
        .host
        .as_deref()
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("");

    if !name.is_empty() && !mailbox.is_empty() && !host.is_empty() {
        format!("{name} <{mailbox}@{host}>")
    } else if !mailbox.is_empty() && !host.is_empty() {
        format!("{mailbox}@{host}")
    } else {
        name
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_smtp_url_with_port() {
        assert_eq!(
            parse_smtp_url("smtps://smtp.gmail.com:465"),
            Some(("smtp.gmail.com".to_owned(), 465))
        );
    }

    #[test]
    fn test_parse_smtp_url_without_port_defaults_465() {
        assert_eq!(
            parse_smtp_url("smtps://smtp.gmail.com"),
            Some(("smtp.gmail.com".to_owned(), 465))
        );
    }

    #[test]
    fn test_parse_smtp_url_starttls_defaults_587() {
        assert_eq!(
            parse_smtp_url("smtp://smtp.example.com"),
            Some(("smtp.example.com".to_owned(), 587))
        );
    }

    #[test]
    fn test_parse_smtp_url_invalid_returns_none() {
        assert_eq!(parse_smtp_url(""), None);
        assert_eq!(parse_smtp_url("http://example.com"), None);
    }

    #[test]
    fn test_parse_rfc2822_extracts_fields() {
        let raw = b"From: Alice <alice@example.com>\r\n\
                    To: Bob <bob@example.com>\r\n\
                    Subject: Hello\r\n\
                    Date: Mon, 1 Jan 2025 00:00:00 +0000\r\n\
                    Message-ID: <abc@example.com>\r\n\
                    References: <prev@example.com>\r\n\
                    \r\n\
                    Hi there!\r\n";
        let msg = parse_rfc2822(42, raw);
        assert_eq!(msg.uid, 42);
        assert_eq!(msg.from, "Alice <alice@example.com>");
        assert_eq!(msg.to, "Bob <bob@example.com>");
        assert_eq!(msg.subject, "Hello");
        assert_eq!(msg.message_id, "<abc@example.com>");
        assert_eq!(msg.references, "<prev@example.com>");
        assert!(msg.body.contains("Hi there!"));
    }

    #[test]
    fn test_parse_rfc2822_lf_only_separator() {
        let raw = b"From: a@b.com\nSubject: Test\n\nBody text\n";
        let msg = parse_rfc2822(1, raw);
        assert_eq!(msg.subject, "Test");
        assert!(msg.body.contains("Body text"));
    }

    #[test]
    fn test_parse_rfc2822_no_body() {
        let raw = b"From: a@b.com\r\nSubject: Empty\r\n\r\n";
        let msg = parse_rfc2822(1, raw);
        assert_eq!(msg.subject, "Empty");
        assert!(msg.body.is_empty());
    }

    #[test]
    fn test_format_address_with_name() {
        let addr = Address {
            name: Some(b"Alice"),
            adl: None,
            mailbox: Some(b"alice"),
            host: Some(b"example.com"),
        };
        assert_eq!(format_address(&addr), "Alice <alice@example.com>");
    }

    #[test]
    fn test_format_address_without_name() {
        let addr = Address {
            name: None,
            adl: None,
            mailbox: Some(b"bob"),
            host: Some(b"example.com"),
        };
        assert_eq!(format_address(&addr), "bob@example.com");
    }

    /// Live integration test — skipped unless SICOMPASS_TEST_IMAP_URL is set.
    #[test]
    #[ignore]
    fn real_imap_smoke() {
        let imap_url = std::env::var("SICOMPASS_TEST_IMAP_URL").unwrap();
        let username = std::env::var("SICOMPASS_TEST_USERNAME").unwrap();
        let password = std::env::var("SICOMPASS_TEST_PASSWORD").unwrap();
        let mut config = EmailClientConfig::default();
        config.imap_url = imap_url;
        config.username = username;
        config.password = password;

        let mut backend = RealImap::from_config(&config);
        let folders = backend.list_folders().expect("list_folders failed");
        assert!(!folders.is_empty(), "expected at least one folder");
        println!("folders: {folders:?}");

        let inbox = folders.iter().find(|f| f.to_uppercase() == "INBOX")
            .expect("INBOX not found");
        let headers = backend.list_messages(inbox, 5).expect("list_messages failed");
        println!("inbox headers: {headers:?}");
    }
}
