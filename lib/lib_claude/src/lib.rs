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
mod sessions;
mod skills;

pub use sessions::{_set_test_no_ambient_projects, _set_test_projects_root};
pub use skills::_set_test_no_ambient_skills;

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

/// Command id: open one past session's transcript. Carries the row's key, so
/// unlike the other three this one is dispatched with a payload — the app's
/// Right handler passes the session row it was standing on.
///
/// No longer advertised in [`Provider::commands`]: `:` opens the *list* now, and
/// a command the app might fire blind must not be the one that needs an
/// argument. `close repository` in the git client is dispatched the same way.
pub const CMD_SESSION: &str = "session";

/// Command id: swap from the folder listing to the list of past sessions for
/// that folder and the folders below it. The app binds `:` to this.
///
/// Advertised in all three views, doing a different job in each: in the folder
/// listing it is the single transition `:` fires, in the list it marks "this
/// level *is* the list", and in a session it tells the app there is a list to go
/// back to — which is what lets Left be decided by shape rather than by name.
pub const CMD_SESSIONS: &str = "session list";

/// Command id: ask to delete the session row the cursor is on. Carries the row's
/// key. Nothing is removed until the confirmation it renders is answered.
///
/// Deliberately **not** the reserved id `"delete"`. That one routes through
/// `invoke_provider_delete`, which unwinds the cursor to depth 3 before acting —
/// email-shaped surgery, and this list lives at whatever depth the folder it
/// describes does. The git client and the board provider avoid it for the same
/// reason.
pub const CMD_DELETE_SESSION: &str = "delete session";

/// Command id: press the `<button>` row the cursor is on. Carries the row's key.
///
/// Exists because `on_button_press` returns `()` and so cannot hand the app a
/// row to insert, which the `new session` button has to do. Routing every
/// session-list button through one command keeps the button vocabulary
/// (`BTN_*`) entirely inside this crate.
pub const CMD_ACTIVATE_ROW: &str = "activate row";

/// Button id: start a session, typing its first prompt into the row this opens.
const BTN_NEW_SESSION: &str = "new-session";
/// Button id: confirm the pending deletion.
const BTN_CONFIRM_DELETE: &str = "confirm-delete-yes";
/// Button id: call the pending deletion off.
const BTN_CANCEL_DELETE: &str = "confirm-delete-no";

/// How much of a title or first prompt a session row shows.
const SESSION_LABEL_CHARS: usize = 120;

/// Command id: swap back to the folder listing, from either the session list or
/// a session. Idempotent, so the app can fire it without first asking which view
/// is active, and it always means the folder listing — Escape leaves the whole
/// command layer in one press from any depth, which is the split the rest of the
/// app keeps (Escape unwinds modes, Left unwinds one level).
pub const CMD_BROWSE: &str = "browse";

/// Command id: list the Claude Code skills the running session can invoke.
///
/// Unlike the view swaps above this is *not* one. The app fills the prompt with
/// `/name` and the session view stays exactly as it was, which is why it needs
/// no [`View`] variant of its own.
///
/// That restriction is about commands which are not view swaps. The session list
/// *is* one, it is inside the same escape-only layer, and it does answer yes to
/// the app's "is this the session view?" test, so it earns a variant.
pub const CMD_SKILLS: &str = "skills";

