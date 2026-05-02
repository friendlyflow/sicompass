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
//!   [invite] name  (obj)  — pending invite, navigating in asks to accept/reject
//!   [space] name   (obj)  — space (folder), navigating in shows children
//!   [dm] @user     (obj)  — direct message room
//!   room-name      (obj)  — joined group room
//!
//! Room "/{display_key}"
//!   [members]      (obj)  — navigate in to see member list
//!   sender: body   (str)  — one per message (chronological)
//!   <input></input>(str)  — message composition bar
//!
//! Members "/{display_key}/[members]"
//!   @user:server   (obj)  — one per member; elem_key usable in kick/ban/unban
//!
//! Space "/[space] name"
//!   room-name      (obj)  — child room
//! ```
//!
//! ## Configuration
//!
//! Set via `on_setting_change` from the settings provider:
//! - `chatHomeserver`   — Matrix homeserver URL
//! - `chatAccessToken`  — Bearer access token
//! - `chatUsername`     — Username (for login/register)
//! - `chatPassword`     — Password (for login/register)
//! - `chatUserId`       — Our own MXID (set on login/register, used for m.direct)

mod sync;

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

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

    if let (Some(session_val), Some(flows_val)) = (resp.get("session"), resp.get("flows")) {
        if let Some(session) = session_val.as_str() {
            result.requires_auth = true;
            result.session = session.to_owned();

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

    if let Some(errcode) = resp.get("errcode").and_then(|v| v.as_str()) {
        let errmsg = resp.get("error").and_then(|v| v.as_str()).unwrap_or("");
        result.error = format!("{errcode}: {errmsg}");
        return result;
    }

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
// PendingAction — for commands that need a free-text input follow-up
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum PendingAction {
    JoinRoom,
    CreateRoom { encrypted: bool, is_space: bool },
    CreateDm,
    KickMember { room_id: String, member_id: String },
    BanMember { room_id: String, member_id: String },
    UnbanMember { room_id: String, member_id: String },
}

// ---------------------------------------------------------------------------
// ChatClientProvider
// ---------------------------------------------------------------------------

pub struct ChatClientProvider {
    homeserver: String,
    access_token: String,
    username: String,
    password: String,
    /// Our own Matrix user ID — set from login/register responses.
    user_id: String,
    current_path: String,
    sync_cache: Arc<Mutex<sync::SyncCache>>,
    needs_refresh_flag: Arc<AtomicBool>,
    sync_controller: sync::SyncController,
    /// Serialises settings-file writes between the main thread and the sync thread.
    file_lock: Arc<Mutex<()>>,
    uia_session: String,
    config_path_override: Option<std::path::PathBuf>,
    sync_disabled: bool,
    email: String,
    register_3pid_sid: String,
    register_3pid_client_secret: String,
    register_error: Option<String>,
    /// Error from the most recent pending-action or command execution.
    last_error: Option<String>,
    register_mode: bool,
    /// Pending input-driven action (set by a command, consumed by commit_edit).
    pending_action: Option<PendingAction>,
    /// Cached public rooms list from the last "browse public rooms" command.
    public_rooms_cache: Vec<String>,
}

impl ChatClientProvider {
    pub fn new() -> Self {
        let cache = Arc::new(Mutex::new(sync::SyncCache::default()));
        let flag = Arc::new(AtomicBool::new(false));
        let ctrl = sync::SyncController::new(Arc::clone(&cache), Arc::clone(&flag));
        ChatClientProvider {
            homeserver: String::new(),
            access_token: String::new(),
            username: String::new(),
            password: String::new(),
            user_id: String::new(),
            current_path: "/".to_owned(),
            sync_cache: cache,
            needs_refresh_flag: flag,
            sync_controller: ctrl,
            file_lock: Arc::new(Mutex::new(())),
            uia_session: String::new(),
            config_path_override: None,
            sync_disabled: false,
            email: String::new(),
            register_3pid_sid: String::new(),
            register_3pid_client_secret: String::new(),
            register_error: None,
            last_error: None,
            register_mode: true,
            pending_action: None,
            public_rooms_cache: Vec::new(),
        }
    }

    pub fn with_config_path(mut self, path: std::path::PathBuf) -> Self {
        self.config_path_override = Some(path);
        self
    }

    pub fn with_sync_disabled(mut self) -> Self {
        self.sync_disabled = true;
        self
    }

    fn config_path(&self) -> Option<std::path::PathBuf> {
        self.config_path_override
            .clone()
            .or_else(|| sicompass_sdk::platform::main_config_path())
    }

    fn save_setting(&self, key: &str, value: &str) {
        use serde_json::{Map, Value};
        let Some(path) = self.config_path() else { return };
        let _guard = self.file_lock.lock().unwrap();
        let mut root: Map<String, Value> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| if let Value::Object(m) = v { Some(m) } else { None })
            .unwrap_or_default();
        let section = root
            .entry("chat client".to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(m) = section {
            m.insert(key.to_owned(), Value::String(value.to_owned()));
        }
        if let Some(parent) = path.parent() {
            sicompass_sdk::platform::make_dirs(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&Value::Object(root)) {
            let _ = std::fs::write(&path, json);
        }
    }

    fn save_access_token(&self, token: &str) {
        self.save_setting("chatAccessToken", token);
    }

    fn save_user_id(&self, user_id: &str) {
        self.save_setting("chatUserId", user_id);
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

    // ---- Path helpers -------------------------------------------------------

    /// Return the path split into segments, e.g. "/General/[members]" → ["General", "[members]"].
    fn current_path_segments(&self) -> Vec<String> {
        let path = self.current_path.trim_start_matches('/');
        if path.is_empty() {
            vec![]
        } else {
            path.split('/').map(|s| s.to_owned()).collect()
        }
    }

    fn room_name_from_path(&self) -> Option<&str> {
        let path = self.current_path.trim_start_matches('/');
        if path.is_empty() { None } else { Some(path) }
    }

    // ---- Sync lifecycle -----------------------------------------------------

    fn maybe_start_sync(&mut self) {
        if self.sync_disabled || self.homeserver.is_empty() || self.access_token.is_empty() {
            return;
        }
        self.sync_controller.start(
            self.homeserver.clone(),
            self.access_token.clone(),
            self.config_path(),
            self.user_id.clone(),
            Arc::clone(&self.file_lock),
        );
    }

    // ---- Fetch helpers ------------------------------------------------------

    fn fetch_joined_rooms(&self) -> Vec<FfonElement> {
        if self.register_mode && self.access_token.is_empty() {
            return self.build_register_form();
        }
        if self.homeserver.is_empty() || self.access_token.is_empty() {
            return vec![FfonElement::new_str(
                "configure homeserver URL, username and password in settings, then run login command"
                    .to_owned(),
            )];
        }
        let cache = self.sync_cache.lock().unwrap();
        let has_rooms = !cache.rooms.is_empty();
        let has_invites = !cache.invites.is_empty();

        if !has_rooms && !has_invites {
            return if cache.next_batch.is_empty() {
                vec![FfonElement::new_str("Loading\u{2026}".to_owned())]
            } else {
                vec![FfonElement::new_str("no rooms found".to_owned())]
            };
        }

        let mut items: Vec<FfonElement> = Vec::new();

        // Invites first.
        let mut invites: Vec<&sync::InviteState> = cache.invites.values().collect();
        invites.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        for inv in invites {
            items.push(FfonElement::new_obj(format!("[invite] {}", inv.display_name)));
        }

        // Spaces.
        let mut spaces: Vec<&sync::RoomState> =
            cache.rooms.values().filter(|r| r.kind == sync::RoomKind::Space).collect();
        spaces.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        for space in spaces {
            items.push(FfonElement::new_obj(format!("[space] {}", space.display_name)));
        }

        // DM rooms.
        let mut dm_keys: Vec<String> = cache
            .rooms
            .values()
            .filter(|r| r.is_dm && r.kind == sync::RoomKind::Room)
            .filter_map(|r| {
                cache.direct_room_to_user.get(&r.room_id).map(|u| format!("[dm] {u}"))
            })
            .collect();
        dm_keys.sort();
        for key in dm_keys {
            items.push(FfonElement::new_obj(key));
        }

        // Regular rooms.
        let mut rooms: Vec<&sync::RoomState> = cache
            .rooms
            .values()
            .filter(|r| !r.is_dm && r.kind == sync::RoomKind::Room)
            .collect();
        rooms.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        for room in rooms {
            items.push(FfonElement::new_obj(room.display_name.clone()));
        }

        items
    }

    fn fetch_room_messages(&self, display_key: &str) -> Vec<FfonElement> {
        let cache = self.sync_cache.lock().unwrap();
        let room_id = cache
            .display_to_id
            .get(display_key)
            .or_else(|| cache.invite_display_to_id.get(display_key))
            .cloned();
        let Some(room_id) = room_id else {
            return vec![FfonElement::new_str("room not found".to_owned())];
        };
        let Some(room) = cache.rooms.get(&room_id) else {
            return vec![FfonElement::new_str("room not found".to_owned())];
        };
        let mut items = vec![FfonElement::new_obj("[members]".to_owned())];
        items.extend(
            room.timeline
                .iter()
                .map(|ev| FfonElement::new_str(format!("{}: {}", ev.sender, ev.body))),
        );
        items.push(FfonElement::new_str("<input></input>".to_owned()));
        items
    }

    fn fetch_space_children(&self, space_key: &str) -> Vec<FfonElement> {
        let cache = self.sync_cache.lock().unwrap();
        let Some(space_id) = cache.display_to_id.get(space_key).cloned() else {
            return vec![FfonElement::new_str("space not found".to_owned())];
        };
        let Some(space) = cache.rooms.get(&space_id) else {
            return vec![FfonElement::new_str("space not found".to_owned())];
        };
        if space.space_children.is_empty() {
            return vec![FfonElement::new_str("no rooms in this space".to_owned())];
        }
        space
            .space_children
            .iter()
            .map(|child_id| {
                let label = cache
                    .rooms
                    .get(child_id)
                    .map(|r| r.display_name.clone())
                    .unwrap_or_else(|| child_id.clone());
                FfonElement::new_obj(label)
            })
            .collect()
    }

    fn fetch_members(&self, room_key: &str) -> Vec<FfonElement> {
        let cache = self.sync_cache.lock().unwrap();
        let Some(room_id) = cache.display_to_id.get(room_key).cloned() else {
            return vec![FfonElement::new_str("room not found".to_owned())];
        };
        let Some(room) = cache.rooms.get(&room_id) else {
            return vec![FfonElement::new_str("room not found".to_owned())];
        };
        if room.members.is_empty() {
            return vec![FfonElement::new_str("no member data yet".to_owned())];
        }
        // Sort by membership (join first), then by user_id.
        let mut members: Vec<&sync::Member> = room.members.values().collect();
        members.sort_by(|a, b| {
            let order = |m: &str| match m {
                "join" => 0,
                "invite" => 1,
                "leave" => 2,
                _ => 3,
            };
            order(&a.membership).cmp(&order(&b.membership)).then(a.user_id.cmp(&b.user_id))
        });
        members
            .into_iter()
            .map(|m| {
                let label = m
                    .display_name
                    .as_deref()
                    .map(|n| format!("{} ({})", m.user_id, n))
                    .unwrap_or_else(|| m.user_id.clone());
                FfonElement::new_obj(label)
            })
            .collect()
    }

    fn fetch_public_rooms_list(&self) -> Vec<FfonElement> {
        self.public_rooms_cache
            .iter()
            .map(|s| FfonElement::new_str(s.clone()))
            .collect()
    }

    // ---- Matrix API calls ---------------------------------------------------

    fn build_register_form(&self) -> Vec<FfonElement> {
        let homeserver =
            if self.homeserver.is_empty() { "https://matrix.org" } else { &self.homeserver };
        let mut items = Vec::new();
        if let Some(err) = &self.register_error {
            items.push(FfonElement::new_str(format!("Error: {err}")));
        }
        items.push(FfonElement::new_str(format!(
            "Homeserver: <input>{homeserver}</input>"
        )));
        items.push(FfonElement::new_str(format!(
            "Username: <input>{}</input>",
            self.username
        )));
        items.push(FfonElement::new_str(format!("Email: <input>{}</input>", self.email)));
        items.push(FfonElement::new_str(format!(
            "Password: <input>{}</input>",
            self.password
        )));
        items.push(FfonElement::new_str(
            "<button>register</button>Register account".to_owned(),
        ));
        if !self.uia_session.is_empty() {
            items.push(FfonElement::new_str(
                "<button>complete-registration</button>Complete registration after email verify"
                    .to_owned(),
            ));
        }
        items
    }

    fn request_email_token(&mut self) -> Result<(), String> {
        let client = self.client().map_err(|e| format!("HTTP client error: {e}"))?;
        let secret = format!(
            "{:x}{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos(),
            std::process::id(),
        );
        let homeserver = if self.homeserver.is_empty() {
            "https://matrix.org".to_owned()
        } else {
            self.homeserver.clone()
        };
        let url = format!(
            "{}/_matrix/client/v3/register/email/requestToken",
            homeserver.trim_end_matches('/'),
        );
        let payload = serde_json::json!({
            "client_secret": secret,
            "email": self.email,
            "send_attempt": 1,
        });
        let resp = client
            .post(&url)
            .json(&payload)
            .send()
            .map_err(|e| format!("request failed: {e}"))?;
        let body: serde_json::Value = resp.json().map_err(|e| format!("parse error: {e}"))?;
        let sid = body.get("sid").and_then(|v| v.as_str()).ok_or_else(|| {
            let errcode = body.get("errcode").and_then(|v| v.as_str()).unwrap_or("");
            let errmsg = body.get("error").and_then(|v| v.as_str()).unwrap_or("");
            format!("{errcode}: {errmsg}")
        })?;
        self.register_3pid_sid = sid.to_owned();
        self.register_3pid_client_secret = secret;
        Ok(())
    }

    fn handle_register_result(&mut self, result: AuthResult) {
        if result.success {
            self.access_token = result.access_token.clone();
            self.user_id = result.user_id.clone();
            self.uia_session.clear();
            self.register_error = None;
            self.register_mode = false;
            self.save_access_token(&result.access_token);
            self.save_user_id(&result.user_id);
            self.save_setting("chatHomeserver", &self.homeserver.clone());
            self.save_setting("chatUsername", &self.username.clone());
            self.save_setting("chatEmail", &self.email.clone());
            self.maybe_start_sync();
        } else if result.requires_auth && !result.session.is_empty() {
            self.uia_session = result.session.clone();
            let stage_hint = if result.next_stage.is_empty() {
                "authentication required in browser".to_owned()
            } else {
                format!(
                    "complete {} in browser, then click Complete registration",
                    result.next_stage
                )
            };
            let prior_err = self.register_error.take();
            self.register_error = Some(match prior_err {
                Some(e) => format!("{stage_hint} (note: {e})"),
                None => stage_hint,
            });
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
        } else {
            self.uia_session.clear();
            self.register_error = Some(format!("registration failed: {}", result.error));
        }
    }

    fn fetch_room_id_for_path_segment(&self, segment: &str) -> Option<String> {
        let cache = self.sync_cache.lock().unwrap();
        cache.display_to_id.get(segment).cloned()
    }

    fn send_message(&self, room_display_key: &str, body_text: &str) -> bool {
        let room_id = match self
            .sync_cache
            .lock()
            .unwrap()
            .display_to_id
            .get(room_display_key)
            .cloned()
        {
            Some(id) => id,
            None => return false,
        };
        let client = match self.client() {
            Ok(c) => c,
            Err(_) => return false,
        };
        let encoded_id = encode_room_id(&room_id);
        let txn_id = TXN_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
        let url =
            self.api(&format!("/_matrix/client/v3/rooms/{encoded_id}/send/m.room.message/m{txn_id}"));
        let payload = serde_json::json!({ "msgtype": "m.text", "body": body_text });
        client
            .put(&url)
            .header("Authorization", self.auth_header())
            .json(&payload)
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    fn do_join(&self, alias_or_id: &str) -> Result<String, String> {
        let client = self.client().map_err(|e| e.to_string())?;
        let encoded = encode_room_id(alias_or_id.trim());
        let url = self.api(&format!("/_matrix/client/v3/join/{encoded}"));
        let resp = client
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({}))
            .send()
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
            let room_id = body
                .get("room_id")
                .and_then(|v| v.as_str())
                .unwrap_or(alias_or_id)
                .to_owned();
            Ok(room_id)
        } else {
            let body: serde_json::Value = resp.json().unwrap_or_default();
            let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("join failed");
            Err(err.to_owned())
        }
    }

    fn do_leave(&self, room_id: &str) -> Result<(), String> {
        let client = self.client().map_err(|e| e.to_string())?;
        let encoded = encode_room_id(room_id);
        let url = self.api(&format!("/_matrix/client/v3/rooms/{encoded}/leave"));
        let resp = client
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({}))
            .send()
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let body: serde_json::Value = resp.json().unwrap_or_default();
            let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("leave failed");
            Err(err.to_owned())
        }
    }

    fn do_forget(&self, room_id: &str) -> Result<(), String> {
        let client = self.client().map_err(|e| e.to_string())?;
        let encoded = encode_room_id(room_id);
        let url = self.api(&format!("/_matrix/client/v3/rooms/{encoded}/forget"));
        let resp = client
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({}))
            .send()
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err("forget failed".to_owned())
        }
    }

    fn do_create_room(
        &self,
        name: &str,
        encrypted: bool,
        is_space: bool,
    ) -> Result<String, String> {
        let client = self.client().map_err(|e| e.to_string())?;
        let url = self.api("/_matrix/client/v3/createRoom");
        let mut body = serde_json::json!({
            "name": name,
            "preset": "private_chat",
        });
        if is_space {
            body["creation_content"] = serde_json::json!({ "type": "m.space" });
        }
        if encrypted {
            body["initial_state"] = serde_json::json!([{
                "type": "m.room.encryption",
                "state_key": "",
                "content": { "algorithm": "m.megolm.v1.aes-sha2" }
            }]);
        }
        let resp = client
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            let b: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
            let room_id =
                b.get("room_id").and_then(|v| v.as_str()).unwrap_or("").to_owned();
            Ok(room_id)
        } else {
            let b: serde_json::Value = resp.json().unwrap_or_default();
            let err = b.get("error").and_then(|v| v.as_str()).unwrap_or("createRoom failed");
            Err(err.to_owned())
        }
    }

    fn do_create_dm(&self, target_mxid: &str) -> Result<String, String> {
        let client = self.client().map_err(|e| e.to_string())?;
        let url = self.api("/_matrix/client/v3/createRoom");
        let body = serde_json::json!({
            "is_direct": true,
            "invite": [target_mxid.trim()],
            "preset": "trusted_private_chat",
        });
        let resp = client
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            let b: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
            let room_id =
                b.get("room_id").and_then(|v| v.as_str()).unwrap_or("").to_owned();
            Ok(room_id)
        } else {
            let b: serde_json::Value = resp.json().unwrap_or_default();
            let err =
                b.get("error").and_then(|v| v.as_str()).unwrap_or("createRoom (DM) failed");
            Err(err.to_owned())
        }
    }

    fn do_kick_member(&self, room_id: &str, member_id: &str, reason: &str) -> Result<(), String> {
        let client = self.client().map_err(|e| e.to_string())?;
        let encoded = encode_room_id(room_id);
        let url = self.api(&format!("/_matrix/client/v3/rooms/{encoded}/kick"));
        let mut body = serde_json::json!({ "user_id": member_id });
        if !reason.is_empty() {
            body["reason"] = serde_json::Value::String(reason.to_owned());
        }
        let resp = client
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let b: serde_json::Value = resp.json().unwrap_or_default();
            Err(b.get("error").and_then(|v| v.as_str()).unwrap_or("kick failed").to_owned())
        }
    }

    fn do_ban_member(&self, room_id: &str, member_id: &str, reason: &str) -> Result<(), String> {
        let client = self.client().map_err(|e| e.to_string())?;
        let encoded = encode_room_id(room_id);
        let url = self.api(&format!("/_matrix/client/v3/rooms/{encoded}/ban"));
        let mut body = serde_json::json!({ "user_id": member_id });
        if !reason.is_empty() {
            body["reason"] = serde_json::Value::String(reason.to_owned());
        }
        let resp = client
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let b: serde_json::Value = resp.json().unwrap_or_default();
            Err(b.get("error").and_then(|v| v.as_str()).unwrap_or("ban failed").to_owned())
        }
    }

    fn do_unban_member(&self, room_id: &str, member_id: &str) -> Result<(), String> {
        let client = self.client().map_err(|e| e.to_string())?;
        let encoded = encode_room_id(room_id);
        let url = self.api(&format!("/_matrix/client/v3/rooms/{encoded}/unban"));
        let body = serde_json::json!({ "user_id": member_id });
        let resp = client
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let b: serde_json::Value = resp.json().unwrap_or_default();
            Err(b.get("error").and_then(|v| v.as_str()).unwrap_or("unban failed").to_owned())
        }
    }

    fn do_public_rooms(&self) -> Result<Vec<String>, String> {
        let client = self.client().map_err(|e| e.to_string())?;
        let url = self.api("/_matrix/client/v3/publicRooms");
        let body = serde_json::json!({ "limit": 50 });
        let resp = client
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        let b: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let chunk = b.get("chunk").and_then(|c| c.as_array()).cloned().unwrap_or_default();
        let lines: Vec<String> = chunk
            .iter()
            .map(|room| {
                let alias = room
                    .get("canonical_alias")
                    .and_then(|v| v.as_str())
                    .or_else(|| room.get("room_id").and_then(|v| v.as_str()))
                    .unwrap_or("?");
                let name = room.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let members =
                    room.get("num_joined_members").and_then(|v| v.as_u64()).unwrap_or(0);
                format!("{alias} — {name} ({members} members)")
            })
            .collect();
        Ok(lines)
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
            Err(e) => return AuthResult { error: format!("HTTP client error: {e}"), ..Default::default() },
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
                return AuthResult { error: format!("request failed: {e}"), ..Default::default() }
            }
        };
        match resp.json::<serde_json::Value>() {
            Ok(body) => {
                let result = parse_auth_response(body);
                if result.success {
                    self.access_token = result.access_token.clone();
                    self.user_id = result.user_id.clone();
                }
                result
            }
            Err(_) => {
                AuthResult { error: "failed to parse server response".to_owned(), ..Default::default() }
            }
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
            Err(e) => return AuthResult { error: format!("HTTP client error: {e}"), ..Default::default() },
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
                return AuthResult { error: format!("request failed: {e}"), ..Default::default() }
            }
        };
        match resp.json::<serde_json::Value>() {
            Ok(body) => parse_auth_response(body),
            Err(_) => {
                AuthResult { error: "failed to parse server response".to_owned(), ..Default::default() }
            }
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
            Err(e) => return AuthResult { error: format!("HTTP client error: {e}"), ..Default::default() },
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
                return AuthResult { error: format!("request failed: {e}"), ..Default::default() }
            }
        };
        match resp.json::<serde_json::Value>() {
            Ok(body) => parse_auth_response(body),
            Err(_) => {
                AuthResult { error: "failed to parse server response".to_owned(), ..Default::default() }
            }
        }
    }

    fn on_button_press(&mut self, function_name: &str) {
        match function_name {
            "register" => {
                if !self.email.is_empty() {
                    if let Err(e) = self.request_email_token() {
                        self.register_error = Some(format!("email token request failed: {e}"));
                    }
                }
                if self.homeserver.is_empty() {
                    self.homeserver = "https://matrix.org".to_owned();
                }
                let result = self.do_register();
                self.handle_register_result(result);
            }
            "complete-registration" => {
                if self.uia_session.is_empty() {
                    return;
                }
                let session = self.uia_session.clone();
                let result = self.do_register_complete(&session);
                self.handle_register_result(result);
            }
            _ => {}
        }
    }

    fn take_error(&mut self) -> Option<String> {
        self.register_error.take().or_else(|| self.last_error.take())
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        match key {
            "chatHomeserver" => {
                self.homeserver = value.to_owned();
                self.sync_controller.stop();
                *self.sync_cache.lock().unwrap() = sync::SyncCache::default();
                self.maybe_start_sync();
            }
            "chatAccessToken" => {
                self.access_token = value.to_owned();
                if !value.is_empty() {
                    self.register_mode = false;
                }
                self.sync_controller.stop();
                *self.sync_cache.lock().unwrap() = sync::SyncCache::default();
                self.maybe_start_sync();
            }
            "chatUsername" => self.username = value.to_owned(),
            "chatPassword" => self.password = value.to_owned(),
            "chatEmail" => self.email = value.to_owned(),
            "chatUserId" => self.user_id = value.to_owned(),
            _ => {}
        }
    }

    /// Execute a pending input-driven action using the typed text.
    fn execute_pending_action(
        &mut self,
        action: PendingAction,
        text: &str,
        error: &mut String,
    ) -> bool {
        match action {
            PendingAction::JoinRoom => match self.do_join(text) {
                Ok(_) => true,
                Err(e) => {
                    *error = format!("join failed: {e}");
                    false
                }
            },
            PendingAction::CreateRoom { encrypted, is_space } => {
                match self.do_create_room(text, encrypted, is_space) {
                    Ok(_) => true,
                    Err(e) => {
                        *error = format!("create room failed: {e}");
                        false
                    }
                }
            }
            PendingAction::CreateDm => match self.do_create_dm(text) {
                Ok(_) => true,
                Err(e) => {
                    *error = format!("create DM failed: {e}");
                    false
                }
            },
            PendingAction::KickMember { room_id, member_id } => {
                match self.do_kick_member(&room_id, &member_id, text) {
                    Ok(()) => true,
                    Err(e) => {
                        *error = format!("kick failed: {e}");
                        false
                    }
                }
            }
            PendingAction::BanMember { room_id, member_id } => {
                match self.do_ban_member(&room_id, &member_id, text) {
                    Ok(()) => true,
                    Err(e) => {
                        *error = format!("ban failed: {e}");
                        false
                    }
                }
            }
            PendingAction::UnbanMember { room_id, member_id } => {
                match self.do_unban_member(&room_id, &member_id) {
                    Ok(()) => true,
                    Err(e) => {
                        *error = format!("unban failed: {e}");
                        false
                    }
                }
            }
        }
    }
}

