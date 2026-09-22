//! Making sure a login screen appears, whatever happens to the first attempt.
//!
//! The Vulkan greeter can fail in ways that are not a `Result`. Beyond the
//! polite ones (no `libvulkan.so.1`, no ICD, no device with a presentable
//! queue), `sicompass-ui` has `.expect`s on swapchain recreation, a graphics
//! driver can take a `SIGSEGV`, and SDL can `abort()`. `catch_unwind` catches
//! the third of those cases that are panics and none of the rest, and a login
//! screen that dies to a segfault is a black screen with no way into the
//! machine.
//!
//! So the process that greetd starts is a supervisor. It re-execs itself as
//! the GPU greeter and, if that dies without having started a session, re-execs
//! itself once more as the software fallback.
//!
//! # Why a pipe rather than the exit status
//!
//! greetd tears the greeter's session down the instant `start_session`
//! succeeds, so a successful child may well be `SIGKILL`ed a millisecond after
//! it has done its job — indistinguishable, by exit status alone, from one that
//! crashed. The child therefore writes one byte to an inherited pipe the moment
//! greetd accepts the session, and the supervisor reads *that*, not the status.
//!
//! # Why re-exec rather than fork
//!
//! Whatever killed the first child may have left driver state behind. A fresh
//! process image is the point.

use std::os::fd::{AsRawFd, OwnedFd};
use std::process::{Command, ExitStatus};

/// Environment variable carrying the done-pipe's write end to the child.
pub const DONE_FD_ENV: &str = "LOGINSICOMPASS_DONE_FD";

/// What the supervisor should do after a child exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// A session was started. Exit 0 and get out of greetd's way.
    Done,
    /// Try the software renderer.
    FallBack,
    /// Nothing left to try. Exit non-zero so the failure is visible.
    GiveUp,
}

/// The whole policy, as a pure function so it can be tested without spawning
/// anything.
///
/// `done_signalled` is the byte on the pipe; `status` is the child's exit
/// status, which is deliberately only consulted for logging.
pub fn decide(_status: Option<ExitStatus>, done_signalled: bool, already_fell_back: bool) -> Decision {
    if done_signalled {
        // The session is running. Whether the child then exited 0 or was
        // killed is not our business.
        return Decision::Done;
    }
    if already_fell_back {
        // The software renderer was the last resort.
        return Decision::GiveUp;
    }
    Decision::FallBack
}

/// Run the greeter under supervision. Returns the process exit code.
pub fn run(argv: &[String]) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("cannot find my own executable: {e}");
            return 1;
        }
    };

    let mut fell_back = false;
    let mut backend = crate::Backend::Gpu;

    loop {
        let (reader, writer) = match make_pipe() {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("cannot create the done pipe: {e}");
                return 1;
            }
        };

        let label = match backend {
            crate::Backend::Gpu => "gpu",
            crate::Backend::Shm => "shm",
        };
        tracing::info!("starting the {label} greeter");

        let mut cmd = Command::new(&exe);
        cmd.args(argv)
            .arg("--render-backend")
            .arg(label)
            .env(DONE_FD_ENV, writer.as_raw_fd().to_string());

        // The child needs the write end; we need it closed here, or reading
        // the pipe would never see EOF.
        let status = spawn_with_fd(&mut cmd, &writer);
        drop(writer);

        let status = match status {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::error!("could not start the {label} greeter: {e}");
                None
            }
        };
        let done = pipe_had_byte(reader);

        match decide(status, done, fell_back) {
            Decision::Done => {
                tracing::info!("a session was started; the greeter is stepping aside");
                return 0;
            }
            Decision::GiveUp => {
                tracing::error!(
                    "the {label} greeter exited without starting a session, and there is \
                     nothing left to fall back to"
                );
                return 1;
            }
            Decision::FallBack => {
                tracing::error!(
                    "the {label} greeter exited ({}) without starting a session; \
                     falling back to the software renderer",
                    describe(status)
                );
                fell_back = true;
                backend = crate::Backend::Shm;
            }
        }
    }
}

fn describe(status: Option<ExitStatus>) -> String {
    match status {
        Some(s) => s.to_string(),
        None => "never started".to_owned(),
    }
}

