//! Email client provider — Rust port of `lib_emailclient/`.
//!
//! Implements the [`Provider`] trait for IMAP/SMTP email access.
//! IMAP and SMTP operations are injected via the [`ImapBackend`] and
//! [`SmtpBackend`] traits, making the provider fully unit-testable.
//!
//! ## FFON tree layout
//!
//! ```text
//! Root "/"
//!   meta           (obj)  — shortcut hints
//!   compose        (obj)  — empty compose form
//!   folder-name    (obj)  — one per IMAP folder, navigable
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
//!   reply          (obj)  — prefilled compose form
//!   forward        (obj)  — prefilled compose form
//!
//! Compose "/{compose|reply|forward|reply all}/"
//!   meta           (obj)
//!   To: <input>    (str)
//!   Subject:<input>(str)
//!   Body: <input>  (str)
//!   <button>send</button>Send  (str)
//! ```

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

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
}

/// A compose-form draft.
#[derive(Debug, Clone, Default)]
pub struct Draft {
    pub to: String,
    pub subject: String,
    pub body: String,
}

// ---------------------------------------------------------------------------
// Injectable backend traits
// ---------------------------------------------------------------------------

/// IMAP backend — all operations used by the provider.
pub trait ImapBackend: Send {
    /// List all selectable folder names.
    fn list_folders(&mut self) -> Result<Vec<String>, String>;
    /// Fetch headers for the most recent `limit` messages in `folder`.
    fn list_messages(&mut self, folder: &str, limit: usize) -> Result<Vec<MessageHeader>, String>;
    /// Fetch the full content of a message by UID.
    fn fetch_message(&mut self, folder: &str, uid: u32) -> Result<Option<EmailMessage>, String>;
}

