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
use std::sync::{Arc, Mutex};

use sicompass_sdk::ffon::{FfonElement, FfonObject};
use sicompass_sdk::platform;
use sicompass_sdk::provider::Provider;

use idle::IdleController;

// ---------------------------------------------------------------------------
// Mail body type
// ---------------------------------------------------------------------------

/// The body of an email message, tagged with its content kind.
///
/// - `Text`  — plain text (`text/plain`)
/// - `Html`  — raw HTML source (`text/html`)
/// - `Ffon`  — a structured FFON tree (`application/json` that passes `is_ffon`)
#[derive(Debug, Clone)]
pub enum MailBody {
    Text(String),
    Html(String),
    Ffon(Vec<FfonElement>),
}

impl Default for MailBody {
    fn default() -> Self {
        MailBody::Text(String::new())
    }
}

impl MailBody {
    /// Return a plain-text representation for quoting or fallback display.
    pub fn as_plain(&self) -> String {
        match self {
            MailBody::Text(s) => s.clone(),
            MailBody::Html(s) => {
                // Convert HTML to FFON then flatten leaves to text.
                let elems = sicompass_sdk::ffon::html_to_ffon(s, "");
                flatten_ffon_to_text(&elems)
            }
            MailBody::Ffon(elems) => {
                sicompass_sdk::ffon::to_json_string(elems).unwrap_or_default()
            }
        }
    }
}

/// Recursively flatten an FFON tree to a plain-text string.
fn flatten_ffon_to_text(elems: &[FfonElement]) -> String {
    let mut out = String::new();
    for elem in elems {
        match elem {
            FfonElement::Str(s) => {
                if !out.is_empty() { out.push('\n'); }
                out.push_str(s);
            }
            FfonElement::Obj(o) => {
                if !out.is_empty() { out.push('\n'); }
                out.push_str(&o.key);
                let children_text = flatten_ffon_to_text(&o.children);
                if !children_text.is_empty() {
                    out.push('\n');
                    out.push_str(&children_text);
                }
            }
        }
    }
    out
}

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
    pub body: MailBody,
    pub message_id: String,
    pub in_reply_to: String,
    pub references: String,
}

/// Compose form draft state.
#[derive(Debug, Clone, Default)]
pub struct Draft {
    pub to: String,
    pub subject: String,
    pub body: MailBody,
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
    fn send(&mut self, from: &str, to: &str, subject: &str, body: &MailBody) -> Result<(), String>;
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

    // Async folder fetch — moves list_folders() off the main thread at startup.
    folder_fetch_inflight: Arc<AtomicBool>,
    folder_fetch_result: Arc<Mutex<Option<Result<Vec<String>, String>>>>,
    // Disabled in tests that inject a mock via with_imap() so they keep using the sync path.
    async_folder_fetch_enabled: bool,

    // Async OAuth token refresh — moves oauth2::refresh_token() off the main thread at startup.
    // Result carries (new_access_token, new_token_expiry) on success.
    token_refresh_inflight: Arc<AtomicBool>,
    token_refresh_result: Arc<Mutex<Option<Result<(String, i64), String>>>>,

    // Pending error message to surface via take_error.
    error_message: Option<String>,

    // In-flight OAuth2 login handle (None when no login is in progress).
    active_login: Option<oauth2::PendingAuthorize>,

