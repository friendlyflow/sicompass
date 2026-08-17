//! Sicompass Claude provider.
//!
//! Two views behind one `fetch()`, the same shape the terminal provider uses:
//!
//! * **Directory browse** (default). `fetch()` lists the subdirectories of
//!   `browse_path` as childless Objs, so the app's generic navigation walks the
//!   filesystem with the arrow keys exactly as it does for the file browser. No
//!   `claude` process exists yet — a claude tab that is only being browsed
//!   costs nothing.
//! * **Session**. `handle_command("session")` — which the app binds to `:` —
//!   runs `claude` in the folder **being listed** and swaps the listing for the
//!   conversation plus a trailing `<input>` slot.
//!
//! The working directory is the point of the browse view. `claude` resolves its
//! whole project context from its cwd (`CLAUDE.md`, `.claude/skills/`,
//! `.claude/settings.json`, hooks, `.mcp.json`), and it fixes that cwd at spawn:
//! there is no `cd` to send it afterwards, unlike the terminal's PTY. So `:` in
//! a folder other than the one the running child was spawned in kills that child
//! and starts a fresh session there. A conversation belongs to a working
//! directory, and silently answering questions about the wrong tree is worse
//! than losing the transcript.
//!
//! The session itself runs the CLI in streaming-JSON mode
//! (`--print --output-format stream-json --input-format stream-json --verbose`),
//! keeps the process alive for a multi-turn conversation, and renders the JSONL
//! event stream into a navigable FFON document. Unlike running `claude` inside
//! the terminal provider — which detects the TUI and routes it into an opaque
//! cell-grid dashboard — this treats Claude's structured JSON protocol as a
//! first-class FFON source: assistant messages, tool calls, tool results, and
//! the cost summary each become navigable nodes.
//!
//! * `commit_edit()` writes the typed prompt to the child as a `user` message.
//! * `tick()` drains buffered JSONL lines and folds them into conversation
//!   state.
//!
//! The child process lives in [`session`]; the event schema in [`events`]; the
//! conversation state and FFON projection in [`render`].

mod events;
mod render;
mod session;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sicompass_sdk::localize;
use sicompass_sdk::{
    BuiltinManifest, FfonElement, Provider, SettingDecl, register_builtin_manifest,
    register_provider_factory,
};

use render::Conversation;
use session::{Session, SessionConfig};

/// Register this crate's translation bundles with the SDK localizer.
/// Idempotent.
pub fn register_translations() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = localize::register_bundle("en-US", include_str!("../locales/en-US.ftl"));
        let _ = localize::register_bundle("nl-BE", include_str!("../locales/nl-BE.ftl"));
        let _ = localize::register_bundle("fr-BE", include_str!("../locales/fr-BE.ftl"));
        let _ = localize::register_bundle("de-BE", include_str!("../locales/de-BE.ftl"));
    });
}

/// Cap on remembered prompts for `<input>`-slot recall.
const HISTORY_CAP: usize = 1000;

/// Command id: swap from the folder listing to the session, running `claude` in
/// the folder the user browsed to. The app binds `:` to this.
///
/// Deliberately not the terminal's `"shell"`: there is no shell here, and the
/// app no longer hardcodes the enter-command anyway — it fires whichever single
/// transition [`Provider::commands`] is currently advertising.
pub const CMD_SESSION: &str = "session";

/// Command id: swap from the session back to the folder listing. Idempotent, so
/// the app can fire it without first asking which view is active.
pub const CMD_BROWSE: &str = "browse";

/// Which list the provider is currently serving from `fetch()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum View {
    /// Subdirectories of `browse_path`. No `claude` process required.
    #[default]
    Browse,
    /// The conversation plus the live `<input>` slot.
    Session,
}

/// A dedicated provider that streams a `claude` session as FFON.
pub struct ClaudeProvider {
    // --- configuration (applied on next spawn) --------------------------
    program: String,
    permission_mode: String,
    model: Option<String>,
    extra_args: Vec<String>,
    /// Working directory the next spawn runs in. Assigned from `browse_path`
    /// by [`ClaudeProvider::enter_session`] and nowhere else.
    cwd: Option<PathBuf>,
    /// Stream token-level deltas (`--include-partial-messages`).
    include_partial: bool,

    // --- view state -----------------------------------------------------
    /// Which list `fetch()` serves. Starts at [`View::Browse`].
    view: View,
    /// Filesystem position of the directory-browse view. Rooted at `/` (like
    /// the file browser) so every directory on the machine is reachable;
    /// `pop_path` clamps there. Also the cwd `claude` is spawned into when the
    /// user presses `:`, which is the folder whose contents the list is showing
    /// at that moment.
    browse_path: PathBuf,
    /// Where the *running* child was spawned, as a string so `current_path()`
    /// can hand out a `&str`. Never moves for the life of a session: `claude`
    /// fixes its cwd at spawn and cannot be walked over with a `cd`.
    session_path: String,
    /// Why the last spawn attempt failed, shown as the one row above the input
    /// slot. Deliberately *not* folded into the conversation: a failed `:`
    /// leaves the tab session-less, so the next `:` in another folder tries
    /// again, and a history of refusals would follow the user around, each one
    /// looking like it belonged to the folder they had just moved to. Only the
    /// current attempt is worth reporting, so a new attempt replaces it and a
    /// success clears it.
    spawn_error: Option<String>,

