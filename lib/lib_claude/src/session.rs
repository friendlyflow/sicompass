//! Child-process management for a streaming `claude` session.
//!
//! Mirrors the background-reader-thread pattern of `sicompass-shell`'s `Shell`,
//! but uses **plain pipes** (`std::process::Command`) rather than a PTY:
//! `stream-json` wants a clean newline-delimited byte stream, not a terminal
//! grid. A reader thread blocks on `BufReader::read_line` so the synchronous
//! `Provider` methods only ever do non-blocking drains.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

/// How to spawn the `claude` child process.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// `claude` binary — a bare name (PATH-searched) or an absolute path.
    pub program: String,
    /// `--permission-mode` value: default | acceptEdits | plan | bypassPermissions.
    pub permission_mode: String,
    /// `--model` override; `None` omits the flag.
    pub model: Option<String>,
    /// Free-form extra CLI args appended verbatim.
    pub extra_args: Vec<String>,
    /// Working directory for the child; `None` inherits the parent's.
    pub cwd: Option<PathBuf>,
    /// `--resume <session_id>` — set when re-spawning after an unexpected exit.
    pub resume: Option<String>,
    /// Add `--include-partial-messages` to stream token-level deltas.
    pub include_partial: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            program: "claude".to_owned(),
            permission_mode: "default".to_owned(),
            model: None,
            extra_args: Vec::new(),
            cwd: None,
            resume: None,
            include_partial: true,
        }
    }
}

/// A spawned `claude --output-format stream-json` process.
///
/// `drain_lines()` is non-blocking and returns whatever complete JSONL lines
/// the background reader thread has buffered since the previous call.
pub struct Session {
    child: Child,
    stdin: ChildStdin,
    lines: Arc<Mutex<Vec<String>>>,
    stderr: Arc<Mutex<String>>,
    alive: Arc<AtomicBool>,
}

impl Session {
    /// Spawn `cfg.program` in streaming-JSON mode with piped stdio.
    pub fn spawn(cfg: &SessionConfig) -> std::io::Result<Session> {
        tracing::debug!(
            program = %cfg.program,
            permission_mode = %cfg.permission_mode,
            model = ?cfg.model,
            cwd = ?cfg.cwd,
            resume = ?cfg.resume,
            include_partial = cfg.include_partial,
            "claude: Session::spawn"
        );
        let mut cmd = new_command(&cfg.program)?;
        cmd.args([
            "--print",
            "--output-format",
            "stream-json",
            "--input-format",
            "stream-json",
            "--verbose",
        ]);
        if cfg.include_partial {
            cmd.arg("--include-partial-messages");
        }
        cmd.args(["--permission-mode", &cfg.permission_mode]);
        if let Some(model) = &cfg.model {
            if !model.is_empty() {
                cmd.args(["--model", model]);
            }
        }
        if let Some(resume) = &cfg.resume {
            if !resume.is_empty() {
                cmd.args(["--resume", resume]);
            }
        }
        for arg in &cfg.extra_args {
            cmd.arg(arg);
        }
        if let Some(cwd) = &cfg.cwd {
            cmd.current_dir(cwd);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(program = %cfg.program, error = %e, kind = ?e.kind(), "claude: spawn failed");
                return Err(e);
            }
        };
        tracing::debug!(pid = child.id(), "claude: child spawned");
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let stderr_buf = Arc::new(Mutex::new(String::new()));
        let alive = Arc::new(AtomicBool::new(true));

