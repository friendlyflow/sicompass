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
//! - `try-wait` reports the exit only once that output has all arrived, so a
//!   plugin that reads until then has the whole of it;
//! - dropping the resource, or the whole instance, kills the program, so a
//!   plugin cannot leave processes behind.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use wasmtime::component::Resource;

use super::HostState;
use super::sicompass::plugin as wit;

/// Most unread output kept per stream. A reader thread waits while its buffer
/// is full, so a plugin that stops reading slows the program down instead of
/// growing host memory.
const MAX_BUFFERED: usize = 8 * 1024 * 1024;

/// How long after a program exits `try-wait` still waits for its output to
/// arrive. Normally the streams end with the program, well within this. A
/// program it started in the background can keep one open for as long as that
/// runs, and the exit must not wait on that.
const OUTPUT_GRACE: Duration = Duration::from_millis(500);

/// A running program: the host side of the `child` resource.
pub struct ProcessChild {
    out: Arc<Mutex<Vec<u8>>>,
    err: Arc<Mutex<Vec<u8>>>,
    writer: Option<Box<dyn Write + Send>>,
    /// What the program wrote to its channel (fd 4), with `spawn-with-channel`.
    chan_out: Arc<Mutex<Vec<u8>>>,
    /// Its channel's other end (fd 3), with `spawn-with-channel`.
    chan_in: Option<Box<dyn Write + Send>>,
    kind: Kind,
    /// Reader threads still copying output into `out` and `err`.
    open: Arc<AtomicUsize>,
    /// The exit code, and when it was first seen.
    exited: Option<(i32, Instant)>,
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

    /// The exit code once the program has exited and its output has arrived
    /// (or [`OUTPUT_GRACE`] has passed), so a `read` after this returns the
    /// rest of it. Reporting the bare exit would let a plugin stop reading
    /// with the last of the output still in a pipe, and a truncated
    /// `git status` reads as a clean tree.
    fn try_wait(&mut self) -> Option<i32> {
        let (code, at) = match self.exited {
            Some(e) => e,
            None => {
                let e = (self.exit_code()?, Instant::now());
                self.exited = Some(e);
                e
            }
        };
        (self.open.load(Ordering::Acquire) == 0 || at.elapsed() >= OUTPUT_GRACE).then_some(code)
    }