    // --- runtime state --------------------------------------------------
    session: Option<Session>,
    /// Guards against re-spawning every frame after a spawn failure. Cleared
    /// deliberately when we *want* a re-spawn (after an unexpected child exit).
    init_attempted: bool,
    convo: Conversation,
    /// Past prompts, oldest first — recall history for the input slot.
    history: Vec<String>,
    /// Value rendered inside the live `<input>` slot on the next `fetch()`.
    pending_input: String,
    /// Last `session_id` seen — used for `--resume` on re-spawn.
    last_session_id: Option<String>,
    error: Option<String>,
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        ClaudeProvider::new()
    }
}

impl ClaudeProvider {
    pub fn new() -> Self {
        ClaudeProvider {
            program: "claude".to_owned(),
            permission_mode: "default".to_owned(),
            model: None,
            extra_args: Vec::new(),
            cwd: None,
            include_partial: true,
            view: View::Browse,
            // Same root as the file browser, so every directory on the machine
            // is reachable. On Windows this resolves to the current drive root.
            browse_path: PathBuf::from("/"),
            session_path: String::new(),
            spawn_error: None,
            session: None,
            init_attempted: false,
            convo: Conversation::default(),
            history: Vec::new(),
            pending_input: String::new(),
            last_session_id: None,
            error: None,
        }
    }

    fn session_config(&self, resume: Option<String>) -> SessionConfig {
        SessionConfig {
            program: self.program.clone(),
            permission_mode: self.permission_mode.clone(),
            model: self.model.clone(),
            extra_args: self.extra_args.clone(),
            cwd: self.cwd.clone(),
            resume,
            include_partial: self.include_partial,
        }
    }

    /// Lazily spawn the `claude` child. A failed spawn sets `init_attempted` so
    /// it is not retried every frame; a re-spawn after an unexpected exit is
    /// requested by clearing `init_attempted` first (see [`Self::pump`]).
    fn ensure_session(&mut self) {
        if self.session.is_some() || self.init_attempted {
            return;
        }
        self.init_attempted = true;
        let resume = self.last_session_id.clone();
        let restarting = resume.is_some();
        tracing::debug!(program = %self.program, restarting, "claude: ensure_session spawning");
        match Session::spawn(&self.session_config(resume)) {
            Ok(s) => {
                tracing::debug!("claude: ensure_session spawn OK");
                self.session = Some(s);
                self.spawn_error = None;
                if restarting {
                    self.error = Some("claude session restarted".to_owned());
                }
            }
            Err(e) => {
                tracing::error!(program = %self.program, error = %e, kind = ?e.kind(), "claude: ensure_session spawn failed");
                // Name the directory as well as the reason: `:` in a folder the
                // user cannot enter fails for a reason that belongs to *that*
                // folder, and a bare "could not start" reads like a problem with
                // whatever folder they move to next.
                let reason = if e.kind() == std::io::ErrorKind::NotFound {
                    format!(
                        "could not start `{}`: binary not found on PATH",
                        self.program
                    )
                } else {
                    format!("could not start `{}`: {e}", self.program)
                };
                let msg = match &self.cwd {
                    Some(dir) => format!("({reason} in {})", dir.display()),
                    None => reason,
                };
                // Both: `spawn_error` is the row that stays readable above the
                // input slot, `error` is the transient status line the app
                // drains once per frame.
                self.spawn_error = Some(msg.clone());
                self.error = Some(msg);
            }
        }
    }