/// Which list the provider is currently serving from `fetch()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum View {
    /// Subdirectories of `browse_path`. No `claude` process required.
    #[default]
    Browse,
    /// Past sessions for `browse_path` and the folders below it, under a
    /// `new session` button. No `claude` process required either: this list is
    /// read from Claude Code's own transcripts.
    Sessions,
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
    /// Skills found by the last `handle_command(CMD_SKILLS)`.
    ///
    /// Cached rather than scanned on demand because `command_list_items` takes
    /// `&self` and the app rebuilds the palette list on every keystroke of its
    /// type-to-filter — a `read_dir` there would run twice per character typed.
    /// Refreshed on every palette open, so a skill added while the app is
    /// running shows up the next time `:` is pressed.
    skills: Vec<skills::Skill>,
    /// Last `session_id` seen — used for `--resume` on re-spawn.
    last_session_id: Option<String>,
    error: Option<String>,
    /// Something worth saying that is not a failure, so it must not go through
    /// `take_error` (which renders in the header as an error).
    announcement: Option<String>,

    // --- session list ---------------------------------------------------
    /// Per-transcript metadata cache, so rebuilding the list costs one `stat`
    /// per session rather than a re-read.
    session_index: sessions::SessionIndex,
    /// What the last `fetch()` of the session list showed, in the order it
    /// showed it. The lookup table for the row keys the app hands back.
    listed: Vec<sessions::SessionMeta>,
    /// Session id awaiting a yes/no answer. The list renders the confirmation in
    /// place of that row while this is set, and nothing is removed until the
    /// `yes` button is pressed.
    pending_delete: Option<String>,
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
            skills: Vec::new(),
            last_session_id: None,
            error: None,
            announcement: None,
            session_index: sessions::SessionIndex::default(),
            listed: Vec::new(),
            pending_delete: None,
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

    /// Swap back to the folder listing. The child keeps running: this is a view
    /// change, not a teardown, so `:` back into the same folder resumes the same
    /// conversation.
    ///
    /// Unlike the terminal there is nothing to follow. A shell can `cd` itself
    /// out from under the folder the user picked; `claude` cannot, so
    /// `browse_path` is already the folder the session is running in.
    fn leave_session(&mut self) {
        self.view = View::Browse;
        // A confirmation the user walked away from is a cancel.
        self.pending_delete = None;
    }

    /// Swap to the list of past sessions for `browse_path` and below.
    ///
    /// Reached two ways, and both land here: `:` from the folder listing, and
    /// Left out of a session. The child keeps running either way — like
    /// [`Self::leave_session`] this is a view change, not a teardown.
    ///
    /// `pending_input` is deliberately left alone. It holds the draft in the
    /// live prompt, and a draft surviving a trip out to the folders is the
    /// contract the terminal gives its shell too. It cannot leak into the
    /// new-session row, which the app inserts empty.
    fn enter_sessions(&mut self) {
        self.view = View::Sessions;
        self.pending_delete = None;
    }

    /// Open one past session: render its transcript from disk and arm
    /// `--resume` for the first thing the user sends.
    ///
    /// Deliberately does **not** spawn. Reading a past session costs no process
    /// and no API call, and most of them are opened to be read, not continued.
    /// `commit_edit` spawns on the first prompt, and `ensure_session` hands
    /// `last_session_id` to `--resume` there.
    fn open_session(&mut self, id: &str) -> bool {
        // Re-opening the session that is already up is a pure view swap, the
        // same bargain `enter_session` struck for a folder. Without this, Left
        // out to the list and Right straight back in would kill a running child
        // and replace the live conversation with whatever had reached disk.
        if self.session.is_some() && self.last_session_id.as_deref() == Some(id) {
            self.view = View::Session;
            self.pending_delete = None;
            return true;
        }
        let Some(meta) = self.listed.iter().find(|m| m.id == id).cloned() else {
            return false;
        };
        self.view = View::Session;
        // Session::Drop kills any child still attached to the old transcript.
        self.session = None;
        self.convo = sessions::replay(&meta.path);
        self.convo.session_id = Some(meta.id.clone());
        // The one thing `enter_session` clears and this must set: it is what
        // makes the next spawn a `--resume` rather than a fresh session.
        self.last_session_id = Some(meta.id.clone());
        self.pending_input.clear();
        self.pending_delete = None;
        // From the transcript, not from `browse_path`: the "and below" scan
        // turns up sessions belonging to subfolders, and `claude` fixes its
        // working directory at spawn.
        self.cwd = Some(meta.cwd.clone());
        self.session_path = meta.cwd.to_string_lossy().into_owned();
        self.init_attempted = false;
        self.spawn_error = None;
        true
    }

    /// Start a brand-new session in `browse_path`, dropping whatever was open.
    ///
    /// [`Self::enter_session`]'s same-folder reuse is deliberately absent: the
    /// user asked for a *new* session, so an existing child in the same folder
    /// is not something to rejoin. `last_session_id` is cleared for the same
    /// reason — a new session must not be spawned with `--resume`.
    fn start_new_session(&mut self) {
        self.view = View::Session;
        self.session = None;
        self.convo = Conversation::default();
        // A draft belongs to the session it was being typed into.
        self.pending_input.clear();
        self.last_session_id = None;
        self.pending_delete = None;
        self.cwd = Some(self.browse_path.clone());
        self.session_path = self.browse_path.to_string_lossy().into_owned();
        self.init_attempted = false;
        self.spawn_error = None;
        self.ensure_session();
    }

    /// Enter on a `<button>` row of the session list.
    ///
    /// Returns the row the app should insert and drop into Insert mode on —
    /// only `new session` has one. The button vocabulary never leaves this
    /// crate: the app forwards the row key and learns nothing about it.
    fn activate_row(&mut self, element_key: &str) -> Option<FfonElement> {
        register_translations();
        let name = sicompass_sdk::tags::extract_button_function_name(element_key)?;
        match name.as_str() {
            BTN_NEW_SESSION => {
                // A label prefix, not a bare `<input></input>`: an empty input
                // with no prefix takes the app's create-file commit path, whose
                // fallback calls `handle_escape` and strips the row right after
                // a *successful* commit. It also gives the row something to say.
                // The space lives here rather than in the bundles: Fluent trims
                // trailing whitespace off a value, so a translator cannot put
                // one there even if they wanted to.
                Some(FfonElement::new_str(format!(
                    "{} <input></input>",
                    localize::t("claude-prompt-label")
                )))
            }
            BTN_CANCEL_DELETE => {
                self.pending_delete = None;
                None
            }
            BTN_CONFIRM_DELETE => {
                self.delete_pending();
                None
            }
            _ => None,
        }
    }

    /// Carry out the deletion the confirmation was asking about.
    fn delete_pending(&mut self) {
        register_translations();
        let Some(id) = self.pending_delete.take() else {
            return;
        };
        let Some(meta) = self.listed.iter().find(|m| m.id == id).cloned() else {
            return;
        };
        let label = self.row_label(&meta);
        if sessions::delete_transcript(&meta.path) {
            self.session_index.forget(&meta.path);
            self.announcement = Some(format!(
                "{} {}",
                localize::t("claude-session-deleted"),
                label
            ));
        } else {
            self.error = Some(localize::t("claude-session-delete-failed"));
        }
    }

    /// A stand-in row for the session that is up right now, when the scan did
    /// not turn it up. `None` once the real transcript is on disk, or when no
    /// session has been started.
    fn live_session_row(&self, listed: &[sessions::SessionMeta]) -> Option<sessions::SessionMeta> {
        let id = self.last_session_id.as_deref()?;
        if listed.iter().any(|m| m.id == id) {
            return None;
        }
        // The first thing sent in this session is the best label available, and
        // it is what the real row will fall back to anyway until Claude titles
        // it.
        let first_prompt = self.history.first().cloned();
        let cwd = PathBuf::from(&self.session_path);
        Some(sessions::SessionMeta {
            id: id.to_owned(),
            path: PathBuf::new(),
            cwd,
            title: None,
            first_prompt,
            modified: std::time::SystemTime::now(),
        })
    }

    /// What one session row reads: Claude's own title, else the first prompt,
    /// else a placeholder for a session that has neither yet.
    fn row_label(&self, meta: &sessions::SessionMeta) -> String {
        match meta.label() {
            Some(l) => sessions::one_line(l, SESSION_LABEL_CHARS),
            None => localize::t("claude-untitled-session"),
        }
    }

    /// The session list: a `new session` button, then one row per past session,
    /// most recent first.
    ///
    /// Each session is a **childless `Obj`**, which is the one place in this
    /// crate that is allowed to be. The app renders every `Obj` with a `+`, and
    /// here the `+` is honest — Right on the row really does open something —
    /// but it opens it by swapping the view rather than by descending, so there
    /// is nothing to hang underneath. `handlers::open_session_row` is what makes
    /// it true; see `no_element_is_a_childless_obj` for the invariant that still
    /// holds over the transcript.
    fn fetch_sessions(&mut self) -> Vec<FfonElement> {
        register_translations();
        let mut listed = sessions::scan(&self.browse_path, &mut self.session_index);
        // Claude Code writes the transcript as it goes, so a session started
        // moments ago may have no file yet, or a file with no `ai-title`. Left
        // out of it would then land on a list that does not contain the thing
        // the user was just looking at. Synthesise the row from what is in
        // memory until the real one turns up.
        if let Some(live) = self.live_session_row(&listed) {
            listed.insert(0, live);
        }
        let mut out = Vec::with_capacity(listed.len() + 3);
        out.push(FfonElement::new_str(format!(
            "<button>{BTN_NEW_SESSION}</button>{}",
            localize::t("claude-new-session")
        )));
        for meta in &listed {
            let label = self.row_label(meta);
            if self.pending_delete.as_deref() == Some(meta.id.as_str()) {
                // The confirmation stands in for the row it is about, so the
                // session being deleted cannot be misread off a neighbouring
                // line. `no` comes first: the list refresh clamps the cursor to
                // the top of what replaced the row, so the safe answer is the
                // one the cursor lands on.
                out.push(FfonElement::new_str(render::escape_markup(&format!(
                    "{} {}",
                    localize::t("claude-confirm-delete"),
                    label
                ))));
                out.push(FfonElement::new_str(format!(
                    "<button>{BTN_CANCEL_DELETE}</button>{}",
                    localize::t("claude-confirm-delete-no")
                )));
                out.push(FfonElement::new_str(format!(
                    "<button>{BTN_CONFIRM_DELETE}</button>{}",
                    localize::t("claude-confirm-delete-yes")
                )));
                continue;
            }
            // The id rides outside any other tag so the app can hand the raw key
            // back and `open_session` can resolve the row by identity rather
            // than by a label two sessions could share.
            out.push(FfonElement::new_obj(format!(
                "{}{}",
                sicompass_sdk::tags::format_id(&meta.id),
                render::escape_markup(&label)
            )));
        }
        self.listed = listed;
        out
    }

    /// Send one prompt to the child, spawning it first if need be.
    ///
    /// Shared by the live input slot and the session list's first-prompt row, so
    /// a session started from the list logs, records and clears exactly like one
    /// continued from inside.
    fn send_prompt(&mut self, prompt: &str) -> bool {
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

    /// The session view: the conversation, then the live `<input>` slot, with a
    /// spawn failure spliced in directly above that slot.
    ///
    /// Splicing here rather than inside [`render::build`] keeps the conversation
    /// projection untouched. `build` always ends with the input slot, so
    /// inserting at `len - 1` puts the message where the last thing that
    /// happened is the last thing read out before the prompt.
    fn fetch_session(&mut self) -> Vec<FfonElement> {
        self.pump();
        let mut out = render::build(&self.convo, &self.pending_input);
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
    /// Ctrl+Shift+T tears the tab down, which matters more here than for the terminal:
    /// closing takes the transcript with it.
    fn is_busy(&self) -> bool {
        self.convo.busy
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        match self.view {
            View::Browse => self.list_subdirectories(),
            View::Sessions => self.fetch_sessions(),
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
            // The list describes the folder being browsed, so the app's
            // rebuild-from-root walk still lands on that folder's level.
            View::Browse | View::Sessions => self.browse_path.to_str().unwrap_or("/"),
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
        // The session list's first-prompt row, which the `new session` button
        // opened. Ahead of both rejections below, and on purpose: the view is
        // `Sessions` rather than `Session`, and a re-edit of the row arrives
        // with a non-empty `old`. It is the only editable row that level has,
        // so no further test is needed to recognise it.
        if self.view == View::Sessions {
            let prompt = new.trim().to_owned();
            if prompt.is_empty() {
                // Refusing leaves the cursor in the row to try again, rather
                // than starting a session with nothing to say.
                return false;
            }
            self.start_new_session();
            if !self.send_prompt(&prompt) {
                // The child did not start. The swap still happened and
                // `spawn_error` is already on screen above the slot, so report
                // success: landing in the session with the reason showing beats
                // leaving the user in a list with an unexplained refusal. The
                // prompt goes back into the slot rather than being thrown away.
                self.pending_input = prompt;
            }
            return true;
        }
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
        let prompt = new.trim().to_owned();
        if prompt.is_empty() {
            tracing::debug!("claude: commit_edit rejected — empty prompt");
            return false;
        }
        self.send_prompt(&prompt)
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

    /// Deleting a session is worth saying out loud but is not a failure, so it
    /// goes here rather than through `take_error`, which renders in the header
    /// as an error and would read as one.
    fn take_announcement(&mut self) -> Option<String> {
        self.announcement.take()
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

    /// `CMD_BROWSE` stays **first** in the session arm. The app reads
    /// "am I in the session view?" as "does this contain `browse`?", which is
    /// order-independent, but the one place that reads `.next()` — the `:`
    /// handler deciding which transition to fire — is only safe because it
    /// early-returns inside a session. Leading with the view swap costs nothing
    /// and removes the trap.
    fn commands(&self) -> Vec<String> {
        match self.view {
            View::Browse => vec![CMD_SESSIONS.to_owned()],
            View::Sessions => vec![CMD_BROWSE.to_owned(), CMD_SESSIONS.to_owned()],
            View::Session => vec![
                CMD_BROWSE.to_owned(),
                CMD_SESSIONS.to_owned(),
                CMD_SKILLS.to_owned(),
            ],
        }
    }

    fn command_label(&self, cmd: &str) -> String {
        register_translations();
        match cmd {
            CMD_SESSION => localize::t("claude-command-session"),
            CMD_SESSIONS => localize::t("claude-command-sessions"),
            CMD_DELETE_SESSION => localize::t("claude-command-delete-session"),
            CMD_BROWSE => localize::t("claude-command-browse"),
            CMD_SKILLS => localize::t("claude-command-skills"),
            other => other.to_owned(),
        }
    }

    fn handle_command(
        &mut self,
        command: &str,
        element_key: &str,
        _element_type: i32,
        _error: &mut String,
    ) -> Option<FfonElement> {
        match command {
            CMD_SESSION => {
                // Carries the row it was fired on, unlike every other command
                // here. An id that no longer resolves (a list rebuilt under a
                // stale key) leaves the view alone rather than opening the
                // wrong transcript.
                if let Some(id) = sicompass_sdk::tags::extract_id(element_key) {
                    self.open_session(&id);
                }
            }
            CMD_SESSIONS => self.enter_sessions(),
            CMD_DELETE_SESSION => {
                if let Some(id) = sicompass_sdk::tags::extract_id(element_key) {
                    // Only arms the confirmation. The transcript is still there.
                    // The stand-in row for a just-started session has no file
                    // behind it yet, so there is nothing to offer to delete.
                    let real = self
                        .listed
                        .iter()
                        .any(|m| m.id == id && !m.path.as_os_str().is_empty());
                    if real {
                        self.pending_delete = Some(id);
                    }
                }
            }
            CMD_ACTIVATE_ROW => return self.activate_row(element_key),
            CMD_BROWSE => self.leave_session(),
            CMD_SKILLS => {
                // Scanned here, in the `&mut self` hook, because
                // `command_list_items` is `&self` and the app calls it on every
                // list rebuild.
                //
                // `session_path` rather than `browse_path`: `claude` reads
                // `.claude/skills/` from the directory it was *spawned* in. It
                // is assigned before the spawn is attempted, so this is right
                // even when the spawn failed.
                //
                // Deliberately never writes `*error`: the caller rebuilds the
                // list immediately afterwards, and that clears `error_message`,
                // so anything reported here would vanish. Discovery is
                // silent-skip by construction, so there is nothing to report.
                self.skills = skills::discover(&self.session_path, &self.program);
            }
            _ => {}
        }
        // No element to insert and no error: the app treats this as a state
        // toggle and refreshes the current level, which is exactly the view swap.
        None
    }

    fn command_list_items(&self, command: &str) -> Vec<sicompass_sdk::provider::ListItem> {
        if command != CMD_SKILLS {
            return Vec::new();
        }
        self.skills
            .iter()
            .map(|s| sicompass_sdk::provider::ListItem {
                // Human text in `label`, payload in `data` — the shape the file
                // browser's "open file with" uses.
                //
                // `data` is the text to insert, not the bare name: the app
                // splices it into the live input slot verbatim and knows nothing
                // about skills. The leading `/` belongs here, where the fact
                // that a skill is invoked as a slash command is known.
                label: s.label(),
                data: format!("/{}", s.name),
            })
            .collect()
    }

    fn execute_command(&mut self, command: &str, selection: &str) -> bool {
        if command != CMD_SKILLS || selection.is_empty() {
            return false;
        }
        // Not on the hot path: the app owns the FFON and splices the selection
        // into the input slot itself, so the `:` palette never reaches here.
        // Implemented so the generic command route — the WASM bridge, and any
        // test driving the provider through the SDK trait — is not a silent
        // no-op. `selection` is already the full insert text (`/name`), and
        // appending rather than replacing matches the app-side behaviour.
        self.pending_input.push_str(selection);
        true
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

    /// The trailing input slot's key, whatever element type it is: a `Str`
    /// (`-i`) while there is no recall history to expand into, an `Obj` (`+i`)
    /// once history gives it children.
    fn slot_key(out: &[FfonElement]) -> &str {
        let last = out.last().expect("an input slot");
        last.as_str()
            .or_else(|| last.as_obj().map(|o| o.key.as_str()))
            .expect("the slot carries a key")
    }

    #[test]
    fn set_input_value_prefills_input_slot() {
        let mut p = ClaudeProvider::new();
        p.set_input_value("half-typed prompt");
        // Build directly to avoid spawning a real child.
        let out = render::build(&p.convo, &p.pending_input);
        let key = slot_key(&out);
        // A plain editable slot — an <input> with no <radio> wrapper.
        assert!(key.contains("<input>half-typed prompt</input>"));
        assert!(!key.contains("<radio>"));
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
        p.start_new_session();
        let err = p.take_error().expect("spawn failure should set an error");
        assert!(err.contains("could not start"), "got: {err}");

        // Asking for another new session *does* retry, and should: it takes a
        // deliberate button press and a typed prompt to get here, so a failure
        // is something to try again rather than something to latch.
        //
        // The hazard the one-shot latch used to cover — `:` spawning once per
        // keypress — is gone by construction now: `:` opens the session list,
        // which starts nothing at all. `the_session_list_is_reached_and_left_
        // without_spawning` pins that.
        p.start_new_session();
        let again = p
            .take_error()
            .expect("a fresh attempt reports its own failure");
        assert!(again.contains("could not start"), "got: {again}");
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
        assert!(slot_key(&out).contains("<input>"));
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
            cand.start_new_session();
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

    // ---- Skills palette ---------------------------------------------------

    /// Point this thread at a fake projects root and write one transcript into
    /// it for `cwd`. Returns the tempdir, which must outlive the test.
    fn with_fake_sessions(cwd: &std::path::Path, entries: &[(&str, &str)]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("projects");
        let dir = root.join(sessions::dashify(cwd));
        std::fs::create_dir_all(&dir).unwrap();
        for (id, title) in entries {
            let body = format!(
                concat!(
                    r#"{{"type":"user","cwd":{},"message":{{"role":"user","content":"first prompt of {}"}}}}"#,
                    "\n",
                    r#"{{"type":"ai-title","aiTitle":{},"sessionId":"{}"}}"#,
                    "\n"
                ),
                serde_json::to_string(&cwd.to_string_lossy().into_owned()).unwrap(),
                id,
                serde_json::to_string(title).unwrap(),
                id
            );
            std::fs::write(dir.join(format!("{id}.jsonl")), body).unwrap();
        }
        _set_test_projects_root(Some(root));
        tmp
    }

    /// Key of the row at `idx`, raw markup and all.
    fn row_key(out: &[FfonElement], idx: usize) -> String {
        match &out[idx] {
            FfonElement::Obj(o) => o.key.clone(),
            FfonElement::Str(s) => s.clone(),
        }
    }

    /// Write `<root>/.claude/skills/<name>/SKILL.md`.
    fn project_skill(root: &std::path::Path, name: &str, contents: &str) {
        let dir = root.join(".claude").join("skills").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), contents).unwrap();
    }

    #[test]
    fn the_session_view_offers_the_view_swap_before_the_skills() {
        // The app reads "am I in the session view?" as "does this offer
        // `browse`?", and the one caller that takes the first entry is only
        // safe because it early-returns in a session. Leading with the swap
        // keeps that true regardless.
        let mut p = ClaudeProvider::new();
        assert_eq!(p.commands(), vec![CMD_SESSIONS.to_owned()]);

        p.view = View::Sessions;
        assert_eq!(
            p.commands(),
            vec![CMD_BROWSE.to_owned(), CMD_SESSIONS.to_owned()]
        );

        p.view = View::Session;
        assert_eq!(
            p.commands(),
            vec![
                CMD_BROWSE.to_owned(),
                CMD_SESSIONS.to_owned(),
                CMD_SKILLS.to_owned()
            ],
            "the session also advertises the list, so the app knows Left has \
             somewhere to go without being told which provider this is"
        );

        for view in [View::Sessions, View::Session] {
            p.view = view;
            assert_eq!(
                p.commands().iter().filter(|c| *c == CMD_BROWSE).count(),
                1,
                "exactly one `browse`, or the session-view check breaks"
            );
            assert_eq!(
                p.commands().first().map(String::as_str),
                Some(CMD_BROWSE),
                "`browse` stays first in every view that offers it"
            );
        }
    }

    #[test]
    fn handle_command_skills_fills_the_cache_and_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        project_skill(&root, "review", "---\ndescription: Review the diff\n---\n");

        let mut p = browsing(&root);
        p.start_new_session();
        let mut err = String::new();
        let out = p.handle_command(CMD_SKILLS, "", 0, &mut err);

        assert!(out.is_none(), "not an element-insert command");
        assert!(err.is_empty(), "an error here would be silently discarded");
        assert_eq!(p.skills.len(), 1);
        assert_eq!(p.skills[0].name, "review");
    }

    #[test]
    fn skills_are_discovered_from_the_folder_the_session_runs_in() {
        // Not `browse_path`: claude reads `.claude/skills/` from the directory
        // it was spawned in, and the spawn folder is recorded even when the
        // spawn itself failed.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let inner = root.join("workspace");
        std::fs::create_dir(&inner).unwrap();
        project_skill(&inner, "deep", "");

        let mut p = browsing(&root);
        p.push_path("workspace");
        p.start_new_session();
        assert!(p.session.is_none(), "the bogus binary cannot spawn");
        p.handle_command(CMD_SKILLS, "", 0, &mut String::new());
        assert_eq!(p.skills.len(), 1, "found despite the failed spawn");
    }

    #[test]
    fn command_list_items_carry_the_insert_text_in_data() {
        // `data` is what the app splices into the input slot, verbatim — so the
        // leading `/` lives here, not in the app.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        project_skill(&root, "review", "---\ndescription: Review the diff\n---\n");

        let mut p = browsing(&root);
        p.start_new_session();
        p.handle_command(CMD_SKILLS, "", 0, &mut String::new());

        let items = p.command_list_items(CMD_SKILLS);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].data, "/review");
        assert_eq!(items[0].label, "review - Review the diff");
    }

    #[test]
    fn command_list_items_is_empty_for_other_commands() {
        let mut p = ClaudeProvider::new();
        p.view = View::Session;
        assert!(p.command_list_items(CMD_BROWSE).is_empty());
        assert!(p.command_list_items("nonsense").is_empty());
    }

    #[test]
    fn execute_command_appends_the_selection_to_the_prompt() {
        let mut p = ClaudeProvider::new();
        p.view = View::Session;
        p.pending_input = "explain ".to_owned();
        assert!(p.execute_command(CMD_SKILLS, "/review"));
        assert_eq!(p.pending_input, "explain /review");
        // And it reaches the rendered slot.
        let out = render::build(&p.convo, &p.pending_input);
        assert!(slot_key(&out).contains("<input>explain /review</input>"));
    }

    #[test]
    fn execute_command_ignores_other_commands_and_empty_selections() {
        let mut p = ClaudeProvider::new();
        assert!(!p.execute_command(CMD_BROWSE, "/review"));
        assert!(!p.execute_command(CMD_SKILLS, ""));
        assert!(p.pending_input.is_empty());
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
        // `:` reaches the session list now, not the session. `browse` is still
        // the one way back and still means the folder listing from either view,
        // which is what lets Escape leave the whole layer in one press.
        let dir = tempfile::tempdir().unwrap();
        let mut p = browsing(dir.path());
        let mut err = String::new();
        p.handle_command(CMD_SESSIONS, "", 0, &mut err);
        assert_eq!(p.view, View::Sessions);
        p.handle_command(CMD_BROWSE, "", 0, &mut err);
        assert_eq!(p.view, View::Browse);

        // And from a session, in one step rather than down the ladder.
        p.view = View::Session;
        p.handle_command(CMD_BROWSE, "", 0, &mut err);
        assert_eq!(p.view, View::Browse);
    }

    #[test]
    fn the_session_list_is_reached_and_left_without_spawning() {
        // The whole point of the list: opening it costs no process. Enabling
        // claude is free until a session is actually started.
        let dir = tempfile::tempdir().unwrap();
        let mut p = browsing(dir.path());
        p.program = "definitely-not-claude-xyz-9000".to_owned();
        let mut err = String::new();
        p.handle_command(CMD_SESSIONS, "", 0, &mut err);
        assert!(p.process_id().is_none(), "no child was started");
        assert!(p.take_error().is_none(), "and nothing failed to start");
    }

    #[test]
    fn leaving_the_session_keeps_the_folder_the_session_runs_in() {
        // Unlike a shell, claude cannot `cd` itself elsewhere, so there is
        // nothing to follow on the way out.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let mut p = browsing(&root);
        p.start_new_session();
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
        p.start_new_session();
        assert!(p.session.is_some(), "test needs a live child");
        p
    }

    #[test]
    #[cfg(unix)]
    fn reopening_the_live_session_keeps_the_conversation() {
        // Stepping out to the session list and back in is a view swap, not a
        // restart. The route changed when the list landed (Left out, Right back
        // in, on the session's own row) but the guarantee did not: a running
        // child and its transcript must survive the round trip.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();

        let mut p = browsing_with_a_live_child(&root);
        p.convo.push_user("a question about this project");
        p.last_session_id = Some("live-session".to_owned());

        p.enter_sessions();
        assert!(p.open_session("live-session"), "the live session reopens");

        assert_eq!(p.convo.turns.len(), 1, "the transcript survives");
        assert_eq!(p.last_session_id.as_deref(), Some("live-session"));
        assert_eq!(p.session_path, root.to_str().unwrap());
    }

    #[test]
    #[cfg(unix)]
    fn starting_a_new_session_drops_the_conversation_and_the_resume_id() {
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

        p.enter_sessions();
        p.browse_path = other.clone();
        p.start_new_session();

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
        p.start_new_session();
        assert!(p.take_error().is_some(), "first attempt reports");
        assert!(p.spawn_error.is_some());

        p.leave_session();
        p.browse_path = root.join("other");
        p.start_new_session();
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
        p.start_new_session();

        let out = p.fetch();
        let rows = names(&out);
        let msg = &rows[rows.len() - 2];
        assert!(msg.contains("could not start"), "got: {msg}");
        assert!(
            msg.contains(root.to_str().unwrap()),
            "the failing directory must be named: {msg}"
        );
        assert!(
            slot_key(&out).contains("<input>"),
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

    // ---- The session list ------------------------------------------------

    #[test]
    fn the_session_list_leads_with_the_new_session_button() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let _fake = with_fake_sessions(&root, &[("s1", "Fix search wrap layout")]);

        let mut p = browsing(&root);
        p.enter_sessions();
        let out = p.fetch();

        assert_eq!(out.len(), 2, "the button and one session, got {out:?}");
        assert!(
            sicompass_sdk::tags::has_button(&row_key(&out, 0)),
            "the button is first, so the cursor lands on it"
        );
        assert_eq!(
            sicompass_sdk::tags::strip_display(&row_key(&out, 1)),
            "Fix search wrap layout"
        );
        _set_test_projects_root(None);
    }

    #[test]
    fn a_session_row_is_a_childless_obj_on_purpose() {
        // The app renders every Obj with `+`, and the `+` is honest here: Right
        // on the row opens the session. It opens it by swapping the view rather
        // than by descending, so there is deliberately nothing underneath —
        // the one place in this crate exempt from `no_element_is_a_childless_obj`.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let _fake = with_fake_sessions(&root, &[("s1", "Some title")]);

        let mut p = browsing(&root);
        p.enter_sessions();
        let out = p.fetch();
        match &out[1] {
            FfonElement::Obj(o) => assert!(o.children.is_empty()),
            other => panic!("a session row must be an Obj, got {other:?}"),
        }
        _set_test_projects_root(None);
    }

    #[test]
    fn a_session_row_carries_its_id_so_two_alike_titles_stay_apart() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let _fake = with_fake_sessions(&root, &[("s1", "Same title"), ("s2", "Same title")]);

        let mut p = browsing(&root);
        p.enter_sessions();
        let out = p.fetch();
        let mut ids: Vec<String> = out
            .iter()
            .skip(1)
            .filter_map(|e| sicompass_sdk::tags::extract_id(&row_key(&[e.clone()], 0)))
            .collect();
        ids.sort();
        assert_eq!(ids, vec!["s1", "s2"], "identity travels with the row");
        _set_test_projects_root(None);
    }

    #[test]
    fn the_new_session_button_opens_a_labelled_input_row() {
        // A bare `<input></input>` would take the app's create-file commit path,
        // whose fallback strips the row right after a successful commit. The
        // prefix is what keeps it on the ordinary path, and what gives the row
        // something to say.
        let mut p = ClaudeProvider::new();
        p.view = View::Sessions;
        let mut err = String::new();
        let key = format!("<button>{BTN_NEW_SESSION}</button>new session");
        let row = p
            .handle_command(CMD_ACTIVATE_ROW, &key, 0, &mut err)
            .expect("the button hands back a row to type into");
        let text = row_key(&[row], 0);
        assert!(sicompass_sdk::tags::has_input(&text));
        assert!(
            !text.starts_with("<input>"),
            "the row needs a label before the input, got {text:?}"
        );
    }

    #[test]
    fn committing_the_first_prompt_starts_a_fresh_session() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let mut p = browsing(&root);
        p.program = "definitely-not-claude-xyz-9000".to_owned();
        p.last_session_id = Some("some-old-session".to_owned());
        p.enter_sessions();

        assert!(!p.commit_edit("", "  "), "a blank prompt is refused");
        assert_eq!(p.view, View::Sessions, "and leaves the cursor in the row");

        // The spawn fails (the binary cannot exist). The swap still reports
        // success, so the user lands in the session with the reason on screen
        // rather than being refused in the list with no explanation.
        assert!(p.commit_edit("", "what does this crate do?"));
        assert_eq!(p.view, View::Session);
        assert!(p.spawn_error.is_some(), "and the reason is on screen");
        assert_eq!(
            p.pending_input, "what does this crate do?",
            "the prompt goes back in the slot rather than being thrown away"
        );
        assert!(
            p.last_session_id.is_none(),
            "a new session must not be spawned with --resume"
        );
    }

    #[test]
    fn opening_a_past_session_resumes_it_without_spawning() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let _fake = with_fake_sessions(&root, &[("s7", "An older session")]);

        let mut p = browsing(&root);
        p.program = "definitely-not-claude-xyz-9000".to_owned();
        p.enter_sessions();
        let out = p.fetch();
        let key = row_key(&out, 1);

        let mut err = String::new();
        p.handle_command(CMD_SESSION, &key, 1, &mut err);

        assert_eq!(p.view, View::Session);
        assert_eq!(
            p.last_session_id.as_deref(),
            Some("s7"),
            "armed for --resume on the first thing sent"
        );
        assert!(
            p.process_id().is_none(),
            "reading a past session costs no process and no API call"
        );
        assert!(
            !p.convo.turns.is_empty(),
            "the transcript is rendered from disk, not left blank"
        );
        assert!(p.take_error().is_none());
        _set_test_projects_root(None);
    }

    #[test]
    fn tick_is_silent_in_the_session_list() {
        // A `true` tick makes the app re-fetch the level every frame, which here
        // would be a directory scan per frame for a list nothing is changing.
        let mut p = ClaudeProvider::new();
        p.view = View::Sessions;
        assert!(!p.tick());
    }

    #[test]
    fn delete_confirms_before_it_removes_anything() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let fake = with_fake_sessions(&root, &[("doomed", "Throwaway")]);
        let file = fake
            .path()
            .join("projects")
            .join(sessions::dashify(&root))
            .join("doomed.jsonl");

        let mut p = browsing(&root);
        p.enter_sessions();
        let key = row_key(&p.fetch(), 1);
        let mut err = String::new();

        p.handle_command(CMD_DELETE_SESSION, &key, 1, &mut err);
        let asked = p.fetch();
        assert!(file.exists(), "arming the confirmation removes nothing");
        assert_eq!(asked.len(), 4, "question plus two answers, got {asked:?}");
        assert!(
            row_key(&asked, 2).contains(BTN_CANCEL_DELETE),
            "the safe answer comes first, where the cursor lands"
        );

        // Saying no puts the list back untouched.
        let no = format!("<button>{BTN_CANCEL_DELETE}</button>x");
        p.handle_command(CMD_ACTIVATE_ROW, &no, 0, &mut err);
        assert_eq!(p.fetch().len(), 2);
        assert!(file.exists());

        // Saying yes removes the transcript and says so.
        p.handle_command(CMD_DELETE_SESSION, &key, 1, &mut err);
        let yes = format!("<button>{BTN_CONFIRM_DELETE}</button>x");
        p.handle_command(CMD_ACTIVATE_ROW, &yes, 0, &mut err);
        assert!(!file.exists(), "the transcript is gone");
        assert_eq!(p.fetch().len(), 1, "and so is its row");
        let said = p.take_announcement().expect("a deletion is announced");
        assert!(said.contains("Throwaway"), "naming what went, got {said:?}");
        assert!(
            p.take_error().is_none(),
            "a deletion is not a failure and must not read as one"
        );
        _set_test_projects_root(None);
    }

    #[test]
    fn a_just_started_session_has_a_row_before_its_transcript_lands() {
        // Claude Code writes the transcript as it goes, so Left out of a session
        // started moments ago would otherwise land on a list that does not
        // contain the thing the user was just looking at.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let _fake = with_fake_sessions(&root, &[]);

        let mut p = browsing(&root);
        p.last_session_id = Some("brand-new".to_owned());
        p.session_path = root.to_string_lossy().into_owned();
        p.record_history("the thing I just asked");
        p.enter_sessions();

        let out = p.fetch();
        assert_eq!(out.len(), 2, "the button and the live session, got {out:?}");
        assert_eq!(
            sicompass_sdk::tags::strip_display(&row_key(&out, 1)),
            "the thing I just asked"
        );
        _set_test_projects_root(None);
    }

    #[test]
    fn the_stand_in_row_cannot_be_deleted() {
        // It has no file behind it yet, so there is nothing to offer to remove —
        // and `delete_transcript` would be handed an empty path.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let _fake = with_fake_sessions(&root, &[]);

        let mut p = browsing(&root);
        p.last_session_id = Some("brand-new".to_owned());
        p.session_path = root.to_string_lossy().into_owned();
        p.enter_sessions();
        let key = row_key(&p.fetch(), 1);

        let mut err = String::new();
        p.handle_command(CMD_DELETE_SESSION, &key, 1, &mut err);
        assert!(p.pending_delete.is_none(), "no confirmation is offered");
        assert_eq!(p.fetch().len(), 2, "the list is unchanged");
        _set_test_projects_root(None);
    }

    #[test]
    fn the_session_list_is_empty_without_a_projects_root() {
        // The guard that keeps `cargo test` away from the developer's own
        // transcripts: no injected root means nothing is read at all.
        _set_test_projects_root(None);
        let dir = tempfile::tempdir().unwrap();
        let mut p = browsing(dir.path());
        p.enter_sessions();
        assert_eq!(p.fetch().len(), 1, "just the new-session button");
    }
}
