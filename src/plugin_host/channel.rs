//! A running plugin process and the channel to it.
//!
//! The process is started with stdin and stdout piped and talks
//! [`sicompass_sdk::plugin_ipc`] over them; its stderr is its log. One reader
//! thread routes what it sends: replies to the call waiting for them, and its
//! own calls to a thread each, because one of them (a sign-in) can wait for
//! minutes while the plugin keeps answering the app.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sicompass_sdk::plugin_ipc::{
    HostRequest, HostResponse, Message, Request, Response, protocol_compatible, read_message,
    write_message,
};

/// How long a plugin has to say hello once started. A call has no deadline:
/// the app waits as long as the plugin takes, as it always did for a plugin
/// that was a program or a library. A plugin keeps slow work on a thread of
/// its own, so the app does not freeze on it.
const HELLO_DEADLINE: Duration = Duration::from_secs(10);

/// How long a plugin let go of has to exit on its own before it is killed.
const EXIT_GRACE: Duration = Duration::from_secs(3);

/// How many of the plugin's last log lines are kept, to say why it stopped.
const LOG_TAIL: usize = 20;

/// What the reader hands the waiting caller.
enum Incoming {
    Hello {
        protocol: String,
    },
    Reply {
        id: u64,
        response: Response,
        moved_to: Option<String>,
    },
    /// The channel ended: the plugin exited, crashed or sent garbage.
    Closed(String),
}

/// Why a call got no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum CallError {
    /// No answer within the deadline set with [`Channel::set_deadline`]; the
    /// plugin has been stopped.
    TimedOut,
    /// The plugin is gone, with what its log said last.
    Closed(String),
}

/// Answers the plugin's own calls. Runs on a thread per call.
pub type Answer = Arc<dyn Fn(HostRequest) -> HostResponse + Send + Sync>;

/// A plugin process.
pub struct Channel {
    child: Arc<Mutex<Child>>,
    to_plugin: Arc<Mutex<Option<ChildStdin>>>,
    incoming: Mutex<Receiver<Incoming>>,
    next_id: AtomicU64,
    /// Set when the channel ended or the plugin was let go of. Ends a
    /// sign-in the plugin is waiting on.
    closed: Arc<AtomicBool>,
    log_tail: Arc<Mutex<Vec<String>>>,
    /// How long one call may take; `None`, the default, waits forever.
    deadline: Mutex<Option<Duration>>,
    pid: u32,
    name: String,
    /// The protocol its hello named, which says what it can be asked.
    protocol: std::sync::OnceLock<String>,
}

