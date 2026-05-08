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
    DashboardKey, DashboardKind, FfonElement, Provider, SettingDecl,
};
use sicompass_shell::{default_program, Shell, ShellConfig};

const INPUT_PLACEHOLDER: &str = "<input></input>";

/// One entry in the terminal scrollback: a submitted command and the bytes
/// the shell has produced in response so far.
#[derive(Debug, Clone)]
struct Entry {
    /// The synthesized prompt that was active when this command was submitted
    /// (e.g. `"nico@verysilly:~$ "`). Captured at submit time so the rendered
    /// scrollback shows where each command was actually run, not where the
    /// user is now.
    prompt: String,
    input: String,
    output: String,
}

pub struct TerminalProvider {
    shell: Option<Shell>,
    entries: Vec<Entry>,
    shell_program: String,
    cwd: Option<PathBuf>,
    init_attempted: bool,
    /// User name for the synthesized prompt — captured once at construction
    /// from `$USER`, falling back to `"user"`.
    user: String,
    /// Hostname for the synthesized prompt — captured once at construction,
    /// truncated at the first `.`.
    host: String,

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
            user: std::env::var("USER").unwrap_or_else(|_| "user".to_owned()),
            host: hostname_short(),
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
                prompt: String::new(),
                input: format!("(failed to start `{}`)", self.shell_program),
                output: e.to_string(),
            }),
        }
    }

    /// Build the prompt string from `user`, `host`, and the shell's *live*
    /// working directory. We don't try to mimic PS1 escapes from bash because
    /// those have proven unreliable across shells, locales, and rc-file edge
    /// cases (literal backslashes, missing prompt on first entry, ...).
    /// Synthesizing in-process gives us a stable, predictable prompt that
    /// updates after `cd` (via `/proc/<pid>/cwd`).
    fn current_prompt(&self) -> String {
        let cwd = self.shell_cwd();
        let display_cwd = collapse_home(&cwd);
        format!("{}@{}:{}$ ", self.user, self.host, display_cwd)
    }

    /// Read the shell child's current working directory. Linux: `readlink
    /// /proc/<pid>/cwd`. Other platforms or read failures fall back to the
    /// initial cwd (or `~`).
    fn shell_cwd(&self) -> String {
        #[cfg(target_os = "linux")]
        {
            if let Some(pid) = self.shell.as_ref().and_then(|s| s.pid()) {
                let link = format!("/proc/{}/cwd", pid);
                if let Ok(p) = std::fs::read_link(&link) {
                    return p.to_string_lossy().into_owned();
                }
            }
        }
        self.cwd
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .or_else(|| std::env::var("HOME").ok())
            .unwrap_or_else(|| "/".to_owned())
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
        // Flat layout: one Str per prompt-line (`{prompt}{cmd}`) and one Str
        // per output-line, in chronological order. The trailing element is the
        // `<input></input>` slot, prefixed with the *live* prompt (recomputed
        // every fetch so it reflects the shell's current cwd after `cd`).
        let live_prompt = self.current_prompt();
        let mut out: Vec<FfonElement> = Vec::new();
        for e in &self.entries {
            out.push(FfonElement::Str(format!("{}{}", e.prompt, e.input)));
            for line in entry_lines(&e.input, &e.output) {
                out.push(FfonElement::Str(line.to_owned()));
            }
        }
        out.push(FfonElement::Str(format!("{}{}", live_prompt, INPUT_PLACEHOLDER)));
        out
    }

    fn commit_edit(&mut self, old: &str, new: &str) -> bool {
        // The handler extracts the inner value of `<input>...</input>` before
        // calling commit_edit, so for the trailing input slot we receive
        // `old == ""`. Reject any non-empty `old` (e.g. editing a past entry).
        if !old.is_empty() {
            return false;
        }
        self.ensure_shell();
        let Some(shell) = self.shell.as_mut() else {
            return false;
        };
        if shell.write_line(new).is_err() {
            return false;
        }
        // Snapshot the prompt at submit time — past entries should display
        // the cwd they were run in, even after the user runs `cd` afterwards.
        let prompt = self.current_prompt();
        self.entries.push(Entry {
            prompt,
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
        // Drop pre-command output (shell startup banner, the shell's own PS1).
        // We synthesize our own prompt from `{user}@{host}:{cwd}$ ` instead of
        // capturing what the shell emits, so any PS1 bytes that arrive here
        // are noise and can be discarded.
        if let Some(last) = self.entries.last_mut() {
            last.output.push_str(&text);
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

/// Render an entry's output as a list of child lines, stripping the noise
/// produced by the PTY around the actual command output:
///
/// * The PTY echoes the typed command back as the first line — drop it if it
///   matches `input`.
/// * Bash emits the next `$PS1` immediately after the command — that lands as
///   a trailing line with no terminating `\n` (bash sits at the prompt waiting
///   for input). Drop the final element of `split('\n')` to remove it; if the
///   output ends with `\n` instead, that final element is `""` and dropping
///   it is also correct.
fn entry_lines<'a>(input: &str, output: &'a str) -> Vec<&'a str> {
    let mut lines: Vec<&str> = output.split('\n').collect();
    // Drop the trailing prompt (or trailing empty string from the final \n).
    lines.pop();
    if lines.first() == Some(&input) {
        lines.remove(0);
    }
    lines
}

/// Read the system hostname, truncate at the first `.` (so `host.example.com`
/// renders as `host`), and fall back to `"host"` on failure.
fn hostname_short() -> String {
    let raw = std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| if o.status.success() { String::from_utf8(o.stdout).ok() } else { None })
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| std::env::var("HOSTNAME").unwrap_or_else(|_| "host".to_owned()));
    raw.split('.').next().unwrap_or("host").to_owned()
}

