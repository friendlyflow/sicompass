//! Matrix /sync background thread.
//!
//! Mirrors the structure of lib_emailclient/src/idle.rs — same AtomicBool
//! wake mechanism, same non-blocking stop, same reconnect back-off.

use std::collections::{HashMap, HashSet};
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

#[derive(Debug, Clone, Default, PartialEq)]
pub enum RoomKind {
    #[default]
    Room,
    Space,
}

#[derive(Debug, Clone)]
pub struct Member {
    pub user_id: String,
    pub display_name: Option<String>,
    pub membership: String,
}

#[derive(Debug, Clone, Default)]
pub struct RoomState {
    pub room_id: String,
    pub display_name: String,
    pub timeline: Vec<TimelineEvent>,
    pub kind: RoomKind,
    pub is_dm: bool,
    pub members: HashMap<String, Member>,
    pub space_children: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct InviteState {
    pub room_id: String,
    pub display_name: String,
    pub inviter: String,
}

#[derive(Debug, Default)]
pub struct SyncCache {
    pub next_batch: String,
    /// Our own Matrix user ID — set once from login/register or on sync-thread start.
    pub self_user_id: String,
    /// Joined rooms keyed by room_id.
    pub rooms: HashMap<String, RoomState>,
    /// display key → room_id for joined rooms (spaces prefixed "[space] ").
    pub display_to_id: HashMap<String, String>,
    /// Pending invites keyed by room_id.
    pub invites: HashMap<String, InviteState>,
    /// "[invite] name" → room_id for pending invites.
    pub invite_display_to_id: HashMap<String, String>,
    /// Set of room_ids that are DMs (from m.direct account-data).
    pub direct_room_ids: HashSet<String>,
    /// room_id → partner user_id for DM rooms.
    pub direct_room_to_user: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// parse_sync_response — pure function, testable without HTTP
// ---------------------------------------------------------------------------

/// Merge a Matrix /sync JSON response body into `cache`.
///
/// Returns `true` if the cache changed (new room, new event, updated name,
/// a new `next_batch` token, invite/leave/member change, or DM update).
pub fn parse_sync_response(json: serde_json::Value, cache: &mut SyncCache) -> bool {
    let mut changed = false;

    if let Some(nb) = json.get("next_batch").and_then(|v| v.as_str()) {
        if cache.next_batch != nb {
            cache.next_batch = nb.to_owned();
            changed = true;
        }
    }

    // Parse m.direct from account_data first — rooms below need it to set is_dm.
    parse_account_data(&json, cache, &mut changed);

    // Process rooms.
    let rooms_val = json.get("rooms");

    // rooms.invite — pending invites.
    if let Some(invite_map) = rooms_val
        .and_then(|r| r.get("invite"))
        .and_then(|i| i.as_object())
    {
        for (room_id, room_data) in invite_map {
            if cache.rooms.contains_key(room_id) {
                continue; // already joined; skip
            }
            let mut display_name = room_id.clone();
            let mut inviter = String::new();
            if let Some(events) = room_data
                .get("invite_state")
                .and_then(|s| s.get("events"))
                .and_then(|e| e.as_array())
            {
                for ev in events {
                    let ev_type = ev.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    let state_key = ev.get("state_key").and_then(|s| s.as_str()).unwrap_or("");
                    match ev_type {
                        "m.room.name" => {
                            if let Some(name) = ev
                                .get("content")
                                .and_then(|c| c.get("name"))
                                .and_then(|n| n.as_str())
                            {
                                if !name.is_empty() {
                                    display_name = name.to_owned();
                                }
                            }
                        }
                        "m.room.member"
                            if !cache.self_user_id.is_empty()
                                && state_key == cache.self_user_id =>
                        {
                            inviter = ev
                                .get("sender")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_owned();
                        }
                        _ => {}
                    }
                }
            }
            if !cache.invites.contains_key(room_id) {
                cache.invites.insert(
                    room_id.clone(),
                    InviteState { room_id: room_id.clone(), display_name, inviter },
                );
                changed = true;
            }
        }
    }

    // rooms.join — active joined rooms.
    if let Some(join) = rooms_val
        .and_then(|r| r.get("join"))
        .and_then(|j| j.as_object())
    {
        for (room_id, room_data) in join {
            // Clear any pending invite — the user accepted it.
            if cache.invites.remove(room_id).is_some() {
                changed = true;
            }

            let is_dm = cache.direct_room_ids.contains(room_id.as_str());

            let entry = cache.rooms.entry(room_id.clone()).or_insert_with(|| {
                changed = true;
                RoomState {
                    room_id: room_id.clone(),
                    display_name: room_id.clone(),
                    is_dm,
                    ..Default::default()
                }
            });

            // State events — room name, kind, space children, members.
            if let Some(state_events) = room_data
                .get("state")
                .and_then(|s| s.get("events"))
                .and_then(|e| e.as_array())
            {
                for ev in state_events {
                    let ev_type = ev.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    let state_key = ev.get("state_key").and_then(|s| s.as_str()).unwrap_or("");
                    apply_state_event(ev, ev_type, state_key, entry, &mut changed);
                }
            }

            // Timeline events — room name updates, member changes, messages.
            if let Some(timeline_events) = room_data
                .get("timeline")
                .and_then(|t| t.get("events"))
                .and_then(|e| e.as_array())
            {
                for ev in timeline_events {
                    let ev_type = ev.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    let state_key = ev.get("state_key").and_then(|s| s.as_str()).unwrap_or("");

                    // State events delivered via timeline still carry state_key.
                    if ev.get("state_key").is_some() {
                        apply_state_event(ev, ev_type, state_key, entry, &mut changed);
                        if ev_type == "m.room.message" {
                            // m.room.message is never a state event — fall through to message parsing.
                        } else {
                            continue;
                        }
                    }

                    if ev_type != "m.room.message" {
                        continue;
                    }

                    let event_id = ev
                        .get("event_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    if !event_id.is_empty()
                        && entry.timeline.iter().any(|e| e.event_id == event_id)
                    {
                        continue; // duplicate
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

                if entry.timeline.len() > MAX_TIMELINE {
                    let drain = entry.timeline.len() - MAX_TIMELINE;
                    entry.timeline.drain(..drain);
                }
            }
        }
    }

    // rooms.leave — remove rooms the user left or was kicked from.
    if let Some(leave_map) = rooms_val
        .and_then(|r| r.get("leave"))
        .and_then(|l| l.as_object())
    {
        for room_id in leave_map.keys() {
            if cache.rooms.remove(room_id).is_some() {
                changed = true;
            }
            if cache.invites.remove(room_id).is_some() {
                changed = true;
            }
        }
    }

    rebuild_display_map(cache);
    changed
}

/// Apply a single state event to a room's cached state.
fn apply_state_event(
    ev: &serde_json::Value,
    ev_type: &str,
    state_key: &str,
    entry: &mut RoomState,
    changed: &mut bool,
) {
    match ev_type {
        "m.room.name" => {
            if let Some(name) = ev
                .get("content")
                .and_then(|c| c.get("name"))
                .and_then(|n| n.as_str())
            {
                if !name.is_empty() && entry.display_name != name {
                    entry.display_name = name.to_owned();
                    *changed = true;
                }
            }
        }
        "m.room.create" => {
            if ev
                .get("content")
                .and_then(|c| c.get("type"))
                .and_then(|t| t.as_str())
                == Some("m.space")
            {
                if entry.kind != RoomKind::Space {
                    entry.kind = RoomKind::Space;
                    *changed = true;
                }
            }
        }
        "m.space.child" => {
            let child_id = state_key;
            let is_active = ev
                .get("content")
                .and_then(|c| c.get("via"))
                .and_then(|v| v.as_array())
                .map_or(false, |a| !a.is_empty());
            if is_active
                && !child_id.is_empty()
                && !entry.space_children.contains(&child_id.to_owned())
            {
                entry.space_children.push(child_id.to_owned());
                *changed = true;
            }
        }
        "m.room.member" => {
            let user_id = state_key;
            if user_id.is_empty() {
                return;
            }
            let membership = ev
                .get("content")
                .and_then(|c| c.get("membership"))
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_owned();
            if membership.is_empty() {
                return;
            }
            let display_name = ev
                .get("content")
                .and_then(|c| c.get("displayname"))
                .and_then(|d| d.as_str())
                .map(|s| s.to_owned());
            let member =
                Member { user_id: user_id.to_owned(), display_name, membership };
            entry.members.insert(user_id.to_owned(), member);
            *changed = true;
        }
        _ => {}
    }
}

/// Parse top-level account_data events and update the DM room sets in `cache`.
fn parse_account_data(json: &serde_json::Value, cache: &mut SyncCache, changed: &mut bool) {
    let Some(acc_events) = json
        .get("account_data")
        .and_then(|a| a.get("events"))
        .and_then(|e| e.as_array())
    else {
        return;
    };

    for ev in acc_events {
        if ev.get("type").and_then(|t| t.as_str()) != Some("m.direct") {
            continue;
        }
        let Some(content) = ev.get("content").and_then(|c| c.as_object()) else {
            continue;
        };
        let mut new_ids: HashSet<String> = HashSet::new();
        let mut new_to_user: HashMap<String, String> = HashMap::new();
        for (user_id, room_list) in content {
            if let Some(rooms) = room_list.as_array() {
                for room_val in rooms {
                    if let Some(room_id) = room_val.as_str() {
                        new_ids.insert(room_id.to_owned());
                        new_to_user.insert(room_id.to_owned(), user_id.clone());
                    }
                }
            }
        }
        if new_ids != cache.direct_room_ids {
            cache.direct_room_ids = new_ids;
            cache.direct_room_to_user = new_to_user;
            // Update is_dm on already-cached rooms.
            let direct_ids = &cache.direct_room_ids;
            for room in cache.rooms.values_mut() {
                room.is_dm = direct_ids.contains(&room.room_id);
            }
            *changed = true;
        }
        break; // only one m.direct event expected
    }
}

fn rebuild_display_map(cache: &mut SyncCache) {
    cache.display_to_id.clear();
    cache.invite_display_to_id.clear();
    for (room_id, state) in &cache.rooms {
        let key = match state.kind {
            RoomKind::Space => format!("[space] {}", state.display_name),
            RoomKind::Room => {
                if state.is_dm {
                    if let Some(partner) = cache.direct_room_to_user.get(room_id) {
                        format!("[dm] {}", partner)
                    } else {
                        state.display_name.clone()
                    }
                } else {
                    state.display_name.clone()
                }
            }
        };
        cache.display_to_id.insert(key, room_id.clone());
    }
    for (room_id, inv) in &cache.invites {
        let key = format!("[invite] {}", inv.display_name);
        cache.invite_display_to_id.insert(key, room_id.clone());
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
    /// `user_id` seeds `cache.self_user_id` for invite parsing.
    /// `file_lock` serialises settings-file writes with the main thread.
    pub fn start(
        &mut self,
        homeserver: String,
        access_token: String,
        config_path: Option<std::path::PathBuf>,
        user_id: String,
        file_lock: Arc<Mutex<()>>,
    ) {
        self.stop();

        let cache = Arc::clone(&self.cache);
        let notify = Arc::clone(&self.notify);
        let running = Arc::clone(&self.running);
        running.store(true, Ordering::Relaxed);

        self.thread = Some(std::thread::spawn(move || {
            sync_loop(homeserver, access_token, config_path, user_id, file_lock, cache, notify, running);
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
    user_id: String,
    file_lock: Arc<Mutex<()>>,
    cache: Arc<Mutex<SyncCache>>,
    notify: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) {
    while running.load(Ordering::Relaxed) {
        match run_sync_session(
            &homeserver,
            &access_token,
            &config_path,
            &user_id,
            &file_lock,
            &cache,
            &notify,
            &running,
        ) {
            Ok(()) => {}
            Err(e) => eprintln!("chatclient_sync: session error: {e}"),
        }

        if !running.load(Ordering::Relaxed) {
            break;
        }

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
    user_id: &str,
    file_lock: &Arc<Mutex<()>>,
    cache: &Arc<Mutex<SyncCache>>,
    notify: &Arc<AtomicBool>,
    running: &Arc<AtomicBool>,
) -> Result<(), String> {
    // Seed self_user_id into the cache if provided.
    if !user_id.is_empty() {
        let mut locked = cache.lock().unwrap();
        if locked.self_user_id.is_empty() {
            locked.self_user_id = user_id.to_owned();
        }
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent("sicompass/1.0")
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    let base = homeserver.trim_end_matches('/');

    loop {
        if !running.load(Ordering::Relaxed) {
            return Ok(());
        }

        let since = cache.lock().unwrap().next_batch.clone();
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
                persist_next_batch(config_path, &next_batch, file_lock);
            }
        }
    }
}

/// Write `next_batch` into settings.json under `"chat client"."chatSyncNextBatch"`.
///
/// `file_lock` must be the same mutex used by `save_setting` on the main thread
/// to serialise all read-modify-write cycles on the settings file.
fn persist_next_batch(config_path: &Option<std::path::PathBuf>, token: &str, file_lock: &Arc<Mutex<()>>) {
    use serde_json::{Map, Value};
    let Some(path) = config_path else { return };
    let _guard = file_lock.lock().unwrap();
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
            .map(|(eid, sender, body)| {
                serde_json::json!({
                    "type": "m.room.message",
                    "event_id": eid,
                    "sender": sender,
                    "content": { "body": body },
                    "origin_server_ts": 1000u64
                })
            })
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
            make_sync_response(
                "s1",
                "!r:s",
                None,
                &[("$e1", "@a:s", "first"), ("$e2", "@a:s", "second")],
            ),
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
        cache.next_batch = "s1".to_owned();
        cache.rooms.insert(
            "!r:s".to_owned(),
            RoomState {
                room_id: "!r:s".to_owned(),
                display_name: "Room".to_owned(),
                timeline: vec![TimelineEvent {
                    event_id: "$e1".to_owned(),
                    sender: "@a:s".to_owned(),
                    body: "hi".to_owned(),
                    origin_server_ts: 1000,
                }],
                ..Default::default()
            },
        );
        let json = make_sync_response("s1", "!r:s", None, &[("$e1", "@a:s", "hi")]);
        assert!(!parse_sync_response(json, &mut cache));
    }

    #[test]
    fn sync_controller_start_stop_noop_without_token() {
        let cache = Arc::new(Mutex::new(SyncCache::default()));
        let notify = Arc::new(AtomicBool::new(false));
        let mut ctrl = SyncController::new(Arc::clone(&cache), Arc::clone(&notify));
        ctrl.start(
            "http://127.0.0.1:1".to_owned(),
            "tok".to_owned(),
            None,
            String::new(),
            Arc::new(Mutex::new(())),
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
        ctrl.start("http://127.0.0.1:1".to_owned(), "tok".to_owned(), None, String::new(), Arc::new(Mutex::new(())));
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

    // ---- New tests -----------------------------------------------------------

    #[test]
    fn parse_sync_response_captures_invite() {
        let mut cache = SyncCache::default();
        let json = serde_json::json!({
            "next_batch": "s1",
            "rooms": {
                "invite": {
                    "!inv:server": {
                        "invite_state": { "events": [
                            { "type": "m.room.name", "state_key": "", "content": { "name": "Party" } }
                        ]}
                    }
                }
            }
        });
        parse_sync_response(json, &mut cache);
        assert!(cache.invites.contains_key("!inv:server"));
        assert_eq!(cache.invites["!inv:server"].display_name, "Party");
        assert_eq!(cache.invite_display_to_id["[invite] Party"], "!inv:server");
    }

    #[test]
    fn parse_sync_response_captures_inviter() {
        let mut cache = SyncCache::default();
        cache.self_user_id = "@bob:server".to_owned();
        let json = serde_json::json!({
            "next_batch": "s1",
            "rooms": {
                "invite": {
                    "!inv:server": {
                        "invite_state": { "events": [
                            {
                                "type": "m.room.member",
                                "state_key": "@bob:server",
                                "sender": "@alice:server",
                                "content": { "membership": "invite" }
                            }
                        ]}
                    }
                }
            }
        });
        parse_sync_response(json, &mut cache);
        assert_eq!(cache.invites["!inv:server"].inviter, "@alice:server");
    }

    #[test]
    fn parse_sync_response_clears_invite_on_join() {
        let mut cache = SyncCache::default();
        cache.invites.insert(
            "!inv:server".to_owned(),
            InviteState {
                room_id: "!inv:server".to_owned(),
                display_name: "Party".to_owned(),
                inviter: "@alice:server".to_owned(),
            },
        );
        let json = make_sync_response("s1", "!inv:server", Some("Party"), &[]);
        parse_sync_response(json, &mut cache);
        assert!(!cache.invites.contains_key("!inv:server"));
        assert!(cache.rooms.contains_key("!inv:server"));
    }

    #[test]
    fn parse_sync_response_drops_left_rooms() {
        let mut cache = SyncCache::default();
        cache.rooms.insert(
            "!old:s".to_owned(),
            RoomState {
                room_id: "!old:s".to_owned(),
                display_name: "Old Room".to_owned(),
                ..Default::default()
            },
        );
        let json = serde_json::json!({
            "next_batch": "s2",
            "rooms": {
                "leave": { "!old:s": {} }
            }
        });
        parse_sync_response(json, &mut cache);
        assert!(!cache.rooms.contains_key("!old:s"));
    }

    #[test]
    fn parse_sync_response_marks_space_kind() {
        let mut cache = SyncCache::default();
        let json = serde_json::json!({
            "next_batch": "s1",
            "rooms": {
                "join": {
                    "!space:server": {
                        "state": { "events": [
                            { "type": "m.room.name", "state_key": "", "content": { "name": "Work" } },
                            { "type": "m.room.create", "state_key": "", "content": { "type": "m.space" } }
                        ]},
                        "timeline": { "events": [] }
                    }
                }
            }
        });
        parse_sync_response(json, &mut cache);
        assert_eq!(cache.rooms["!space:server"].kind, RoomKind::Space);
        assert_eq!(cache.display_to_id["[space] Work"], "!space:server");
    }

    #[test]
    fn parse_sync_response_collects_space_children() {
        let mut cache = SyncCache::default();
        let json = serde_json::json!({
            "next_batch": "s1",
            "rooms": {
                "join": {
                    "!space:server": {
                        "state": { "events": [
                            { "type": "m.room.create", "state_key": "", "content": { "type": "m.space" } },
                            { "type": "m.space.child", "state_key": "!general:server", "content": { "via": ["server"] } },
                            { "type": "m.space.child", "state_key": "!eng:server", "content": { "via": ["server"] } }
                        ]},
                        "timeline": { "events": [] }
                    }
                }
            }
        });
        parse_sync_response(json, &mut cache);
        let children = &cache.rooms["!space:server"].space_children;
        assert!(children.contains(&"!general:server".to_owned()));
        assert!(children.contains(&"!eng:server".to_owned()));
    }

    #[test]
    fn parse_sync_response_captures_members() {
        let mut cache = SyncCache::default();
        let json = serde_json::json!({
            "next_batch": "s1",
            "rooms": {
                "join": {
                    "!r:s": {
                        "state": { "events": [
                            {
                                "type": "m.room.member",
                                "state_key": "@alice:s",
                                "content": {
                                    "membership": "join",
                                    "displayname": "Alice"
                                }
                            },
                            {
                                "type": "m.room.member",
                                "state_key": "@bob:s",
                                "content": { "membership": "invite" }
                            }
                        ]},
                        "timeline": { "events": [] }
                    }
                }
            }
        });
        parse_sync_response(json, &mut cache);
        let members = &cache.rooms["!r:s"].members;
        assert!(members.contains_key("@alice:s"));
        assert_eq!(members["@alice:s"].membership, "join");
        assert_eq!(members["@alice:s"].display_name, Some("Alice".to_owned()));
        assert!(members.contains_key("@bob:s"));
        assert_eq!(members["@bob:s"].membership, "invite");
    }

    #[test]
    fn parse_sync_response_marks_dm_from_account_data() {
        let mut cache = SyncCache::default();
        // First get the room into the cache
        let join_json = make_sync_response("s1", "!dm:s", Some("DM Room"), &[]);
        parse_sync_response(join_json, &mut cache);
        assert!(!cache.rooms["!dm:s"].is_dm);

        // Now receive m.direct
        let dm_json = serde_json::json!({
            "next_batch": "s2",
            "account_data": {
                "events": [{
                    "type": "m.direct",
                    "content": { "@alice:s": ["!dm:s"] }
                }]
            },
            "rooms": { "join": {} }
        });
        parse_sync_response(dm_json, &mut cache);
        assert!(cache.rooms["!dm:s"].is_dm);
        assert!(cache.direct_room_ids.contains("!dm:s"));
        assert_eq!(cache.direct_room_to_user["!dm:s"], "@alice:s");
    }

    #[test]
    fn parse_sync_response_space_child_inactive_not_added() {
        let mut cache = SyncCache::default();
        let json = serde_json::json!({
            "next_batch": "s1",
            "rooms": {
                "join": {
                    "!space:s": {
                        "state": { "events": [
                            { "type": "m.room.create", "state_key": "", "content": { "type": "m.space" } },
                            // via is empty array → inactive child
                            { "type": "m.space.child", "state_key": "!old:s", "content": { "via": [] } }
                        ]},
                        "timeline": { "events": [] }
                    }
                }
            }
        });
        parse_sync_response(json, &mut cache);
        assert!(cache.rooms["!space:s"].space_children.is_empty());
    }

    #[test]
    fn parse_sync_response_invite_display_uses_room_id_when_no_name() {
        let mut cache = SyncCache::default();
        let json = serde_json::json!({
            "next_batch": "s1",
            "rooms": {
                "invite": {
                    "!nameless:server": {
                        "invite_state": { "events": [] }
                    }
                }
            }
        });
        parse_sync_response(json, &mut cache);
        assert_eq!(cache.invites["!nameless:server"].display_name, "!nameless:server");
        assert!(cache.invite_display_to_id.contains_key("[invite] !nameless:server"));
    }
}
