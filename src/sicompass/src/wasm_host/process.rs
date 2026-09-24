//! `sicompass:plugin/process`: programs on the user's machine.
//!
//! The one interface that reaches outside the sandbox: a started program runs
//! with the user's full rights. So it is held the tightest:
//!
//! - linked only when `permissions.process` lists programs (and the user
//!   approved that list, see `plugin_manifest::grants_for`);
//! - only a listed program starts, by bare name, resolved on `PATH` when started
//!   (`$SHELL` is the user's login shell);
//! - its working directory must lie inside a folder the plugin was granted, or
//!   is the user's home;
//! - reads never block (a reader thread per stream fills a capped buffer);
//! - dropping the resource, or the whole instance, kills the program, so a
//!   plugin cannot leave processes behind.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use wasmtime::component::Resource;

use super::HostState;
use super::sicompass::plugin as wit;

/// Most unread output kept per stream. A reader thread waits while its buffer
/// is full, so a plugin that stops reading slows the program down instead of
/// growing host memory.
const MAX_BUFFERED: usize = 8 * 1024 * 1024;

/// A running program: the host side of the `child` resource.
pub struct ProcessChild {
    out: Arc<Mutex<Vec<u8>>>,
    err: Arc<Mutex<Vec<u8>>>,
    writer: Option<Box<dyn Write + Send>>,
    kind: Kind,
}

enum Kind {
    Pty {
        master: Box<dyn MasterPty + Send>,
        child: Box<dyn portable_pty::Child + Send + Sync>,
    },
    Pipes(std::process::Child),
}

