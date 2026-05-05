//! Matrix member management operations for ChatClientProvider.

use super::{ChatClientProvider, encode_room_id};

impl ChatClientProvider {
    pub(crate) fn do_kick_member(
        &self,
        room_id: &str,
        member_id: &str,
        reason: &str,
    ) -> Result<(), String> {
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

    pub(crate) fn do_ban_member(
        &self,
        room_id: &str,
        member_id: &str,
        reason: &str,
    ) -> Result<(), String> {
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

    pub(crate) fn do_invite_user(&self, room_id: &str, user_id: &str) -> Result<(), String> {
        let client = self.client().map_err(|e| e.to_string())?;
        let encoded = encode_room_id(room_id);
        let url = self.api(&format!("/_matrix/client/v3/rooms/{encoded}/invite"));
        let resp = client
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({ "user_id": user_id.trim() }))
            .send()
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let b: serde_json::Value = resp.json().unwrap_or_default();
            Err(b.get("error").and_then(|v| v.as_str()).unwrap_or("invite failed").to_owned())
        }
    }

    pub(crate) fn do_unban_member(&self, room_id: &str, member_id: &str) -> Result<(), String> {
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
}
