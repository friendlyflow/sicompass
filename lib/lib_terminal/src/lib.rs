//! Sicompass terminal provider.
//!
//! Two views over the same underlying PTY-backed shell:
//!
//! * **Scrollback list** (default). `fetch()` exposes one Obj per submitted
//!   command with its output as children, plus a trailing `<input/>` slot.
//!   `commit_edit()` writes a line to the shell, `tick()` drains output into
//!   the latest entry. Suitable for "type a command, see output" workflows.
//!
//! * **Interactive dashboard** (Phase 2b). When the user presses `d` the app
//!   switches to `Coordinate::DashboardInteractive` and routes raw keys +
//!   text input + resize events to this provider. We feed PTY bytes through
//!   a [`vte::Parser`]-backed [`emulator::Emulator`] and snapshot the cell
//!   grid back into a `DashboardFrame` every frame. This is the path that
//!   makes `vim`, `less`, `htop` etc. usable.
//!
//! The actual shell process lives in the internal `sicompass-shell` crate.

mod emulator;

use std::path::PathBuf;

use emulator::{encode_dashboard_key, Emulator};
use sicompass_sdk::{
    register_builtin_manifest, register_provider_factory, BuiltinManifest, DashboardFrame,
    DashboardKey, DashboardKind, FfonElement, FfonObject, Provider, SettingDecl,
};
use sicompass_shell::{default_program, Shell, ShellConfig};

const INPUT_PLACEHOLDER: &str = "<input></input>";

/// One entry in the terminal scrollback: a submitted command and the bytes
/// the shell has produced in response so far.
#[derive(Debug, Clone)]
struct Entry {
    input: String,
    output: String,
}

pub struct TerminalProvider {
    shell: Option<Shell>,
    entries: Vec<Entry>,
    shell_program: String,
    cwd: Option<PathBuf>,
    init_attempted: bool,

    /// Lazily created on first `enter_dashboard()`. Lives across enter/leave
    /// so a long-running interactive session (e.g. `vim`) survives toggling
    /// out of the dashboard and back.
    emulator: Option<Emulator>,
    /// While `true`, `tick()` routes shell output into `emulator`; otherwise
    /// it appends to the scrollback as before.
    in_dashboard: bool,
}

impl TerminalProvider {
    pub fn new() -> Self {
        TerminalProvider {
            shell: None,
            entries: Vec::new(),
            shell_program: default_program(),
            cwd: None,
            init_attempted: false,
            emulator: None,
            in_dashboard: false,
        }
    }

    fn ensure_shell(&mut self) {
        if self.shell.is_some() || self.init_attempted {
            return;
        }
        self.init_attempted = true;
        let cfg = ShellConfig {
            program: self.shell_program.clone(),
            cwd: self.cwd.clone(),
            ..ShellConfig::default()
        };
        match Shell::spawn(cfg) {
            Ok(s) => self.shell = Some(s),
            Err(e) => self.entries.push(Entry {
                input: format!("(failed to start `{}`)", self.shell_program),
                output: e.to_string(),
            }),
        }
    }
}

impl Default for TerminalProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for TerminalProvider {
    fn name(&self) -> &str {
        "terminal"
    }

    fn display_name(&self) -> &str {
        "terminal"
    }

    fn init(&mut self) {
        self.ensure_shell();
    }

    fn cleanup(&mut self) {
        self.shell = None;
        self.init_attempted = false;
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        let mut out: Vec<FfonElement> = Vec::with_capacity(self.entries.len() + 1);
        for e in &self.entries {
            let mut obj = FfonObject::new(format!("$ {}", e.input));
            for line in e.output.split('\n') {
                obj.children.push(FfonElement::Str(line.to_owned()));
            }
            out.push(FfonElement::Obj(obj));
        }
        out.push(FfonElement::Str(INPUT_PLACEHOLDER.to_owned()));
        out
    }

    fn commit_edit(&mut self, old: &str, new: &str) -> bool {
        if old != INPUT_PLACEHOLDER {
            return false;
        }
        self.ensure_shell();
        let Some(shell) = self.shell.as_mut() else {
            return false;
        };
        if shell.write_line(new).is_err() {
            return false;
        }
        self.entries.push(Entry {
            input: new.to_owned(),
            output: String::new(),
        });
        true
    }

    fn tick(&mut self) -> bool {
        let Some(shell) = self.shell.as_mut() else {
            return false;
        };
        let bytes = shell.drain_output();
        if bytes.is_empty() {
            return false;
        }
        if self.in_dashboard {
            // Route raw bytes through the ANSI/VT emulator. The next
            // `dashboard_render` call will snapshot the updated grid.
            if let Some(em) = self.emulator.as_mut() {
                em.feed(&bytes);
            }
            return true;
        }
        let text = decode_terminal_output(&bytes);
        if text.is_empty() {
            return false;
        }
        if let Some(last) = self.entries.last_mut() {
            last.output.push_str(&text);
        } else {
            // Output arrived before any command (shell prompt / banner). Hide it
            // in a synthetic header entry so `fetch()` always exposes it.
            self.entries.push(Entry {
                input: "(shell)".to_owned(),
                output: text,
            });
        }
        true
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        if key == "shellProgram" && !value.is_empty() {
            self.shell_program = value.to_owned();
        }
    }