impl Channel {
    /// Start `exe` in `dir` and wait for its hello. `name` labels its log, and
    /// `closed` is set once the plugin is gone or let go of.
    pub fn spawn(
        exe: &Path,
        dir: &Path,
        name: &str,
        env: &[(std::ffi::OsString, std::ffi::OsString)],
        answer: Answer,
        closed: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let mut cmd = Command::new(exe);
        cmd.current_dir(dir)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child =
            spawn_retrying(&mut cmd).map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
        let pid = child.id();
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let mut stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;

        let log_tail = Arc::new(Mutex::new(Vec::new()));
        let to_plugin = Arc::new(Mutex::new(Some(stdin)));

        // The plugin's log.
        {
            let name = name.to_owned();
            let tail = log_tail.clone();
            std::thread::Builder::new()
                .name(format!("plugin-log-{name}"))
                .spawn(move || {
                    for line in BufReader::new(stderr).lines() {
                        let Ok(line) = line else { break };
                        tracing::info!(target: "plugin", plugin = %name, "{line}");
                        if let Ok(mut t) = tail.lock() {
                            if t.len() == LOG_TAIL {
                                t.remove(0);
                            }
                            t.push(line);
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
        }

        // What it sends.
        let (tx, rx) = mpsc::channel();
        {
            let name = name.to_owned();
            let closed = closed.clone();
            let to_plugin = to_plugin.clone();
            std::thread::Builder::new()
                .name(format!("plugin-channel-{name}"))
                .spawn(move || {
                    let why = loop {
                        match read_message(&mut stdout) {
                            Ok(Some(Message::Hello { protocol, .. })) => {
                                let _ = tx.send(Incoming::Hello { protocol });
                            }
                            Ok(Some(Message::Reply {
                                id,
                                response,
                                moved_to,
                            })) => {
                                let _ = tx.send(Incoming::Reply {
                                    id,
                                    response,
                                    moved_to,
                                });
                            }
                            Ok(Some(Message::HostCall { id, request })) => {
                                let answer = answer.clone();
                                let to_plugin = to_plugin.clone();
                                let spawned = std::thread::Builder::new()
                                    .name(format!("plugin-call-{name}"))
                                    .spawn(move || {
                                        let response = answer(request);
                                        send(&to_plugin, &Message::HostReply { id, response });
                                    });
                                if let Err(e) = spawned {
                                    tracing::warn!(target: "plugin", plugin = %name, "cannot answer a call: {e}");
                                }
                            }
                            Ok(Some(other)) => {
                                break format!("it sent a message only the app sends: {other:?}");
                            }
                            Ok(None) => break "it exited".to_owned(),
                            Err(e) => break format!("the channel broke: {e}"),
                        }
                    };
                    closed.store(true, Ordering::Release);
                    let _ = tx.send(Incoming::Closed(why));
                })
                .map_err(|e| e.to_string())?;
        }

        let me = Channel {
            child: Arc::new(Mutex::new(child)),
            to_plugin,
            incoming: Mutex::new(rx),
            next_id: AtomicU64::new(1),
            closed,
            log_tail,
            deadline: Mutex::new(None),
            pid,
            name: name.to_owned(),
            protocol: std::sync::OnceLock::new(),
        };
        me.await_hello()?;
        Ok(me)
    }

    fn await_hello(&self) -> Result<(), String> {
        let rx = self.incoming.lock().map_err(|e| e.to_string())?;
        match rx.recv_timeout(HELLO_DEADLINE) {
            Ok(Incoming::Hello { protocol }) if protocol_compatible(&protocol) => {
                let _ = self.protocol.set(protocol);
                Ok(())
            }
            Ok(Incoming::Hello { protocol }) => Err(format!(
                "it speaks plugin protocol {protocol}, and this sicompass speaks {}; \
                 update it from the Store",
                sicompass_sdk::plugin_abi::PROTOCOL_VERSION
            )),
            Ok(Incoming::Closed(why)) => Err(self.stopped(&why)),
            Ok(Incoming::Reply { .. }) => Err("it answered before it was asked".to_owned()),
            Err(_) => Err("it did not start talking to sicompass".to_owned()),
        }
    }

    /// Call the plugin and wait for its answer, and where it moved to.
    pub fn call(&self, request: Request) -> Result<(Response, Option<String>), CallError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(CallError::Closed(self.stopped("it exited")));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if !send(&self.to_plugin, &Message::Call { id, request }) {
            return Err(CallError::Closed(self.stopped("it stopped reading")));
        }
        let rx = self
            .incoming
            .lock()
            .map_err(|e| CallError::Closed(e.to_string()))?;
        let deadline = self
            .deadline
            .lock()
            .ok()
            .and_then(|d| *d)
            .map(|d| Instant::now() + d);
        loop {
            let next = match deadline {
                Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match next {
                Ok(Incoming::Reply {
                    id: got,
                    response,
                    moved_to,
                }) if got == id => return Ok((response, moved_to)),
                // An answer to an earlier call that gave up waiting cannot
                // arrive: giving up stops the plugin.
                Ok(Incoming::Reply { .. } | Incoming::Hello { .. }) => {}
                Ok(Incoming::Closed(why)) => return Err(CallError::Closed(self.stopped(&why))),
                Err(RecvTimeoutError::Timeout) => {
                    self.kill();
                    return Err(CallError::TimedOut);
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(CallError::Closed(self.stopped("it exited")));
                }
            }
        }
    }

    /// Why the plugin stopped, with the last thing its log said.
    fn stopped(&self, why: &str) -> String {
        let last = self
            .log_tail
            .lock()
            .ok()
            .and_then(|t| t.iter().rev().find(|l| !l.trim().is_empty()).cloned());
        match last {
            Some(line) => format!("{why} ({line})"),
            None => why.to_owned(),
        }
    }

    /// Change how long one call may take, `None` for forever.
    pub fn set_deadline(&self, deadline: Option<Duration>) {
        if let Ok(mut d) = self.deadline.lock() {
            *d = deadline;
        }
    }

    /// Stop the plugin now.
    pub fn kill(&self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// The plugin process's own id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Whether the plugin understands what protocol `since` added. An older
    /// one cannot read such a request and would exit on it.
    pub fn speaks(&self, since: &str) -> bool {
        self.protocol
            .get()
            .is_some_and(|p| sicompass_sdk::plugin_abi::protocol_has(p, since))
    }
}

/// Start `cmd`, retrying while its executable is busy.
///
/// A file just written (the Store installing a plugin, a test copying one) is
/// briefly held open for writing by any process another thread forks in the
/// meantime, until that process execs and drops it. Executing the file then
/// fails with ETXTBSY. It clears within milliseconds, so wait and try again.
fn spawn_retrying(cmd: &mut Command) -> std::io::Result<Child> {
    let mut wait = Duration::from_millis(5);
    for _ in 0..8 {
        match cmd.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(wait);
                wait *= 2;
            }
            other => return other,
        }
    }
    cmd.spawn()
}

/// Write one message to the plugin. `false` when it no longer reads.
fn send(to_plugin: &Mutex<Option<ChildStdin>>, m: &Message) -> bool {
    let Ok(mut guard) = to_plugin.lock() else {
        return false;
    };
    match guard.as_mut() {
        Some(w) => write_message(w, m).and_then(|()| w.flush()).is_ok(),
        None => false,
    }
}

impl Drop for Channel {
    /// Let the plugin go: closing its stdin is its signal to clean up and exit.
    /// One that has not exited after [`EXIT_GRACE`] is killed. Either way it is
    /// waited for on a thread of its own, so closing a tab never waits on it.
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(mut w) = self.to_plugin.lock() {
            *w = None;
        }
        let child = self.child.clone();
        let name = self.name.clone();
        let _ = std::thread::Builder::new()
            .name(format!("plugin-exit-{name}"))
            .spawn(move || {
                let start = Instant::now();
                loop {
                    let Ok(mut c) = child.lock() else { return };
                    match c.try_wait() {
                        Ok(Some(_)) | Err(_) => return,
                        Ok(None) if start.elapsed() >= EXIT_GRACE => {
                            tracing::warn!(target: "plugin", plugin = %name, "did not exit when let go of; killed");
                            let _ = c.kill();
                            let _ = c.wait();
                            return;
                        }
                        Ok(None) => {}
                    }
                    drop(c);
                    std::thread::sleep(Duration::from_millis(25));
                }
            });
    }
}
