//! Sicompass terminal provider.
//!
//! Renders a scrollback list (one entry per submitted command, with its output
//! as children) plus a trailing `<input/>` slot for the next command. The
//! actual shell process lives in the internal `sicompass-shell` crate.

use std::path::PathBuf;

use sicompass_sdk::{
    register_builtin_manifest, register_provider_factory, BuiltinManifest, DashboardCell,
    DashboardFrame, DashboardKey, DashboardKeysym, DashboardKind, FfonElement, FfonObject,
    Provider, SettingDecl,
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

    // ---- Phase 2a interactive-dashboard demo state ------------------------
    // These exist only to prove out the dashboard plumbing. Phase 2b replaces
    // them with a vte::Parser-backed terminal emulator that drives the real
    // shell over the existing PTY.
    db_keys_received: u32,
    db_last_keysym: String,
    db_text_buffer: String,
}

impl TerminalProvider {
    pub fn new() -> Self {
        TerminalProvider {
            shell: None,
            entries: Vec::new(),
            shell_program: default_program(),
            cwd: None,
            init_attempted: false,
            db_keys_received: 0,
            db_last_keysym: String::new(),
            db_text_buffer: String::new(),
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

    // ---- Phase 2a interactive-dashboard demo ---------------------------

    fn dashboard_kind(&self) -> DashboardKind {
        DashboardKind::Interactive
    }

    fn enter_dashboard(&mut self) {
        self.db_keys_received = 0;
        self.db_last_keysym.clear();
        self.db_text_buffer.clear();
    }

    fn dashboard_key(&mut self, key: DashboardKey) -> bool {
        self.db_keys_received = self.db_keys_received.saturating_add(1);
        self.db_last_keysym = format_keysym(&key);
        true
    }

    fn dashboard_text(&mut self, text: &str) {
        for ch in text.chars() {
            if self.db_text_buffer.chars().count() >= 60 {
                self.db_text_buffer.clear();
            }
            self.db_text_buffer.push(ch);
        }
    }

    fn dashboard_resize(&mut self, _rows: u16, _cols: u16) {
        // Phase 2a: nothing to do — the demo regenerates the frame to fit
        // whatever (cols, rows) `dashboard_render` is called with. Phase 2b
        // will forward this to `Shell::resize` and the cell-grid emulator.
    }

    fn dashboard_render(&mut self, cols: u16, rows: u16) -> DashboardFrame {
        let mut frame = DashboardFrame::empty(cols, rows);

        // Checkerboard background so the cell layout is visible.
        let bg_a: u32 = 0x101820FF;
        let bg_b: u32 = 0x182030FF;
        let fg_text: u32 = 0xE0E0E0FF;
        let fg_accent: u32 = 0x80C0FFFF;
        for r in 0..rows {
            for c in 0..cols {
                let bg = if (c + r) % 2 == 0 { bg_a } else { bg_b };
                let cell = frame.cell_mut(c, r);
                cell.bg = bg;
                cell.fg = fg_text;
            }
        }

        // Title and stats — clipped automatically by `write_str`.
        let line1 = "Sicompass Terminal — Phase 2a interactive-dashboard demo";
        frame.write_str(0, 0, line1, fg_accent);

        let line2 = format!("Grid: {} cols × {} rows", cols, rows);
        frame.write_str(0, 1, &line2, fg_text);

        let line3 = format!(
            "Keys received: {}    last: {}",
            self.db_keys_received,
            if self.db_last_keysym.is_empty() { "—" } else { self.db_last_keysym.as_str() },
        );
        frame.write_str(0, 2, &line3, fg_text);

        let prompt = "Type: ";
        frame.write_str(0, 4, prompt, fg_accent);
        let typed_col = prompt.chars().count() as u16;
        frame.write_str(typed_col, 4, &self.db_text_buffer, fg_text);

        frame.write_str(0, rows.saturating_sub(1), "Esc to exit", fg_accent);

        // Cursor sits one cell after the typed buffer.
        let cursor_col = typed_col + (self.db_text_buffer.chars().count() as u16);
        frame.cursor = Some((cursor_col.min(cols.saturating_sub(1)), 4));

        frame
    }
}

fn format_keysym(key: &DashboardKey) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if key.ctrl  { parts.push("Ctrl"); }
    if key.alt   { parts.push("Alt"); }
    if key.shift { parts.push("Shift"); }
    let name = match key.keysym {
        DashboardKeysym::Enter => "Enter".to_owned(),
        DashboardKeysym::Backspace => "Backspace".to_owned(),
        DashboardKeysym::Tab => "Tab".to_owned(),
        DashboardKeysym::Up => "Up".to_owned(),
        DashboardKeysym::Down => "Down".to_owned(),
        DashboardKeysym::Left => "Left".to_owned(),
        DashboardKeysym::Right => "Right".to_owned(),
        DashboardKeysym::Home => "Home".to_owned(),
        DashboardKeysym::End => "End".to_owned(),
        DashboardKeysym::PageUp => "PageUp".to_owned(),
        DashboardKeysym::PageDown => "PageDown".to_owned(),
        DashboardKeysym::Insert => "Insert".to_owned(),
        DashboardKeysym::Delete => "Delete".to_owned(),
        DashboardKeysym::F(n) => format!("F{n}"),
        DashboardKeysym::Char(c) => format!("'{c}'"),
        DashboardKeysym::Unknown => "?".to_owned(),
    };
    parts.push(&name);
    parts.join("+")
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

    // ---- Phase 2a interactive-dashboard demo tests ---------------------

    #[test]
    fn dashboard_kind_is_interactive() {
        let p = TerminalProvider::new();
        assert_eq!(p.dashboard_kind(), DashboardKind::Interactive);
    }

    #[test]
    fn enter_dashboard_resets_demo_state() {
        let mut p = TerminalProvider::new();
        p.db_keys_received = 7;
        p.db_last_keysym = "X".to_owned();
        p.db_text_buffer = "hello".to_owned();
        p.enter_dashboard();
        assert_eq!(p.db_keys_received, 0);
        assert!(p.db_last_keysym.is_empty());
        assert!(p.db_text_buffer.is_empty());
    }

    #[test]
    fn dashboard_key_increments_and_returns_redraw() {
        let mut p = TerminalProvider::new();
        let k = DashboardKey {
            keysym: DashboardKeysym::Up,
            ctrl: false, shift: false, alt: false,
        };
        assert!(p.dashboard_key(k));
        assert_eq!(p.db_keys_received, 1);
        assert_eq!(p.db_last_keysym, "Up");
    }

    #[test]
    fn dashboard_key_formats_modifiers() {
        let mut p = TerminalProvider::new();
        let k = DashboardKey {
            keysym: DashboardKeysym::Char('c'),
            ctrl: true, shift: false, alt: false,
        };
        p.dashboard_key(k);
        assert_eq!(p.db_last_keysym, "Ctrl+'c'");
    }

    #[test]
    fn dashboard_text_appends_chars() {
        let mut p = TerminalProvider::new();
        p.dashboard_text("hi");
        p.dashboard_text(" there");
        assert_eq!(p.db_text_buffer, "hi there");
    }

    #[test]
    fn dashboard_text_wraps_at_60_chars() {
        let mut p = TerminalProvider::new();
        p.dashboard_text(&"x".repeat(60));
        assert_eq!(p.db_text_buffer.chars().count(), 60);
        p.dashboard_text("Y");
        assert_eq!(p.db_text_buffer, "Y");
    }

    #[test]
    fn dashboard_render_has_correct_dimensions_and_cursor() {
        let mut p = TerminalProvider::new();
        let frame = p.dashboard_render(80, 24);
        assert_eq!(frame.cols, 80);
        assert_eq!(frame.rows, 24);
        assert_eq!(frame.cells.len(), 80 * 24);
        assert!(frame.cursor.is_some());
        // Title is drawn on the first row.
        assert_eq!(frame.cell(0, 0).ch, 'S');
    }

    #[test]
    fn dashboard_render_paints_checkerboard_background() {
        let mut p = TerminalProvider::new();
        let frame = p.dashboard_render(4, 2);
        // Adjacent cells alternate background colors.
        let bg_00 = frame.cell(0, 0).bg;
        let bg_10 = frame.cell(1, 0).bg;
        let bg_01 = frame.cell(0, 1).bg;
        assert_ne!(bg_00, bg_10);
        assert_ne!(bg_00, bg_01);
        assert_eq!(bg_10, bg_01);
    }

    #[test]
    fn dashboard_render_handles_tiny_grid_without_panicking() {
        let mut p = TerminalProvider::new();
        let frame = p.dashboard_render(1, 1);
        assert_eq!(frame.cells.len(), 1);
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
