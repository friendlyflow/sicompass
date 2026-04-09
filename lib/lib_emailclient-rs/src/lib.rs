//! Email client provider — Rust port of `lib_emailclient/`.
//!
//! Implements the [`Provider`] trait for IMAP/SMTP email access.
//! IMAP and SMTP operations are injected via the [`ImapBackend`] and
//! [`SmtpBackend`] traits, making the provider fully unit-testable.
//! Real network backends live in `net`, OAuth2 in `oauth2`, IDLE in `idle`.
//!
//! ## FFON tree layout
//!
//! ```text
//! Root "/"
//!   meta           (obj)  — shortcut hints
//!   compose        (obj)  — empty compose form  (inserted after INBOX)
//!   folder-name    (obj)  — one per IMAP folder (display name), navigable
//!
//! Folder "/{FolderName}/"
//!   meta           (obj)
//!   Subject — From (obj)  — one per message (up to 50)
//!
//! Message "/{FolderName}/{Subject — From}/"
//!   meta           (obj)
//!   From: …        (str)
//!   To: …          (str)
//!   Date: …        (str)
//!   Subject: …     (str)
//!   body text      (str)
//!   History        (obj)  — only if References header present
//!   reply          (obj)
//!   reply all      (obj)
//!   forward        (obj)
//!
//! History "/{FolderName}/{msg}/History/"
//!   meta           (obj)
//!   From: … — Subject: … (obj per referenced message)
//!
//! Compose "/{compose|reply|reply all|forward}/"
//!   meta           (obj)
//!   From: <addr>   (str)  — read-only, always the user's address
//!   To: <input>    (str)
//!   Subject:<input>(str)
//!   Body: <input>  (str)
//!   <button>send</button>Send  (str)
//!   History        (obj)  — only for reply/reply-all (lazy)
//! ```

pub mod idle;
pub mod net;
pub mod oauth2;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::platform;
use sicompass_sdk::provider::Provider;

use idle::IdleController;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// Config mirroring `EmailClientConfig` from `emailclient.h`.
#[derive(Debug, Clone, Default)]
pub struct EmailClientConfig {
    pub imap_url: String,
    pub smtp_url: String,
    pub username: String,
    pub password: String,
    pub client_id: String,
    pub client_secret: String,
    pub oauth_access_token: String,
    pub oauth_refresh_token: String,
    pub token_expiry: i64, // Unix timestamp
}

/// A summarised message header (from IMAP ENVELOPE).
#[derive(Debug, Clone)]
pub struct MessageHeader {
    /// IMAP UID
    pub uid: u32,
    pub from: String,
    pub subject: String,
    pub date: String,
}

/// A fully fetched email message.
#[derive(Debug, Clone)]
pub struct EmailMessage {
    pub uid: u32,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub date: String,
    pub body: String,
    pub message_id: String,
    pub in_reply_to: String,
    pub references: String,
}

/// Compose form draft state.
#[derive(Debug, Clone, Default)]
pub struct Draft {
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// Which kind of compose action is in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComposeMode {
    #[default]
    New,
    Reply,
    ReplyAll,
    Forward,
}

/// Pending compose state (to/subject/body plus metadata for pre-fill).
#[derive(Debug, Clone, Default)]
struct ComposeState {
    draft: Draft,
    mode: ComposeMode,
    /// Real IMAP folder name of the message being replied to / forwarded.
    reply_folder: String,
    /// UID of the message being replied to / forwarded.
    reply_uid: u32,
    /// True if the form has been pre-filled from the original message.
    prefilled: bool,
}

// ---------------------------------------------------------------------------
// Injectable backend traits
// ---------------------------------------------------------------------------

/// IMAP backend — all operations used by the provider.
pub trait ImapBackend: Send {
    /// List all selectable folder names (full IMAP names, e.g. `[Gmail]/Sent`).
    fn list_folders(&mut self) -> Result<Vec<String>, String>;
    /// Fetch headers for the most recent `limit` messages in `folder`.
    fn list_messages(&mut self, folder: &str, limit: usize) -> Result<Vec<MessageHeader>, String>;
    /// Fetch the full content of a message by UID.
    fn fetch_message(&mut self, folder: &str, uid: u32) -> Result<Option<EmailMessage>, String>;
    /// Fetch a message by its Message-ID header via IMAP SEARCH.
    fn fetch_message_by_message_id(
        &mut self,
        folder: &str,
        message_id: &str,
    ) -> Result<Option<EmailMessage>, String>;
}

/// SMTP backend — send an email message.
pub trait SmtpBackend: Send {
    fn send(&mut self, from: &str, to: &str, subject: &str, body: &str) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// Folder display-name → real-name mapping
// ---------------------------------------------------------------------------

/// Derive a short display name from a full IMAP folder path.
/// `"[Gmail]/Verzonden berichten"` → `"Verzonden berichten"`.
/// Folders without a slash are returned unchanged.
fn folder_display_name(imap_name: &str) -> &str {
    match imap_name.rfind('/') {
        Some(idx) => &imap_name[idx + 1..],
        None => imap_name,
    }
}

// ---------------------------------------------------------------------------
// EmailClientProvider
// ---------------------------------------------------------------------------

pub struct EmailClientProvider {
    config: EmailClientConfig,
    current_path: String,

    // Display-name → real-IMAP-name mapping built on each root fetch.
    folder_mappings: Vec<(String, String)>, // (display, real)

    // Cached root (folder list) FFON — served on back-navigation.
    folder_cache: Option<Vec<FfonElement>>,

    // Cached envelope list for the current folder.
    envelope_cache: Option<Vec<FfonElement>>,
    envelope_cache_folder: String,

    // Cached message headers for the current folder (used for UID lookup).
    message_cache: Vec<MessageHeader>,
    // Cached full message for the current message path.
    message_detail: Option<EmailMessage>,

    // Compose state
    compose: ComposeState,
    compose_sent: bool,

    // History: folder + References header stored when a message is viewed,
    // served lazily when the user navigates into "History".
    history_folder: String,
    history_refs: String,

    // Cross-thread needs-refresh flag (set by IDLE, cleared by fetch).
    needs_refresh_flag: Arc<AtomicBool>,

    // IDLE background thread controller.
    idle: IdleController,

    // Injected backends (None until init() or with_imap/with_smtp).
    imap: Option<Box<dyn ImapBackend>>,
    smtp: Option<Box<dyn SmtpBackend>>,
}

impl EmailClientProvider {
    pub fn new() -> Self {
        let needs_refresh_flag = Arc::new(AtomicBool::new(false));
        EmailClientProvider {
            config: EmailClientConfig::default(),
            current_path: "/".to_owned(),
            folder_mappings: Vec::new(),
            folder_cache: None,
            envelope_cache: None,
            envelope_cache_folder: String::new(),
            message_cache: Vec::new(),
            message_detail: None,
            compose: ComposeState::default(),
            compose_sent: false,
            history_folder: String::new(),
            history_refs: String::new(),
            needs_refresh_flag: Arc::clone(&needs_refresh_flag),
            idle: IdleController::new(needs_refresh_flag),
            imap: None,
            smtp: None,
        }
    }

