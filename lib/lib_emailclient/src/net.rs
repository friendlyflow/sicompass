//! Production IMAP and SMTP backends.
//!
//! `RealImap` implements `ImapBackend` on top of `async-imap` +
//! `async-native-tls`; `RealSmtp` implements `SmtpBackend` on lettre's async
//! transport. Both are instantiated lazily from `EmailClientConfig` in `init()`.
//!
//! Every exchange is bounded by [`IMAP_TIMEOUT`]. The previous blocking backend
//! never called `set_read_timeout`, so a server that accepted the connection and
//! then went silent would block the caller forever.

use crate::cache::EnvelopeCache;
use crate::connection::{connect_imap, ImapSession, RawImap};
use crate::{
    EmailAttachment, EmailClientConfig, EmailMessage, FolderInfo, ImapBackend, MailBody,
    MessageHeader, SmtpBackend,
};
use async_imap::imap_proto::types::Address;
use async_imap::types::Fetch;
use async_trait::async_trait;
use futures::TryStreamExt;
use lettre::message::header::ContentType;
use lettre::message::{Attachment as LettreAttachment, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::time::Duration;

/// Upper bound on a single IMAP or SMTP exchange.
pub const IMAP_TIMEOUT: Duration = Duration::from_secs(30);

/// Run `$inner` under [`IMAP_TIMEOUT`], dropping the session when it expires.
///
/// The future is bound to a `let` first so that its borrow of `$self` has ended
/// by the time `reset_session` needs `&mut $self` again.
macro_rules! timed {
    ($self:ident, $inner:expr) => {{
        let outcome = tokio::time::timeout(IMAP_TIMEOUT, $inner).await;
        match outcome {
            Ok(result) => result,
            Err(_) => {
                $self.reset_session().await;
                Err(format!(
                    "IMAP server did not respond within {}s",
                    IMAP_TIMEOUT.as_secs()
                ))
            }
        }
    }};
}

// ---------------------------------------------------------------------------
// RealImap
// ---------------------------------------------------------------------------

pub struct RealImap {
    config: EmailClientConfig,
    session: Option<ImapSession>,
    cache: Option<EnvelopeCache>,
    /// Separate connection used only for `UID THREAD`; see `fetch_threads`.
    /// Opened lazily on first use and reused across folders.
    thread_conn: Option<RawImap>,
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
            thread_conn: None,
        }
    }

    /// Open the session if it is not already live.
    ///
    /// Returning `()` rather than `&mut ImapSession` keeps the borrow of `self`
    /// from outliving the call, so callers can still reach `reset_session` on
    /// the error path.
    async fn ensure_session(&mut self) -> Result<(), String> {
        if self.session.is_none() {
            self.session = Some(connect_imap(&self.config).await?);
        }
        Ok(())
    }

    /// The live session. Only call after `ensure_session` has succeeded.
    fn session_mut(&mut self) -> &mut ImapSession {
        self.session.as_mut().expect("ensure_session succeeded")
    }

    /// Invalidate the cached session (called after errors and timeouts).
    async fn reset_session(&mut self) {
        if let Some(mut s) = self.session.take() {
            // Best effort: the session is being discarded either way, and after
            // a timeout the server is by definition not answering.
            let _ = tokio::time::timeout(Duration::from_secs(5), s.logout()).await;
        }
    }

    async fn list_folders_inner(&mut self) -> Result<Vec<FolderInfo>, String> {
        self.ensure_session().await?;
        let session = self.session_mut();

        let stream = session.list(None, Some("*")).await.map_err(|e| e.to_string())?;
        let names: Vec<async_imap::types::Name> =
            Box::pin(stream).try_collect().await.map_err(|e| e.to_string())?;

        let folders: Vec<FolderInfo> = names
            .iter()
            .filter_map(|n| {
                // Skip \Noselect folders (containers).
                if n.attributes().iter().any(|a| {
                    matches!(a, async_imap::types::NameAttribute::NoSelect)
                }) {
                    return None;
                }
                // Collect SPECIAL-USE and system attributes as raw strings.
                //
                // imap-proto 0.16 promotes the RFC 6154 attributes to their own
                // variants, where 0.10 delivered every one of them as
                // `Custom("\\Trash")`. Map them back to the same raw strings the
                // rest of the crate matches on (`SpecialFolders`), so folder
                // routing is unaffected by the parser change.
                let attributes: Vec<String> = n
                    .attributes()
                    .iter()
                    .map(|a| {
                        use async_imap::types::NameAttribute as NA;
                        match a {
                            NA::NoInferiors  => "\\Noinferiors".to_owned(),
                            NA::NoSelect     => "\\Noselect".to_owned(),
                            NA::Marked       => "\\Marked".to_owned(),
                            NA::Unmarked     => "\\Unmarked".to_owned(),
                            NA::All          => "\\All".to_owned(),
                            NA::Archive      => "\\Archive".to_owned(),
                            NA::Drafts       => "\\Drafts".to_owned(),
                            NA::Flagged      => "\\Flagged".to_owned(),
                            NA::Junk         => "\\Junk".to_owned(),
                            NA::Sent         => "\\Sent".to_owned(),
                            NA::Trash        => "\\Trash".to_owned(),
                            // Already carries its leading backslash.
                            NA::Extension(s) => s.to_string(),
                            // `NameAttribute` is #[non_exhaustive].
                            other            => format!("{other:?}"),
                        }
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

    /// Inner implementation of `list_messages` that accepts the envelope cache
    /// as a separate parameter, allowing the caller to satisfy the borrow
    /// checker by taking the cache out of `self` first.
    async fn list_messages_inner(
        &mut self,
        folder: &str,
        limit: usize,
        cache: &mut Option<EnvelopeCache>,
    ) -> Result<Vec<MessageHeader>, String> {
        self.ensure_session().await?;
        let session = self.session_mut();

        let mailbox = session.select(folder).await.map_err(|e| e.to_string())?;
        let total = mailbox.exists as usize;
        let uid_validity = mailbox.uid_validity.unwrap_or(0);

        if total == 0 {
            if let &mut Some(ref c) = cache {
                c.invalidate_folder(folder, uid_validity);
            }
            return Ok(vec![]);
        }

        // --- Cache logic ---
        //
        // `EnvelopeCache` wraps a `rusqlite::Connection`, which is `Send` but
        // not `Sync`, so a shared borrow of it may not be held across an
        // `.await` or the whole future stops being `Send` (and `#[async_trait]`
        // requires `Send`). Decide what to do first, drop the borrow, then do
        // the I/O.
        enum Plan {
            /// Cache already holds every message the server reports.
            ServeCached,
            /// Cache is valid but stale; fetch only UIDs above this one.
            Incremental(u32),
            /// No usable cache: fetch the whole window.
            Full,
        }

        let plan = match cache.as_ref() {
            Some(c) if c.get_uidvalidity(folder) == Some(uid_validity) => {
                if c.cached_count(folder) >= total {
                    Plan::ServeCached
                } else if let Some(max_uid) = c.max_uid(folder) {
                    Plan::Incremental(max_uid)
                } else {
                    c.invalidate_folder(folder, uid_validity);
                    Plan::Full
                }
            }
            Some(c) => {
                // UIDVALIDITY mismatch or first visit: flush and refetch.
                c.invalidate_folder(folder, uid_validity);
                Plan::Full
            }
            None => Plan::Full,
        };

        match plan {
            Plan::ServeCached => {
                let c = cache.as_ref().expect("ServeCached implies a cache");
                return Ok(c.get_latest(folder, limit));
            }
            Plan::Incremental(max_uid) => {
                let new_uid_range = format!("{}:*", max_uid + 1);
                let stream = session
                    .uid_fetch(&new_uid_range, "(UID ENVELOPE FLAGS)")
                    .await
                    .map_err(|e| e.to_string())?;
                let fetched: Vec<Fetch> =
                    stream.try_collect().await.map_err(|e| e.to_string())?;
                let new_headers: Vec<MessageHeader> = fetched
                    .iter()
                    .filter_map(parse_fetch_to_header)
                    .collect();

                let c = cache.as_ref().expect("Incremental implies a cache");
                if !new_headers.is_empty() {
                    c.upsert_all(folder, &new_headers);
                }
                return Ok(c.get_latest(folder, limit));
            }
            Plan::Full => {}
        }

        // Full IMAP fetch (cache miss or no cache).
        let start = if total > limit { total - limit + 1 } else { 1 };
        let fetch_range = format!("{start}:{total}");
        let stream = session
            .fetch(&fetch_range, "(UID ENVELOPE FLAGS)")
            .await
            .map_err(|e| e.to_string())?;
        let fetched: Vec<Fetch> = Box::pin(stream)
            .try_collect()
            .await
            .map_err(|e| e.to_string())?;

        let mut headers: Vec<MessageHeader> = fetched
            .iter()
            .filter_map(parse_fetch_to_header)
            .collect();

        headers.reverse(); // Most-recent-first.

        if let &mut Some(ref c) = cache {
            c.upsert_all(folder, &headers);
        }

        Ok(headers)
    }

    async fn fetch_message_inner(
        &mut self,
        folder: &str,
        uid: u32,
    ) -> Result<Option<EmailMessage>, String> {
        self.ensure_session().await?;
        let session = self.session_mut();

        session.select(folder).await.map_err(|e| e.to_string())?;
        let uid_str = uid.to_string();
        let stream = session
            .uid_fetch(&uid_str, "BODY[]")
            .await
            .map_err(|e| e.to_string())?;
        let fetched: Vec<Fetch> = stream.try_collect().await.map_err(|e| e.to_string())?;

        let raw = fetched
            .iter()
            .find(|m| m.uid == Some(uid))
            .and_then(|m| m.body())
            .map(|b| b.to_vec());

        match raw {
            None => Ok(None),
            Some(bytes) => Ok(Some(parse_rfc2822(uid, &bytes))),
        }
    }

    async fn fetch_by_message_id_inner(
        &mut self,
        folder: &str,
        message_id: &str,
    ) -> Result<Option<u32>, String> {
        self.ensure_session().await?;
        let session = self.session_mut();

        session.select(folder).await.map_err(|e| e.to_string())?;
        let search = format!("HEADER Message-ID {message_id}");
        let uids = session.uid_search(&search).await.map_err(|e| e.to_string())?;
        Ok(uids.iter().next().copied())
    }

    async fn set_flags_inner(
        &mut self,
        folder: &str,
        uid: u32,
        add: &[&str],
        remove: &[&str],
    ) -> Result<(), String> {
        self.ensure_session().await?;
        let session = self.session_mut();

        session.select(folder).await.map_err(|e| e.to_string())?;
        let uid_str = uid.to_string();
        if !add.is_empty() {
            let query = format!("+FLAGS ({})", add.join(" "));
            let stream = session
                .uid_store(&uid_str, &query)
                .await
                .map_err(|e| e.to_string())?;
            // The response stream has to be drained or the command never
            // completes on the wire.
            let _: Vec<Fetch> = Box::pin(stream)
                .try_collect()
                .await
                .map_err(|e| e.to_string())?;
        }
        if !remove.is_empty() {
            let query = format!("-FLAGS ({})", remove.join(" "));
            let stream = session
                .uid_store(&uid_str, &query)
                .await
                .map_err(|e| e.to_string())?;
            let _: Vec<Fetch> = Box::pin(stream)
                .try_collect()
                .await
                .map_err(|e| e.to_string())?;
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

    async fn copy_message_inner(
        &mut self,
        folder: &str,
        uid: u32,
        dest: &str,
    ) -> Result<(), String> {
        self.ensure_session().await?;
        let session = self.session_mut();
        session.select(folder).await.map_err(|e| e.to_string())?;
        session
            .uid_copy(&uid.to_string(), dest)
            .await
            .map_err(|e| e.to_string())
    }

    async fn move_message_inner(
        &mut self,
        folder: &str,
        uid: u32,
        dest: &str,
    ) -> Result<(), String> {
        self.ensure_session().await?;
        let session = self.session_mut();
        session.select(folder).await.map_err(|e| e.to_string())?;
        let uid_str = uid.to_string();

        // Try MOVE extension (RFC 6851) first; fall back to COPY + \Deleted + EXPUNGE.
        if session.uid_mv(&uid_str, dest).await.is_ok() {
            return Ok(());
        }
        // Fallback ordering matters: a failed COPY must not leave the message
        // marked \Deleted, or it would be destroyed without arriving.
        session
            .uid_copy(&uid_str, dest)
            .await
            .map_err(|e| e.to_string())?;
        let stream = session
            .uid_store(&uid_str, "+FLAGS (\\Deleted)")
            .await
            .map_err(|e| e.to_string())?;
        let _: Vec<Fetch> = Box::pin(stream)
            .try_collect()
            .await
            .map_err(|e| e.to_string())?;
        let stream = session
            .uid_expunge(&uid_str)
            .await
            .map_err(|e| e.to_string())?;
        let _: Vec<u32> = Box::pin(stream)
            .try_collect()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn expunge_uid_inner(&mut self, folder: &str, uid: u32) -> Result<(), String> {
        self.ensure_session().await?;
        let session = self.session_mut();
        session.select(folder).await.map_err(|e| e.to_string())?;
        let stream = session
            .uid_expunge(&uid.to_string())
            .await
            .map_err(|e| e.to_string())?;
        let _: Vec<u32> = Box::pin(stream)
            .try_collect()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn append_inner(&mut self, folder: &str, message: &[u8]) -> Result<(), String> {
        self.ensure_session().await?;
        let session = self.session_mut();
        session
            .append(folder, None, None, message)
            .await
            .map_err(|e| e.to_string())
    }

    /// `fetch_threads` minus the error bookkeeping.
    async fn threads_inner(&mut self, folder: &str) -> Result<Option<Vec<Vec<u32>>>, String> {
        if self.thread_conn.is_none() {
            self.thread_conn = Some(RawImap::connect(&self.config).await?);
        }
        let raw = self.thread_conn.as_mut().expect("connected above");

        // Returns None (not an error) when the server cannot thread, so the
        // caller falls back to the per-Message-ID SEARCH path.
        let caps = raw.capabilities().await?;
        let algo = if caps.iter().any(|c| c == "THREAD=REFERENCES") {
            "REFERENCES"
        } else if caps.iter().any(|c| c == "THREAD=ORDEREDSUBJECT") {
            "ORDEREDSUBJECT"
        } else {
            return Ok(None);
        };

        let response = raw.uid_thread(folder, algo).await?;
        Ok(Some(parse_thread_response(&response)))
    }
}

#[async_trait]
impl ImapBackend for RealImap {
    async fn list_folders(&mut self) -> Result<Vec<FolderInfo>, String> {
        timed!(self, self.list_folders_inner())
    }

    async fn list_messages(
        &mut self,
        folder: &str,
        limit: usize,
    ) -> Result<Vec<MessageHeader>, String> {
        // Take the cache out of self so we can hold a session borrow at the
        // same time (the borrow checker can't prove they're disjoint fields).
        let mut cache = self.cache.take();
        let result = timed!(self, self.list_messages_inner(folder, limit, &mut cache));
        self.cache = cache;
        result
    }

    async fn fetch_message(
        &mut self,
        folder: &str,
        uid: u32,
    ) -> Result<Option<EmailMessage>, String> {
        timed!(self, self.fetch_message_inner(folder, uid))
    }

    async fn fetch_message_by_message_id(
        &mut self,
        folder: &str,
        message_id: &str,
    ) -> Result<Option<EmailMessage>, String> {
        let uid = timed!(self, self.fetch_by_message_id_inner(folder, message_id))?;
        match uid {
            // Reuse the normal fetch path.
            Some(uid) => self.fetch_message(folder, uid).await,
            None => Ok(None),
        }
    }

    async fn set_flags(
        &mut self,
        folder: &str,
        uid: u32,
        add: &[&str],
        remove: &[&str],
    ) -> Result<(), String> {
        timed!(self, self.set_flags_inner(folder, uid, add, remove))
    }

    async fn copy_message(&mut self, folder: &str, uid: u32, dest: &str) -> Result<(), String> {
        timed!(self, self.copy_message_inner(folder, uid, dest))
    }

    async fn move_message(&mut self, folder: &str, uid: u32, dest: &str) -> Result<(), String> {
        timed!(self, self.move_message_inner(folder, uid, dest))
    }

    async fn expunge_uid(&mut self, folder: &str, uid: u32) -> Result<(), String> {
        timed!(self, self.expunge_uid_inner(folder, uid))
    }

    async fn append(&mut self, folder: &str, message: &[u8]) -> Result<(), String> {
        timed!(self, self.append_inner(folder, message))
    }

    async fn fetch_threads(&mut self, folder: &str) -> Result<Option<Vec<Vec<u32>>>, String> {
        // Runs on a dedicated `RawImap` rather than the main session: neither
        // imap-proto release can decode a `* THREAD` response (see `RawImap`'s
        // docs), and keeping it off the main session also stops it from
        // changing which mailbox that session has selected.
        let outcome = tokio::time::timeout(IMAP_TIMEOUT, self.threads_inner(folder)).await;
        match outcome {
            Ok(Ok(threads)) => Ok(threads),
            Ok(Err(e)) => {
                // Force a reconnect on the next call — a half-open connection
                // would fail every subsequent fetch.
                self.thread_conn = None;
                Err(e)
            }
            Err(_) => {
                self.thread_conn = None;
                Err(format!(
                    "IMAP server did not respond to THREAD within {}s",
                    IMAP_TIMEOUT.as_secs()
                ))
            }
        }
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

#[async_trait]
impl SmtpBackend for RealSmtp {
    async fn send(
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

        let transport: AsyncSmtpTransport<Tokio1Executor> =
            if self.config.oauth_access_token.is_empty() {
                let creds = Credentials::new(
                    self.config.username.clone(),
                    self.config.password.clone(),
                );
                AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
                    .map_err(|e| e.to_string())?
                    .port(port)
                    .credentials(creds)
                    .build()
            } else {
                let creds = Credentials::new(
                    self.config.username.clone(),
                    self.config.oauth_access_token.clone(),
                );
                AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
                    .map_err(|e| e.to_string())?
                    .port(port)
                    .credentials(creds)
                    .authentication(vec![Mechanism::Xoauth2])
                    .build()
            };

        // Connect + TLS + auth + DATA used to run on the render thread with no
        // bound at all.
        let sent = tokio::time::timeout(IMAP_TIMEOUT, transport.send(email.clone())).await;
        match sent {
            Ok(Ok(_)) => Ok(raw),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err(format!(
                "SMTP server did not respond within {}s",
                IMAP_TIMEOUT.as_secs()
            )),
        }
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
fn parse_fetch_to_header(m: &Fetch) -> Option<MessageHeader> {
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
    // `flags()` yields an iterator in async-imap, where the blocking crate
    // returned a slice.
    let seen = m.flags().any(|f| matches!(f, async_imap::types::Flag::Seen));
    let flagged = m.flags().any(|f| matches!(f, async_imap::types::Flag::Flagged));
    let message_id = env
        .message_id
        .as_deref()
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("")
        .to_owned();
    Some(MessageHeader { uid, from, subject, date, seen, flagged, message_id })
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
            name: Some(b"Alice".as_slice().into()),
            adl: None,
            mailbox: Some(b"alice".as_slice().into()),
            host: Some(b"example.com".as_slice().into()),
        };
        assert_eq!(format_address(&addr), "Alice <alice@example.com>");
    }

    #[test]
    fn test_format_address_without_name() {
        let addr = Address {
            name: None,
            adl: None,
            mailbox: Some(b"bob".as_slice().into()),
            host: Some(b"example.com".as_slice().into()),
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
        let folders = crate::connection::block_on(backend.list_folders())
            .expect("list_folders failed");
        assert!(!folders.is_empty(), "expected at least one folder");
        println!("folders: {:?}", folders.iter().map(|f| &f.name).collect::<Vec<_>>());

        let inbox = folders.iter().find(|f| f.name.to_uppercase() == "INBOX")
            .expect("INBOX not found");
        let headers = crate::connection::block_on(backend.list_messages(&inbox.name, 5))
            .expect("list_messages failed");
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
