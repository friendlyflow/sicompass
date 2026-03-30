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
//! Set via `on_text_change` from the settings provider:
//! - `chatHomeserver`   — Matrix homeserver URL
//! - `chatAccessToken`  — Bearer access token
//! - `chatUsername`     — Username (for login)
//! - `chatPassword`     — Password (for login)

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;

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
                "not configured — set homeserver and access token in Settings".to_owned(),
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
        // Millisecond-based transaction ID
        let txn_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
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

    fn do_login(&mut self) -> bool {
        if self.homeserver.is_empty() || self.username.is_empty() || self.password.is_empty() {
            return false;
        }
        let client = match self.client() {
            Ok(c) => c,
            Err(_) => return false,
        };
        let url = self.api("/_matrix/client/v3/login");
        let payload = serde_json::json!({
            "type": "m.login.password",
            "identifier": { "type": "m.id.user", "user": self.username },
            "password": self.password,
        });
        let resp = match client.post(&url).json(&payload).send() {
            Ok(r) => r,
            Err(_) => return false,
        };
        let body: serde_json::Value = match resp.json() {
            Ok(v) => v,
            Err(_) => return false,
        };
        if let Some(token) = body["access_token"].as_str() {
            self.access_token = token.to_owned();
            true
        } else {
            false
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
        let mut result = Vec::new();

        // meta
        let mut meta = FfonElement::new_obj("meta");
        {
            let m = meta.as_obj_mut().unwrap();
            m.push(FfonElement::new_str("/   Search".to_owned()));
            m.push(FfonElement::new_str("F5  Refresh".to_owned()));
            m.push(FfonElement::new_str(":   Commands".to_owned()));
        }
        result.push(meta);

        if let Some(room_name) = self.room_name_from_path().map(|s| s.to_owned()) {
            result.extend(self.fetch_room_messages(&room_name));
        } else {
            result.extend(self.fetch_joined_rooms());
        }

        result
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
            "login" => {
                if !self.do_login() {
                    *error = "Login failed — check homeserver, username, and password in Settings".to_owned();
                }
                None
            }
            "refresh" | "send message" => None, // no UI element needed
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

/// URL-encode a Matrix room ID (encodes `!` → `%21`, `:` → `%3A`).
fn encode_room_id(room_id: &str) -> String {
    room_id
        .replace('!', "%21")
        .replace(':', "%3A")
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

    #[test]
    fn test_fetch_root_no_config_returns_error() {
        let mut p = ChatClientProvider::new();
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("not configured"))
        }));
    }

    #[test]
    fn test_fetch_root_meta_always_first() {
        let (rt, server) = start_mock_server();
        mount(&rt, &server, Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/joined_rooms"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "joined_rooms": [] }),
            )));
        let mut p = provider_for(&server);
        let items = p.fetch();
        assert!(items[0].as_obj().map_or(false, |o| o.key == "meta"));
        drop(rt);
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
                serde_json::json!({ "access_token": "new_token_xyz" }),
            )));
        let mut p = ChatClientProvider::new();
        p.homeserver = server.uri();
        p.username = "user".to_owned();
        p.password = "pass".to_owned();
        let ok = p.do_login();
        assert!(ok);
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
        let ok = p.do_login();
        assert!(!ok);
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
        // No homeserver/credentials set — login should fail and set error
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
}