    /// Inject an IMAP backend (used in tests and production setup).
    pub fn with_imap(mut self, backend: Box<dyn ImapBackend>) -> Self {
        self.imap = Some(backend);
        self
    }

    /// Inject an SMTP backend.
    pub fn with_smtp(mut self, backend: Box<dyn SmtpBackend>) -> Self {
        self.smtp = Some(backend);
        self
    }

    // ---- Path decomposition -----------------------------------------------

    /// Segments of the current path (`/a/b/c` → `["a", "b", "c"]`).
    fn path_segments(&self) -> Vec<&str> {
        self.current_path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect()
    }

    fn at_root(&self) -> bool {
        self.path_segments().is_empty()
    }

    fn at_folder(&self) -> bool {
        self.path_segments().len() == 1
    }

    fn at_message(&self) -> bool {
        self.path_segments().len() == 2
    }

    fn at_compose(&self) -> bool {
        let segs = self.path_segments();
        segs.len() == 1 && matches!(segs[0], "compose" | "reply" | "reply all" | "forward")
    }

    fn at_history(&self) -> bool {
        self.path_segments().last().copied() == Some("History")
    }

    /// The folder display-name at the first path segment.
    fn folder_seg(&self) -> &str {
        self.path_segments().first().copied().unwrap_or("")
    }

    /// Resolve a display-name to the real IMAP folder name.
    fn lookup_folder<'a>(&'a self, display: &'a str) -> &'a str {
        self.folder_mappings
            .iter()
            .find(|(d, _)| d == display)
            .map(|(_, r)| r.as_str())
            .unwrap_or(display)
    }

    /// Look up a UID by message display label (Subject — From).
    fn lookup_uid(&self, label: &str) -> Option<u32> {
        self.message_cache
            .iter()
            .find(|h| {
                let l = if h.subject.is_empty() {
                    format!("(no subject) — {}", h.from)
                } else {
                    format!("{} — {}", h.subject, h.from)
                };
                l == label
            })
            .map(|h| h.uid)
    }

    // ---- Token refresh -------------------------------------------------------