/// SMTP backend — send an email message.
pub trait SmtpBackend: Send {
    fn send(
        &mut self,
        from: &str,
        to: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// EmailClientProvider
// ---------------------------------------------------------------------------

pub struct EmailClientProvider {
    imap_url: String,
    smtp_url: String,
    username: String,
    password: String,
    current_path: String,

    /// Cached folder list (display_name → folder_name are the same here).
    folder_cache: Vec<String>,
    /// Cached message headers for the current folder.
    message_cache: Vec<MessageHeader>,
    /// Cached full message for the current message path.
    message_detail: Option<EmailMessage>,
    /// Active compose draft.
    draft: Draft,

    // Injected backends
    imap: Option<Box<dyn ImapBackend>>,
    smtp: Option<Box<dyn SmtpBackend>>,
}

impl EmailClientProvider {
    pub fn new() -> Self {
        EmailClientProvider {
            imap_url: String::new(),
            smtp_url: String::new(),
            username: String::new(),
            password: String::new(),
            current_path: "/".to_owned(),
            folder_cache: Vec::new(),
            message_cache: Vec::new(),
            message_detail: None,
            draft: Draft::default(),
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

    fn folder_name(&self) -> &str {
        self.path_segments().first().copied().unwrap_or("")
    }

    // ---- FFON tree builders -----------------------------------------------

    fn build_root(&mut self) -> Vec<FfonElement> {
        let mut items = vec![];

        // Compose entry
        items.push(FfonElement::new_obj("compose"));

        // Folder list
        if let Some(ref mut imap) = self.imap {
            match imap.list_folders() {
                Ok(folders) => {
                    self.folder_cache = folders.clone();
                    for f in folders {
                        items.push(FfonElement::new_obj(f));
                    }
                }
                Err(e) => items.push(FfonElement::new_str(format!("IMAP error: {e}"))),
            }
        } else if !self.folder_cache.is_empty() {
            for f in self.folder_cache.clone() {
                items.push(FfonElement::new_obj(f));
            }
        } else {
            items.push(FfonElement::new_str(
                "not configured — set IMAP/SMTP settings".to_owned(),
            ));
        }

        items
    }

    fn build_folder(&mut self, folder: &str) -> Vec<FfonElement> {
        let mut items = vec![];

        if let Some(ref mut imap) = self.imap {
            match imap.list_messages(folder, 50) {
                Ok(headers) => {
                    self.message_cache = headers.clone();
                    for h in headers {
                        let label = format!("{} — {}", h.subject, h.from);
                        items.push(FfonElement::new_obj(label));
                    }
                }
                Err(e) => items.push(FfonElement::new_str(format!("IMAP error: {e}"))),
            }
        } else if !self.message_cache.is_empty() {
            for h in self.message_cache.clone() {
                let label = format!("{} — {}", h.subject, h.from);
                items.push(FfonElement::new_obj(label));
            }
        } else {
            items.push(FfonElement::new_str("(empty folder)".to_owned()));
        }

        items
    }

    fn build_message(&mut self, folder: &str, msg_label: &str) -> Vec<FfonElement> {
        // Find the UID from message_cache by matching label
        let uid = self
            .message_cache
            .iter()
            .find(|h| format!("{} — {}", h.subject, h.from) == msg_label)
            .map(|h| h.uid);

        if let Some(uid) = uid {
            if let Some(ref mut imap) = self.imap {
                if let Ok(Some(msg)) = imap.fetch_message(folder, uid) {
                    self.message_detail = Some(msg.clone());
                    return build_message_view(&msg);
                }
            }
        }

        // Use cached detail if available
        if let Some(ref msg) = self.message_detail.clone() {
            return build_message_view(msg);
        }

        vec![FfonElement::new_str("(message not found)".to_owned())]
    }

    fn build_compose_view(&self) -> Vec<FfonElement> {
        let mut items = vec![];
        items.push(FfonElement::new_str(format!(
            "To: <input>{}</input>",
            self.draft.to
        )));
        items.push(FfonElement::new_str(format!(
            "Subject: <input>{}</input>",
            self.draft.subject
        )));
        items.push(FfonElement::new_str(format!(
            "Body: <input-all>{}</input-all>",
            self.draft.body
        )));
        items.push(FfonElement::new_str("<button>send</button>Send".to_owned()));
        items
    }

    fn send_draft(&mut self) -> bool {
        if let Some(ref mut smtp) = self.smtp {
            let from = self.username.clone();
            let to = self.draft.to.clone();
            let subject = self.draft.subject.clone();
            let body = self.draft.body.clone();
            smtp.send(&from, &to, &subject, &body).is_ok()
        } else {
            false
        }
    }
}

impl Default for EmailClientProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for EmailClientProvider {
    fn name(&self) -> &str { "emailclient" }
    fn display_name(&self) -> &str { "email client" }

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
                self.build_message(&folder, &msg_label)
            }
            _ => vec![FfonElement::new_str("(invalid path)".to_owned())],
        }
    }

    fn push_path(&mut self, segment: &str) {
        let segs = self.path_segments().len();
        if segs == 0 {
            self.current_path = format!("/{segment}");
        } else {
            self.current_path = format!("{}/{segment}", self.current_path.trim_end_matches('/'));
        }

        // Pre-populate draft when entering reply/forward
        match segment {
            "reply" => {
                if let Some(ref msg) = self.message_detail.clone() {
                    self.draft.to = msg.from.clone();
                    self.draft.subject = if msg.subject.starts_with("Re:") {
                        msg.subject.clone()
                    } else {
                        format!("Re: {}", msg.subject)
                    };
                    self.draft.body = format!("\n\nOn {} <{}> wrote:\n{}", msg.date, msg.from, msg.body);
                }
            }
            "forward" => {
                if let Some(ref msg) = self.message_detail.clone() {
                    self.draft.to.clear();
                    self.draft.subject = if msg.subject.starts_with("Fwd:") {
                        msg.subject.clone()
                    } else {
                        format!("Fwd: {}", msg.subject)
                    };
                    self.draft.body = format!("\n\n--- Forwarded message ---\n{}", msg.body);
                }
            }
            "compose" => {
                self.draft = Draft::default();
            }
            _ => {}
        }
    }

    fn pop_path(&mut self) {
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
        let segs = self.path_segments().len();
        match segs {
            0 => vec![
                "/   Search".to_owned(),
                "F5  Refresh".to_owned(),
                ":   Commands".to_owned(),
            ],
            1 if matches!(
                self.path_segments().first().copied().unwrap_or(""),
                "compose" | "reply" | "reply all" | "forward"
            ) => vec![
                "Tab  Next field".to_owned(),
            ],
            _ => vec![
                "/   Search".to_owned(),
                "F5  Refresh".to_owned(),
            ],
        }
    }

    fn commit_edit(&mut self, _old: &str, new_content: &str) -> bool {
        // Determine which draft field to update from the last path segment
        // e.g. "/compose/To" → field = "To"
        let field = self.current_path
            .rfind('/')
            .map(|i| &self.current_path[i + 1..])
            .unwrap_or("");
        match field {
            "To" => { self.draft.to = new_content.to_owned(); true }
            "Subject" => { self.draft.subject = new_content.to_owned(); true }
            "Body" => { self.draft.body = new_content.to_owned(); true }
            _ => false,
        }
    }

    fn on_button_press(&mut self, function_name: &str) {
        if function_name == "send" {
            self.send_draft();
            self.draft = Draft::default();
            self.pop_path(); // go back to message/folder after sending
        }
    }

    fn commands(&self) -> Vec<String> {
        vec!["compose".to_owned(), "refresh".to_owned()]
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        _elem_key: &str,
        _elem_type: i32,
        _error: &mut String,
    ) -> Option<FfonElement> {
        match cmd {
            "compose" => {
                self.push_path("compose");
            }
            _ => {}
        }
        None
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        match key {
            "emailImapUrl" => self.imap_url = value.to_owned(),
            "emailSmtpUrl" => self.smtp_url = value.to_owned(),
            "emailUsername" => self.username = value.to_owned(),
            "emailPassword" => self.password = value.to_owned(),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// FFON tree helpers
// ---------------------------------------------------------------------------

fn build_message_view(msg: &EmailMessage) -> Vec<FfonElement> {
    let mut items = vec![
        FfonElement::new_str(format!("From: {}", msg.from)),
        FfonElement::new_str(format!("To: {}", msg.to)),
        FfonElement::new_str(format!("Date: {}", msg.date)),
        FfonElement::new_str(format!("Subject: {}", msg.subject)),
        FfonElement::new_str(msg.body.clone()),
        FfonElement::new_obj("reply"),
        FfonElement::new_obj("forward"),
    ];
    items
}

// ---------------------------------------------------------------------------
// Tests — port of tests/lib_emailclient/ (39 tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Mock backends ----

    struct MockImap {
        folders: Vec<String>,
        messages: Vec<MessageHeader>,
        detail: Option<EmailMessage>,
        error: Option<String>,
    }

    impl MockImap {
        fn new() -> Self {
            MockImap { folders: vec![], messages: vec![], detail: None, error: None }
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
        fn with_error(mut self, e: &str) -> Self {
            self.error = Some(e.to_owned());
            self
        }
    }

    impl ImapBackend for MockImap {
        fn list_folders(&mut self) -> Result<Vec<String>, String> {
            if let Some(ref e) = self.error { return Err(e.clone()); }
            Ok(self.folders.clone())
        }
        fn list_messages(&mut self, _folder: &str, _limit: usize) -> Result<Vec<MessageHeader>, String> {
            if let Some(ref e) = self.error { return Err(e.clone()); }
            Ok(self.messages.clone())
        }
        fn fetch_message(&mut self, _folder: &str, _uid: u32) -> Result<Option<EmailMessage>, String> {
            if let Some(ref e) = self.error { return Err(e.clone()); }
            Ok(self.detail.clone())
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
            self.sent.lock().unwrap().push((from.to_owned(), to.to_owned(), subject.to_owned(), body.to_owned()));
            Ok(())
        }
    }

    fn make_header(uid: u32, from: &str, subject: &str) -> MessageHeader {
        MessageHeader { uid, from: from.to_owned(), subject: subject.to_owned(), date: "2025-01-01".to_owned() }
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
        }
    }

    // ---- Tests ----

    #[test]
    fn test_name_and_display_name() {
        let p = EmailClientProvider::new();
        assert_eq!(p.name(), "emailclient");
        assert_eq!(p.display_name(), "email client");
    }

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
    fn test_fetch_message_shows_headers() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        // populate message cache
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
    fn test_fetch_message_has_reply_and_forward() {
        let msgs = vec![make_header(1, "alice@example.com", "Hello")];
        let msg = make_message(1);
        let imap = MockImap::new().with_messages(msgs).with_detail(msg);
        let mut p = EmailClientProvider::new().with_imap(Box::new(imap));
        p.push_path("INBOX");
        p.fetch();
        p.push_path("Hello — alice@example.com");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "reply")));
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "forward")));
    }

    #[test]
    fn test_compose_view_has_input_fields() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("To:") && s.contains("<input>"))));
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("Subject:") && s.contains("<input>"))));
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("Body:") && s.contains("<input-all>"))));
    }

    #[test]
    fn test_compose_view_has_send_button() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_str().map_or(false, |s| s.contains("<button>send</button>"))));
    }

    #[test]
    fn test_reply_prefills_to_field() {
        let msg = make_message(1);
        let mut p = EmailClientProvider::new();
        p.message_detail = Some(msg.clone());
        p.push_path("reply");
        assert_eq!(p.draft.to, "alice@example.com");
    }

    #[test]
    fn test_reply_prefills_subject_with_re() {
        let msg = make_message(1);
        let mut p = EmailClientProvider::new();
        p.message_detail = Some(msg);
        p.push_path("reply");
        assert!(p.draft.subject.starts_with("Re:"));
    }

    #[test]
    fn test_reply_already_re_no_double_re() {
        let mut msg = make_message(1);
        msg.subject = "Re: Hello".to_owned();
        let mut p = EmailClientProvider::new();
        p.message_detail = Some(msg);
        p.push_path("reply");
        assert!(!p.draft.subject.starts_with("Re: Re:"));
    }

    #[test]
    fn test_forward_prefills_subject_with_fwd() {
        let msg = make_message(1);
        let mut p = EmailClientProvider::new();
        p.message_detail = Some(msg);
        p.push_path("forward");
        assert!(p.draft.subject.starts_with("Fwd:"));
    }

    #[test]
    fn test_forward_clears_to_field() {
        let msg = make_message(1);
        let mut p = EmailClientProvider::new();
        p.message_detail = Some(msg);
        p.push_path("forward");
        assert!(p.draft.to.is_empty());
    }

    #[test]
    fn test_on_button_press_send_calls_smtp() {
        let smtp = MockSmtp::new();
        let sent = smtp.sent.clone();
        let mut p = EmailClientProvider::new().with_smtp(Box::new(smtp));
        p.push_path("compose");
        p.draft.to = "test@x.com".to_owned();
        p.draft.subject = "Greet".to_owned();
        p.draft.body = "Hello!".to_owned();
        p.on_button_press("send");
        assert_eq!(sent.lock().unwrap().len(), 1);
        assert_eq!(sent.lock().unwrap()[0].1, "test@x.com");
    }

    #[test]
    fn test_on_button_press_send_clears_draft() {
        let smtp = MockSmtp::new();
        let mut p = EmailClientProvider::new().with_smtp(Box::new(smtp));
        p.push_path("compose");
        p.draft.to = "x@y.com".to_owned();
        p.on_button_press("send");
        assert!(p.draft.to.is_empty());
    }

    #[test]
    fn test_smtp_failure_does_not_panic() {
        let smtp = MockSmtp::failing();
        let mut p = EmailClientProvider::new().with_smtp(Box::new(smtp));
        p.push_path("compose");
        p.draft.to = "x@y.com".to_owned();
        // Should not panic even when SMTP fails
        p.on_button_press("send");
    }

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
    fn test_on_setting_change_imap_url() {
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailImapUrl", "imaps://imap.gmail.com");
        assert_eq!(p.imap_url, "imaps://imap.gmail.com");
    }

    #[test]
    fn test_on_setting_change_smtp_url() {
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailSmtpUrl", "smtps://smtp.gmail.com");
        assert_eq!(p.smtp_url, "smtps://smtp.gmail.com");
    }

    #[test]
    fn test_on_setting_change_username() {
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailUsername", "user@gmail.com");
        assert_eq!(p.username, "user@gmail.com");
    }

    #[test]
    fn test_on_setting_change_password() {
        let mut p = EmailClientProvider::new();
        p.on_setting_change("emailPassword", "secret");
        assert_eq!(p.password, "secret");
    }

    #[test]
    fn test_commands_include_compose_and_refresh() {
        let p = EmailClientProvider::new();
        let cmds = p.commands();
        assert!(cmds.contains(&"compose".to_owned()));
        assert!(cmds.contains(&"refresh".to_owned()));
    }

    #[test]
    fn test_handle_command_compose_changes_path() {
        let mut p = EmailClientProvider::new();
        let mut err = String::new();
        p.handle_command("compose", "", 0, &mut err);
        assert!(p.current_path().contains("compose"));
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

    #[test]
    fn test_commit_stores_to_field() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.push_path("To");
        assert!(p.commit_edit("", "user@example.com"));
        assert_eq!(p.draft.to, "user@example.com");
    }

    #[test]
    fn test_commit_stores_subject_field() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.push_path("Subject");
        assert!(p.commit_edit("", "Test Subject"));
        assert_eq!(p.draft.subject, "Test Subject");
    }

    #[test]
    fn test_commit_stores_body_field() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.push_path("Body");
        assert!(p.commit_edit("", "Hello world!"));
        assert_eq!(p.draft.body, "Hello world!");
    }

    #[test]
    fn test_commit_unknown_field_returns_false() {
        let mut p = EmailClientProvider::new();
        p.push_path("compose");
        p.push_path("Unknown");
        assert!(!p.commit_edit("", "value"));
    }
}