    /// Children of the browse view: the subdirectories of `browse_path`,
    /// natural-sorted case-insensitively (the same ordering the file browser
    /// uses for its alphabetical mode).
    ///
    /// Files are deliberately omitted. The browse view exists only to pick a
    /// working directory, and leaving files out keeps a big directory short
    /// enough to walk by ear.
    ///
    /// Each entry is a *bare* `Obj` with no `<input>` tag, unlike the file
    /// browser. That renders as `+ name` rather than `+i name` (nothing here is
    /// renameable), and it keeps a directory from matching the app's "live input
    /// slot" check, which special-cases this provider by name and would
    /// otherwise mistake a `+i` directory for the session's input line.
    fn list_subdirectories(&self) -> Vec<FfonElement> {
        let Ok(read_dir) = std::fs::read_dir(&self.browse_path) else {
            return Vec::new();
        };
        let mut names: Vec<String> = Vec::new();
        for entry in read_dir.flatten() {
            // `metadata()` follows symlinks, so a symlink pointing at a
            // directory is offered as one — which is what `cd` would do too.
            // Entries whose metadata can't be read (broken symlinks, races,
            // permission holes) are skipped rather than shown as dead ends.
            if entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort_by(|a, b| natord::compare_ignore_case(a, b));
        names
            .into_iter()
            .map(|n| FfonElement::new_obj(&n))
            .collect()
    }

    /// Swap to the session view, running `claude` in the folder the browse view
    /// is listing.
    ///
    /// Re-entering the folder the running child was already spawned in is a pure
    /// view swap: the conversation, the child, and the recall history all
    /// survive a trip back out to the folders.
    ///
    /// Entering from anywhere else respawns. `claude` fixes its working
    /// directory at spawn — there is no `cd` to send it, unlike the terminal's
    /// PTY — so a session started in one project cannot be walked over into
    /// another. The transcript goes with the child, because it describes a tree
    /// the new session cannot see.
    fn enter_session(&mut self) {
        self.view = View::Session;

        // Compared before `session_path` is reassigned below, so a repeated `:`
        // in the folder a failed spawn was attempted in is recognised as the
        // same folder and does not re-attempt — that is the `init_attempted`
        // latch's whole job. Only a genuinely different folder clears it.
        let same_folder = !self.session_path.is_empty()
            && Path::new(&self.session_path) == self.browse_path.as_path();

        if self.session.is_some() && same_folder {
            return;
        }

        if self.session.is_some() {
            // Session::Drop kills the child.
            self.session = None;
            self.convo = Conversation::default();
            self.pending_input.clear();
            // Must not survive: `ensure_session` would hand it to `--resume` and
            // reopen the *old* folder's transcript in the new folder.
            self.last_session_id = None;
            // `history` is deliberately kept — those are prompts the user typed,
            // and ↑-recall outliving one session is the same contract the
            // terminal gives its shell.
        }

        self.cwd = Some(self.browse_path.clone());
        self.session_path = self.browse_path.to_string_lossy().into_owned();
        if !same_folder {
            // A respawn in a new folder is exactly the case the one-shot latch
            // must not block.
            self.init_attempted = false;
            self.spawn_error = None;
        }
        // A failure leaves `init_attempted` set, so hammering `:` in the same
        // folder does not spawn once per keypress. The cwd is user-chosen
        // though, so a failure here (an unreadable directory, one deleted
        // between listing and `:`) says nothing about the next folder — and the
        // `!same_folder` reset above is what lets `:` elsewhere try again,
        // instead of leaving the tab permanently session-less.
        self.ensure_session();
    }

    /// Swap back to the folder listing. The child keeps running: this is a view
    /// change, not a teardown, so `:` back into the same folder resumes the same
    /// conversation.
    ///
    /// Unlike the terminal there is nothing to follow. A shell can `cd` itself
    /// out from under the folder the user picked; `claude` cannot, so
    /// `browse_path` is already the folder the session is running in.
    fn leave_session(&mut self) {
        self.view = View::Browse;
    }

    /// The session view: the conversation, then the live `<input>` slot, with a
    /// spawn failure spliced in directly above that slot.
    ///
    /// Splicing here rather than inside [`render::build`] keeps the conversation
    /// projection untouched. `build` always ends with the input slot, so
    /// inserting at `len - 1` puts the message where the last thing that
    /// happened is the last thing read out before the prompt.
    fn fetch_session(&mut self) -> Vec<FfonElement> {
        self.pump();
        let mut out = render::build(&self.convo, &self.history, &self.pending_input);
        if let Some(msg) = &self.spawn_error {
            let at = out.len().saturating_sub(1);
            out.insert(at, FfonElement::new_str(msg.clone()));
        }
        out
    }

    /// Drain buffered JSONL lines, fold them into conversation state, and watch
    /// for an unexpected child exit. Returns `true` if anything changed.
    fn pump(&mut self) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        let mut changed = false;
        let drained = session.drain_lines();
        if !drained.is_empty() {
            tracing::debug!(count = drained.len(), "claude: pump drained lines");
        }
        for line in drained {
            match events::parse_line(&line) {
                Some(ev) => {
                    self.convo.apply(ev);
                    changed = true;
                }
                None => {
                    let preview: String = line.chars().take(120).collect();
                    tracing::debug!(line = %preview, "claude: pump could not parse line");
                }
            }
        }
        if let Some(sid) = &self.convo.session_id {
            self.last_session_id = Some(sid.clone());
        }
        // Unexpected child exit: surface stderr, drop the session, and allow a
        // `--resume` re-spawn on the next `ensure_session()`.
        if !session.is_alive() {
            let stderr = session.take_stderr();
            tracing::error!(stderr = %stderr.trim(), "claude: child exited unexpectedly");
            self.session = None;
            self.init_attempted = false;
            self.convo.busy = false;
            if !stderr.trim().is_empty() {
                self.error = Some(format!("claude exited: {}", stderr.trim()));
            }
            changed = true;
        }
        changed
    }

    fn record_history(&mut self, prompt: &str) {
        self.history.push(prompt.to_owned());
        if self.history.len() > HISTORY_CAP {
            let drop = self.history.len() - HISTORY_CAP;
            self.history.drain(..drop);
        }
    }
}

impl Provider for ClaudeProvider {
    fn name(&self) -> &str {
        "claude"
    }