impl Default for ChatClientProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for ChatClientProvider {
    fn name(&self) -> &str {
        "chatclient"
    }
    fn display_name(&self) -> &str {
        "chat client"
    }
    fn refresh_on_navigate(&self) -> bool {
        true
    }
    fn stable_root_key(&self) -> bool {
        true
    }

    fn init(&mut self) {
        use serde_json::Value;
        let Some(path) = self.config_path() else {
            self.maybe_start_sync();
            return;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            self.maybe_start_sync();
            return;
        };
        let Ok(root) = serde_json::from_str::<Value>(&content) else {
            self.maybe_start_sync();
            return;
        };
        let Some(section) = root.get("chat client").and_then(|v| v.as_object()) else {
            self.maybe_start_sync();
            return;
        };

        macro_rules! load_str {
            ($key:literal, $field:expr) => {
                if let Some(v) = section.get($key).and_then(|v| v.as_str()) {
                    if !v.is_empty() {
                        $field = v.to_owned();
                    }
                }
            };
        }

        load_str!("chatHomeserver", self.homeserver);
        load_str!("chatAccessToken", self.access_token);
        load_str!("chatUsername", self.username);
        load_str!("chatPassword", self.password);
        load_str!("chatEmail", self.email);
        load_str!("chatUserId", self.user_id);
        self.register_mode = self.access_token.is_empty();

        if let Some(nb) = section.get("chatSyncNextBatch").and_then(|v| v.as_str()) {
            if !nb.is_empty() {
                self.sync_cache.lock().unwrap().next_batch = nb.to_owned();
            }
        }

        self.maybe_start_sync();
    }

