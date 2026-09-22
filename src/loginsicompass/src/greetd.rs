//! greetd IPC client.
//!
//! Implements the framing protocol from `greetd-ipc(7)`:
//!
//! ```text
//! <payload-length> <payload>
//! ```
//!
//! where *"`<payload-length>` is a 32-bit integer in **native byte order**"* and
//! the payload is a UTF-8 JSON string, over a Unix domain socket whose path
//! comes from the `GREETD_SOCK` environment variable.
//!
//! # Native byte order, not big-endian
//!
//! This is the one detail worth stating loudly, because getting it wrong is
//! silent: greetd reads the length with the host's own byte order. The man page
//! spells out the wire bytes for a `create_session` request:
//!
//! ```text
//! 00000000  2c 00 00 00 7b 22 74 79  70 65 22 3a 20 22 63 72  |,...{"type": "cr|
//! ```
//!
//! `0x2c` = 44 is the payload length, little-endian-first on a little-endian
//! host. An earlier revision of this file used `to_be_bytes`/`from_be_bytes`,
//! which on x86_64 sends `00 00 00 2c` — greetd then waits for a 738 MB frame
//! and the connection stalls. The C original this was ported from
//! (`src/loginsicompass-c/ipc.c`) sent the `uint32_t` straight out of memory,
//! i.e. native order, and was correct.
//!
//! Mirrors `ipc.c` + `greetd.c` in `src/loginsicompass-c/`.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;

/// Largest payload we will allocate for a single response.
///
/// greetd's messages are a prompt or an error string; the biggest realistic one
/// is a PAM error description. A length beyond this means the stream has
/// desynchronised (most likely a framing disagreement like the byte-order bug
/// described above), and the right answer is to fail loudly rather than try to
/// allocate it.
const MAX_FRAME_LEN: usize = 1024 * 1024;

// ---------------------------------------------------------------------------
// Protocol types
// ---------------------------------------------------------------------------

/// Requests sent to greetd.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Begin a session for `username`.
    CreateSession { username: String },
    /// Reply to an `auth_message` challenge (password, OTP, etc.).
    PostAuthMessageResponse {
        /// `None` for info/error acknowledgements that need no actual text.
        response: Option<String>,
    },
    /// Launch the session.
    ///
    /// `cmd` is a full argv, not a shell command line: a session's
    /// `Exec=` is split into words before it gets here, or greetd will
    /// `execve` a file whose name is the entire line. `env` is added to the
    /// environment PAM built, and is where `XDG_SESSION_TYPE`,
    /// `XDG_SESSION_DESKTOP` and `XDG_CURRENT_DESKTOP` come from.
    StartSession {
        cmd: Vec<String>,
        env: Vec<String>,
    },
    /// Abort the current session.
    CancelSession,
}

/// Responses received from greetd.
///
/// `Serialize` is not needed to talk to greetd, only to *be* greetd: the
/// scripted server in [`crate::fakegreetd`] writes these on the wire, in tests
/// and when running the greeter with no PAM behind it.
#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Success,
    Error {
        error_type: ErrorType,
        description: String,
    },
    AuthMessage {
        auth_message_type: AuthMessageType,
        auth_message: String,
    },
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    AuthError,
    Error,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum AuthMessageType {
    Visible,
    Secret,
    Info,
    Error,
}

// ---------------------------------------------------------------------------
// GreetdClient
// ---------------------------------------------------------------------------

/// A blocking greetd IPC client over a Unix socket.
pub struct GreetdClient {
    stream: UnixStream,
}

