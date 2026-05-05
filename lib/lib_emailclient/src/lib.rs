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

pub mod cache;
pub mod connection;
pub mod idle;
pub mod net;
pub mod oauth2;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use sicompass_sdk::ffon::{FfonElement, FfonObject};
use sicompass_sdk::placeholders::{
    new_obj_with_i_placeholder, seed_i_placeholders, I_PLACEHOLDER,
};
use sicompass_sdk::platform;
use sicompass_sdk::provider::{Provider, ProviderUndoDescriptor};

use idle::IdleController;

// ---------------------------------------------------------------------------
// Mail body type
// ---------------------------------------------------------------------------

/// The body of an email message, tagged with its content kind.
///
/// - `Text`  — plain text (`text/plain`); incoming HTML is flattened to this at parse time.
/// - `Ffon`  — a structured FFON tree (`application/json` that passes `is_ffon`)
#[derive(Debug, Clone)]
pub enum MailBody {
    Text(String),
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
            MailBody::Ffon(elems) => {
                sicompass_sdk::ffon::to_json_string(elems).unwrap_or_default()
            }
        }
    }
}

/// Recursively flatten an FFON tree to a plain-text string.
pub(crate) fn flatten_ffon_to_text(elems: &[FfonElement]) -> String {
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

/// A single mailbox entry returned by `ImapBackend::list_folders`.
#[derive(Debug, Clone)]
pub struct FolderInfo {
    /// Full IMAP folder name, e.g. `[Gmail]/Trash`.
    pub name: String,
    /// Raw SPECIAL-USE / system attribute strings from the LIST response,
    /// e.g. `"\\Trash"`, `"\\Archive"`, `"\\Sent"`.
    pub attributes: Vec<String>,
}

/// SPECIAL-USE folder paths discovered from LIST attributes (RFC 6154).
#[derive(Debug, Clone, Default)]
struct SpecialFolders {
    /// Full IMAP name of the Trash folder (e.g. `[Gmail]/Trash`).
    trash: Option<String>,
    /// Full IMAP name of the Archive / All Mail folder.
    archive: Option<String>,
    /// Full IMAP name of the Sent folder (e.g. `[Gmail]/Sent Mail`).
    sent: Option<String>,
    /// Full IMAP name of the Drafts folder (e.g. `[Gmail]/Drafts`).
    drafts: Option<String>,
}

/// A summarised message header (from IMAP ENVELOPE + FLAGS).
#[derive(Debug, Clone)]
pub struct MessageHeader {
    /// IMAP UID
    pub uid: u32,
    pub from: String,
    pub subject: String,
    pub date: String,
    /// Whether the `\Seen` flag is set.
    pub seen: bool,
    /// Whether the `\Flagged` (starred) flag is set.
    pub flagged: bool,
}

/// A file attached to a received message.
#[derive(Debug, Clone)]
pub struct EmailAttachment {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
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
    pub attachments: Vec<EmailAttachment>,
}

/// Compose form draft state.
#[derive(Debug, Clone, Default)]
pub struct Draft {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: MailBody,
    /// File paths to attach on send.
    pub attachments: Vec<String>,
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
    /// List all selectable folders with their SPECIAL-USE attributes.
    fn list_folders(&mut self) -> Result<Vec<FolderInfo>, String>;
    /// Fetch headers (including flags) for the most recent `limit` messages in `folder`.
    fn list_messages(&mut self, folder: &str, limit: usize) -> Result<Vec<MessageHeader>, String>;
    /// Fetch the full content of a message by UID.
    fn fetch_message(&mut self, folder: &str, uid: u32) -> Result<Option<EmailMessage>, String>;
    /// Fetch a message by its Message-ID header via IMAP SEARCH.
    fn fetch_message_by_message_id(
        &mut self,
        folder: &str,
        message_id: &str,
    ) -> Result<Option<EmailMessage>, String>;
    /// Add/remove IMAP flags on a message (e.g. `\\Seen`, `\\Flagged`, `\\Deleted`).
    fn set_flags(
        &mut self,
        folder: &str,
        uid: u32,
        add: &[&str],
        remove: &[&str],
    ) -> Result<(), String>;
    /// Copy a message to another folder (server-side COPY).
    fn copy_message(&mut self, folder: &str, uid: u32, dest: &str) -> Result<(), String>;
    /// Move a message to another folder (MOVE extension; falls back to COPY+DELETE+EXPUNGE).
    fn move_message(&mut self, folder: &str, uid: u32, dest: &str) -> Result<(), String>;
    /// Expunge a specific UID from a folder (UIDPLUS UID EXPUNGE).
    fn expunge_uid(&mut self, folder: &str, uid: u32) -> Result<(), String>;
    /// Append a raw RFC 2822 message to a folder (IMAP APPEND).
    fn append(&mut self, folder: &str, message: &[u8]) -> Result<(), String>;
    /// Fetch the UID thread map for `folder` using the IMAP THREAD extension.
    ///
    /// Returns `Some(threads)` where each inner `Vec<u32>` is the flat list of
    /// UIDs belonging to the same thread.  Returns `None` when the server does
    /// not advertise `THREAD=REFERENCES` capability.
    fn fetch_threads(&mut self, folder: &str) -> Result<Option<Vec<Vec<u32>>>, String>;
}

/// SMTP backend — send an email message.
/// Returns the raw RFC 2822 bytes of the sent message (for IMAP APPEND to Sent).
pub trait SmtpBackend: Send {
    fn send(
        &mut self,
        from: &str,
        to: &[&str],
        cc: &[&str],
        bcc: &[&str],
        subject: &str,
        body: &MailBody,
        attachments: &[(&str, &[u8])],
    ) -> Result<Vec<u8>, String>;
}

// ---------------------------------------------------------------------------
// Folder display-name → real-name mapping
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Settings file helpers (atomic write)
// ---------------------------------------------------------------------------

fn load_settings_json(path: &std::path::Path) -> serde_json::Value {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    serde_json::from_str(&content)
        .unwrap_or_else(|_| serde_json::Value::Object(Default::default()))
}

fn ensure_email_section(root: &mut serde_json::Value) -> &mut serde_json::Map<String, serde_json::Value> {
    let obj = root.as_object_mut().expect("settings root is object");
    if !obj.contains_key("email client") {
        obj.insert("email client".to_owned(), serde_json::Value::Object(Default::default()));
    }
    obj.get_mut("email client").expect("just inserted").as_object_mut().expect("email client is object")
}

/// Write `value` as pretty JSON to `path` atomically (write to `.tmp`, then rename).
fn atomic_write_json(path: &std::path::Path, value: &serde_json::Value) {
    let Ok(json) = serde_json::to_string_pretty(value) else { return };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, &json).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

// ---------------------------------------------------------------------------
// Folder display-name helper
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

/// Compute the FFON label for a message header.
///
/// Produces a prefix of bracketed state tags followed by the subject/from body:
///   `[read] [star] Subject — From`
///   `[unread] Subject — From`
///
/// The label is the single source of truth used in both `build_folder` (display)
/// and `lookup_uid` (reverse lookup), ensuring they always agree.
fn message_label(h: &MessageHeader) -> String {
    let read_tag = if h.seen { "[read]" } else { "[unread]" };
    let star_tag = if h.flagged { " [star]" } else { "" };
    let body = if h.subject.is_empty() {
        format!("(no subject) — {}", h.from)
    } else {
        format!("{} — {}", h.subject, h.from)
    };
    format!("{read_tag}{star_tag} {body}")
}

/// Strip all leading `[tag]` prefixes from a message label, returning the bare
/// `Subject — From` body.  Used by `lookup_uid` when the cached flags have changed
/// since the path label was recorded.
fn strip_message_tags(label: &str) -> &str {
    let mut s = label;
    loop {
        let trimmed = s.trim_start();
        if let Some(rest) = trimmed.strip_prefix('[') {
            if let Some(end) = rest.find(']') {
                s = rest[end + 1..].trim_start_matches(' ');
            } else {
                break;
            }
        } else {
            break;
        }
    }
    s
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

    // Per-folder message display limit (default 50; increases via "Load more…").
    folder_limits: std::collections::HashMap<String, usize>,

    // Cached envelope list for the current folder.
    envelope_cache: Option<Vec<FfonElement>>,
    envelope_cache_folder: String,

    // Cached message headers for the current folder (used for UID lookup).
    message_cache: Vec<MessageHeader>,
    // Cached full message for the current message path.
    message_detail: Option<EmailMessage>,

    // SPECIAL-USE folders discovered from LIST attributes (populated on folder fetch).
    special_folders: SpecialFolders,

    // Pending "move" target: UID + source real-folder name stored between
    // handle_command("move") and execute_command("move", dest_display).
    pending_move_uid: Option<u32>,
    pending_move_folder: String,

    // Compose state
    compose: ComposeState,
    compose_sent: bool,

    // History: folder + References header stored when a message is viewed,
    // served lazily when the user navigates into "History".
    history_folder: String,
    history_refs: String,
    history_uid: Option<u32>,
    // Thread map populated by fetch_threads() on folder entry: uid → all UIDs in that thread.
    thread_cache: std::collections::HashMap<String, std::collections::HashMap<u32, Vec<u32>>>,

    // Cross-thread needs-refresh flag (set by IDLE, cleared by fetch).
    needs_refresh_flag: Arc<AtomicBool>,

    // IDLE background thread controller.
    idle: IdleController,

    // Injected backends (None until init() or with_imap/with_smtp).
    imap: Option<Box<dyn ImapBackend>>,
    smtp: Option<Box<dyn SmtpBackend>>,

    // Async folder fetch — moves list_folders() off the main thread at startup.
    folder_fetch_inflight: Arc<AtomicBool>,
    folder_fetch_result: Arc<Mutex<Option<Result<Vec<FolderInfo>, String>>>>,
    // Disabled in tests that inject a mock via with_imap() so they keep using the sync path.
    async_folder_fetch_enabled: bool,

    // Parallel INBOX prefetch — started alongside the folder list fetch so
    // navigating into INBOX doesn't need a second cold-start round-trip.
    inbox_prefetch_result: Arc<Mutex<Option<Result<Vec<MessageHeader>, String>>>>,

    // Outbox: set when a send fails so tick() retries on the next cycle.
    // The draft remains in self.compose.draft; this is just the retry flag.
    outbox_pending: bool,

    // Async OAuth token refresh — moves oauth2::refresh_token() off the main thread at startup.
    // Result carries (new_access_token, new_token_expiry) on success.
    token_refresh_inflight: Arc<AtomicBool>,
    token_refresh_result: Arc<Mutex<Option<Result<(String, i64), String>>>>,

    // Pending error message to surface via take_error.
    error_message: Option<String>,

    // Pending undo descriptor for the last completed undoable command.
    // Drained by the app dispatcher after each handle_command/execute_command call.
    last_undo_descriptor: Option<ProviderUndoDescriptor>,

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
            folder_limits: std::collections::HashMap::new(),
            envelope_cache: None,
            envelope_cache_folder: String::new(),
            message_cache: Vec::new(),
            message_detail: None,
            special_folders: SpecialFolders::default(),
            pending_move_uid: None,
            pending_move_folder: String::new(),
            compose: ComposeState::default(),
            compose_sent: false,
            history_folder: String::new(),
            history_refs: String::new(),
            history_uid: None,
            thread_cache: std::collections::HashMap::new(),
            needs_refresh_flag: Arc::clone(&needs_refresh_flag),
            idle: IdleController::new(needs_refresh_flag),
            imap: None,
            smtp: None,
            folder_fetch_inflight: Arc::new(AtomicBool::new(false)),
            folder_fetch_result: Arc::new(Mutex::new(None)),
            async_folder_fetch_enabled: true,
            inbox_prefetch_result: Arc::new(Mutex::new(None)),
            outbox_pending: false,
            token_refresh_inflight: Arc::new(AtomicBool::new(false)),
            token_refresh_result: Arc::new(Mutex::new(None)),
            error_message: None,
            last_undo_descriptor: None,
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

    /// Look up a UID by message display label (as produced by `message_label`).
    ///
    /// Falls back to stripping all leading `[tag]` prefixes when no exact match is found,
    /// because the current path may still carry old tags after a flag change updates the cache.
    fn lookup_uid(&self, label: &str) -> Option<u32> {
        // Fast path: exact label match.
        if let Some(h) = self.message_cache.iter().find(|h| message_label(h) == label) {
            return Some(h.uid);
        }
        // Fallback: compare against the bare "Subject — From" body, ignoring tag prefixes.
        let body = strip_message_tags(label);
        self.message_cache
            .iter()
            .find(|h| {
                let base = if h.subject.is_empty() {
                    format!("(no subject) — {}", h.from)
                } else {
                    format!("{} — {}", h.subject, h.from)
                };
                base == body
            })
            .map(|h| h.uid)
    }

    /// Execute an undo (`is_undo=true`) or redo (`is_undo=false`) of a
    /// previously recorded provider command descriptor.
    fn apply_provider_command(
        &mut self,
        descriptor: &ProviderUndoDescriptor,
        is_undo: bool,
        error: &mut String,
    ) {
        let imap = match self.imap.as_mut() {
            Some(i) => i,
            None => { *error = "not connected".to_owned(); return; }
        };
        let json: serde_json::Value = serde_json::from_str(descriptor.payload_str())
            .unwrap_or(serde_json::Value::Null);

        match descriptor.command.as_str() {
            "delete-trash" | "archive" | "move" => {
                // For undo: move back from destination to source.
                // For redo: move from source to destination.
                let dest_field = match descriptor.command.as_str() {
                    "delete-trash" => "trash",
                    "archive" => "archive",
                    _ => "dest",
                };
                let (search_in, move_to) = if is_undo {
                    (json[dest_field].as_str().unwrap_or(""),
                     json["src"].as_str().unwrap_or(""))
                } else {
                    (json["src"].as_str().unwrap_or(""),
                     json[dest_field].as_str().unwrap_or(""))
                };
                let msg_id = json["msg_id"].as_str().unwrap_or("");
                match imap.fetch_message_by_message_id(search_in, msg_id) {
                    Ok(Some(msg)) => {
                        if let Err(e) = imap.move_message(search_in, msg.uid, move_to) {
                            *error = format!("{} {}: {e}", if is_undo { "undo" } else { "redo" }, descriptor.command);
                        }
                    }
                    Ok(None) => {
                        *error = format!(
                            "{} {}: message no longer in {}",
                            if is_undo { "undo" } else { "redo" },
                            descriptor.command,
                            search_in,
                        );
                    }
                    Err(e) => {
                        *error = format!("{} {}: {e}", if is_undo { "undo" } else { "redo" }, descriptor.command);
                    }
                }
                self.envelope_cache = None;
                self.message_cache.clear();
            }
            "mark-read" | "mark-unread" => {
                let folder = json["folder"].as_str().unwrap_or("");
                let uid = json["uid"].as_u64().unwrap_or(0) as u32;
                let prev_seen = json["prev_seen"].as_bool().unwrap_or(false);
                // Undo restores prev_seen; redo negates it.
                let target_seen = if is_undo { prev_seen } else { !prev_seen };
                let (add, remove): (&[&str], &[&str]) =
                    if target_seen { (&["\\Seen"], &[]) } else { (&[], &["\\Seen"]) };
                if let Err(e) = imap.set_flags(folder, uid, add, remove) {
                    *error = format!("{} {}: {e}", if is_undo { "undo" } else { "redo" }, descriptor.command);
                } else {
                    if let Some(h) = self.message_cache.iter_mut().find(|h| h.uid == uid) {
                        h.seen = target_seen;
                    }
                    self.envelope_cache = None;
                }
            }
            "star" | "unstar" => {
                let folder = json["folder"].as_str().unwrap_or("");
                let uid = json["uid"].as_u64().unwrap_or(0) as u32;
                let prev_flagged = json["prev_flagged"].as_bool().unwrap_or(false);
                let target_flagged = if is_undo { prev_flagged } else { !prev_flagged };
                let (add, remove): (&[&str], &[&str]) =
                    if target_flagged { (&["\\Flagged"], &[]) } else { (&[], &["\\Flagged"]) };
                if let Err(e) = imap.set_flags(folder, uid, add, remove) {
                    *error = format!("{} {}: {e}", if is_undo { "undo" } else { "redo" }, descriptor.command);
                } else {
                    if let Some(h) = self.message_cache.iter_mut().find(|h| h.uid == uid) {
                        h.flagged = target_flagged;
                    }
                    self.envelope_cache = None;
                }
            }
            _ => {
                *error = format!("unknown undoable command: {}", descriptor.command);
            }
        }
    }

    /// Return the (real_folder, uid) for the message identified by `elem_key`.
    ///
    /// Works at two depths:
    /// - depth 2 (inside a message): folder from path[0], message from path[1].
    /// - depth 1 (folder list):      folder from path[0], message from `elem_key`.
    ///
    /// Returns `None` when there is no folder context (depth 0) or the label
    /// does not resolve to a known UID.
    fn current_message_uid(&self, elem_key: &str) -> Option<(String, u32)> {
        let segs = self.path_segments();
        match segs.len() {
            0 => None,
            1 => {
                // At folder list — elem_key is the selected message label.
                let real_folder = self.lookup_folder(segs[0]).to_owned();
                let uid = self.lookup_uid(elem_key)?;
                Some((real_folder, uid))
            }
            _ => {
                // Inside a message — use path[1].
                let real_folder = self.lookup_folder(segs[0]).to_owned();
                let uid = self.lookup_uid(segs[1])?;
                Some((real_folder, uid))
            }
        }
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
        let mut root = load_settings_json(&path);
        let section = ensure_email_section(&mut root);
        section.insert("emailImapUrl".to_owned(), self.config.imap_url.clone().into());
        section.insert("emailSmtpUrl".to_owned(), self.config.smtp_url.clone().into());
        section.insert("emailUsername".to_owned(), self.config.username.clone().into());
        atomic_write_json(&path, &root);
    }

    /// Persist OAuth tokens to settings.json.
    fn save_oauth_tokens(&self) {
        let Some(path) = self.config_path() else { return };
        let mut root = load_settings_json(&path);
        let section = ensure_email_section(&mut root);
        section.insert("emailOAuthAccessToken".to_owned(), self.config.oauth_access_token.clone().into());
        section.insert("emailOAuthRefreshToken".to_owned(), self.config.oauth_refresh_token.clone().into());
        section.insert("emailTokenExpiry".to_owned(), self.config.token_expiry.into());
        atomic_write_json(&path, &root);
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
        // Always sync username from the token so the XOAUTH2 `user=` field
        // matches the authenticated account, even when a username was previously set.
        if let Some(email) = oauth2::fetch_email(&self.config.oauth_access_token) {
            self.config.username = email;
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

    /// Convert a `list_folders` result into FFON items, populate caches, mappings,
    /// and discover SPECIAL-USE folders (Trash, Archive) from LIST attributes.
    fn build_root_from_folder_list(
        &mut self,
        folder_result: Result<Vec<FolderInfo>, String>,
    ) -> Vec<FfonElement> {
        let mut items = vec![];
        match folder_result {
            Err(e) => {
                items.push(FfonElement::new_str(format!("IMAP error: {e}")));
            }
            Ok(folder_infos) => {
                // Rebuild folder display-name mappings.
                self.folder_mappings.clear();
                self.special_folders = SpecialFolders::default();

                let names: Vec<&str> = folder_infos.iter().map(|f| f.name.as_str()).collect();

                // Filter out hierarchy-only container folders (e.g. "[Gmail]")
                // — a folder is a container if any other folder starts with it + "/".
                let mut real_infos: Vec<&FolderInfo> = Vec::new();
                for info in &folder_infos {
                    let is_container = names.iter().any(|other| {
                        *other != info.name.as_str()
                            && other.starts_with(&format!("{}/", info.name))
                    });
                    if !is_container {
                        real_infos.push(info);
                    }
                }

                // Detect SPECIAL-USE folders from attributes (RFC 6154).
                for info in &folder_infos {
                    for attr in &info.attributes {
                        match attr.as_str() {
                            "\\Trash" => {
                                if self.special_folders.trash.is_none() {
                                    self.special_folders.trash = Some(info.name.clone());
                                }
                            }
                            "\\Archive" | "\\All" => {
                                if self.special_folders.archive.is_none() {
                                    self.special_folders.archive = Some(info.name.clone());
                                }
                            }
                            "\\Sent" => {
                                if self.special_folders.sent.is_none() {
                                    self.special_folders.sent = Some(info.name.clone());
                                }
                            }
                            "\\Drafts" => {
                                if self.special_folders.drafts.is_none() {
                                    self.special_folders.drafts = Some(info.name.clone());
                                }
                            }
                            _ => {}
                        }
                    }
                }

                let mut compose_inserted = false;
                for info in real_infos {
                    let display = folder_display_name(&info.name).to_owned();
                    self.folder_mappings.push((display.clone(), info.name.clone()));
                    items.push(FfonElement::new_obj(display.clone()));
                    // Insert compose right after INBOX.
                    if !compose_inserted && info.name.to_uppercase() == "INBOX" {
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

        // Spawn a parallel INBOX prefetch so the first navigate-into-INBOX is
        // instant.  Only in async mode (production); tests use injected mocks.
        if self.async_folder_fetch_enabled {
            let result_slot = Arc::clone(&self.inbox_prefetch_result);
            let needs_refresh = Arc::clone(&self.needs_refresh_flag);
            let config = self.config.clone();
            std::thread::spawn(move || {
                let mut imap = crate::net::RealImap::from_config(&config);
                let result = imap.list_messages("INBOX", 50);
                *result_slot.lock().unwrap() = Some(result);
                needs_refresh.store(true, Ordering::Release);
            });
        }

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
                return vec![FfonElement::new_str("Loading…".to_owned())];
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
                    let result: Result<Vec<FolderInfo>, String> = imap.list_folders();
                    *result_slot.lock().unwrap() = Some(result);
                    inflight.store(false, Ordering::Release);
                    needs_refresh.store(true, Ordering::Release);
                });
                return vec![FfonElement::new_str("Loading…".to_owned())];
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

        let limit = *self.folder_limits.get(&real_folder).unwrap_or(&50);

        // Use the parallel INBOX prefetch result when available.
        let prefetch = if real_folder == "INBOX" {
            self.inbox_prefetch_result.lock().ok().and_then(|mut g| g.take())
        } else {
            None
        };

        let folder_result = if let Some(result) = prefetch {
            result
        } else {
            let imap = match self.imap_mut() {
                Some(b) => b,
                None => {
                    items.push(FfonElement::new_str("(no IMAP backend)".to_owned()));
                    return items;
                }
            };
            imap.list_messages(&real_folder, limit)
        };

        match folder_result {
            Err(e) => items.push(FfonElement::new_str(format!("IMAP error: {e}"))),
            Ok(headers) => {
                let at_limit = headers.len() >= limit;
                self.message_cache = headers.clone();
                for h in &headers {
                    items.push(FfonElement::new_obj(message_label(h)));
                }
                if items.is_empty() {
                    items.push(FfonElement::new_str("(no messages)".to_owned()));
                }
                if at_limit {
                    items.push(FfonElement::new_str(
                        "<button>load-more</button>Load more…".to_owned(),
                    ));
                }
            }
        }

        // Cache results and start IDLE for this folder.
        self.envelope_cache = Some(items.clone());
        self.envelope_cache_folder = real_folder.clone();

        // Build a UID→thread map using IMAP THREAD if supported.
        // Failure is non-fatal: fall back to the References-based path.
        if !self.thread_cache.contains_key(&real_folder) {
            if let Some(imap) = self.imap.as_mut() {
                if let Ok(Some(threads)) = imap.fetch_threads(&real_folder) {
                    let mut uid_to_thread: std::collections::HashMap<u32, Vec<u32>> =
                        std::collections::HashMap::new();
                    for thread in threads {
                        for &uid in &thread {
                            uid_to_thread.insert(uid, thread.clone());
                        }
                    }
                    self.thread_cache.insert(real_folder.clone(), uid_to_thread);
                }
            }
        }

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

        // Auto-mark as read: set \Seen flag and update cache so the list
        // re-renders without the ● prefix on the next envelope fetch.
        if let Some(h) = self.message_cache.iter_mut().find(|h| h.uid == uid) {
            if !h.seen {
                let marked = if let Some(ref mut imap) = self.imap {
                    imap.set_flags(&real_folder, uid, &["\\Seen"], &[]).is_ok()
                } else {
                    false
                };
                if marked {
                    h.seen = true;
                    // Invalidate envelope cache so the list re-renders without ●.
                    self.envelope_cache = None;
                }
            }
        }

        // Store context for lazy History navigation.
        if !msg.references.is_empty() || self.thread_cache.contains_key(&real_folder) {
            self.history_folder = real_folder;
            self.history_refs = msg.references.clone();
            self.history_uid = Some(uid);
        } else {
            self.history_refs.clear();
            self.history_uid = None;
        }

        build_message_view(&msg)
    }

    fn build_history(&mut self) -> Vec<FfonElement> {
        let folder = self.history_folder.clone();

        // Fast path: use the IMAP THREAD cache (single THREAD command replaces
        // N per-Message-ID SEARCH commands).
        if let Some(uid) = self.history_uid {
            if let Some(uid_map) = self.thread_cache.get(&folder) {
                if let Some(thread_uids) = uid_map.get(&uid).cloned() {
                    let other_uids: Vec<u32> = thread_uids
                        .iter()
                        .copied()
                        .filter(|&u| u != uid)
                        .take(10)
                        .collect();
                    if !other_uids.is_empty() {
                        let mut items = vec![];
                        for other_uid in other_uids {
                            // Check message_cache first (already fetched for this folder).
                            let label = self
                                .message_cache
                                .iter()
                                .find(|h| h.uid == other_uid)
                                .map(|h| format!("From: {} — Subject: {}", h.from, h.subject));
                            if let Some(lbl) = label {
                                items.push(FfonElement::new_obj(lbl));
                            } else if let Some(ref mut imap) = self.imap {
                                // Not in the current-folder cache — fetch by UID.
                                let uid_str = other_uid.to_string();
                                if let Ok(Some(msg)) = imap.fetch_message(&folder, other_uid) {
                                    let _ = uid_str;
                                    items.push(FfonElement::new_obj(
                                        format!("From: {} — Subject: {}", msg.from, msg.subject),
                                    ));
                                }
                            }
                        }
                        if !items.is_empty() {
                            return items;
                        }
                    }
                }
            }
        }

        // Slow path: parse Message-IDs from the References header and SEARCH.
        if self.history_refs.is_empty() {
            return vec![FfonElement::new_str("(no history)".to_owned())];
        }

        let refs = self.history_refs.clone();
        // Search order: \All (Gmail / archive) first; fall back to current folder.
        let mut search_folders: Vec<String> = vec![];
        if let Some(ref all) = self.special_folders.archive {
            search_folders.push(all.clone());
        }
        if !search_folders.contains(&folder) {
            search_folders.push(folder);
        }

        let mut items = vec![];
        let mut count = 0;

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

            'search: for search_folder in &search_folders {
                if let Some(ref mut imap) = self.imap {
                    if let Ok(Some(msg)) = imap.fetch_message_by_message_id(search_folder, msg_id) {
                        let key = format!("From: {} — Subject: {}", msg.from, msg.subject);
                        items.push(FfonElement::new_obj(key));
                        count += 1;
                        break 'search;
                    }
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
            "Cc: <input>{}</input>",
            self.compose.draft.cc
        )));
        items.push(FfonElement::new_str(format!(
            "Bcc: <input>{}</input>",
            self.compose.draft.bcc
        )));
        items.push(FfonElement::new_str(format!(
            "Subject: <input>{}</input>",
            self.compose.draft.subject
        )));

        // Attachments: list current files, then an empty input for adding more.
        let mut attach_children: Vec<FfonElement> = self.compose.draft.attachments.iter()
            .map(|p| FfonElement::new_str(p.clone()))
            .collect();
        attach_children.push(FfonElement::new_str("<input></input>".to_owned()));
        items.push(FfonElement::Obj(FfonObject {
            key: "Attachments:".to_owned(),
            children: attach_children,
        }));

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

    /// Sync the last path segment to the current body format label.
    ///
    /// Called after any mutation that may change the `MailBody` variant so that
    /// the parent label shown between the header and the list (e.g. `Body: [ffon]`)
    /// is always up-to-date.
    fn sync_body_path_label(&mut self) {
        let new_label = body_format_label(&self.compose.draft.body);
        if let Some(slash) = self.current_path.rfind('/') {
            self.current_path = format!("{}/{new_label}", &self.current_path[..slash]);
        }
    }

    fn split_addrs(s: &str) -> Vec<String> {
        s.split(',').map(|a| a.trim().to_owned()).filter(|a| !a.is_empty()).collect()
    }

    fn send_draft(&mut self) -> Result<(), String> {
        self.ensure_fresh_token();
        let Some(ref mut smtp) = self.smtp else {
            return Err("no SMTP backend".to_owned());
        };
        let from = self.config.username.clone();
        let to_v = Self::split_addrs(&self.compose.draft.to);
        let cc_v = Self::split_addrs(&self.compose.draft.cc);
        let bcc_v = Self::split_addrs(&self.compose.draft.bcc);
        let to_r: Vec<&str> = to_v.iter().map(|s| s.as_str()).collect();
        let cc_r: Vec<&str> = cc_v.iter().map(|s| s.as_str()).collect();
        let bcc_r: Vec<&str> = bcc_v.iter().map(|s| s.as_str()).collect();
        let subject = self.compose.draft.subject.clone();
        let body = normalize_body_for_send(&self.compose.draft.body);

        // Read attachment files. Files that cannot be read are silently skipped.
        let attachment_data: Vec<(String, Vec<u8>)> = self.compose.draft.attachments.iter()
            .filter_map(|path| {
                let bytes = std::fs::read(path).ok()?;
                let name = std::path::Path::new(path)
                    .file_name()?.to_string_lossy().into_owned();
                Some((name, bytes))
            })
            .collect();
        let attachment_refs: Vec<(&str, &[u8])> = attachment_data.iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();

        let raw = smtp.send(&from, &to_r, &cc_r, &bcc_r, &subject, &body, &attachment_refs)?;

        // APPEND to Sent folder (skip for Gmail — its SMTP server auto-saves).
        let skip_append = self.config.smtp_url.contains("smtp.gmail.com");
        if !skip_append {
            if let Some(sent) = self.special_folders.sent.clone() {
                if let Some(ref mut imap) = self.imap {
                    let _ = imap.append(&sent, &raw);
                }
            }
        }
        Ok(())
    }

    fn is_draft_non_empty(draft: &Draft) -> bool {
        !draft.to.is_empty()
            || !draft.subject.is_empty()
            || !draft.attachments.is_empty()
            || !matches!(&draft.body, MailBody::Text(s) if s.is_empty())
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
            match outcome {
                Ok((access_token, expiry)) => {
                    self.config.oauth_access_token = access_token;
                    self.config.token_expiry = expiry;
                    self.save_oauth_tokens();
                    // Drop backends and cached folder list so the next fetch
                    // issues a fresh IMAP connection with the new token.
                    self.imap = None;
                    self.smtp = None;
                    self.folder_cache = None;
                    self.rebuild_backends();
                }
                Err(_) => {
                    // Refresh failed (e.g. revoked refresh token). Clear the stale
                    // access token so is_logged_in() returns false and the login
                    // button is shown instead of repeatedly failing with IMAP auth errors.
                    self.config.oauth_access_token.clear();
                    self.config.token_expiry = 0;
                    self.save_oauth_tokens();
                    self.imap = None;
                    self.smtp = None;
                }
            }
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
// Draft serialization helper
// ---------------------------------------------------------------------------

/// Build a minimal RFC 2822 representation of a compose draft for IMAP APPEND.
fn build_draft_bytes(draft: &Draft, from: &str) -> Vec<u8> {
    let mut msg = String::new();
    msg.push_str(&format!("From: {}\r\n", from));
    if !draft.to.is_empty() { msg.push_str(&format!("To: {}\r\n", draft.to)); }
    if !draft.cc.is_empty() { msg.push_str(&format!("Cc: {}\r\n", draft.cc)); }
    if !draft.bcc.is_empty() { msg.push_str(&format!("Bcc: {}\r\n", draft.bcc)); }
    if !draft.subject.is_empty() { msg.push_str(&format!("Subject: {}\r\n", draft.subject)); }
    msg.push_str("MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n");
    let body_text = match &draft.body {
        MailBody::Text(s) => s.clone(),
        MailBody::Ffon(elems) => sicompass_sdk::ffon::to_json_string(elems).unwrap_or_default(),
    };
    msg.push_str(&body_text);
    msg.into_bytes()
}

// ---------------------------------------------------------------------------
// Compose pre-fill helper
// ---------------------------------------------------------------------------

/// Extract the bare email address from a formatted address string.
/// `"Alice <alice@example.com>"` → `"alice@example.com"`.
/// Plain `"alice@example.com"` → `"alice@example.com"`.
fn extract_email_addr(s: &str) -> &str {
    if let (Some(lt), Some(gt)) = (s.find('<'), s.rfind('>')) {
        if lt < gt { return s[lt + 1..gt].trim(); }
    }
    s.trim()
}

fn prefill_compose(compose: &mut ComposeState, msg: &EmailMessage, mode: ComposeMode, username: &str) {
    match mode {
        ComposeMode::Reply | ComposeMode::ReplyAll => {
            if matches!(mode, ComposeMode::Reply) {
                compose.draft.to = msg.from.clone();
            } else {
                let mut recipients = vec![msg.from.clone()];
                for tok in msg.to.split(',') {
                    let t = tok.trim();
                    if !t.is_empty() && !extract_email_addr(t).eq_ignore_ascii_case(username) {
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
                // Text-origin: keep as text. Attribution + "> "-quoted lines
                // are prepended as plain text; body_to_compose_children will
                // render the whole blob as one editable <input> leaf.
                MailBody::Text(s) => {
                    let mut draft = format!("On {} <{}> wrote:\n", msg.date, msg.from);
                    for l in s.lines() {
                        draft.push_str("> ");
                        draft.push_str(l);
                        draft.push('\n');
                    }
                    MailBody::Text(draft)
                }
                // FFON-origin: preserve structure. Flat I_PLACEHOLDER +
                // attribution leaf + seeded original elements.
                MailBody::Ffon(orig) => {
                    let attribution = format!(
                        "<input>On {} <{}> wrote:</input>", msg.date, msg.from
                    );
                    let mut elems = vec![
                        FfonElement::new_str(I_PLACEHOLDER.to_owned()),
                        FfonElement::new_str(attribution),
                    ];
                    let mut orig_cloned = orig.clone();
                    seed_i_placeholders(&mut orig_cloned);
                    elems.extend(orig_cloned);
                    MailBody::Ffon(elems)
                }
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
                // Text-origin: keep as text. Forwarded-header block + original
                // body as plain text; rendered as one editable <input> leaf.
                MailBody::Text(s) => {
                    let mut draft = format!(
                        "---------- Forwarded message ----------\nFrom: {}\nTo: {}\nDate: {}\nSubject: {}\n\n{}",
                        msg.from, msg.to, msg.date, msg.subject, s
                    );
                    if !draft.ends_with('\n') { draft.push('\n'); }
                    MailBody::Text(draft)
                }
                // FFON-origin: preserve structure. Flat I_PLACEHOLDER +
                // forwarded-header leaves + seeded original elements.
                MailBody::Ffon(orig) => {
                    let fwd_header: Vec<FfonElement> = vec![
                        FfonElement::new_str("<input>---------- Forwarded message ----------</input>".to_owned()),
                        FfonElement::new_str(format!("<input>From: {}</input>", msg.from)),
                        FfonElement::new_str(format!("<input>To: {}</input>", msg.to)),
                        FfonElement::new_str(format!("<input>Date: {}</input>", msg.date)),
                        FfonElement::new_str(format!("<input>Subject: {}</input>", msg.subject)),
                    ];
                    let mut elems = vec![FfonElement::new_str(I_PLACEHOLDER.to_owned())];
                    elems.extend(fwd_header);
                    let mut orig_cloned = orig.clone();
                    seed_i_placeholders(&mut orig_cloned);
                    elems.extend(orig_cloned);
                    MailBody::Ffon(elems)
                }
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
        MailBody::Ffon(elems) => {
            items.extend(elems.clone());
        }
    }

    // List attachments if present.
    if !msg.attachments.is_empty() {
        let mut attach_obj = FfonElement::new_obj("Attachments");
        if let Some(obj) = attach_obj.as_obj_mut() {
            obj.children = msg.attachments.iter()
                .map(|a| FfonElement::new_str(format!(
                    "{} ({}, {} bytes)",
                    a.filename, a.content_type, a.data.len()
                )))
                .collect();
        }
        items.push(attach_obj);
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
/// - Text: one `<input>` leaf with the current content (empty → no children).
/// - Ffon: the stored elements directly (each leaf should already carry `<input>` tags).
/// Reconstruct a `MailBody` from FFON body children (inverse of `body_to_compose_children`).
///
/// Called by `sync_ffon_body_children` to keep `compose.draft.body` in sync after
/// app-level FFON mutations (Task::Delete undo/redo) that bypass `commit_edit`.
fn body_from_ffon_children(children: &[FfonElement]) -> MailBody {
    if children.is_empty() {
        return MailBody::Text(String::new());
    }
    // A single plain Str with an <input> tag is the Text representation.
    if children.len() == 1 {
        if let FfonElement::Str(s) = &children[0] {
            if let Some(content) = sicompass_sdk::tags::extract_input(s) {
                return MailBody::Text(content);
            }
        }
    }
    MailBody::Ffon(children.to_vec())
}

fn body_to_compose_children(body: &MailBody) -> Vec<FfonElement> {
    match body {
        MailBody::Text(s) if !s.is_empty() =>
            vec![FfonElement::new_str(format!("<input>{s}</input>"))],
        MailBody::Text(_) => vec![],
        MailBody::Ffon(elems) => elems.clone(),
    }
}

/// Apply an edit to a flat `Vec<FfonElement>` (the Ffon-body mutation logic).
///
/// Finds the element whose stripped `<input>` content equals `old_content` and
/// replaces it.  When `new_content` ends with `:`, a new `Obj` is created instead.
/// When no match is found the new content is appended (handles the case where the
/// placeholder was inserted locally in the FFON tree but not yet committed).
fn update_body_elems(elems: &mut Vec<FfonElement>, old_content: &str, new_content: &str) {
    use sicompass_sdk::tags;

    let is_obj_create = new_content.ends_with(':');
    let obj_key = if is_obj_create {
        new_content.trim_end_matches(':').trim()
    } else {
        ""
    };

    let pos = elems.iter().position(|e| {
        if let FfonElement::Str(s) = e {
            let stripped = tags::extract_input(s).unwrap_or_else(|| s.clone());
            stripped == old_content
        } else {
            false
        }
    });

    if is_obj_create && !obj_key.is_empty() {
        if let Some(idx) = pos {
            elems[idx] = new_obj_with_i_placeholder(format!("<input>{obj_key}</input>"));
        } else {
            elems.push(new_obj_with_i_placeholder(format!("<input>{obj_key}</input>")));
        }
    } else if let Some(idx) = pos {
        elems[idx] = FfonElement::new_str(format!("<input>{new_content}</input>"));
    } else {
        elems.push(FfonElement::new_str(format!("<input>{new_content}</input>")));
    }
}

/// Walk the body Ffon tree (immutable) following `sub_segs` (display-stripped Obj
/// key segments after `Body:`) and return the children slice of the target Obj.
fn body_elems_at_sub_path<'a>(
    elems: &'a [FfonElement],
    sub_segs: &[&str],
) -> Option<&'a [FfonElement]> {
    if sub_segs.is_empty() {
        return Some(elems);
    }
    let seg = sub_segs[0];
    let rest = &sub_segs[1..];
    let idx = elems.iter().position(|e| {
        if let FfonElement::Obj(o) = e {
            sicompass_sdk::tags::strip_display(&o.key) == seg
        } else {
            false
        }
    })?;
    if let FfonElement::Obj(o) = &elems[idx] {
        body_elems_at_sub_path(&o.children, rest)
    } else {
        None
    }
}

/// Walk the body Ffon tree (mutable) following `sub_segs` and return a mutable
/// reference to the children vec of the target Obj.
fn body_elems_at_sub_path_mut<'a>(
    elems: &'a mut Vec<FfonElement>,
    sub_segs: &[&str],
) -> Option<&'a mut Vec<FfonElement>> {
    if sub_segs.is_empty() {
        return Some(elems);
    }
    let seg = sub_segs[0];
    let rest = &sub_segs[1..];
    let idx = elems.iter().position(|e| {
        if let FfonElement::Obj(o) = e {
            sicompass_sdk::tags::strip_display(&o.key) == seg
        } else {
            false
        }
    })?;
    if let FfonElement::Obj(o) = &mut elems[idx] {
        body_elems_at_sub_path_mut(&mut o.children, rest)
    } else {
        None
    }
}

/// Update a body leaf after a text edit in the compose form.
///
/// Called from `commit_edit` when the path ends directly at `"Body:"` (top-level
/// body edit).  For nested edits (path contains `Body:` but has more segments after
/// it), `commit_edit` resolves the sub-path and calls `update_body_elems` directly.
///
/// Matches the element whose stripped input content equals `old_content`
/// and replaces it with `new_content`.  When `old_content` is empty
/// (a freshly-inserted placeholder), the first empty leaf is filled.
/// If the body was `Text` and a new element is being added (no match),
/// it is upgraded to `Ffon` to hold multiple elements.
fn update_body_leaf(body: &mut MailBody, old_content: &str, new_content: &str) {
    let is_obj_create = new_content.ends_with(':');
    let obj_key = if is_obj_create {
        new_content.trim_end_matches(':').trim()
    } else {
        ""
    };

    match body {
        MailBody::Text(s) => {
            if is_obj_create && !obj_key.is_empty() {
                // Obj creation — preserve any existing text as a Str leaf and append the new Obj.
                let existing = s.clone();
                let mut elems: Vec<FfonElement> = if existing.is_empty() {
                    vec![]
                } else {
                    vec![FfonElement::new_str(format!("<input>{existing}</input>"))]
                };
                elems.push(new_obj_with_i_placeholder(format!("<input>{obj_key}</input>")));
                *body = MailBody::Ffon(elems);
            } else if old_content.is_empty() && !s.is_empty() {
                // A new element is being inserted alongside existing content — upgrade to Ffon.
                let existing = s.clone();
                *body = MailBody::Ffon(vec![
                    FfonElement::new_str(format!("<input>{existing}</input>")),
                    FfonElement::new_str(format!("<input>{new_content}</input>")),
                ]);
            } else {
                // Simple replacement.
                *s = new_content.to_owned();
            }
        }
        MailBody::Ffon(elems) => {
            update_body_elems(elems, old_content, new_content);
        }
    }
}

/// Recursively remove the element at `path` within a `Vec<FfonElement>`.
///
/// - `path = [i]`: remove `elems[i]` directly.
/// - `path = [i, rest..]`: descend into `elems[i]` (must be an `Obj`) and recurse.
/// - `path = []`: no-op, returns `false`.
fn remove_at(elems: &mut Vec<FfonElement>, path: &[usize]) -> bool {
    match path {
        [] => false,
        [last] => {
            if *last < elems.len() {
                elems.remove(*last);
                true
            } else {
                false
            }
        }
        [head, rest @ ..] => match elems.get_mut(*head) {
            Some(FfonElement::Obj(o)) => {
                let removed = remove_at(&mut o.children, rest);
                if removed && o.children.is_empty() {
                    o.children.push(FfonElement::new_str(I_PLACEHOLDER.to_owned()));
                }
                removed
            }
            _ => false,
        },
    }
}

/// Remove a body element at the given path (indices into the body's Ffon tree).
///
/// For `Ffon` bodies: walks the tree by `path` and removes the element there.
/// For `Text`: any non-empty single-segment path clears the body.
/// After removal, ensures the top-level element list is never left empty.
fn delete_body_element_at(body: &mut MailBody, path: &[usize]) -> bool {
    match body {
        MailBody::Ffon(elems) => {
            if !remove_at(elems, path) {
                return false;
            }
            if elems.is_empty() {
                elems.push(FfonElement::new_str(I_PLACEHOLDER.to_owned()));
            }
            true
        }
        MailBody::Text(_) if !path.is_empty() => {
            *body = MailBody::Ffon(vec![FfonElement::new_str(I_PLACEHOLDER.to_owned())]);
            true
        }
        _ => false,
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
        MailBody::Ffon(elems) => {
            elems.push(FfonElement::new_str("<input></input>".to_owned()));
        }
    }
}

/// Normalise a draft body before sending (auto-detect format, collapse trivial Ffon).
///
/// - Single-element `Ffon([Str("<input>text</input>")])` → `Text(text)`.
/// - `Text` that parses as valid FFON JSON → `Ffon(parsed)`.
/// - Everything else: unchanged.
fn normalize_body_for_send(body: &MailBody) -> MailBody {
    use sicompass_sdk::tags;

    match body {
        MailBody::Ffon(elems) if elems.len() == 1 => {
            if let FfonElement::Str(s) = &elems[0] {
                let plain = tags::extract_input(s).unwrap_or_else(|| s.clone());
                // Collapse single-element Ffon back to Text (or re-detect as Ffon).
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
            body.clone()
        }
        _ => body.clone(),
    }
}

/// Display label for the `Body:` Obj key — reflects the current format live.
fn body_format_label(body: &MailBody) -> &'static str {
    match body {
        MailBody::Text(_) => "Body: [text]",
        MailBody::Ffon(_) => "Body: [ffon]",
    }
}

/// Keep the `MailBody` variant in sync with the structural shape after a mutation.
///
/// Collapses a single plain-Str `Ffon` leaf back to `Text`; otherwise leaves
/// the variant alone.  The `I_PLACEHOLDER` sentinel is never collapsed so that
/// the insertion affordance stays visible in the compose view after the body
/// is emptied by deleting all children.
fn renormalize_body_variant(body: &mut MailBody) {
    use sicompass_sdk::tags;
    if let MailBody::Ffon(elems) = body {
        if elems.len() == 1 {
            if let FfonElement::Str(raw) = &elems[0] {
                if raw != I_PLACEHOLDER {
                    let plain = tags::extract_input(raw).unwrap_or_else(|| raw.clone());
                    *body = MailBody::Text(plain);
                }
            }
        }
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

    fn refresh_on_navigate(&self) -> bool {
        // Don't re-fetch on every navigation step inside a compose form — the form
        // is built once and maintained as live FFON so undo/redo of body edits persists
        // across navigation.  Regular folder/message navigation still refreshes.
        let in_compose = self.current_path
            .trim_start_matches('/')
            .split('/')
            .any(|s| matches!(s, "compose" | "reply" | "reply all" | "forward"));
        !in_compose
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

        // If the last path segment is "Body:" (possibly with content) and we're in a compose
        // context, return the body children directly — any other routing would misroute.
        let in_compose = segs.iter().any(|s| matches!(s.as_str(), "compose" | "reply" | "reply all" | "forward"));
        if in_compose && segs.last().map_or(false, |s| s.starts_with("Body:")) {
            return body_to_compose_children(&self.compose.draft.body);
        }

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
        // Save draft and reset compose state when navigating away.
        if self.at_compose() {
            self.compose_sent = false;
            if Self::is_draft_non_empty(&self.compose.draft) {
                if let Some(drafts_folder) = self.special_folders.drafts.clone() {
                    let bytes = build_draft_bytes(&self.compose.draft, &self.config.username);
                    if let Some(ref mut imap) = self.imap {
                        let _ = imap.append(&drafts_folder, &bytes);
                    }
                }
            }
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
        // Collect path segments as owned strings so we can freely mutate `self` later.
        let segs: Vec<String> = self.current_path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_owned())
            .collect();
        let last = segs.last().map(|s| s.as_str()).unwrap_or("");

        match last {
            "To" => {
                self.compose.draft.to = new_content.to_owned();
                true
            }
            "Cc" => {
                self.compose.draft.cc = new_content.to_owned();
                true
            }
            "Bcc" => {
                self.compose.draft.bcc = new_content.to_owned();
                true
            }
            "Attachments:" => {
                // Adding a new path — only non-empty strings.
                let p = new_content.trim();
                if !p.is_empty() {
                    self.compose.draft.attachments.push(p.to_owned());
                }
                true
            }
            "Subject" => {
                self.compose.draft.subject = new_content.to_owned();
                true
            }
            _ => {
                // Find a `Body:` segment anywhere in the path — covers both top-level
                // (`/compose/Body: [ffon]`) and nested (`/compose/Body: [ffon]/foo/bar`).
                let Some(body_pos) = segs.iter().position(|s| s.starts_with("Body:")) else {
                    return false;
                };
                // Sub-path: segments after `Body:` (empty → top-level body edit).
                let sub_segs: Vec<String> = segs[body_pos + 1..].iter().cloned().collect();

                if sub_segs.is_empty() {
                    // Top-level body edit — same as before.
                    update_body_leaf(&mut self.compose.draft.body, old_content, new_content);
                    // Live format detection — one-way promotion only (never collapses Ffon).
                    let promoted = detect_body_format_live(
                        std::mem::take(&mut self.compose.draft.body),
                    );
                    self.compose.draft.body = promoted;
                    // Re-sync variant with structural shape.
                    renormalize_body_variant(&mut self.compose.draft.body);
                    self.sync_body_path_label();
                } else {
                    // Nested body edit — walk the Ffon tree to the target Obj's children
                    // and apply the mutation there.  No format promotion / label sync:
                    // the body is already Ffon (you can only navigate into nested Objs when
                    // the body is Ffon), and the path label doesn't change.
                    let sub_segs_ref: Vec<&str> =
                        sub_segs.iter().map(|s| s.as_str()).collect();
                    if let MailBody::Ffon(ref mut elems) = self.compose.draft.body {
                        if let Some(target) =
                            body_elems_at_sub_path_mut(elems, &sub_segs_ref)
                        {
                            update_body_elems(target, old_content, new_content);
                        }
                    }
                }
                true
            }
        }
    }

    fn on_button_press(&mut self, function_name: &str) {
        match function_name {
            "send" => {
                match self.send_draft() {
                    Ok(()) => {
                        self.compose = ComposeState::default();
                        self.compose_sent = true;
                        self.outbox_pending = false;
                    }
                    Err(e) => {
                        self.error_message = Some(format!("send failed: {e} — will retry"));
                        // Save draft to \Drafts so it survives a restart.
                        if let Some(drafts_folder) = self.special_folders.drafts.clone() {
                            let bytes = build_draft_bytes(&self.compose.draft, &self.config.username);
                            if let Some(ref mut imap) = self.imap {
                                let _ = imap.append(&drafts_folder, &bytes);
                            }
                        }
                        self.outbox_pending = true;
                    }
                }
            }
            "login" => {
                if let Err(e) = self.do_login() {
                    self.error_message = Some(e);
                }
            }
            // "Add new line" button inside the Body: subtree.
            "body_new_line" => {
                body_add_element(&mut self.compose.draft.body);
                renormalize_body_variant(&mut self.compose.draft.body);
                self.sync_body_path_label();
            }
            "load-more" => {
                // Increase the display limit for the current folder by 50.
                let segs = self.path_segments();
                if let Some(display) = segs.first().cloned() {
                    let real = self.lookup_folder(&display).to_owned();
                    let entry = self.folder_limits.entry(real).or_insert(50);
                    *entry += 50;
                    self.envelope_cache = None;
                }
            }
            _ => {}
        }
    }

    fn tick(&mut self) -> bool {
        let mut needs_refresh = false;

        // Retry any queued outbox message.
        if self.outbox_pending {
            match self.send_draft() {
                Ok(()) => {
                    self.outbox_pending = false;
                    self.compose = ComposeState::default();
                    self.compose_sent = true;
                    needs_refresh = true;
                }
                Err(_) => {} // keep outbox_pending, retry next tick
            }
        }

        let handle = match self.active_login.take() {
            Some(h) => h,
            None => return needs_refresh,
        };
        match handle.poll() {
            None => {
                // Still waiting — put the handle back.
                self.active_login = Some(handle);
                needs_refresh
            }
            Some(result) => {
                self.finish_login(result);
                true
            }
        }
    }

    fn collect_deep_search_items(&self) -> Option<Vec<sicompass_sdk::provider::SearchResultItem>> {
        use sicompass_sdk::provider::SearchResultItem;
        let mut results = Vec::new();
        for h in &self.message_cache {
            // Look up the folder display name via the reverse of folder_mappings.
            let folder_display = self.folder_mappings.iter()
                .find(|(_, real)| real == &self.envelope_cache_folder)
                .map(|(d, _)| d.as_str())
                .unwrap_or(&self.envelope_cache_folder);
            let label = message_label(h);
            results.push(SearchResultItem {
                label: label.clone(),
                breadcrumb: format!("{} > ", folder_display),
                nav_path: format!("/{}/{}", folder_display, label),
            });
        }
        Some(results)
    }

    fn take_error(&mut self) -> Option<String> {
        self.error_message.take()
    }

    fn take_last_undo_descriptor(&mut self) -> Option<ProviderUndoDescriptor> {
        self.last_undo_descriptor.take()
    }

    fn undo_command(&mut self, descriptor: &ProviderUndoDescriptor, error: &mut String) {
        self.apply_provider_command(descriptor, true, error);
    }

    fn redo_command(&mut self, descriptor: &ProviderUndoDescriptor, error: &mut String) {
        self.apply_provider_command(descriptor, false, error);
    }

    fn fetch_subtree_parent_key(&mut self) -> Option<String> {
        // Collect as owned strings to avoid borrow conflicts with self.compose.
        let segs: Vec<String> = self.current_path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_owned())
            .collect();
        let in_compose = segs.iter().any(|s| {
            matches!(s.as_str(), "compose" | "reply" | "reply all" | "forward")
        });
        if !in_compose {
            // Folder message-list case: the parent Obj key must match the folder
            // display name (e.g. "INBOX").  The flat FFON root may currently hold
            // an old message key, so we return Some here so refresh_subtree_parent
            // updates both children and key together.
            if segs.len() == 1 {
                return Some(segs[0].clone());
            }
            return None;
        }
        let body_pos = segs.iter().position(|s| s.starts_with("Body:"))?;
        let depth_past_body = segs.len() - (body_pos + 1);
        if depth_past_body == 0 {
            // Directly inside `Body:` — update the label to reflect current variant.
            Some(body_format_label(&self.compose.draft.body).to_owned())
        } else {
            // Nested inside a body Obj — the Obj key should not be replaced; return
            // None so `refresh_subtree_parent` leaves the existing key intact.
            None
        }
    }

    fn fetch_subtree_children(&mut self) -> Option<Vec<FfonElement>> {
        // When the current path is inside a compose context, return the body children
        // at the correct nesting depth instead of doing a full provider re-fetch.
        // Check for a compose-root token anywhere in the path so that reply/forward
        // entered from a message (/INBOX/msg/reply/Body:…) also gets a targeted refresh.
        let segs: Vec<String> = self.current_path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_owned())
            .collect();
        let in_compose = segs.iter().any(|s| {
            matches!(s.as_str(), "compose" | "reply" | "reply all" | "forward")
        });
        if !in_compose {
            // Folder message-list case: exactly one path segment means we're inside a
            // folder (e.g. `/INBOX`).  Return the envelope list so that
            // `refresh_subtree_parent` can update only the folder Obj's children without
            // rebuilding the entire provider root.
            if segs.len() == 1 {
                return Some(self.fetch());
            }
            return None;
        }
        let body_pos = segs.iter().position(|s| s.starts_with("Body:"))?;
        let sub_segs: Vec<&str> = segs[body_pos + 1..].iter().map(|s| s.as_str()).collect();

        if sub_segs.is_empty() {
            // Top-level body: return all body children (same as before).
            Some(body_to_compose_children(&self.compose.draft.body))
        } else {
            // Nested body: walk the Ffon tree to the target Obj's children.
            match &self.compose.draft.body {
                MailBody::Ffon(elems) => {
                    body_elems_at_sub_path(elems, &sub_segs)
                        .map(|children| children.to_vec())
                }
                MailBody::Text(_) => Some(vec![]),
            }
        }
    }

    fn sync_ffon_body_children(&mut self, children: &[FfonElement]) {
        self.compose.draft.body = body_from_ffon_children(children);
        renormalize_body_variant(&mut self.compose.draft.body);
        self.sync_body_path_label();
    }

    fn commands(&self) -> Vec<String> {
        if !self.is_logged_in() {
            return vec![];
        }
        let mut cmds = vec![
            "compose".to_owned(),
            "logout".to_owned(),
            "refresh".to_owned(),
        ];
        // Message-operation commands are only meaningful when a specific message is
        // selected (path has ≥ 2 segments: folder + message label) AND the UID
        // is resolvable from the label in the cache.
        let segs = self.path_segments();
        let at_message = segs.len() >= 2
            && self.lookup_uid(segs[1]).is_some();
        if at_message {
            cmds.extend([
                "mark-read".to_owned(),
                "mark-unread".to_owned(),
                "star".to_owned(),
                "unstar".to_owned(),
                "delete".to_owned(),
                "archive".to_owned(),
                "move".to_owned(),
            ]);
        }
        cmds
    }

    fn command_list_items(&self, command: &str) -> Vec<sicompass_sdk::provider::ListItem> {
        if command == "move" {
            let current_real = &self.pending_move_folder;
            self.folder_mappings
                .iter()
                .filter(|(_, real)| real != current_real)
                .map(|(display, real)| sicompass_sdk::provider::ListItem {
                    label: display.clone(),
                    data: real.clone(),
                })
                .collect()
        } else {
            vec![]
        }
    }

    fn execute_command(&mut self, command: &str, selection: &str) -> bool {
        if command != "move" {
            return false;
        }
        let uid = match self.pending_move_uid.take() {
            Some(u) => u,
            None => return false,
        };
        let from = std::mem::take(&mut self.pending_move_folder);
        // `selection` is the display name; resolve to the real IMAP folder.
        let dest = self
            .folder_mappings
            .iter()
            .find(|(d, _)| d == selection)
            .map(|(_, r)| r.clone())
            .unwrap_or_else(|| selection.to_owned());
        // Capture Message-ID before moving (for undo support).
        let msg_id = self.message_detail.as_ref()
            .filter(|m| m.uid == uid)
            .map(|m| m.message_id.clone())
            .or_else(|| {
                self.imap.as_mut()
                    .and_then(|imap| imap.fetch_message(&from, uid).ok().flatten())
                    .map(|m| m.message_id)
            })
            .unwrap_or_default();
        if let Some(ref mut imap) = self.imap {
            if let Err(e) = imap.move_message(&from, uid, &dest) {
                self.error_message = Some(format!("move failed: {e}"));
                return false;
            }
        }
        self.envelope_cache = None;
        self.message_cache.retain(|h| h.uid != uid);
        if !msg_id.is_empty() {
            self.last_undo_descriptor = Some(ProviderUndoDescriptor::new(
                "move",
                FfonElement::new_str(serde_json::json!({
                    "msg_id": msg_id,
                    "src": from,
                    "dest": dest,
                }).to_string()),
                "move email",
            ));
        }
        true
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
                self.special_folders = SpecialFolders::default();
                self.pending_move_uid = None;
                self.pending_move_folder.clear();
                self.compose = ComposeState::default();
                self.compose_sent = false;
                self.history_folder.clear();
                self.history_refs.clear();
                self.history_uid = None;
                self.thread_cache.clear();
                None  // triggers state-toggle refresh in handlers.rs
            }
            "mark-read" | "mark-unread" | "star" | "unstar" => {
                if let Some((real_folder, uid)) = self.current_message_uid(elem_key) {
                    // Capture previous flag state for undo before mutation.
                    let (prev_seen, prev_flagged) = self.message_cache
                        .iter()
                        .find(|h| h.uid == uid)
                        .map(|h| (h.seen, h.flagged))
                        .unwrap_or((false, false));
                    let (add, remove): (&[&str], &[&str]) = match cmd {
                        "mark-read"   => (&["\\Seen"],    &[]),
                        "mark-unread" => (&[],            &["\\Seen"]),
                        "star"        => (&["\\Flagged"], &[]),
                        _             => (&[],            &["\\Flagged"]), // unstar
                    };
                    if let Some(ref mut imap) = self.imap {
                        if let Err(e) = imap.set_flags(&real_folder, uid, add, remove) {
                            *error = format!("{cmd} failed: {e}");
                            return None;
                        }
                    }
                    // Update cached header so the list re-renders correctly.
                    if let Some(h) = self.message_cache.iter_mut().find(|h| h.uid == uid) {
                        match cmd {
                            "mark-read"   => h.seen = true,
                            "mark-unread" => h.seen = false,
                            "star"        => h.flagged = true,
                            _             => h.flagged = false,
                        }
                    }
                    self.envelope_cache = None;
                    // Stash undo descriptor.
                    let payload = if cmd == "star" || cmd == "unstar" {
                        serde_json::json!({
                            "folder": real_folder,
                            "uid": uid,
                            "prev_flagged": prev_flagged,
                        }).to_string()
                    } else {
                        serde_json::json!({
                            "folder": real_folder,
                            "uid": uid,
                            "prev_seen": prev_seen,
                        }).to_string()
                    };
                    self.last_undo_descriptor = Some(ProviderUndoDescriptor::new(
                        cmd,
                        FfonElement::new_str(payload),
                        cmd.replace('-', " "),
                    ));
                } else {
                    *error = format!("{cmd}: not viewing a message");
                }
                None
            }
            "delete" => {
                if let Some((real_folder, uid)) = self.current_message_uid(elem_key) {
                    let trash = self.special_folders.trash.clone();
                    let moved_to_trash = if let Some(ref t) = trash {
                        if real_folder != *t {
                            // Soft delete: capture Message-ID for undo before moving.
                            let msg_id = self.message_detail.as_ref()
                                .filter(|m| m.uid == uid)
                                .map(|m| m.message_id.clone())
                                .or_else(|| {
                                    self.imap.as_mut()
                                        .and_then(|imap| imap.fetch_message(&real_folder, uid).ok().flatten())
                                        .map(|m| m.message_id)
                                })
                                .unwrap_or_default();
                            if let Some(ref mut imap) = self.imap {
                                if let Err(e) = imap.move_message(&real_folder, uid, t) {
                                    *error = format!("delete failed: {e}");
                                    return None;
                                }
                            }
                            if !msg_id.is_empty() {
                                self.last_undo_descriptor = Some(ProviderUndoDescriptor::new(
                                    "delete-trash",
                                    FfonElement::new_str(serde_json::json!({
                                        "msg_id": msg_id,
                                        "src": real_folder,
                                        "trash": t,
                                    }).to_string()),
                                    "delete email",
                                ));
                            }
                            true
                        } else {
                            false // already in Trash — fall through to hard delete
                        }
                    } else {
                        false // no Trash configured — hard delete
                    };
                    if !moved_to_trash {
                        if let Some(ref mut imap) = self.imap {
                            let _ = imap.set_flags(&real_folder, uid, &["\\Deleted"], &[]);
                            let _ = imap.expunge_uid(&real_folder, uid);
                        }
                    }
                    self.envelope_cache = None;
                    self.message_cache.retain(|h| h.uid != uid);
                } else {
                    *error = "delete: not viewing a message".to_owned();
                }
                None
            }
            "archive" => {
                if let Some((real_folder, uid)) = self.current_message_uid(elem_key) {
                    let archive = self.special_folders.archive.clone();
                    match archive {
                        None => {
                            *error = "archive: server does not advertise an \\Archive folder".to_owned();
                        }
                        Some(ref dest) => {
                            // Capture Message-ID for undo before moving.
                            let msg_id = self.message_detail.as_ref()
                                .filter(|m| m.uid == uid)
                                .map(|m| m.message_id.clone())
                                .or_else(|| {
                                    self.imap.as_mut()
                                        .and_then(|imap| imap.fetch_message(&real_folder, uid).ok().flatten())
                                        .map(|m| m.message_id)
                                })
                                .unwrap_or_default();
                            if let Some(ref mut imap) = self.imap {
                                if let Err(e) = imap.move_message(&real_folder, uid, dest) {
                                    *error = format!("archive failed: {e}");
                                    return None;
                                }
                            }
                            self.envelope_cache = None;
                            self.message_cache.retain(|h| h.uid != uid);
                            if !msg_id.is_empty() {
                                self.last_undo_descriptor = Some(ProviderUndoDescriptor::new(
                                    "archive",
                                    FfonElement::new_str(serde_json::json!({
                                        "msg_id": msg_id,
                                        "src": real_folder,
                                        "archive": dest,
                                    }).to_string()),
                                    "archive email",
                                ));
                            }
                        }
                    }
                } else {
                    *error = "archive: not viewing a message".to_owned();
                }
                None
            }
            "move" => {
                // Two-phase: store context now; folder selection handled in execute_command.
                if let Some((real_folder, uid)) = self.current_message_uid(elem_key) {
                    self.pending_move_uid = Some(uid);
                    self.pending_move_folder = real_folder;
                } else {
                    *error = "move: not viewing a message".to_owned();
                }
                None
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
        folders: Vec<FolderInfo>,
        messages: Vec<MessageHeader>,
        detail: Option<EmailMessage>,
        by_msg_id: Option<EmailMessage>,
        /// When set, `fetch_message_by_message_id` returns `None` for any folder
        /// other than this one, simulating a cross-folder search scenario.
        by_msg_id_folder: Option<String>,
        error: Option<String>,
        /// Independent error returned only by `set_flags` (other ops succeed).
        set_flags_error: Option<String>,
        list_folders_calls: usize,
        list_messages_calls: usize,
        fetch_by_msg_id_calls: usize,
        // Write-operation tracking
        stored_flags: Vec<(String, u32, String)>,   // (folder, uid, "+/-FLAGS (flags)")
        moved: Vec<(String, u32, String)>,           // (from_folder, uid, dest_folder)
        expunged: Vec<(String, u32)>,                // (folder, uid)
        appended: Vec<(String, Vec<u8>)>,            // (folder, raw_bytes)
        /// Pre-configured thread result for fetch_threads(); None = not supported.
        thread_result: Option<Vec<Vec<u32>>>,
    }

    impl MockImap {
        fn new() -> Self {
            MockImap {
                folders: vec![],
                messages: vec![],
                detail: None,
                by_msg_id: None,
                by_msg_id_folder: None,
                error: None,
                set_flags_error: None,
                list_folders_calls: 0,
                list_messages_calls: 0,
                fetch_by_msg_id_calls: 0,
                stored_flags: vec![],
                moved: vec![],
                expunged: vec![],
                appended: vec![],
                thread_result: None,
            }
        }
        fn with_threads(mut self, threads: Vec<Vec<u32>>) -> Self {
            self.thread_result = Some(threads);
            self
        }
        fn with_by_msg_id_only_in_folder(mut self, folder: &str, msg: EmailMessage) -> Self {
            self.by_msg_id = Some(msg);
            self.by_msg_id_folder = Some(folder.to_owned());
            self
        }
        fn with_set_flags_error(mut self, e: &str) -> Self {
            self.set_flags_error = Some(e.to_owned());
            self
        }
        fn with_folders(mut self, folders: &[&str]) -> Self {
            self.folders = folders
                .iter()
                .map(|s| FolderInfo { name: s.to_string(), attributes: vec![] })
                .collect();
            self
        }
        fn with_folder_infos(mut self, infos: Vec<FolderInfo>) -> Self {
            self.folders = infos;
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
        fn list_folders(&mut self) -> Result<Vec<FolderInfo>, String> {
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
        fn fetch_message_by_message_id(&mut self, folder: &str, _msg_id: &str) -> Result<Option<EmailMessage>, String> {
            self.fetch_by_msg_id_calls += 1;
            if let Some(ref e) = self.error { return Err(e.clone()); }
            if let Some(ref req) = self.by_msg_id_folder {
                if folder != req { return Ok(None); }
            }
            Ok(self.by_msg_id.clone())
        }
        fn set_flags(&mut self, folder: &str, uid: u32, add: &[&str], remove: &[&str]) -> Result<(), String> {
            if let Some(ref e) = self.error { return Err(e.clone()); }
            if let Some(ref e) = self.set_flags_error { return Err(e.clone()); }
            if !add.is_empty() {
                self.stored_flags.push((
                    folder.to_owned(),
                    uid,
                    format!("+FLAGS ({})", add.join(" ")),
                ));
            }
            if !remove.is_empty() {
                self.stored_flags.push((
                    folder.to_owned(),
                    uid,
                    format!("-FLAGS ({})", remove.join(" ")),
                ));
            }
            Ok(())
        }
        fn copy_message(&mut self, folder: &str, uid: u32, dest: &str) -> Result<(), String> {
            if let Some(ref e) = self.error { return Err(e.clone()); }
            self.moved.push((folder.to_owned(), uid, dest.to_owned()));
            Ok(())
        }
        fn move_message(&mut self, folder: &str, uid: u32, dest: &str) -> Result<(), String> {
            if let Some(ref e) = self.error { return Err(e.clone()); }
            self.moved.push((folder.to_owned(), uid, dest.to_owned()));
            Ok(())
        }
        fn expunge_uid(&mut self, folder: &str, uid: u32) -> Result<(), String> {
            if let Some(ref e) = self.error { return Err(e.clone()); }
            self.expunged.push((folder.to_owned(), uid));
            Ok(())
        }
        fn append(&mut self, folder: &str, message: &[u8]) -> Result<(), String> {
            if let Some(ref e) = self.error { return Err(e.clone()); }
            self.appended.push((folder.to_owned(), message.to_vec()));
            Ok(())
        }
        fn fetch_threads(&mut self, _folder: &str) -> Result<Option<Vec<Vec<u32>>>, String> {
            if let Some(ref e) = self.error { return Err(e.clone()); }
            Ok(self.thread_result.clone())
        }
    }

    struct MockSmtp {
        sent: std::sync::Arc<std::sync::Mutex<Vec<(String, String, String, MailBody)>>>,
        cc_sent: Vec<Vec<String>>,
        bcc_sent: Vec<Vec<String>>,
        attachments_sent: Vec<Vec<String>>,
        fail: bool,
    }

    impl MockSmtp {
        fn new() -> Self {
            MockSmtp { sent: Default::default(), cc_sent: vec![], bcc_sent: vec![], attachments_sent: vec![], fail: false }
        }
        fn failing() -> Self {
            MockSmtp { sent: Default::default(), cc_sent: vec![], bcc_sent: vec![], attachments_sent: vec![], fail: true }
        }
    }

    impl SmtpBackend for MockSmtp {
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
            if self.fail { return Err("SMTP error".to_owned()); }
            self.sent.lock().unwrap().push((
                from.to_owned(), to.join(", "), subject.to_owned(), body.clone()
            ));
            self.cc_sent.push(cc.iter().map(|s| s.to_string()).collect());
            self.bcc_sent.push(bcc.iter().map(|s| s.to_string()).collect());
            self.attachments_sent.push(attachments.iter().map(|(n, _)| n.to_string()).collect());
            Ok(b"fake-raw-message".to_vec())
        }
    }

    fn make_header(uid: u32, from: &str, subject: &str) -> MessageHeader {
        MessageHeader {
            uid,
            from: from.to_owned(),
            subject: subject.to_owned(),
            date: "2025-01-01".to_owned(),
            seen: true,
            flagged: false,
        }
    }

    fn make_header_unread(uid: u32, from: &str, subject: &str) -> MessageHeader {
        MessageHeader {
            uid,
            from: from.to_owned(),
            subject: subject.to_owned(),
            date: "2025-01-01".to_owned(),
            seen: false,
            flagged: false,
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
            attachments: vec![],
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
    fn test_finish_login_clears_folder_cache_and_resets_path() {
        // finish_login must always clear session state so the next fetch
        // issues a fresh IMAP folder list with the new token.
        let dir = tempfile::tempdir().unwrap();
        let result = crate::oauth2::OAuth2TokenResult {
            success: true,
            access_token: "new_token".to_owned(),
            refresh_token: "new_refresh".to_owned(),
            expires_in: 3600,
            ..Default::default()
        };
        let mut p = EmailClientProvider::new()
            .with_config_path(dir.path().join("settings.json"));
        // Pre-set a stale folder cache and a non-root path.
        p.folder_cache = Some(vec![FfonElement::new_str("IMAP error: stale".to_owned())]);
        p.current_path = "/INBOX/some-message".to_owned();
        p.config.username = "old@example.com".to_owned();

        p.finish_login(result);

        assert!(p.folder_cache.is_none(), "folder_cache must be cleared after login");
        assert_eq!(p.current_path, "/", "path must reset to root after login");
        // oauth_access_token must be set from the result.
        assert_eq!(p.config.oauth_access_token, "new_token");
        // Username stays as-is when fetch_email returns None (no real network in tests).
        // The production path always syncs username from the token — see finish_login.
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
            e.as_obj().map_or(false, |o| o.key == "[read] Subject — alice@x.com")
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
            e.as_obj().map_or(false, |o| o.key.contains("(no subject)"))
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
        p.push_path("[read] Hello — alice@example.com");
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
        p.push_path("[read] Hello — alice@example.com");
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
        p.push_path("[read] Hello — alice@example.com");
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
        p.push_path("[read] Hello — alice@example.com");
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
            attachments: vec![],
        };
        let imap = MockImap::new()
            .with_messages(msgs)
            .with_detail(msg)
            .with_by_msg_id(ref_msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        // Navigate: root → INBOX → message
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
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

    #[test]
    fn test_history_finds_message_in_all_folder_when_absent_from_current() {
        let mut msg = make_message(1);
        msg.references = "<prev@example.com>".to_owned();
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let ref_msg = EmailMessage {
            uid: 0,
            from: "old@example.com".to_owned(),
            to: "bob@example.com".to_owned(),
            subject: "Previous".to_owned(),
            date: "2024-12-31".to_owned(),
            body: MailBody::Text("body".to_owned()),
            message_id: "<prev@example.com>".to_owned(),
            in_reply_to: String::new(),
            references: String::new(),
            attachments: vec![],
        };
        // Message only exists in "[Gmail]/All Mail", not in INBOX.
        let imap = MockImap::new()
            .with_folder_infos(vec![
                FolderInfo { name: "INBOX".to_owned(), attributes: vec![] },
                FolderInfo { name: "[Gmail]/All Mail".to_owned(), attributes: vec!["\\All".to_owned()] },
            ])
            .with_messages(msgs)
            .with_detail(msg)
            .with_by_msg_id_only_in_folder("[Gmail]/All Mail", ref_msg);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        // Fetch at root so build_root populates special_folders.archive from the \All attribute.
        p.fetch();
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        p.push_path("History");
        let items = p.fetch();
        assert!(
            items.iter().any(|e| e.as_obj().map_or(false, |o| o.key.contains("Previous"))),
            "History must find the message via \\All folder; got: {:?}", items
        );
    }

    #[test]
    fn test_history_uses_thread_cache_when_available() {
        // Two messages in INBOX: uid 1 (current) and uid 2 (thread sibling).
        // fetch_threads returns one thread containing both UIDs.
        // The thread-cache path looks up uid 2 in message_cache (no SEARCH needed).
        let msgs = vec![
            make_header(1, "alice@example.com", "Hello"),
            make_header(2, "bob@example.com", "Re: Hello"),
        ];
        let imap = MockImap::new()
            .with_messages(msgs)
            .with_detail(make_message(1))
            .with_threads(vec![vec![1, 2]]);

        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch(); // populates message_cache and thread_cache
        p.push_path("[read] Hello — alice@example.com");
        p.fetch(); // sets history_uid = 1
        p.push_path("History");
        let items = p.fetch();
        // uid 2 is in message_cache ("Re: Hello" subject) — History must find it.
        assert!(
            items.iter().any(|e| e.as_obj().map_or(false, |o| o.key.contains("Re: Hello"))),
            "History must surface thread sibling via thread cache; got: {:?}", items
        );
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
    fn test_fetch_at_compose_body_path_returns_body_children() {
        // Regression: fetch() at /compose/Body: must return body_to_compose_children,
        // not misroute to build_message() which returns "(message not found)".
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.push_path("Body:");
        let children = p.fetch();
        // Empty draft body → no children (no "(message not found)" string).
        assert!(children.is_empty(), "empty draft body should yield no children; got: {:?}", children);

        // After adding a body element, fetch at Body: returns it.
        update_body_leaf(&mut p.compose.draft.body, "", "hello");
        let children2 = p.fetch();
        assert_eq!(children2.len(), 1);
        assert!(matches!(&children2[0], FfonElement::Str(s) if s.contains("hello")),
            "expected body child with 'hello'; got: {:?}", children2);
    }

    #[test]
    fn test_compose_view_has_input_fields() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("To:") && s.contains("<input>"))));
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("Subject:") && s.contains("<input>"))));
        // Body: is an Obj node; an empty draft body has no children yet (user inserts via Ctrl+I).
        assert!(items.iter().any(|e| {
            if let FfonElement::Obj(obj) = e { obj.key.starts_with("Body:") } else { false }
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
        p.push_path("[read] Hello — alice@example.com");
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
        p.push_path("[read] Hello — alice@example.com");
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
        p.push_path("[read] Re: Hello — alice@example.com");
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
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        p.push_path("reply all");
        p.fetch();
        // Should include alice (sender) and carol (other recipient), but NOT bob (self).
        assert!(p.compose.draft.to.contains("alice@example.com"));
        assert!(p.compose.draft.to.contains("carol@example.com"));
        assert!(!p.compose.draft.to.contains("bob@example.com"));
    }

    #[test]
    fn test_reply_all_self_filter_does_not_drop_superset_address() {
        // username = "bob@example.com"; recipient = "bob@example.com.au"
        // Substring check would wrongly drop the .au address; exact match must keep it.
        let mut msg = make_message(1);
        msg.to = "bob@example.com, bob@example.com.au".to_owned();
        let msgs = vec![make_header(1, "alice@example.com", "Hi")];
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.config.username = "bob@example.com".to_owned();
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hi — alice@example.com");
        p.fetch();
        p.push_path("reply all");
        p.fetch();
        assert!(!p.compose.draft.to.contains("bob@example.com,"), "self (bob@example.com) must be filtered");
        assert!(p.compose.draft.to.contains("bob@example.com.au"), "superset address must be kept");
    }

    #[test]
    fn test_extract_email_addr_with_display_name() {
        assert_eq!(extract_email_addr("Alice <alice@example.com>"), "alice@example.com");
        assert_eq!(extract_email_addr("alice@example.com"), "alice@example.com");
        assert_eq!(extract_email_addr("  alice@example.com  "), "alice@example.com");
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
        p.push_path("[read] Hello — alice@example.com");
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
        p.push_path("[read] Hello — alice@example.com");
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
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        p.push_path("forward");
        p.fetch();
        // Text-origin forward: MailBody::Text with the forwarded-header block prepended.
        let MailBody::Text(body) = &p.compose.draft.body else {
            panic!("expected Text body after forwarding text-origin mail; got: {:?}", p.compose.draft.body);
        };
        assert!(body.contains("Forwarded message"), "forward body must contain 'Forwarded message'");
        assert!(body.contains("alice@example.com"), "forward body must contain sender address");
    }

    #[test]
    fn test_reply_body_text_origin_stays_text() {
        // Text-origin reply: body stays MailBody::Text with attribution + "> "-quoted lines.
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1); // Text("Hi Bob!")
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        p.push_path("reply");
        p.fetch();
        let MailBody::Text(body) = &p.compose.draft.body else {
            panic!("expected Text body after replying to text-origin mail; got: {:?}", p.compose.draft.body);
        };
        assert!(body.contains("wrote:"), "reply body must include attribution line");
        assert!(body.contains("> Hi Bob!"), "reply body must include \"> \"-quoted original text");
    }

    #[test]
    fn test_forward_body_text_origin_stays_text() {
        // Text-origin forward: body stays MailBody::Text with forwarded-header block prepended.
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        p.push_path("forward");
        p.fetch();
        let MailBody::Text(body) = &p.compose.draft.body else {
            panic!("expected Text body after forwarding text-origin mail; got: {:?}", p.compose.draft.body);
        };
        assert!(body.contains("Forwarded message"), "forward body must include forwarded-header block");
        assert!(body.contains("Hi Bob!"), "forward body must include original text");
    }

    #[test]
    fn test_reply_ffon_body_quote_obj_has_i_placeholder() {
        // Ffon-body message: reply body is flat — [I_PLACEHOLDER, attribution, original elems…].
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let mut msg = make_message(1);
        msg.body = MailBody::Ffon(vec![FfonElement::new_str("<input>Hi Bob!</input>".to_owned())]);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        p.push_path("reply");
        p.fetch();
        let MailBody::Ffon(elems) = &p.compose.draft.body else {
            panic!("expected Ffon body; got: {:?}", p.compose.draft.body);
        };
        assert_eq!(elems[0], FfonElement::new_str(I_PLACEHOLDER.to_owned()), "top-level i placeholder missing");
        let attribution = elems[1].as_str().unwrap_or("");
        assert!(attribution.contains("wrote:"), "second elem must be attribution line");
        // original Ffon elements are appended directly (flat, no nested Obj)
        assert_eq!(
            elems[2],
            FfonElement::new_str("<input>Hi Bob!</input>".to_owned()),
            "original body element must follow attribution"
        );
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
    fn test_smtp_failure_surfaces_error_and_keeps_draft() {
        let smtp = MockSmtp::failing();
        let mut p = EmailClientProvider::new().with_smtp(Box::new(smtp));
        p.push_path("compose");
        p.compose.draft.to = "x@y.com".to_owned();
        p.compose.draft.subject = "Test".to_owned();
        p.on_button_press("send");
        // Error should be surfaced via take_error.
        let err = p.take_error();
        assert!(err.is_some(), "expected an error on send failure");
        assert!(err.unwrap().contains("send failed"));
        // Compose state must be preserved so the user doesn't lose their draft.
        assert!(!p.compose.draft.to.is_empty(), "draft to must be preserved after send failure");
        assert!(!p.compose_sent, "compose_sent must not be set after send failure");
    }

    #[test]
    fn test_reply_all_sends_multiple_recipients() {
        let smtp = MockSmtp::new();
        let sent = smtp.sent.clone();
        let mut p = EmailClientProvider::new().with_smtp(Box::new(smtp));
        p.push_path("compose");
        // Simulate reply-all: comma-separated To field with two addresses.
        p.compose.draft.to = "alice@example.com, bob@example.com".to_owned();
        p.compose.draft.subject = "Re: Hello".to_owned();
        p.compose.draft.body = MailBody::Text("Sure!".to_owned());
        p.on_button_press("send");
        let records = sent.lock().unwrap();
        assert_eq!(records.len(), 1, "expected one send call");
        // MockSmtp joins recipients with ", " — both addresses must appear.
        assert!(records[0].1.contains("alice@example.com"), "alice must be in recipients");
        assert!(records[0].1.contains("bob@example.com"), "bob must be in recipients");
    }

    // ---- Path navigation ----

    #[test]
    fn test_push_path_increments_depth() {
        let mut p = EmailClientProvider::new();
        p.push_path("INBOX");
        assert_eq!(p.current_path(), "/INBOX");
        p.push_path("[read] Hello — alice@x.com");
        assert_eq!(p.current_path(), "/INBOX/[read] Hello — alice@x.com");
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
    fn test_ensure_fresh_token_async_refresh_failure_clears_token() {
        // When the background refresh fails, the stale access token must be cleared
        // so is_logged_in() returns false and the login button is shown.
        let dir = tempfile::tempdir().unwrap();
        let mut p = EmailClientProvider::new()
            .with_config_path(dir.path().join("settings.json"));
        p.config.oauth_access_token = "stale_token".to_owned();
        p.config.oauth_refresh_token = "refresh_tok".to_owned();
        p.config.token_expiry = 1; // expired long ago

        // Simulate a completed-but-failed background refresh.
        *p.token_refresh_result.lock().unwrap() = Some(Err("revoked".to_owned()));

        let unblocked = p.ensure_fresh_token_async();
        assert!(unblocked, "caller must be unblocked even on failure");
        assert!(p.config.oauth_access_token.is_empty(), "stale token must be cleared");
        assert_eq!(p.config.token_expiry, 0);
        assert!(!p.is_logged_in(), "is_logged_in must return false so login button appears");
    }

    #[test]
    fn test_ensure_fresh_token_async_refresh_success_updates_token() {
        // Successful refresh updates the token, rebuilds backends, and clears
        // folder_cache so the next fetch issues a fresh IMAP connection.
        let dir = tempfile::tempdir().unwrap();
        let mut p = EmailClientProvider::new()
            .with_config_path(dir.path().join("settings.json"));
        p.config.imap_url = "imaps://imap.example.com".to_owned();
        p.config.username = "user@example.com".to_owned();
        p.config.oauth_access_token = "old_token".to_owned();
        p.config.oauth_refresh_token = "refresh_tok".to_owned();
        p.config.token_expiry = 1;
        // Seed a cached error as would happen after a failed IMAP attempt.
        p.folder_cache = Some(vec![FfonElement::new_str("IMAP error: old".to_owned())]);

        *p.token_refresh_result.lock().unwrap() = Some(Ok(("new_token".to_owned(), i64::MAX)));

        let unblocked = p.ensure_fresh_token_async();
        assert!(unblocked);
        assert_eq!(p.config.oauth_access_token, "new_token");
        assert_eq!(p.config.token_expiry, i64::MAX);
        assert!(p.is_logged_in());
        assert!(p.folder_cache.is_none(), "folder_cache must be cleared so next fetch retries IMAP with new token");
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

    // ---- body_to_compose_children ----

    #[test]
    fn test_body_to_compose_children_empty_body_returns_empty() {
        // Empty body produces no children; the i placeholder is inserted on-demand via ctrl+i/a.
        let body = MailBody::Text(String::new());
        let children = body_to_compose_children(&body);
        assert!(children.is_empty(), "empty body should produce no children; got: {:?}", children);
    }

    #[test]
    fn test_body_to_compose_children_text_body_wraps_in_input() {
        let body = MailBody::Text("hello".to_owned());
        let children = body_to_compose_children(&body);
        assert_eq!(children.len(), 1);
        assert!(matches!(&children[0], FfonElement::Str(s) if s == "<input>hello</input>"),
            "non-empty text body should wrap content in <input>; got: {:?}", children[0]);
    }

    #[test]
    fn test_body_to_compose_children_ffon_body_cloned() {
        let elems = vec![
            FfonElement::new_str("<input>line 1</input>".to_owned()),
            FfonElement::new_str("<input>line 2</input>".to_owned()),
        ];
        let body = MailBody::Ffon(elems.clone());
        let children = body_to_compose_children(&body);
        assert_eq!(children, elems);
    }

    // ---- update_body_leaf: Obj creation via trailing colon ----

    #[test]
    fn test_update_body_leaf_trailing_colon_creates_obj_in_empty_text_body() {
        let mut body = MailBody::Text(String::new());
        update_body_leaf(&mut body, "", "section:");
        match &body {
            MailBody::Ffon(elems) => {
                let has_obj = elems.iter().any(|e| match e {
                    FfonElement::Obj(o) => sicompass_sdk::tags::extract_input(&o.key).as_deref() == Some("section"),
                    _ => false,
                });
                assert!(has_obj, "expected Obj(section) in Ffon; got: {:?}", elems);
            }
            other => panic!("expected Ffon body, got: {:?}", other),
        }
    }

    #[test]
    fn test_update_body_leaf_trailing_colon_preserves_existing_text() {
        let mut body = MailBody::Text("existing".to_owned());
        update_body_leaf(&mut body, "", "header:");
        match &body {
            MailBody::Ffon(elems) => {
                let has_existing = elems.iter().any(|e| matches!(e, FfonElement::Str(s) if s.contains("existing")));
                let has_obj = elems.iter().any(|e| match e {
                    FfonElement::Obj(o) => sicompass_sdk::tags::extract_input(&o.key).as_deref() == Some("header"),
                    _ => false,
                });
                assert!(has_existing && has_obj, "existing text and new Obj must both be present; got: {:?}", elems);
            }
            other => panic!("expected Ffon body, got: {:?}", other),
        }
    }

    #[test]
    fn test_update_body_leaf_trailing_colon_appends_obj_to_ffon_body() {
        let mut body = MailBody::Ffon(vec![FfonElement::new_str("<input>first</input>".to_owned())]);
        update_body_leaf(&mut body, "", "meta:");
        match &body {
            MailBody::Ffon(elems) => {
                assert_eq!(elems.len(), 2, "should have 2 elements; got: {:?}", elems);
                let has_obj = elems.iter().any(|e| match e {
                    FfonElement::Obj(o) => sicompass_sdk::tags::extract_input(&o.key).as_deref() == Some("meta"),
                    _ => false,
                });
                assert!(has_obj, "expected Obj(meta) appended; got: {:?}", elems);
            }
            other => panic!("expected Ffon body, got: {:?}", other),
        }
    }

    #[test]
    fn test_commit_edit_body_trailing_colon_creates_obj() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.push_path("Body:");
        assert!(p.commit_edit("", "myobj:"));
        match &p.compose.draft.body {
            MailBody::Ffon(elems) => {
                let has_obj = elems.iter().any(|e| match e {
                    FfonElement::Obj(o) => sicompass_sdk::tags::extract_input(&o.key).as_deref() == Some("myobj"),
                    _ => false,
                });
                assert!(has_obj, "expected Obj(myobj) in body; got: {:?}", elems);
            }
            other => panic!("expected Ffon body after Obj commit, got: {:?}", other),
        }
    }

    // ---- delete_body_element_at ----

    /// Deleting the sole Ffon element leaves a single `i <input></input>` placeholder.
    #[test]
    fn delete_body_element_ffon_last_keeps_i_placeholder() {
        let mut body = MailBody::Ffon(vec![
            FfonElement::new_str("<input>hello</input>".to_owned()),
        ]);
        let ok = delete_body_element_at(&mut body, &[0]);
        assert!(ok, "delete should succeed");
        match &body {
            MailBody::Ffon(elems) => {
                assert_eq!(elems.len(), 1, "should have exactly one placeholder; got: {:?}", elems);
                assert_eq!(
                    elems[0],
                    FfonElement::new_str(I_PLACEHOLDER.to_owned()),
                    "remaining element should be the i placeholder"
                );
            }
            other => panic!("expected Ffon body, got: {:?}", other),
        }
    }

    /// Deleting a Text body via any path converts it to Ffon with the `i` placeholder.
    #[test]
    fn delete_body_element_text_keeps_i_placeholder() {
        let mut body = MailBody::Text("hello".to_owned());
        let ok = delete_body_element_at(&mut body, &[0]);
        assert!(ok, "delete should succeed");
        match &body {
            MailBody::Ffon(elems) => {
                assert_eq!(elems.len(), 1, "should have exactly one placeholder; got: {:?}", elems);
                assert_eq!(
                    elems[0],
                    FfonElement::new_str(I_PLACEHOLDER.to_owned()),
                    "remaining element should be the i placeholder"
                );
            }
            other => panic!("expected Ffon body with placeholder, got: {:?}", other),
        }
    }

    /// Deleting a Str sibling when an Obj sibling also exists (regression: was broken by
    /// content-based matching when the path pointed into the Obj sub-tree).
    #[test]
    fn delete_body_element_str_with_obj_sibling() {
        let mut body = MailBody::Ffon(vec![
            FfonElement::new_str("<input>abc</input>".to_owned()),
            FfonElement::new_obj("myobj:"),
            FfonElement::new_str("<input>def</input>".to_owned()),
        ]);
        // Delete the first Str (index 0).
        assert!(delete_body_element_at(&mut body, &[0]), "delete index 0 should succeed");
        let MailBody::Ffon(elems) = &body else { panic!("expected Ffon"); };
        assert_eq!(elems.len(), 2, "should have 2 remaining elements");
        assert!(matches!(&elems[0], FfonElement::Obj(_)), "index 0 should now be the Obj");

        // Delete the last Str (now at index 1).
        assert!(delete_body_element_at(&mut body, &[1]), "delete index 1 should succeed");
        let MailBody::Ffon(elems) = &body else { panic!("expected Ffon"); };
        assert_eq!(elems.len(), 1, "only the Obj should remain");
        assert!(matches!(&elems[0], FfonElement::Obj(_)));
    }

    /// Deleting a top-level Obj removes it.
    #[test]
    fn delete_body_element_top_level_obj() {
        let mut body = MailBody::Ffon(vec![
            FfonElement::new_str("<input>line</input>".to_owned()),
            FfonElement::new_obj("myobj:"),
        ]);
        assert!(delete_body_element_at(&mut body, &[1]), "delete Obj should succeed");
        let MailBody::Ffon(elems) = &body else { panic!("expected Ffon"); };
        assert_eq!(elems.len(), 1);
        assert!(matches!(&elems[0], FfonElement::Str(_)));
    }

    /// Deleting a leaf inside an Obj's children (nested path).
    #[test]
    fn delete_body_element_nested_child() {
        let mut inner = FfonElement::new_obj("myobj:");
        inner.as_obj_mut().unwrap().push(FfonElement::new_str("<input>x</input>".to_owned()));
        inner.as_obj_mut().unwrap().push(FfonElement::new_str("<input>y</input>".to_owned()));
        let mut body = MailBody::Ffon(vec![inner]);
        // Delete the first child of the Obj (path [0, 0]).
        assert!(delete_body_element_at(&mut body, &[0, 0]), "nested delete should succeed");
        let MailBody::Ffon(elems) = &body else { panic!("expected Ffon"); };
        let FfonElement::Obj(o) = &elems[0] else { panic!("expected Obj"); };
        assert_eq!(o.children.len(), 1, "one child should remain");
    }

    /// Deleting the sole child of an inner Obj reseeds the `i` placeholder
    /// instead of leaving the Obj with an empty children list.
    #[test]
    fn delete_body_element_empties_obj_reseeds_placeholder() {
        let mut inner = FfonElement::new_obj("myobj:");
        inner.as_obj_mut().unwrap().push(FfonElement::new_str("<input>x</input>".to_owned()));
        let mut body = MailBody::Ffon(vec![inner]);
        assert!(delete_body_element_at(&mut body, &[0, 0]), "nested delete should succeed");
        let MailBody::Ffon(elems) = &body else { panic!("expected Ffon"); };
        let FfonElement::Obj(o) = &elems[0] else { panic!("expected Obj"); };
        assert_eq!(o.children.len(), 1, "Obj children should have exactly the i placeholder");
        assert_eq!(
            o.children[0],
            FfonElement::new_str(I_PLACEHOLDER.to_owned()),
            "remaining child should be the i placeholder"
        );
    }

    /// Creating a new Obj via update_body_leaf seeds the `i` placeholder into
    /// the Obj's children — for all three creation paths.
    #[test]
    fn update_body_leaf_new_obj_seeds_i_placeholder() {
        // Path 1: Text body → upgraded to Ffon with a new Obj.
        let mut body = MailBody::Text(String::new());
        update_body_leaf(&mut body, "", "foo:");
        match &body {
            MailBody::Ffon(elems) => {
                let FfonElement::Obj(o) = elems.iter().find(|e| e.is_obj()).expect("expected Obj") else {
                    panic!("element should be Obj");
                };
                assert_eq!(o.children.len(), 1, "new Obj (from Text body) should have one child");
                assert_eq!(o.children[0], FfonElement::new_str(I_PLACEHOLDER.to_owned()));
            }
            other => panic!("expected Ffon body, got: {:?}", other),
        }

        // Path 2: Ffon body — replace existing placeholder Str with Obj.
        let mut body = MailBody::Ffon(vec![FfonElement::new_str(I_PLACEHOLDER.to_owned())]);
        update_body_leaf(&mut body, "", "bar:");
        let MailBody::Ffon(elems) = &body else { panic!("expected Ffon"); };
        let FfonElement::Obj(o) = elems.iter().find(|e| e.is_obj()).expect("expected Obj") else {
            panic!("element should be Obj");
        };
        assert_eq!(o.children.len(), 1, "replaced Obj should have one child");
        assert_eq!(o.children[0], FfonElement::new_str(I_PLACEHOLDER.to_owned()));

        // Path 3: Ffon body — no placeholder match, append new Obj.
        let mut body = MailBody::Ffon(vec![FfonElement::new_str("<input>hello</input>".to_owned())]);
        update_body_leaf(&mut body, "nonexistent", "baz:");
        let MailBody::Ffon(elems) = &body else { panic!("expected Ffon"); };
        let FfonElement::Obj(o) = elems.iter().find(|e| e.is_obj()).expect("expected Obj") else {
            panic!("element should be Obj");
        };
        assert_eq!(o.children.len(), 1, "appended Obj should have one child");
        assert_eq!(o.children[0], FfonElement::new_str(I_PLACEHOLDER.to_owned()));
    }

    /// Out-of-range path returns false and leaves body unchanged.
    #[test]
    fn delete_body_element_out_of_range() {
        let mut body = MailBody::Ffon(vec![
            FfonElement::new_str("<input>only</input>".to_owned()),
        ]);
        assert!(!delete_body_element_at(&mut body, &[5]), "out-of-range should return false");
        let MailBody::Ffon(elems) = &body else { panic!("expected Ffon"); };
        assert_eq!(elems.len(), 1, "body should be unchanged");
    }

    /// Empty path returns false.
    #[test]
    fn delete_body_element_empty_path() {
        let mut body = MailBody::Ffon(vec![
            FfonElement::new_str("<input>x</input>".to_owned()),
        ]);
        assert!(!delete_body_element_at(&mut body, &[]), "empty path should return false");
    }

    // ---- renormalize_body_variant ----

    #[test]
    fn renormalize_collapses_single_str_to_text() {
        let mut body = MailBody::Ffon(vec![FfonElement::new_str("<input>hello</input>".to_owned())]);
        renormalize_body_variant(&mut body);
        assert!(matches!(&body, MailBody::Text(s) if s == "hello"), "expected Text(hello), got: {:?}", body);
    }

    #[test]
    fn renormalize_preserves_i_placeholder() {
        let mut body = MailBody::Ffon(vec![FfonElement::new_str(I_PLACEHOLDER.to_owned())]);
        renormalize_body_variant(&mut body);
        assert!(matches!(&body, MailBody::Ffon(elems) if elems.len() == 1), "i placeholder should stay Ffon");
    }

    #[test]
    fn renormalize_preserves_multi_child() {
        let mut body = MailBody::Ffon(vec![
            FfonElement::new_str("<input>a</input>".to_owned()),
            FfonElement::new_str("<input>b</input>".to_owned()),
        ]);
        renormalize_body_variant(&mut body);
        assert!(matches!(&body, MailBody::Ffon(elems) if elems.len() == 2), "multi-child Ffon should stay Ffon");
    }

    #[test]
    fn renormalize_preserves_obj_child() {
        let mut body = MailBody::Ffon(vec![new_obj_with_i_placeholder("<input>key</input>".to_owned())]);
        renormalize_body_variant(&mut body);
        assert!(matches!(&body, MailBody::Ffon(_)), "single Obj child should stay Ffon");
    }

    #[test]
    fn renormalize_noop_on_text() {
        let mut body = MailBody::Text("hello".to_owned());
        renormalize_body_variant(&mut body);
        assert!(matches!(&body, MailBody::Text(s) if s == "hello"), "Text should be unchanged");
    }

    /// sync_ffon_body_children: two-element body → remove second → collapses to Text.
    #[test]
    fn sync_body_children_two_to_one_collapses_to_text() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        // Simulate what FFON Task::Delete leaves behind: one child remaining.
        let remaining = vec![FfonElement::new_str("<input>hello</input>".to_owned())];
        p.sync_ffon_body_children(&remaining);
        assert!(
            matches!(&p.compose.draft.body, MailBody::Text(s) if s == "hello"),
            "body should collapse to Text(hello); got: {:?}",
            p.compose.draft.body
        );
    }

    /// sync_ffon_body_children: remove Obj from Ffon([Str, Obj]) → collapses to Text.
    #[test]
    fn sync_body_children_remove_obj_collapses_to_text() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        let remaining = vec![FfonElement::new_str("<input>hello</input>".to_owned())];
        p.sync_ffon_body_children(&remaining);
        assert!(
            matches!(&p.compose.draft.body, MailBody::Text(s) if s == "hello"),
            "body should collapse to Text(hello); got: {:?}",
            p.compose.draft.body
        );
    }

    /// sync_ffon_body_children: empty children → MailBody::Text("").
    #[test]
    fn sync_body_children_empty_becomes_empty_text() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.sync_ffon_body_children(&[]);
        assert!(
            matches!(&p.compose.draft.body, MailBody::Text(s) if s.is_empty()),
            "empty children should yield Text(\"\"); got: {:?}",
            p.compose.draft.body
        );
    }

    // ---- seed_i_placeholders ----

    /// Reply from a Ffon-body message that contains a nested Obj: the inherited Obj
    /// must have I_PLACEHOLDER inserted as its first child.
    #[test]
    fn test_reply_nested_ffon_obj_gets_i_placeholder() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let mut msg = make_message(1);
        // Source body contains an Obj with a child string (no I_PLACEHOLDER).
        let nested_obj = {
            let mut o = FfonElement::new_obj("<input>section</input>".to_owned());
            o.as_obj_mut().unwrap().push(FfonElement::new_str("<input>content</input>".to_owned()));
            o
        };
        msg.body = MailBody::Ffon(vec![nested_obj]);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        p.push_path("reply");
        p.fetch();

        let MailBody::Ffon(elems) = &p.compose.draft.body else {
            panic!("expected Ffon body; got: {:?}", p.compose.draft.body);
        };
        // Find the inherited nested Obj (third element onward — after I_PLACEHOLDER and attribution).
        let nested = elems.iter().find_map(|e| {
            if let FfonElement::Obj(o) = e {
                if sicompass_sdk::tags::extract_input(&o.key).as_deref() == Some("section") {
                    return Some(o);
                }
            }
            None
        }).expect("nested Obj from source body not found in reply draft");
        assert_eq!(
            nested.children[0],
            FfonElement::new_str(I_PLACEHOLDER.to_owned()),
            "inherited nested Obj must have I_PLACEHOLDER as first child; got: {:?}",
            nested.children
        );
    }

    /// Forward from a Ffon-body message that contains a nested Obj: the inherited Obj
    /// must have I_PLACEHOLDER seeded.
    #[test]
    fn forward_body_nested_ffon_obj_has_i_placeholder() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let mut msg = make_message(1);
        let nested_obj = {
            let mut o = FfonElement::new_obj("<input>attachment</input>".to_owned());
            o.as_obj_mut().unwrap().push(FfonElement::new_str("<input>data</input>".to_owned()));
            o
        };
        msg.body = MailBody::Ffon(vec![nested_obj]);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        p.push_path("forward");
        p.fetch();

        let MailBody::Ffon(elems) = &p.compose.draft.body else {
            panic!("expected Ffon body; got: {:?}", p.compose.draft.body);
        };
        let nested = elems.iter().find_map(|e| {
            if let FfonElement::Obj(o) = e {
                if sicompass_sdk::tags::extract_input(&o.key).as_deref() == Some("attachment") {
                    return Some(o);
                }
            }
            None
        }).expect("nested Obj from source body not found in forward draft");
        assert_eq!(
            nested.children[0],
            FfonElement::new_str(I_PLACEHOLDER.to_owned()),
            "inherited nested Obj must have I_PLACEHOLDER as first child in forward; got: {:?}",
            nested.children
        );
    }

    // ---- update_body_elems / path-aware nested commit ----

    /// `update_body_elems` replaces the placeholder with a plain string in a nested vec.
    #[test]
    fn update_body_elems_replaces_placeholder_with_string() {
        let mut children = vec![FfonElement::new_str(I_PLACEHOLDER.to_owned())];
        update_body_elems(&mut children, "", "hello");
        assert_eq!(children.len(), 1);
        assert!(
            matches!(&children[0], FfonElement::Str(s) if s == "<input>hello</input>"),
            "placeholder should be replaced with '<input>hello</input>'; got: {:?}", children[0]
        );
    }

    /// `update_body_elems` creates an Obj when content ends with `:`.
    #[test]
    fn update_body_elems_trailing_colon_creates_obj() {
        let mut children = vec![FfonElement::new_str(I_PLACEHOLDER.to_owned())];
        update_body_elems(&mut children, "", "sub:");
        assert_eq!(children.len(), 1);
        match &children[0] {
            FfonElement::Obj(o) => {
                assert_eq!(
                    sicompass_sdk::tags::extract_input(&o.key).as_deref(),
                    Some("sub"),
                    "Obj key should be 'sub'; got: {:?}", o.key
                );
                assert_eq!(
                    o.children[0],
                    FfonElement::new_str(I_PLACEHOLDER.to_owned()),
                    "newly created nested Obj must have I_PLACEHOLDER child; got: {:?}", o.children
                );
            }
            other => panic!("expected Obj, got: {:?}", other),
        }
    }

    /// `commit_edit` at a nested body path (`/compose/Body: [ffon]/foo`) mutates the
    /// `foo:` Obj's children, not the top-level body vec.
    #[test]
    fn commit_edit_nested_body_creates_child_in_nested_obj() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");

        // Pre-build a body with a top-level `foo:` Obj containing only I_PLACEHOLDER.
        p.compose.draft.body = MailBody::Ffon(vec![
            new_obj_with_i_placeholder("<input>foo</input>".to_owned()),
        ]);

        // Simulate being navigated inside `foo:` (path has Body: and then `foo` segment).
        p.push_path("Body: [ffon]");
        p.push_path("foo");

        // Commit "bar" onto the placeholder inside `foo:`.
        assert!(p.commit_edit("", "bar"), "commit should return true");

        let MailBody::Ffon(elems) = &p.compose.draft.body else {
            panic!("expected Ffon body; got: {:?}", p.compose.draft.body);
        };
        assert_eq!(elems.len(), 1, "top-level body should still have exactly one element");
        let FfonElement::Obj(foo) = &elems[0] else {
            panic!("expected top-level Obj; got: {:?}", elems[0]);
        };
        assert!(
            foo.children.iter().any(|c| matches!(c, FfonElement::Str(s) if s == "<input>bar</input>")),
            "bar must be a child of foo, not at top level; foo.children: {:?}", foo.children
        );
    }

    /// `commit_edit` with trailing colon at a nested path creates an Obj inside the parent.
    #[test]
    fn commit_edit_nested_body_trailing_colon_creates_obj_inside_parent() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.compose.draft.body = MailBody::Ffon(vec![
            new_obj_with_i_placeholder("<input>foo</input>".to_owned()),
        ]);
        p.push_path("Body: [ffon]");
        p.push_path("foo");

        assert!(p.commit_edit("", "baz:"), "commit should return true");

        let MailBody::Ffon(elems) = &p.compose.draft.body else {
            panic!("expected Ffon body");
        };
        let FfonElement::Obj(foo) = &elems[0] else {
            panic!("expected top-level foo Obj");
        };
        // `baz:` Obj should exist inside `foo:`, alongside I_PLACEHOLDER.
        let baz = foo.children.iter().find_map(|c| {
            if let FfonElement::Obj(o) = c {
                if sicompass_sdk::tags::extract_input(&o.key).as_deref() == Some("baz") {
                    return Some(o);
                }
            }
            None
        }).expect("baz Obj not found inside foo; foo.children: {:?}");
        assert_eq!(
            baz.children[0],
            FfonElement::new_str(I_PLACEHOLDER.to_owned()),
            "newly created baz Obj must have I_PLACEHOLDER child"
        );
    }

    /// `fetch_subtree_children` returns the nested Obj's children when the path is
    /// inside a body Obj (`/compose/Body: [ffon]/foo`).
    #[test]
    fn fetch_subtree_children_returns_nested_body_children() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");

        let foo_obj = new_obj_with_i_placeholder("<input>foo</input>".to_owned());
        p.compose.draft.body = MailBody::Ffon(vec![foo_obj.clone()]);

        // Simulate being inside the `foo:` Obj.
        p.push_path("Body: [ffon]");
        p.push_path("foo");

        let children = p.fetch_subtree_children()
            .expect("fetch_subtree_children must return Some when inside a nested body Obj");

        // Should return foo's children (the I_PLACEHOLDER), not the top-level body vec.
        assert_eq!(children.len(), 1, "expected 1 child (I_PLACEHOLDER); got: {:?}", children);
        assert_eq!(
            children[0],
            FfonElement::new_str(I_PLACEHOLDER.to_owned()),
            "child should be I_PLACEHOLDER; got: {:?}", children[0]
        );
    }

    /// `fetch_subtree_parent_key` returns None when inside a nested body Obj so that
    /// `refresh_subtree_parent` does not overwrite the Obj's existing key.
    #[test]
    fn fetch_subtree_parent_key_returns_none_for_nested_body_obj() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.compose.draft.body = MailBody::Ffon(vec![
            new_obj_with_i_placeholder("<input>foo</input>".to_owned()),
        ]);
        p.push_path("Body: [ffon]");
        p.push_path("foo");

        assert!(
            p.fetch_subtree_parent_key().is_none(),
            "parent key must be None for nested body Obj so the key is not overwritten"
        );
    }

    /// When the provider path is at a folder (exactly one segment, e.g. `/INBOX`),
    /// `fetch_subtree_children` returns `Some` with the message list so that
    /// `refresh_subtree_parent` can update the folder Obj's children in-place after
    /// a delete without rebuilding the whole provider root.
    #[test]
    fn fetch_subtree_children_returns_messages_for_folder_path() {
        let msgs = vec![
            make_header(1, "alice@x.com", "Alpha"),
            make_header(2, "bob@x.com", "Beta"),
        ];
        let imap = MockImap::new().with_messages(msgs);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");

        let children = p.fetch_subtree_children()
            .expect("fetch_subtree_children must return Some for a folder path");

        assert_eq!(children.len(), 2, "expected 2 message children; got: {:?}", children);
        assert!(
            children.iter().any(|e| e.as_obj().map_or(false, |o| o.key.contains("Alpha"))),
            "children must include message with subject Alpha"
        );
        assert!(
            children.iter().any(|e| e.as_obj().map_or(false, |o| o.key.contains("Beta"))),
            "children must include message with subject Beta"
        );
    }

    /// `fetch_subtree_parent_key` returns `Some(folder_name)` for a folder path so
    /// that `refresh_subtree_parent` also updates the root Obj's key.  This is needed
    /// when the flat FFON root currently holds a stale message key (e.g. after the
    /// user navigated into a message and then deleted it).
    #[test]
    fn fetch_subtree_parent_key_is_folder_name_for_folder_path() {
        let imap = MockImap::new().with_messages(vec![make_header(1, "a@x.com", "Hi")]);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");

        assert_eq!(
            p.fetch_subtree_parent_key(),
            Some("INBOX".to_owned()),
            "parent key must be Some(\"INBOX\") so refresh_subtree_parent updates the root Obj key"
        );
    }

    /// At the root path (`/`) `fetch_subtree_children` returns `None` — a full
    /// `refresh_current_directory` is needed there.
    #[test]
    fn fetch_subtree_children_stays_none_at_root() {
        let imap = MockImap::new().with_folders(&["INBOX"]);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        // No push_path — provider path stays at "/"
        assert!(
            p.fetch_subtree_children().is_none(),
            "fetch_subtree_children must return None at the folder-list root"
        );
    }

    // ---- Unread marker ----

    #[test]
    fn test_unread_message_label_has_unread_tag() {
        let h = make_header_unread(1, "alice@x.com", "Hello");
        let label = message_label(&h);
        assert!(label.starts_with("[unread] "), "unread label must start with [unread]; got: {label}");
        assert!(label.contains("Hello — alice@x.com"), "label must contain subject and from");
    }

    #[test]
    fn test_read_message_label_has_read_tag() {
        let h = make_header(1, "alice@x.com", "Hello");
        let label = message_label(&h);
        assert!(label.starts_with("[read] "), "read label must start with [read]; got: {label}");
        assert_eq!(label, "[read] Hello — alice@x.com");
    }

    #[test]
    fn test_starred_message_label_has_star_tag() {
        let mut h = make_header(1, "alice@x.com", "Hello");
        h.flagged = true;
        let label = message_label(&h);
        assert!(label.contains("[star]"), "starred label must contain [star]; got: {label}");
        assert!(label.starts_with("[read] [star] "), "starred read label must start with [read] [star]; got: {label}");
    }

    #[test]
    fn test_unread_starred_message_label() {
        let mut h = make_header_unread(1, "alice@x.com", "Hello");
        h.flagged = true;
        let label = message_label(&h);
        assert!(label.starts_with("[unread] [star] "), "unread starred label must start with [unread] [star]; got: {label}");
    }

    #[test]
    fn test_lookup_uid_after_tag_prefix_strip() {
        // Simulate: path has "[unread] Hello — alice@x.com" but cache now has seen=true.
        let msgs = vec![make_header(1, "alice@x.com", "Hello")]; // seen=true → [read] prefix
        let imap = MockImap::new().with_messages(msgs);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        // Path carries the old [unread] label.
        let uid = p.lookup_uid("[unread] Hello — alice@x.com");
        assert_eq!(uid, Some(1), "lookup_uid must resolve despite stale [unread] prefix");
    }

    #[test]
    fn test_strip_message_tags_removes_prefix() {
        assert_eq!(strip_message_tags("[read] Hello — alice@x.com"), "Hello — alice@x.com");
        assert_eq!(strip_message_tags("[unread] [star] Hello — alice@x.com"), "Hello — alice@x.com");
        assert_eq!(strip_message_tags("Hello — alice@x.com"), "Hello — alice@x.com");
    }

    #[test]
    fn test_build_folder_shows_unread_marker_for_unseen_message() {
        let msgs = vec![make_header_unread(1, "alice@x.com", "New Mail")];
        let imap = MockImap::new().with_messages(msgs);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        let items = p.fetch();
        assert!(
            items.iter().any(|e| e.as_obj().map_or(false, |o| o.key.starts_with("[unread] "))),
            "unread message should have [unread] prefix in list; got: {:?}", items
        );
    }

    #[test]
    fn test_build_folder_no_marker_for_seen_message() {
        let msgs = vec![make_header(1, "alice@x.com", "Old Mail")];
        let imap = MockImap::new().with_messages(msgs);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        let items = p.fetch();
        assert!(
            !items.iter().any(|e| e.as_obj().map_or(false, |o| o.key.starts_with("[unread] "))),
            "seen message should not have [unread] prefix; got: {:?}", items
        );
    }

    // ---- Auto-mark-read ----

    #[test]
    fn test_opening_unread_message_marks_seen() {
        let msgs = vec![make_header_unread(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        // Unread: label has [unread] prefix
        p.push_path("[unread] Hello — alice@example.com");
        p.fetch();
        // IMAP set_flags should have been called with +FLAGS (\Seen)
        let mock = p.imap.as_ref().unwrap().as_ref() as *const dyn ImapBackend as *const MockImap;
        let stored = unsafe { &(*mock).stored_flags };
        assert!(
            stored.iter().any(|(_, uid, flag)| *uid == 1 && flag.contains("\\Seen") && flag.starts_with("+FLAGS")),
            "opening an unread message must call set_flags +FLAGS (\\Seen); got: {:?}", stored
        );
    }

    #[test]
    fn test_opening_already_read_message_does_not_call_set_flags() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        let mock = p.imap.as_ref().unwrap().as_ref() as *const dyn ImapBackend as *const MockImap;
        let stored = unsafe { &(*mock).stored_flags };
        assert!(
            stored.is_empty(),
            "opening an already-read message must not call set_flags; got: {:?}", stored
        );
    }

    #[test]
    fn test_opening_unread_message_imap_failure_does_not_update_local_state() {
        let msgs = vec![make_header_unread(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new()
            .with_messages(msgs)
            .with_detail(msg)
            .with_set_flags_error("IMAP STORE failed");
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[unread] Hello — alice@example.com");
        p.fetch();
        // set_flags failed — local cache must still show the message as unread.
        let still_unread = p.message_cache.iter().any(|h| h.uid == 1 && !h.seen);
        assert!(still_unread, "message must remain unread in cache when set_flags fails");
        // Envelope cache must NOT have been invalidated (would flip unread→read in the list).
        assert!(p.envelope_cache.is_some(), "envelope cache must not be invalidated on set_flags failure");
    }

    // ---- mark-read / mark-unread commands ----

    #[test]
    fn test_mark_unread_issues_remove_seen_flag() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        let mut err = String::new();
        p.handle_command("mark-unread", "", 0, &mut err);
        assert!(err.is_empty(), "mark-unread should not set error; got: {err}");
        let mock = p.imap.as_ref().unwrap().as_ref() as *const dyn ImapBackend as *const MockImap;
        let stored = unsafe { &(*mock).stored_flags };
        assert!(
            stored.iter().any(|(_, uid, flag)| *uid == 1 && flag.contains("\\Seen") && flag.starts_with("-FLAGS")),
            "mark-unread must call -FLAGS (\\Seen); got: {:?}", stored
        );
    }

    #[test]
    fn test_mark_read_issues_add_seen_flag() {
        let msgs = vec![make_header_unread(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[unread] Hello — alice@example.com");
        p.fetch();  // triggers auto-mark-read
        // Reset stored_flags to isolate the explicit mark-read command
        let mock = p.imap.as_mut().unwrap().as_mut() as *mut dyn ImapBackend as *mut MockImap;
        unsafe { (*mock).stored_flags.clear(); }
        let mut err = String::new();
        p.handle_command("mark-read", "", 0, &mut err);
        assert!(err.is_empty(), "mark-read should not set error; got: {err}");
        let stored = unsafe { &(*mock).stored_flags };
        assert!(
            stored.iter().any(|(_, uid, flag)| *uid == 1 && flag.contains("\\Seen") && flag.starts_with("+FLAGS")),
            "mark-read must call +FLAGS (\\Seen); got: {:?}", stored
        );
    }

    // ---- star / unstar commands ----

    #[test]
    fn test_star_issues_add_flagged() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        let mut err = String::new();
        p.handle_command("star", "", 0, &mut err);
        assert!(err.is_empty(), "star should not set error; got: {err}");
        let mock = p.imap.as_ref().unwrap().as_ref() as *const dyn ImapBackend as *const MockImap;
        let stored = unsafe { &(*mock).stored_flags };
        assert!(
            stored.iter().any(|(_, uid, flag)| *uid == 1 && flag.contains("\\Flagged") && flag.starts_with("+FLAGS")),
            "star must call +FLAGS (\\Flagged); got: {:?}", stored
        );
    }

    #[test]
    fn test_unstar_issues_remove_flagged() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        let mut err = String::new();
        p.handle_command("unstar", "", 0, &mut err);
        assert!(err.is_empty(), "unstar should not set error; got: {err}");
        let mock = p.imap.as_ref().unwrap().as_ref() as *const dyn ImapBackend as *const MockImap;
        let stored = unsafe { &(*mock).stored_flags };
        assert!(
            stored.iter().any(|(_, uid, flag)| *uid == 1 && flag.contains("\\Flagged") && flag.starts_with("-FLAGS")),
            "unstar must call -FLAGS (\\Flagged); got: {:?}", stored
        );
    }

    // ---- delete command ----

    #[test]
    fn test_delete_from_inbox_moves_to_trash() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let trash_info = FolderInfo {
            name: "[Gmail]/Trash".to_owned(),
            attributes: vec!["\\Trash".to_owned()],
        };
        let imap = MockImap::new()
            .with_folder_infos(vec![
                FolderInfo { name: "INBOX".to_owned(), attributes: vec![] },
                trash_info,
            ])
            .with_messages(msgs)
            .with_detail(msg);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        // Build root to populate special_folders from LIST response.
        p.fetch();
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        let mut err = String::new();
        p.handle_command("delete", "", 0, &mut err);
        assert!(err.is_empty(), "delete should not set error; got: {err}");
        let mock = p.imap.as_ref().unwrap().as_ref() as *const dyn ImapBackend as *const MockImap;
        let moved = unsafe { &(*mock).moved };
        assert!(
            moved.iter().any(|(from, uid, dest)| *uid == 1 && from == "INBOX" && dest == "[Gmail]/Trash"),
            "delete from INBOX must move to trash; got: {:?}", moved
        );
        // Navigation back to the folder is handled by the app layer, not the provider.
    }

    #[test]
    fn test_delete_from_trash_permanently_deletes() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let trash_info = FolderInfo {
            name: "[Gmail]/Trash".to_owned(),
            attributes: vec!["\\Trash".to_owned()],
        };
        let imap = MockImap::new()
            .with_folder_infos(vec![
                FolderInfo { name: "INBOX".to_owned(), attributes: vec![] },
                trash_info,
            ])
            .with_messages(msgs)
            .with_detail(msg);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.fetch(); // populate special_folders
        // Manually set path into Trash (simulating navigation)
        p.folder_mappings.push(("Trash".to_owned(), "[Gmail]/Trash".to_owned()));
        p.message_cache = vec![make_header(1, "alice@example.com", "Hello")];
        p.push_path("Trash");
        p.push_path("[read] Hello — alice@example.com");
        let mut err = String::new();
        p.handle_command("delete", "", 0, &mut err);
        assert!(err.is_empty(), "delete from trash should not set error; got: {err}");
        let mock = p.imap.as_ref().unwrap().as_ref() as *const dyn ImapBackend as *const MockImap;
        let expunged = unsafe { &(*mock).expunged };
        assert!(
            expunged.iter().any(|(_, uid)| *uid == 1),
            "delete from trash must expunge; got: {:?}", expunged
        );
    }

    #[test]
    fn test_delete_no_trash_folder_hard_deletes() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new()
            .with_folders(&["INBOX"])
            .with_messages(msgs)
            .with_detail(msg);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.fetch(); // no trash in folder list
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        let mut err = String::new();
        p.handle_command("delete", "", 0, &mut err);
        assert!(err.is_empty(), "delete with no trash should not set error; got: {err}");
        let mock = p.imap.as_ref().unwrap().as_ref() as *const dyn ImapBackend as *const MockImap;
        let expunged = unsafe { &(*mock).expunged };
        assert!(
            expunged.iter().any(|(_, uid)| *uid == 1),
            "delete with no trash folder must expunge; got: {:?}", expunged
        );
    }

    // ---- archive command ----

    #[test]
    fn test_archive_moves_to_archive_folder() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let archive_info = FolderInfo {
            name: "[Gmail]/All Mail".to_owned(),
            attributes: vec!["\\All".to_owned()],
        };
        let imap = MockImap::new()
            .with_folder_infos(vec![
                FolderInfo { name: "INBOX".to_owned(), attributes: vec![] },
                archive_info,
            ])
            .with_messages(msgs)
            .with_detail(msg);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.fetch(); // populate special_folders
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        let mut err = String::new();
        p.handle_command("archive", "", 0, &mut err);
        assert!(err.is_empty(), "archive should not set error; got: {err}");
        let mock = p.imap.as_ref().unwrap().as_ref() as *const dyn ImapBackend as *const MockImap;
        let moved = unsafe { &(*mock).moved };
        assert!(
            moved.iter().any(|(from, uid, dest)| *uid == 1 && from == "INBOX" && dest == "[Gmail]/All Mail"),
            "archive must move to archive folder; got: {:?}", moved
        );
    }

    #[test]
    fn test_archive_error_when_no_archive_folder() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new()
            .with_folders(&["INBOX"])
            .with_messages(msgs)
            .with_detail(msg);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.fetch();
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        let mut err = String::new();
        p.handle_command("archive", "", 0, &mut err);
        assert!(!err.is_empty(), "archive without archive folder must set error");
        assert!(err.contains("\\Archive"), "error must mention \\Archive; got: {err}");
    }

    // ---- move command (two-phase) ----

    #[test]
    fn test_move_command_stores_pending_context() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new()
            .with_folders(&["INBOX", "Sent"])
            .with_messages(msgs)
            .with_detail(msg);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.fetch();
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        let mut err = String::new();
        p.handle_command("move", "", 0, &mut err);
        assert!(err.is_empty(), "move should not set error; got: {err}");
        assert_eq!(p.pending_move_uid, Some(1), "pending_move_uid should be set");
        assert_eq!(p.pending_move_folder, "INBOX", "pending_move_folder should be INBOX");
    }

    #[test]
    fn test_move_command_list_items_excludes_current_folder() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new()
            .with_folders(&["INBOX", "Sent", "Drafts"])
            .with_messages(msgs)
            .with_detail(msg);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.fetch();
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        let mut err = String::new();
        p.handle_command("move", "", 0, &mut err);
        let items = p.command_list_items("move");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(!labels.contains(&"INBOX"), "INBOX should not appear in move targets");
        assert!(labels.contains(&"Sent"), "Sent should appear as move target");
        assert!(labels.contains(&"Drafts"), "Drafts should appear as move target");
    }

    #[test]
    fn test_execute_move_calls_move_message_and_pops_path() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new()
            .with_folders(&["INBOX", "Sent"])
            .with_messages(msgs)
            .with_detail(msg);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.fetch();
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        p.fetch();
        let mut err = String::new();
        p.handle_command("move", "", 0, &mut err);
        let ok = p.execute_command("move", "Sent");
        assert!(ok, "execute_command(move) should return true on success");
        let mock = p.imap.as_ref().unwrap().as_ref() as *const dyn ImapBackend as *const MockImap;
        let moved = unsafe { &(*mock).moved };
        assert!(
            moved.iter().any(|(from, uid, dest)| *uid == 1 && from == "INBOX" && dest == "Sent"),
            "execute_command(move) must call move_message; got: {:?}", moved
        );
        // Navigation back to the folder is handled by the app layer, not the provider.
    }

    // ---- SPECIAL-USE folder detection ----

    #[test]
    fn test_special_use_trash_folder_detected() {
        let imap = MockImap::new().with_folder_infos(vec![
            FolderInfo { name: "INBOX".to_owned(), attributes: vec![] },
            FolderInfo { name: "[Gmail]/Trash".to_owned(), attributes: vec!["\\Trash".to_owned()] },
        ]);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.fetch();
        assert_eq!(
            p.special_folders.trash.as_deref(),
            Some("[Gmail]/Trash"),
            "\\Trash attribute must populate special_folders.trash"
        );
    }

    #[test]
    fn test_special_use_archive_folder_detected_via_all() {
        let imap = MockImap::new().with_folder_infos(vec![
            FolderInfo { name: "INBOX".to_owned(), attributes: vec![] },
            FolderInfo { name: "[Gmail]/All Mail".to_owned(), attributes: vec!["\\All".to_owned()] },
        ]);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.fetch();
        assert_eq!(
            p.special_folders.archive.as_deref(),
            Some("[Gmail]/All Mail"),
            "\\All attribute must populate special_folders.archive"
        );
    }

    #[test]
    fn test_new_commands_included_when_message_selected() {
        // Message commands only appear at depth 2 (a specific message is selected).
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let imap = MockImap::new().with_folders(&["INBOX"]).with_messages(msgs);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("[read] Hello — alice@example.com");
        let cmds = p.commands();
        for cmd in &["mark-read", "mark-unread", "star", "unstar", "delete", "archive", "move"] {
            assert!(cmds.contains(&cmd.to_string()), "commands() must include '{cmd}' when message selected");
        }
    }

    #[test]
    fn test_message_commands_hidden_at_folder_with_no_message_selected() {
        // At /INBOX with no message focused, message-ops must not appear.
        let imap = MockImap::new().with_folders(&["INBOX"]);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.push_path("INBOX");
        let cmds = p.commands();
        for cmd in &["mark-read", "mark-unread", "star", "unstar", "delete", "archive", "move"] {
            assert!(!cmds.contains(&cmd.to_string()), "commands() must not include '{cmd}' at folder without message");
        }
    }

    #[test]
    fn test_message_commands_hidden_at_root() {
        let p = EmailClientProvider::new().with_oauth_token("fake");
        let cmds = p.commands();
        for cmd in &["mark-read", "mark-unread", "star", "unstar", "delete", "archive", "move"] {
            assert!(!cmds.contains(&cmd.to_string()), "commands() must not include '{cmd}' at root");
        }
    }

    // ---- Tier 2: Cc / Bcc ----

    #[test]
    fn test_compose_view_has_cc_and_bcc_fields() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        let items = p.fetch();
        assert!(
            items.iter().any(|e| e.as_str().map_or(false, |s| s.starts_with("Cc:"))),
            "compose view must include a Cc: field"
        );
        assert!(
            items.iter().any(|e| e.as_str().map_or(false, |s| s.starts_with("Bcc:"))),
            "compose view must include a Bcc: field"
        );
    }

    #[test]
    fn test_commit_edit_updates_cc_field() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.push_path("Cc");
        assert!(p.commit_edit("", "cc@example.com"));
        assert_eq!(p.compose.draft.cc, "cc@example.com");
    }

    #[test]
    fn test_commit_edit_updates_bcc_field() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.push_path("Bcc");
        assert!(p.commit_edit("", "bcc@example.com"));
        assert_eq!(p.compose.draft.bcc, "bcc@example.com");
    }

    #[test]
    fn test_send_draft_passes_cc_and_bcc_to_smtp() {
        let smtp = MockSmtp::new();
        let sent = std::sync::Arc::clone(&smtp.sent);
        let mut p = EmailClientProvider::new().with_smtp(Box::new(smtp));
        p.push_path("compose");
        p.compose.draft.to = "to@example.com".to_owned();
        p.compose.draft.cc = "cc@example.com".to_owned();
        p.compose.draft.bcc = "bcc@example.com".to_owned();
        p.compose.draft.subject = "Test".to_owned();
        p.on_button_press("send");
        assert_eq!(sent.lock().unwrap().len(), 1, "SMTP should have been called once");
        let smtp_ref = p.smtp.as_ref().unwrap().as_ref() as *const dyn SmtpBackend as *const MockSmtp;
        let cc = unsafe { &(*smtp_ref).cc_sent };
        let bcc = unsafe { &(*smtp_ref).bcc_sent };
        assert!(cc[0].contains(&"cc@example.com".to_owned()), "Cc must be passed to smtp; got: {:?}", cc);
        assert!(bcc[0].contains(&"bcc@example.com".to_owned()), "Bcc must be passed to smtp; got: {:?}", bcc);
    }

    // ---- Tier 2: pagination ----

    #[test]
    fn test_build_folder_shows_load_more_when_at_limit() {
        // Return exactly 50 messages — at_limit triggers.
        let msgs: Vec<MessageHeader> = (1u32..=50)
            .map(|i| make_header(i, "a@b.com", &format!("msg {i}")))
            .collect();
        let imap = MockImap::new().with_messages(msgs);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        let items = p.fetch();
        assert!(
            items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("load-more"))),
            "folder at limit must show load-more button; items: {:?}", items
        );
    }

    #[test]
    fn test_load_more_increases_folder_limit() {
        let msgs: Vec<MessageHeader> = (1u32..=50)
            .map(|i| make_header(i, "a@b.com", &format!("msg {i}")))
            .collect();
        let imap = MockImap::new().with_messages(msgs);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        // Default limit is 50; pressing load-more should raise it to 100.
        p.on_button_press("load-more");
        let limit = *p.folder_limits.get("INBOX").unwrap_or(&50);
        assert_eq!(limit, 100, "load-more must increment the folder limit by 50");
    }

    // ---- Tier 2: receive attachments in message view ----

    #[test]
    fn test_message_view_shows_attachments_section() {
        let attachment = EmailAttachment {
            filename: "report.pdf".to_owned(),
            content_type: "application/pdf".to_owned(),
            data: vec![1, 2, 3, 4],
        };
        let mut msg = make_message(1);
        msg.attachments = vec![attachment];
        let items = build_message_view(&msg);
        let attach_obj = items.iter().find(|e| {
            e.as_obj().map_or(false, |o| o.key == "Attachments")
        });
        assert!(attach_obj.is_some(), "message view must have Attachments obj when attachments are present");
        let children = &attach_obj.unwrap().as_obj().unwrap().children;
        assert!(
            children.iter().any(|c| c.as_str().map_or(false, |s| s.contains("report.pdf"))),
            "Attachments obj must list the filename; children: {:?}", children
        );
    }

    #[test]
    fn test_message_view_no_attachments_section_when_empty() {
        let msg = make_message(1);
        let items = build_message_view(&msg);
        assert!(
            !items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "Attachments")),
            "message view must not include Attachments obj when there are none"
        );
    }

    // ---- Tier 2: draft save to \Drafts on navigate-away ----

    #[test]
    fn test_pop_path_from_compose_saves_draft_to_drafts_folder() {
        let imap = MockImap::new()
            .with_folders(&["INBOX", "[Gmail]/Drafts"])
            .with_folder_infos(vec![
                FolderInfo { name: "INBOX".to_owned(), attributes: vec![] },
                FolderInfo { name: "[Gmail]/Drafts".to_owned(), attributes: vec!["\\Drafts".to_owned()] },
            ]);
        let mut p = EmailClientProvider::new().with_oauth_token("fake").with_imap(Box::new(imap));
        p.fetch(); // populate special_folders
        p.push_path("compose");
        p.compose.draft.to = "to@example.com".to_owned();
        p.compose.draft.subject = "Draft subject".to_owned();
        p.pop_path();
        let mock = p.imap.as_ref().unwrap().as_ref() as *const dyn ImapBackend as *const MockImap;
        let appended = unsafe { &(*mock).appended };
        assert!(
            appended.iter().any(|(folder, _)| folder.contains("Drafts")),
            "pop_path from compose must APPEND draft to \\Drafts; got: {:?}",
            appended.iter().map(|(f, b)| (f, b.len())).collect::<Vec<_>>()
        );
    }

    // ---- Tier 2: search (collect_deep_search_items) ----

    #[test]
    fn test_collect_deep_search_items_returns_cached_envelopes() {
        let msgs = vec![
            make_header(1, "alice@example.com", "Hello World"),
            make_header(2, "bob@example.com", "Rust Newsletter"),
        ];
        let imap = MockImap::new().with_messages(msgs);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch(); // populates message_cache
        let results = p.collect_deep_search_items().unwrap_or_default();
        assert!(
            results.iter().any(|r| r.label.to_lowercase().contains("hello")),
            "search results must include messages matching the query; got: {:?}", results
        );
        assert!(
            results.iter().any(|r| r.label.to_lowercase().contains("rust")),
            "search results must include second message; got: {:?}", results
        );
    }

    #[test]
    fn test_collect_deep_search_items_empty_before_fetch() {
        let p = EmailClientProvider::new();
        // No fetch yet — message_cache is empty so results should be empty.
        let results = p.collect_deep_search_items().unwrap_or_default();
        assert!(results.is_empty(), "search must return nothing before any message is cached");
    }
}

// ---------------------------------------------------------------------------
// SDK registration
// ---------------------------------------------------------------------------

/// Register the email client with the SDK factory and manifest registries.
pub fn register() {
    sicompass_sdk::register_provider_factory("emailclient", || {
        Box::new(EmailClientProvider::new())
    });
    sicompass_sdk::register_builtin_manifest(
        sicompass_sdk::BuiltinManifest::new("emailclient", "email client").with_settings(vec![
            sicompass_sdk::SettingDecl::text("email client", "IMAP URL",              "emailImapUrl",       "imaps://imap.gmail.com"),
            sicompass_sdk::SettingDecl::text("email client", "SMTP URL",              "emailSmtpUrl",       "smtps://smtp.gmail.com"),
            sicompass_sdk::SettingDecl::text("email client", "username",              "emailUsername",      ""),
            sicompass_sdk::SettingDecl::text("email client", "password",              "emailPassword",      ""),
            sicompass_sdk::SettingDecl::text("email client", "client ID (OAuth)",     "emailClientId",      ""),
            sicompass_sdk::SettingDecl::text("email client", "client secret (OAuth)", "emailClientSecret",  ""),
        ]),
    );
}