    fn refresh_on_navigate(&self) -> bool {
        false
    }

    // ---- Interactive dashboard (Phase 2b) -------------------------------

    fn dashboard_kind(&self) -> DashboardKind {
        DashboardKind::Interactive
    }

    fn enter_dashboard(&mut self) {
        self.ensure_shell();
        self.in_dashboard = true;
        if self.emulator.is_none() {
            // Spawn with a placeholder size. `dashboard_resize` fires on the
            // first frame and updates the emulator + PTY to the real grid.
            self.emulator = Some(Emulator::new(80, 24));
        }
    }

    fn leave_dashboard(&mut self) {
        self.in_dashboard = false;
    }

    fn dashboard_resize(&mut self, rows: u16, cols: u16) {
        if let Some(shell) = self.shell.as_mut() {
            let _ = shell.resize(rows, cols);
        }
        if let Some(em) = self.emulator.as_mut() {
            em.resize(cols, rows);
        }
    }

    fn dashboard_key(&mut self, key: DashboardKey) -> bool {
        if let Some(bytes) = encode_dashboard_key(&key) {
            if let Some(shell) = self.shell.as_mut() {
                let _ = shell.write_input(&bytes);
            }
        }
        // Always request redraw — the shell may produce output before the
        // next tick and we want the cursor blink to keep up.
        true
    }

    fn dashboard_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(shell) = self.shell.as_mut() {
            let _ = shell.write_input(text.as_bytes());
        }
    }

    fn dashboard_render(&mut self, cols: u16, rows: u16) -> DashboardFrame {
        // Pull any bytes the shell has produced since the last `tick()`.
        // Normally the main loop's `tick()` runs first, but draining here
        // means a frame triggered by user input shows the response without
        // waiting one extra frame.
        if let Some(shell) = self.shell.as_mut() {
            let bytes = shell.drain_output();
            if !bytes.is_empty() {
                if let Some(em) = self.emulator.as_mut() {
                    em.feed(&bytes);
                }
            }
        }
        match self.emulator.as_ref() {
            Some(em) => em.snapshot(),
            None => DashboardFrame::empty(cols, rows),
        }
    }
}

/// Decode raw PTY bytes into displayable UTF-8, stripping the most common
/// terminal control sequences. Phase-1 best-effort: full ANSI handling is
/// deferred to the interactive-dashboard follow-up.
fn decode_terminal_output(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => {
                    // CSI: parameters then a final byte in 0x40..=0x7E.
                    while let Some(nc) = chars.next() {
                        let n = nc as u32;
                        if (0x40..=0x7E).contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC: terminated by BEL (0x07) or ESC \ .
                    while let Some(nc) = chars.next() {
                        if nc == '\x07' {
                            break;
                        }
                        if nc == '\x1b' {
                            let _ = chars.next();
                            break;
                        }
                    }
                }
                _ => { /* drop the 2-byte ESC sequence */ }
            },
            '\r' | '\x07' => {} // strip bare CR + BEL
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' => {
                // drop other C0 control characters
            }
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// SDK registration
// ---------------------------------------------------------------------------