impl GreetdClient {
    /// Connect to the socket specified by `GREETD_SOCK`.
    pub fn connect() -> io::Result<Self> {
        let path = std::env::var("GREETD_SOCK")
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "GREETD_SOCK not set"))?;
        let stream = UnixStream::connect(path)?;
        Ok(Self { stream })
    }

    /// Connect to an explicit socket path (used in tests).
    pub fn connect_to(path: &str) -> io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        Ok(Self { stream })
    }

    /// Send a request and return the parsed response.
    pub fn send(&mut self, req: &Request) -> io::Result<Response> {
        let payload =
            serde_json::to_vec(req).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Write: [u32 native length][payload]
        let len = payload.len() as u32;
        self.stream.write_all(&len.to_ne_bytes())?;
        self.stream.write_all(&payload)?;
        self.stream.flush()?;

        // Read: [u32 native length][response]
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf)?;
        let resp_len = u32::from_ne_bytes(len_buf) as usize;

        if resp_len > MAX_FRAME_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "greetd frame of {resp_len} bytes exceeds the {MAX_FRAME_LEN}-byte cap; \
                     the stream has desynchronised"
                ),
            ));
        }

        let mut resp_buf = vec![0u8; resp_len];
        self.stream.read_exact(&mut resp_buf)?;

        serde_json::from_slice(&resp_buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    // ---- Convenience wrappers (mirror greetd.c helper functions) ----

    pub fn create_session(&mut self, username: &str) -> io::Result<Response> {
        self.send(&Request::CreateSession {
            username: username.to_owned(),
        })
    }

    pub fn post_auth_message_response(&mut self, response: Option<&str>) -> io::Result<Response> {
        self.send(&Request::PostAuthMessageResponse {
            response: response.map(str::to_owned),
        })
    }

    /// Start the session.
    ///
    /// Takes a full argv and an environment, both of which greetd needs: see
    /// [`Request::StartSession`].
    pub fn start_session_argv(
        &mut self,
        cmd: Vec<String>,
        env: Vec<String>,
    ) -> io::Result<Response> {
        self.send(&Request::StartSession { cmd, env })
    }

    pub fn cancel_session(&mut self) -> io::Result<Response> {
        self.send(&Request::CancelSession)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Framing ----

    /// The regression test for the byte-order bug, pinned to the published
    /// wire format rather than to our own encoder.
    ///
    /// `greetd-ipc(7)` gives this hexdump for a `create_session` of `"me"`:
    ///
    /// ```text
    /// 00000000  2c 00 00 00 7b 22 74 79  70 65 22 3a 20 22 63 72  |,...{"type": "cr|
    /// 00000010  65 61 74 65 5f 73 65 73  73 69 6f 6e 22 2c 20 22  |eate_session", "|
    /// 00000020  75 73 65 72 6e 61 6d 65  22 3a 20 22 6d 65 22 7d  |username": "me"}|
    /// ```
    ///
    /// The payload there is spaced (`{"type": "create_session", ...}`, 44
    /// bytes) where `serde_json` emits it compact, so the length differs; what
    /// must match is the *order* of the length bytes. On a little-endian host a
    /// length of 44 is `2c 00 00 00`, and a big-endian encoder would emit
    /// `00 00 00 2c` — which is exactly the bug this pins down.
    #[test]
    fn frame_length_is_native_endian() {
        let payload = br#"{"type": "create_session", "username": "me"}"#;
        assert_eq!(payload.len(), 44, "the man page's example payload is 44 bytes");

        let framed = (payload.len() as u32).to_ne_bytes();

        #[cfg(target_endian = "little")]
        assert_eq!(
            framed,
            [0x2c, 0x00, 0x00, 0x00],
            "little-endian host must frame 44 as the man page's `2c 00 00 00`"
        );

        #[cfg(target_endian = "big")]
        assert_eq!(framed, [0x00, 0x00, 0x00, 0x2c]);

        // And the round trip agrees with itself.
        assert_eq!(u32::from_ne_bytes(framed) as usize, payload.len());
    }

    #[test]
    fn our_own_request_frames_native_endian() {
        let req = Request::CreateSession {
            username: "me".to_owned(),
        };
        let payload = serde_json::to_vec(&req).unwrap();
        let framed = (payload.len() as u32).to_ne_bytes();
        // Compact serialisation: {"type":"create_session","username":"me"}
        assert_eq!(payload.len(), 41);
        #[cfg(target_endian = "little")]
        assert_eq!(framed, [41, 0, 0, 0]);
    }

    #[test]
    fn rejects_absurd_frame_length() {
        use std::os::unix::net::UnixListener;
        use std::thread;

        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("greetd.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();

        let server = thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut len_buf = [0u8; 4];
            conn.read_exact(&mut len_buf).unwrap();
            let len = u32::from_ne_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            conn.read_exact(&mut buf).unwrap();
            // Claim a frame far larger than any real greetd message.
            let absurd = (MAX_FRAME_LEN as u32 + 1).to_ne_bytes();
            let _ = conn.write_all(&absurd);
        });

        let mut client = GreetdClient::connect_to(sock_path.to_str().unwrap()).unwrap();
        let err = client.create_session("bob").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(
            err.to_string().contains("desynchronised"),
            "unexpected error: {err}"
        );
        server.join().unwrap();
    }

    // ---- Request serialisation ----

    #[test]
    fn serialize_create_session() {
        let req = Request::CreateSession {
            username: "alice".to_owned(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"create_session","username":"alice"}"#);
    }

    #[test]
    fn serialize_post_auth_message_response_with_password() {
        let req = Request::PostAuthMessageResponse {
            response: Some("hunter2".to_owned()),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(
            json,
            r#"{"type":"post_auth_message_response","response":"hunter2"}"#
        );
    }

    #[test]
    fn serialize_post_auth_message_response_null() {
        let req = Request::PostAuthMessageResponse { response: None };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(
            json,
            r#"{"type":"post_auth_message_response","response":null}"#
        );
    }

    #[test]
    fn serialize_start_session() {
        let req = Request::StartSession {
            cmd: vec!["sway".to_owned()],
            env: vec![],
        };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"start_session","cmd":["sway"],"env":[]}"#);
    }

    /// A session's `Exec=` is an argv, and its environment rides along. Both
    /// halves are load-bearing: `desicompass.desktop` on a NixOS host is nine
    /// words, and a session with no `XDG_SESSION_TYPE` comes up subtly wrong.
    #[test]
    fn serialize_start_session_with_argv_and_env() {
        let req = Request::StartSession {
            cmd: vec![
                "/nix/store/x-systemd/bin/systemd-cat".to_owned(),
                "--identifier=desicompass".to_owned(),
                "/nix/store/y-desicompass/bin/desicompass".to_owned(),
                "--backend".to_owned(),
                "tty".to_owned(),
            ],
            env: vec![
                "XDG_SESSION_TYPE=wayland".to_owned(),
                "XDG_SESSION_DESKTOP=desicompass".to_owned(),
                "XDG_CURRENT_DESKTOP=Desicompass".to_owned(),
            ],
        };
        let v: serde_json::Value = serde_json::to_value(&req).unwrap();
        assert_eq!(v["type"], "start_session");
        assert_eq!(v["cmd"].as_array().unwrap().len(), 5);
        assert_eq!(v["cmd"][1], "--identifier=desicompass");
        assert_eq!(v["env"].as_array().unwrap().len(), 3);
        assert_eq!(v["env"][0], "XDG_SESSION_TYPE=wayland");
    }

    #[test]
    fn serialize_cancel_session() {
        let req = Request::CancelSession;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"cancel_session"}"#);
    }

    // ---- Response deserialisation ----

    #[test]
    fn deserialize_success() {
        let json = r#"{"type":"success"}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert_eq!(resp, Response::Success);
    }

    #[test]
    fn deserialize_error_auth() {
        let json = r#"{"type":"error","error_type":"auth_error","description":"Wrong password"}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert!(matches!(
            resp,
            Response::Error {
                error_type: ErrorType::AuthError,
                ..
            }
        ));
    }

    #[test]
    fn deserialize_error_generic() {
        let json = r#"{"type":"error","error_type":"error","description":"Internal error"}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert!(matches!(
            resp,
            Response::Error {
                error_type: ErrorType::Error,
                ..
            }
        ));
    }

    #[test]
    fn deserialize_auth_message_secret() {
        let json =
            r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password:"}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert!(
            matches!(resp, Response::AuthMessage { auth_message_type: AuthMessageType::Secret, ref auth_message, .. } if auth_message == "Password:")
        );
    }

    #[test]
    fn deserialize_auth_message_visible() {
        let json = r#"{"type":"auth_message","auth_message_type":"visible","auth_message":"OTP:"}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert!(matches!(
            resp,
            Response::AuthMessage {
                auth_message_type: AuthMessageType::Visible,
                ..
            }
        ));
    }

    #[test]
    fn deserialize_auth_message_info() {
        let json =
            r#"{"type":"auth_message","auth_message_type":"info","auth_message":"Login notice"}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert!(matches!(
            resp,
            Response::AuthMessage {
                auth_message_type: AuthMessageType::Info,
                ..
            }
        ));
    }

    // ---- Wire protocol round-trip via a mock socket pair ----

    fn make_mock_response(resp: &Response) -> Vec<u8> {
        let json = serde_json::to_vec(resp).unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(&(json.len() as u32).to_ne_bytes());
        out.extend_from_slice(&json);
        out
    }

    #[test]
    fn wire_protocol_send_receive() {
        use std::os::unix::net::UnixListener;
        use std::thread;

        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("greetd.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();

        // Server thread: read one request, write an auth_message response
        let server = thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();

            let mut len_buf = [0u8; 4];
            conn.read_exact(&mut len_buf).unwrap();
            let len = u32::from_ne_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            conn.read_exact(&mut buf).unwrap();

            let req: serde_json::Value = serde_json::from_slice(&buf).unwrap();
            assert_eq!(req["type"], "create_session");
            assert_eq!(req["username"], "bob");

            let resp_bytes = make_mock_response(&Response::AuthMessage {
                auth_message_type: AuthMessageType::Secret,
                auth_message: "Password:".to_owned(),
            });
            conn.write_all(&resp_bytes).unwrap();
        });

        let mut client = GreetdClient::connect_to(sock_path.to_str().unwrap()).unwrap();
        let resp = client.create_session("bob").unwrap();
        assert!(matches!(
            resp,
            Response::AuthMessage {
                auth_message_type: AuthMessageType::Secret,
                ..
            }
        ));
        server.join().unwrap();
    }
}
