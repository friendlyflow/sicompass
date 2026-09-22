//! The greetd conversation, on a thread of its own.
//!
//! greetd IPC is blocking and PAM can take seconds — a fingerprint prompt, an
//! LDAP round trip, the deliberate delay after a wrong password. The provider
//! runs on the UI thread, which must keep drawing and keep talking to the
//! screen reader throughout, so the conversation lives here instead and the two
//! sides exchange messages.
//!
//! A plain `std::thread` and two channels. No async runtime: this is one
//! request in flight at a time against one socket.
//!
//! # Acknowledging info and error messages
//!
//! greetd's `auth_message` covers four kinds. `secret` and `visible` are
//! questions for the user; `info` and `error` are statements, and the protocol
//! still requires a `post_auth_message_response` with a null response before it
//! will continue. That acknowledgement happens *inside* the worker: it is a
//! protocol obligation with no decision in it, and routing it through the UI
//! thread would add a frame of latency per notice and a state the UI could get
//! stuck in. (This is the bug cosmic-greeter fixed in its PR #373 — without the
//! ACK, greetd waits forever and the greeter looks frozen on the last message.)

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::JoinHandle;

use crate::greetd::{AuthMessageType, ErrorType, GreetdClient, Request, Response};

/// What the UI asks greetd to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GreetdCmd {
    /// Begin authenticating this user.
    Create { username: String },
    /// Answer the outstanding prompt. `None` acknowledges without text.
    Answer { response: Option<String> },
    /// Launch the session. `cmd` is an argv, `env` rides along.
    Start { cmd: Vec<String>, env: Vec<String> },
    /// Abandon the current attempt.
    Cancel,
}

/// What greetd says back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GreetdEvent {
    /// greetd is asking something. `secret` means render and speak it masked.
    Prompt { secret: bool, text: String },
    /// An `info` or `error` message. Already acknowledged by the worker.
    Notice { text: String, is_error: bool },
    /// `success` for `create_session` or `post_auth_message_response`: the user
    /// is through, and the session can be started.
    Authenticated,
    /// `success` for `start_session`. The last event; the greeter now exits so
    /// greetd can hand the display over.
    Started,
    /// `error`. `auth` separates a wrong password (try again) from a broken
    /// session (start over).
    Failed { auth: bool, text: String },
    /// The socket died. Nothing further will arrive.
    Io { text: String },
}

/// A handle onto the conversation thread.
pub struct GreetdWorker {
    tx: Option<Sender<GreetdCmd>>,
    rx: Receiver<GreetdEvent>,
    handle: Option<JoinHandle<()>>,
}

impl GreetdWorker {
    /// Take ownership of a connected client and start talking.
    pub fn spawn(client: GreetdClient) -> Self {
        let (cmd_tx, cmd_rx) = channel::<GreetdCmd>();
        let (evt_tx, evt_rx) = channel::<GreetdEvent>();
        let handle = std::thread::Builder::new()
            .name("loginsicompass-greetd".into())
            .spawn(move || run(client, &cmd_rx, &evt_tx))
            .expect("spawning the greetd worker thread");
        Self {
            tx: Some(cmd_tx),
            rx: evt_rx,
            handle: Some(handle),
        }
    }

    /// Queue a command. A dead worker is not an error here: the `Io` event that
    /// killed it has already been delivered, and the UI reacts to that.
    pub fn send(&self, cmd: GreetdCmd) {
        if let Some(tx) = &self.tx
            && let Err(e) = tx.send(cmd)
        {
            tracing::warn!("greetd worker is gone, dropping command: {e}");
        }
    }