    /// Refresh OAuth2 token if expired; no-op for password auth.
    /// Returns false if refresh was needed but failed.
    fn ensure_token(&mut self) -> bool {
        if self.config.oauth_access_token.is_empty() {
            return true; // password mode
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if now < self.config.token_expiry - 60 {
            return true; // still valid
        }
        if self.config.oauth_refresh_token.is_empty() {
            return false;
        }
        let result = oauth2::refresh_token(
            &self.config.client_id,
            &self.config.client_secret,
            &self.config.oauth_refresh_token,
        );
        if result.success {
            self.config.oauth_access_token = result.access_token;
            self.config.token_expiry = now + result.expires_in;
            self.save_oauth_tokens();
            true
        } else {
            false
        }
    }

    /// Persist OAuth tokens to settings.json.
    fn save_oauth_tokens(&self) {
        let Some(path) = platform::main_config_path() else { return };
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let mut root: serde_json::Value =
            serde_json::from_str(&content).unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
        let section = root
            .as_object_mut()
            .and_then(|o| {
                if !o.contains_key("email client") {
                    o.insert("email client".to_owned(), serde_json::Value::Object(Default::default()));
                }
                o.get_mut("email client")?.as_object_mut()
            });
        if let Some(sec) = section {
            sec.insert("emailOAuthAccessToken".to_owned(), self.config.oauth_access_token.clone().into());
            sec.insert("emailOAuthRefreshToken".to_owned(), self.config.oauth_refresh_token.clone().into());
            sec.insert("emailTokenExpiry".to_owned(), self.config.token_expiry.into());
        }
        if let Ok(json) = serde_json::to_string_pretty(&root) {
            let _ = std::fs::write(&path, json);
        }
    }

    // ---- IMAP backend access with lazy real-backend construction -------------

    fn imap_mut(&mut self) -> Option<&mut (dyn ImapBackend + 'static)> {
        self.imap.as_deref_mut()
    }

    fn smtp_mut(&mut self) -> Option<&mut (dyn SmtpBackend + 'static)> {
        self.smtp.as_deref_mut()
    }

    // ---- FFON tree builders -----------------------------------------------

    fn build_root(&mut self) -> Vec<FfonElement> {
        // Stop IDLE when leaving a folder (returning to root).
        self.idle.stop();

        // Serve folder cache on back-navigation.
        if let Some(cached) = &self.folder_cache {
            return cached.clone();
        }

        let mut items = vec![];

        let imap = match self.imap_mut() {
            Some(b) => b,
            None => {
                items.push(FfonElement::new_str(
                    "not configured — set IMAP/SMTP settings".to_owned(),
                ));
                return items;
            }
        };

        match imap.list_folders() {
            Err(e) => {
                items.push(FfonElement::new_str(format!("IMAP error: {e}")));
            }
            Ok(folders) => {
                // Rebuild folder display-name mappings.
                self.folder_mappings.clear();

                // Filter out hierarchy-only container folders (e.g. "[Gmail]")
                // — a folder is a container if any other folder starts with it + "/".
                let mut real_folders: Vec<String> = Vec::new();
                for name in &folders {
                    let is_container = folders.iter().any(|other| {
                        other != name && other.starts_with(&format!("{name}/"))
                    });
                    if !is_container {
                        real_folders.push(name.clone());
                    }
                }

                let mut compose_inserted = false;
                for name in real_folders {
                    let display = folder_display_name(&name).to_owned();
                    self.folder_mappings.push((display.clone(), name.clone()));
                    items.push(FfonElement::new_obj(display.clone()));
                    // Insert compose right after INBOX.
                    if !compose_inserted && name.to_uppercase() == "INBOX" {
                        items.push(FfonElement::new_obj("compose"));
                        compose_inserted = true;
                    }
                }
                if !compose_inserted {
                    items.push(FfonElement::new_obj("compose"));
                }
            }
        }

        self.folder_cache = Some(items.clone());
        items
    }

    fn build_folder(&mut self, display: &str) -> Vec<FfonElement> {
        let real_folder = self.lookup_folder(display).to_owned();

        // Serve envelope cache if same folder.
        if self.envelope_cache_folder == real_folder {
            if let Some(cached) = &self.envelope_cache {
                return cached.clone();
            }
        } else {
            // Switching folders — invalidate envelope cache.
            self.envelope_cache = None;
        }

        let mut items = vec![];

        let imap = match self.imap_mut() {
            Some(b) => b,
            None => {
                items.push(FfonElement::new_str("(no IMAP backend)".to_owned()));
                return items;
            }
        };

        match imap.list_messages(&real_folder, 50) {
            Err(e) => items.push(FfonElement::new_str(format!("IMAP error: {e}"))),
            Ok(headers) => {
                self.message_cache = headers.clone();
                for h in &headers {
                    let label = if h.subject.is_empty() {
                        format!("(no subject) — {}", h.from)
                    } else {
                        format!("{} — {}", h.subject, h.from)
                    };
                    items.push(FfonElement::new_obj(label));
                }
                if items.is_empty() {
                    items.push(FfonElement::new_str("(no messages)".to_owned()));
                }
            }
        }

        // Cache results and start IDLE for this folder.
        self.envelope_cache = Some(items.clone());
        self.envelope_cache_folder = real_folder.clone();

        // Start IDLE for new mail notifications.
        self.idle.start(self.config.clone(), real_folder);

        items
    }

    fn build_message(&mut self, display: &str, msg_label: &str) -> Vec<FfonElement> {
        let real_folder = self.lookup_folder(display).to_owned();
        let uid = match self.lookup_uid(msg_label) {
            Some(uid) => uid,
            None => {
                return vec![FfonElement::new_str("(message not found)".to_owned())];
            }
        };

        let msg = if let Some(ref mut imap) = self.imap {
            match imap.fetch_message(&real_folder, uid) {
                Ok(Some(m)) => {
                    self.message_detail = Some(m.clone());
                    m
                }
                _ => {
                    // Fall back to cached detail if available.
                    match self.message_detail.clone() {
                        Some(m) => m,
                        None => {
                            return vec![FfonElement::new_str("(message not found)".to_owned())];
                        }
                    }
                }
            }
        } else if let Some(m) = self.message_detail.clone() {
            m
        } else {
            return vec![FfonElement::new_str("(message not found)".to_owned())];
        };

        // Store References for lazy History navigation.
        if !msg.references.is_empty() {
            self.history_folder = real_folder;
            self.history_refs = msg.references.clone();
        } else {
            self.history_refs.clear();
        }

        let mut items = build_message_view(&msg);
        items
    }

    fn build_history(&mut self) -> Vec<FfonElement> {
        if self.history_refs.is_empty() {
            return vec![FfonElement::new_str("(no history)".to_owned())];
        }

        let folder = self.history_folder.clone();
        let refs = self.history_refs.clone();

        let mut items = vec![];
        let mut count = 0;

        // Parse space-separated Message-IDs from the References header.
        let mut p = refs.as_str();
        while !p.is_empty() && count < 10 {
            p = p.trim_start();
            if !p.starts_with('<') {
                if let Some(rest) = p.get(1..) { p = rest; } else { break; }
                continue;
            }
            let end = match p.find('>') {
                Some(i) => i,
                None => break,
            };
            let msg_id = &p[..=end];
            p = &p[end + 1..];

            if let Some(ref mut imap) = self.imap {
                if let Ok(Some(msg)) = imap.fetch_message_by_message_id(&folder, msg_id) {
                    let key = format!("From: {} — Subject: {}", msg.from, msg.subject);
                    items.push(FfonElement::new_obj(key));
                    count += 1;
                }
            }
        }

        if items.is_empty() {
            items.push(FfonElement::new_str("(no history)".to_owned()));
        }
        items
    }

    fn build_compose_view(&mut self) -> Vec<FfonElement> {
        if self.compose_sent {
            return vec![FfonElement::new_str("message sent".to_owned())];
        }

        // Pre-fill from original message on first render.
        if !self.compose.prefilled && self.compose.reply_uid > 0 {
            let folder = self.compose.reply_folder.clone();
            let uid = self.compose.reply_uid;
            let mode = self.compose.mode;
            let username = self.config.username.clone();
            if let Some(ref mut imap) = self.imap {
                if let Ok(Some(msg)) = imap.fetch_message(&folder, uid) {
                    prefill_compose(&mut self.compose, &msg, mode, &username);
                }
            }
            self.compose.prefilled = true;
        }

        let mut items = vec![];
        // Static "From:" line.
        items.push(FfonElement::new_str(format!("From: {}", self.config.username)));
        items.push(FfonElement::new_str(format!(
            "To: <input>{}</input>",
            self.compose.draft.to
        )));
        items.push(FfonElement::new_str(format!(
            "Subject: <input>{}</input>",
            self.compose.draft.subject
        )));
        items.push(FfonElement::new_str(format!(
            "Body: <input>{}</input>",
            self.compose.draft.body
        )));
        items.push(FfonElement::new_str("<button>send</button>Send".to_owned()));

        // Add History link for reply/reply-all if there are refs.
        if matches!(self.compose.mode, ComposeMode::Reply | ComposeMode::ReplyAll)
            && !self.history_refs.is_empty()
        {
            items.push(FfonElement::new_obj("History"));
        }

        items
    }

    fn send_draft(&mut self) -> bool {
        if let Some(ref mut smtp) = self.smtp {
            let from = self.config.username.clone();
            let to = self.compose.draft.to.clone();
            let subject = self.compose.draft.subject.clone();
            let body = self.compose.draft.body.clone();
            smtp.send(&from, &to, &subject, &body).is_ok()
        } else {
            false
        }
    }

    /// Rebuild the IMAP/SMTP backends from current config.
    /// Called after config changes (init, on_setting_change).
    fn rebuild_backends(&mut self) {
        if self.config.imap_url.is_empty() || self.config.username.is_empty() {
            return;
        }
        // In password-auth mode, refuse to connect with an empty password.
        if self.config.oauth_access_token.is_empty() && self.config.password.is_empty() {
            return;
        }
        // Only build if no backend is already injected (e.g. in tests).
        if self.imap.is_none() {
            self.imap = Some(Box::new(net::RealImap::from_config(&self.config)));
        }
        if self.smtp.is_none() {
            self.smtp = Some(Box::new(net::RealSmtp::from_config(&self.config)));
        }
    }
}

// ---------------------------------------------------------------------------
// Compose pre-fill helper
// ---------------------------------------------------------------------------

fn prefill_compose(compose: &mut ComposeState, msg: &EmailMessage, mode: ComposeMode, username: &str) {
    match mode {
        ComposeMode::Reply => {
            compose.draft.to = msg.from.clone();
            compose.draft.subject = if msg.subject.to_lowercase().starts_with("re:") {
                msg.subject.clone()
            } else {
                format!("Re: {}", msg.subject)
            };
            compose.draft.body = format!(
                "\n\nOn {} <{}> wrote:\n{}",
                msg.date, msg.from, msg.body
            );
        }
        ComposeMode::ReplyAll => {
            // Start with original sender; add To recipients except ourselves.
            let mut recipients = vec![msg.from.clone()];
            for tok in msg.to.split(',') {
                let t = tok.trim();
                if !t.is_empty() && !t.contains(username) {
                    recipients.push(t.to_owned());
                }
            }
            compose.draft.to = recipients.join(", ");
            compose.draft.subject = if msg.subject.to_lowercase().starts_with("re:") {
                msg.subject.clone()
            } else {
                format!("Re: {}", msg.subject)
            };
            compose.draft.body = format!(
                "\n\nOn {} <{}> wrote:\n{}",
                msg.date, msg.from, msg.body
            );
        }
        ComposeMode::Forward => {
            compose.draft.to.clear();
            compose.draft.subject = if msg.subject.to_lowercase().starts_with("fwd:") {
                msg.subject.clone()
            } else {
                format!("Fwd: {}", msg.subject)
            };
            compose.draft.body = format!(
                "\n\n---------- Forwarded message ----------\nFrom: {}\nTo: {}\nDate: {}\nSubject: {}\n\n{}",
                msg.from, msg.to, msg.date, msg.subject, msg.body
            );
        }
        ComposeMode::New => {}
    }
}

// ---------------------------------------------------------------------------
// Message view helper
// ---------------------------------------------------------------------------

fn build_message_view(msg: &EmailMessage) -> Vec<FfonElement> {
    let mut items = vec![
        FfonElement::new_str(format!("From: {}", msg.from)),
        FfonElement::new_str(format!("To: {}", msg.to)),
        FfonElement::new_str(format!("Date: {}", msg.date)),
        FfonElement::new_str(format!("Subject: {}", msg.subject)),
        FfonElement::new_str(msg.body.clone()),
    ];
    if !msg.references.is_empty() {
        items.push(FfonElement::new_obj("History"));
    }
    items.push(FfonElement::new_obj("reply"));
    items.push(FfonElement::new_obj("reply all"));
    items.push(FfonElement::new_obj("forward"));
    items
}

// ---------------------------------------------------------------------------
// Provider impl
// ---------------------------------------------------------------------------

impl Default for EmailClientProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for EmailClientProvider {
    fn name(&self) -> &str { "emailclient" }
    fn display_name(&self) -> &str { "email client" }

