//! The client the compositor was started to run.
//!
//! `--startup-cmd` makes desicompass a kiosk: it exists to host one client,
//! and when that client is finished so is it. Without that, nothing ends the
//! compositor when its client exits, and whoever started desicompass has to
//! kill it instead.
//!
//! That cost about five seconds at every login. greetd only launches the user's
//! session once the greeter's session has finished, so the sequence was:
//! loginsicompass starts the session and exits, desicompass keeps running
//! because nothing told it not to, greetd waits, times out, and kills the
//! process group. The journal showed the gap exactly:
//!
//! ```text
//! 19:04:23.183  loginsicompass: a session was started; the greeter is stepping aside
//! 19:04:28.139  systemd-logind: Session 5 logged out. Waiting for processes to exit.
//! ```
//!
//! The same applies to the session itself (`--startup-cmd sicompass --session`):
//! quitting sicompass should end the session, not leave an empty compositor
//! holding the display until something reaps it.

use std::process::{Child, Command};
use tracing::{error, info};

/// A spawned startup command, or nothing when none was asked for.
pub struct StartupChild {
    child: Option<Child>,
    /// Set once the child has been reaped, so `has_exited` keeps answering
    /// after the `Child` is gone.
    exited: bool,
}

impl StartupChild {
    /// Spawn `cmd` through `sh -c`, pointed at our Wayland socket.
    pub fn spawn(cmd: Option<&str>, socket_name: &str) -> Self {
        let Some(cmd) = cmd else {
            return Self {
                child: None,
                exited: false,
            };
        };
        info!("launching startup command: {cmd}");
        let child = Command::new("/bin/sh")
            .args(["-c", cmd])
            // The socket name is handed to the child explicitly instead of
            // being exported into our own environment: a process-wide
            // `set_var` is `unsafe` under edition 2024 and would repoint every
            // library in *this* process at our socket.
            .env("WAYLAND_DISPLAY", socket_name)
            // sicompass checks this and drops its self-drawn titlebar, which is
            // unreachable here: no pointer exists to click it. Any other client
            // ignores it.
            .env("SICOMPASS_SESSION", "1")
            // Drop the *host* session's DISPLAY. Without this a toolkit that
            // can speak both protocols may quietly pick X11 and render into the
            // desktop we are nested in, instead of into us: the client looks
            // healthy, the compositor stays empty, and nothing anywhere reports
            // an error. SDL3 does exactly this. On a real TTY session there is
            // no DISPLAY to begin with, so this only ever matters while
            // developing nested - which is when it costs the most time.
            .env_remove("DISPLAY")
            .spawn();

        match child {
            Ok(child) => {
                info!("startup command running as pid {}", child.id());
                Self {
                    child: Some(child),
                    exited: false,
                }
            }
            // Never silent: on a bare TTY a startup command that failed to exec
            // is a black screen with no other diagnosis available.
            Err(e) => {
                error!("startup command {cmd:?} failed to start: {e}");
                // Treat a command that never started as exited, so the
                // compositor comes down instead of sitting on a black screen
                // waiting for a client that is never coming.
                Self {
                    child: None,
                    exited: true,
                }
            }
        }
    }

    /// Whether the compositor should stop.
    ///
    /// Always false when there was no startup command: a bare `desicompass` is
    /// a development compositor that outlives whatever is run inside it, and
    /// exiting the moment it has no client would make it useless.
    pub fn has_exited(&mut self) -> bool {
        if self.exited {
            return true;
        }
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                info!("startup command exited ({status}); shutting down");
                self.exited = true;
                self.child = None;
                true
            }
            Ok(None) => false,
            Err(e) => {
                // Cannot tell any more. Staying up would be the worse failure:
                // it is the one that hangs a login.
                error!("cannot check on the startup command: {e}; shutting down");
                self.exited = true;
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_startup_command_never_ends_the_compositor() {
        let mut s = StartupChild::spawn(None, "wayland-9");
        assert!(!s.has_exited());
        assert!(!s.has_exited(), "and stays that way");
    }

    #[test]
    fn a_finished_command_ends_the_compositor() {
        let mut s = StartupChild::spawn(Some("exit 0"), "wayland-9");
        // try_wait is non-blocking, so give the child a moment to be reaped.
        for _ in 0..200 {
            if s.has_exited() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("a command that exited should have ended the compositor");
    }

    /// A client that fails is still a client that is finished. Staying up would
    /// leave a black screen nobody can get out of.
    #[test]
    fn a_failing_command_also_ends_the_compositor() {
        let mut s = StartupChild::spawn(Some("exit 3"), "wayland-9");
        for _ in 0..200 {
            if s.has_exited() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("a command that failed should have ended the compositor");
    }

    #[test]
    fn a_command_that_cannot_start_ends_the_compositor_immediately() {
        // `sh -c` itself always starts, so the failure surfaces as a non-zero
        // exit rather than a spawn error; either way the answer must be yes.
        let mut s = StartupChild::spawn(Some("exec /nonexistent/binary"), "wayland-9");
        for _ in 0..200 {
            if s.has_exited() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("a command that could not run should have ended the compositor");
    }

    #[test]
    fn a_running_command_keeps_the_compositor_up() {
        let mut s = StartupChild::spawn(Some("sleep 30"), "wayland-9");
        assert!(!s.has_exited());
    }
}
