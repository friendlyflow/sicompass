//! Production IMAP and SMTP backends — Step 2.
//!
//! `RealImap` implements `ImapBackend` using the `imap` crate (native-tls).
//! `RealSmtp` implements `SmtpBackend` using the `lettre` crate.
//!
//! Both are instantiated lazily from `EmailClientConfig` inside `init()`.

use crate::{EmailAttachment, EmailClientConfig, EmailMessage, FolderInfo, ImapBackend, MailBody, MessageHeader, SmtpBackend};
use crate::cache::EnvelopeCache;
use crate::connection::connect_imap;
use lettre::message::{Attachment as LettreAttachment, MultiPart, SinglePart};

use imap_proto::types::Address;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{Message, SmtpTransport, Transport};

// ---------------------------------------------------------------------------
// RealImap
// ---------------------------------------------------------------------------

use crate::connection::ImapSession;

pub struct RealImap {
    config: EmailClientConfig,
    session: Option<ImapSession>,
    cache: Option<EnvelopeCache>,
}

impl RealImap {
    pub fn from_config(config: &EmailClientConfig) -> Self {
        let cache = if config.username.is_empty() {
            None
        } else {
            EnvelopeCache::open(&config.username)
        };
        RealImap {
            config: config.clone(),
            session: None,
            cache,
        }
    }

    /// Get (or create) a live IMAP session.
    fn session(&mut self) -> Result<&mut ImapSession, String> {
        if self.session.is_none() {
            self.session = Some(connect_imap(&self.config)?);
        }
        Ok(self.session.as_mut().expect("session set above"))
    }

    /// Invalidate the cached session (called after errors).
    fn reset_session(&mut self) {
        if let Some(mut s) = self.session.take() {
            let _ = s.logout();
        }
    }

    /// Inner implementation of `list_messages` that accepts the envelope cache
    /// as a separate parameter, allowing the caller to satisfy the borrow
    /// checker by taking the cache out of `self` first.
    fn list_messages_inner(
        &mut self,
        folder: &str,
        limit: usize,
        cache: &mut Option<EnvelopeCache>,
    ) -> Result<Vec<MessageHeader>, String> {
        let session = match self.session() {
            Ok(s) => s,
            Err(e) => { self.reset_session(); return Err(e); }
        };

        let mailbox = session.select(folder).map_err(|e| e.to_string())?;
        let total = mailbox.exists as usize;
        let uid_validity = mailbox.uid_validity.unwrap_or(0);

        if total == 0 {
            if let Some(ref c) = cache {
                c.invalidate_folder(folder, uid_validity);
            }
            return Ok(vec![]);
        }

        // --- Cache logic ---
        if let Some(ref c) = cache {
            let cached_validity = c.get_uidvalidity(folder);
            if cached_validity == Some(uid_validity) {
                let cached_count = c.cached_count(folder);
                if cached_count >= total {
                    // Cache is complete — serve without an IMAP fetch.
                    return Ok(c.get_latest(folder, limit));
                }
                // New messages arrived: fetch only UIDs beyond the highest cached UID.
                if let Some(max_uid) = c.max_uid(folder) {
                    let new_uid_range = format!("{}:*", max_uid + 1);
                    let new_messages = session
                        .uid_fetch(&new_uid_range, "(UID ENVELOPE FLAGS)")
                        .map_err(|e| e.to_string())?;
                    let new_headers: Vec<MessageHeader> = new_messages
                        .iter()
                        .filter_map(parse_fetch_to_header)
                        .collect();
                    if !new_headers.is_empty() {
                        c.upsert_all(folder, &new_headers);
                    }
                    return Ok(c.get_latest(folder, limit));
                }
            }
            // UIDVALIDITY mismatch or first visit: flush and refetch.
            c.invalidate_folder(folder, uid_validity);
        }

        // Full IMAP fetch (cache miss or no cache).
        let start = if total > limit { total - limit + 1 } else { 1 };
        let fetch_range = format!("{start}:{total}");
        let messages = session
            .fetch(&fetch_range, "(UID ENVELOPE FLAGS)")
            .map_err(|e| e.to_string())?;

        let mut headers: Vec<MessageHeader> = messages
            .iter()
            .filter_map(parse_fetch_to_header)
            .collect();

        headers.reverse(); // Most-recent-first.

        if let Some(ref c) = cache {
            c.upsert_all(folder, &headers);
        }

        Ok(headers)
    }
}