/// Replace a leading `$HOME` in `cwd` with `~` for shell-style display.
fn collapse_home(cwd: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            if cwd == home { return "~".to_owned(); }
            if let Some(rest) = cwd.strip_prefix(&format!("{}/", home)) {
                return format!("~/{}", rest);
            }
        }
    }
    cwd.to_owned()
}

/// Decode raw PTY bytes into displayable UTF-8, stripping terminal control
/// sequences. Best-effort — full ANSI handling lives in the interactive
/// dashboard's `Emulator`; this path is only the linear scrollback view.
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
                // String-introducer escapes — OSC, DCS, PM, APC. All run until
                // ST (`ESC \`) or BEL.
                Some(']') | Some('P') | Some('^') | Some('_') => {
                    while let Some(nc) = chars.next() {
                        if nc == '\x07' { break; }
                        if nc == '\x1b' { let _ = chars.next(); break; }
                    }
                }
                // nF escape sequences (charset designators like `ESC ( B`,
                // `ESC ) 0`, etc.) — intermediate byte in 0x20..=0x2F followed
                // by a final byte. Without this branch the final byte (`B`,
                // `0`, …) leaks into the rendered output.
                Some(c) if matches!(c as u32, 0x20..=0x2F) => {
                    let _ = chars.next();
                }
                _ => { /* short ESC sequence (e.g. ESC =, ESC 7) */ }
            },
            '\r' | '\x07' => {} // strip bare CR + BEL
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' => {
                // drop other C0 controls
            }
            c if matches!(c as u32, 0x7F | 0x80..=0x9F) => {
                // drop DEL + C1 control range (these sneak in via UTF-8 of
                // 0xC2 0x80..0x9F or as raw bytes when bash emits 8-bit
                // control codes).
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
        // Synthesized prompt is appended; the trailing tag must still be present.
        assert!(elems[0].as_str().unwrap().ends_with(INPUT_PLACEHOLDER));
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
    fn commit_edit_rejects_non_empty_old() {
        let mut p = TerminalProvider::new();
        assert!(!p.commit_edit("previous command", "ls"));
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

    #[test]
    fn entry_lines_strips_echoed_input_and_trailing_prompt() {
        // Typical bash output for `ls`: echo, two output lines, next prompt.
        let lines = entry_lines("ls", "ls\nfile1\nfile2\nuser@host ~ $ ");
        assert_eq!(lines, vec!["file1", "file2"]);
    }

    #[test]
    fn entry_lines_strips_trailing_empty_string_when_output_ends_with_newline() {
        // No prompt yet (still streaming), but output ends with \n.
        let lines = entry_lines("ls", "ls\nfile1\n");
        assert_eq!(lines, vec!["file1"]);
    }

    #[test]
    fn entry_lines_empty_output_yields_no_children() {
        assert!(entry_lines("ls", "").is_empty());
    }

    #[test]
    fn entry_lines_keeps_first_line_when_it_does_not_match_input() {
        // Edge case: shell rewrote the input (e.g. alias expansion). Don't strip.
        let lines = entry_lines("ll", "ls -la\nfile1\nuser@host ~ $ ");
        assert_eq!(lines, vec!["ls -la", "file1"]);
    }

    #[test]
    fn fetch_uses_synthesized_prompt_for_input_slot() {
        // No shell spawned, no entries — the trailing element should still be
        // a synthesized "{user}@{host}:{cwd}$ <input></input>" line.
        let mut p = TerminalProvider::new();
        let elems = p.fetch();
        assert_eq!(elems.len(), 1);
        let s = elems[0].as_str().unwrap();
        assert!(s.ends_with("$ <input></input>"), "expected prompt suffix; got {:?}", s);
        assert!(s.contains('@'), "expected user@host segment; got {:?}", s);
    }

    #[test]
    fn fetch_uses_entry_prompt_for_past_commands() {
        let mut p = TerminalProvider::new();
        p.entries.push(Entry {
            prompt: "user@host:/tmp$ ".to_owned(),
            input: "ls".to_owned(),
            output: "ls\nfile1\nfile2\n".to_owned(),
        });
        let elems = p.fetch();
        // Past prompt + cmd, two output lines, then synthesized live prompt.
        assert_eq!(elems.len(), 4);
        assert_eq!(elems[0].as_str(), Some("user@host:/tmp$ ls"));
        assert_eq!(elems[1].as_str(), Some("file1"));
        assert_eq!(elems[2].as_str(), Some("file2"));
        assert!(elems[3].as_str().unwrap().ends_with("$ <input></input>"));
    }

    #[test]
    fn collapse_home_replaces_leading_home_with_tilde() {
        std::env::set_var("HOME", "/home/nico");
        assert_eq!(collapse_home("/home/nico"), "~");
        assert_eq!(collapse_home("/home/nico/projects"), "~/projects");
        assert_eq!(collapse_home("/home/nicolae"), "/home/nicolae"); // not a prefix match
        assert_eq!(collapse_home("/tmp"), "/tmp");
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

        assert!(p.commit_edit("", "echo terminal-it-test"));
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
        // Flat layout: "{PS1}echo terminal-it-test" + N output lines + a final
        // "{PS1}<input></input>" slot. We don't pin the exact PS1 (depends on
        // /bin/sh + the user's rc files), but the first element must end with
        // the submitted command, the last must end with the input placeholder,
        // and somewhere in between the echoed marker must appear.
        assert!(elems.len() >= 2);
        assert!(elems[0].as_str().map_or(false, |s| s.ends_with("echo terminal-it-test")),
            "first element should end with command; got {:?}", elems[0].as_str());
        assert!(elems.last().and_then(|e| e.as_str()).map_or(false, |s| s.ends_with(INPUT_PLACEHOLDER)),
            "last element should end with input slot; got {:?}", elems.last().and_then(|e| e.as_str()));
        assert!(elems.iter().any(|e| e.as_str().map_or(false, |s| s.contains("terminal-it-test"))));
    }
}