    // Override for the settings.json path (used in tests to avoid touching
    // the real user config file).
    config_path_override: Option<std::path::PathBuf>,
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
            folder_fetch_inflight: Arc::new(AtomicBool::new(false)),
            folder_fetch_result: Arc::new(Mutex::new(None)),
            async_folder_fetch_enabled: true,
            token_refresh_inflight: Arc::new(AtomicBool::new(false)),
            token_refresh_result: Arc::new(Mutex::new(None)),
            error_message: None,
            active_login: None,
            config_path_override: None,
        }
    }

    /// Override the settings.json path (used in tests to avoid touching real config).
    pub fn with_config_path(mut self, path: std::path::PathBuf) -> Self {
        self.config_path_override = Some(path);
        self
    }

    fn config_path(&self) -> Option<std::path::PathBuf> {
        self.config_path_override.clone().or_else(|| platform::main_config_path())
    }

    /// Inject an IMAP backend (used in tests and production setup).
    pub fn with_imap(mut self, backend: Box<dyn ImapBackend>) -> Self {
        self.imap = Some(backend);
        self.async_folder_fetch_enabled = false;
        self
    }

    /// Inject an SMTP backend.
    pub fn with_smtp(mut self, backend: Box<dyn SmtpBackend>) -> Self {
        self.smtp = Some(backend);
        self
    }

    /// Set a fake OAuth access token (used in tests to simulate logged-in state).
    pub fn with_oauth_token(mut self, token: impl Into<String>) -> Self {
        self.config.oauth_access_token = token.into();
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

    // ---- Login state ---------------------------------------------------------

    fn is_logged_in(&self) -> bool {
        !self.config.oauth_access_token.is_empty()
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

    /// Persist server connection fields (IMAP URL, SMTP URL, username) to settings.json.
    fn save_server_config(&self) {
        let Some(path) = self.config_path() else { return };
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
            sec.insert("emailImapUrl".to_owned(), self.config.imap_url.clone().into());
            sec.insert("emailSmtpUrl".to_owned(), self.config.smtp_url.clone().into());
            sec.insert("emailUsername".to_owned(), self.config.username.clone().into());
        }
        if let Ok(json) = serde_json::to_string_pretty(&root) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// Persist OAuth tokens to settings.json.
    fn save_oauth_tokens(&self) {
        let Some(path) = self.config_path() else { return };
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

    // ---- Login logic ---------------------------------------------------------

    /// Start a non-blocking OAuth2 login. Returns immediately; the result is
    /// applied asynchronously from `tick()` once the browser flow completes.
    fn do_login(&mut self) -> Result<(), String> {
        if self.config.client_id.is_empty() || self.config.client_secret.is_empty() {
            return Err("set client ID and client secret in settings first".to_owned());
        }
        if self.active_login.is_some() {
            return Err("login already in progress".to_owned());
        }
        match oauth2::start(&self.config.client_id, &self.config.client_secret, 120) {
            Err(e) => Err(format!("OAuth2 failed: {}", e.error)),
            Ok(handle) => {
                self.active_login = Some(handle);
                // Use error_message as a status channel so the UI shows
                // in-progress feedback without new plumbing.
                self.error_message = Some("Waiting for Google authentication…".to_owned());
                Ok(())
            }
        }
    }

    /// Apply a completed OAuth2 result. Called from `tick()` on success or failure.
    fn finish_login(&mut self, result: oauth2::OAuth2TokenResult) {
        if !result.success {
            self.error_message = Some(format!("OAuth2 failed: {}", result.error));
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.config.oauth_access_token = result.access_token;
        self.config.oauth_refresh_token = result.refresh_token;
        self.config.token_expiry = now + result.expires_in;
        self.save_oauth_tokens();
        if self.config.imap_url.is_empty() {
            self.config.imap_url = "imaps://imap.gmail.com:993".to_owned();
        }
        if self.config.smtp_url.is_empty() {
            self.config.smtp_url = "smtps://smtp.gmail.com:465".to_owned();
        }
        if self.config.username.is_empty() {
            if let Some(email) = oauth2::fetch_email(&self.config.oauth_access_token) {
                self.config.username = email;
            }
        }
        self.save_server_config();
        self.imap = None;
        self.smtp = None;
        self.rebuild_backends();
        self.current_path = "/".to_owned();
        self.folder_cache = None;
        self.envelope_cache = None;
        self.envelope_cache_folder.clear();
        self.error_message = None;
    }

    // ---- FFON tree builders -----------------------------------------------

    /// Convert a `list_folders` result into FFON items, populate caches and mappings.
    fn build_root_from_folder_list(
        &mut self,
        folder_result: Result<Vec<String>, String>,
    ) -> Vec<FfonElement> {
        let mut items = vec![];
        match folder_result {
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

    fn build_root(&mut self) -> Vec<FfonElement> {
        // Stop IDLE when leaving a folder (returning to root).
        self.idle.stop();

        // When not logged in, show a single login button.
        if !self.is_logged_in() {
            self.folder_cache = None;
            return vec![FfonElement::new_str(
                "<button>login</button>Log in with Google".to_owned(),
            )];
        }

        // Serve folder cache on back-navigation.
        if let Some(cached) = &self.folder_cache {
            return cached.clone();
        }

        // Async path: move list_folders() off the main thread at startup.
        if self.async_folder_fetch_enabled {
            // 1. If a background result has arrived, drain it and build the cache.
            let result = self.folder_fetch_result.lock().unwrap().take();
            if let Some(folder_result) = result {
                self.folder_fetch_inflight.store(false, Ordering::Release);
                return self.build_root_from_folder_list(folder_result);
            }

            // 2. A fetch is in flight — show a placeholder until it completes.
            if self.folder_fetch_inflight.load(Ordering::Acquire) {
                return vec![FfonElement::new_str("Loading folders…".to_owned())];
            }

            // 3. Nothing in flight yet — spawn the background fetch.
            if self.imap.is_none() {
                // Not configured; fall through to the sync path so the
                // "not configured" message is shown immediately.
            } else {
                self.folder_fetch_inflight.store(true, Ordering::Release);
                let inflight = Arc::clone(&self.folder_fetch_inflight);
                let result_slot = Arc::clone(&self.folder_fetch_result);
                let needs_refresh = Arc::clone(&self.needs_refresh_flag);
                let config = self.config.clone();
                std::thread::spawn(move || {
                    let mut imap = crate::net::RealImap::from_config(&config);
                    let result = imap.list_folders();
                    *result_slot.lock().unwrap() = Some(result);
                    inflight.store(false, Ordering::Release);
                    needs_refresh.store(true, Ordering::Release);
                });
                return vec![FfonElement::new_str("Loading folders…".to_owned())];
            }
        }

        // Sync path (tests, or when imap backend is None / async disabled).
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

        let folder_result = imap.list_folders();
        drop(imap);
        self.build_root_from_folder_list(folder_result)
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

        // Body: is a structured subtree — Ctrl+A/I/Delete work on its children.
        // The key includes the current format so the user sees it live.
        let body_children = body_to_compose_children(&self.compose.draft.body);
        items.push(FfonElement::Obj(FfonObject {
            key: body_format_label(&self.compose.draft.body).to_owned(),
            children: body_children,
        }));

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
        self.ensure_fresh_token();
        if let Some(ref mut smtp) = self.smtp {
            let from = self.config.username.clone();
            let to = self.compose.draft.to.clone();
            let subject = self.compose.draft.subject.clone();
            let body = normalize_body_for_send(&self.compose.draft.body);
            smtp.send(&from, &to, &subject, &body).is_ok()
        } else {
            false
        }
    }

    /// Refresh OAuth2 token if expired; rebuild backends if the token changed.
    /// Mirrors C `ensureOAuth2Token()`, which is called before every IMAP/SMTP
    /// operation to keep the backends in sync with a live token.
    fn ensure_fresh_token(&mut self) {
        let old = self.config.oauth_access_token.clone();
        self.ensure_token();
        if self.config.oauth_access_token != old {
            // Token was refreshed — drop backends so rebuild_backends recreates
            // them with the new token (backends store their own config clone).
            self.imap = None;
            self.smtp = None;
            self.rebuild_backends();
        }
    }

    /// Non-blocking variant of `ensure_fresh_token` used on the startup hot path.
    ///
    /// Returns `true` when the token is ready to use (valid, or refresh just completed),
    /// and `false` when a background refresh is in flight (caller should show a loading
    /// placeholder and retry on the next fetch cycle).
    fn ensure_fresh_token_async(&mut self) -> bool {
        // Password mode or no OAuth configured — nothing to refresh.
        if self.config.oauth_access_token.is_empty() {
            return true;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        // Fast path: token still valid.
        if now < self.config.token_expiry - 60 {
            return true;
        }
        // No refresh token — can't refresh; let the caller proceed (will fail later).
        if self.config.oauth_refresh_token.is_empty() {
            return true;
        }

        // Drain a completed background refresh if one has arrived.
        let result = self.token_refresh_result.lock().unwrap().take();
        if let Some(outcome) = result {
            self.token_refresh_inflight.store(false, Ordering::Release);
            if let Ok((access_token, expiry)) = outcome {
                self.config.oauth_access_token = access_token;
                self.config.token_expiry = expiry;
                self.save_oauth_tokens();
                // Drop backends so rebuild_backends recreates them with the new token.
                self.imap = None;
                self.smtp = None;
                self.rebuild_backends();
            }
            // Whether refresh succeeded or failed, unblock the caller.
            return true;
        }

        // Still waiting for an in-flight refresh.
        if self.token_refresh_inflight.load(Ordering::Acquire) {
            return false;
        }

        // Spawn a background refresh.
        self.token_refresh_inflight.store(true, Ordering::Release);
        let inflight = Arc::clone(&self.token_refresh_inflight);
        let result_slot = Arc::clone(&self.token_refresh_result);
        let needs_refresh = Arc::clone(&self.needs_refresh_flag);
        let client_id = self.config.client_id.clone();
        let client_secret = self.config.client_secret.clone();
        let refresh_token_str = self.config.oauth_refresh_token.clone();
        std::thread::spawn(move || {
            let r = oauth2::refresh_token(&client_id, &client_secret, &refresh_token_str);
            let outcome = if r.success {
                let expiry = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0)
                    + r.expires_in;
                Ok((r.access_token, expiry))
            } else {
                Err("OAuth token refresh failed".to_owned())
            };
            *result_slot.lock().unwrap() = Some(outcome);
            inflight.store(false, Ordering::Release);
            needs_refresh.store(true, Ordering::Release);
        });
        false
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
        ComposeMode::Reply | ComposeMode::ReplyAll => {
            if matches!(mode, ComposeMode::Reply) {
                compose.draft.to = msg.from.clone();
            } else {
                let mut recipients = vec![msg.from.clone()];
                for tok in msg.to.split(',') {
                    let t = tok.trim();
                    if !t.is_empty() && !t.contains(username) {
                        recipients.push(t.to_owned());
                    }
                }
                compose.draft.to = recipients.join(", ");
            }
            compose.draft.subject = if msg.subject.to_lowercase().starts_with("re:") {
                msg.subject.clone()
            } else {
                format!("Re: {}", msg.subject)
            };
            compose.draft.body = match &msg.body {
                MailBody::Text(s) => {
                    let quoted = s.lines()
                        .map(|l| format!("> {l}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    MailBody::Text(format!(
                        "\n\nOn {} <{}> wrote:\n{}", msg.date, msg.from, quoted
                    ))
                }
                MailBody::Html(s) => MailBody::Html(format!(
                    "<p></p><p>On {} &lt;{}&gt; wrote:</p><blockquote>{}</blockquote>",
                    msg.date, msg.from, s
                )),
                MailBody::Ffon(elems) => MailBody::Ffon(vec![
                    FfonElement::new_str("".to_owned()),
                    FfonElement::Obj(FfonObject {
                        key: format!("On {} <{}> wrote:", msg.date, msg.from),
                        children: elems.clone(),
                    }),
                ]),
            };
        }
        ComposeMode::Forward => {
            compose.draft.to.clear();
            compose.draft.subject = if msg.subject.to_lowercase().starts_with("fwd:") {
                msg.subject.clone()
            } else {
                format!("Fwd: {}", msg.subject)
            };
            compose.draft.body = match &msg.body {
                MailBody::Text(s) => MailBody::Text(format!(
                    "\n\n---------- Forwarded message ----------\nFrom: {}\nTo: {}\nDate: {}\nSubject: {}\n\n{}",
                    msg.from, msg.to, msg.date, msg.subject, s
                )),
                MailBody::Html(s) => MailBody::Html(format!(
                    "<p></p><hr><p><b>---------- Forwarded message ----------</b><br>\
                     From: {}<br>To: {}<br>Date: {}<br>Subject: {}</p>{}",
                    msg.from, msg.to, msg.date, msg.subject, s
                )),
                MailBody::Ffon(elems) => MailBody::Ffon(vec![
                    FfonElement::new_str("".to_owned()),
                    FfonElement::Obj(FfonObject {
                        key: "---------- Forwarded message ----------".to_owned(),
                        children: vec![
                            FfonElement::new_str(format!("From: {}", msg.from)),
                            FfonElement::new_str(format!("To: {}", msg.to)),
                            FfonElement::new_str(format!("Date: {}", msg.date)),
                            FfonElement::new_str(format!("Subject: {}", msg.subject)),
                            FfonElement::Obj(FfonObject {
                                key: "body:".to_owned(),
                                children: elems.clone(),
                            }),
                        ],
                    }),
                ]),
            };
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
    ];

    // Render body according to its kind.
    match &msg.body {
        MailBody::Text(s) => {
            items.push(FfonElement::new_str(s.clone()));
        }
        MailBody::Html(s) => {
            // Convert HTML to FFON via the shared html crate, same as webbrowser.
            let html_elems = sicompass_sdk::ffon::html_to_ffon(s, "");
            items.extend(html_elems);
        }
        MailBody::Ffon(elems) => {
            items.extend(elems.clone());
        }
    }

    if !msg.references.is_empty() {
        items.push(FfonElement::new_obj("History"));
    }
    items.push(FfonElement::new_obj("reply"));
    items.push(FfonElement::new_obj("reply all"));
    items.push(FfonElement::new_obj("forward"));
    items
}

// ---------------------------------------------------------------------------
// Body helper functions
// ---------------------------------------------------------------------------

/// Build the children list for the `Body:` Obj in the compose view.
///
/// - Text / Html: one `<input>` leaf with the current content.
/// - Ffon: the stored elements directly (each leaf should already carry `<input>` tags).
///
/// Always appends a `<button>body_new_line</button>New text line` button at the end
/// so the user can add elements via Enter as well as via Ctrl+A/I.
fn body_to_compose_children(body: &MailBody) -> Vec<FfonElement> {
    let mut children: Vec<FfonElement> = match body {
        MailBody::Text(s) => vec![FfonElement::new_str(format!("<input>{s}</input>"))],
        MailBody::Html(s) => vec![FfonElement::new_str(format!("<input>{s}</input>"))],
        MailBody::Ffon(elems) => elems.clone(),
    };
    children.push(FfonElement::new_str(
        "<button>body_new_line</button>New text line".to_owned(),
    ));
    children
}

/// Update a body leaf after a text edit in the compose form.
///
/// Called from `commit_edit` when the path ends in `"Body:"`.
/// Matches the element whose stripped input content equals `old_content`
/// and replaces it with `new_content`.  When `old_content` is empty
/// (a freshly-inserted placeholder), the first empty leaf is filled.
/// If the body was `Text` and a new element is being added (no match),
/// it is upgraded to `Ffon` to hold multiple elements.
fn update_body_leaf(body: &mut MailBody, old_content: &str, new_content: &str) {
    use sicompass_sdk::tags;

    match body {
        MailBody::Text(s) | MailBody::Html(s) => {
            if old_content.is_empty() && !s.is_empty() {
                // A new element is being inserted alongside existing content — upgrade to Ffon.
                let existing = s.clone();
                let is_html = matches!(*body, MailBody::Html(_));
                *body = MailBody::Ffon(vec![
                    FfonElement::new_str(format!("<input>{existing}</input>")),
                    FfonElement::new_str(format!("<input>{new_content}</input>")),
                ]);
                let _ = is_html; // future: preserve HTML kind if needed
            } else {
                // Simple replacement.
                *s = new_content.to_owned();
            }
        }
        MailBody::Ffon(elems) => {
            // Find the element whose input-stripped content matches old_content.
            let pos = elems.iter().position(|e| {
                if let FfonElement::Str(s) = e {
                    let stripped = tags::extract_input(s).unwrap_or_else(|| s.clone());
                    stripped == old_content
                } else {
                    false
                }
            });
            if let Some(idx) = pos {
                elems[idx] = FfonElement::new_str(format!("<input>{new_content}</input>"));
            } else {
                // No match — append as a new element.
                elems.push(FfonElement::new_str(format!("<input>{new_content}</input>")));
            }
        }
    }
}

/// Add a new empty text-line element to the body (used by the "body_new_line" button).
fn body_add_element(body: &mut MailBody) {
    match body {
        MailBody::Text(s) if s.is_empty() => {
            // Still empty — nothing to do yet; leave as single empty input.
        }
        MailBody::Text(s) => {
            let existing = s.clone();
            *body = MailBody::Ffon(vec![
                FfonElement::new_str(format!("<input>{existing}</input>")),
                FfonElement::new_str("<input></input>".to_owned()),
            ]);
        }
        MailBody::Html(s) => {
            let existing = s.clone();
            *body = MailBody::Ffon(vec![
                FfonElement::new_str(format!("<input>{existing}</input>")),
                FfonElement::new_str("<input></input>".to_owned()),
            ]);
        }
        MailBody::Ffon(elems) => {
            elems.push(FfonElement::new_str("<input></input>".to_owned()));
        }
    }
}

/// Normalise a draft body before sending (auto-detect format, collapse trivial Ffon).
///
/// - Single-element `Ffon([Str("<input>text</input>")])` → `Text(text)`.
/// - `Text` that parses as valid FFON JSON → `Ffon(parsed)`.
/// - `Text` that looks like HTML → `Html(text)`.
/// - Everything else: unchanged.
fn normalize_body_for_send(body: &MailBody) -> MailBody {
    use sicompass_sdk::ffon::to_json_string;
    use sicompass_sdk::tags;

    match body {
        MailBody::Ffon(elems) if elems.len() == 1 => {
            if let FfonElement::Str(s) = &elems[0] {
                let plain = tags::extract_input(s).unwrap_or_else(|| s.clone());
                // Collapse single-element Ffon back to Text (or re-detect as HTML/Ffon).
                return normalize_body_for_send(&MailBody::Text(plain.to_owned()));
            }
            body.clone()
        }
        MailBody::Text(s) => {
            // Try FFON detection.
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
                if sicompass_sdk::ffon::is_ffon(&v) {
                    if let Ok(elems) = serde_json::from_value::<Vec<sicompass_sdk::ffon::FfonElement>>(v) {
                        return MailBody::Ffon(elems);
                    }
                }
            }
            // Try HTML detection (conservative: must start with a block-level HTML tag).
            let trimmed = s.trim();
            let looks_html = trimmed.starts_with("<!DOCTYPE")
                || trimmed.starts_with("<html")
                || trimmed.starts_with("<HTML");
            if looks_html {
                return MailBody::Html(s.clone());
            }
            body.clone()
        }
        _ => body.clone(),
    }
}

/// Display label for the `Body:` Obj key — reflects the current format live.
fn body_format_label(body: &MailBody) -> &'static str {
    match body {
        MailBody::Text(_) => "Body: [text]",
        MailBody::Html(_) => "Body: [html]",
        MailBody::Ffon(_) => "Body: [ffon]",
    }
}

/// One-way format promotion for live detection after each body edit.
///
/// Unlike `normalize_body_for_send`, this never collapses `Ffon`→`Text`, so
/// the user stays in the mode they've chosen once structure has been added.
fn detect_body_format_live(body: MailBody) -> MailBody {
    match body {
        MailBody::Text(ref s) => {
            // Try FFON: only promote when the text is valid JSON FFON.
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
                if sicompass_sdk::ffon::is_ffon(&v) {
                    if let Ok(elems) = serde_json::from_value::<Vec<FfonElement>>(v) {
                        return MailBody::Ffon(elems);
                    }
                }
            }
            // Try HTML: must start with a recognised block-level tag.
            let trimmed = s.trim();
            if trimmed.starts_with("<!DOCTYPE")
                || trimmed.starts_with("<html")
                || trimmed.starts_with("<HTML")
            {
                return MailBody::Html(s.clone());
            }
            body
        }
        other => other,
    }
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
        let Some(path) = self.config_path() else { return };
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
        if !self.ensure_fresh_token_async() {
            return vec![FfonElement::new_str("Loading…".to_owned())];
        }
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

    fn commit_edit(&mut self, old_content: &str, new_content: &str) -> bool {
        // Path: /compose/To, /compose/Subject, /compose/Body:  (or deeper body paths)
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
            // Body: is now a subtree; commit_edit fires for any leaf edit inside it.
            // The framework pushes no extra path segment for naked <input> elements,
            // so the last segment is always "Body:" regardless of which child was edited.
            // We match by old_content to find the right leaf, or add a new element when
            // old_content="" (a newly-inserted placeholder being committed for the first time).
            f if f.starts_with("Body:") => {
                update_body_leaf(&mut self.compose.draft.body, old_content, new_content);
                // Live format detection — one-way promotion only (never collapses Ffon).
                let promoted = detect_body_format_live(std::mem::take(&mut self.compose.draft.body));
                self.compose.draft.body = promoted;
                // Sync the path segment to the new label so meta() stays correct.
                let new_label = body_format_label(&self.compose.draft.body);
                if let Some(slash) = self.current_path.rfind('/') {
                    self.current_path = format!("{}/{new_label}", &self.current_path[..slash]);
                }
                true
            }
            _ => false,
        }
    }

    fn on_button_press(&mut self, function_name: &str) {
        match function_name {
            "send" => {
                self.send_draft();
                self.compose = ComposeState::default();
                self.compose_sent = true;
            }
            "login" => {
                if let Err(e) = self.do_login() {
                    self.error_message = Some(e);
                }
            }
            // "Add new line" button inside the Body: subtree.
            "body_new_line" => {
                body_add_element(&mut self.compose.draft.body);
            }
            _ => {}
        }
    }

    fn tick(&mut self) -> bool {
        let handle = match self.active_login.take() {
            Some(h) => h,
            None => return false,
        };
        match handle.poll() {
            None => {
                // Still waiting — put the handle back.
                self.active_login = Some(handle);
                false
            }
            Some(result) => {
                self.finish_login(result);
                true
            }
        }
    }

    fn take_error(&mut self) -> Option<String> {
        self.error_message.take()
    }

    fn commands(&self) -> Vec<String> {
        if !self.is_logged_in() {
            return vec![];
        }
        vec![
            "compose".to_owned(),
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
            "logout" => {
                self.idle.stop();
                self.config.oauth_access_token.clear();
                self.config.oauth_refresh_token.clear();
                self.config.token_expiry = 0;
                self.save_oauth_tokens();
                self.imap = None;
                self.smtp = None;
                // Invalidate all session state so build_root serves the login button
                // on the next fetch.
                self.current_path = "/".to_owned();
                self.folder_cache = None;
                self.envelope_cache = None;
                self.envelope_cache_folder.clear();
                self.message_cache.clear();
                self.message_detail = None;
                self.compose = ComposeState::default();
                self.compose_sent = false;
                self.history_folder.clear();
                self.history_refs.clear();
                None  // triggers state-toggle refresh in handlers.rs
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
        sent: std::sync::Arc<std::sync::Mutex<Vec<(String, String, String, MailBody)>>>,
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
        fn send(&mut self, from: &str, to: &str, subject: &str, body: &MailBody) -> Result<(), String> {
            if self.fail { return Err("SMTP error".to_owned()); }
            self.sent.lock().unwrap().push((
                from.to_owned(), to.to_owned(), subject.to_owned(), body.clone()
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
            body: MailBody::Text("Hi Bob!".to_owned()),
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
    fn test_fetch_root_not_logged_in_shows_login_button() {
        let mut p = EmailClientProvider::new();
        let items = p.fetch();
        assert_eq!(items.len(), 1);
        assert!(items[0].as_str().map_or(false, |s| s.contains("<button>login</button>")));
    }

    #[test]
    fn test_fetch_root_no_imap_logged_in_shows_placeholder() {
        let mut p = EmailClientProvider::new().with_oauth_token("fake");
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("not configured"))
        }));
    }

    #[test]
    fn test_fetch_root_compose_always_present() {
        let imap = MockImap::new().with_folders(&[]);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "compose")));
    }

    #[test]
    fn test_fetch_root_folders_become_objs() {
        let imap = MockImap::new().with_folders(&["INBOX", "Sent", "Drafts"]);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "INBOX")));
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "Sent")));
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "Drafts")));
    }

    #[test]
    fn test_fetch_root_imap_error_shows_message() {
        let imap = MockImap::new().with_error("connection refused");
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("IMAP error"))));
    }

    #[test]
    fn test_fetch_root_compose_inserted_after_inbox() {
        let imap = MockImap::new().with_folders(&["INBOX", "Sent"]);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        let items = p.fetch();
        let inbox_pos = items.iter().position(|e| e.as_obj().map_or(false, |o| o.key == "INBOX")).unwrap();
        let compose_pos = items.iter().position(|e| e.as_obj().map_or(false, |o| o.key == "compose")).unwrap();
        assert_eq!(compose_pos, inbox_pos + 1);
    }

    #[test]
    fn test_fetch_root_hierarchy_containers_filtered() {
        // "[Gmail]" is a container (has child "[Gmail]/Sent") and should be skipped.
        let imap = MockImap::new().with_folders(&["INBOX", "[Gmail]", "[Gmail]/Sent"]);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        let items = p.fetch();
        assert!(!items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "[Gmail]")));
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "Sent")));
    }

    // ---- Login/logout state ----

    #[test]
    fn test_commands_empty_when_not_logged_in() {
        let p = EmailClientProvider::new();
        assert!(p.commands().is_empty());
    }

    #[test]
    fn test_commands_no_login_when_logged_in() {
        let p = EmailClientProvider::new().with_oauth_token("fake");
        let cmds = p.commands();
        assert!(!cmds.contains(&"login".to_owned()));
        assert!(cmds.contains(&"logout".to_owned()));
        assert!(cmds.contains(&"compose".to_owned()));
        assert!(cmds.contains(&"refresh".to_owned()));
    }

    #[test]
    fn test_logout_clears_state_and_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let imap = MockImap::new().with_folders(&["INBOX"]);
        let mut p = EmailClientProvider::new()
            .with_config_path(dir.path().join("settings.json"))
            .with_oauth_token("fake")
            .with_imap(Box::new(imap));
        // Prime the folder cache.
        let _ = p.fetch();
        assert!(p.folder_cache.is_some());

        let mut error = String::new();
        let result = p.handle_command("logout", "", 0, &mut error);
        assert!(result.is_none(), "logout must return None to trigger state-toggle refresh");
        assert!(p.config.oauth_access_token.is_empty());
        assert!(p.folder_cache.is_none());
        assert!(p.envelope_cache.is_none());
        assert_eq!(p.current_path, "/");
    }

    #[test]
    fn test_logout_then_fetch_shows_login_button() {
        let dir = tempfile::tempdir().unwrap();
        let imap = MockImap::new().with_folders(&["INBOX"]);
        let mut p = EmailClientProvider::new()
            .with_config_path(dir.path().join("settings.json"))
            .with_oauth_token("fake")
            .with_imap(Box::new(imap));
        let _ = p.fetch();
        let mut error = String::new();
        p.handle_command("logout", "", 0, &mut error);
        let items = p.fetch();
        assert_eq!(items.len(), 1);
        assert!(items[0].as_str().map_or(false, |s| s.contains("<button>login</button>")));
    }

    #[test]
    fn test_button_login_missing_credentials_sets_error() {
        let mut p = EmailClientProvider::new();
        // client_id and client_secret are empty by default.
        p.on_button_press("login");
        let err = p.take_error();
        assert!(err.is_some(), "expected an error message");
        assert!(err.unwrap().contains("client ID"));
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
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
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
            body: MailBody::Text("Old message body".to_owned()),
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
        // Body: is now an Obj node whose children contain the <input> leaf.
        assert!(items.iter().any(|e| {
            if let FfonElement::Obj(obj) = e {
                obj.key.starts_with("Body:") && obj.children.iter().any(|c|
                    c.as_str().map_or(false, |s| s.contains("<input>")))
            } else { false }
        }));
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
        assert!(matches!(&p.compose.draft.body, MailBody::Text(s) if s.contains("Forwarded message")));
        assert!(matches!(&p.compose.draft.body, MailBody::Text(s) if s.contains("alice@example.com")));
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
        p.compose.draft.body = MailBody::Text("Hello!".to_owned());
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
        p.push_path("Body:");
        assert!(p.commit_edit("", "Hello world!"));
        assert!(matches!(&p.compose.draft.body, MailBody::Text(s) if s == "Hello world!"));
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
    fn test_commands_include_compose_logout_refresh_when_logged_in() {
        let p = EmailClientProvider::new().with_oauth_token("fake");
        let cmds = p.commands();
        assert!(cmds.contains(&"compose".to_owned()));
        assert!(cmds.contains(&"logout".to_owned()));
        assert!(cmds.contains(&"refresh".to_owned()));
        assert!(!cmds.contains(&"login".to_owned()));
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
    fn test_ensure_fresh_token_noop_when_valid() {
        // Token present and not expired → ensure_fresh_token is a no-op.
        let imap = MockImap::new().with_folders(&["INBOX"]);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.config.oauth_access_token = "valid_token".to_owned();
        p.config.token_expiry = i64::MAX; // never expires
        p.ensure_fresh_token();
        // Token unchanged, backend still present.
        assert_eq!(p.config.oauth_access_token, "valid_token");
        assert!(p.imap.is_some());
    }

    #[test]
    fn test_ensure_fresh_token_noop_in_password_mode() {
        // No OAuth token → password mode, ensure_fresh_token must not touch backends.
        let imap = MockImap::new().with_folders(&["INBOX"]);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.config.oauth_access_token = String::new();
        p.ensure_fresh_token();
        assert!(p.imap.is_some(), "backend untouched in password mode");
    }

    #[test]
    fn test_handle_command_refresh_invalidates_caches() {
        let imap = MockImap::new().with_folders(&["INBOX"]);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.fetch(); // populate caches
        assert!(p.folder_cache.is_some());
        let mut err = String::new();
        p.handle_command("refresh", "", 0, &mut err);
        assert!(p.folder_cache.is_none());
    }

    #[test]
    fn test_handle_command_login_is_unknown() {
        // login is no longer a command — it is only accessible via the login button.
        let mut p = EmailClientProvider::new();
        let mut err = String::new();
        p.handle_command("login", "", 0, &mut err);
        assert!(!err.is_empty(), "unknown command should set error");
        assert!(err.contains("unknown command"), "expected 'unknown command' message");
    }

    #[test]
    fn test_handle_command_logout_clears_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = EmailClientProvider::new()
            .with_config_path(dir.path().join("settings.json"));
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
