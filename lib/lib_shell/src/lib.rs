//! PTY-backed interactive shell session.
//!
//! This crate is internal: `sicompass-terminal` uses it to spawn a real shell
//! attached to a pseudo-terminal, push input, and drain output. It has no
//! sicompass-sdk dependency and is not registered as a provider.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

/// Configuration for spawning a [`Shell`].
#[derive(Debug, Clone)]
pub struct ShellConfig {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Extra/overriding env vars. Always layered on top of the parent process
    /// environment, so callers don't need to repopulate `PATH`/`HOME`/etc.
    pub env: Vec<(String, String)>,
    pub rows: u16,
    pub cols: u16,
    /// Optional process title so the spawned shell is identifiable in process
    /// monitors (btop, `ps`, Task Manager). The shell is launched through a
    /// temp link renamed to `<title>` (symlink on Unix, hard link / copy on
    /// Windows), which makes the process's actual *name* — `comm` on Unix, the
    /// image name on Windows — equal to `<title>`. This is shell-agnostic, so
    /// it works for fish/dash too (no `exec -a` needed). Falls back to a normal
    /// spawn if the link can't be created. Note: Linux truncates `comm` to 15
    /// bytes, so keep titles short; on Windows this works for `cmd.exe` but a
    /// relocated PowerShell may fail to find its modules.
    pub title: Option<String>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        ShellConfig {
            program: default_program(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            rows: 24,
            cols: 80,
            title: None,
        }
    }
}

/// A spawned, PTY-backed shell process.
///
/// `drain_output()` is non-blocking and returns whatever bytes the background
/// reader thread has buffered since the previous call.
pub struct Shell {
    master: Box<dyn MasterPty + Send>,
    /// Wrapped in `Arc<Mutex<…>>` so the background reader thread can also
    /// write — required to auto-respond to DSR cursor-position queries
    /// (`ESC[6n`) on Windows, where cmd.exe under ConPTY blocks until the
    /// host replies.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    output: Arc<Mutex<Vec<u8>>>,
    /// Temp dir holding the renamed launch link (see `ShellConfig::title`).
    /// Removed on drop. `None` when no title was applied.
    title_link_dir: Option<PathBuf>,
}