    fn cleanup(&mut self) {
        self.sync_controller.stop();
        *self.sync_cache.lock().unwrap() = sync::SyncCache::default();
    }

    fn needs_refresh(&self) -> bool {
        self.needs_refresh_flag.load(Ordering::Relaxed)
    }

    fn clear_needs_refresh(&mut self) {
        self.needs_refresh_flag.store(false, Ordering::Relaxed);
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        let segs = self.current_path_segments();
        match segs.len() {
            0 => self.fetch_joined_rooms(),
            1 => {
                let seg = segs.into_iter().next().unwrap();
                if seg.starts_with("[space] ") {
                    self.fetch_space_children(&seg)
                } else if seg == "[public]" {
                    self.fetch_public_rooms_list()
                } else {
                    self.fetch_room_messages(&seg)
                }
            }
            2 => {
                let mut it = segs.into_iter();
                let first = it.next().unwrap();
                let second = it.next().unwrap();
                if second == "[members]" {
                    self.fetch_members(&first)
                } else if first.starts_with("[space] ") {
                    // Child room inside a space.
                    self.fetch_room_messages(&second)
                } else {
                    vec![FfonElement::new_str("navigation error".to_owned())]
                }
            }
            _ => vec![FfonElement::new_str("navigation error".to_owned())],
        }
    }