/// A `CLOEXEC` pipe. The write end has `CLOEXEC` cleared just before exec, in
/// [`spawn_with_fd`], so only the intended child inherits it.
fn make_pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0i32; 2];
    // SAFETY: `fds` is a two-element array, which is what `pipe2` writes.
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: both descriptors are fresh and owned by us.
    unsafe {
        use std::os::fd::FromRawFd;
        Ok((OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])))
    }
}

/// Spawn `cmd` with `keep`'s `CLOEXEC` flag cleared, so the child inherits it.
fn spawn_with_fd(cmd: &mut Command, keep: &OwnedFd) -> std::io::Result<ExitStatus> {
    use std::os::unix::process::CommandExt;
    let raw = keep.as_raw_fd();
    // SAFETY: `pre_exec` runs between fork and exec in the child. `fcntl` is
    // async-signal-safe, and it is the only thing this closure does.
    unsafe {
        cmd.pre_exec(move || {
            let flags = libc::fcntl(raw, libc::F_GETFD);
            if flags == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::fcntl(raw, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.spawn()?.wait()
}

/// Whether anything was written to the pipe. The write end is already closed
/// here, so a read returns immediately.
fn pipe_had_byte(reader: OwnedFd) -> bool {
    use std::io::Read;
    let mut f = std::fs::File::from(reader);
    let mut buf = [0u8; 1];
    matches!(f.read(&mut buf), Ok(n) if n > 0)
}

/// Tell the supervisor a session was started. Called by the child.
///
/// Best-effort: a greeter run without a supervisor has no pipe, and that is
/// the normal case when testing nested.
pub fn signal_done() {
    let Ok(raw) = std::env::var(DONE_FD_ENV) else {
        return;
    };
    let Ok(fd) = raw.parse::<i32>() else {
        return;
    };
    // SAFETY: a single byte from a buffer we own, to a descriptor the
    // supervisor set up for exactly this.
    unsafe {
        let byte = b"1";
        libc::write(fd, byte.as_ptr().cast(), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    fn ok() -> Option<ExitStatus> {
        Some(ExitStatus::from_raw(0))
    }
    fn crashed() -> Option<ExitStatus> {
        // Killed by SIGSEGV.
        Some(ExitStatus::from_raw(libc::SIGSEGV))
    }
    fn failed() -> Option<ExitStatus> {
        Some(ExitStatus::from_raw(1 << 8))
    }

    /// The case the pipe exists for: greetd killed the child a moment after it
    /// did its job, so the status says "signalled" and the pipe says "done".
    #[test]
    fn a_byte_then_a_kill_is_success() {
        assert_eq!(decide(crashed(), true, false), Decision::Done);
    }

    #[test]
    fn a_byte_then_a_clean_exit_is_success() {
        assert_eq!(decide(ok(), true, false), Decision::Done);
    }

    /// Exit 0 without the byte is *not* success: the child came up, drew
    /// something and went away without ever logging anyone in.
    #[test]
    fn a_clean_exit_without_the_byte_falls_back() {
        assert_eq!(decide(ok(), false, false), Decision::FallBack);
    }

    #[test]
    fn a_crash_without_the_byte_falls_back() {
        assert_eq!(decide(crashed(), false, false), Decision::FallBack);
        assert_eq!(decide(failed(), false, false), Decision::FallBack);
    }

    #[test]
    fn a_child_that_never_started_falls_back() {
        assert_eq!(decide(None, false, false), Decision::FallBack);
    }

    /// Only ever fall back once: the software renderer is the last resort, and
    /// looping would spin greetd.
    #[test]
    fn the_fallback_failing_gives_up() {
        assert_eq!(decide(ok(), false, true), Decision::GiveUp);
        assert_eq!(decide(crashed(), false, true), Decision::GiveUp);
        assert_eq!(decide(None, false, true), Decision::GiveUp);
    }

    /// Even on the last attempt, the byte still wins.
    #[test]
    fn the_fallback_succeeding_is_success() {
        assert_eq!(decide(crashed(), true, true), Decision::Done);
    }
}
