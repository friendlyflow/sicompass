//! A scripted greetd, for tests and for running the greeter without PAM.
//!
//! This is the only way to drive "wrong password" ten times in a row without
//! ten real authentication failures, and the only way to exercise the greeter
//! nested inside a desktop session where there is no greetd at all.
//!
//! It speaks the real wire format — native-endian length prefix, JSON payload —
//! so it exercises [`crate::greetd`] rather than standing in for it.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use crate::greetd::Response;

/// One scripted exchange: what the client is expected to ask, and the reply.
pub struct Step {
    /// Substring that must appear in the received JSON. Empty matches anything.
    pub expect: &'static str,
    pub reply: Response,
}

impl Step {
    pub fn new(expect: &'static str, reply: Response) -> Self {
        Self { expect, reply }
    }
}

/// Read one framed request. `None` at a clean end of stream.
pub fn read_frame(conn: &mut UnixStream) -> Option<String> {
    let mut len_buf = [0u8; 4];
    conn.read_exact(&mut len_buf).ok()?;
    let len = u32::from_ne_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    conn.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

/// Write one framed response.
pub fn write_frame(conn: &mut UnixStream, resp: &Response) -> std::io::Result<()> {
    let json = serde_json::to_vec(resp).expect("serialising a greetd response");
    conn.write_all(&(json.len() as u32).to_ne_bytes())?;
    conn.write_all(&json)?;
    conn.flush()
}

/// Serve exactly `script`, in order, on one connection, then close.
///
/// Returns every request line it saw, so a test can assert on the whole
/// conversation rather than one message at a time.
pub fn serve_script(listener: &UnixListener, script: Vec<Step>) -> Vec<String> {
    let mut seen = Vec::new();
    let Ok((mut conn, _)) = listener.accept() else {
        return seen;
    };
    for step in script {
        let Some(line) = read_frame(&mut conn) else {
            break;
        };
        assert!(
            step.expect.is_empty() || line.contains(step.expect),
            "expected a request containing {:?}, got {line}",
            step.expect
        );
        seen.push(line);
        if write_frame(&mut conn, &step.reply).is_err() {
            break;
        }
    }
    seen
}

/// Bind a socket in `dir` and return its path.
pub fn bind(dir: &Path) -> (UnixListener, std::path::PathBuf) {
    let path = dir.join("greetd.sock");
    let listener = UnixListener::bind(&path).expect("binding the fake greetd socket");
    (listener, path)
}

/// A standing fake greetd for interactive use: accepts connection after
/// connection and always asks for a password, accepting `password`.
///
/// Any other password is refused with an `auth_error` whose text matches what
/// PAM would produce, so the greeter's error path is exercised too.
pub fn serve_forever(listener: &UnixListener, password: &str) {
    for conn in listener.incoming() {
        let Ok(mut conn) = conn else { continue };
        if let Err(e) = converse(&mut conn, password) {
            tracing::warn!("fake greetd connection ended: {e}");
        }
    }
}

fn converse(conn: &mut UnixStream, password: &str) -> std::io::Result<()> {
    while let Some(line) = read_frame(conn) {
        let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
        let reply = match v["type"].as_str() {
            Some("create_session") => Response::AuthMessage {
                auth_message_type: crate::greetd::AuthMessageType::Secret,
                auth_message: "Password:".to_owned(),
            },
            Some("post_auth_message_response") => match v["response"].as_str() {
                Some(got) if got == password => Response::Success,
                Some(_) => Response::Error {
                    error_type: crate::greetd::ErrorType::AuthError,
                    description: "authentication error: PERM_DENIED".to_owned(),
                },
                // A null response is an acknowledgement, not an attempt.
                None => Response::Success,
            },
            Some("start_session") => {
                tracing::info!("fake greetd: would start {}", v["cmd"]);
                Response::Success
            }
            Some("cancel_session") => Response::Success,
            _ => Response::Error {
                error_type: crate::greetd::ErrorType::Error,
                description: format!("unknown request: {line}"),
            },
        };
        write_frame(conn, &reply)?;
    }
    Ok(())
}