    fn display_name(&self) -> String {
        register_translations();
        localize::t("claude-display-name")
    }

    /// Start at the filesystem root in the browse view. `claude` is *not*
    /// spawned here — that happens on the first `:` (see [`Self::enter_session`]),
    /// so a claude tab the user only browses never starts a process.
    fn init(&mut self) {
        self.view = View::Browse;
        self.browse_path = PathBuf::from("/");
    }

    fn cleanup(&mut self) {
        // Dropping the Session kills the child (Session::Drop).
        self.session = None;
        self.init_attempted = false;
        self.spawn_error = None;
    }

    /// OS process id of the child, if started. `None` until the first `:`, so
    /// the tab-switcher label falls back gracefully.
    fn process_id(&self) -> Option<u32> {
        self.session.as_ref().map(|s| s.pid())
    }

    /// Busy while a turn is in flight. The app uses this to confirm before
    /// Ctrl+W tears the tab down, which matters more here than for the terminal:
    /// closing takes the transcript with it.
    fn is_busy(&self) -> bool {
        self.convo.busy
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        match self.view {
            View::Browse => self.list_subdirectories(),
            View::Session => self.fetch_session(),
        }
    }

    // ---- Directory browsing ---------------------------------------------
    //
    // Same contract as the file browser, so the app's generic navigation does
    // all the work: Right pushes a segment and re-`fetch()`es, Left pops. The
    // path methods are inert in the session view — the cursor is inside the
    // conversation there, and moving `browse_path` under it would send the next
    // `:` somewhere the user never asked for.

    fn push_path(&mut self, segment: &str) {
        if self.view != View::Browse {
            return;
        }
        self.browse_path
            .push(segment.trim_end_matches('/').trim_end_matches('\\'));
    }

    fn pop_path(&mut self) {
        if self.view != View::Browse {
            return;
        }
        if self.browse_path.parent().is_some() && self.browse_path != Path::new("/") {
            self.browse_path.pop();
        }
    }

    /// Where the provider currently is: the folder being listed while browsing,
    /// and the folder the *session* is running in while it is up.
    ///
    /// This is what the app persists on close, so the session answer has to name
    /// the directory `claude` was actually spawned into. Until a first `:` has
    /// succeeded `session_path` is empty, which would send the app's
    /// rebuild-from-root walk nowhere — fall back to the browse path.
    fn current_path(&self) -> &str {
        match self.view {
            View::Browse => self.browse_path.to_str().unwrap_or("/"),
            View::Session if self.session_path.is_empty() => {
                self.browse_path.to_str().unwrap_or("/")
            }
            View::Session => &self.session_path,
        }
    }

    fn set_current_path(&mut self, path: &str) {
        self.browse_path = PathBuf::from(path);
    }

    fn path_is_filesystem(&self) -> bool {
        true
    }

    fn at_root(&self) -> bool {
        self.browse_path == Path::new("/")
    }

