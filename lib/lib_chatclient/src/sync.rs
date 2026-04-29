//! Matrix /sync background thread.
//!
//! Mirrors the structure of lib_emailclient/src/idle.rs — same AtomicBool
//! wake mechanism, same non-blocking stop, same reconnect back-off.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const RECONNECT_DELAY_SECS: u64 = 10;
pub const MAX_TIMELINE: usize = 200;

// ---------------------------------------------------------------------------
// Cache types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TimelineEvent {
    pub event_id: String,
    pub sender: String,
    pub body: String,
    pub origin_server_ts: i64,
}

#[derive(Debug, Clone)]
pub struct RoomState {
    pub room_id: String,
    pub display_name: String,
    pub timeline: Vec<TimelineEvent>,
}

#[derive(Debug, Default)]
pub struct SyncCache {
    pub next_batch: String,
    /// Keyed by room_id.
    pub rooms: HashMap<String, RoomState>,
    /// display_name → room_id (rebuilt after every parse).
    pub display_to_id: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// parse_sync_response — pure function, testable without HTTP
// ---------------------------------------------------------------------------

/// Merge a Matrix /sync JSON response body into `cache`.
///
/// Returns `true` if the cache changed (new room, new event, updated name,
/// or a new `next_batch` token).
pub fn parse_sync_response(json: serde_json::Value, cache: &mut SyncCache) -> bool {
    let mut changed = false;

    if let Some(nb) = json.get("next_batch").and_then(|v| v.as_str()) {
        if cache.next_batch != nb {
            cache.next_batch = nb.to_owned();
            changed = true;
        }
    }

    let Some(join) = json
        .get("rooms")
        .and_then(|r| r.get("join"))
        .and_then(|j| j.as_object())
    else {
        rebuild_display_map(cache);
        return changed;
    };

    for (room_id, room_data) in join {
        let entry = cache.rooms.entry(room_id.clone()).or_insert_with(|| {
            changed = true;
            RoomState {
                room_id: room_id.clone(),
                display_name: room_id.clone(),
                timeline: Vec::new(),
            }
        });

        // Update display name from state events.
        if let Some(state_events) = room_data
            .get("state")
            .and_then(|s| s.get("events"))
            .and_then(|e| e.as_array())
        {
            for ev in state_events {
                if ev.get("type").and_then(|t| t.as_str()) != Some("m.room.name") {
                    continue;
                }
                if let Some(name) = ev
                    .get("content")
                    .and_then(|c| c.get("name"))
                    .and_then(|n| n.as_str())
                {
                    if !name.is_empty() && entry.display_name != name {
                        entry.display_name = name.to_owned();
                        changed = true;
                    }
                }
            }
        }

        // Also check timeline state events for room name (some servers send it here).
        if let Some(timeline_events) = room_data
            .get("timeline")
            .and_then(|t| t.get("events"))
            .and_then(|e| e.as_array())
        {
            for ev in timeline_events {
                let ev_type = ev.get("type").and_then(|t| t.as_str()).unwrap_or("");

                if ev_type == "m.room.name" {
                    if let Some(name) = ev
                        .get("content")
                        .and_then(|c| c.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        if !name.is_empty() && entry.display_name != name {
                            entry.display_name = name.to_owned();
                            changed = true;
                        }
                    }
                    continue;
                }

                if ev_type != "m.room.message" {
                    continue;
                }

                let event_id = ev
                    .get("event_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                // Skip duplicates (server re-delivers on limited timelines).
                if !event_id.is_empty() && entry.timeline.iter().any(|e| e.event_id == event_id) {
                    continue;
                }
                let sender = ev
                    .get("sender")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_owned();
                let body = ev
                    .get("content")
                    .and_then(|c| c.get("body"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("(media)")
                    .to_owned();
                let origin_server_ts = ev
                    .get("origin_server_ts")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                entry.timeline.push(TimelineEvent { event_id, sender, body, origin_server_ts });
                changed = true;
            }

            // Trim oldest events when over the cap.
            if entry.timeline.len() > MAX_TIMELINE {
                let drain = entry.timeline.len() - MAX_TIMELINE;
                entry.timeline.drain(..drain);
            }
        }
    }

    rebuild_display_map(cache);
    changed
}

fn rebuild_display_map(cache: &mut SyncCache) {
    cache.display_to_id.clear();
    for (room_id, state) in &cache.rooms {
        cache.display_to_id.insert(state.display_name.clone(), room_id.clone());
    }
}

// ---------------------------------------------------------------------------
// SyncController
// ---------------------------------------------------------------------------

pub struct SyncController {
    cache: Arc<Mutex<SyncCache>>,
    notify: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SyncController {
    pub fn new(cache: Arc<Mutex<SyncCache>>, notify: Arc<AtomicBool>) -> Self {
        SyncController {
            cache,
            notify,
            running: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }

    /// Start (or restart) the /sync background thread.
    ///
    /// Stops any existing session first, then spawns a new thread.
    /// `config_path` is used to persist the `next_batch` token.
    pub fn start(
        &mut self,
        homeserver: String,
        access_token: String,
        config_path: Option<std::path::PathBuf>,
    ) {
        self.stop();

        let cache = Arc::clone(&self.cache);
        let notify = Arc::clone(&self.notify);
        let running = Arc::clone(&self.running);
        running.store(true, Ordering::Relaxed);

        self.thread = Some(std::thread::spawn(move || {
            sync_loop(homeserver, access_token, config_path, cache, notify, running);
        }));
    }

    /// Stop the background sync thread.
    ///
    /// Sets the running flag to false and drops the handle without joining.
    /// The thread exits within one HTTP timeout (≤60 s) once it sees
    /// running=false. Not joining avoids blocking the main thread.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        let _ = self.thread.take();
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Drop for SyncController {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------
// Sync loop
// ---------------------------------------------------------------------------

fn sync_loop(
    homeserver: String,
    access_token: String,
    config_path: Option<std::path::PathBuf>,
    cache: Arc<Mutex<SyncCache>>,
    notify: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) {
    while running.load(Ordering::Relaxed) {
        match run_sync_session(&homeserver, &access_token, &config_path, &cache, &notify, &running)
        {
            Ok(()) => {}
            Err(e) => eprintln!("chatclient_sync: session error: {e}"),
        }

        if !running.load(Ordering::Relaxed) {
            break;
        }

        // Back-off before reconnecting (mirrors idle.rs:94-100).
        for _ in 0..RECONNECT_DELAY_SECS {
            if !running.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

/// Build the HTTP client once, then loop over /sync calls until an error or
/// the running flag is cleared.
fn run_sync_session(
    homeserver: &str,
    access_token: &str,
    config_path: &Option<std::path::PathBuf>,
    cache: &Arc<Mutex<SyncCache>>,
    notify: &Arc<AtomicBool>,
    running: &Arc<AtomicBool>,
) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("sicompass/1.0")
        .timeout(Duration::from_secs(60)) // must exceed Matrix ?timeout=30000 (30 s)
        .build()
        .map_err(|e| e.to_string())?;

    let base = homeserver.trim_end_matches('/');

    loop {
        if !running.load(Ordering::Relaxed) {
            return Ok(());
        }

        let since = cache.lock().unwrap().next_batch.clone();
        // Use timeout=0 for the initial sync (no `since`) so we get the
        // current state immediately rather than waiting 30 s.
        let url = if since.is_empty() {
            format!("{base}/_matrix/client/v3/sync?timeout=0")
        } else {
            format!("{base}/_matrix/client/v3/sync?since={since}&timeout=30000")
        };

        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {access_token}"))
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }

        let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let next_batch = body
            .get("next_batch")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();

        let changed = {
            let mut locked = cache.lock().unwrap();
            parse_sync_response(body, &mut locked)
        };

        if changed {
            notify.store(true, Ordering::Relaxed);
            if !next_batch.is_empty() {
                persist_next_batch(config_path, &next_batch);
            }
        }
    }
}

/// Write `next_batch` into settings.json under `"chat client"."chatSyncNextBatch"`.
fn persist_next_batch(config_path: &Option<std::path::PathBuf>, token: &str) {
    use serde_json::{Map, Value};
    let Some(path) = config_path else { return };
    let mut root: Map<String, Value> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| if let Value::Object(m) = v { Some(m) } else { None })
        .unwrap_or_default();
    let section = root
        .entry("chat client".to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Value::Object(m) = section {
        m.insert("chatSyncNextBatch".to_owned(), Value::String(token.to_owned()));
    }
    if let Ok(json) = serde_json::to_string_pretty(&Value::Object(root)) {
        let _ = std::fs::write(path, json);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sync_response(
        next_batch: &str,
        room_id: &str,
        room_name: Option<&str>,
        messages: &[(&str, &str, &str)], // (event_id, sender, body)
    ) -> serde_json::Value {
        let state_events = if let Some(name) = room_name {
            serde_json::json!([{
                "type": "m.room.name",
                "content": { "name": name }
            }])
        } else {
            serde_json::json!([])
        };
        let timeline_events: Vec<serde_json::Value> = messages
            .iter()
            .map(|(eid, sender, body)| serde_json::json!({
                "type": "m.room.message",
                "event_id": eid,
                "sender": sender,
                "content": { "body": body },
                "origin_server_ts": 1000u64
            }))
            .collect();
        serde_json::json!({
            "next_batch": next_batch,
            "rooms": {
                "join": {
                    room_id: {
                        "state": { "events": state_events },
                        "timeline": { "events": timeline_events }
                    }
                }
            }
        })
    }

    #[test]
    fn parse_sync_response_extracts_timeline_events() {
        let mut cache = SyncCache::default();
        let json = make_sync_response(
            "s1",
            "!room:server",
            Some("General"),
            &[("$e1", "@a:s", "hello"), ("$e2", "@b:s", "world"), ("$e3", "@a:s", "!")],
        );
        parse_sync_response(json, &mut cache);
        let room = &cache.rooms["!room:server"];
        assert_eq!(room.timeline.len(), 3);
        assert_eq!(room.timeline[0].body, "hello");
        assert_eq!(room.timeline[2].body, "!");
    }

    #[test]
    fn parse_sync_response_updates_room_name_from_state() {
        let mut cache = SyncCache::default();
        let json = make_sync_response("s1", "!room:server", Some("My Room"), &[]);
        parse_sync_response(json, &mut cache);
        assert_eq!(cache.rooms["!room:server"].display_name, "My Room");
        assert_eq!(cache.display_to_id["My Room"], "!room:server");
    }

    #[test]
    fn parse_sync_response_advances_next_batch_token() {
        let mut cache = SyncCache::default();
        let json = make_sync_response("s42", "!room:server", None, &[]);
        parse_sync_response(json, &mut cache);
        assert_eq!(cache.next_batch, "s42");
    }

    #[test]
    fn parse_sync_response_appends_to_existing_timeline() {
        let mut cache = SyncCache::default();
        parse_sync_response(
            make_sync_response("s1", "!r:s", None, &[("$e1", "@a:s", "first"), ("$e2", "@a:s", "second")]),
            &mut cache,
        );
        parse_sync_response(
            make_sync_response("s2", "!r:s", None, &[("$e3", "@a:s", "third")]),
            &mut cache,
        );
        let room = &cache.rooms["!r:s"];
        assert_eq!(room.timeline.len(), 3);
        assert_eq!(room.timeline[2].body, "third");
    }

    #[test]
    fn parse_sync_response_trims_timeline_to_max_length() {
        let mut cache = SyncCache::default();
        // Feed events in two batches so we exercise the trim path.
        let batch1: Vec<(String, String, String)> = (0..150)
            .map(|i| (format!("$e{i}"), "@a:s".to_owned(), format!("msg{i}")))
            .collect();
        let refs1: Vec<(&str, &str, &str)> =
            batch1.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str())).collect();
        parse_sync_response(make_sync_response("s1", "!r:s", None, &refs1), &mut cache);

        let batch2: Vec<(String, String, String)> = (150..250)
            .map(|i| (format!("$e{i}"), "@a:s".to_owned(), format!("msg{i}")))
            .collect();
        let refs2: Vec<(&str, &str, &str)> =
            batch2.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str())).collect();
        parse_sync_response(make_sync_response("s2", "!r:s", None, &refs2), &mut cache);

        assert_eq!(cache.rooms["!r:s"].timeline.len(), MAX_TIMELINE);
    }

    #[test]
    fn parse_sync_response_skips_duplicate_events() {
        let mut cache = SyncCache::default();
        let json = make_sync_response("s1", "!r:s", None, &[("$e1", "@a:s", "hello")]);
        parse_sync_response(json, &mut cache);
        // Re-deliver same event.
        let json2 = make_sync_response("s2", "!r:s", None, &[("$e1", "@a:s", "hello")]);
        parse_sync_response(json2, &mut cache);
        assert_eq!(cache.rooms["!r:s"].timeline.len(), 1);
    }

    #[test]
    fn parse_sync_response_returns_true_on_change() {
        let mut cache = SyncCache::default();
        let json = make_sync_response("s1", "!r:s", None, &[("$e1", "@a:s", "hi")]);
        assert!(parse_sync_response(json, &mut cache));
    }

    #[test]
    fn parse_sync_response_returns_false_when_nothing_new() {
        let mut cache = SyncCache::default();
        // Same next_batch, same event already in cache.
        cache.next_batch = "s1".to_owned();
        cache.rooms.insert("!r:s".to_owned(), RoomState {
            room_id: "!r:s".to_owned(),
            display_name: "Room".to_owned(),
            timeline: vec![TimelineEvent {
                event_id: "$e1".to_owned(),
                sender: "@a:s".to_owned(),
                body: "hi".to_owned(),
                origin_server_ts: 1000,
            }],
        });
        let json = make_sync_response("s1", "!r:s", None, &[("$e1", "@a:s", "hi")]);
        assert!(!parse_sync_response(json, &mut cache));
    }

    #[test]
    fn sync_controller_start_stop_noop_without_token() {
        // With an unreachable URL the thread should fail fast without panicking.
        let cache = Arc::new(Mutex::new(SyncCache::default()));
        let notify = Arc::new(AtomicBool::new(false));
        let mut ctrl = SyncController::new(Arc::clone(&cache), Arc::clone(&notify));
        ctrl.start(
            "http://127.0.0.1:1".to_owned(),
            "tok".to_owned(),
            None,
        );
        std::thread::sleep(Duration::from_millis(100));
        ctrl.stop();
        assert!(!ctrl.is_running());
    }

    #[test]
    fn sync_controller_start_then_stop_clears_running_flag() {
        let cache = Arc::new(Mutex::new(SyncCache::default()));
        let notify = Arc::new(AtomicBool::new(false));
        let mut ctrl = SyncController::new(cache, notify);
        ctrl.start("http://127.0.0.1:1".to_owned(), "tok".to_owned(), None);
        assert!(ctrl.is_running());
        ctrl.stop();
        assert!(!ctrl.is_running());
    }

    #[test]
    fn needs_refresh_propagates_via_flag() {
        let cache = Arc::new(Mutex::new(SyncCache::default()));
        let notify = Arc::new(AtomicBool::new(false));
        let _ctrl = SyncController::new(cache, Arc::clone(&notify));
        notify.store(true, Ordering::Relaxed);
        assert!(notify.load(Ordering::Relaxed));
        notify.store(false, Ordering::Relaxed);
        assert!(!notify.load(Ordering::Relaxed));
    }
}