    fn push_path(&mut self, segment: &str) {
        if self.current_path == "/" {
            self.current_path = format!("/{segment}");
        } else {
            self.current_path = format!("{}/{segment}", self.current_path);
        }
    }

    fn pop_path(&mut self) {
        match self.current_path.rfind('/') {
            Some(0) | None => self.current_path = "/".to_owned(),
            Some(pos) => self.current_path = self.current_path[..pos].to_owned(),
        }
    }

    fn current_path(&self) -> &str {
        &self.current_path
    }

    fn set_current_path(&mut self, path: &str) {
        self.current_path = path.to_owned();
    }

    fn commit_edit(&mut self, _old: &str, new_content: &str) -> bool {
        // Pending action (from a command that requested text input) takes priority.
        if let Some(action) = self.pending_action.take() {
            let mut err = String::new();
            let ok = self.execute_pending_action(action, new_content, &mut err);
            return ok;
        }

        let segs = self.current_path_segments();
        let Some(first) = segs.first().cloned() else { return false };

        // Register form field edit (path is the field label, value is the typed text).
        if self.register_mode && self.access_token.is_empty() {
            return match first.as_str() {
                "Homeserver" => {
                    self.homeserver = new_content.to_owned();
                    self.save_setting("chatHomeserver", new_content);
                    true
                }
                "Username" => {
                    self.username = new_content.to_owned();
                    self.save_setting("chatUsername", new_content);
                    true
                }
                "Email" => {
                    self.email = new_content.to_owned();
                    self.save_setting("chatEmail", new_content);
                    true
                }
                "Password" => {
                    self.password = new_content.to_owned();
                    self.save_setting("chatPassword", new_content);
                    true
                }
                _ => false,
            };
        }

        // Inside a room at depth 1: send message if the segment resolves to a room.
        if segs.len() == 1 {
            let exists = self.sync_cache.lock().unwrap().display_to_id.contains_key(&first);
            if exists {
                return self.send_message(&first, new_content);
            }
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
            "join room".to_owned(),
            "create room".to_owned(),
            "create encrypted room".to_owned(),
            "create space".to_owned(),
            "create dm".to_owned(),
            "leave room".to_owned(),
            "accept invite".to_owned(),
            "reject invite".to_owned(),
            "browse public rooms".to_owned(),
            "members".to_owned(),
            "kick member".to_owned(),
            "ban member".to_owned(),
            "unban member".to_owned(),
        ]
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        elem_key: &str,
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
                    self.access_token = result.access_token.clone();
                    self.user_id = result.user_id.clone();
                    self.save_access_token(&result.access_token);
                    self.save_user_id(&result.user_id);
                    self.maybe_start_sync();
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
                    self.user_id = result.user_id.clone();
                    self.save_access_token(&result.access_token);
                    self.save_user_id(&result.user_id);
                    self.uia_session.clear();
                    self.maybe_start_sync();
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
                    self.user_id = result.user_id.clone();
                    self.save_access_token(&result.access_token);
                    self.save_user_id(&result.user_id);
                    self.uia_session.clear();
                    self.maybe_start_sync();
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

            "join room" => {
                self.pending_action = Some(PendingAction::JoinRoom);
                Some(FfonElement::new_str("<input></input>".to_owned()))
            }

            "create room" => {
                self.pending_action =
                    Some(PendingAction::CreateRoom { encrypted: false, is_space: false });
                Some(FfonElement::new_str("<input></input>".to_owned()))
            }

            "create encrypted room" => {
                self.pending_action =
                    Some(PendingAction::CreateRoom { encrypted: true, is_space: false });
                Some(FfonElement::new_str("<input></input>".to_owned()))
            }

            "create space" => {
                self.pending_action =
                    Some(PendingAction::CreateRoom { encrypted: false, is_space: true });
                Some(FfonElement::new_str("<input></input>".to_owned()))
            }

            "create dm" => {
                self.pending_action = Some(PendingAction::CreateDm);
                Some(FfonElement::new_str("<input></input>".to_owned()))
            }

            "leave room" => {
                let segs = self.current_path_segments();
                let Some(room_key) = segs.first().cloned() else {
                    *error = "navigate into a room first".to_owned();
                    return None;
                };
                if room_key.starts_with("[space] ")
                    || room_key == "[public]"
                    || segs.len() > 1
                {
                    *error = "navigate into a room first".to_owned();
                    return None;
                }
                let room_id = match self.fetch_room_id_for_path_segment(&room_key) {
                    Some(id) => id,
                    None => {
                        *error = "room not found".to_owned();
                        return None;
                    }
                };
                match self.do_leave(&room_id) {
                    Ok(()) => {
                        self.pop_path();
                        Some(FfonElement::new_str("left room".to_owned()))
                    }
                    Err(e) => {
                        *error = format!("leave failed: {e}");
                        None
                    }
                }
            }

            "accept invite" => {
                let invite_key = if elem_key.starts_with("[invite] ") {
                    elem_key.to_owned()
                } else {
                    *error = "select an invite row first".to_owned();
                    return None;
                };
                let room_id = {
                    let cache = self.sync_cache.lock().unwrap();
                    cache.invite_display_to_id.get(&invite_key).cloned()
                };
                let Some(room_id) = room_id else {
                    *error = "invite not found".to_owned();
                    return None;
                };
                match self.do_join(&room_id) {
                    Ok(_) => Some(FfonElement::new_str("accepted invite".to_owned())),
                    Err(e) => {
                        *error = format!("accept failed: {e}");
                        None
                    }
                }
            }

            "reject invite" => {
                let invite_key = if elem_key.starts_with("[invite] ") {
                    elem_key.to_owned()
                } else {
                    *error = "select an invite row first".to_owned();
                    return None;
                };
                let room_id = {
                    let cache = self.sync_cache.lock().unwrap();
                    cache.invite_display_to_id.get(&invite_key).cloned()
                };
                let Some(room_id) = room_id else {
                    *error = "invite not found".to_owned();
                    return None;
                };
                let _ = self.do_leave(&room_id);
                let _ = self.do_forget(&room_id);
                Some(FfonElement::new_str("rejected invite".to_owned()))
            }

            "browse public rooms" => match self.do_public_rooms() {
                Ok(lines) => {
                    let count = lines.len();
                    self.public_rooms_cache = lines;
                    self.current_path = "/[public]".to_owned();
                    Some(FfonElement::new_str(format!("{count} public rooms")))
                }
                Err(e) => {
                    *error = format!("public rooms failed: {e}");
                    None
                }
            },

            "members" => {
                let segs = self.current_path_segments();
                let Some(room_key) = segs.first().cloned() else {
                    *error = "navigate into a room first".to_owned();
                    return None;
                };
                // Navigate into [members] sub-view.
                self.push_path("[members]");
                Some(FfonElement::new_str(format!("members of {room_key}")))
            }

            "kick member" => {
                let member_id = if elem_key.starts_with('@') {
                    elem_key.to_owned()
                } else {
                    // elem_key may be "user_id (display name)" — extract up to first space
                    let id = elem_key.split_whitespace().next().unwrap_or("").to_owned();
                    if id.is_empty() {
                        *error = "select a member first".to_owned();
                        return None;
                    }
                    id
                };
                let segs = self.current_path_segments();
                let room_key = segs.first().cloned().unwrap_or_default();
                let room_id = match self.fetch_room_id_for_path_segment(&room_key) {
                    Some(id) => id,
                    None => {
                        *error = "room not found".to_owned();
                        return None;
                    }
                };
                self.pending_action =
                    Some(PendingAction::KickMember { room_id, member_id });
                Some(FfonElement::new_str(
                    "<input>reason (optional)</input>".to_owned(),
                ))
            }

            "ban member" => {
                let member_id = if elem_key.starts_with('@') {
                    elem_key.to_owned()
                } else {
                    let id = elem_key.split_whitespace().next().unwrap_or("").to_owned();
                    if id.is_empty() {
                        *error = "select a member first".to_owned();
                        return None;
                    }
                    id
                };
                let segs = self.current_path_segments();
                let room_key = segs.first().cloned().unwrap_or_default();
                let room_id = match self.fetch_room_id_for_path_segment(&room_key) {
                    Some(id) => id,
                    None => {
                        *error = "room not found".to_owned();
                        return None;
                    }
                };
                self.pending_action =
                    Some(PendingAction::BanMember { room_id, member_id });
                Some(FfonElement::new_str(
                    "<input>reason (optional)</input>".to_owned(),
                ))
            }

            "unban member" => {
                let member_id = if elem_key.starts_with('@') {
                    elem_key.to_owned()
                } else {
                    let id = elem_key.split_whitespace().next().unwrap_or("").to_owned();
                    if id.is_empty() {
                        *error = "select a member first".to_owned();
                        return None;
                    }
                    id
                };
                let segs = self.current_path_segments();
                let room_key = segs.first().cloned().unwrap_or_default();
                let room_id = match self.fetch_room_id_for_path_segment(&room_key) {
                    Some(id) => id,
                    None => {
                        *error = "room not found".to_owned();
                        return None;
                    }
                };
                // unban has no reason, execute immediately
                match self.do_unban_member(&room_id, &member_id) {
                    Ok(()) => Some(FfonElement::new_str(format!("unbanned {member_id}"))),
                    Err(e) => {
                        *error = format!("unban failed: {e}");
                        None
                    }
                }
            }

            _ => None,
        }
    }

    fn on_button_press(&mut self, function_name: &str) {
        match function_name {
            "register" => {
                if !self.email.is_empty() {
                    if let Err(e) = self.request_email_token() {
                        self.register_error = Some(format!("email token request failed: {e}"));
                    }
                }
                if self.homeserver.is_empty() {
                    self.homeserver = "https://matrix.org".to_owned();
                }
                let result = self.do_register();
                self.handle_register_result(result);
            }
            "complete-registration" => {
                if self.uia_session.is_empty() {
                    return;
                }
                let session = self.uia_session.clone();
                let result = self.do_register_complete(&session);
                self.handle_register_result(result);
            }
            _ => {}
        }
    }

    fn take_error(&mut self) -> Option<String> {
        self.take_error()
    }

    fn create_file(&mut self, name: &str) -> bool {
        if let Some(action) = self.pending_action.take() {
            let mut err = String::new();
            self.execute_pending_action(action, name, &mut err);
            if !err.is_empty() {
                self.last_error = Some(err);
            }
        }
        false
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        self.on_setting_change(key, value);
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
// Test helpers — compiled only during `cargo test` and accessible from
// integration tests in src/sicompass/tests/.
// ---------------------------------------------------------------------------

impl ChatClientProvider {
    pub fn test_set_credentials(&mut self, homeserver: &str, token: &str) {
        self.homeserver = homeserver.to_owned();
        self.access_token = token.to_owned();
    }

    pub fn test_seed_room(&mut self, room_id: &str, display_name: &str) {
        let mut cache = self.sync_cache.lock().unwrap();
        cache.rooms.insert(
            room_id.to_owned(),
            sync::RoomState {
                room_id: room_id.to_owned(),
                display_name: display_name.to_owned(),
                ..Default::default()
            },
        );
        cache.display_to_id.insert(display_name.to_owned(), room_id.to_owned());
    }

    pub fn test_set_needs_refresh(&self) {
        self.needs_refresh_flag.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn test_clear_register_mode(&mut self) {
        self.register_mode = false;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn start_mock_server() -> (tokio::runtime::Runtime, MockServer) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let server = rt.block_on(MockServer::start());
        (rt, server)
    }

    fn mount(rt: &tokio::runtime::Runtime, server: &MockServer, mock: Mock) {
        rt.block_on(mock.mount(server));
    }

    fn provider_for(server: &MockServer) -> ChatClientProvider {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = server.uri();
        p.access_token = "test_token".to_owned();
        p
    }

    fn seed_room(p: &mut ChatClientProvider, room_id: &str, display_name: &str) {
        let mut cache = p.sync_cache.lock().unwrap();
        cache.rooms.insert(
            room_id.to_owned(),
            sync::RoomState {
                room_id: room_id.to_owned(),
                display_name: display_name.to_owned(),
                ..Default::default()
            },
        );
        cache.display_to_id.insert(display_name.to_owned(), room_id.to_owned());
    }

    fn seed_room_with_events(
        p: &mut ChatClientProvider,
        room_id: &str,
        display_name: &str,
        events: &[(&str, &str, &str)],
    ) {
        let timeline = events
            .iter()
            .enumerate()
            .map(|(i, (eid, sender, body))| sync::TimelineEvent {
                event_id: eid.to_string(),
                sender: sender.to_string(),
                body: body.to_string(),
                origin_server_ts: i as i64,
            })
            .collect();
        let mut cache = p.sync_cache.lock().unwrap();
        cache.rooms.insert(
            room_id.to_owned(),
            sync::RoomState {
                room_id: room_id.to_owned(),
                display_name: display_name.to_owned(),
                timeline,
                ..Default::default()
            },
        );
        cache.display_to_id.insert(display_name.to_owned(), room_id.to_owned());
    }

    // ---- original tests (adapted) ------------------------------------------

    #[test]
    fn test_fetch_root_no_config_shows_register_form() {
        let mut p = ChatClientProvider::new();
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("<button>register</button>"))
        }));
    }

    #[test]
    fn test_fetch_root_partial_config_prefills_register_form() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.username = "alice".to_owned();
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("<input>alice</input>"))
        }));
    }

    #[test]
    fn test_fetch_root_no_token_after_prior_login_shows_login_prompt() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.test_clear_register_mode();
        p.homeserver = "https://matrix.org".to_owned();
        p.username = "alice".to_owned();
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("configure homeserver"))
        }));
    }

    #[test]
    fn test_fetch_root_empty_rooms_message() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        p.sync_cache.lock().unwrap().next_batch = "s1".to_owned();
        let items = p.fetch();
        assert!(items.iter().any(|e| { e.as_str().map_or(false, |s| s.contains("no rooms found")) }));
    }

    #[test]
    fn test_fetch_root_rooms_become_objs() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        seed_room(&mut p, "!abc:example.com", "!abc:example.com");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.is_obj() && e.as_obj().map_or(false, |o| o.key != "meta")));
    }

    #[test]
    fn test_fetch_root_room_display_name_from_state() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        seed_room(&mut p, "!abc:example.com", "General");
        let items = p.fetch();
        assert!(items.iter().any(|e| { e.as_obj().map_or(false, |o| o.key == "General") }));
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
    fn push_path_appends_for_sub_navigation() {
        let mut p = ChatClientProvider::new();
        p.push_path("General");
        p.push_path("[members]");
        assert_eq!(p.current_path(), "/General/[members]");
    }

    #[test]
    fn pop_path_goes_up_one_level() {
        let mut p = ChatClientProvider::new();
        p.push_path("General");
        p.push_path("[members]");
        p.pop_path();
        assert_eq!(p.current_path(), "/General");
        p.pop_path();
        assert_eq!(p.current_path(), "/");
    }

    #[test]
    fn test_fetch_room_messages_chronological() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        seed_room_with_events(
            &mut p,
            "!abc:x",
            "General",
            &[("$e1", "@a:x", "first"), ("$e2", "@b:x", "second")],
        );
        p.push_path("General");
        let items = p.fetch();
        let msg_items: Vec<_> =
            items.iter().filter_map(|e| e.as_str()).filter(|s| s.contains(": ")).collect();
        assert_eq!(msg_items.len(), 2);
        assert!(msg_items[0].contains("first"));
        assert!(msg_items[1].contains("second"));
    }

    #[test]
    fn test_fetch_room_ends_with_input_bar() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        seed_room(&mut p, "!abc:x", "General");
        p.push_path("General");
        let items = p.fetch();
        let last = items.last().unwrap();
        assert!(last.as_str().map_or(false, |s| s.contains("<input>")));
    }

    #[test]
    fn test_fetch_room_has_members_nav_item() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        seed_room(&mut p, "!abc:x", "General");
        p.push_path("General");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "[members]")));
    }

    #[test]
    fn test_fetch_room_not_in_cache_returns_error() {
        let mut p = ChatClientProvider::new();
        p.homeserver = "http://127.0.0.1:1".to_owned();
        p.access_token = "tok".to_owned();
        p.push_path("NoSuchRoom");
        let items = p.fetch();
        assert!(items.iter().any(|e| { e.as_str().map_or(false, |s| s.contains("room not found")) }));
    }

    #[test]
    fn test_login_success_sets_access_token() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(path("/_matrix/client/v3/login"))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "access_token": "new_token_xyz", "user_id": "@user:server" }),
                )),
        );
        let mut p = ChatClientProvider::new();
        p.homeserver = server.uri();
        p.username = "user".to_owned();
        p.password = "pass".to_owned();
        let result = p.do_login();
        assert!(result.success);
        assert_eq!(p.access_token, "new_token_xyz");
        assert_eq!(p.user_id, "@user:server");
        drop(rt);
    }

    #[test]
    fn test_login_failure_returns_false() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(path("/_matrix/client/v3/login"))
                .respond_with(ResponseTemplate::new(403).set_body_json(
                    serde_json::json!({ "errcode": "M_FORBIDDEN", "error": "Bad credentials" }),
                )),
        );
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
        assert!(cmds.contains(&"join room".to_owned()));
        assert!(cmds.contains(&"create room".to_owned()));
        assert!(cmds.contains(&"leave room".to_owned()));
        assert!(cmds.contains(&"accept invite".to_owned()));
        assert!(cmds.contains(&"reject invite".to_owned()));
        assert!(cmds.contains(&"browse public rooms".to_owned()));
        assert!(cmds.contains(&"members".to_owned()));
        assert!(cmds.contains(&"kick member".to_owned()));
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
        mount(
            &rt,
            &server,
            Mock::given(method("PUT")).respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "event_id": "$abc" })),
            ),
        );
        let mut p = provider_for(&server);
        seed_room(&mut p, "!abc:x", "General");
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
        mount(
            &rt,
            &server,
            Mock::given(method("PUT")).respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "event_id": "$xyz" })),
            ),
        );
        let mut p = provider_for(&server);
        seed_room(&mut p, "!abc:x", "General");
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
        assert!(!err.is_empty());
    }

    #[test]
    fn test_handle_command_refresh_returns_none() {
        let mut p = ChatClientProvider::new();
        let mut err = String::new();
        let result = p.handle_command("refresh", "", 0, &mut err);
        assert!(result.is_none());
    }

    #[test]
    fn test_handle_command_send_message_returns_input() {
        let mut p = ChatClientProvider::new();
        let mut err = String::new();
        let result = p.handle_command("send message", "", 0, &mut err);
        assert!(result.is_some());
        assert!(result.unwrap().as_str().map_or(false, |s| s.contains("<input>")));
    }

    #[test]
    fn test_login_success_returns_user_id_element() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(path("/_matrix/client/v3/login"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": "syt_xyz",
                    "user_id": "@alice:server.org",
                }))),
        );
        let dir = tempfile::tempdir().unwrap();
        let mut p = ChatClientProvider::new()
            .with_config_path(dir.path().join("settings.json"))
            .with_sync_disabled();
        p.homeserver = server.uri();
        p.username = "alice".to_owned();
        p.password = "pass".to_owned();
        let mut err = String::new();
        let elem = p.handle_command("login", "", 0, &mut err);
        assert!(err.is_empty(), "no error on success, got: {err}");
        assert!(elem.is_some());
        assert!(elem.unwrap().as_str().map_or(false, |s| s.contains("@alice:server.org")));
        assert_eq!(p.user_id, "@alice:server.org");
        drop(rt);
    }

    #[test]
    fn test_register_without_credentials() {
        let mut p = ChatClientProvider::new();
        p.homeserver = "http://localhost".to_owned();
        let mut err = String::new();
        let result = p.handle_command("register", "", 0, &mut err);
        assert!(result.is_none());
        assert!(!err.is_empty());
    }

    #[test]
    fn test_register_success_sets_access_token() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(path("/_matrix/client/v3/register"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": "reg_token_abc",
                    "user_id": "@newuser:server.org",
                }))),
        );
        let dir = tempfile::tempdir().unwrap();
        let mut p = ChatClientProvider::new()
            .with_config_path(dir.path().join("settings.json"))
            .with_sync_disabled();
        p.homeserver = server.uri();
        p.username = "newuser".to_owned();
        p.password = "pass123".to_owned();
        let mut err = String::new();
        let elem = p.handle_command("register", "", 0, &mut err);
        assert!(err.is_empty(), "no error on success, got: {err}");
        assert!(elem.is_some());
        assert!(elem.unwrap().as_str().map_or(false, |s| s.contains("@newuser:server.org")));
        assert_eq!(p.access_token, "reg_token_abc");
        assert!(p.uia_session.is_empty());
        drop(rt);
    }

    #[test]
    fn test_register_requires_uia_stores_session() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(path("/_matrix/client/v3/register"))
                .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                    "session": "uia_session_xyz",
                    "flows": [{ "stages": ["m.login.recaptcha"] }],
                    "completed": [],
                }))),
        );
        let mut p = ChatClientProvider::new();
        p.homeserver = server.uri();
        p.username = "alice".to_owned();
        p.password = "pass".to_owned();
        let mut err = String::new();
        let elem = p.handle_command("register", "", 0, &mut err);
        assert!(err.is_empty(), "UIA requires-auth is not an error, got: {err}");
        assert!(elem.is_some());
        assert!(elem.unwrap().as_str().map_or(false, |s| s.contains("m.login.recaptcha")));
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
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(path("/_matrix/client/v3/register"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": "final_token",
                    "user_id": "@alice:server.org",
                }))),
        );
        let dir = tempfile::tempdir().unwrap();
        let mut p = ChatClientProvider::new()
            .with_config_path(dir.path().join("settings.json"))
            .with_sync_disabled();
        p.homeserver = server.uri();
        p.username = "alice".to_owned();
        p.password = "pass".to_owned();
        p.uia_session = "pending_session_abc".to_owned();
        let mut err = String::new();
        let elem = p.handle_command("complete registration", "", 0, &mut err);
        assert!(err.is_empty(), "no error on success, got: {err}");
        assert!(elem.is_some());
        assert!(elem.unwrap().as_str().map_or(false, |s| s.contains("@alice:server.org")));
        assert_eq!(p.access_token, "final_token");
        assert!(p.uia_session.is_empty());
        drop(rt);
    }

    #[test]
    fn test_encode_room_id_at_sign() {
        assert_eq!(encode_room_id("@user:server.org"), "%40user%3Aserver.org");
    }

    #[test]
    fn test_encode_room_id_slash() {
        assert_eq!(encode_room_id("a/b"), "a%2Fb");
    }

    #[test]
    fn test_encode_room_id_multibyte() {
        let encoded = encode_room_id("caf\u{e9}");
        assert_eq!(encoded, "caf%C3%A9");
    }

    #[test]
    fn test_encode_room_id_unreserved_pass_through() {
        assert_eq!(encode_room_id("abc-123_test.room~"), "abc-123_test.room~");
    }

    #[test]
    fn test_txn_id_is_monotonic() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("PUT")).respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "event_id": "$e" })),
            ),
        );
        let mut p = provider_for(&server);
        seed_room(&mut p, "!x:x", "Room");
        let before = TXN_COUNTER.load(Ordering::Relaxed);
        let ok1 = p.send_message("Room", "first");
        let mid = TXN_COUNTER.load(Ordering::Relaxed);
        let ok2 = p.send_message("Room", "second");
        let after = TXN_COUNTER.load(Ordering::Relaxed);
        assert!(ok1);
        assert!(ok2);
        assert!(mid > before);
        assert!(after > mid);
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
        let resp = serde_json::json!({ "errcode": "M_FORBIDDEN", "error": "Invalid password" });
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

    fn write_settings(path: &std::path::Path, json: serde_json::Value) {
        std::fs::write(path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
    }

    #[test]
    fn init_loads_credentials_from_config() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        write_settings(
            &cfg,
            serde_json::json!({
                "chat client": {
                    "chatHomeserver":  "https://matrix.org",
                    "chatAccessToken": "syt_test_token",
                    "chatUsername":    "alice",
                    "chatPassword":    "secret"
                }
            }),
        );
        let mut p = ChatClientProvider::new().with_config_path(cfg);
        p.init();
        assert_eq!(p.homeserver, "https://matrix.org");
        assert_eq!(p.access_token, "syt_test_token");
        assert_eq!(p.username, "alice");
        assert_eq!(p.password, "secret");
    }

    #[test]
    fn init_missing_config_leaves_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        let mut p = ChatClientProvider::new().with_config_path(cfg);
        p.init();
        assert!(p.homeserver.is_empty());
        assert!(p.access_token.is_empty());
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("<button>register</button>"))
        }));
    }

    #[test]
    fn init_partial_config_only_loads_present_keys() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        write_settings(
            &cfg,
            serde_json::json!({
                "chat client": {
                    "chatHomeserver": "https://matrix.org",
                    "chatUsername":   "bob"
                }
            }),
        );
        let mut p = ChatClientProvider::new().with_config_path(cfg);
        p.init();
        assert_eq!(p.homeserver, "https://matrix.org");
        assert_eq!(p.username, "bob");
        assert!(p.access_token.is_empty());
        assert!(p.password.is_empty());
    }

    #[test]
    fn init_then_setting_change_overrides_config() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        write_settings(
            &cfg,
            serde_json::json!({ "chat client": { "chatAccessToken": "old_token" } }),
        );
        let mut p = ChatClientProvider::new().with_config_path(cfg);
        p.init();
        assert_eq!(p.access_token, "old_token");
        p.on_setting_change("chatAccessToken", "live_token");
        assert_eq!(p.access_token, "live_token");
    }

    #[test]
    fn next_batch_loaded_from_settings_json_on_init() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        write_settings(
            &cfg,
            serde_json::json!({
                "chat client": {
                    "chatHomeserver":    "https://matrix.org",
                    "chatAccessToken":   "tok",
                    "chatSyncNextBatch": "s999"
                }
            }),
        );
        let mut p =
            ChatClientProvider::new().with_config_path(cfg).with_sync_disabled();
        p.init();
        assert_eq!(p.sync_cache.lock().unwrap().next_batch, "s999");
    }

    #[test]
    fn init_starts_sync_when_credentials_present() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("GET")).respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "next_batch": "s1", "rooms": { "join": {} } }),
                ),
            ),
        );
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        write_settings(
            &cfg,
            serde_json::json!({
                "chat client": {
                    "chatHomeserver":  server.uri(),
                    "chatAccessToken": "tok"
                }
            }),
        );
        let mut p = ChatClientProvider::new().with_config_path(cfg);
        p.init();
        assert!(p.sync_controller.is_running());
        p.sync_controller.stop();
        drop(rt);
    }

    #[test]
    fn needs_refresh_and_clear_works() {
        let mut p = ChatClientProvider::new();
        assert!(!p.needs_refresh());
        p.needs_refresh_flag.store(true, Ordering::Relaxed);
        assert!(p.needs_refresh());
        p.clear_needs_refresh();
        assert!(!p.needs_refresh());
    }

    #[test]
    fn on_setting_change_homeserver_clears_cache() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        seed_room(&mut p, "!abc:x", "General");
        assert!(!p.sync_cache.lock().unwrap().rooms.is_empty());
        p.on_setting_change("chatHomeserver", "https://new.example.com");
        assert!(p.sync_cache.lock().unwrap().rooms.is_empty());
    }

    #[test]
    fn fetch_root_shows_loading_when_no_sync_token() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        let items = p.fetch();
        assert!(items.iter().any(|e| { e.as_str().map_or(false, |s| s.contains("Loading")) }));
    }

    #[test]
    fn register_form_prefills_homeserver_default() {
        let mut p = ChatClientProvider::new();
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("https://matrix.org"))
        }));
    }

    #[test]
    fn register_form_renders_four_inputs_and_button() {
        let mut p = ChatClientProvider::new();
        let items = p.fetch();
        let inputs =
            items.iter().filter(|e| e.as_str().map_or(false, |s| s.contains("<input>"))).count();
        assert_eq!(inputs, 4);
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("<button>register</button>"))
        }));
    }

    #[test]
    fn register_form_renders_existing_field_values() {
        let mut p = ChatClientProvider::new();
        p.username = "friendlyflow".to_owned();
        p.email = "2friendlyflow@gmail.com".to_owned();
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("<input>friendlyflow</input>"))
        }));
        assert!(items.iter().any(|e| {
            e.as_str()
                .map_or(false, |s| s.contains("<input>2friendlyflow@gmail.com</input>"))
        }));
    }

    #[test]
    fn register_form_shows_complete_button_when_uia_pending() {
        let mut p = ChatClientProvider::new();
        p.uia_session = "sess123".to_owned();
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("<button>complete-registration</button>"))
        }));
    }

    #[test]
    fn commit_edit_updates_username_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        let mut p = ChatClientProvider::new().with_config_path(cfg.clone());
        p.push_path("Username");
        let changed = p.commit_edit("", "friendlyflow");
        p.pop_path();
        assert!(changed);
        assert_eq!(p.username, "friendlyflow");
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(saved["chat client"]["chatUsername"].as_str(), Some("friendlyflow"));
    }

    #[test]
    fn commit_edit_updates_email_field() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        let mut p = ChatClientProvider::new().with_config_path(cfg);
        p.push_path("Email");
        let changed = p.commit_edit("", "2friendlyflow@gmail.com");
        p.pop_path();
        assert!(changed);
        assert_eq!(p.email, "2friendlyflow@gmail.com");
    }

    #[test]
    fn on_button_press_register_without_email_calls_register_endpoint() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(wiremock::matchers::path("/_matrix/client/v3/register"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": "new_tok",
                    "user_id": "@friendlyflow:localhost",
                    "device_id": "DEVICE1",
                }))),
        );
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        let mut p =
            ChatClientProvider::new().with_config_path(cfg).with_sync_disabled();
        p.homeserver = server.uri();
        p.username = "friendlyflow".to_owned();
        p.password = "secret".to_owned();
        p.on_button_press("register");
        assert_eq!(p.access_token, "new_tok");
        drop(rt);
    }

    #[test]
    fn on_button_press_register_with_email_calls_request_token_then_register() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(wiremock::matchers::path(
                    "/_matrix/client/v3/register/email/requestToken",
                ))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "sid": "sid_abc" }),
                )),
        );
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(wiremock::matchers::path("/_matrix/client/v3/register"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": "tok2",
                    "user_id": "@friendlyflow:localhost",
                    "device_id": "D2",
                }))),
        );
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        let mut p =
            ChatClientProvider::new().with_config_path(cfg).with_sync_disabled();
        p.homeserver = server.uri();
        p.username = "friendlyflow".to_owned();
        p.password = "secret".to_owned();
        p.email = "2friendlyflow@gmail.com".to_owned();
        p.on_button_press("register");
        assert_eq!(p.register_3pid_sid, "sid_abc");
        assert_eq!(p.access_token, "tok2");
        drop(rt);
    }

    #[test]
    fn on_button_press_register_stores_uia_session() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(wiremock::matchers::path("/_matrix/client/v3/register"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "session": "uia_sess",
                    "flows": [{ "stages": ["m.login.dummy"] }],
                }))),
        );
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = server.uri();
        p.username = "friendlyflow".to_owned();
        p.password = "secret".to_owned();
        p.on_button_press("register");
        assert_eq!(p.uia_session, "uia_sess");
        let items = p.fetch();
        assert!(items.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("<button>complete-registration</button>"))
        }));
        drop(rt);
    }

    // ---- New room-lifecycle tests --------------------------------------------

    #[test]
    fn handle_command_join_room_sets_pending_and_returns_input() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        let mut err = String::new();
        let elem = p.handle_command("join room", "", 0, &mut err);
        assert!(err.is_empty());
        assert!(elem.is_some());
        assert!(elem.unwrap().as_str().map_or(false, |s| s.contains("<input>")));
        assert!(p.pending_action.is_some());
    }

    #[test]
    fn handle_command_create_room_sets_pending() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        let mut err = String::new();
        let elem = p.handle_command("create room", "", 0, &mut err);
        assert!(elem.is_some());
        assert!(p.pending_action.is_some());
    }

    #[test]
    fn handle_command_leave_room_without_room_returns_error() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        let mut err = String::new();
        let elem = p.handle_command("leave room", "", 0, &mut err);
        assert!(elem.is_none());
        assert!(!err.is_empty());
    }

    #[test]
    fn handle_command_join_room_posts_to_join_endpoint() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(wiremock::matchers::path_regex("/_matrix/client/v3/join/.*"))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "room_id": "!abc:server" }),
                )),
        );
        let mut p = provider_for(&server);
        let mut err = String::new();
        p.handle_command("join room", "", 0, &mut err);
        let ok = p.commit_edit("", "#general:server");
        assert!(ok, "join should succeed");
        drop(rt);
    }

    #[test]
    fn handle_command_create_room_posts_to_create_endpoint() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(wiremock::matchers::path("/_matrix/client/v3/createRoom"))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "room_id": "!new:server" }),
                )),
        );
        let mut p = provider_for(&server);
        let mut err = String::new();
        p.handle_command("create room", "", 0, &mut err);
        let ok = p.commit_edit("", "My Room");
        assert!(ok, "create room should succeed");
        drop(rt);
    }

    #[test]
    fn handle_command_leave_room_posts_to_leave_endpoint() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(wiremock::matchers::path_regex("/_matrix/client/v3/rooms/.*/leave"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({}))),
        );
        let mut p = provider_for(&server);
        seed_room(&mut p, "!abc:x", "General");
        p.push_path("General");
        let mut err = String::new();
        let elem = p.handle_command("leave room", "", 0, &mut err);
        assert!(err.is_empty(), "got: {err}");
        assert!(elem.is_some());
        assert_eq!(p.current_path(), "/");
        drop(rt);
    }

    #[test]
    fn handle_command_accept_invite_without_invite_key_errors() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        let mut err = String::new();
        let elem = p.handle_command("accept invite", "General", 0, &mut err);
        assert!(elem.is_none());
        assert!(!err.is_empty());
    }

    #[test]
    fn handle_command_accept_invite_calls_join() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("POST"))
                .and(wiremock::matchers::path_regex("/_matrix/client/v3/join/.*"))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "room_id": "!inv:server" }),
                )),
        );
        let mut p = provider_for(&server);
        {
            let mut cache = p.sync_cache.lock().unwrap();
            cache.invites.insert(
                "!inv:server".to_owned(),
                sync::InviteState {
                    room_id: "!inv:server".to_owned(),
                    display_name: "Party".to_owned(),
                    inviter: "@alice:server".to_owned(),
                },
            );
            cache
                .invite_display_to_id
                .insert("[invite] Party".to_owned(), "!inv:server".to_owned());
        }
        let mut err = String::new();
        let elem = p.handle_command("accept invite", "[invite] Party", 0, &mut err);
        assert!(err.is_empty(), "got: {err}");
        assert!(elem.is_some());
        drop(rt);
    }

    #[test]
    fn fetch_root_shows_invites_before_rooms() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        {
            let mut cache = p.sync_cache.lock().unwrap();
            cache.rooms.insert(
                "!r:s".to_owned(),
                sync::RoomState {
                    room_id: "!r:s".to_owned(),
                    display_name: "Zzz".to_owned(),
                    ..Default::default()
                },
            );
            cache.display_to_id.insert("Zzz".to_owned(), "!r:s".to_owned());
            cache.invites.insert(
                "!inv:s".to_owned(),
                sync::InviteState {
                    room_id: "!inv:s".to_owned(),
                    display_name: "Aaa".to_owned(),
                    inviter: String::new(),
                },
            );
            cache.invite_display_to_id.insert("[invite] Aaa".to_owned(), "!inv:s".to_owned());
        }
        let items = p.fetch();
        let keys: Vec<&str> = items.iter().filter_map(|e| e.as_obj().map(|o| o.key.as_str())).collect();
        let invite_pos = keys.iter().position(|k| k.starts_with("[invite]"));
        let room_pos = keys.iter().position(|k| *k == "Zzz");
        assert!(invite_pos.is_some() && room_pos.is_some());
        assert!(invite_pos.unwrap() < room_pos.unwrap(), "invites should appear before rooms");
    }

    #[test]
    fn fetch_space_shows_children() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        {
            let mut cache = p.sync_cache.lock().unwrap();
            cache.rooms.insert(
                "!space:s".to_owned(),
                sync::RoomState {
                    room_id: "!space:s".to_owned(),
                    display_name: "Work".to_owned(),
                    kind: sync::RoomKind::Space,
                    space_children: vec!["!general:s".to_owned()],
                    ..Default::default()
                },
            );
            cache.rooms.insert(
                "!general:s".to_owned(),
                sync::RoomState {
                    room_id: "!general:s".to_owned(),
                    display_name: "general".to_owned(),
                    ..Default::default()
                },
            );
            cache.display_to_id.insert("[space] Work".to_owned(), "!space:s".to_owned());
            cache.display_to_id.insert("general".to_owned(), "!general:s".to_owned());
        }
        p.push_path("[space] Work");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "general")));
    }

    #[test]
    fn fetch_members_shows_member_list() {
        let mut p = ChatClientProvider::new().with_sync_disabled();
        p.homeserver = "https://matrix.org".to_owned();
        p.access_token = "tok".to_owned();
        {
            let mut cache = p.sync_cache.lock().unwrap();
            let mut members = std::collections::HashMap::new();
            members.insert(
                "@alice:s".to_owned(),
                sync::Member {
                    user_id: "@alice:s".to_owned(),
                    display_name: Some("Alice".to_owned()),
                    membership: "join".to_owned(),
                },
            );
            cache.rooms.insert(
                "!r:s".to_owned(),
                sync::RoomState {
                    room_id: "!r:s".to_owned(),
                    display_name: "General".to_owned(),
                    members,
                    ..Default::default()
                },
            );
            cache.display_to_id.insert("General".to_owned(), "!r:s".to_owned());
        }
        p.push_path("General");
        p.push_path("[members]");
        let items = p.fetch();
        assert!(items.iter().any(|e| e.as_obj().map_or(false, |o| o.key.contains("@alice:s"))));
    }
}

// ---------------------------------------------------------------------------
// SDK registration
// ---------------------------------------------------------------------------

/// Register the chat client with the SDK factory and manifest registries.
pub fn register() {
    sicompass_sdk::register_provider_factory("chatclient", || {
        Box::new(ChatClientProvider::new())
    });
    sicompass_sdk::register_builtin_manifest(
        sicompass_sdk::BuiltinManifest::new("chatclient", "chat client").with_settings(vec![
            sicompass_sdk::SettingDecl::text(
                "chat client",
                "homeserver URL",
                "chatHomeserver",
                "https://matrix.org",
            ),
            sicompass_sdk::SettingDecl::text(
                "chat client",
                "access token",
                "chatAccessToken",
                "",
            ),
            sicompass_sdk::SettingDecl::text(
                "chat client",
                "username",
                "chatUsername",
                "",
            ),
            sicompass_sdk::SettingDecl::text(
                "chat client",
                "password",
                "chatPassword",
                "",
            ),
            sicompass_sdk::SettingDecl::text("chat client", "email", "chatEmail", ""),
        ]),
    );
}