/// Register the terminal with the SDK factory and manifest registries.
pub fn register() {
    register_provider_factory("terminal", || Box::new(TerminalProvider::new()));
    register_builtin_manifest(
        BuiltinManifest::new("terminal", "terminal").with_settings(vec![SettingDecl::text(
            "terminal",
            "shell program",
            "shellProgram",
            &default_program(),
        )]),
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_empty_returns_input_placeholder() {
        let mut p = TerminalProvider::new();
        let elems = p.fetch();
        assert_eq!(elems.len(), 1);
        assert_eq!(elems[0].as_str(), Some(INPUT_PLACEHOLDER));
    }

    #[test]
    fn name_and_display_name() {
        let p = TerminalProvider::new();
        assert_eq!(p.name(), "terminal");
        assert_eq!(p.display_name(), "terminal");
    }

    #[test]
    fn refresh_on_navigate_is_false() {
        let p = TerminalProvider::new();
        assert!(!p.refresh_on_navigate());
    }

    #[test]
    fn on_setting_change_updates_shell_program() {
        let mut p = TerminalProvider::new();
        p.on_setting_change("shellProgram", "/bin/dash");
        assert_eq!(p.shell_program, "/bin/dash");
    }

    #[test]
    fn on_setting_change_ignores_empty_and_other_keys() {
        let mut p = TerminalProvider::new();
        let original = p.shell_program.clone();
        p.on_setting_change("shellProgram", "");
        p.on_setting_change("unrelated", "/bin/zsh");
        assert_eq!(p.shell_program, original);
    }

    #[test]
    fn commit_edit_rejects_non_placeholder_old() {
        let mut p = TerminalProvider::new();
        assert!(!p.commit_edit("not the placeholder", "ls"));
        assert!(p.entries.is_empty());
    }

    #[test]
    fn decode_strips_csi_and_cr_keeps_text() {
        let s = decode_terminal_output(b"\x1b[31mhello\x1b[0m\r\nworld");
        assert_eq!(s, "hello\nworld");
    }

    #[test]
    fn decode_strips_osc_with_bel_terminator() {
        let s = decode_terminal_output(b"\x1b]0;title\x07ok");
        assert_eq!(s, "ok");
    }

    #[test]
    fn decode_strips_osc_with_st_terminator() {
        let s = decode_terminal_output(b"\x1b]0;title\x1b\\ok");
        assert_eq!(s, "ok");
    }

    #[test]
    fn decode_keeps_newline_and_tab() {
        let s = decode_terminal_output(b"a\tb\nc");
        assert_eq!(s, "a\tb\nc");
    }

    // ---- Phase 2b interactive-dashboard tests --------------------------

    #[test]
    fn dashboard_kind_is_interactive() {
        let p = TerminalProvider::new();
        assert_eq!(p.dashboard_kind(), DashboardKind::Interactive);
    }

    #[test]
    fn dashboard_render_without_shell_returns_blank_frame() {
        // No shell, no emulator → we should still get a well-shaped frame
        // (so the renderer can paint *something* even if spawn failed).
        let mut p = TerminalProvider::new();
        let frame = p.dashboard_render(20, 5);
        assert_eq!(frame.cols, 20);
        assert_eq!(frame.rows, 5);
        assert_eq!(frame.cells.len(), 100);
    }

    #[test]
    fn leave_dashboard_clears_in_dashboard_flag() {
        let mut p = TerminalProvider::new();
        p.in_dashboard = true;
        p.leave_dashboard();
        assert!(!p.in_dashboard);
    }

    #[cfg(unix)]
    #[test]
    fn end_to_end_dashboard_renders_shell_prompt() {
        use std::thread;
        use std::time::{Duration, Instant};

        let mut p = TerminalProvider::new();
        p.on_setting_change("shellProgram", "/bin/sh");
        p.enter_dashboard();
        // Skip if spawn failed (e.g. CI sandbox).
        if p.shell.is_none() {
            return;
        }
        // Fire the resize so the PTY knows about an 80×24 grid (matches what
        // view.rs does on the first frame).
        p.dashboard_resize(24, 80);
        // Send a command and wait for output to land in the emulator grid.
        p.dashboard_text("echo dashboard-it-marker\n");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut saw = false;
        while Instant::now() < deadline {
            p.tick();
            let frame = p.dashboard_render(80, 24);
            // Scan all cells for the marker as a contiguous string.
            let row_text: Vec<String> = (0..frame.rows).map(|r| {
                (0..frame.cols).map(|c| frame.cell(c, r).ch).collect::<String>()
            }).collect();
            if row_text.iter().any(|line| line.contains("dashboard-it-marker")) {
                saw = true;
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(saw, "expected marker in emulator grid; rows: {:#?}",
            (0..p.emulator.as_ref().unwrap().rows()).map(|r| {
                let f = p.emulator.as_ref().unwrap().snapshot();
                (0..f.cols).map(|c| f.cell(c, r).ch).collect::<String>()
            }).collect::<Vec<_>>(),
        );
    }

    #[cfg(unix)]
    #[test]
    fn end_to_end_commit_edit_runs_shell_command() {
        use std::thread;
        use std::time::{Duration, Instant};

        let mut p = TerminalProvider::new();
        p.on_setting_change("shellProgram", "/bin/sh");
        p.init();

        // Skip if spawn failed (e.g. CI sandbox).
        if p.shell.is_none() {
            return;
        }

        assert!(p.commit_edit(INPUT_PLACEHOLDER, "echo terminal-it-test"));
        assert_eq!(p.entries.len(), 1);

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut saw_output = false;
        while Instant::now() < deadline {
            if p.tick() {
                if p.entries.last().unwrap().output.contains("terminal-it-test") {
                    saw_output = true;
                    break;
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            saw_output,
            "expected echoed marker in last entry output; got: {:?}",
            p.entries.last().unwrap().output
        );

        let elems = p.fetch();
        // entries.len() (1) + trailing input slot (1) = 2 elements
        assert_eq!(elems.len(), 2);
        assert!(elems[0].is_obj());
        assert_eq!(elems[1].as_str(), Some(INPUT_PLACEHOLDER));
    }
}