    fn commit_edit(&mut self, old: &str, new: &str) -> bool {
        tracing::debug!(old_len = old.len(), new = %new, "claude: commit_edit called");
        // Browsing is read-only: reject the `i` placeholder the app seeds into
        // an empty directory rather than turning it into a file-creation path.
        if self.view != View::Session {
            tracing::debug!("claude: commit_edit rejected — not in the session view");
            return false;
        }
        // The handler strips the `<input>...</input>` wrapper before calling
        // us, so the trailing live slot arrives with `old == ""`. Reject any
        // non-empty `old` (editing a past conversation line is not supported).
        if !old.is_empty() {
            tracing::debug!("claude: commit_edit rejected — non-empty `old`");
            return false;
        }
        let prompt = new.trim();
        if prompt.is_empty() {
            tracing::debug!("claude: commit_edit rejected — empty prompt");
            return false;
        }
        self.ensure_session();
        let Some(session) = self.session.as_mut() else {
            // `ensure_session` already set a descriptive error.
            tracing::error!(error = ?self.error, "claude: commit_edit — no session after ensure_session");
            return false;
        };
        let msg = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{ "type": "text", "text": prompt }],
            },
        });
        let line = msg.to_string();
        tracing::debug!(prompt = %prompt, "claude: writing user message to child");
        if let Err(e) = session.write_user(&line) {
            // Broken pipe → the child died; drop it so the next call re-spawns
            // with `--resume`.
            self.error = Some(format!("claude is not accepting input: {e}"));
            self.session = None;
            self.init_attempted = false;
            return false;
        }
        self.convo.push_user(prompt);
        self.record_history(prompt);
        self.pending_input.clear();
        true
    }

    fn set_input_value(&mut self, value: &str) {
        self.pending_input = value.to_owned();
    }

    fn tick(&mut self) -> bool {
        // Pump unconditionally: the reader thread buffers into `Session::lines`
        // whether or not anyone is looking, and leaving that undrained means a
        // whole response lands in one frame on the next `:`, plus unbounded
        // growth while the user browses.
        let changed = self.pump();
        // Report selectively. The app answers a `true` tick on the active
        // provider by re-fetching the current level, which in the browse view is
        // a `read_dir` per frame for output nobody is reading.
        changed && self.view == View::Session
    }

    fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    fn no_cache(&self) -> bool {
        true
    }

    // ---- Commands --------------------------------------------------------
    //
    // For this provider the app routes `:` straight to `handle_command`, rather
    // than opening the command palette, so these are normally invoked without
    // the list ever being drawn. They are still implemented properly: the WASM
    // plugin bridge and the tests reach the provider through the generic command
    // path, and `commands()` is what tells the app which of the two transitions
    // is currently available — that is how the app decides whether `:` should
    // enter or leave the session without querying view state.

    fn commands(&self) -> Vec<String> {
        match self.view {
            View::Browse => vec![CMD_SESSION.to_owned()],
            View::Session => vec![CMD_BROWSE.to_owned()],
        }
    }

    fn command_label(&self, cmd: &str) -> String {
        register_translations();
        match cmd {
            CMD_SESSION => localize::t("claude-command-session"),
            CMD_BROWSE => localize::t("claude-command-browse"),
            other => other.to_owned(),
        }
    }

    fn handle_command(
        &mut self,
        command: &str,
        _element_key: &str,
        _element_type: i32,
        _error: &mut String,
    ) -> Option<FfonElement> {
        match command {
            CMD_SESSION => self.enter_session(),
            CMD_BROWSE => self.leave_session(),
            _ => {}
        }
        // No element to insert and no error: the app treats this as a state
        // toggle and refreshes the current level, which is exactly the view swap.
        None
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        // All settings take effect on the next spawn — a live session is not
        // hot-restarted.
        match key {
            "claudeBinary" => {
                if !value.is_empty() {
                    self.program = value.to_owned();
                }
            }
            "claudePermissionMode" => {
                if !value.is_empty() {
                    self.permission_mode = value.to_owned();
                }
            }
            "claudeModel" => {
                self.model = if value.is_empty() {
                    None
                } else {
                    Some(value.to_owned())
                };
            }
            "claudeExtraArgs" => {
                self.extra_args = value.split_whitespace().map(str::to_owned).collect();
            }
            "claudeStreamPartial" => {
                self.include_partial = matches!(value, "true" | "1" | "on");
            }
            _ => {}
        }
    }
}

