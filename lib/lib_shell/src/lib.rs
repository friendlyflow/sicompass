//! PTY-backed interactive shell session.
//!
//! This crate is internal: `sicompass-terminal` uses it to spawn a real shell
//! attached to a pseudo-terminal, push input, and drain output. It has no
//! sicompass-sdk dependency and is not registered as a provider.

use std::io::{Read, Write};
use std::path::PathBuf;
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
        }
    }
}

/// A spawned, PTY-backed shell process.
///
/// `drain_output()` is non-blocking and returns whatever bytes the background
/// reader thread has buffered since the previous call.
pub struct Shell {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    output: Arc<Mutex<Vec<u8>>>,
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

        let mut cmd = CommandBuilder::new(&cfg.program);
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
        let writer = pair.master.take_writer().map_err(io_err)?;

        let output = Arc::new(Mutex::new(Vec::<u8>::new()));
        {
            let output = Arc::clone(&output);
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Ok(mut o) = output.lock() {
                                o.extend_from_slice(&buf[..n]);
                            }
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
        })
    }

    /// Send raw bytes to the child's stdin (PTY master).
    pub fn write_input(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }

    /// Send `s` followed by `\n`.
    pub fn write_line(&mut self, s: &str) -> std::io::Result<()> {
        self.write_input(s.as_bytes())?;
        self.write_input(b"\n")
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

    /// Best-effort kill of the child process.
    pub fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()
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
}