        // stdout reader: BufReader::read_line reassembles partial lines across
        // read boundaries for free, so each pushed entry is a complete line.
        {
            let lines = Arc::clone(&lines);
            let alive = Arc::clone(&alive);
            thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut buf = String::new();
                loop {
                    buf.clear();
                    match reader.read_line(&mut buf) {
                        Ok(0) => break,
                        Ok(_) => {
                            let line = buf.trim_end_matches(['\r', '\n']).to_owned();
                            if !line.is_empty() {
                                if let Ok(mut l) = lines.lock() {
                                    l.push(line);
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
                alive.store(false, Ordering::SeqCst);
            });
        }

        // stderr reader: accumulate so the provider can surface a real error.
        {
            let stderr_buf = Arc::clone(&stderr_buf);
            thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut buf = String::new();
                loop {
                    buf.clear();
                    match reader.read_line(&mut buf) {
                        Ok(0) => break,
                        Ok(_) => {
                            if let Ok(mut s) = stderr_buf.lock() {
                                s.push_str(&buf);
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        Ok(Session {
            child,
            stdin,
            lines,
            stderr: stderr_buf,
            alive,
        })
    }

    /// Take all complete JSONL lines buffered since the last call.
    pub fn drain_lines(&self) -> Vec<String> {
        match self.lines.lock() {
            Ok(mut l) => std::mem::take(&mut *l),
            Err(_) => Vec::new(),
        }
    }

    /// Take all stderr text accumulated so far.
    pub fn take_stderr(&self) -> String {
        match self.stderr.lock() {
            Ok(mut s) => std::mem::take(&mut *s),
            Err(_) => String::new(),
        }
    }

    /// Write one JSONL user message to the child's stdin.
    pub fn write_user(&mut self, json_line: &str) -> std::io::Result<()> {
        self.stdin.write_all(json_line.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()
    }

    /// `true` while the stdout reader has not yet seen EOF and the child has
    /// not been reaped.
    pub fn is_alive(&mut self) -> bool {
        if !self.alive.load(Ordering::SeqCst) {
            return false;
        }
        !matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Kill the child and reap it.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.alive.store(false, Ordering::SeqCst);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Build the `Command` used to launch the `claude` CLI.
///
/// On Unix a bare `Command::new` is enough. On Windows it is not: npm installs
/// the CLI as a batch shim `claude.cmd` (the native installer as `claude.exe`),
/// but Rust's `Command` only appends `.exe` when PATH-searching a bare name, so
/// `claude.cmd` is invisible and the spawn fails with `NotFound`. We resolve the
/// name against `PATHEXT` ourselves and hand the full path to `Command::new`;
/// for a `.cmd`/`.bat` shim std then routes through `cmd.exe` with hardened
/// argument escaping. We also set `CREATE_NO_WINDOW` so launching the shim from
/// a TUI app doesn't flash a console window.
#[cfg(not(windows))]
fn new_command(program: &str) -> std::io::Result<Command> {
    Ok(Command::new(program))
}

#[cfg(windows)]
fn new_command(program: &str) -> std::io::Result<Command> {
    use std::os::windows::process::CommandExt;
    /// Suppress the console window a batch shim would otherwise pop up.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let resolved = resolve_windows_program(program).ok_or_else(|| {
        tracing::error!(program, "claude: resolve_windows_program found nothing on PATH/PATHEXT");
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("`{program}` not found on PATH"),
        )
    })?;
    tracing::debug!(program, resolved = %resolved.display(), "claude: resolved windows program");
    let mut cmd = Command::new(resolved);
    cmd.creation_flags(CREATE_NO_WINDOW);
    Ok(cmd)
}

/// Resolve a Windows program name to a concrete executable path, honoring
/// `PATHEXT`. Bare names are searched on `PATH`; names containing a separator
/// (or an absolute path) are tried as given. In both cases, when the name has
/// no extension we also try each `PATHEXT` suffix (`.CMD`, `.EXE`, …) so npm's
/// `claude.cmd` shim is found.
///
/// When the `PATH` search comes up empty for a bare name, we also probe the
/// well-known per-user install locations claude ships to. This matters on
/// Windows: the native installer drops `claude.exe` in `%USERPROFILE%\.local\bin`
/// and adds that directory to the **User** `PATH`, but a process launched from a
/// terminal that started *before* the installer ran inherits the stale `PATH`
/// without it. Without this fallback the app then reports "not found" even
/// though claude is installed — the exact "works on Linux, not Windows" failure
/// (on Linux `~/.local/bin` is already on `PATH` via the shell profile).
///
/// Returns `None` if nothing matches.
#[cfg(windows)]
fn resolve_windows_program(program: &str) -> Option<PathBuf> {
    use std::path::Path;

    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned())
        .split(';')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();

    // Try `base` verbatim, then — only if it carries no extension — `base` with
    // each PATHEXT suffix (so `claude.exe` is never mangled into `claude.exe.cmd`).
    let try_with_exts = |base: &Path| -> Option<PathBuf> {
        if base.is_file() {
            return Some(base.to_path_buf());
        }
        if base.extension().is_none() {
            let base_str = base.to_str()?;
            for ext in &exts {
                let cand = PathBuf::from(format!("{base_str}{ext}"));
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
        None
    };

    let p = Path::new(program);
    if p.is_absolute() || program.contains(['\\', '/']) {
        return try_with_exts(p);
    }

    // Search PATH first, then the known per-user install dirs a stale PATH may
    // have missed. Ordering matters: an on-PATH hit always wins over a fallback.
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    resolve_bare_in_dirs(program, &exts, path_dirs.iter().chain(windows_fallback_dirs().iter()))
}

/// Search `dirs` (in order) for a bare `program`, honoring `exts` when the name
/// carries no extension. Split out so the PATH-miss → fallback-hit behavior is
/// testable without mutating process-global environment variables.
#[cfg(windows)]
fn resolve_bare_in_dirs<'a, I>(program: &str, exts: &[String], dirs: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = &'a PathBuf>,
{
    use std::path::Path;
    let try_with_exts = |base: &Path| -> Option<PathBuf> {
        if base.is_file() {
            return Some(base.to_path_buf());
        }
        if base.extension().is_none() {
            let base_str = base.to_str()?;
            for ext in exts {
                let cand = PathBuf::from(format!("{base_str}{ext}"));
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
        None
    };
    dirs.into_iter().find_map(|dir| try_with_exts(&dir.join(program)))
}

/// Well-known per-user directories claude's Windows installers place shims in,
/// probed only after a `PATH` search fails (see [`resolve_windows_program`]).
#[cfg(windows)]
fn windows_fallback_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    // Native installer → %USERPROFILE%\.local\bin\claude.exe
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        dirs.push(PathBuf::from(&profile).join(".local").join("bin"));
    }
    // npm global → %APPDATA%\npm\claude.cmd
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(&appdata).join("npm"));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_missing_binary_errors() {
        let cfg = SessionConfig {
            program: "definitely-not-claude-xyz-9000".to_owned(),
            ..SessionConfig::default()
        };
        let result = Session::spawn(&cfg);
        assert!(result.is_err(), "missing binary should fail");
        assert_eq!(
            result.err().unwrap().kind(),
            std::io::ErrorKind::NotFound
        );
    }

    // On Windows npm ships the CLI as `claude.cmd`, which a bare
    // `Command::new("claude")` never finds. `resolve_windows_program` must pick
    // it up by appending a PATHEXT suffix to an extensionless name. Uses an
    // explicit path (no PATH mutation) to stay deterministic and race-free.
    #[test]
    #[cfg(windows)]
    fn resolve_finds_cmd_shim_by_pathext() {
        let dir = std::env::temp_dir().join(format!(
            "lib-claude-resolve-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let shim = dir.join("claude.cmd");
        std::fs::write(&shim, b"@echo off\r\n").unwrap();

        // Ask for the extensionless path; resolution should append a PATHEXT
        // suffix and land on the shim. The suffix's case follows PATHEXT (often
        // uppercase `.CMD`), which the case-insensitive filesystem still opens,
        // so compare case-insensitively rather than byte-for-byte.
        let extless = dir.join("claude");
        let got = resolve_windows_program(extless.to_str().unwrap())
            .expect("shim should resolve");
        assert!(got.is_file(), "resolved path must exist");
        assert_eq!(
            got.to_string_lossy().to_lowercase(),
            shim.to_string_lossy().to_lowercase()
        );

        // A name that resolves to nothing yields None.
        assert!(resolve_windows_program(&dir.join("nope").to_string_lossy()).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // A stale PATH (missing the install dir) must still resolve when the binary
    // sits in a fallback dir — the real Windows failure mode where the native
    // installer added %USERPROFILE%\.local\bin to PATH but the running process
    // inherited the pre-install PATH. Exercises the dir-search seam directly so
    // no process-global PATH/USERPROFILE mutation is needed.
    #[test]
    #[cfg(windows)]
    fn resolve_bare_falls_back_to_known_dir() {
        let dir = std::env::temp_dir().join(format!(
            "lib-claude-fallback-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("claude.exe"), b"MZ").unwrap();

        let exts: Vec<String> =
            [".COM", ".EXE", ".CMD"].iter().map(|s| s.to_string()).collect();

        // PATH-only dirs (no match) → None.
        let empty: Vec<PathBuf> = vec![std::env::temp_dir()];
        assert!(resolve_bare_in_dirs("claude", &exts, empty.iter()).is_none());

        // PATH miss followed by the fallback dir → hit (order preserved).
        let with_fallback: Vec<PathBuf> = vec![std::env::temp_dir(), dir.clone()];
        let got = resolve_bare_in_dirs("claude", &exts, with_fallback.iter())
            .expect("should resolve in fallback dir");
        assert_eq!(
            got.to_string_lossy().to_lowercase(),
            dir.join("claude.exe").to_string_lossy().to_lowercase()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_default_is_plain_claude() {
        let cfg = SessionConfig::default();
        assert_eq!(cfg.program, "claude");
        assert_eq!(cfg.permission_mode, "default");
        assert!(cfg.model.is_none());
        assert!(cfg.resume.is_none());
    }

    // Reader-thread plumbing: a child that echoes stdin to stdout should have
    // each written line surface back through `drain_lines()`. `cat` is the
    // simplest such program; skipped on platforms without it. The reader/
    // stderr threads are wired exactly as in `Session::spawn` — only the
    // process and its args differ (plain `cat`, no claude flags).
    #[test]
    #[cfg(unix)]
    fn write_then_drain_round_trips_a_line() {
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn cat");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let alive = Arc::new(AtomicBool::new(true));
        {
            let lines = Arc::clone(&lines);
            let alive = Arc::clone(&alive);
            thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut buf = String::new();
                loop {
                    buf.clear();
                    match reader.read_line(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let line = buf.trim_end_matches(['\r', '\n']).to_owned();
                            if !line.is_empty() {
                                lines.lock().unwrap().push(line);
                            }
                        }
                    }
                }
                alive.store(false, Ordering::SeqCst);
            });
        }
        let mut session = Session {
            child,
            stdin,
            lines,
            stderr: Arc::new(Mutex::new(String::new())),
            alive,
        };
        session.write_user("hello").expect("write");
        let mut drained = Vec::new();
        for _ in 0..200 {
            drained = session.drain_lines();
            if !drained.is_empty() {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(drained, vec!["hello".to_owned()]);
        session.kill();
    }
}
