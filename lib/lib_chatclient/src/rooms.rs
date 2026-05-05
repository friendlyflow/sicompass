//! Matrix room CRUD operations for ChatClientProvider.

use super::{ChatClientProvider, encode_room_id};

impl ChatClientProvider {
    pub(crate) fn fetch_room_id_for_path_segment(&self, segment: &str) -> Option<String> {
        self.cache().display_to_id.get(segment).cloned()
    }

    pub(crate) fn do_join(&self, alias_or_id: &str) -> Result<String, String> {
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

    pub(crate) fn do_leave(&self, room_id: &str) -> Result<(), String> {
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

    pub(crate) fn do_forget(&self, room_id: &str) -> Result<(), String> {
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

    pub(crate) fn do_create_room(
        &self,
        name: &str,
        encrypted: bool,
        is_space: bool,
        is_public: bool,
    ) -> Result<String, String> {
        let client = self.client().map_err(|e| e.to_string())?;
        let url = self.api("/_matrix/client/v3/createRoom");
        let preset = if is_public { "public_chat" } else { "private_chat" };
        let mut body = serde_json::json!({
            "name": name,
            "preset": preset,
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
            let room_id = b.get("room_id").and_then(|v| v.as_str()).unwrap_or("").to_owned();
            // Publish to the room directory so it's discoverable.
            if is_public && !room_id.is_empty() {
                let encoded = encode_room_id(&room_id);
                let dir_url =
                    self.api(&format!("/_matrix/client/v3/directory/list/room/{encoded}"));
                match client
                    .put(&dir_url)
                    .header("Authorization", self.auth_header())
                    .json(&serde_json::json!({ "visibility": "public" }))
                    .send()
                {
                    Ok(r) if r.status().is_success() => {}
                    Ok(r) => {
                        let b: serde_json::Value = r.json().unwrap_or_default();
                        let err =
                            b.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
                        return Err(format!(
                            "room created ({room_id}) but failed to publish to directory: {err}"
                        ));
                    }
                    Err(e) => {
                        return Err(format!(
                            "room created ({room_id}) but failed to publish to directory: {e}"
                        ));
                    }
                }
            }
            Ok(room_id)
        } else {
            let b: serde_json::Value = resp.json().unwrap_or_default();
            let err = b.get("error").and_then(|v| v.as_str()).unwrap_or("createRoom failed");
            Err(err.to_owned())
        }
    }

    pub(crate) fn do_create_dm(&self, target_mxid: &str) -> Result<String, String> {
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
            let room_id = b.get("room_id").and_then(|v| v.as_str()).unwrap_or("").to_owned();
            Ok(room_id)
        } else {
            let b: serde_json::Value = resp.json().unwrap_or_default();
            let err =
                b.get("error").and_then(|v| v.as_str()).unwrap_or("createRoom (DM) failed");
            Err(err.to_owned())
        }
    }

    pub(crate) fn do_public_rooms(&self, search: &str) -> Result<Vec<(String, String)>, String> {
        let client = self.client().map_err(|e| e.to_string())?;
        let url = self.api("/_matrix/client/v3/publicRooms");
        let body = if search.is_empty() {
            serde_json::json!({ "limit": 50 })
        } else {
            serde_json::json!({ "limit": 50, "filter": { "generic_search_term": search } })
        };
        let resp = client
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        let b: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let chunk = b.get("chunk").and_then(|c| c.as_array()).cloned().unwrap_or_default();
        let items: Vec<(String, String)> = chunk
            .iter()
            .map(|room| {
                let room_id = room
                    .get("room_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let alias = room
                    .get("canonical_alias")
                    .and_then(|v| v.as_str())
                    .or_else(|| room.get("room_id").and_then(|v| v.as_str()))
                    .unwrap_or("?");
                let name = room.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let members =
                    room.get("num_joined_members").and_then(|v| v.as_u64()).unwrap_or(0);
                let display = format!("{alias} — {name} ({members} members)");
                (display, room_id)
            })
            .collect();
        Ok(items)
    }
}