    fn exit_code(&mut self) -> Option<i32> {
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

    /// Its working directory now (Linux: `/proc/<pid>/cwd`).
    fn cwd(&mut self) -> Option<String> {
        if self.exited.is_some() || self.exit_code().is_some() {
            return None;
        }
        #[cfg(target_os = "linux")]
        {
            let pid = self.pid()?;
            std::fs::read_link(format!("/proc/{pid}/cwd"))
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    /// Whether another process group holds its PTY's foreground.
    ///
    /// Linux: the program's `/proc/<pid>/stat` has its own process group
    /// (`pgrp`) and its terminal's foreground group (`tpgid`). At a prompt the
    /// shell *is* the foreground group; while a command runs the shell has put
    /// that command in a group of its own, and the two differ.
    fn foreground_busy(&self) -> bool {
        if !matches!(self.kind, Kind::Pty { .. }) {
            return false;
        }
        #[cfg(target_os = "linux")]
        {
            let Some(pid) = self.pid() else {
                return false;
            };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                return false;
            };
            // `comm` (field 2) is parenthesised and may itself hold spaces or
            // parentheses, so the fields are counted after the last `)`:
            // state, ppid, pgrp, session, tty_nr, tpgid.
            let Some(rparen) = stat.rfind(')') else {
                return false;
            };
            let fields: Vec<&str> = stat[rparen + 1..].split_whitespace().collect();
            let pgrp = fields.get(2).and_then(|s| s.parse::<i32>().ok());
            let tpgid = fields.get(5).and_then(|s| s.parse::<i32>().ok());
            matches!((pgrp, tpgid), (Some(pg), Some(tp)) if tp >= 0 && tp != pg)
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }
}

impl Drop for ProcessChild {
    fn drop(&mut self) {
        if self.exited.is_none() && self.exit_code().is_none() {
            self.kill();
        }
    }
}

/// Copy a stream into a shared buffer until it ends. `open` counts the pumps
/// still running.
fn pump(
    mut from: impl Read + Send + 'static,
    into: Arc<Mutex<Vec<u8>>>,
    open: &Arc<AtomicUsize>,
    what: &'static str,
) {
    struct Done(Arc<AtomicUsize>);
    impl Drop for Done {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::AcqRel);
        }
    }
    open.fetch_add(1, Ordering::AcqRel);
    // Dropped when the thread ends, or right here if it never started.
    let done = Done(open.clone());
    let _ = std::thread::Builder::new()
        .name(format!("plugin-{what}"))
        .spawn(move || {
            let _done = done;
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
    // After PATH, the user's own `~/.local/bin`, where per-user installers
    // (Claude Code's, pip's) put programs. A desktop session's PATH often
    // lacks it, and one started before the installer ran always does.
    let own_bin = sicompass_sdk::platform::home_dir().map(|h| h.join(".local").join("bin"));
    find_program(program, std::env::split_paths(&path).chain(own_bin))
}

/// The first of `dirs` that holds `program`.
fn find_program(
    program: &str,
    dirs: impl Iterator<Item = PathBuf>,
) -> Result<PathBuf, String> {
    for dir in dirs {
        for candidate in candidates(&dir, program) {
            if is_executable(&candidate) {
                return Ok(candidate);
            }
        }
    }
    Err(format!("`{program}` was not found on PATH"))
}

/// The files `program` can be in `dir`.
#[cfg(not(windows))]
fn candidates(dir: &std::path::Path, program: &str) -> Vec<PathBuf> {
    vec![dir.join(program)]
}

/// On Windows, with each `PATHEXT` extension too: npm installs a CLI as a
/// `.cmd` shim, which a bare name never matches.
#[cfg(windows)]
fn candidates(dir: &std::path::Path, program: &str) -> Vec<PathBuf> {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned());
    std::iter::once(dir.join(program))
        .chain(
            exts.split(';')
                .filter(|e| !e.is_empty())
                .map(|e| dir.join(format!("{program}{}", e.to_ascii_lowercase()))),
        )
        .collect()
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

/// The two pipes of a message channel, before and after the program starts.
struct Channel {
    /// Becomes the program's fd 3.
    child_reads: std::io::PipeReader,
    /// Becomes the program's fd 4.
    child_writes: std::io::PipeWriter,
    /// The host writes here (`channel-write`).
    to_child: std::io::PipeWriter,
    /// The host reads here (`channel-read`).
    from_child: std::io::PipeReader,
}

/// Give `cmd` a message channel on fds 3 (it reads) and 4 (it writes).
#[cfg(unix)]
fn attach_channel(cmd: &mut std::process::Command) -> Result<Channel, String> {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    let (child_reads, to_child) = std::io::pipe().map_err(|e| e.to_string())?;
    let (from_child, child_writes) = std::io::pipe().map_err(|e| e.to_string())?;
    let (r, w) = (child_reads.as_raw_fd(), child_writes.as_raw_fd());
    // SAFETY: runs in the forked child before exec, and calls only fcntl, dup2
    // and close, which are async-signal-safe. The pipes are close-on-exec, so
    // only the copies on 3 and 4 (dup2 clears the flag) reach the program. The
    // copies above 10 first keep a pipe that happens to sit on 3 or 4 from
    // being overwritten by the other.
    unsafe {
        cmd.pre_exec(move || {
            let (a, b) = (libc::fcntl(r, libc::F_DUPFD, 10), libc::fcntl(w, libc::F_DUPFD, 10));
            if a < 0 || b < 0 || libc::dup2(a, 3) < 0 || libc::dup2(b, 4) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            libc::close(a);
            libc::close(b);
            Ok(())
        });
    }
    Ok(Channel {
        child_reads,
        child_writes,
        to_child,
        from_child,
    })
}

#[cfg(not(unix))]
fn attach_channel(_cmd: &mut std::process::Command) -> Result<Channel, String> {
    Err("a program with a message channel needs a Unix system".to_owned())
}

impl HostState {
    fn start(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        env: &[(String, String)],
        unset: &[String],
        pty: Option<wit::process::PtySize>,
    ) -> Result<ProcessChild, String> {
        self.launch(program, args, cwd, env, unset, pty, false)
    }

    /// `start` on pipes, with the message channel on fds 3 and 4.
    fn start_with_channel(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        env: &[(String, String)],
        unset: &[String],
    ) -> Result<ProcessChild, String> {
        self.launch(program, args, cwd, env, unset, None, true)
    }

    #[allow(clippy::too_many_arguments)] // `start`'s arguments, and how to connect them
    fn launch(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        env: &[(String, String)],
        unset: &[String],
        pty: Option<wit::process::PtySize>,
        channel: bool,
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
        let chan_out = Arc::new(Mutex::new(Vec::new()));
        let open = Arc::new(AtomicUsize::new(0));

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
            for k in unset {
                cmd.env_remove(k);
            }
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
            pump(reader, out.clone(), &open, "pty");
            return Ok(ProcessChild {
                out,
                err,
                writer: Some(writer),
                chan_out,
                chan_in: None,
                kind: Kind::Pty {
                    master: pair.master,
                    child,
                },
                open,
                exited: None,
            });
        }

        let mut cmd = std::process::Command::new(&exe);
        for k in unset {
            cmd.env_remove(k);
        }
        let chan = if channel {
            Some(attach_channel(&mut cmd)?)
        } else {
            None
        };
        let mut child = cmd
            .args(args)
            .current_dir(&dir)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start `{program}`: {e}"))?;
        if let Some(stdout) = child.stdout.take() {
            pump(stdout, out.clone(), &open, "stdout");
        }
        if let Some(stderr) = child.stderr.take() {
            pump(stderr, err.clone(), &open, "stderr");
        }
        let writer = child.stdin.take().map(|s| Box::new(s) as Box<dyn Write + Send>);
        let chan_in = chan.map(|c| {
            // The program's ends are its own now.
            drop(c.child_reads);
            drop(c.child_writes);
            pump(c.from_child, chan_out.clone(), &open, "channel");
            Box::new(c.to_child) as Box<dyn Write + Send>
        });
        Ok(ProcessChild {
            out,
            err,
            writer,
            chan_out,
            chan_in,
            kind: Kind::Pipes(child),
            open,
            exited: None,
        })
    }

    fn child(&mut self, r: &Resource<ProcessChild>) -> Option<&mut ProcessChild> {
        self.table.get_mut(r).ok()
    }
}

impl wit::process::Host for HostState {
    fn which(&mut self, program: String) -> Result<String, String> {
        resolve_program(&program, &self.process_allowed).map(|p| p.to_string_lossy().into_owned())
    }
}

impl wit::process::HostChild for HostState {
    fn spawn(
        &mut self,
        program: String,
        args: Vec<String>,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        unset: Vec<String>,
        pty: Option<wit::process::PtySize>,
    ) -> Result<Resource<ProcessChild>, String> {
        let child = self.start(&program, &args, cwd.as_deref(), &env, &unset, pty)?;
        let pid = child.pid();
        let r = self.table.push(child).map_err(|e| e.to_string())?;
        if let Some(pid) = pid {
            self.child_pids.push((r.rep(), pid));
        }
        Ok(r)
    }

    fn spawn_with_channel(
        &mut self,
        program: String,
        args: Vec<String>,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        unset: Vec<String>,
    ) -> Result<Resource<ProcessChild>, String> {
        let child = self.start_with_channel(&program, &args, cwd.as_deref(), &env, &unset)?;
        let pid = child.pid();
        let r = self.table.push(child).map_err(|e| e.to_string())?;
        if let Some(pid) = pid {
            self.child_pids.push((r.rep(), pid));
        }
        Ok(r)
    }

    fn channel_read(&mut self, r: Resource<ProcessChild>, max: u32) -> Vec<u8> {
        self.child(&r).map(|c| take(&c.chan_out, max)).unwrap_or_default()
    }

    fn channel_write(&mut self, r: Resource<ProcessChild>, bytes: Vec<u8>) -> Result<(), String> {
        let c = self.child(&r).ok_or("the program has gone")?;
        let w = c.chan_in.as_mut().ok_or("the program has no channel")?;
        w.write_all(&bytes).and_then(|_| w.flush()).map_err(|e| e.to_string())
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

    fn cwd(&mut self, r: Resource<ProcessChild>) -> Option<String> {
        self.child(&r).and_then(|c| c.cwd())
    }

    fn foreground_busy(&mut self, r: Resource<ProcessChild>) -> bool {
        self.child(&r).is_some_and(|c| c.foreground_busy())
    }

    fn kill(&mut self, r: Resource<ProcessChild>) {
        if let Some(c) = self.child(&r) {
            c.kill();
        }
    }

    fn drop(&mut self, r: Resource<ProcessChild>) -> wasmtime::Result<()> {
        // Dropping the ProcessChild kills a program that is still running.
        self.child_pids.retain(|(rep, _)| *rep != r.rep());
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
        let child = s
            .start("sleep", &["30".to_owned()], None, &[], &[], None)
            .unwrap();
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

    /// Everything a program wrote is readable once `try_wait` reports its exit:
    /// a plugin that stops reading there has it all. The output is larger than
    /// the host buffer and read in small bites, so the reader thread is still
    /// throttled with the tail in the pipe when the program exits.
    #[cfg(unix)]
    #[test]
    fn the_exit_is_reported_after_the_last_of_the_output() {
        let mut s = HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new());
        s.process_allowed = allowed(&["sh"]);
        let lines = 2_000_000;
        let script = format!("seq 1 {lines}; echo stderr-end >&2");
        for _ in 0..3 {
            let mut child = s
                .start("sh", &["-c".to_owned(), script.clone()], None, &[], &[], None)
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            let (mut out, mut err) = (Vec::new(), Vec::new());
            loop {
                let exited = child.try_wait().is_some();
                if exited {
                    out.extend(take(&child.out, u32::MAX));
                    err.extend(take(&child.err, u32::MAX));
                    break;
                }
                out.extend(take(&child.out, 4096));
                err.extend(take(&child.err, 64 * 1024));
                assert!(Instant::now() < deadline, "the program never finished");
            }
            let out = String::from_utf8(out).unwrap();
            assert_eq!(out.lines().count(), lines);
            assert!(out.ends_with(&format!("{lines}\n")));
            assert_eq!(String::from_utf8(err).unwrap(), "stderr-end\n");
        }
    }

    /// Read `child` until `done` says so, or 10 s pass.
    #[cfg(target_os = "linux")]
    fn until(child: &mut ProcessChild, what: &str, done: impl Fn(&mut ProcessChild) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(child) {
            take(&child.out, u32::MAX);
            assert!(Instant::now() < deadline, "never: {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// A shell's `cd` moves what `cwd` answers, and a running command holds
    /// the foreground until it ends: what a terminal's prompt and its "a
    /// command is running" confirmation read.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_pty_shell_reports_its_directory_and_a_running_command() {
        let tmp = tempfile::TempDir::new().unwrap();
        let there = tmp.path().canonicalize().unwrap();
        let mut s = HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new());
        s.process_allowed = allowed(&["sh"]);
        let size = wit::process::PtySize { rows: 24, cols: 80 };
        let mut child = s
            .start("sh", &["-i".to_owned()], None, &[], &[], Some(size))
            .unwrap();
        assert!(child.cwd().is_some(), "a live program has a directory");
        assert!(!child.foreground_busy(), "at the prompt");

        let w = child.writer.as_mut().unwrap();
        writeln!(w, "cd '{}'", there.display()).unwrap();
        w.flush().unwrap();
        let want = there.to_string_lossy().into_owned();
        until(&mut child, "the cd", |c| c.cwd().as_deref() == Some(want.as_str()));

        let w = child.writer.as_mut().unwrap();
        writeln!(w, "sleep 30").unwrap();
        w.flush().unwrap();
        until(&mut child, "sleep holding the terminal", |c| c.foreground_busy());

        child.kill();
        until(&mut child, "the exit", |c| c.try_wait().is_some());
        assert_eq!(child.cwd(), None, "an exited program has none");
    }

    #[test]
    fn a_program_on_pipes_is_never_foreground_busy() {
        let mut s = HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new());
        s.process_allowed = allowed(&["sleep"]);
        let child = s
            .start("sleep", &["30".to_owned()], None, &[], &[], None)
            .unwrap();
        assert!(!child.foreground_busy());
    }

    #[cfg(unix)]
    #[test]
    fn a_program_is_found_in_the_folders_after_path_in_order() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let (first, second) = (tmp.path().join("a"), tmp.path().join("b"));
        for d in [&first, &second] {
            std::fs::create_dir(d).unwrap();
        }
        let prog = second.join("sicompass-test-prog");
        std::fs::write(&prog, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Not executable: skipped, as a shell would.
        std::fs::write(first.join("sicompass-test-prog"), "").unwrap();
        let dirs = || [first.clone(), second.clone()].into_iter();
        assert_eq!(find_program("sicompass-test-prog", dirs()).unwrap(), prog);
        assert!(find_program("sicompass-missing", dirs()).is_err());
    }

    #[test]
    fn which_answers_only_for_a_listed_program() {
        use wit::process::Host;
        let mut s = HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new());
        s.process_allowed = allowed(&["sh"]);
        let sh = s.which("sh".to_owned()).unwrap();
        assert!(std::path::Path::new(&sh).is_absolute(), "{sh}");
        assert!(s.which("rm".to_owned()).unwrap_err().contains("not among"));
    }

    /// What the program writes to fd 4 comes back through the channel, and
    /// what the host sends arrives on its fd 3: Chrome's pipe convention.
    #[cfg(unix)]
    #[test]
    fn a_channel_carries_messages_both_ways_on_fds_3_and_4() {
        let mut s = HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new());
        s.process_allowed = allowed(&["sh"]);
        // Echo one line from fd 3 to fd 4, upper-cased, then say so on stdout.
        let script = "read line <&3; printf '%s\\n' \"$line\" | tr a-z A-Z >&4; echo done";
        let mut child = s
            .start_with_channel("sh", &["-c".to_owned(), script.to_owned()], None, &[], &[])
            .unwrap();
        let w = child.chan_in.as_mut().unwrap();
        w.write_all(b"{\"id\":1}\n").unwrap();
        w.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.try_wait().is_none() {
            assert!(Instant::now() < deadline, "the program never finished");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(take(&child.chan_out, u32::MAX), b"{\"ID\":1}\n");
        assert_eq!(take(&child.out, u32::MAX), b"done\n");
    }

    #[cfg(unix)]
    #[test]
    fn without_a_channel_fds_3_and_4_are_closed() {
        let mut s = HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new());
        s.process_allowed = allowed(&["sh"]);
        let script = "if [ -e /proc/self/fd/3 ] || [ -e /proc/self/fd/4 ]; then echo open; else echo closed; fi";
        let mut child = s
            .start("sh", &["-c".to_owned(), script.to_owned()], None, &[], &[], None)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.try_wait().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        if cfg!(target_os = "linux") {
            assert_eq!(take(&child.out, u32::MAX), b"closed\n");
        }
        assert!(child.chan_in.is_none());
    }

    #[test]
    fn shell_means_the_login_shell() {
        let p = resolve_program("$SHELL", &allowed(&["$SHELL"])).unwrap();
        assert!(p.is_absolute(), "{}", p.display());
    }
}