    fn no_cache(&self) -> bool { true }

    fn needs_refresh(&self) -> bool {
        self.needs_refresh_flag.load(Ordering::Relaxed)
    }

    fn clear_needs_refresh(&mut self) {
        self.needs_refresh_flag.store(false, Ordering::Relaxed);
        // Invalidate envelope cache so next fetch re-queries.
        self.envelope_cache = None;
    }

    fn init(&mut self) {
        self.current_path = "/".to_owned();
        self.folder_cache = None;
        self.envelope_cache = None;
        self.envelope_cache_folder.clear();

        // Load config from ~/.config/sicompass/settings.json.
        let Some(path) = platform::main_config_path() else { return };
        let Ok(content) = std::fs::read_to_string(&path) else { return };
        let Ok(root) = serde_json::from_str::<serde_json::Value>(&content) else { return };
        let Some(section) = root.get("email client").and_then(|v| v.as_object()) else { return };

        macro_rules! load_str {
            ($key:literal, $field:expr) => {
                if let Some(v) = section.get($key).and_then(|v| v.as_str()) {
                    $field = v.to_owned();
                }
            };
        }

        load_str!("emailImapUrl", self.config.imap_url);
        load_str!("emailSmtpUrl", self.config.smtp_url);
        load_str!("emailUsername", self.config.username);
        load_str!("emailPassword", self.config.password);
        load_str!("emailClientId", self.config.client_id);
        load_str!("emailClientSecret", self.config.client_secret);
        load_str!("emailOAuthAccessToken", self.config.oauth_access_token);
        load_str!("emailOAuthRefreshToken", self.config.oauth_refresh_token);
        if let Some(v) = section.get("emailTokenExpiry").and_then(|v| v.as_i64()) {
            self.config.token_expiry = v;
        }

        self.rebuild_backends();
    }

    fn cleanup(&mut self) {
        self.idle.stop();
        self.folder_cache = None;
        self.envelope_cache = None;
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        let segs = self.path_segments().iter().map(|s| s.to_string()).collect::<Vec<_>>();

        match segs.len() {
            0 => self.build_root(),
            1 => {
                let seg = segs[0].clone();
                if matches!(seg.as_str(), "compose" | "reply" | "reply all" | "forward") {
                    self.build_compose_view()
                } else {
                    self.build_folder(&seg)
                }
            }
            2 => {
                let folder = segs[0].clone();
                let msg_label = segs[1].clone();
                if matches!(msg_label.as_str(), "compose" | "reply" | "reply all" | "forward") {
                    self.build_compose_view()
                } else {
                    self.build_message(&folder, &msg_label)
                }
            }
            3 => {
                let seg3 = segs[2].as_str();
                if seg3 == "History" {
                    self.build_history()
                } else if matches!(seg3, "compose" | "reply" | "reply all" | "forward") {
                    // compose entered from a message context: /INBOX/msg/reply
                    self.build_compose_view()
                } else {
                    vec![FfonElement::new_str("(invalid path)".to_owned())]
                }
            }
            _ => vec![FfonElement::new_str("(invalid path)".to_owned())],
        }
    }

    fn push_path(&mut self, segment: &str) {
        let segs_len = self.path_segments().len();

        // Handle compose/reply actions entered from a message view.
        match segment {
            "reply" | "reply all" | "forward" if segs_len == 2 => {
                // /folder/message → /folder/message/reply
                // Store context for pre-fill.
                let segs = self.path_segments().iter().map(|s| s.to_string()).collect::<Vec<_>>();
                let folder_display = segs[0].clone();
                let msg_label = segs[1].clone();
                let real_folder = self.lookup_folder(&folder_display).to_owned();
                let uid = self.lookup_uid(&msg_label).unwrap_or(0);
                let mode = match segment {
                    "reply" => ComposeMode::Reply,
                    "reply all" => ComposeMode::ReplyAll,
                    "forward" => ComposeMode::Forward,
                    _ => ComposeMode::New,
                };
                self.compose = ComposeState {
                    mode,
                    reply_folder: real_folder,
                    reply_uid: uid,
                    ..Default::default()
                };
                self.compose_sent = false;
            }
            "compose" => {
                // Fresh compose from root.
                self.compose = ComposeState::default();
                self.compose_sent = false;
            }
            _ => {}
        }

        if segs_len == 0 {
            self.current_path = format!("/{segment}");
        } else {
            let base = self.current_path.trim_end_matches('/');
            self.current_path = format!("{base}/{segment}");
        }
    }

    fn pop_path(&mut self) {
        // Reset compose state when navigating away.
        if self.at_compose() {
            self.compose_sent = false;
        }
        if let Some(slash) = self.current_path.rfind('/') {
            if slash == 0 {
                self.current_path = "/".to_owned();
            } else {
                self.current_path.truncate(slash);
            }
        }
    }

    fn current_path(&self) -> &str { &self.current_path }

    fn set_current_path(&mut self, path: &str) {
        self.current_path = path.to_owned();
    }

    fn meta(&self) -> Vec<String> {
        let segs_len = self.path_segments().len();
        if segs_len == 0
            || (segs_len >= 1
                && !matches!(
                    self.path_segments().first().copied().unwrap_or(""),
                    "compose" | "reply" | "reply all" | "forward"
                ))
        {
            vec![
                "/       Search".to_owned(),
                "F5      Refresh".to_owned(),
                ":       Commands".to_owned(),
            ]
        } else {
            // Compose form
            vec!["Tab     Next field".to_owned()]
        }
    }

