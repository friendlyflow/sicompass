//! greetd IPC client.
//!
//! Implements the binary framing protocol:
//!   `[u32 length (big-endian)][JSON payload]`
//!
//! over a Unix domain socket whose path comes from the `GREETD_SOCK`
//! environment variable.
//!
//! Mirrors `ipc.c` + `greetd.c` in `src/loginsicompass/`.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;

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
    /// Launch the session with the given command argv.
    StartSession { cmd: Vec<String> },
    /// Abort the current session.
    CancelSession,
}

/// Responses received from greetd.
#[derive(Debug, Deserialize, PartialEq)]
#[cfg_attr(test, derive(Serialize))]
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

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[cfg_attr(test, derive(Serialize))]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    AuthError,
    Error,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[cfg_attr(test, derive(Serialize))]
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
        let payload = serde_json::to_vec(req)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Write: [u32 be length][payload]
        let len = payload.len() as u32;
        self.stream.write_all(&len.to_be_bytes())?;
        self.stream.write_all(&payload)?;
        self.stream.flush()?;

        // Read: [u32 be length][response]
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf)?;
        let resp_len = u32::from_be_bytes(len_buf) as usize;

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

    pub fn start_session(&mut self, command: &str) -> io::Result<Response> {
        self.send(&Request::StartSession {
            cmd: vec![command.to_owned()],
        })
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
        };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"start_session","cmd":["sway"]}"#);
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
        let json =
            r#"{"type":"error","error_type":"auth_error","description":"Wrong password"}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert!(matches!(resp, Response::Error { error_type: ErrorType::AuthError, .. }));
    }

    #[test]
    fn deserialize_error_generic() {
        let json = r#"{"type":"error","error_type":"error","description":"Internal error"}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert!(matches!(resp, Response::Error { error_type: ErrorType::Error, .. }));
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
        let json =
            r#"{"type":"auth_message","auth_message_type":"visible","auth_message":"OTP:"}"#;
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
        out.extend_from_slice(&(json.len() as u32).to_be_bytes());
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

        // Server thread: read one request, write a Success response
        let server = thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();

            // Read request
            let mut len_buf = [0u8; 4];
            conn.read_exact(&mut len_buf).unwrap();
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            conn.read_exact(&mut buf).unwrap();

            // Verify it's a create_session
            let req: serde_json::Value = serde_json::from_slice(&buf).unwrap();
            assert_eq!(req["type"], "create_session");
            assert_eq!(req["username"], "bob");

            // Write success response
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