impl Shell {
    /// Spawn `cfg.program` attached to a fresh PTY.
    pub fn spawn(cfg: ShellConfig) -> std::io::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: cfg.rows,
                cols: cfg.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io_err)?;

        // Optionally launch the shell through a temp link named after
        // `cfg.title` so it shows up under that name in process monitors (btop,
        // `ps`, Task Manager). This changes the process's actual *name* (`comm`
        // on Unix, image name on Windows) — not just its command line — and is
        // shell-agnostic, so it works for shells without `exec -a` (fish/dash)
        // too. If the link can't be created, fall back to the real program.
        let mut title_link_dir: Option<PathBuf> = None;
        let program_for_cmd: String = match cfg.title.as_deref()
            .and_then(|t| resolve_program(&cfg.program).map(|p| (p, t)))
            .and_then(|(p, t)| make_title_link(&p, t))
        {
            Some((dir, link_path)) => {
                title_link_dir = Some(dir);
                link_path.to_string_lossy().into_owned()
            }
            None => cfg.program.clone(),
        };

        let mut cmd = CommandBuilder::new(&program_for_cmd);
        // Default to interactive mode for known shells. Without an interactive
        // flag bash/zsh skip PS1 and disable the `complete` builtin, so a
        // user's rc file typically prints "complete: command not found" and no
        // prompt shows. PowerShell uses `-NoLogo`; cmd.exe has no analog.
        // Caller-supplied args override this. The flag is derived from the
        // ORIGINAL program name (the link is renamed and wouldn't match).
        if cfg.args.is_empty() {
            if let Some(flag) = interactive_flag(&cfg.program) {
                cmd.arg(flag);
            }
        }
        for arg in &cfg.args {
            cmd.arg(arg);
        }
        if let Some(cwd) = &cfg.cwd {
            cmd.cwd(cwd);
        }
        for (k, v) in std::env::vars() {
            cmd.env(k, v);
        }
        if !cfg.env.iter().any(|(k, _)| k == "TERM") {
            cmd.env("TERM", "xterm-256color");
        }
        for (k, v) in &cfg.env {
            cmd.env(k, v);
        }

        let child = pair.slave.spawn_command(cmd).map_err(io_err)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(io_err)?;
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(pair.master.take_writer().map_err(io_err)?));

        let output = Arc::new(Mutex::new(Vec::<u8>::new()));
        {
            let output = Arc::clone(&output);
            let writer = Arc::clone(&writer);
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                // Tail of the previous read kept in case a control sequence
                // straddles a buffer boundary. `ESC[6n` is 4 bytes — keep
                // up to 3 bytes around for the next scan.
                let mut tail: Vec<u8> = Vec::with_capacity(4);
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let chunk = &buf[..n];
                            if let Ok(mut o) = output.lock() {
                                o.extend_from_slice(chunk);
                            }
                            // Scan tail+chunk for ESC[6n; respond once per
                            // occurrence with a fixed `ESC[1;1R`. Real
                            // emulators report the actual cursor position,
                            // but cmd.exe only needs *some* valid reply to
                            // unblock its prompt.
                            let mut scan: Vec<u8> = Vec::with_capacity(tail.len() + n);
                            scan.extend_from_slice(&tail);
                            scan.extend_from_slice(chunk);
                            let mut hits = 0usize;
                            let mut i = 0usize;
                            while i + 4 <= scan.len() {
                                if &scan[i..i + 4] == b"\x1b[6n" {
                                    hits += 1;
                                    i += 4;
                                } else {
                                    i += 1;
                                }
                            }
                            if hits > 0 {
                                if let Ok(mut w) = writer.lock() {
                                    for _ in 0..hits {
                                        let _ = w.write_all(b"\x1b[1;1R");
                                    }
                                    let _ = w.flush();
                                }
                            }
                            tail.clear();
                            let keep = scan.len().min(3);
                            tail.extend_from_slice(&scan[scan.len() - keep..]);
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        Ok(Shell {
            master: pair.master,
            writer,
            child,
            output,
            title_link_dir,
        })
    }

    /// Send raw bytes to the child's stdin (PTY master).
    pub fn write_input(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let mut w = self.writer.lock().expect("shell writer mutex poisoned");
        w.write_all(bytes)?;
        w.flush()
    }

    /// Send `s` followed by an Enter keystroke (`\r\n` on Windows, `\n` on
    /// Unix). On Windows, cmd.exe under ConPTY treats a bare `\n` as a
    /// continuation character, not a line submission — only `\r` triggers
    /// Enter. Unix shells in cooked mode are happy with `\n`.
    pub fn write_line(&mut self, s: &str) -> std::io::Result<()> {
        self.write_input(s.as_bytes())?;
        #[cfg(windows)]
        {
            self.write_input(b"\r\n")
        }
        #[cfg(not(windows))]
        {
            self.write_input(b"\n")
        }
    }

    /// Drain whatever output the background reader has buffered. Non-blocking.
    pub fn drain_output(&mut self) -> Vec<u8> {
        let mut guard = self.output.lock().expect("shell output mutex poisoned");
        std::mem::take(&mut *guard)
    }

    /// Whether the child process is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// PID of the child shell process, if known. Used by callers (e.g. the
    /// terminal provider) to read the shell's *current* working directory via
    /// `/proc/<pid>/cwd` so the rendered prompt can update after `cd`.
    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// Best-effort kill of the child process.
    pub fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()
    }

    /// Whether a foreground command (something other than the shell itself) is
    /// currently running on the PTY.
    ///
    /// Linux-only: reads the shell's `/proc/<pid>/stat` and compares the
    /// controlling terminal's foreground process group (`tpgid`, field 8)
    /// against the shell's own process group (`pgrp`, field 5). At the prompt
    /// the shell *is* the foreground group, so they match; while a command
    /// runs the shell moves it into its own group and `tpgid` diverges. Returns
    /// `false` on other platforms or when the stat line can't be read/parsed.
    pub fn foreground_busy(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            let Some(pid) = self.pid() else { return false; };
            let Ok(content) = std::fs::read_to_string(format!("/proc/{}/stat", pid))
            else {
                return false;
            };
            // `comm` (field 2) is parenthesized and may itself contain spaces
            // or parens, so split after the final ')'. The remaining
            // whitespace-separated fields are state, ppid, pgrp, session,
            // tty_nr, tpgid, … → pgrp at index 2, tpgid at index 5.
            let Some(rparen) = content.rfind(')') else { return false; };
            let fields: Vec<&str> = content[rparen + 1..].split_whitespace().collect();
            let pgrp = fields.get(2).and_then(|s| s.parse::<i32>().ok());
            let tpgid = fields.get(5).and_then(|s| s.parse::<i32>().ok());
            match (pgrp, tpgid) {
                (Some(pg), Some(tp)) => tp >= 0 && tp != pg,
                _ => false,
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    /// Inform the PTY (and child) of a new size.
    pub fn resize(&mut self, rows: u16, cols: u16) -> std::io::Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io_err)
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        if let Some(dir) = &self.title_link_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Default shell program for the current platform.
///
/// Honours `$SHELL` on Unix and `%ComSpec%` on Windows; falls back to
/// `/bin/sh` and `cmd.exe` respectively.
pub fn default_program() -> String {
    #[cfg(windows)]
    {
        std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_owned())
    }
    #[cfg(not(windows))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned())
    }
}