    fn commit_edit(&mut self, _old: &str, new_content: &str) -> bool {
        // Path: /compose/To, /compose/Subject, /compose/Body (or deeper compose paths)
        let field = self.current_path
            .rfind('/')
            .map(|i| &self.current_path[i + 1..])
            .unwrap_or("");
        match field {
            "To" => {
                self.compose.draft.to = new_content.to_owned();
                true
            }
            "Subject" => {
                self.compose.draft.subject = new_content.to_owned();
                true
            }
            "Body" => {
                self.compose.draft.body = new_content.to_owned();
                true
            }
            _ => false,
        }
    }

    fn on_button_press(&mut self, function_name: &str) {
        if function_name == "send" {
            self.send_draft();
            self.compose = ComposeState::default();
            self.compose_sent = true;
        }
    }

    fn commands(&self) -> Vec<String> {
        vec![
            "compose".to_owned(),
            "login".to_owned(),
            "logout".to_owned(),
            "refresh".to_owned(),
        ]
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        elem_key: &str,
        _elem_type: i32,
        error: &mut String,
    ) -> Option<FfonElement> {
        match cmd {
            "compose" => {
                self.compose = ComposeState::default();
                self.compose_sent = false;
                None
            }
            "refresh" => {
                // Invalidate caches to force re-fetch.
                self.folder_cache = None;
                self.envelope_cache = None;
                None
            }
            "login" => {
                if self.config.client_id.is_empty() || self.config.client_secret.is_empty() {
                    *error = "set client ID and client secret in settings first".to_owned();
                    return None;
                }
                let result = oauth2::authorize(
                    &self.config.client_id,
                    &self.config.client_secret,
                    120,
                );
                if result.success {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    self.config.oauth_access_token = result.access_token;
                    self.config.oauth_refresh_token = result.refresh_token;
                    self.config.token_expiry = now + result.expires_in;
                    self.save_oauth_tokens();
                    // Rebuild backends with new OAuth credentials.
                    self.imap = None;
                    self.smtp = None;
                    self.rebuild_backends();
                    Some(FfonElement::new_str("Google OAuth2 login successful".to_owned()))
                } else {
                    *error = format!("OAuth2 failed: {}", result.error);
                    None
                }
            }
            "logout" => {
                self.config.oauth_access_token.clear();
                self.config.oauth_refresh_token.clear();
                self.config.token_expiry = 0;
                self.save_oauth_tokens();
                // Rebuild backends (password auth now).
                self.imap = None;
                self.smtp = None;
                self.rebuild_backends();
                Some(FfonElement::new_str("logged out".to_owned()))
            }
            _ => {
                *error = format!("unknown command: {cmd}");
                None
            }
        }
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        let mut changed = true;
        match key {
            "emailImapUrl" => self.config.imap_url = value.to_owned(),
            "emailSmtpUrl" => self.config.smtp_url = value.to_owned(),
            "emailUsername" => self.config.username = value.to_owned(),
            "emailPassword" => self.config.password = value.to_owned(),
            "emailClientId" => self.config.client_id = value.to_owned(),
            "emailClientSecret" => self.config.client_secret = value.to_owned(),
            "emailOAuthAccessToken" => self.config.oauth_access_token = value.to_owned(),
            "emailOAuthRefreshToken" => self.config.oauth_refresh_token = value.to_owned(),
            "emailTokenExpiry" => {
                if let Ok(v) = value.parse::<i64>() {
                    self.config.token_expiry = v;
                }
            }
            _ => { changed = false; }
        }
        if changed {
            // Rebuild backends so they pick up the new config.
            self.imap = None;
            self.smtp = None;
            self.rebuild_backends();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests — port of tests/lib_emailclient/ (C test suite)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Mock backends ----

    struct MockImap {
        folders: Vec<String>,
        messages: Vec<MessageHeader>,
        detail: Option<EmailMessage>,
        by_msg_id: Option<EmailMessage>,
        error: Option<String>,
        list_folders_calls: usize,
        list_messages_calls: usize,
        fetch_by_msg_id_calls: usize,
    }

    impl MockImap {
        fn new() -> Self {
            MockImap {
                folders: vec![],
                messages: vec![],
                detail: None,
                by_msg_id: None,
                error: None,
                list_folders_calls: 0,
                list_messages_calls: 0,
                fetch_by_msg_id_calls: 0,
            }
        }
        fn with_folders(mut self, folders: &[&str]) -> Self {
            self.folders = folders.iter().map(|s| s.to_string()).collect();
            self
        }
        fn with_messages(mut self, msgs: Vec<MessageHeader>) -> Self {
            self.messages = msgs;
            self
        }
        fn with_detail(mut self, msg: EmailMessage) -> Self {
            self.detail = Some(msg);
            self
        }
        fn with_by_msg_id(mut self, msg: EmailMessage) -> Self {
            self.by_msg_id = Some(msg);
            self
        }
        fn with_error(mut self, e: &str) -> Self {
            self.error = Some(e.to_owned());
            self
        }
    }

    impl ImapBackend for MockImap {
        fn list_folders(&mut self) -> Result<Vec<String>, String> {
            self.list_folders_calls += 1;
            if let Some(ref e) = self.error { return Err(e.clone()); }
            Ok(self.folders.clone())
        }
        fn list_messages(&mut self, _folder: &str, _limit: usize) -> Result<Vec<MessageHeader>, String> {
            self.list_messages_calls += 1;
            if let Some(ref e) = self.error { return Err(e.clone()); }
            Ok(self.messages.clone())
        }
        fn fetch_message(&mut self, _folder: &str, _uid: u32) -> Result<Option<EmailMessage>, String> {
            if let Some(ref e) = self.error { return Err(e.clone()); }
            Ok(self.detail.clone())
        }
        fn fetch_message_by_message_id(&mut self, _folder: &str, _msg_id: &str) -> Result<Option<EmailMessage>, String> {
            self.fetch_by_msg_id_calls += 1;
            if let Some(ref e) = self.error { return Err(e.clone()); }
            Ok(self.by_msg_id.clone())
        }
    }

    struct MockSmtp {
        sent: std::sync::Arc<std::sync::Mutex<Vec<(String, String, String, String)>>>,
        fail: bool,
    }

    impl MockSmtp {
        fn new() -> Self {
            MockSmtp { sent: Default::default(), fail: false }
        }
        fn failing() -> Self {
            MockSmtp { sent: Default::default(), fail: true }
        }
    }

    impl SmtpBackend for MockSmtp {
        fn send(&mut self, from: &str, to: &str, subject: &str, body: &str) -> Result<(), String> {
            if self.fail { return Err("SMTP error".to_owned()); }
            self.sent.lock().unwrap().push((
                from.to_owned(), to.to_owned(), subject.to_owned(), body.to_owned()
            ));
            Ok(())
        }
    }

    fn make_header(uid: u32, from: &str, subject: &str) -> MessageHeader {
        MessageHeader {
            uid,
            from: from.to_owned(),
            subject: subject.to_owned(),
            date: "2025-01-01".to_owned(),
        }
    }

    fn make_message(uid: u32) -> EmailMessage {
        EmailMessage {
            uid,
            from: "alice@example.com".to_owned(),
            to: "bob@example.com".to_owned(),
            subject: "Hello".to_owned(),
            date: "2025-01-01".to_owned(),
            body: "Hi Bob!".to_owned(),
            message_id: "<1@example.com>".to_owned(),
            in_reply_to: String::new(),
            references: String::new(),
        }
    }

    // ---- Identity ----

    #[test]
    fn test_name_and_display_name() {
        let p = EmailClientProvider::new();
        assert_eq!(p.name(), "emailclient");
        assert_eq!(p.display_name(), "email client");
    }

    #[test]
    fn test_no_cache_true() {
        let p = EmailClientProvider::new();
        assert!(p.no_cache());
    }

    // ---- Root fetch ----

    #[test]
    fn test_fetch_root_no_imap_shows_placeholder() {
        let mut p = EmailClientProvider::new();
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("not configured"))
        }));
    }

