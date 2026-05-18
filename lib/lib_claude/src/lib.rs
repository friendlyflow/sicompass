//! Sicompass Claude provider.
//!
//! Runs the `claude` CLI once in streaming-JSON mode
//! (`--print --output-format stream-json --input-format stream-json --verbose`),
//! keeps the process alive for a multi-turn conversation, and renders the
//! JSONL event stream into a navigable FFON document.
//!
//! Unlike running `claude` inside the terminal provider — which detects the
//! TUI and routes it into an opaque cell-grid dashboard — this provider treats
//! Claude's structured JSON protocol as a first-class FFON source: assistant
//! messages, tool calls, tool results, and the cost summary each become
//! navigable nodes.
//!
//! * `fetch()` builds the conversation tree plus a trailing `<input>` slot.
//! * `commit_edit()` writes the typed prompt to the child as a `user` message.
//! * `tick()` drains buffered JSONL lines and folds them into conversation
//!   state.
//!
//! The child process lives in [`session`]; the event schema in [`events`]; the
//! conversation state and FFON projection in [`render`].

mod events;
mod render;
mod session;

use std::path::PathBuf;

use sicompass_sdk::{
    register_builtin_manifest, register_provider_factory, BuiltinManifest, FfonElement, Provider,
    SettingDecl,
};

use render::Conversation;
use session::{Session, SessionConfig};

/// Cap on remembered prompts for `<input>`-slot recall.
const HISTORY_CAP: usize = 1000;

/// A dedicated provider that streams a `claude` session as FFON.
pub struct ClaudeProvider {
    // --- configuration (applied on next spawn) --------------------------
    program: String,
    permission_mode: String,
    model: Option<String>,
    extra_args: Vec<String>,
    cwd: Option<PathBuf>,

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
        match Session::spawn(&self.session_config(resume)) {
            Ok(s) => {
                self.session = Some(s);
                if restarting {
                    self.error = Some("claude session restarted".to_owned());
                }
            }
            Err(e) => {
                self.error = Some(if e.kind() == std::io::ErrorKind::NotFound {
                    format!(
                        "could not start `{}`: binary not found on PATH",
                        self.program
                    )
                } else {
                    format!("could not start `{}`: {e}", self.program)
                });
            }
        }
    }

    /// Drain buffered JSONL lines, fold them into conversation state, and watch
    /// for an unexpected child exit. Returns `true` if anything changed.
    fn pump(&mut self) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        let mut changed = false;
        for line in session.drain_lines() {
            if let Some(ev) = events::parse_line(&line) {
                self.convo.apply(ev);
                changed = true;
            }
        }
        if let Some(sid) = &self.convo.session_id {
            self.last_session_id = Some(sid.clone());
        }
        // Unexpected child exit: surface stderr, drop the session, and allow a
        // `--resume` re-spawn on the next `ensure_session()`.
        if !session.is_alive() {
            let stderr = session.take_stderr();
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

    fn init(&mut self) {
        self.ensure_session();
    }

    fn cleanup(&mut self) {
        // Dropping the Session kills the child (Session::Drop).
        self.session = None;
        self.init_attempted = false;
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        self.ensure_session();
        self.pump();
        render::build(&self.convo, &self.history, &self.pending_input)
    }

    fn commit_edit(&mut self, old: &str, new: &str) -> bool {
        // The handler strips the `<input>...</input>` wrapper before calling
        // us, so the trailing live slot arrives with `old == ""`. Reject any
        // non-empty `old` (editing a past conversation line is not supported).
        if !old.is_empty() {
            return false;
        }
        let prompt = new.trim();
        if prompt.is_empty() {
            return false;
        }
        self.ensure_session();
        let Some(session) = self.session.as_mut() else {
            // `ensure_session` already set a descriptive error.
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
        self.pump()
    }

    fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    fn no_cache(&self) -> bool {
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
                self.extra_args =
                    value.split_whitespace().map(str::to_owned).collect();
            }
            _ => {}
        }
    }
}

/// Register the Claude provider with the SDK factory and manifest registries.
pub fn register() {
    register_provider_factory("claude", || Box::new(ClaudeProvider::new()));
    register_builtin_manifest(
        BuiltinManifest::new("claude", "claude").with_settings(vec![
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
        ]),
    );
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
        assert!(slot.key.contains("<input>half-typed prompt</input>"));
    }

    #[test]
    fn commit_edit_rejects_non_empty_old() {
        let mut p = ClaudeProvider::new();
        assert!(!p.commit_edit("a past line", "new text"));
    }

    #[test]
    fn commit_edit_rejects_blank_prompt() {
        let mut p = ClaudeProvider::new();
        assert!(!p.commit_edit("", "   "));
    }

    #[test]
    fn spawn_failure_surfaces_error() {
        let mut p = ClaudeProvider::new();
        p.program = "definitely-not-claude-xyz-9000".to_owned();
        p.init();
        let err = p.take_error().expect("spawn failure should set an error");
        assert!(err.contains("could not start"), "got: {err}");
        // A second `init` must not retry (init_attempted guard).
        p.init();
        assert!(p.take_error().is_none());
    }

    #[test]
    fn commit_edit_with_no_session_fails_gracefully() {
        let mut p = ClaudeProvider::new();
        p.program = "definitely-not-claude-xyz-9000".to_owned();
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
        // Empty / unknown keys are ignored.
        p.on_setting_change("claudeBinary", "");
        assert_eq!(p.program, "/opt/claude");
        p.on_setting_change("unrelated", "x");
    }

    #[test]
    fn fetch_without_session_still_returns_input_slot() {
        let mut p = ClaudeProvider::new();
        p.program = "definitely-not-claude-xyz-9000".to_owned();
        let out = p.fetch();
        assert!(out.last().unwrap().as_obj().unwrap().key.contains("<input>"));
    }

    #[test]
    fn register_makes_factory_available() {
        register();
        let p = sicompass_sdk::create_provider_by_name("claude");
        assert!(p.is_some());
        assert_eq!(p.unwrap().name(), "claude");
    }
}
