//! Production IMAP and SMTP backends — Step 2.
//!
//! `RealImap` implements `ImapBackend` using the `imap` crate (native-tls).
//! `RealSmtp` implements `SmtpBackend` using the `lettre` crate.
//!
//! Both are instantiated lazily from `EmailClientConfig` inside `init()`.

use crate::{EmailClientConfig, EmailMessage, FolderInfo, ImapBackend, MailBody, MessageHeader, SmtpBackend};
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
    fn list_folders(&mut self) -> Result<Vec<FolderInfo>, String> {
        // Avoid borrow conflict: get error first, reset session, then unwrap.
        if let Err(e) = self.session() {
            self.reset_session();
            return Err(e);
        }
        let session = self.session.as_mut().unwrap();
        let names = session
            .list(None, Some("*"))
            .map_err(|e| e.to_string())?;
        let folders: Vec<FolderInfo> = names
            .iter()
            .filter_map(|n| {
                // Skip \Noselect folders (containers).
                if n.attributes().iter().any(|a| {
                    matches!(a, imap::types::NameAttribute::NoSelect)
                }) {
                    return None;
                }
                // Collect SPECIAL-USE and system attributes as raw strings.
                let attributes: Vec<String> = n
                    .attributes()
                    .iter()
                    .map(|a| match a {
                        imap::types::NameAttribute::NoInferiors => "\\Noinferiors".to_owned(),
                        imap::types::NameAttribute::NoSelect    => "\\Noselect".to_owned(),
                        imap::types::NameAttribute::Marked      => "\\Marked".to_owned(),
                        imap::types::NameAttribute::Unmarked    => "\\Unmarked".to_owned(),
                        imap::types::NameAttribute::Custom(s)   => s.to_string(),
                    })
                    .collect();
                Some(FolderInfo {
                    name: n.name().to_owned(),
                    attributes,
                })
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

        // Include FLAGS so we can show the unread (●) marker in the list.
        let messages = session
            .fetch(&fetch_range, "(UID ENVELOPE FLAGS)")
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
                let seen = m.flags().iter().any(|f| matches!(f, imap::types::Flag::Seen));
                let flagged = m.flags().iter().any(|f| matches!(f, imap::types::Flag::Flagged));
                Some(MessageHeader { uid, from, subject, date, seen, flagged })
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

    fn set_flags(
        &mut self,
        folder: &str,
        uid: u32,
        add: &[&str],
        remove: &[&str],
    ) -> Result<(), String> {
        let session = match self.session() {
            Ok(s) => s,
            Err(e) => { self.reset_session(); return Err(e); }
        };
        session.select(folder).map_err(|e| e.to_string())?;
        let uid_str = uid.to_string();
        if !add.is_empty() {
            let query = format!("+FLAGS ({})", add.join(" "));
            session.uid_store(&uid_str, &query).map_err(|e| e.to_string())?;
        }
        if !remove.is_empty() {
            let query = format!("-FLAGS ({})", remove.join(" "));
            session.uid_store(&uid_str, &query).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn copy_message(&mut self, folder: &str, uid: u32, dest: &str) -> Result<(), String> {
        let session = match self.session() {
            Ok(s) => s,
            Err(e) => { self.reset_session(); return Err(e); }
        };
        session.select(folder).map_err(|e| e.to_string())?;
        let uid_str = uid.to_string();
        session.uid_copy(&uid_str, dest).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn move_message(&mut self, folder: &str, uid: u32, dest: &str) -> Result<(), String> {
        let session = match self.session() {
            Ok(s) => s,
            Err(e) => { self.reset_session(); return Err(e); }
        };
        session.select(folder).map_err(|e| e.to_string())?;
        let uid_str = uid.to_string();
        // Try MOVE extension (RFC 6851) first; fall back to COPY + \Deleted + EXPUNGE.
        match session.uid_mv(&uid_str, dest) {
            Ok(_) => Ok(()),
            Err(_) => {
                // Fallback: COPY then mark \Deleted and expunge.
                session.uid_copy(&uid_str, dest).map_err(|e| e.to_string())?;
                let _ = session.uid_store(&uid_str, "+FLAGS (\\Deleted)");
                let _ = session.uid_expunge(&uid_str);
                Ok(())
            }
        }
    }

    fn expunge_uid(&mut self, folder: &str, uid: u32) -> Result<(), String> {
        let session = match self.session() {
            Ok(s) => s,
            Err(e) => { self.reset_session(); return Err(e); }
        };
        session.select(folder).map_err(|e| e.to_string())?;
        let uid_str = uid.to_string();
        session.uid_expunge(&uid_str).map_err(|e| e.to_string())?;
        Ok(())
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
    fn send(&mut self, from: &str, to: &str, subject: &str, body: &MailBody) -> Result<(), String> {
        let (host, port) = parse_smtp_url(&self.config.smtp_url)
            .ok_or_else(|| format!("cannot parse SMTP URL: {}", self.config.smtp_url))?;

        let builder = Message::builder()
            .from(from.parse().map_err(|e: lettre::address::AddressError| e.to_string())?)
            .to(to.parse().map_err(|e: lettre::address::AddressError| e.to_string())?)
            .subject(subject);

        let email = match body {
            MailBody::Text(s) => builder
                .header(ContentType::TEXT_PLAIN)
                .body(s.clone())
                .map_err(|e| e.to_string())?,
            MailBody::Ffon(elems) => {
                let json = sicompass_sdk::ffon::to_json_string(elems)
                    .map_err(|e| e.to_string())?;
                builder
                    .header(ContentType::TEXT_PLAIN)
                    .body(json)
                    .map_err(|e| e.to_string())?
            }
        };

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
    let (header_block, raw_body) = if let Some(pos) = text.find("\r\n\r\n") {
        (&text[..pos], &text[pos + 4..])
    } else if let Some(pos) = text.find("\n\n") {
        (&text[..pos], &text[pos + 2..])
    } else {
        (text.as_ref(), "")
    };

    let mut from = String::new();
    let mut to = String::new();
    let mut subject = String::new();
    let mut date = String::new();
    let mut message_id = String::new();
    let mut in_reply_to = String::new();
    let mut references = String::new();
    let mut content_type = String::new();
    let mut content_transfer_encoding = String::new();

    // Header parsing with folded-line support (RFC 2822 §2.2.3).
    let mut lines = header_block.lines().peekable();
    while let Some(line) = lines.next() {
        // Unfold continuation lines (lines starting with whitespace).
        let mut value = line.to_owned();
        while lines.peek().map_or(false, |l| l.starts_with(' ') || l.starts_with('\t')) {
            value.push(' ');
            value.push_str(lines.next().unwrap().trim());
        }
        let lc = value.to_ascii_lowercase();
        if lc.starts_with("from: ") { from = value[6..].to_owned(); }
        else if lc.starts_with("to: ") { to = value[4..].to_owned(); }
        else if lc.starts_with("subject: ") { subject = value[9..].to_owned(); }
        else if lc.starts_with("date: ") { date = value[6..].to_owned(); }
        else if lc.starts_with("message-id: ") { message_id = value[12..].to_owned(); }
        else if lc.starts_with("in-reply-to: ") { in_reply_to = value[13..].to_owned(); }
        else if lc.starts_with("references: ") { references = value[12..].to_owned(); }
        else if lc.starts_with("content-type: ") { content_type = value[14..].to_owned(); }
        else if lc.starts_with("content-transfer-encoding: ") {
            content_transfer_encoding = value[27..].trim().to_ascii_lowercase();
        }
    }

    let body = parse_body_part(raw_body, &content_type, &content_transfer_encoding);

    EmailMessage { uid, from, to, subject, date, body, message_id, in_reply_to, references }
}

/// Parse a MIME body part given its content-type and transfer-encoding headers.
fn parse_body_part(raw: &str, content_type: &str, cte: &str) -> MailBody {
    let ct_lc = content_type.to_ascii_lowercase();
    let mime = ct_lc.split(';').next().unwrap_or("").trim();

    // Handle multipart/* by extracting the best sub-part.
    if mime.starts_with("multipart/") {
        if let Some(boundary) = extract_boundary(content_type) {
            return parse_multipart(raw, &boundary);
        }
        return MailBody::Text(raw.to_owned());
    }

    let decoded = decode_transfer_encoding(raw, cte);

    match mime {
        "text/html" => {
            let elems = sicompass_sdk::ffon::html_to_ffon(&decoded, "");
            MailBody::Text(crate::flatten_ffon_to_text(&elems))
        }
        "application/json" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&decoded) {
                if sicompass_sdk::ffon::is_ffon(&v) {
                    if let Ok(elems) = serde_json::from_value(v) {
                        return MailBody::Ffon(elems);
                    }
                }
            }
            MailBody::Text(decoded)
        }
        // text/plain or unknown/empty — treat as plain text, but promote to
        // Ffon if the content is valid FFON JSON (sicompass-sent bodies).
        _ => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&decoded) {
                if sicompass_sdk::ffon::is_ffon(&v) {
                    if let Ok(elems) = serde_json::from_value(v) {
                        return MailBody::Ffon(elems);
                    }
                }
            }
            MailBody::Text(decoded)
        }
    }
}

/// Decode a transfer-encoded body string.
fn decode_transfer_encoding(raw: &str, cte: &str) -> String {
    match cte.trim() {
        "quoted-printable" => {
            quoted_printable::decode(raw.as_bytes(), quoted_printable::ParseMode::Robust)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_else(|_| raw.to_owned())
        }
        "base64" => {
            use base64::Engine as _;
            let compact: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
            base64::engine::general_purpose::STANDARD
                .decode(compact.as_bytes())
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_else(|_| raw.to_owned())
        }
        _ => raw.to_owned(),
    }
}

/// Extract the `boundary=` parameter from a Content-Type value.
fn extract_boundary(content_type: &str) -> Option<String> {
    for part in content_type.split(';').skip(1) {
        let p = part.trim();
        let lc = p.to_ascii_lowercase();
        if lc.starts_with("boundary=") {
            let val = &p[9..].trim_matches('"');
            return Some(val.to_string());
        }
    }
    None
}

/// Split a multipart body and return the best available part.
/// Preference order: FFON (application/json) > HTML > plain text.
fn parse_multipart(raw: &str, boundary: &str) -> MailBody {
    let delimiter = format!("--{boundary}");
    let mut parts: Vec<MailBody> = Vec::new();

    for chunk in raw.split(&delimiter) {
        let chunk = chunk.trim_start_matches('-').trim();
        if chunk.is_empty() { continue; }

        // Split chunk into its own headers and body.
        let (part_headers, part_body) = if let Some(pos) = chunk.find("\r\n\r\n") {
            (&chunk[..pos], &chunk[pos + 4..])
        } else if let Some(pos) = chunk.find("\n\n") {
            (&chunk[..pos], &chunk[pos + 2..])
        } else {
            continue;
        };

        let mut part_ct = String::new();
        let mut part_cte = String::new();
        for line in part_headers.lines() {
            let lc = line.to_ascii_lowercase();
            if lc.starts_with("content-type: ") { part_ct = line[14..].to_owned(); }
            else if lc.starts_with("content-transfer-encoding: ") {
                part_cte = line[27..].trim().to_ascii_lowercase();
            }
        }
        parts.push(parse_body_part(part_body, &part_ct, &part_cte));
    }

    // Pick in preference order: Ffon > Text.
    let ffon = parts.iter().find(|p| matches!(p, MailBody::Ffon(_)));
    if let Some(f) = ffon { return f.clone(); }
    parts.into_iter().find(|p| matches!(p, MailBody::Text(_)))
        .unwrap_or_else(|| MailBody::Text(String::new()))
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
        assert!(matches!(&msg.body, MailBody::Text(s) if s.contains("Hi there!")));
    }

    #[test]
    fn test_parse_rfc2822_lf_only_separator() {
        let raw = b"From: a@b.com\nSubject: Test\n\nBody text\n";
        let msg = parse_rfc2822(1, raw);
        assert_eq!(msg.subject, "Test");
        assert!(matches!(&msg.body, MailBody::Text(s) if s.contains("Body text")));
    }

    #[test]
    fn test_parse_rfc2822_no_body() {
        let raw = b"From: a@b.com\r\nSubject: Empty\r\n\r\n";
        let msg = parse_rfc2822(1, raw);
        assert_eq!(msg.subject, "Empty");
        assert!(matches!(&msg.body, MailBody::Text(s) if s.is_empty()));
    }

    #[test]
    fn test_parse_rfc2822_html_content_type() {
        let raw = b"From: a@b.com\r\nSubject: Html\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Hello</p>\r\n";
        let msg = parse_rfc2822(1, raw);
        // HTML is flattened to plain text at parse time.
        assert!(matches!(&msg.body, MailBody::Text(s) if s.contains("Hello")));
    }

    #[test]
    fn test_parse_rfc2822_multipart_alternative_html_flattened_to_text() {
        let boundary = "bound1";
        let body = format!(
            "--{boundary}\r\nContent-Type: text/plain\r\n\r\nPlain text\r\n\
             --{boundary}\r\nContent-Type: text/html\r\n\r\n<p>Rich</p>\r\n\
             --{boundary}--\r\n"
        );
        let raw = format!(
            "From: a@b.com\r\nSubject: Multi\r\nContent-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\r\n{body}"
        );
        let msg = parse_rfc2822(1, raw.as_bytes());
        // Both parts are Text after parsing; first Text match wins (plain text part).
        assert!(matches!(&msg.body, MailBody::Text(_)));
    }

    #[test]
    fn test_parse_rfc2822_application_json_ffon() {
        let ffon_json = r#"[{"Heading:":["line1","line2"]}]"#;
        let raw = format!(
            "From: a@b.com\r\nSubject: Ffon\r\nContent-Type: application/json; charset=utf-8\r\n\r\n{ffon_json}\r\n"
        );
        let msg = parse_rfc2822(1, raw.as_bytes());
        assert!(matches!(&msg.body, MailBody::Ffon(elems) if !elems.is_empty()));
    }

    #[test]
    fn test_parse_rfc2822_text_plain_ffon_promoted() {
        // sicompass sends FFON as text/plain JSON; receiver must promote it back to Ffon.
        let ffon_json = r#"[{"Heading:":["line1","line2"]}]"#;
        let raw = format!(
            "From: a@b.com\r\nSubject: Ffon\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{ffon_json}\r\n"
        );
        let msg = parse_rfc2822(1, raw.as_bytes());
        assert!(matches!(&msg.body, MailBody::Ffon(elems) if !elems.is_empty()));
    }

    #[test]
    fn test_parse_rfc2822_quoted_printable_decode() {
        // "café" in quoted-printable
        let raw = b"From: a@b.com\r\nSubject: QP\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\ncaf=C3=A9\r\n";
        let msg = parse_rfc2822(1, raw);
        assert!(matches!(&msg.body, MailBody::Text(s) if s.contains("café")));
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
        println!("folders: {:?}", folders.iter().map(|f| &f.name).collect::<Vec<_>>());

        let inbox = folders.iter().find(|f| f.name.to_uppercase() == "INBOX")
            .expect("INBOX not found");
        let headers = backend.list_messages(&inbox.name, 5).expect("list_messages failed");
        println!("inbox headers: {headers:?}");
    }
}