impl ProcessChild {
    fn kill(&mut self) {
        match &mut self.kind {
            Kind::Pty { child, .. } => {
                let _ = child.kill();
            }
            Kind::Pipes(child) => {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn try_wait(&mut self) -> Option<i32> {
        match &mut self.kind {
            Kind::Pty { child, .. } => child
                .try_wait()
                .ok()
                .flatten()
                .map(|s| i32::try_from(s.exit_code()).unwrap_or(-1)),
            Kind::Pipes(child) => child
                .try_wait()
                .ok()
                .flatten()
                .map(|s| s.code().unwrap_or(-1)),
        }
    }
}

impl ProcessChild {
    /// The operating system's id for the program, for tests and diagnostics.
    pub fn pid(&self) -> Option<u32> {
        match &self.kind {
            Kind::Pty { child, .. } => child.process_id(),
            Kind::Pipes(child) => Some(child.id()),
        }
    }
}

impl Drop for ProcessChild {
    fn drop(&mut self) {
        if self.try_wait().is_none() {
            self.kill();
        }
    }
}

/// Copy a stream into a shared buffer until it ends.
fn pump(mut from: impl Read + Send + 'static, into: Arc<Mutex<Vec<u8>>>, what: &'static str) {
    let _ = std::thread::Builder::new()
        .name(format!("plugin-{what}"))
        .spawn(move || {
            let mut chunk = [0u8; 16 * 1024];
            loop {
                while into.lock().map(|b| b.len() >= MAX_BUFFERED).unwrap_or(false) {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                match from.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if let Ok(mut b) = into.lock() {
                            b.extend_from_slice(&chunk[..n]);
                        }
                    }
                }
            }
        });
}

fn take(buf: &Arc<Mutex<Vec<u8>>>, max: u32) -> Vec<u8> {
    let Ok(mut b) = buf.lock() else {
        return Vec::new();
    };
    let n = b.len().min(max as usize);
    b.drain(..n).collect()
}

/// The user's login shell, the way the flake's dev shell finds it: the user
/// database first, then `$SHELL`, then `/bin/sh`.
fn login_shell() -> PathBuf {
    #[cfg(unix)]
    {
        let me = std::env::var("USER").or_else(|_| std::env::var("LOGNAME"));
        if let (Ok(me), Ok(passwd)) = (me, std::fs::read_to_string("/etc/passwd")) {
            for line in passwd.lines() {
                let fields: Vec<&str> = line.split(':').collect();
                if fields.len() >= 7 && fields[0] == me && !fields[6].is_empty() {
                    return PathBuf::from(fields[6]);
                }
            }
        }
    }
    std::env::var_os("SHELL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

/// Resolve `program` if the plugin may start it: listed, a bare name, on `PATH`.
pub fn resolve_program(program: &str, allowed: &[String]) -> Result<PathBuf, String> {
    if !allowed.iter().any(|a| a == program) {
        return Err(format!(
            "`{program}` is not among the programs this plugin may start ({})",
            allowed.join(", ")
        ));
    }
    if program == "$SHELL" {
        return Ok(login_shell());
    }
    if program.is_empty() || program.contains(['/', '\\']) {
        return Err(format!("`{program}` must be a program name, not a path"));
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if is_executable(&candidate) {
            return Ok(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{program}.exe"));
            if exe.is_file() {
                return Ok(exe);
            }
        }
    }
    Err(format!("`{program}` was not found on PATH"))
}

#[cfg(unix)]
fn is_executable(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &std::path::Path) -> bool {
    p.is_file()
}

impl HostState {
    fn start(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        env: &[(String, String)],
        pty: Option<wit::process::PtySize>,
    ) -> Result<ProcessChild, String> {
        let exe = resolve_program(program, &self.process_allowed)?;
        let dir = match cwd {
            Some(guest) => {
                let dir = self.confine_granted(guest, true)?;
                if !dir.is_dir() {
                    return Err(format!("`{guest}` is not a directory"));
                }
                dir
            }
            None => sicompass_sdk::platform::home_dir().unwrap_or_else(|| PathBuf::from("/")),
        };
        let out = Arc::new(Mutex::new(Vec::new()));
        let err = Arc::new(Mutex::new(Vec::new()));

        if let Some(size) = pty {
            let pair = native_pty_system()
                .openpty(PtySize {
                    rows: size.rows,
                    cols: size.cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| format!("no pseudo-terminal: {e}"))?;
            let mut cmd = CommandBuilder::new(&exe);
            cmd.args(args);
            cmd.cwd(&dir);
            for (k, v) in env {
                cmd.env(k, v);
            }
            let child = pair
                .slave
                .spawn_command(cmd)
                .map_err(|e| format!("cannot start `{program}`: {e}"))?;
            drop(pair.slave);
            let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
            let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
            pump(reader, out.clone(), "pty");
            return Ok(ProcessChild {
                out,
                err,
                writer: Some(writer),
                kind: Kind::Pty {
                    master: pair.master,
                    child,
                },
            });
        }

        let mut child = std::process::Command::new(&exe)
            .args(args)
            .current_dir(&dir)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start `{program}`: {e}"))?;
        if let Some(stdout) = child.stdout.take() {
            pump(stdout, out.clone(), "stdout");
        }
        if let Some(stderr) = child.stderr.take() {
            pump(stderr, err.clone(), "stderr");
        }
        let writer = child.stdin.take().map(|s| Box::new(s) as Box<dyn Write + Send>);
        Ok(ProcessChild {
            out,
            err,
            writer,
            kind: Kind::Pipes(child),
        })
    }

    fn child(&mut self, r: &Resource<ProcessChild>) -> Option<&mut ProcessChild> {
        self.table.get_mut(r).ok()
    }
}

impl wit::process::Host for HostState {}

impl wit::process::HostChild for HostState {
    fn spawn(
        &mut self,
        program: String,
        args: Vec<String>,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        pty: Option<wit::process::PtySize>,
    ) -> Result<Resource<ProcessChild>, String> {
        let child = self.start(&program, &args, cwd.as_deref(), &env, pty)?;
        self.table.push(child).map_err(|e| e.to_string())
    }

    fn read(&mut self, r: Resource<ProcessChild>, max: u32) -> Vec<u8> {
        self.child(&r).map(|c| take(&c.out, max)).unwrap_or_default()
    }

    fn read_stderr(&mut self, r: Resource<ProcessChild>, max: u32) -> Vec<u8> {
        self.child(&r).map(|c| take(&c.err, max)).unwrap_or_default()
    }

    fn write(&mut self, r: Resource<ProcessChild>, bytes: Vec<u8>) -> Result<(), String> {
        let c = self.child(&r).ok_or("the program has gone")?;
        let w = c.writer.as_mut().ok_or("the program has no input")?;
        w.write_all(&bytes).and_then(|_| w.flush()).map_err(|e| e.to_string())
    }

    fn resize(&mut self, r: Resource<ProcessChild>, size: wit::process::PtySize) {
        if let Some(ProcessChild {
            kind: Kind::Pty { master, .. },
            ..
        }) = self.child(&r)
        {
            let _ = master.resize(PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    fn try_wait(&mut self, r: Resource<ProcessChild>) -> Option<i32> {
        self.child(&r).and_then(|c| c.try_wait())
    }

    fn kill(&mut self, r: Resource<ProcessChild>) {
        if let Some(c) = self.child(&r) {
            c.kill();
        }
    }

    fn drop(&mut self, r: Resource<ProcessChild>) -> wasmtime::Result<()> {
        // Dropping the ProcessChild kills a program that is still running.
        self.table.delete(r)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn only_listed_bare_names_resolve() {
        let list = allowed(&["sh"]);
        assert!(resolve_program("sh", &list).unwrap().is_absolute());
        assert!(resolve_program("rm", &list).unwrap_err().contains("not among"));
        let list = allowed(&["/bin/sh", "../sh"]);
        assert!(resolve_program("/bin/sh", &list).unwrap_err().contains("not a path"));
        assert!(resolve_program("../sh", &list).unwrap_err().contains("not a path"));
    }

    #[test]
    fn a_listed_but_missing_program_says_so() {
        let list = allowed(&["no-such-program-sicompass"]);
        assert!(
            resolve_program("no-such-program-sicompass", &list)
                .unwrap_err()
                .contains("not found")
        );
    }

    /// A plugin cannot leave a program running: dropping the resource (or the
    /// instance holding it) kills it.
    #[cfg(target_os = "linux")]
    #[test]
    fn dropping_a_child_kills_the_program() {
        let mut s = HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new());
        s.process_allowed = allowed(&["sleep"]);
        let child = s.start("sleep", &["30".to_owned()], None, &[], None).unwrap();
        let pid = child.pid().unwrap();
        let alive = |pid: u32| {
            std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .is_ok_and(|st| !st.split(')').nth(1).unwrap_or("").trim_start().starts_with('Z'))
        };
        assert!(alive(pid));
        drop(child);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while alive(pid) {
            assert!(std::time::Instant::now() < deadline, "the program outlived its resource");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn shell_means_the_login_shell() {
        let p = resolve_program("$SHELL", &allowed(&["$SHELL"])).unwrap();
        assert!(p.is_absolute(), "{}", p.display());
    }
}