impl ImapBackend for RealImap {
    fn list_folders(&mut self) -> Result<Vec<FolderInfo>, String> {
        // Avoid borrow conflict: get error first, reset session, then unwrap.
        if let Err(e) = self.session() {
            self.reset_session();
            return Err(e);
        }
        let session = self.session.as_mut().expect("session() succeeded above");
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
        // Take the cache out of self so we can hold a session borrow at the
        // same time (the borrow checker can't prove they're disjoint fields).
        let mut cache = self.cache.take();
        let result = self.list_messages_inner(folder, limit, &mut cache);
        self.cache = cache;
        result
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
        // Keep the envelope cache in sync.
        if let Some(ref cache) = self.cache {
            let new_seen = if add.contains(&"\\Seen") {
                Some(true)
            } else if remove.contains(&"\\Seen") {
                Some(false)
            } else {
                None
            };
            let new_flagged = if add.contains(&"\\Flagged") {
                Some(true)
            } else if remove.contains(&"\\Flagged") {
                Some(false)
            } else {
                None
            };
            cache.patch_flags(folder, uid, new_seen, new_flagged);
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
                session.uid_store(&uid_str, "+FLAGS (\\Deleted)").map_err(|e| e.to_string())?;
                session.uid_expunge(&uid_str).map_err(|e| e.to_string())?;
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

    fn append(&mut self, folder: &str, message: &[u8]) -> Result<(), String> {
        let session = match self.session() {
            Ok(s) => s,
            Err(e) => { self.reset_session(); return Err(e); }
        };
        session.append(folder, message).map(|_| ()).map_err(|e| e.to_string())
    }

    fn fetch_threads(&mut self, folder: &str) -> Result<Option<Vec<Vec<u32>>>, String> {
        let session = match self.session() {
            Ok(s) => s,
            Err(e) => { self.reset_session(); return Err(e); }
        };

        // Check capability — returns None (unsupported) when THREAD=REFERENCES
        // is absent so the caller falls back to the per-message-ID SEARCH path.
        let caps = session.capabilities().map_err(|e| e.to_string())?;
        if !caps.has_str("THREAD=REFERENCES") && !caps.has_str("THREAD=ORDEREDSUBJECT") {
            return Ok(None);
        }

        let algo = if caps.has_str("THREAD=REFERENCES") { "REFERENCES" } else { "ORDEREDSUBJECT" };
        session.select(folder).map_err(|e| e.to_string())?;

        let raw = session
            .run_command_and_read_response(&format!("UID THREAD {algo} UTF-8 ALL"))
            .map_err(|e| e.to_string())?;
        let response = String::from_utf8_lossy(&raw);
        Ok(Some(parse_thread_response(&response)))
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
    fn send(
        &mut self,
        from: &str,
        to: &[&str],
        cc: &[&str],
        bcc: &[&str],
        subject: &str,
        body: &MailBody,
        attachments: &[(&str, &[u8])],
    ) -> Result<Vec<u8>, String> {
        let (host, port) = parse_smtp_url(&self.config.smtp_url)
            .ok_or_else(|| format!("cannot parse SMTP URL: {}", self.config.smtp_url))?;

        if to.is_empty() {
            return Err("no recipients".to_owned());
        }
        let mut builder = Message::builder()
            .from(from.parse().map_err(|e: lettre::address::AddressError| e.to_string())?);
        for addr in to {
            builder = builder.to(addr.parse().map_err(|e: lettre::address::AddressError| e.to_string())?);
        }
        for addr in cc {
            builder = builder.cc(addr.parse().map_err(|e: lettre::address::AddressError| e.to_string())?);
        }
        for addr in bcc {
            builder = builder.bcc(addr.parse().map_err(|e: lettre::address::AddressError| e.to_string())?);
        }
        let builder = builder.subject(subject);

        let body_str = match body {
            MailBody::Text(s) => s.clone(),
            MailBody::Ffon(elems) => sicompass_sdk::ffon::to_json_string(elems)
                .map_err(|e| e.to_string())?,
        };

        let email = if attachments.is_empty() {
            builder
                .header(ContentType::TEXT_PLAIN)
                .body(body_str)
                .map_err(|e| e.to_string())?
        } else {
            let body_part = SinglePart::builder()
                .header(ContentType::TEXT_PLAIN)
                .body(body_str);
            let mut mp = MultiPart::mixed().singlepart(body_part);
            for (filename, bytes) in attachments {
                let ct = "application/octet-stream"
                    .parse::<ContentType>()
                    .map_err(|e| e.to_string())?;
                mp = mp.singlepart(
                    LettreAttachment::new(filename.to_string())
                        .body(bytes.to_vec(), ct),
                );
            }
            builder.multipart(mp).map_err(|e| e.to_string())?
        };

        let raw = email.formatted();

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

        transport.send(&email).map_err(|e| e.to_string())?;
        Ok(raw)
    }
}

// ---------------------------------------------------------------------------
// XOAUTH2 IMAP authenticator
// ---------------------------------------------------------------------------

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
            if let Some(cont) = lines.next() {
                value.push(' ');
                value.push_str(cont.trim());
            }
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
    let attachments = parse_attachments(raw_body, &content_type);

    EmailMessage { uid, from, to, subject, date, body, message_id, in_reply_to, references, attachments }
}

/// Walk a MIME body looking for attachment parts (Content-Disposition: attachment
/// or non-text, non-multipart parts in multipart/mixed).
fn parse_attachments(raw_body: &str, content_type: &str) -> Vec<EmailAttachment> {
    let ct_lc = content_type.to_ascii_lowercase();
    let mime = ct_lc.split(';').next().unwrap_or("").trim();
    if !mime.starts_with("multipart/") {
        return vec![];
    }
    let boundary = match extract_boundary(content_type) {
        Some(b) => b,
        None => return vec![],
    };
    let delimiter = format!("--{boundary}");
    let mut attachments = Vec::new();

    for chunk in raw_body.split(&delimiter) {
        let chunk = chunk.trim_start_matches('-').trim();
        if chunk.is_empty() { continue; }

        let (part_headers, part_body) = if let Some(pos) = chunk.find("\r\n\r\n") {
            (&chunk[..pos], &chunk[pos + 4..])
        } else if let Some(pos) = chunk.find("\n\n") {
            (&chunk[..pos], &chunk[pos + 2..])
        } else {
            continue;
        };

        let mut part_ct = String::new();
        let mut part_cte = String::new();
        let mut disposition = String::new();
        let mut filename = String::new();

        for line in part_headers.lines() {
            let lc = line.to_ascii_lowercase();
            if lc.starts_with("content-type: ") {
                part_ct = line[14..].to_owned();
            } else if lc.starts_with("content-transfer-encoding: ") {
                part_cte = line[27..].trim().to_ascii_lowercase();
            } else if lc.starts_with("content-disposition: ") {
                disposition = lc[21..].to_owned();
                // Extract filename= from the same header line.
                for param in line[21..].split(';') {
                    let p = param.trim();
                    let pl = p.to_ascii_lowercase();
                    if pl.starts_with("filename=") || pl.starts_with("filename*=") {
                        filename = p.splitn(2, '=').nth(1)
                            .unwrap_or("")
                            .trim_matches('"')
                            .to_owned();
                    }
                }
            }
        }

        let is_attachment = disposition.trim_start().starts_with("attachment");
        let part_mime = part_ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        let is_non_text = !part_mime.is_empty()
            && !part_mime.starts_with("text/")
            && !part_mime.starts_with("multipart/");

        if is_attachment || is_non_text {
            // Decode bytes.
            let data: Vec<u8> = match part_cte.as_str() {
                "base64" => {
                    use base64::Engine as _;
                    let compact: String = part_body.chars().filter(|c| !c.is_whitespace()).collect();
                    base64::engine::general_purpose::STANDARD
                        .decode(compact.as_bytes())
                        .unwrap_or_default()
                }
                _ => part_body.as_bytes().to_vec(),
            };
            if filename.is_empty() { filename = "attachment".to_owned(); }
            attachments.push(EmailAttachment {
                filename,
                content_type: part_mime,
                data,
            });
        }
    }
    attachments
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
/// Convert a single IMAP FETCH result into a `MessageHeader`, or `None` if
/// the fetch result is missing UID or ENVELOPE data.
fn parse_fetch_to_header(m: &imap::types::Fetch) -> Option<MessageHeader> {
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
}

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

/// Parse the raw bytes from `UID THREAD … ALL` into a list of threads.
///
/// Each thread is a flat `Vec<u32>` of all UIDs belonging to it (nested
/// children are flattened; ordering is depth-first).  Returns an empty vec
/// when the response contains no `* THREAD` line or no UIDs.
///
/// Example input line: `* THREAD (1 2 3)(4)(5 (6)(7 8))\r\n`
/// Returns: `[[1,2,3], [4], [5,6,7,8]]`
pub(crate) fn parse_thread_response(response: &str) -> Vec<Vec<u32>> {
    // Find the * THREAD untagged response.
    let data = response
        .lines()
        .find(|l| l.starts_with("* THREAD"))
        .and_then(|l| l.strip_prefix("* THREAD"))
        .unwrap_or("")
        .trim();

    let mut threads: Vec<Vec<u32>> = Vec::new();
    let mut current: Vec<u32> = Vec::new();
    let mut depth: usize = 0;
    let mut num_buf = String::new();

    let flush_num = |buf: &mut String, cur: &mut Vec<u32>| {
        if !buf.is_empty() {
            if let Ok(uid) = buf.parse::<u32>() {
                cur.push(uid);
            }
            buf.clear();
        }
    };

    for ch in data.chars() {
        match ch {
            '(' => {
                flush_num(&mut num_buf, &mut current);
                depth += 1;
            }
            ')' => {
                flush_num(&mut num_buf, &mut current);
                if depth > 0 {
                    depth -= 1;
                }
                if depth == 0 && !current.is_empty() {
                    threads.push(std::mem::take(&mut current));
                }
            }
            ' ' | '\t' => {
                flush_num(&mut num_buf, &mut current);
            }
            c if c.is_ascii_digit() => {
                num_buf.push(c);
            }
            _ => {
                flush_num(&mut num_buf, &mut current);
            }
        }
    }

    threads
}

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

    // ---- parse_thread_response ----

    #[test]
    fn test_parse_thread_linear_threads() {
        let response = "* THREAD (1 2 3)(4)(5)\r\nA001 OK\r\n";
        let threads = parse_thread_response(response);
        assert_eq!(threads, vec![vec![1, 2, 3], vec![4], vec![5]]);
    }

    #[test]
    fn test_parse_thread_nested() {
        // (5 (6)(7 8)) → all four UIDs in one thread
        let response = "* THREAD (1)(2 3)(4)(5 (6)(7 8))\r\nA002 OK\r\n";
        let threads = parse_thread_response(response);
        assert_eq!(threads.len(), 4);
        assert_eq!(threads[0], vec![1]);
        assert_eq!(threads[1], vec![2, 3]);
        assert_eq!(threads[2], vec![4]);
        // 5, then two children (6) and (7 8) — flattened to [5,6,7,8]
        assert!(threads[3].contains(&5));
        assert!(threads[3].contains(&6));
        assert!(threads[3].contains(&7));
        assert!(threads[3].contains(&8));
    }

    #[test]
    fn test_parse_thread_empty_response() {
        let threads = parse_thread_response("A003 OK THREAD completed\r\n");
        assert!(threads.is_empty());
    }

    #[test]
    fn test_parse_thread_no_thread_line() {
        let threads = parse_thread_response("* OK [CAPABILITY IMAP4rev1]\r\n");
        assert!(threads.is_empty());
    }
}