fn io_err<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
}

/// Resolve `program` to an existing absolute path so a renamed launch link can
/// target it. Absolute paths are returned as-is when they exist; bare names are
/// looked up on `PATH` (also trying `<name>.exe` on Windows). Returns `None` if
/// nothing is found.
fn resolve_program(program: &str) -> Option<PathBuf> {
    let p = Path::new(program);
    if p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let cand = dir.join(program);
        if cand.is_file() {
            return Some(cand);
        }
        #[cfg(windows)]
        {
            let cand_exe = dir.join(format!("{program}.exe"));
            if cand_exe.is_file() {
                return Some(cand_exe);
            }
        }
    }
    None
}

/// Create a temp dir containing a link named `title` that points at `program`,
/// returning `(dir, link_path)`. Launching the link makes the process appear
/// under `title` in monitors (`comm` on Unix, image name on Windows). The link
/// is a symlink on Unix and a hard link (falling back to a copy) on Windows.
/// Returns `None` if neither can be created. The kernel truncates `comm` to 15
/// bytes on Linux, so keep titles short.
fn make_title_link(program: &Path, title: &str) -> Option<(PathBuf, PathBuf)> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir()
        .join(format!("sicompass-shell-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).ok()?;

    #[cfg(windows)]
    let link_name = format!("{title}.exe");
    #[cfg(not(windows))]
    let link_name = title.to_owned();
    let link_path = dir.join(&link_name);

    #[cfg(unix)]
    let ok = std::os::unix::fs::symlink(program, &link_path).is_ok();
    #[cfg(windows)]
    let ok = {
        let linked = std::fs::hard_link(program, &link_path).is_ok()
            || std::fs::copy(program, &link_path).is_ok();
        // System executables (cmd.exe, most Windows tools) load their
        // user-facing strings from a satellite MUI file at
        // `<exe_dir>\<lang>\<exe_name>.mui`. The loader resolves that path
        // relative to the *running* image, so a renamed copy in our temp dir
        // can't find `System32\<lang>\cmd.exe.mui` and every message-table
        // lookup fails ("The system cannot find message text for message
        // number 0x…" — e.g. `dir`'s summary and `<DIR>` labels). Replicate
        // the MUI files next to the link, renamed to match, so they resolve.
        if linked {
            copy_mui_siblings(program, &dir, &link_name);
        }
        linked
    };
    #[cfg(not(any(unix, windows)))]
    let ok = false;

    if ok {
        Some((dir, link_path))
    } else {
        let _ = std::fs::remove_dir_all(&dir);
        None
    }
}

/// Replicate a program's satellite MUI resource files next to a renamed launch
/// link. For a source `…\System32\cmd.exe`, each language folder holding
/// `cmd.exe.mui` (e.g. `…\System32\en-US\cmd.exe.mui`) is mirrored to
/// `<link_dir>\en-US\<link_name>.mui` so the Windows resource loader — which
/// searches relative to the running image — can find the strings. Best-effort:
/// programs with embedded resources (no `.mui`) simply have nothing to copy.
#[cfg(windows)]
fn copy_mui_siblings(program: &Path, link_dir: &Path, link_name: &str) {
    let (Some(src_dir), Some(src_file)) = (program.parent(), program.file_name())
    else {
        return;
    };
    let Some(src_file) = src_file.to_str() else {
        return;
    };
    let mui_name = format!("{src_file}.mui");
    let Ok(entries) = std::fs::read_dir(src_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let src_mui = entry.path().join(&mui_name);
        if !src_mui.is_file() {
            continue;
        }
        let Some(lang) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let dst_lang = link_dir.join(&lang);
        if std::fs::create_dir_all(&dst_lang).is_err() {
            continue;
        }
        let dst_mui = dst_lang.join(format!("{link_name}.mui"));
        let _ = std::fs::hard_link(&src_mui, &dst_mui).is_ok()
            || std::fs::copy(&src_mui, &dst_mui).is_ok();
    }
}

/// For known interactive shells, return the flag that should be appended so
/// PS1 and programmable completion work under a PTY. Returns `None` if the
/// shell either has no such flag (cmd.exe) or is unrecognized.
fn interactive_flag(program: &str) -> Option<&'static str> {
    let basename = std::path::Path::new(program)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    let basename = basename.strip_suffix(".exe").unwrap_or(&basename);
    match basename {
        "bash" | "zsh" | "sh" | "dash" | "ksh" | "fish" => Some("-i"),
        "pwsh" | "powershell" => Some("-NoLogo"),
        // cmd.exe has no interactive flag; spawning it bare already gives a
        // prompt under ConPTY.
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    #[test]
    fn spawn_sh_echo_observes_output() {
        let cfg = ShellConfig {
            program: "/bin/sh".to_owned(),
            ..Default::default()
        };
        let mut shell = Shell::spawn(cfg).expect("spawn /bin/sh");
        shell.write_line("echo sicompass-shell-test").expect("write");

        let mut acc: Vec<u8> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            acc.extend(shell.drain_output());
            if String::from_utf8_lossy(&acc).contains("sicompass-shell-test") {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "did not observe echoed marker; got: {:?}",
            String::from_utf8_lossy(&acc)
        );
    }

    #[cfg(unix)]
    #[test]
    fn kill_terminates_child() {
        let cfg = ShellConfig {
            program: "/bin/sh".to_owned(),
            ..Default::default()
        };
        let mut shell = Shell::spawn(cfg).expect("spawn /bin/sh");
        assert!(shell.is_alive(), "child should be alive after spawn");
        shell.kill().expect("kill");

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !shell.is_alive() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("child still alive after kill");
    }

    #[test]
    fn default_program_returns_non_empty() {
        assert!(!default_program().is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn foreground_busy_false_at_prompt_true_while_running() {
        // A freshly spawned interactive shell sits at its prompt, so the
        // foreground process group is the shell itself → not busy.
        let cfg = ShellConfig {
            program: "/bin/sh".to_owned(),
            ..Default::default()
        };
        let mut shell = Shell::spawn(cfg).expect("spawn /bin/sh");
        // Give the shell a moment to reach its prompt.
        let settle = Instant::now() + Duration::from_millis(300);
        while Instant::now() < settle {
            let _ = shell.drain_output();
            thread::sleep(Duration::from_millis(20));
        }
        assert!(!shell.foreground_busy(), "idle shell at prompt is not busy");

        // Launch a foreground command that blocks; the shell hands the terminal
        // to it, so `foreground_busy` must report true while it runs.
        shell.write_line("sleep 5").expect("write");
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut saw_busy = false;
        while Instant::now() < deadline {
            let _ = shell.drain_output();
            if shell.foreground_busy() {
                saw_busy = true;
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(saw_busy, "shell should report busy while `sleep` runs");
    }

    #[cfg(windows)]
    #[test]
    fn spawn_cmd_echo_observes_output() {
        // The submitted command must not contain the marker string itself,
        // or cmd's typing-echo alone would satisfy the assertion even when
        // Enter was never registered. We use `echo MARKER` so the marker
        // appears in output only if the command actually executed.
        let cfg = ShellConfig {
            program: "cmd.exe".to_owned(),
            ..Default::default()
        };
        let mut shell = Shell::spawn(cfg).expect("spawn cmd.exe");
        shell.write_line("echo SICOMPASS_OK").expect("write");

        let mut acc: Vec<u8> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            acc.extend(shell.drain_output());
            // The marker must appear at least twice: once as cmd's
            // typing-echo of the submitted line, and once as the actual
            // output of `echo`. A single occurrence means Enter was never
            // registered and the command never ran.
            let text = String::from_utf8_lossy(&acc);
            if text.matches("SICOMPASS_OK").count() >= 2 {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "did not observe executed marker; got: {:?}",
            String::from_utf8_lossy(&acc)
        );
    }

    /// A renamed launch link (`title`) runs a *copy* of `cmd.exe` from a temp
    /// dir. cmd loads its user-facing strings from a satellite `cmd.exe.mui`
    /// resolved next to the running image, so without the MUI being replicated
    /// (see `copy_mui_siblings`) every message-table lookup fails with "The
    /// system cannot find message text for message number 0x… in the message
    /// file for Application" — `dir` in particular becomes unusable. This
    /// guards that regression: `dir` under a titled cmd must produce real
    /// output, not the message-lookup error.
    #[cfg(windows)]
    #[test]
    fn titled_cmd_loads_message_strings() {
        let cfg = ShellConfig {
            program: "cmd.exe".to_owned(),
            title: Some("sicompass-shell".to_owned()),
            ..Default::default()
        };
        let mut shell = Shell::spawn(cfg).expect("spawn cmd.exe");
        shell.write_line("dir").expect("write");

        let mut acc: Vec<u8> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            acc.extend(shell.drain_output());
            let text = String::from_utf8_lossy(&acc);
            // `dir`'s summary line is loaded from the MUI; its presence means
            // message strings resolved.
            if text.contains("File(s)") {
                assert!(
                    !text.contains("cannot find message text"),
                    "dir emitted message-lookup errors: {text:?}"
                );
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "did not observe dir summary; got: {:?}",
            String::from_utf8_lossy(&acc)
        );
    }

    #[cfg(windows)]
    #[test]
    fn kill_terminates_cmd_child() {
        let cfg = ShellConfig {
            program: "cmd.exe".to_owned(),
            ..Default::default()
        };
        let mut shell = Shell::spawn(cfg).expect("spawn cmd.exe");
        assert!(shell.is_alive());
        shell.kill().expect("kill");

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !shell.is_alive() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("child still alive after kill");
    }

    #[cfg(unix)]
    #[test]
    fn make_title_link_creates_symlink_to_program() {
        let target = std::path::Path::new("/bin/sh");
        if !target.exists() { return; }
        let (dir, link) = make_title_link(target, "sicompass-shell")
            .expect("symlink should be creatable in temp dir");
        assert!(link.exists(), "link should exist");
        assert_eq!(link.file_name().unwrap(), "sicompass-shell");
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_program_handles_absolute_and_path_lookup() {
        // Absolute path that exists resolves to itself.
        #[cfg(unix)]
        if std::path::Path::new("/bin/sh").exists() {
            assert_eq!(resolve_program("/bin/sh").unwrap(),
                std::path::PathBuf::from("/bin/sh"));
        }
        // A clearly-missing absolute path resolves to None.
        assert!(resolve_program("/definitely/not/here/xyzzy").is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn title_sets_process_name_comm() {
        // Works for any shell; use /bin/sh which is always present.
        let cfg = ShellConfig {
            program: "/bin/sh".to_owned(),
            title: Some("sicompass-shell".to_owned()),
            ..Default::default()
        };
        let shell = Shell::spawn(cfg).expect("spawn shell with title");
        let pid = shell.pid().expect("pid");

        // The renamed link is what's exec'd, so the kernel sets `comm` to it.
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm")) {
                if comm.trim_end() == "sicompass-shell" {
                    return;
                }
            }
            if Instant::now() >= deadline {
                let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
                    .unwrap_or_default();
                panic!("comm never became the title; comm={comm:?}");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn interactive_flag_recognises_unix_shells() {
        assert_eq!(interactive_flag("/bin/bash"), Some("-i"));
        assert_eq!(interactive_flag("/usr/bin/zsh"), Some("-i"));
        assert_eq!(interactive_flag("fish"), Some("-i"));
    }

    #[test]
    fn interactive_flag_recognises_windows_shells() {
        assert_eq!(interactive_flag("powershell.exe"), Some("-NoLogo"));
        assert_eq!(interactive_flag("PowerShell.EXE"), Some("-NoLogo"));
        assert_eq!(interactive_flag("pwsh"), Some("-NoLogo"));
        // cmd.exe and unknown shells get no flag.
        assert_eq!(interactive_flag("cmd.exe"), None);
        assert_eq!(interactive_flag("CMD.exe"), None);
        assert_eq!(interactive_flag("nu"), None);
    }
}