    /// Take one event if there is one. Never blocks.
    pub fn try_recv(&self) -> Option<GreetdEvent> {
        match self.rx.try_recv() {
            Ok(evt) => Some(evt),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }
}

impl Drop for GreetdWorker {
    fn drop(&mut self) {
        // Closing the command channel is what ends the loop.
        self.tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Which request a `success` is answering, since greetd's `success` carries no
/// hint of its own.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// `create_session` or `post_auth_message_response` — the user is in.
    Auth,
    /// `start_session` — the session is running.
    Start,
}

fn run(mut client: GreetdClient, cmds: &Receiver<GreetdCmd>, events: &Sender<GreetdEvent>) {
    // `for` ends when the sender is dropped, i.e. when GreetdWorker is dropped.
    for cmd in cmds {
        let (req, origin) = match cmd {
            GreetdCmd::Create { username } => (Request::CreateSession { username }, Origin::Auth),
            GreetdCmd::Answer { response } => {
                (Request::PostAuthMessageResponse { response }, Origin::Auth)
            }
            GreetdCmd::Start { cmd, env } => (Request::StartSession { cmd, env }, Origin::Start),
            GreetdCmd::Cancel => (Request::CancelSession, Origin::Auth),
        };
        let cancelling = matches!(req, Request::CancelSession);

        if !exchange(&mut client, req, origin, cancelling, events) {
            // The socket is gone; an `Io` event was already sent. Keep draining
            // commands so the UI's `send` calls do not pile up, but say nothing
            // more.
            break;
        }
    }
}

/// Drive one request to a conclusion the UI cares about.
///
/// Returns `false` when the connection failed. `info`/`error` messages are
/// acknowledged here and the loop continues, because greetd may send several
/// before it gets to the question.
fn exchange(
    client: &mut GreetdClient,
    first: Request,
    origin: Origin,
    cancelling: bool,
    events: &Sender<GreetdEvent>,
) -> bool {
    let mut req = first;
    loop {
        let resp = match client.send(&req) {
            Ok(r) => r,
            Err(e) => {
                let _ = events.send(GreetdEvent::Io {
                    text: format!("greetd connection lost: {e}"),
                });
                return false;
            }
        };

        match resp {
            Response::AuthMessage {
                auth_message_type: AuthMessageType::Secret,
                auth_message,
            } => {
                let _ = events.send(GreetdEvent::Prompt {
                    secret: true,
                    text: auth_message,
                });
                return true;
            }
            Response::AuthMessage {
                auth_message_type: AuthMessageType::Visible,
                auth_message,
            } => {
                let _ = events.send(GreetdEvent::Prompt {
                    secret: false,
                    text: auth_message,
                });
                return true;
            }
            Response::AuthMessage {
                auth_message_type: kind @ (AuthMessageType::Info | AuthMessageType::Error),
                auth_message,
            } => {
                let _ = events.send(GreetdEvent::Notice {
                    text: auth_message,
                    is_error: kind == AuthMessageType::Error,
                });
                // The protocol obligation. Keep the original `origin`: a notice
                // in the middle of a start_session is still a start_session.
                req = Request::PostAuthMessageResponse { response: None };
                continue;
            }
            Response::Success => {
                // A successful cancel is bookkeeping, not news.
                if !cancelling {
                    let _ = events.send(match origin {
                        Origin::Auth => GreetdEvent::Authenticated,
                        Origin::Start => GreetdEvent::Started,
                    });
                }
                return true;
            }
            Response::Error {
                error_type,
                description,
            } => {
                // An error answering a cancel is not worth reporting: the
                // session is being thrown away regardless.
                if !cancelling {
                    let _ = events.send(GreetdEvent::Failed {
                        auth: error_type == ErrorType::AuthError,
                        text: description,
                    });
                }
                return true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakegreetd::{Step, bind, serve_script};
    use crate::greetd::AuthMessageType;
    use std::time::{Duration, Instant};

    fn secret(text: &str) -> Response {
        Response::AuthMessage {
            auth_message_type: AuthMessageType::Secret,
            auth_message: text.to_owned(),
        }
    }
    fn info(text: &str) -> Response {
        Response::AuthMessage {
            auth_message_type: AuthMessageType::Info,
            auth_message: text.to_owned(),
        }
    }
    fn auth_err(text: &str) -> Response {
        Response::Error {
            error_type: ErrorType::AuthError,
            description: text.to_owned(),
        }
    }

    /// Drive a script against a real worker over a real socket, collecting
    /// every event until `want` of them have arrived or time runs out.
    fn run_script(script: Vec<Step>, cmds: Vec<GreetdCmd>, want: usize) -> (Vec<GreetdEvent>, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let (listener, path) = bind(dir.path());
        let server = std::thread::spawn(move || serve_script(&listener, script));

        let client = GreetdClient::connect_to(path.to_str().unwrap()).unwrap();
        let worker = GreetdWorker::spawn(client);
        for c in cmds {
            worker.send(c);
        }

        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while events.len() < want && Instant::now() < deadline {
            match worker.try_recv() {
                Some(e) => events.push(e),
                None => std::thread::sleep(Duration::from_millis(2)),
            }
        }
        drop(worker);
        let seen = server.join().unwrap();
        (events, seen)
    }

    #[test]
    fn happy_path_create_answer_start() {
        let (events, seen) = run_script(
            vec![
                Step::new("create_session", secret("Password:")),
                Step::new("hunter2", Response::Success),
                Step::new("start_session", Response::Success),
            ],
            vec![
                GreetdCmd::Create { username: "nico".into() },
                GreetdCmd::Answer { response: Some("hunter2".into()) },
                GreetdCmd::Start {
                    cmd: vec!["/bin/desicompass".into(), "--backend".into(), "tty".into()],
                    env: vec!["XDG_SESSION_TYPE=wayland".into()],
                },
            ],
            3,
        );
        assert_eq!(
            events,
            vec![
                GreetdEvent::Prompt { secret: true, text: "Password:".into() },
                GreetdEvent::Authenticated,
                GreetdEvent::Started,
            ]
        );
        // The argv and the environment reached the wire intact.
        assert!(seen[2].contains(r#""cmd":["/bin/desicompass","--backend","tty"]"#), "{}", seen[2]);
        assert!(seen[2].contains(r#""env":["XDG_SESSION_TYPE=wayland"]"#), "{}", seen[2]);
    }

    #[test]
    fn wrong_password_is_an_auth_failure_not_a_dead_session() {
        let (events, _) = run_script(
            vec![
                Step::new("create_session", secret("Password:")),
                Step::new("wrong", auth_err("authentication error: PERM_DENIED")),
            ],
            vec![
                GreetdCmd::Create { username: "nico".into() },
                GreetdCmd::Answer { response: Some("wrong".into()) },
            ],
            2,
        );
        assert_eq!(events[0], GreetdEvent::Prompt { secret: true, text: "Password:".into() });
        assert!(
            matches!(&events[1], GreetdEvent::Failed { auth: true, text } if text.contains("PERM_DENIED")),
            "got {:?}",
            events[1]
        );
    }

    /// The obligation from the module docs: an `info` message is acknowledged
    /// by the worker with a null response, and the UI sees a Notice rather
    /// than being asked to reply to it.
    #[test]
    fn info_messages_are_acknowledged_inside_the_worker() {
        let (events, seen) = run_script(
            vec![
                Step::new("create_session", info("Insert your fingerprint")),
                Step::new(r#""response":null"#, secret("Password:")),
            ],
            vec![GreetdCmd::Create { username: "nico".into() }],
            2,
        );
        assert_eq!(
            events,
            vec![
                GreetdEvent::Notice { text: "Insert your fingerprint".into(), is_error: false },
                GreetdEvent::Prompt { secret: true, text: "Password:".into() },
            ]
        );
        assert_eq!(seen.len(), 2, "the worker must have sent the ACK itself");
        assert!(seen[1].contains(r#""response":null"#));
    }

    #[test]
    fn an_error_auth_message_is_a_notice_and_is_also_acknowledged() {
        let (events, seen) = run_script(
            vec![
                Step::new("create_session", Response::AuthMessage {
                    auth_message_type: AuthMessageType::Error,
                    auth_message: "Account expires tomorrow".into(),
                }),
                Step::new(r#""response":null"#, secret("Password:")),
            ],
            vec![GreetdCmd::Create { username: "nico".into() }],
            2,
        );
        assert_eq!(
            events[0],
            GreetdEvent::Notice { text: "Account expires tomorrow".into(), is_error: true }
        );
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn two_prompts_password_then_otp() {
        let (events, _) = run_script(
            vec![
                Step::new("create_session", secret("Password:")),
                Step::new("hunter2", Response::AuthMessage {
                    auth_message_type: AuthMessageType::Visible,
                    auth_message: "OTP:".into(),
                }),
                Step::new("123456", Response::Success),
            ],
            vec![
                GreetdCmd::Create { username: "nico".into() },
                GreetdCmd::Answer { response: Some("hunter2".into()) },
                GreetdCmd::Answer { response: Some("123456".into()) },
            ],
            3,
        );
        assert_eq!(
            events,
            vec![
                GreetdEvent::Prompt { secret: true, text: "Password:".into() },
                GreetdEvent::Prompt { secret: false, text: "OTP:".into() },
                GreetdEvent::Authenticated,
            ]
        );
    }

    /// A notice arriving during start_session must not turn the eventual
    /// success into `Authenticated`.
    #[test]
    fn a_notice_during_start_session_still_ends_in_started() {
        let (events, _) = run_script(
            vec![
                Step::new("start_session", info("Last login: yesterday")),
                Step::new(r#""response":null"#, Response::Success),
            ],
            vec![GreetdCmd::Start { cmd: vec!["/bin/sway".into()], env: vec![] }],
            2,
        );
        assert_eq!(events[1], GreetdEvent::Started, "origin must survive the ACK");
    }

    #[test]
    fn a_successful_cancel_is_silent() {
        let (events, seen) = run_script(
            vec![
                Step::new("cancel_session", Response::Success),
                Step::new("create_session", secret("Password:")),
            ],
            vec![
                GreetdCmd::Cancel,
                GreetdCmd::Create { username: "other".into() },
            ],
            1,
        );
        // Only the new prompt: the cancel produced no event of its own.
        assert_eq!(events, vec![GreetdEvent::Prompt { secret: true, text: "Password:".into() }]);
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn a_closed_socket_reports_io_and_does_not_panic() {
        // An empty script: the server accepts, then drops the connection.
        let (events, _) = run_script(
            vec![],
            vec![GreetdCmd::Create { username: "nico".into() }],
            1,
        );
        assert!(
            matches!(&events[0], GreetdEvent::Io { text } if text.contains("connection lost")),
            "got {:?}",
            events.first()
        );
    }
}
