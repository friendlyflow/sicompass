//! Matrix chat client provider — Rust port of `lib_chatclient/`.
//!
//! Communicates with a Matrix homeserver via the Client-Server API.
//! Uses `reqwest` blocking for HTTP (no async runtime required).
//!
//! ## FFON tree layout
//!
//! ```text
//! Root "/"
//!   meta           (obj)  — keyboard shortcut hints
//!   room-name      (obj)  — one per joined room, navigable
//!
//! Room "/{display_name}"
//!   meta           (obj)
//!   sender: body   (str)  — one per message (chronological)
//!   <input></input>(str)  — message composition bar
//! ```
//!
//! ## Configuration
//!
//! Set via `on_setting_change` from the settings provider:
//! - `chatHomeserver`   — Matrix homeserver URL
//! - `chatAccessToken`  — Bearer access token
//! - `chatUsername`     — Username (for login/register)
//! - `chatPassword`     — Password (for login/register)

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Monotonic transaction ID counter (mirrors C's `static uint64_t g_txnId`)
// ---------------------------------------------------------------------------

static TXN_COUNTER: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Auth result (mirrors C's ChatAuthResult)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
struct AuthResult {
    success: bool,
    requires_auth: bool,
    access_token: String,
    user_id: String,
    device_id: String,
    session: String,
    next_stage: String,
    error: String,
}

/// Parse a Matrix auth/login/register JSON response body into an AuthResult.
/// Mirrors `parseAuthResponse` in `lib_chatclient/src/chatclient.c`.
fn parse_auth_response(resp: serde_json::Value) -> AuthResult {
    let mut result = AuthResult::default();

    // UIA: both "session" and "flows" present
    if let (Some(session_val), Some(flows_val)) =
        (resp.get("session"), resp.get("flows"))
    {
        if let Some(session) = session_val.as_str() {
            result.requires_auth = true;
            result.session = session.to_owned();

            // Find the first incomplete stage
            let completed_count = resp
                .get("completed")
                .and_then(|c| c.as_array())
                .map(|a| a.len())
                .unwrap_or(0);

            if let Some(stage) = flows_val
                .as_array()
                .and_then(|fs| fs.first())
                .and_then(|f| f.get("stages"))
                .and_then(|s| s.as_array())
                .and_then(|stages| stages.get(completed_count))
                .and_then(|s| s.as_str())
            {
                result.next_stage = stage.to_owned();
            }

            result.error = if result.next_stage.is_empty() {
                "interactive auth required: unknown stage".to_owned()
            } else {
                format!("interactive auth required: {}", result.next_stage)
            };
            return result;
        }
    }

    // Error response
    if let Some(errcode) = resp.get("errcode").and_then(|v| v.as_str()) {
        let errmsg = resp
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        result.error = format!("{errcode}: {errmsg}");
        return result;
    }

    // Success response
    if let Some(token) = resp.get("access_token").and_then(|v| v.as_str()) {
        result.access_token = token.to_owned();
        result.success = true;
    }
    if let Some(uid) = resp.get("user_id").and_then(|v| v.as_str()) {
        result.user_id = uid.to_owned();
    }
    if let Some(did) = resp.get("device_id").and_then(|v| v.as_str()) {
        result.device_id = did.to_owned();
    }
    if !result.success {
        result.error = "no access_token in response".to_owned();
    }
    result
}

// ---------------------------------------------------------------------------
// Room cache entry
// ---------------------------------------------------------------------------

struct RoomEntry {
    display_name: String,
    room_id: String,
}

// ---------------------------------------------------------------------------
// ChatClientProvider
// ---------------------------------------------------------------------------

pub struct ChatClientProvider {
    homeserver: String,
    access_token: String,
    username: String,
    password: String,
    current_path: String,
    room_cache: Vec<RoomEntry>,
    /// Pending UIA session for multi-stage registration
    uia_session: String,
    /// Override for the settings.json path (used in tests to avoid touching
    /// the real user config file).
    config_path_override: Option<std::path::PathBuf>,
}

impl ChatClientProvider {
    pub fn new() -> Self {
        ChatClientProvider {
            homeserver: String::new(),
            access_token: String::new(),
            username: String::new(),
            password: String::new(),
            current_path: "/".to_owned(),
            room_cache: Vec::new(),
            uia_session: String::new(),
            config_path_override: None,
        }
    }

    pub fn with_config_path(mut self, path: std::path::PathBuf) -> Self {
        self.config_path_override = Some(path);
        self
    }

    fn config_path(&self) -> Option<std::path::PathBuf> {
        self.config_path_override.clone()
            .or_else(|| sicompass_sdk::platform::main_config_path())
    }

    fn save_access_token(&self, token: &str) {
        use serde_json::{Map, Value};
        let Some(path) = self.config_path() else { return };
        let mut root: Map<String, Value> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| if let Value::Object(m) = v { Some(m) } else { None })
            .unwrap_or_default();
        let section = root
            .entry("chat client".to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(m) = section {
            m.insert("chatAccessToken".to_owned(), Value::String(token.to_owned()));
        }
        if let Some(parent) = path.parent() {
            sicompass_sdk::platform::make_dirs(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&Value::Object(root)) {
            let _ = std::fs::write(&path, json);
        }
    }

    fn client(&self) -> Result<reqwest::blocking::Client, String> {
        reqwest::blocking::Client::builder()
            .user_agent("sicompass/1.0")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.access_token)
    }

    fn api(&self, path: &str) -> String {
        format!("{}{}", self.homeserver.trim_end_matches('/'), path)
    }

    // ---- Matrix API calls --------------------------------------------------

    fn fetch_joined_rooms(&mut self) -> Vec<FfonElement> {
        if self.homeserver.is_empty() || self.access_token.is_empty() {
            return vec![FfonElement::new_str(
                "configure homeserver URL, username and password in settings, then run login command"
                    .to_owned(),
            )];
        }

        let client = match self.client() {
            Ok(c) => c,
            Err(e) => return vec![FfonElement::new_str(format!("HTTP error: {e}"))],
        };

        let url = self.api("/_matrix/client/v3/joined_rooms");
        let resp = match client.get(&url).header("Authorization", self.auth_header()).send() {
            Ok(r) => r,
            Err(e) => return vec![FfonElement::new_str(format!("Error: {e}"))],
        };

        let body: serde_json::Value = match resp.json() {
            Ok(v) => v,
            Err(e) => return vec![FfonElement::new_str(format!("Parse error: {e}"))],
        };

        let room_ids: Vec<String> = body["joined_rooms"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| s.to_owned()))
            .collect();

        if room_ids.is_empty() {
            return vec![FfonElement::new_str("no rooms found".to_owned())];
        }

        self.room_cache.clear();
        let mut items = Vec::new();

        for room_id in &room_ids {
            let display_name = self
                .fetch_room_display_name(&client, room_id)
                .unwrap_or_else(|| room_id.clone());
            self.room_cache.push(RoomEntry {
                display_name: display_name.clone(),
                room_id: room_id.clone(),
            });
            items.push(FfonElement::new_obj(display_name));
        }

        items
    }

    fn fetch_room_display_name(
        &self,
        client: &reqwest::blocking::Client,
        room_id: &str,
    ) -> Option<String> {
        let encoded_id = encode_room_id(room_id);
        let url = self.api(&format!(
            "/_matrix/client/v3/rooms/{encoded_id}/state/m.room.name"
        ));
        let resp = client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .ok()?;
        let body: serde_json::Value = resp.json().ok()?;
        body["name"].as_str().map(|s| s.to_owned())
    }

    fn fetch_room_messages(&self, room_display_name: &str) -> Vec<FfonElement> {
        let room_id = match self
            .room_cache
            .iter()
            .find(|r| r.display_name == room_display_name)
        {
            Some(r) => r.room_id.clone(),
            None => {
                return vec![FfonElement::new_str("room not found".to_owned())];
            }
        };

        let client = match self.client() {
            Ok(c) => c,
            Err(e) => return vec![FfonElement::new_str(format!("HTTP error: {e}"))],
        };

        let encoded_id = encode_room_id(&room_id);
        let url = self.api(&format!(
            "/_matrix/client/v3/rooms/{encoded_id}/messages?dir=b&limit=50"
        ));
        let resp = match client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
        {
            Ok(r) => r,
            Err(e) => return vec![FfonElement::new_str(format!("Error: {e}"))],
        };

        let body: serde_json::Value = match resp.json() {
            Ok(v) => v,
            Err(e) => return vec![FfonElement::new_str(format!("Parse error: {e}"))],
        };

        let mut messages: Vec<FfonElement> = body["chunk"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|e| e["type"].as_str() == Some("m.room.message"))
            .filter_map(|e| {
                let sender = e["sender"].as_str().unwrap_or("?");
                let body_text = e["content"]["body"].as_str().unwrap_or("(media)");
                Some(FfonElement::new_str(format!("{sender}: {body_text}")))
            })
            .collect();

        // API returns newest-first; reverse to chronological order
        messages.reverse();

        // Append message composition input bar
        messages.push(FfonElement::new_str("<input></input>".to_owned()));

        messages
    }

    fn send_message(&self, room_display_name: &str, body_text: &str) -> bool {
        let room_id = match self
            .room_cache
            .iter()
            .find(|r| r.display_name == room_display_name)
        {
            Some(r) => r.room_id.clone(),
            None => return false,
        };
        let client = match self.client() {
            Ok(c) => c,
            Err(_) => return false,
        };
        let encoded_id = encode_room_id(&room_id);
        // Monotonic transaction ID (mirrors C's static uint64_t g_txnId)
        let txn_id = TXN_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
        let url = self.api(&format!(
            "/_matrix/client/v3/rooms/{encoded_id}/send/m.room.message/m{txn_id}"
        ));
        let payload = serde_json::json!({
            "msgtype": "m.text",
            "body": body_text,
        });
        client
            .put(&url)
            .header("Authorization", self.auth_header())
            .json(&payload)
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    fn do_login(&mut self) -> AuthResult {
        if self.homeserver.is_empty() || self.username.is_empty() || self.password.is_empty() {
            return AuthResult {
                error: "homeserver, username, and password are required".to_owned(),
                ..Default::default()
            };
        }
        let client = match self.client() {
            Ok(c) => c,
            Err(e) => {
                return AuthResult {
                    error: format!("HTTP client error: {e}"),
                    ..Default::default()
                }
            }
        };
        let url = self.api("/_matrix/client/v3/login");
        let payload = serde_json::json!({
            "type": "m.login.password",
            "identifier": { "type": "m.id.user", "user": self.username },
            "password": self.password,
        });
        let resp = match client.post(&url).json(&payload).send() {
            Ok(r) => r,
            Err(e) => {
                return AuthResult {
                    error: format!("request failed: {e}"),
                    ..Default::default()
                }
            }
        };
        match resp.json::<serde_json::Value>() {
            Ok(body) => {
                let result = parse_auth_response(body);
                if result.success {
                    self.access_token = result.access_token.clone();
                }
                result
            }
            Err(_) => AuthResult {
                error: "failed to parse server response".to_owned(),
                ..Default::default()
            },
        }
    }

    fn do_register(&self) -> AuthResult {
        if self.homeserver.is_empty() || self.username.is_empty() || self.password.is_empty() {
            return AuthResult {
                error: "homeserver, username, and password are required".to_owned(),
                ..Default::default()
            };
        }
        let client = match self.client() {
            Ok(c) => c,
            Err(e) => {
                return AuthResult {
                    error: format!("HTTP client error: {e}"),
                    ..Default::default()
                }
            }
        };
        let url = self.api("/_matrix/client/v3/register");
        let payload = serde_json::json!({
            "auth": { "type": "m.login.dummy" },
            "username": self.username,
            "password": self.password,
        });
        let resp = match client.post(&url).json(&payload).send() {
            Ok(r) => r,
            Err(e) => {
                return AuthResult {
                    error: format!("request failed: {e}"),
                    ..Default::default()
                }
            }
        };
        match resp.json::<serde_json::Value>() {
            Ok(body) => parse_auth_response(body),
            Err(_) => AuthResult {
                error: "failed to parse server response".to_owned(),
                ..Default::default()
            },
        }
    }

    fn do_register_complete(&self, session: &str) -> AuthResult {
        if self.homeserver.is_empty()
            || session.is_empty()
            || self.username.is_empty()
            || self.password.is_empty()
        {
            return AuthResult {
                error: "homeserver, session, username, and password are required".to_owned(),
                ..Default::default()
            };
        }
        let client = match self.client() {
            Ok(c) => c,
            Err(e) => {
                return AuthResult {
                    error: format!("HTTP client error: {e}"),
                    ..Default::default()
                }
            }
        };
        let url = self.api("/_matrix/client/v3/register");
        let payload = serde_json::json!({
            "auth": { "session": session },
            "username": self.username,
            "password": self.password,
        });
        let resp = match client.post(&url).json(&payload).send() {
            Ok(r) => r,
            Err(e) => {
                return AuthResult {
                    error: format!("request failed: {e}"),
                    ..Default::default()
                }
            }
        };
        match resp.json::<serde_json::Value>() {
            Ok(body) => parse_auth_response(body),
            Err(_) => AuthResult {
                error: "failed to parse server response".to_owned(),
                ..Default::default()
            },
        }
    }

    fn room_name_from_path(&self) -> Option<&str> {
        let path = self.current_path.trim_start_matches('/');
        if path.is_empty() { None } else { Some(path) }
    }
}

impl Default for ChatClientProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for ChatClientProvider {
    fn name(&self) -> &str { "chatclient" }
    fn display_name(&self) -> &str { "chat client" }

    fn fetch(&mut self) -> Vec<FfonElement> {
        if let Some(room_name) = self.room_name_from_path().map(|s| s.to_owned()) {
            self.fetch_room_messages(&room_name)
        } else {
            self.fetch_joined_rooms()
        }
    }

    fn push_path(&mut self, segment: &str) {
        self.current_path = format!("/{segment}");
    }

    fn pop_path(&mut self) {
        self.current_path = "/".to_owned();
    }

    fn current_path(&self) -> &str { &self.current_path }

    fn set_current_path(&mut self, path: &str) {
        self.current_path = path.to_owned();
    }

    fn commit_edit(&mut self, _old: &str, new_content: &str) -> bool {
        // In a room view: treat new_content as a message to send
        if let Some(room_name) = self.room_name_from_path().map(|s| s.to_owned()) {
            return self.send_message(&room_name, new_content);
        }
        false
    }

    fn commands(&self) -> Vec<String> {
        vec![
            "send message".to_owned(),
            "refresh".to_owned(),
            "login".to_owned(),
            "register".to_owned(),
            "complete registration".to_owned(),
        ]
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        _elem_key: &str,
        _elem_type: i32,
        error: &mut String,
    ) -> Option<FfonElement> {
        match cmd {
            "send message" => Some(FfonElement::new_str("<input></input>".to_owned())),

            "refresh" => None,

            "login" => {
                if self.homeserver.is_empty()
                    || self.username.is_empty()
                    || self.password.is_empty()
                {
                    *error = "set homeserver, username, and password first".to_owned();
                    return None;
                }
                let result = self.do_login();
                if result.success {
                    self.save_access_token(&result.access_token);
                    Some(FfonElement::new_str(format!("logged in as {}", result.user_id)))
                } else {
                    *error = format!("login failed: {}", result.error);
                    None
                }
            }

            "register" => {
                if self.homeserver.is_empty()
                    || self.username.is_empty()
                    || self.password.is_empty()
                {
                    *error = "set homeserver, username, and password first".to_owned();
                    return None;
                }
                let result = self.do_register();
                if result.success {
                    self.access_token = result.access_token.clone();
                    self.save_access_token(&result.access_token);
                    self.uia_session.clear();
                    Some(FfonElement::new_str(format!("registered as {}", result.user_id)))
                } else if result.requires_auth && !result.session.is_empty() {
                    self.uia_session = result.session.clone();
                    #[cfg(not(test))]
                    {
                        let fallback_url = format!(
                            "{}/_matrix/client/v3/auth/{}/fallback/web?session={}",
                            self.homeserver.trim_end_matches('/'),
                            result.next_stage,
                            result.session,
                        );
                        sicompass_sdk::platform::open_with_default(&fallback_url);
                    }
                    Some(FfonElement::new_str(format!(
                        "complete {} in browser, then run complete registration",
                        result.next_stage,
                    )))
                } else {
                    *error = format!("registration failed: {}", result.error);
                    None
                }
            }

            "complete registration" => {
                if self.uia_session.is_empty() {
                    *error = "no registration in progress".to_owned();
                    return None;
                }
                let session = self.uia_session.clone();
                let result = self.do_register_complete(&session);
                if result.success {
                    self.access_token = result.access_token.clone();
                    self.save_access_token(&result.access_token);
                    self.uia_session.clear();
                    Some(FfonElement::new_str(format!("registered as {}", result.user_id)))
                } else if result.requires_auth && !result.session.is_empty() {
                    self.uia_session = result.session.clone();
                    #[cfg(not(test))]
                    {
                        let fallback_url = format!(
                            "{}/_matrix/client/v3/auth/{}/fallback/web?session={}",
                            self.homeserver.trim_end_matches('/'),
                            result.next_stage,
                            result.session,
                        );
                        sicompass_sdk::platform::open_with_default(&fallback_url);
                    }
                    Some(FfonElement::new_str(format!(
                        "complete {} in browser, then run complete registration",
                        result.next_stage,
                    )))
                } else {
                    self.uia_session.clear();
                    *error = format!("registration failed: {}", result.error);
                    None
                }
            }

            _ => None,
        }
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        match key {
            "chatHomeserver" => self.homeserver = value.to_owned(),
            "chatAccessToken" => self.access_token = value.to_owned(),
            "chatUsername" => self.username = value.to_owned(),
            "chatPassword" => self.password = value.to_owned(),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Percent-encode a Matrix room ID per RFC 3986: keep only unreserved characters
/// (A-Z a-z 0-9 - _ . ~) and percent-encode everything else.
/// Mirrors `urlEncodeRoomId` in `lib_chatclient/src/chatclient.c`.
fn encode_room_id(room_id: &str) -> String {
    let mut out = String::with_capacity(room_id.len() * 3);
    for b in room_id.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests — port of tests/lib_chatclient/test_chatclient.c (25 tests)
//
// Strategy: wiremock requires an async runtime to start the server. We create
// a fresh tokio::runtime::Runtime per test, use it only to start the server
// and register mocks, then drop out to sync context before calling any
// blocking reqwest code. This avoids the "cannot drop runtime in async context"
// panic that occurs when reqwest::blocking runs inside a tokio executor.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Start a wiremock server in a one-shot tokio runtime, then return the
    /// server and keep the runtime alive for the duration of the test.
    fn start_mock_server() -> (tokio::runtime::Runtime, MockServer) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let server = rt.block_on(MockServer::start());
        (rt, server)
    }

    /// Mount a mock on the server using the provided runtime.
    fn mount(rt: &tokio::runtime::Runtime, server: &MockServer, mock: Mock) {
        rt.block_on(mock.mount(server));
    }

    fn provider_for(server: &MockServer) -> ChatClientProvider {
        let mut p = ChatClientProvider::new();
        p.homeserver = server.uri();
        p.access_token = "test_token".to_owned();
        p
    }

    // ---- original 25 tests (adapted) ---------------------------------------

    #[test]
    fn test_fetch_root_no_config_returns_error() {
        let mut p = ChatClientProvider::new();
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("configure homeserver"))
        }));
    }

    #[test]
    fn test_fetch_root_empty_rooms_message() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/joined_rooms"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "joined_rooms": [] }),
            )));
        let mut p = provider_for(&server);
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("no rooms found"))
        }));
        drop(rt);
    }

    #[test]
    fn test_fetch_root_rooms_become_objs() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/joined_rooms"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "joined_rooms": ["!abc:example.com"] }),
            )));
        mount(&rt, &server, Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404)));
        let mut p = provider_for(&server);
        let items = p.fetch();
        assert!(items.iter().any(|e| e.is_obj() && e.as_obj().map_or(false, |o| o.key != "meta")));
        drop(rt);
    }

    #[test]
    fn test_fetch_root_room_display_name_from_state() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/joined_rooms"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "joined_rooms": ["!abc:example.com"] }),
            )));
        mount(&rt, &server, Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/rooms/%21abc%3Aexample.com/state/m.room.name"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "name": "General" }),
            )));
        let mut p = provider_for(&server);
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_obj().map_or(false, |o| o.key == "General")
        }));
        drop(rt);
    }

    #[test]
    fn test_room_id_encoded_in_url() {
        assert_eq!(encode_room_id("!abc:example.com"), "%21abc%3Aexample.com");
    }

    #[test]
    fn test_push_path_sets_room() {
        let mut p = ChatClientProvider::new();
        p.push_path("General");
        assert_eq!(p.current_path(), "/General");
    }

    #[test]
    fn test_pop_path_returns_to_root() {
        let mut p = ChatClientProvider::new();
        p.push_path("General");
        p.pop_path();
        assert_eq!(p.current_path(), "/");
    }

    #[test]
    fn test_fetch_room_messages_chronological() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({
                    "chunk": [
                        { "type": "m.room.message", "sender": "@b:x", "content": { "body": "second" } },
                        { "type": "m.room.message", "sender": "@a:x", "content": { "body": "first" } },
                    ]
                })
            )));
        let mut p = provider_for(&server);
        p.room_cache.push(RoomEntry {
            display_name: "General".to_owned(),
            room_id: "!abc:x".to_owned(),
        });
        p.push_path("General");
        let items = p.fetch();
        let msg_items: Vec<_> = items.iter()
            .filter_map(|e| e.as_str())
            .filter(|s| s.contains(": "))
            .collect();
        assert_eq!(msg_items.len(), 2);
        assert!(msg_items[0].contains("first"));
        assert!(msg_items[1].contains("second"));
        drop(rt);
    }

    #[test]
    fn test_fetch_room_ends_with_input_bar() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "chunk": [] }),
            )));
        let mut p = provider_for(&server);
        p.room_cache.push(RoomEntry {
            display_name: "General".to_owned(),
            room_id: "!abc:x".to_owned(),
        });
        p.push_path("General");
        let items = p.fetch();
        let last = items.last().unwrap();
        assert!(last.as_str().map_or(false, |s| s.contains("<input>")));
        drop(rt);
    }

    #[test]
    fn test_fetch_room_not_in_cache_returns_error() {
        let mut p = ChatClientProvider::new();
        p.homeserver = "http://127.0.0.1:1".to_owned(); // unreachable
        p.access_token = "tok".to_owned();
        p.push_path("NoSuchRoom");
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("room not found"))
        }));
    }

    #[test]
    fn test_login_success_sets_access_token() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/login"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "access_token": "new_token_xyz", "user_id": "@user:server" }),
            )));
        let mut p = ChatClientProvider::new();
        p.homeserver = server.uri();
        p.username = "user".to_owned();
        p.password = "pass".to_owned();
        let result = p.do_login();
        assert!(result.success);
        assert_eq!(p.access_token, "new_token_xyz");
        drop(rt);
    }

    #[test]
    fn test_login_failure_returns_false() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/login"))
            .respond_with(ResponseTemplate::new(403).set_body_json(
                serde_json::json!({ "errcode": "M_FORBIDDEN", "error": "Bad credentials" }),
            )));
        let mut p = ChatClientProvider::new();
        p.homeserver = server.uri();
        p.username = "user".to_owned();
        p.password = "wrongpass".to_owned();
        let result = p.do_login();
        assert!(!result.success);
        assert!(result.error.contains("M_FORBIDDEN"));
        drop(rt);
    }

    #[test]
    fn test_on_setting_change_homeserver() {
        let mut p = ChatClientProvider::new();
        p.on_setting_change("chatHomeserver", "https://matrix.org");
        assert_eq!(p.homeserver, "https://matrix.org");
    }

    #[test]
    fn test_on_setting_change_access_token() {
        let mut p = ChatClientProvider::new();
        p.on_setting_change("chatAccessToken", "syt_abc");
        assert_eq!(p.access_token, "syt_abc");
    }

    #[test]
    fn test_on_setting_change_username() {
        let mut p = ChatClientProvider::new();
        p.on_setting_change("chatUsername", "alice");
        assert_eq!(p.username, "alice");
    }

    #[test]
    fn test_on_setting_change_password() {
        let mut p = ChatClientProvider::new();
        p.on_setting_change("chatPassword", "secret");
        assert_eq!(p.password, "secret");
    }

    #[test]
    fn test_commands_list() {
        let p = ChatClientProvider::new();
        let cmds = p.commands();
        assert!(cmds.contains(&"login".to_owned()));
        assert!(cmds.contains(&"refresh".to_owned()));
        assert!(cmds.contains(&"send message".to_owned()));
        assert!(cmds.contains(&"register".to_owned()));
        assert!(cmds.contains(&"complete registration".to_owned()));
    }

    #[test]
    fn test_name_and_display_name() {
        let p = ChatClientProvider::new();
        assert_eq!(p.name(), "chatclient");
        assert_eq!(p.display_name(), "chat client");
    }

    #[test]
    fn test_send_message_success() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "event_id": "$abc" }),
            )));
        let mut p = provider_for(&server);
        p.room_cache.push(RoomEntry {
            display_name: "General".to_owned(),
            room_id: "!abc:x".to_owned(),
        });
        let ok = p.send_message("General", "Hello!");
        assert!(ok);
        drop(rt);
    }

    #[test]
    fn test_send_message_unknown_room_fails() {
        let mut p = ChatClientProvider::new();
        let ok = p.send_message("NoSuchRoom", "Hello!");
        assert!(!ok);
    }

    #[test]
    fn test_commit_edit_in_room_sends_message() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "event_id": "$xyz" }),
            )));
        let mut p = provider_for(&server);
        p.room_cache.push(RoomEntry {
            display_name: "General".to_owned(),
            room_id: "!abc:x".to_owned(),
        });
        p.push_path("General");
        let ok = p.commit_edit("", "test message");
        assert!(ok);
        drop(rt);
    }

    #[test]
    fn test_commit_edit_at_root_returns_false() {
        let mut p = ChatClientProvider::new();
        let ok = p.commit_edit("", "anything");
        assert!(!ok);
    }

    #[test]
    fn test_handle_command_login_without_credentials() {
        let mut p = ChatClientProvider::new();
        let mut err = String::new();
        let result = p.handle_command("login", "", 0, &mut err);
        assert!(result.is_none());
        assert!(!err.is_empty(), "error message should be set on failed login");
    }

    #[test]
    fn test_handle_command_refresh_returns_none() {
        let mut p = ChatClientProvider::new();
        let mut err = String::new();
        let result = p.handle_command("refresh", "", 0, &mut err);
        assert!(result.is_none());
    }

    // ---- new tests for ported features ------------------------------------

    #[test]
    fn test_handle_command_send_message_returns_input() {
        let mut p = ChatClientProvider::new();
        let mut err = String::new();
        let result = p.handle_command("send message", "", 0, &mut err);
        assert!(result.is_some(), "send message command should return an input element");
        assert!(
            result.unwrap().as_str().map_or(false, |s| s.contains("<input>")),
            "returned element should contain <input>",
        );
    }

    #[test]
    fn test_login_success_returns_user_id_element() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/login"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({
                    "access_token": "syt_xyz",
                    "user_id": "@alice:server.org",
                })
            )));
        let dir = tempfile::tempdir().unwrap();
        let mut p = ChatClientProvider::new()
            .with_config_path(dir.path().join("settings.json"));
        p.homeserver = server.uri();
        p.username = "alice".to_owned();
        p.password = "pass".to_owned();
        let mut err = String::new();
        let elem = p.handle_command("login", "", 0, &mut err);
        assert!(err.is_empty(), "no error on success, got: {err}");
        assert!(elem.is_some(), "login success should return an element");
        assert!(
            elem.unwrap().as_str().map_or(false, |s| s.contains("@alice:server.org")),
            "success element should contain userId",
        );
        drop(rt);
    }

    #[test]
    fn test_register_without_credentials() {
        let mut p = ChatClientProvider::new();
        p.homeserver = "http://localhost".to_owned();
        // no username/password set
        let mut err = String::new();
        let result = p.handle_command("register", "", 0, &mut err);
        assert!(result.is_none());
        assert!(!err.is_empty(), "should set an error when credentials missing");
    }

    #[test]
    fn test_register_success_sets_access_token() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/register"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({
                    "access_token": "reg_token_abc",
                    "user_id": "@newuser:server.org",
                })
            )));
        let dir = tempfile::tempdir().unwrap();
        let mut p = ChatClientProvider::new()
            .with_config_path(dir.path().join("settings.json"));
        p.homeserver = server.uri();
        p.username = "newuser".to_owned();
        p.password = "pass123".to_owned();
        let mut err = String::new();
        let elem = p.handle_command("register", "", 0, &mut err);
        assert!(err.is_empty(), "no error on success, got: {err}");
        assert!(elem.is_some(), "register success should return an element");
        assert!(
            elem.unwrap().as_str().map_or(false, |s| s.contains("@newuser:server.org")),
            "success element should contain userId",
        );
        assert_eq!(p.access_token, "reg_token_abc");
        assert!(p.uia_session.is_empty(), "uia_session should be cleared on success");
        drop(rt);
    }

    #[test]
    fn test_register_requires_uia_stores_session() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/register"))
            .respond_with(ResponseTemplate::new(401).set_body_json(
                serde_json::json!({
                    "session": "uia_session_xyz",
                    "flows": [{ "stages": ["m.login.recaptcha"] }],
                    "completed": [],
                })
            )));
        let mut p = ChatClientProvider::new();
        p.homeserver = server.uri();
        p.username = "alice".to_owned();
        p.password = "pass".to_owned();
        let mut err = String::new();
        let elem = p.handle_command("register", "", 0, &mut err);
        assert!(err.is_empty(), "UIA requires-auth is not an error, got: {err}");
        assert!(elem.is_some(), "should return a status element for UIA");
        assert!(
            elem.unwrap().as_str().map_or(false, |s| s.contains("m.login.recaptcha")),
            "element should mention the next stage",
        );
        assert_eq!(p.uia_session, "uia_session_xyz");
        drop(rt);
    }

    #[test]
    fn test_complete_registration_no_session() {
        let mut p = ChatClientProvider::new();
        p.homeserver = "http://localhost".to_owned();
        p.username = "alice".to_owned();
        p.password = "pass".to_owned();
        let mut err = String::new();
        let result = p.handle_command("complete registration", "", 0, &mut err);
        assert!(result.is_none());
        assert!(err.contains("no registration in progress"), "got: {err}");
    }

    #[test]
    fn test_complete_registration_success() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/register"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({
                    "access_token": "final_token",
                    "user_id": "@alice:server.org",
                })
            )));
        let dir = tempfile::tempdir().unwrap();
        let mut p = ChatClientProvider::new()
            .with_config_path(dir.path().join("settings.json"));
        p.homeserver = server.uri();
        p.username = "alice".to_owned();
        p.password = "pass".to_owned();
        p.uia_session = "pending_session_abc".to_owned();
        let mut err = String::new();
        let elem = p.handle_command("complete registration", "", 0, &mut err);
        assert!(err.is_empty(), "no error on success, got: {err}");
        assert!(elem.is_some(), "should return success element");
        assert!(
            elem.unwrap().as_str().map_or(false, |s| s.contains("@alice:server.org")),
            "element should contain userId",
        );
        assert_eq!(p.access_token, "final_token");
        assert!(p.uia_session.is_empty(), "uia_session should be cleared on success");
        drop(rt);
    }

    #[test]
    fn test_encode_room_id_at_sign() {
        let encoded = encode_room_id("@user:server.org");
        assert_eq!(encoded, "%40user%3Aserver.org");
    }

    #[test]
    fn test_encode_room_id_slash() {
        assert_eq!(encode_room_id("a/b"), "a%2Fb");
    }

    #[test]
    fn test_encode_room_id_multibyte() {
        // UTF-8 bytes: "é" = 0xC3 0xA9
        let encoded = encode_room_id("caf\u{e9}");
        assert_eq!(encoded, "caf%C3%A9");
    }

    #[test]
    fn test_encode_room_id_unreserved_pass_through() {
        // Unreserved chars: A-Z a-z 0-9 - _ . ~
        assert_eq!(encode_room_id("abc-123_test.room~"), "abc-123_test.room~");
    }

    #[test]
    fn test_txn_id_is_monotonic() {
        // Capture the starting counter value then send two messages and verify
        // the second URL has a higher suffix than the first.
        let (rt, server) = start_mock_server();
        // Match any PUT — we just need to capture both requests
        mount(&rt, &server, Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "event_id": "$e" }),
            )));

        let mut p = provider_for(&server);
        p.room_cache.push(RoomEntry {
            display_name: "Room".to_owned(),
            room_id: "!x:x".to_owned(),
        });

        let before = TXN_COUNTER.load(Ordering::Relaxed);
        let ok1 = p.send_message("Room", "first");
        let mid = TXN_COUNTER.load(Ordering::Relaxed);
        let ok2 = p.send_message("Room", "second");
        let after = TXN_COUNTER.load(Ordering::Relaxed);

        assert!(ok1, "first send should succeed");
        assert!(ok2, "second send should succeed");
        assert!(mid > before, "counter should advance after first send");
        assert!(after > mid, "counter should advance after second send");
        drop(rt);
    }

    #[test]
    fn test_parse_auth_response_uia() {
        let resp = serde_json::json!({
            "session": "sess123",
            "flows": [{ "stages": ["m.login.recaptcha", "m.login.email"] }],
            "completed": [],
        });
        let result = parse_auth_response(resp);
        assert!(result.requires_auth);
        assert_eq!(result.session, "sess123");
        assert_eq!(result.next_stage, "m.login.recaptcha");
        assert!(!result.success);
    }

    #[test]
    fn test_parse_auth_response_uia_with_completed() {
        let resp = serde_json::json!({
            "session": "sess456",
            "flows": [{ "stages": ["m.login.recaptcha", "m.login.email"] }],
            "completed": ["m.login.recaptcha"],
        });
        let result = parse_auth_response(resp);
        assert!(result.requires_auth);
        assert_eq!(result.next_stage, "m.login.email");
    }

    #[test]
    fn test_parse_auth_response_error() {
        let resp = serde_json::json!({
            "errcode": "M_FORBIDDEN",
            "error": "Invalid password",
        });
        let result = parse_auth_response(resp);
        assert!(!result.success);
        assert!(result.error.contains("M_FORBIDDEN"));
        assert!(result.error.contains("Invalid password"));
    }

    #[test]
    fn test_parse_auth_response_success() {
        let resp = serde_json::json!({
            "access_token": "syt_abc",
            "user_id": "@user:server",
            "device_id": "ABCDEF",
        });
        let result = parse_auth_response(resp);
        assert!(result.success);
        assert_eq!(result.access_token, "syt_abc");
        assert_eq!(result.user_id, "@user:server");
        assert_eq!(result.device_id, "ABCDEF");
    }
}