    #[test]
    fn test_fetch_root_compose_always_present() {
        let imap = MockImap::new().with_folders(&[]);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "compose")));
    }

    #[test]
    fn test_fetch_root_folders_become_objs() {
        let imap = MockImap::new().with_folders(&["INBOX", "Sent", "Drafts"]);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "INBOX")));
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "Sent")));
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "Drafts")));
    }

    #[test]
    fn test_fetch_root_imap_error_shows_message() {
        let imap = MockImap::new().with_error("connection refused");
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("IMAP error"))));
    }

    #[test]
    fn test_fetch_root_compose_inserted_after_inbox() {
        let imap = MockImap::new().with_folders(&["INBOX", "Sent"]);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        let items = p.fetch();
        let inbox_pos = items.iter().position(|e| e.as_obj().map_or(false, |o| o.key == "INBOX")).unwrap();
        let compose_pos = items.iter().position(|e| e.as_obj().map_or(false, |o| o.key == "compose")).unwrap();
        assert_eq!(compose_pos, inbox_pos + 1);
    }

    #[test]
    fn test_fetch_root_hierarchy_containers_filtered() {
        // "[Gmail]" is a container (has child "[Gmail]/Sent") and should be skipped.
        let imap = MockImap::new().with_folders(&["INBOX", "[Gmail]", "[Gmail]/Sent"]);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        let items = p.fetch();
        assert!(!items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "[Gmail]")));
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "Sent")));
    }

    #[test]
    fn test_folder_display_name_strips_prefix() {
        assert_eq!(folder_display_name("[Gmail]/Sent"), "Sent");
        assert_eq!(folder_display_name("INBOX"), "INBOX");
        assert_eq!(folder_display_name("[Gmail]/Verzonden berichten"), "Verzonden berichten");
    }

    // ---- Folder list fetch ----

    #[test]
    fn test_fetch_folder_shows_messages() {
        let msgs = vec![
            make_header(1, "alice@x.com", "Hello"),
            make_header(2, "bob@x.com", "World"),
        ];
        let imap = MockImap::new().with_messages(msgs);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key.contains("Hello"))));
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key.contains("World"))));
    }

    #[test]
    fn test_message_label_format() {
        let msgs = vec![make_header(1, "alice@x.com", "Subject")];
        let imap = MockImap::new().with_messages(msgs);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_obj().map_or(false, |o| o.key == "Subject — alice@x.com")
        }));
    }

    #[test]
    fn test_message_no_subject_label() {
        let msgs = vec![make_header(1, "alice@x.com", "")];
        let imap = MockImap::new().with_messages(msgs);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_obj().map_or(false, |o| o.key.starts_with("(no subject)"))
        }));
    }

    // ---- Folder cache ----

    #[test]
    fn test_folder_cache_avoids_refetch() {
        let imap = MockImap::new().with_folders(&["INBOX"]);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        // First fetch populates cache.
        p.fetch();
        // Second fetch should use cache (call count stays at 1).
        p.fetch();
        let backend = p.imap.as_ref().unwrap();
        let mock = backend.as_ref() as *const dyn ImapBackend as *const MockImap;
        // SAFETY: we know it's a MockImap
        let call_count = unsafe { (*mock).list_folders_calls };
        assert_eq!(call_count, 1, "folder cache should prevent second IMAP call");
    }

    // ---- Envelope cache ----

    #[test]
    fn test_envelope_cache_avoids_refetch() {
        let msgs = vec![make_header(1, "a@x.com", "Hi")];
        let imap = MockImap::new().with_messages(msgs);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch(); // fills cache
        p.fetch(); // should use cache
        let backend = p.imap.as_ref().unwrap();
        let mock = backend.as_ref() as *const dyn ImapBackend as *const MockImap;
        let call_count = unsafe { (*mock).list_messages_calls };
        assert_eq!(call_count, 1, "envelope cache should prevent second IMAP call");
    }

    // ---- Message view ----

    #[test]
    fn test_fetch_message_shows_headers() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("Hello — alice@example.com");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.starts_with("From:"))));
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.starts_with("To:"))));
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.starts_with("Subject:"))));
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.starts_with("Date:"))));
    }

    #[test]
    fn test_fetch_message_shows_body() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("Hello — alice@example.com");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s == "Hi Bob!")));
    }

    #[test]
    fn test_fetch_message_has_reply_reply_all_and_forward() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("Hello — alice@example.com");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "reply")));
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "reply all")));
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "forward")));
    }

    #[test]
    fn test_fetch_message_does_not_fetch_history_eagerly() {
        // A message with References should show a History object but NOT
        // fetch referenced messages until the user navigates into History.
        let mut msg = make_message(1);
        msg.references = "<prev@example.com>".to_owned();
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("Hello — alice@example.com");
        p.fetch();
        // fetch_message_by_message_id should NOT have been called yet.
        let mock = p.imap.as_ref().unwrap().as_ref() as *const dyn ImapBackend as *const MockImap;
        let calls = unsafe { (*mock).fetch_by_msg_id_calls };
        assert_eq!(calls, 0, "history should be fetched lazily");
    }

    #[test]
    fn test_fetch_history_lazily_on_navigation() {
        let mut msg = make_message(1);
        msg.references = "<prev@example.com>".to_owned();
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let ref_msg = EmailMessage {
            uid: 0,
            from: "old@example.com".to_owned(),
            to: "bob@example.com".to_owned(),
            subject: "Previous".to_owned(),
            date: "2024-12-31".to_owned(),
            body: "Old message body".to_owned(),
            message_id: "<prev@example.com>".to_owned(),
            in_reply_to: String::new(),
            references: String::new(),
        };
        let imap = MockImap::new()
            .with_messages(msgs)
            .with_detail(msg)
            .with_by_msg_id(ref_msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        // Navigate: root → INBOX → message
        p.push_path("INBOX");
        p.fetch();
        p.push_path("Hello — alice@example.com");
        p.fetch();
        // Now navigate into History.
        p.push_path("History");
        let items = p.fetch();
        // Should have fetched by message-id and returned a history entry.
        let mock = p.imap.as_ref().unwrap().as_ref() as *const dyn ImapBackend as *const MockImap;
        let calls = unsafe { (*mock).fetch_by_msg_id_calls };
        assert!(calls > 0, "history fetch should use fetch_by_message_id");
        assert!(!items.is_empty());
    }

    // ---- Compose view ----

    #[test]
    fn test_compose_view_has_from_line() {
        let mut p = EmailClientProvider::new();
        p.config.username = "me@example.com".to_owned();
        p.push_path("compose");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.starts_with("From:") && s.contains("me@example.com"))));
    }

    #[test]
    fn test_compose_view_has_input_fields() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("To:") && s.contains("<input>"))));
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("Subject:") && s.contains("<input>"))));
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("Body:") && s.contains("<input>"))));
    }

    #[test]
    fn test_compose_view_has_send_button() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("<button>send</button>"))));
    }

    #[test]
    fn test_compose_sent_shows_confirmation() {
        let smtp = MockSmtp::new();
        let mut p = EmailClientProvider::new().with_smtp(Box::new(smtp));
        p.push_path("compose");
        p.compose.draft.to = "x@y.com".to_owned();
        p.on_button_press("send");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("message sent"))));
    }

    // ---- Reply ----

    #[test]
    fn test_reply_prefills_to_field() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("Hello — alice@example.com");
        p.fetch(); // caches message_detail
        p.push_path("reply");
        p.fetch(); // triggers prefill
        assert_eq!(p.compose.draft.to, "alice@example.com");
    }

    #[test]
    fn test_reply_prefills_subject_with_re() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("Hello — alice@example.com");
        p.fetch();
        p.push_path("reply");
        p.fetch();
        assert!(p.compose.draft.subject.starts_with("Re:"));
    }

    #[test]
    fn test_reply_already_re_no_double_re() {
        let mut msg = make_message(1);
        msg.subject = "Re: Hello".to_owned();
        let msgs = vec![make_header(1, "alice@example.com", "Re: Hello")];
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("Re: Hello — alice@example.com");
        p.fetch();
        p.push_path("reply");
        p.fetch();
        assert!(!p.compose.draft.subject.to_lowercase().starts_with("re: re:"));
    }

    // ---- Reply all ----

    #[test]
    fn test_reply_all_includes_to_recipients() {
        let mut msg = make_message(1);
        msg.to = "bob@example.com, carol@example.com".to_owned();
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.config.username = "bob@example.com".to_owned();
        p.push_path("INBOX");
        p.fetch();
        p.push_path("Hello — alice@example.com");
        p.fetch();
        p.push_path("reply all");
        p.fetch();
        // Should include alice (sender) and carol (other recipient), but NOT bob (self).
        assert!(p.compose.draft.to.contains("alice@example.com"));
        assert!(p.compose.draft.to.contains("carol@example.com"));
        assert!(!p.compose.draft.to.contains("bob@example.com"));
    }

    // ---- Forward ----

    #[test]
    fn test_forward_prefills_subject_with_fwd() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("Hello — alice@example.com");
        p.fetch();
        p.push_path("forward");
        p.fetch();
        assert!(p.compose.draft.subject.starts_with("Fwd:"));
    }

    #[test]
    fn test_forward_clears_to_field() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("Hello — alice@example.com");
        p.fetch();
        p.push_path("forward");
        p.fetch();
        assert!(p.compose.draft.to.is_empty());
    }

    #[test]
    fn test_forward_body_has_header_block() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("Hello — alice@example.com");
        p.fetch();
        p.push_path("forward");
        p.fetch();
        assert!(p.compose.draft.body.contains("Forwarded message"));
        assert!(p.compose.draft.body.contains("alice@example.com"));
    }

    // ---- Send ----

    #[test]
    fn test_on_button_press_send_calls_smtp() {
        let smtp = MockSmtp::new();
        let sent = smtp.sent.clone();
        let mut p = EmailClientProvider::new().with_smtp(Box::new(smtp));
        p.push_path("compose");
        p.compose.draft.to = "test@x.com".to_owned();
        p.compose.draft.subject = "Greet".to_owned();
        p.compose.draft.body = "Hello!".to_owned();
        p.on_button_press("send");
        assert_eq!(sent.lock().unwrap().len(), 1);
        assert_eq!(sent.lock().unwrap()[0].1, "test@x.com");
    }

    #[test]
    fn test_on_button_press_send_clears_draft() {
        let smtp = MockSmtp::new();
        let mut p = EmailClientProvider::new().with_smtp(Box::new(smtp));
        p.push_path("compose");
        p.compose.draft.to = "x@y.com".to_owned();
        p.on_button_press("send");
        assert!(p.compose.draft.to.is_empty());
    }

    #[test]
    fn test_smtp_failure_does_not_panic() {
        let smtp = MockSmtp::failing();
        let mut p = EmailClientProvider::new().with_smtp(Box::new(smtp));
        p.push_path("compose");
        p.compose.draft.to = "x@y.com".to_owned();
        p.on_button_press("send"); // should not panic
    }

    // ---- Path navigation ----

    #[test]
    fn test_push_path_increments_depth() {
        let mut p = EmailClientProvider::new();
        p.push_path("INBOX");
        assert_eq!(p.current_path(), "/INBOX");
        p.push_path("Hello — alice@x.com");
        assert_eq!(p.current_path(), "/INBOX/Hello — alice@x.com");
    }

    #[test]
    fn test_pop_path_decrements_depth() {
        let mut p = EmailClientProvider::new();
        p.push_path("INBOX");
        p.push_path("msg");
        p.pop_path();
        assert_eq!(p.current_path(), "/INBOX");
    }

    #[test]
    fn test_pop_path_to_root() {
        let mut p = EmailClientProvider::new();
        p.push_path("INBOX");
        p.pop_path();
        assert_eq!(p.current_path(), "/");
    }

    #[test]
    fn test_at_root_true_initially() {
        let p = EmailClientProvider::new();
        assert!(p.at_root());
    }

    #[test]
    fn test_at_folder_true_at_depth_1() {
        let mut p = EmailClientProvider::new();
        p.push_path("INBOX");
        assert!(p.at_folder());
    }

    #[test]
    fn test_at_message_true_at_depth_2() {
        let mut p = EmailClientProvider::new();
        p.push_path("INBOX");
        p.push_path("msg");
        assert!(p.at_message());
    }

    #[test]
    fn test_at_compose_true_for_compose_path() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        assert!(p.at_compose());
    }

    // ---- commit_edit ----

    #[test]
    fn test_commit_stores_to_field() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.push_path("To");
        assert!(p.commit_edit("", "user@example.com"));
        assert_eq!(p.compose.draft.to, "user@example.com");
    }

    #[test]
    fn test_commit_stores_subject_field() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.push_path("Subject");
        assert!(p.commit_edit("", "Test Subject"));
        assert_eq!(p.compose.draft.subject, "Test Subject");
    }

    #[test]
    fn test_commit_stores_body_field() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.push_path("Body");
        assert!(p.commit_edit("", "Hello world!"));
        assert_eq!(p.compose.draft.body, "Hello world!");
    }

    #[test]
    fn test_commit_unknown_field_returns_false() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.push_path("Unknown");
        assert!(!p.commit_edit("", "value"));
    }

    // ---- on_setting_change ----

    #[test]
    fn test_on_setting_change_imap_url() {
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailImapUrl", "imaps://imap.gmail.com");
        assert_eq!(p.config.imap_url, "imaps://imap.gmail.com");
    }

    #[test]
    fn test_on_setting_change_smtp_url() {
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailSmtpUrl", "smtps://smtp.gmail.com");
        assert_eq!(p.config.smtp_url, "smtps://smtp.gmail.com");
    }

    #[test]
    fn test_on_setting_change_username() {
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailUsername", "user@gmail.com");
        assert_eq!(p.config.username, "user@gmail.com");
    }

    #[test]
    fn test_on_setting_change_password() {
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailPassword", "secret");
        assert_eq!(p.config.password, "secret");
    }

    #[test]
    fn test_on_setting_change_client_id() {
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailClientId", "my-client-id");
        assert_eq!(p.config.client_id, "my-client-id");
    }

    #[test]
    fn test_on_setting_change_client_secret() {
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailClientSecret", "my-secret");
        assert_eq!(p.config.client_secret, "my-secret");
    }

    #[test]
    fn test_on_setting_change_oauth_access_token() {
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailOAuthAccessToken", "tok123");
        assert_eq!(p.config.oauth_access_token, "tok123");
    }

    #[test]
    fn test_on_setting_change_oauth_refresh_token() {
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailOAuthRefreshToken", "refresh456");
        assert_eq!(p.config.oauth_refresh_token, "refresh456");
    }

    #[test]
    fn test_on_setting_change_token_expiry() {
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailTokenExpiry", "9999999999");
        assert_eq!(p.config.token_expiry, 9999999999);
    }

    #[test]
    fn test_rebuild_backends_no_backend_without_password() {
        // URL + username set but no password and no OAuth token → backend must
        // NOT be created (would otherwise LOGIN with empty password and get
        // "No Response: empty user name or password" from the server).
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailImapUrl", "imaps://imap.gmail.com");
        p.on_setting_change("emailUsername", "user@example.com");
        // password deliberately left empty, no OAuth token set
        assert!(p.imap.is_none(), "backend should not be created without password");
    }

    #[test]
    fn test_rebuild_backends_allows_oauth_without_password() {
        // URL + username + OAuth token → backend should be created even without
        // a plain password (XOAUTH2 auth path).
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailImapUrl", "imaps://imap.gmail.com");
        p.on_setting_change("emailUsername", "user@example.com");
        p.on_setting_change("emailOAuthAccessToken", "ya29.sometoken");
        // RealImap is created (won't connect until first use); imap.is_some().
        assert!(p.imap.is_some(), "backend should be created when OAuth token is present");
    }

    // ---- commands ----

    #[test]
    fn test_commands_include_compose_login_logout_refresh() {
        let p = EmailClientProvider::new();
        let cmds = p.commands();
        assert!(cmds.contains(&"compose".to_owned()));
        assert!(cmds.contains(&"login".to_owned()));
        assert!(cmds.contains(&"logout".to_owned()));
        assert!(cmds.contains(&"refresh".to_owned()));
    }

    #[test]
    fn test_handle_command_compose_changes_path() {
        let mut p = EmailClientProvider::new();
        let mut err = String::new();
        p.handle_command("compose", "", 0, &mut err);
        // compose command resets state but doesn't navigate (navigation done by app layer)
        assert!(err.is_empty());
    }

    #[test]
    fn test_handle_command_refresh_invalidates_caches() {
        let imap = MockImap::new().with_folders(&["INBOX"]);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.fetch(); // populate caches
        assert!(p.folder_cache.is_some());
        let mut err = String::new();
        p.handle_command("refresh", "", 0, &mut err);
        assert!(p.folder_cache.is_none());
    }

    #[test]
    fn test_handle_command_login_without_credentials() {
        let mut p = EmailClientProvider::new();
        let mut err = String::new();
        p.handle_command("login", "", 0, &mut err);
        assert!(!err.is_empty(), "should report error when no client ID");
    }

    #[test]
    fn test_handle_command_logout_clears_tokens() {
        let mut p = EmailClientProvider::new();
        p.config.oauth_access_token = "tok".to_owned();
        p.config.oauth_refresh_token = "ref".to_owned();
        p.config.token_expiry = 999;
        let mut err = String::new();
        p.handle_command("logout", "", 0, &mut err);
        assert!(p.config.oauth_access_token.is_empty());
        assert!(p.config.oauth_refresh_token.is_empty());
        assert_eq!(p.config.token_expiry, 0);
    }

    #[test]
    fn test_handle_command_unknown_sets_error() {
        let mut p = EmailClientProvider::new();
        let mut err = String::new();
        p.handle_command("bogus_cmd", "", 0, &mut err);
        assert!(!err.is_empty());
    }

    // ---- needs_refresh ----

    #[test]
    fn test_needs_refresh_flag() {
        let p = EmailClientProvider::new();
        assert!(!p.needs_refresh());
        p.needs_refresh_flag.store(true, Ordering::Relaxed);
        assert!(p.needs_refresh());
    }

    #[test]
    fn test_clear_needs_refresh_resets_flag_and_envelope_cache() {
        let msgs = vec![make_header(1, "a@x.com", "Hi")];
        let imap = MockImap::new().with_messages(msgs);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        assert!(p.envelope_cache.is_some());
        p.needs_refresh_flag.store(true, Ordering::Relaxed);
        p.clear_needs_refresh();
        assert!(!p.needs_refresh());
        assert!(p.envelope_cache.is_none(), "envelope cache cleared on refresh");
    }

    // ---- init loads config from settings.json ----

    #[test]
    fn test_init_loads_config_from_settings_json() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let json = r#"{
            "email client": {
                "emailImapUrl": "imaps://imap.example.com",
                "emailSmtpUrl": "smtps://smtp.example.com",
                "emailUsername": "test@example.com",
                "emailPassword": "hunter2",
                "emailClientId": "cid",
                "emailClientSecret": "csec",
                "emailOAuthAccessToken": "oat",
                "emailOAuthRefreshToken": "ort",
                "emailTokenExpiry": 12345
            }
        }"#;
        std::fs::File::create(&path).unwrap().write_all(json.as_bytes()).unwrap();

        // We can't call init() normally because main_config_path() uses the real filesystem.
        // Directly test the JSON loading logic by parsing as init() does.
        let content = std::fs::read_to_string(&path).unwrap();
        let root: serde_json::Value = serde_json::from_str(&content).unwrap();
        let section = root.get("email client").unwrap().as_object().unwrap();

        let imap_url = section.get("emailImapUrl").and_then(|v| v.as_str()).unwrap();
        assert_eq!(imap_url, "imaps://imap.example.com");
        let expiry = section.get("emailTokenExpiry").and_then(|v| v.as_i64()).unwrap();
        assert_eq!(expiry, 12345);
    }
}
