//! Matrix message sending and history pagination for ChatClientProvider.

use std::sync::atomic::Ordering;
use super::{ChatClientProvider, encode_room_id, TXN_COUNTER};
use super::sync;

impl ChatClientProvider {
    pub(crate) fn send_message(&self, room_display_key: &str, body_text: &str) -> bool {
        let room_id = match self.cache().display_to_id.get(room_display_key).cloned() {
            Some(id) => id,
            None => return false,
        };
        let client = match self.client() {
            Ok(c) => c,
            Err(_) => return false,
        };
        let encoded_id = encode_room_id(&room_id);
        let txn_id = TXN_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
        let url = self.api(&format!(
            "/_matrix/client/v3/rooms/{encoded_id}/send/m.room.message/m{txn_id}"
        ));
        let payload = serde_json::json!({ "msgtype": "m.text", "body": body_text });
        client
            .put(&url)
            .header("Authorization", self.auth_header())
            .json(&payload)
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub(crate) fn do_fetch_older_messages(
        &self,
        room_display_key: &str,
    ) -> Result<usize, String> {
        let (room_id, from_token) = {
            let cache = self.cache();
            let room_id = cache
                .display_to_id
                .get(room_display_key)
                .cloned()
                .ok_or_else(|| "room not found".to_owned())?;
            let from_token = cache
                .rooms
                .get(&room_id)
                .and_then(|r| r.prev_batch.clone())
                .ok_or_else(|| "no earlier messages available".to_owned())?;
            (room_id, from_token)
        };
        let client = self.client().map_err(|e| e.to_string())?;
        let encoded = encode_room_id(&room_id);
        let url = self.api(&format!(
            "/_matrix/client/v3/rooms/{encoded}/messages?from={from_token}&dir=b&limit=50"
        ));
        let resp = client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            let b: serde_json::Value = resp.json().unwrap_or_default();
            return Err(b
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("messages failed")
                .to_owned());
        }
        let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let new_prev = body.get("end").and_then(|v| v.as_str()).map(|s| s.to_owned());
        let events =
            body.get("chunk").and_then(|c| c.as_array()).cloned().unwrap_or_default();
        let mut new_events: Vec<sync::TimelineEvent> = events
            .iter()
            .filter_map(|ev| {
                if ev.get("type").and_then(|t| t.as_str()) != Some("m.room.message") {
                    return None;
                }
                let event_id = ev.get("event_id").and_then(|v| v.as_str())?.to_owned();
                let sender =
                    ev.get("sender").and_then(|v| v.as_str()).unwrap_or("?").to_owned();
                let body_text = ev
                    .get("content")
                    .and_then(|c| c.get("body"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("(media)")
                    .to_owned();
                let ts = ev.get("origin_server_ts").and_then(|v| v.as_i64()).unwrap_or(0);
                Some(sync::TimelineEvent {
                    event_id,
                    sender,
                    body: body_text,
                    origin_server_ts: ts,
                })
            })
            .collect();
        // /messages?dir=b returns newest-first; reverse to chronological.
        new_events.reverse();
        let fetched = new_events.len();
        let mut cache = self.cache();
        if let Some(room) = cache.rooms.get_mut(&room_id) {
            let existing: std::collections::HashSet<String> =
                room.timeline.iter().map(|e| e.event_id.clone()).collect();
            let to_prepend: Vec<_> = new_events
                .into_iter()
                .filter(|e| !existing.contains(&e.event_id))
                .collect();
            let mut new_tl = to_prepend;
            new_tl.extend(room.timeline.drain(..));
            room.timeline = new_tl;
            let max = sync::MAX_TIMELINE * 2;
            if room.timeline.len() > max {
                let drain = room.timeline.len() - max;
                room.timeline.drain(..drain);
            }
            room.prev_batch = new_prev;
            Ok(fetched)
        } else {
            Err("room not found".to_owned())
        }
    }
}