/// Register the Claude provider with the SDK factory and manifest registries.
pub fn register() {
    register_translations();
    register_provider_factory("claude", || Box::new(ClaudeProvider::new()));
    register_builtin_manifest(BuiltinManifest::new("claude", "claude").with_settings(vec![
        SettingDecl::text("claude", "claude binary path", "claudeBinary", "claude"),
        SettingDecl::radio(
            "claude",
            "permission mode",
            "claudePermissionMode",
            &["default", "acceptEdits", "plan", "bypassPermissions"],
            "default",
        ),
        SettingDecl::text("claude", "model override", "claudeModel", ""),
        SettingDecl::text("claude", "extra CLI args", "claudeExtraArgs", ""),
        SettingDecl::checkbox(
            "claude",
            "stream responses token-by-token",
            "claudeStreamPartial",
            true,
        ),
    ]));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_and_display_name_are_claude() {
        let p = ClaudeProvider::new();
        assert_eq!(p.name(), "claude");
        assert_eq!(p.display_name(), "claude");
    }

    #[test]
    fn set_input_value_prefills_input_slot() {
        let mut p = ClaudeProvider::new();
        p.set_input_value("half-typed prompt");
        // Build directly to avoid spawning a real child.
        let out = render::build(&p.convo, &p.history, &p.pending_input);
        let slot = out.last().unwrap().as_obj().unwrap();
        // A plain `+i` slot — an <input> with no <radio> wrapper.
        assert!(slot.key.contains("<input>half-typed prompt</input>"));
        assert!(!slot.key.contains("<radio>"));
    }

    #[test]
    fn commit_edit_rejects_non_empty_old() {
        let mut p = ClaudeProvider::new();
        p.view = View::Session;
        assert!(!p.commit_edit("a past line", "new text"));
    }

    #[test]
    fn commit_edit_rejects_blank_prompt() {
        let mut p = ClaudeProvider::new();
        p.view = View::Session;
        assert!(!p.commit_edit("", "   "));
    }

    #[test]
    fn spawn_failure_surfaces_error() {
        let mut p = ClaudeProvider::new();
        p.program = "definitely-not-claude-xyz-9000".to_owned();
        // Spawning is what `:` does now, not what opening the provider does.
        p.enter_session();
        let err = p.take_error().expect("spawn failure should set an error");
        assert!(err.contains("could not start"), "got: {err}");
        // Re-entering the *same* folder must not retry (init_attempted guard).
        p.enter_session();
        assert!(p.take_error().is_none());
    }

    #[test]
    fn commit_edit_with_no_session_fails_gracefully() {
        let mut p = ClaudeProvider::new();
        p.program = "definitely-not-claude-xyz-9000".to_owned();
        // Set the view rather than calling `enter_session`, so no spawn has been
        // attempted yet and `commit_edit`'s own `ensure_session` is the one that
        // fails — which is what this test's name describes.
        p.view = View::Session;
        assert!(!p.commit_edit("", "hello"));
        assert!(p.take_error().is_some());
    }

    #[test]
    fn no_cache_is_true() {
        assert!(ClaudeProvider::new().no_cache());
    }

    #[test]
    fn on_setting_change_updates_config() {
        let mut p = ClaudeProvider::new();
        p.on_setting_change("claudeBinary", "/opt/claude");
        assert_eq!(p.program, "/opt/claude");
        p.on_setting_change("claudePermissionMode", "plan");
        assert_eq!(p.permission_mode, "plan");
        p.on_setting_change("claudeModel", "claude-opus-4-7");
        assert_eq!(p.model.as_deref(), Some("claude-opus-4-7"));
        p.on_setting_change("claudeModel", "");
        assert!(p.model.is_none());
        p.on_setting_change("claudeExtraArgs", "--foo  --bar baz");
        assert_eq!(p.extra_args, vec!["--foo", "--bar", "baz"]);
        assert!(p.include_partial, "partial streaming defaults on");
        p.on_setting_change("claudeStreamPartial", "false");
        assert!(!p.include_partial);
        p.on_setting_change("claudeStreamPartial", "true");
        assert!(p.include_partial);
        // Empty / unknown keys are ignored.
        p.on_setting_change("claudeBinary", "");
        assert_eq!(p.program, "/opt/claude");
        p.on_setting_change("unrelated", "x");
    }

    #[test]
    fn fetch_without_session_still_returns_input_slot() {
        let mut p = ClaudeProvider::new();
        p.program = "definitely-not-claude-xyz-9000".to_owned();
        p.view = View::Session;
        let out = p.fetch();
        assert!(
            out.last()
                .unwrap()
                .as_obj()
                .unwrap()
                .key
                .contains("<input>")
        );
    }

    // ---- Directory browse view -------------------------------------------

    /// A provider whose browse view is re-rooted at `path`, so the listing is a
    /// known set of directories rather than whatever `/` happens to hold.
    fn browsing(path: &std::path::Path) -> ClaudeProvider {
        let mut p = ClaudeProvider::new();
        // Never let a test reach a real `claude`, whatever the machine has.
        p.program = "definitely-not-claude-xyz-9000".to_owned();
        p.set_current_path(path.to_str().unwrap());
        p
    }

    fn names(elems: &[FfonElement]) -> Vec<String> {
        elems
            .iter()
            .map(|e| match e {
                FfonElement::Obj(o) => o.key.clone(),
                FfonElement::Str(s) => s.clone(),
            })
            .collect()
    }

    #[test]
    fn starts_in_the_browse_view_at_the_filesystem_root() {
        let p = ClaudeProvider::new();
        assert_eq!(p.view, View::Browse);
        assert_eq!(p.current_path(), "/");
        assert!(p.at_root());
    }

    #[test]
    fn init_does_not_spawn_a_child() {
        // The whole point of the browse view: a tab the user only browses costs
        // no process. Spawning is what `:` does.
        let mut p = ClaudeProvider::new();
        p.program = "definitely-not-claude-xyz-9000".to_owned();
        p.init();
        assert!(p.session.is_none());
        assert!(!p.init_attempted, "init must not consume the spawn latch");
        assert!(p.take_error().is_none(), "init must not report a failure");
    }

    #[test]
    fn browse_fetch_does_not_spawn_a_child() {
        // `fetch` runs on every navigation, so a spawn here would start one
        // child per folder the user walks through.
        let dir = tempfile::tempdir().unwrap();
        let mut p = browsing(dir.path());
        let _ = p.fetch();
        let _ = p.fetch();
        assert!(p.session.is_none());
        assert!(p.take_error().is_none());
    }

    #[test]
    fn browse_fetch_lists_only_subdirectories_natural_sorted() {
        let dir = tempfile::tempdir().unwrap();
        for d in ["beta", "Alpha", "item10", "item2"] {
            std::fs::create_dir(dir.path().join(d)).unwrap();
        }
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();

        let mut p = browsing(dir.path());
        // Natural order, case-insensitive: item2 before item10, not after.
        assert_eq!(
            names(&p.fetch()),
            vec!["Alpha", "beta", "item2", "item10"],
            "files are omitted and names sort naturally"
        );
    }

    #[test]
    fn browse_entries_carry_no_input_tag() {
        // A `+i` row here would match the app's live-input-slot check, which
        // special-cases this provider by name — Enter on a folder would try to
        // send it to claude as a prompt.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("workspace")).unwrap();
        let mut p = browsing(dir.path());
        for key in names(&p.fetch()) {
            assert!(
                !key.contains("<input>"),
                "directory row carried a tag: {key}"
            );
        }
    }

    #[test]
    fn browse_fetch_of_an_unreadable_directory_is_empty_not_a_panic() {
        let mut p = browsing(std::path::Path::new("/definitely-not-a-directory-xyz-9000"));
        assert!(p.fetch().is_empty());
    }

    #[test]
    fn commit_edit_is_refused_while_browsing() {
        // The app seeds an `i` placeholder into every empty level, including
        // this read-only tree. Refusing here is what keeps it from becoming a
        // file-creation path.
        let dir = tempfile::tempdir().unwrap();
        let mut p = browsing(dir.path());
        assert!(!p.commit_edit("", "some text"));
        assert!(p.session.is_none(), "a refused edit must not spawn");
    }

    #[test]
    fn push_and_pop_path_walk_the_browse_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("workspace")).unwrap();

        let mut p = browsing(&root);
        p.push_path("workspace");
        assert_eq!(p.current_path(), root.join("workspace").to_str().unwrap());
        p.pop_path();
        assert_eq!(p.current_path(), root.to_str().unwrap());
    }

    #[test]
    fn pop_path_clamps_at_the_root() {
        let mut p = ClaudeProvider::new();
        p.pop_path();
        p.pop_path();
        assert_eq!(p.current_path(), "/");
    }

    #[test]
    #[cfg(unix)]
    fn the_child_really_runs_in_the_folder_being_listed() {
        // The point of the whole browse view. `claude` resolves CLAUDE.md, its
        // skills, its settings and its MCP config from the process's working
        // directory, so this asserts on the *child's own* view of where it is
        // rather than on the field we set. A stub stands in for the CLI: it
        // ignores the claude flags and reports its cwd as one JSONL line.
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let target = root.join("workspace");
        std::fs::create_dir(&target).unwrap();

        let stub = root.join("fake-claude");
        let mut f = std::fs::File::create(&stub).unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        // Newline-terminated so the reader thread's `read_line` completes, and
        // held open so the process does not exit before we look.
        writeln!(f, "printf '%s\\n' \"$PWD\"").unwrap();
        writeln!(f, "sleep 30").unwrap();
        drop(f);
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Exec'ing a just-written file can fail with ETXTBSY when another thread
        // in this process forks while the write fd is still open and inherits
        // it. Rare, but the workspace test run forks constantly, so retry with a
        // fresh provider rather than leave a flake in the suite. A genuinely
        // broken stub fails every attempt and reports the last reason.
        let mut p = None;
        let mut last_err = None;
        for _ in 0..25 {
            let mut cand = ClaudeProvider::new();
            cand.program = stub.to_string_lossy().into_owned();
            cand.set_current_path(root.to_str().unwrap());
            // Walk one level in, exactly as Right does, then press `:`.
            cand.push_path("workspace");
            cand.enter_session();
            if cand.session.is_some() {
                p = Some(cand);
                break;
            }
            last_err = cand.spawn_error.clone();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let mut p = p.unwrap_or_else(|| panic!("the stub should spawn; last error {last_err:?}"));

        let mut reported = None;
        for _ in 0..200 {
            if let Some(line) = p.session.as_mut().unwrap().drain_lines().into_iter().next() {
                reported = Some(line);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }

        assert_eq!(
            reported.as_deref(),
            Some(target.to_str().unwrap()),
            "the child's own cwd must be the folder whose contents were listed",
        );
    }

    #[test]
    fn path_methods_are_inert_in_the_session_view() {
        // The cursor is inside the conversation there, so moving `browse_path`
        // under it would send the next `:` somewhere the user never asked for.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let mut p = browsing(&root);
        p.view = View::Session;
        p.push_path("workspace");
        assert_eq!(p.browse_path, root);
        p.pop_path();
        assert_eq!(p.browse_path, root);
    }

    #[test]
    fn current_path_reports_the_spawn_folder_while_in_session() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let mut p = browsing(&root);
        // Before any `:`, the session view has no folder of its own to report
        // and falls back to the browsed one rather than an empty string.
        p.view = View::Session;
        assert_eq!(p.current_path(), root.to_str().unwrap());

        p.session_path = "/somewhere/else".to_owned();
        assert_eq!(p.current_path(), "/somewhere/else");
    }

    #[test]
    fn path_is_filesystem_so_the_app_walks_it_as_directories() {
        assert!(ClaudeProvider::new().path_is_filesystem());
    }

    // ---- View swapping ----------------------------------------------------

    #[test]
    fn commands_offer_exactly_the_available_transition() {
        // This is how the app reads view state without a trait hook.
        let mut p = ClaudeProvider::new();
        assert_eq!(p.commands(), vec![CMD_SESSION.to_owned()]);
        p.view = View::Session;
        assert_eq!(p.commands(), vec![CMD_BROWSE.to_owned()]);
    }

    #[test]
    fn command_label_localizes_both_transitions() {
        let p = ClaudeProvider::new();
        assert_eq!(p.command_label(CMD_SESSION), "session");
        assert_eq!(p.command_label(CMD_BROWSE), "folders");
        // An unknown id falls back to itself rather than an empty label.
        assert_eq!(p.command_label("nonsense"), "nonsense");
    }

    #[test]
    fn handle_command_swaps_the_view_both_ways() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = browsing(dir.path());
        let mut err = String::new();
        p.handle_command(CMD_SESSION, "", 0, &mut err);
        assert_eq!(p.view, View::Session);
        p.handle_command(CMD_BROWSE, "", 0, &mut err);
        assert_eq!(p.view, View::Browse);
    }

    #[test]
    fn leaving_the_session_keeps_the_folder_the_session_runs_in() {
        // Unlike a shell, claude cannot `cd` itself elsewhere, so there is
        // nothing to follow on the way out.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let mut p = browsing(&root);
        p.enter_session();
        p.leave_session();
        assert_eq!(p.browse_path, root);
    }

    /// A provider with a *live* child, so the respawn branch (which is gated on
    /// one existing) actually runs. `cat` is the established stand-in — it
    /// spawns fine, chokes on the claude flags, and never has to speak the
    /// protocol for the state transitions under test.
    #[cfg(unix)]
    fn browsing_with_a_live_child(path: &std::path::Path) -> ClaudeProvider {
        let mut p = ClaudeProvider::new();
        p.program = "cat".to_owned();
        p.set_current_path(path.to_str().unwrap());
        p.enter_session();
        assert!(p.session.is_some(), "test needs a live child");
        p
    }

    #[test]
    #[cfg(unix)]
    fn entering_from_the_same_folder_keeps_the_conversation() {
        // Stepping out to the folders and back is a view swap, not a restart.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();

        let mut p = browsing_with_a_live_child(&root);
        p.convo.push_user("a question about this project");
        p.last_session_id = Some("live-session".to_owned());

        p.leave_session();
        p.enter_session();

        assert_eq!(p.convo.turns.len(), 1, "the transcript survives");
        assert_eq!(p.last_session_id.as_deref(), Some("live-session"));
        assert_eq!(p.session_path, root.to_str().unwrap());
    }

    #[test]
    #[cfg(unix)]
    fn entering_from_a_different_folder_drops_the_conversation_and_the_resume_id() {
        // claude fixes its cwd at spawn, so a session cannot be walked into
        // another project. Carrying `last_session_id` over would reopen the old
        // folder's transcript via `--resume` — the exact thing to avoid.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let other = root.join("other");
        std::fs::create_dir(&other).unwrap();

        let mut p = browsing_with_a_live_child(&root);
        p.convo.push_user("a question about this project");
        p.last_session_id = Some("old-session".to_owned());
        p.record_history("a question about this project");
        p.pending_input = "half-typed".to_owned();

        p.leave_session();
        p.browse_path = other.clone();
        p.enter_session();

        assert!(p.convo.turns.is_empty(), "the old transcript is dropped");
        assert!(
            p.last_session_id.is_none(),
            "a stale id would --resume the old folder's session in the new folder"
        );
        assert!(p.pending_input.is_empty());
        assert_eq!(p.session_path, other.to_str().unwrap());
        assert_eq!(p.cwd.as_deref(), Some(other.as_path()));
        assert_eq!(
            p.history.len(),
            1,
            "typed prompts are the user's, not the session's — ↑-recall survives"
        );
    }

    #[test]
    fn entering_a_second_folder_after_a_failure_retries_the_spawn() {
        // A folder the user cannot enter says nothing about the next one, so
        // the one-shot latch must not follow them around.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("other")).unwrap();

        let mut p = browsing(&root);
        p.enter_session();
        assert!(p.take_error().is_some(), "first attempt reports");
        assert!(p.spawn_error.is_some());

        p.leave_session();
        p.browse_path = root.join("other");
        p.enter_session();
        assert!(
            p.take_error().is_some(),
            "a different folder must try again rather than fail silently"
        );
    }

    #[test]
    fn spawn_failure_renders_a_row_above_the_input_slot() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let mut p = browsing(&root);
        p.enter_session();

        let out = p.fetch();
        let rows = names(&out);
        let msg = &rows[rows.len() - 2];
        assert!(msg.contains("could not start"), "got: {msg}");
        assert!(
            msg.contains(root.to_str().unwrap()),
            "the failing directory must be named: {msg}"
        );
        assert!(
            out.last()
                .unwrap()
                .as_obj()
                .unwrap()
                .key
                .contains("<input>"),
            "the input slot stays last"
        );
    }

    #[test]
    fn tick_reports_no_change_while_browsing() {
        // Returning `true` here would make the app re-read the *directory* from
        // disk on every frame.
        let dir = tempfile::tempdir().unwrap();
        let mut p = browsing(dir.path());
        assert!(!p.tick());
    }

    #[test]
    fn is_busy_follows_the_conversation() {
        let mut p = ClaudeProvider::new();
        assert!(!p.is_busy());
        p.convo.busy = true;
        assert!(p.is_busy());
    }

    #[test]
    fn process_id_is_none_until_a_session_starts() {
        assert!(ClaudeProvider::new().process_id().is_none());
    }

    #[test]
    fn register_makes_factory_available() {
        register();
        let p = sicompass_sdk::create_provider_by_name("claude");
        assert!(p.is_some());
        assert_eq!(p.unwrap().name(), "claude");
    }
}
