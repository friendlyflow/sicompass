//! FFON mutation: `update_state` rebuilds the FFON tree on every recordable
//! task; `record_entry` / `walk_back` / `walk_forward` drive the per-tab
//! `Timeline` for undo/redo.

use crate::app_state::{
    AppRenderer, Coordinate, History, Task, TEXT_CHUNK_IDLE_MS, TIMELINE_CAPACITY,
};
use crate::list;
use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray, next_layer_exists};
use sicompass_sdk::timeline::{StructuralOp, StructuralPayload, TimelineEntry};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Commit a task into the FFON tree, record undo history, rebuild the list.
pub fn update_state(r: &mut AppRenderer, task: Task, history: History) {
    // Capture position before modification (needed for undo of DELETE)
    let history_id = r.current_id.clone();

    // Capture element BEFORE modification (for undo of DELETE / INPUT / PASTE)
    let prev_element = if history == History::None
        && matches!(task, Task::Delete | Task::Input | Task::Paste)
    {
        get_element_at(&r.ffon, &r.current_id).cloned()
    } else {
        None
    };

    // Determine "line" — what we're editing/inserting
    let (line, current_elem_is_obj) = get_current_line(r);
    let is_key = is_line_key(&line) || current_elem_is_obj;

    update_ids(r, is_key, task, history);
    update_ffon(r, &line, is_key, task, history);

    // Capture element AFTER modification (for undo of APPEND / INSERT / INPUT / PASTE)
    let new_element = if history == History::None
        && matches!(
            task,
            Task::Append
                | Task::AppendAppend
                | Task::Insert
                | Task::InsertInsert
                | Task::Input
                | Task::Paste
        ) {
        get_element_at(&r.ffon, &r.current_id).cloned()
    } else {
        None
    };

    let record_id = if matches!(task, Task::Delete | Task::Cut | Task::Paste) {
        history_id
    } else {
        r.current_id.clone()
    };

    // Dual-write a TimelineEntry for Task::Input / Append / Insert / Delete /
    // Cut / Paste. The unified `record_entry` runs alongside the legacy
    // `update_history` stack until the gating flag flips in step 11.
    let timeline_payload = if history == History::None {
        match task {
            Task::Input => match (&prev_element, &new_element) {
                (Some(before), Some(after)) => Some(TimelineEntry::TextChunk {
                    id: record_id.clone(),
                    before: before.clone(),
                    after: after.clone(),
                    chunk_seq: 0,
                }),
                _ => None,
            },
            Task::Append | Task::AppendAppend => new_element.as_ref().map(|e| {
                TimelineEntry::Structural {
                    id: record_id.clone(),
                    op: StructuralOp::Append,
                    payload: StructuralPayload::Inserted(e.clone()),
                }
            }),
            Task::Insert | Task::InsertInsert => new_element.as_ref().map(|e| {
                TimelineEntry::Structural {
                    id: record_id.clone(),
                    op: StructuralOp::Insert,
                    payload: StructuralPayload::Inserted(e.clone()),
                }
            }),
            Task::Delete => prev_element.as_ref().map(|e| TimelineEntry::Structural {
                id: record_id.clone(),
                op: StructuralOp::Delete,
                payload: StructuralPayload::Removed(e.clone()),
            }),
            Task::Cut => prev_element.as_ref().map(|e| TimelineEntry::Structural {
                id: record_id.clone(),
                op: StructuralOp::Cut,
                payload: StructuralPayload::Removed(e.clone()),
            }),
            Task::Paste => match (&prev_element, &new_element) {
                (Some(before), Some(after)) => Some(TimelineEntry::Structural {
                    id: record_id.clone(),
                    op: StructuralOp::Paste,
                    payload: StructuralPayload::Pasted {
                        before: before.clone(),
                        after: after.clone(),
                    },
                }),
                _ => None,
            },
            _ => None,
        }
    } else {
        None
    };

    let _ = record_id;
    let _ = prev_element;
    let _ = new_element;

    if let Some(entry) = timeline_payload {
        record_entry(r, entry);
    }

    list::create_list_current_layer(r);
}

// ---------------------------------------------------------------------------
// updateIds
// ---------------------------------------------------------------------------

pub fn update_ids(r: &mut AppRenderer, is_key: bool, task: Task, history: History) {
    r.previous_id = r.current_id.clone();

    if matches!(history, History::Undo | History::Redo) {
        return;
    }

    let max_id = get_ffon_max_id_at_path(&r.ffon, &r.current_id);
    let current_idx = r.current_id.last().unwrap_or(0);
    let depth = r.current_id.depth();

    match task {
        Task::ArrowUp => {
            if current_idx > 0 {
                r.current_id.set_last(current_idx - 1);
            }
        }
        Task::ArrowDown => {
            if current_idx < max_id {
                r.current_id.set_last(current_idx + 1);
            }
        }
        Task::ArrowLeft => {
            if depth > 1 {
                r.current_id.pop();
            }
        }
        Task::ArrowRight => {
            if next_layer_exists(&r.ffon, &r.previous_id) {
                r.current_id.push(0);
            }
        }
        Task::Append => {
            if r.coordinate.is_general() {
                if !is_key {
                    r.current_id.set_last(current_idx + 1);
                } else if next_layer_exists(&r.ffon, &r.previous_id) {
                    r.current_id.set_last(current_idx + 1);
                } else {
                    r.current_id.push(0);
                }
            }
        }
        Task::AppendAppend => {
            if r.coordinate.is_general() {
                r.current_id.set_last(max_id + 1);
            }
        }
        Task::Insert => {
            // Position stays the same
        }
        Task::InsertInsert => {
            if r.coordinate.is_general() {
                r.current_id.set_last(0);
            }
        }
        Task::Delete => {
            // Position handled in update_ffon
        }
        Task::Input => {
            // Position stays the same
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// updateFfon
// ---------------------------------------------------------------------------

pub fn update_ffon(r: &mut AppRenderer, line: &str, is_key: bool, task: Task, history: History) {
    if r.previous_id.depth() == 0 {
        return;
    }

    // Handle empty root structure
    if r.ffon.is_empty() && r.previous_id.depth() == 1 {
        if matches!(
            task,
            Task::Append | Task::AppendAppend | Task::Insert | Task::InsertInsert | Task::Input
        ) {
            let new_elem = if is_key {
                let mut obj = FfonElement::new_obj(strip_trailing_colon(line));
                obj.as_obj_mut().unwrap().push(FfonElement::new_str(""));
                obj
            } else {
                FfonElement::new_str(line)
            };
            r.ffon.push(new_elem);
        }
        return;
    }

    // Paste: replace current element with clipboard content
    if task == Task::Paste {
        let prev_id = r.previous_id.clone();
        let prev_idx = match prev_id.last() { Some(i) => i, None => return };
        if let Some(elem) = r.clipboard.clone() {
            replace_at(&mut r.ffon, &prev_id, prev_idx, elem);
        }
        return;
    }

    let is_editor = r.current_id.get(0)
        .and_then(|i| r.providers.get(i))
        .map(|p| p.has_editor_semantics())
        .unwrap_or(false);

    // Clone the ids so we can still mutate r.current_id
    let prev_id = r.previous_id.clone();
    let cur_id = r.current_id.clone();

    // Get the target index (last component of prev_id)
    let prev_idx = match prev_id.last() {
        Some(i) => i,
        None => return,
    };
    let cur_idx_last = cur_id.last().unwrap_or(0);

    // Navigate to the parent array (the array containing the element at prev_idx)
    // We need to get a mutable reference. We do this in a separate scope to avoid
    // fighting with the borrow checker on `r`.
    // Strategy: get depth, clone the path, then do the mutation on r.ffon.

    let depth = prev_id.depth();

    if is_key && is_editor {
        // Check if element at prev_idx is an Obj
        let is_obj_at_prev = {
            let arr = navigate_to_slice(&r.ffon, &prev_id);
            arr.and_then(|a| a.get(prev_idx)).map_or(false, |e| e.is_obj())
        };

        if matches!(task, Task::Delete | Task::Cut) {
            // Remove element at prev_idx
            let removed = remove_at(&mut r.ffon, &prev_id, prev_idx);
            let new_len = get_parent_len(&r.ffon, &prev_id);
            if depth != 1 && new_len == 0 {
                // Insert empty string if this was the only child
                insert_at(&mut r.ffon, &prev_id, 0, FfonElement::new_str(""));
            } else if new_len > 0 {
                // Move cursor to previous element
                if prev_idx > 0 {
                    r.current_id.set_last(prev_idx - 1);
                }
            }
        } else if is_obj_at_prev {
            // Element is already an Obj — re-key it
            let new_key = strip_trailing_colon(line);
            rekey_obj_at(&mut r.ffon, &prev_id, prev_idx, new_key);

            if matches!(task, Task::Append | Task::AppendAppend | Task::Insert | Task::InsertInsert)
                && history != History::Redo
            {
                // Insert a new empty sibling at cur_idx_last
                insert_at(&mut r.ffon, &prev_id, cur_idx_last, FfonElement::new_str(""));
            }
        } else {
            // Element is a string — convert it to an Obj
            let new_key = strip_trailing_colon(line);
            let has_children = next_layer_exists(&r.ffon, &prev_id);
            let new_elem = if has_children {
                // Preserve existing children
                let children = {
                    let arr = navigate_to_slice(&r.ffon, &prev_id);
                    arr.and_then(|a| a.get(prev_idx))
                        .and_then(|e| e.as_obj())
                        .map(|o| o.children.clone())
                        .unwrap_or_default()
                };
                let mut obj = FfonElement::new_obj(new_key);
                for c in children {
                    obj.as_obj_mut().unwrap().push(c);
                }
                obj
            } else {
                let mut obj = FfonElement::new_obj(new_key);
                obj.as_obj_mut().unwrap().push(FfonElement::new_str(""));
                obj
            };
            replace_at(&mut r.ffon, &prev_id, prev_idx, new_elem);

            if matches!(task, Task::Append | Task::AppendAppend | Task::Insert | Task::InsertInsert)
                && history != History::Redo
            {
                insert_at(&mut r.ffon, &prev_id, cur_idx_last, FfonElement::new_str(""));
            }
        }
    } else if !is_key && is_editor {
        if matches!(task, Task::Delete | Task::Cut) {
            remove_at(&mut r.ffon, &prev_id, prev_idx);
            let new_len = get_parent_len(&r.ffon, &prev_id);
            // If cursor is at 0 and list is now empty at non-root level, insert placeholder
            if cur_id.last().unwrap_or(1) == 0 && new_len == 0 && depth != 1 {
                insert_at(&mut r.ffon, &prev_id, prev_id.last().unwrap_or(0), FfonElement::new_str(""));
            }
            if r.current_id.last().unwrap_or(0) > 0 {
                let cur = r.current_id.last().unwrap_or(1);
                r.current_id.set_last(cur - 1);
            }
        } else if matches!(
            task,
            Task::Append | Task::AppendAppend | Task::Insert | Task::InsertInsert
        ) {
            // Set the current element to `line`
            replace_at(&mut r.ffon, &prev_id, prev_idx, FfonElement::new_str(line));
            // Insert empty string sibling at cur_idx_last
            if history != History::Redo {
                insert_at(&mut r.ffon, &prev_id, cur_idx_last, FfonElement::new_str(""));
            }
        } else {
            // TASK_INPUT / navigation — update the value
            replace_at(&mut r.ffon, &prev_id, prev_idx, FfonElement::new_str(line));
        }
    } else if matches!(task, Task::Delete | Task::Cut) {
        // Non-editor delete/cut (e.g. file browser in General)
        remove_at(&mut r.ffon, &prev_id, prev_idx);
        let new_len = get_parent_len(&r.ffon, &prev_id);
        if new_len == 0 {
            // Insert placeholder
            let placeholder = FfonElement::new_str("<input></input>");
            insert_at(&mut r.ffon, &prev_id, 0, placeholder);
            r.current_id.set_last(0);
        } else if r.current_id.last().unwrap_or(0) > 0 {
            let cur = r.current_id.last().unwrap_or(1);
            r.current_id.set_last(cur - 1);
        }
    }
}


// ---------------------------------------------------------------------------
// Unified Timeline dispatcher (new model)
// ---------------------------------------------------------------------------

/// Push a `TimelineEntry` onto the active tab's timeline, applying coalescing
/// rules for `TextChunk` (≤500 ms idle on same id) and `Navigate` (consecutive
/// arrow keys merge into one entry).
///
/// Called for every reversible action; the legacy `update_history` stack is
/// dual-written alongside this one until the migration completes.
pub fn record_entry(r: &mut AppRenderer, entry: TimelineEntry) {
    if r.in_history_action {
        // Side-effect emission during undo/redo — discard so the original
        // entry remains the next redo target.
        return;
    }
    let tl = r.active_timeline_mut();

    // Truncate the redo branch on a new action. The truncation itself implies
    // a branch off the timeline, so disable coalescing for this push.
    let mut just_branched = false;
    if tl.position > 0 {
        let new_count = tl.entries.len().saturating_sub(tl.position);
        tl.entries.truncate(new_count);
        tl.position = 0;
        just_branched = true;
    }
    // A walk_back / walk_forward without truncation (position landed on an
    // existing entry) sets the same break.
    let break_coalesce = just_branched || std::mem::take(&mut tl.coalesce_break);

    // Navigate coalescing: a burst of arrow keys is one entry.
    //
    // Coalesce only when the provider context is consistent — a Navigate that
    // crosses from "no path tracking" (e.g. root provider-list navigation) into
    // "path tracking" (e.g. descending into filebrowser) must NOT swallow the
    // newer entry's `from_path`. Otherwise the merged entry's `from_path` is
    // `None`, and undo cannot restore the filebrowser's pre-descent path.
    if !break_coalesce {
        if let TimelineEntry::Navigate {
            provider_idx,
            to_id,
            to_path,
            kind,
            ..
        } = &entry
        {
            // Coalesce same-provider Navigates only when no path change is
            // happening: an idle scroll (same path before/after, or both `None`)
            // collapses into one entry, but a Right-press that descends into a
            // subdirectory (path changes) is always its own undo step.
            let new_has_path = to_path.is_some();
            let can_coalesce = matches!(
                tl.entries.last(),
                Some(TimelineEntry::Navigate {
                    provider_idx: tail_pidx,
                    to_path: tail_tp,
                    ..
                }) if tail_pidx == provider_idx
                    && tail_tp.is_some() == new_has_path
                    && tail_tp.as_ref() == to_path.as_ref()
            );
            if can_coalesce {
                if let Some(TimelineEntry::Navigate {
                    to_id: tail_to_id,
                    to_path: tail_to_path,
                    kind: tail_kind,
                    ..
                }) = tl.entries.last_mut()
                {
                    *tail_to_id = to_id.clone();
                    *tail_to_path = to_path.clone();
                    *tail_kind = *kind;
                    // Any navigation breaks an in-flight text-chunk run.
                    tl.last_text_id = None;
                    tl.last_text_edit_at = None;
                    return;
                }
            }
        }
    }

    // TextChunk coalescing: same id within TEXT_CHUNK_IDLE_MS extends the tail.
    if !break_coalesce {
        if let TimelineEntry::TextChunk { id, after, .. } = &entry {
            let now = Instant::now();
            let coalesce = tl.last_text_id.as_ref() == Some(id)
                && tl
                    .last_text_edit_at
                    .map(|t| (now - t).as_millis() as u64 <= TEXT_CHUNK_IDLE_MS)
                    .unwrap_or(false);
            if coalesce {
                if let Some(TimelineEntry::TextChunk {
                    after: tail_after, ..
                }) = tl.entries.last_mut()
                {
                    *tail_after = after.clone();
                    tl.last_text_edit_at = Some(now);
                    return;
                }
            }
        }
    }

    // Update text-tracking based on the new entry kind.
    match &entry {
        TimelineEntry::TextChunk { id, .. } => {
            tl.last_text_id = Some(id.clone());
            tl.last_text_edit_at = Some(Instant::now());
        }
        _ => {
            tl.last_text_id = None;
            tl.last_text_edit_at = None;
        }
    }

    // Cap the timeline by dropping the oldest entry.
    if tl.entries.len() >= TIMELINE_CAPACITY {
        tl.entries.remove(0);
    }

    // Assign a chunk_seq for TextChunk entries.
    let entry = match entry {
        TimelineEntry::TextChunk {
            id, before, after, ..
        } => {
            let seq = tl.next_chunk_seq;
            tl.next_chunk_seq = tl.next_chunk_seq.wrapping_add(1);
            TimelineEntry::TextChunk {
                id,
                before,
                after,
                chunk_seq: seq,
            }
        }
        other => other,
    };

    tl.entries.push(entry);
    tl.position = 0;
}

/// Apply one undo step to the active tab's timeline.
pub fn walk_back(r: &mut AppRenderer) {
    if r.active_timeline().entries.is_empty() {
        r.error_message = "No undo history".to_owned();
        return;
    }
    if r.active_timeline().position >= r.active_timeline().entries.len() {
        r.error_message = "Nothing to undo".to_owned();
        return;
    }

    crate::handlers::handle_escape(r);

    let entry = {
        let tl = r.active_timeline_mut();
        tl.position += 1;
        tl.coalesce_break = true;
        let idx = tl.entries.len() - tl.position;
        tl.entries[idx].clone()
    };

    r.in_history_action = true;
    apply_undo(r, &entry);
    discard_provider_emissions(r);
    r.in_history_action = false;

    list::create_list_current_layer(r);
    r.list_index = r.current_id.last().unwrap_or(0);
    r.scroll_offset = 0;
}

/// Drain (and throw away) any TimelineEntry that providers emitted as a side
/// effect of `walk_back` / `walk_forward`.
fn discard_provider_emissions(r: &mut AppRenderer) {
    for p in r.providers.iter_mut() {
        let _ = p.take_timeline_entries();
    }
}

/// Apply one redo step to the active tab's timeline.
pub fn walk_forward(r: &mut AppRenderer) {
    if r.active_timeline().entries.is_empty() {
        r.error_message = "No undo history".to_owned();
        return;
    }
    if r.active_timeline().position == 0 {
        r.error_message = "Nothing to redo".to_owned();
        return;
    }

    crate::handlers::handle_escape(r);

    let entry = {
        let tl = r.active_timeline_mut();
        let idx = tl.entries.len() - tl.position;
        tl.position -= 1;
        tl.coalesce_break = true;
        tl.entries[idx].clone()
    };

    r.in_history_action = true;
    apply_redo(r, &entry);
    discard_provider_emissions(r);
    r.in_history_action = false;

    list::create_list_current_layer(r);
    r.list_index = r.current_id.last().unwrap_or(0);
    r.scroll_offset = 0;
}

fn apply_undo(r: &mut AppRenderer, entry: &TimelineEntry) {
    match entry {
        TimelineEntry::Navigate {
            provider_idx,
            from_id, from_path, ..
        } => {
            if let Some(path) = from_path {
                // Only refresh-on-navigate providers (filebrowser, email) need
                // the FFON re-fetched after a path swap. For in-memory
                // providers (tutorial, script-based) the FFON is preloaded;
                // refreshing would call fetch() at the new path, replace the
                // provider root with whatever that returns, and make the
                // restored `from_id` index into the wrong tree — which makes
                // the rendered list look empty after undo.
                let does_refresh = r.providers
                    .get(*provider_idx)
                    .map(|p| p.refresh_on_navigate())
                    .unwrap_or(false);
                if does_refresh {
                    let mut root_id = IdArray::new();
                    root_id.push(*provider_idx);
                    r.current_id = root_id;
                    crate::provider::set_provider_path(r, path);
                    crate::provider::refresh_current_directory(r);
                } else {
                    crate::provider::set_provider_path(r, path);
                }
            }
            r.current_id = from_id.clone();
        }
        TimelineEntry::TextChunk { id, before, .. } => {
            replace_element_at_id(r, id, before.clone());
            r.current_id = id.clone();
            sync_compose_body_if_body_element(r, id);
        }
        TimelineEntry::Structural { id, op, payload } => match (op, payload) {
            (StructuralOp::Append | StructuralOp::Insert, _) => {
                r.current_id = id.clone();
                crate::handlers::handle_delete(r, History::Undo);
                upgrade_body_bare_placeholder(r, id);
                sync_compose_body_if_body_element(r, id);
                if r.current_id.last().unwrap_or(0) > 0 {
                    let cur = r.current_id.last().unwrap_or(1);
                    r.current_id.set_last(cur - 1);
                }
            }
            (
                StructuralOp::Delete | StructuralOp::Cut,
                StructuralPayload::Removed(elem),
            ) => {
                clear_sole_i_placeholder_if_body_element(r, id);
                insert_element_at_id(r, id, elem.clone());
                r.current_id = id.clone();
                sync_compose_body_if_body_element(r, id);
            }
            (StructuralOp::Paste, StructuralPayload::Pasted { before, .. }) => {
                replace_element_at_id(r, id, before.clone());
                r.current_id = id.clone();
            }
            (StructuralOp::Replace, StructuralPayload::Replaced { before, .. }) => {
                replace_element_at_id(r, id, before.clone());
                r.current_id = cursor_inside_radio(id, before);
            }
            _ => {}
        },
        TimelineEntry::FsOp {
            provider_idx,
            id,
            op,
            before,
            after,
            ..
        } => {
            // Delete / Move carry an `FsSideEffect` snapshot that only the
            // provider knows how to reverse (write bytes back to disk,
            // recreate a directory tree). Dispatch to `provider.undo` first
            // so the disk-side restore runs, then fall through to the FFON
            // reinsertion arm so the in-memory tree mirrors the file system.
            if matches!(
                op,
                sicompass_sdk::timeline::FsOpKind::Delete
                    | sicompass_sdk::timeline::FsOpKind::Move
            ) {
                let mut error = String::new();
                if let Some(p) = r.providers.get_mut(*provider_idx) {
                    p.undo(entry, &mut error);
                }
                if !error.is_empty() {
                    r.error_message = error;
                }
            }
            apply_fs_op_undo(r, id, *op, before.as_ref(), after.as_ref());
        }
        TimelineEntry::ImapOp { provider_idx, .. }
        | TimelineEntry::ChatOp { provider_idx, .. }
        | TimelineEntry::ProviderOp { provider_idx, .. } => {
            dispatch_provider_undo(r, *provider_idx, entry);
        }
    }
}

fn apply_redo(r: &mut AppRenderer, entry: &TimelineEntry) {
    match entry {
        TimelineEntry::Navigate {
            provider_idx,
            to_id, to_path, ..
        } => {
            if let Some(path) = to_path {
                // Same `refresh_on_navigate` gate as apply_undo above —
                // in-memory providers should not lose their preloaded FFON
                // on redo.
                let does_refresh = r.providers
                    .get(*provider_idx)
                    .map(|p| p.refresh_on_navigate())
                    .unwrap_or(false);
                if does_refresh {
                    let mut root_id = IdArray::new();
                    root_id.push(*provider_idx);
                    r.current_id = root_id;
                    crate::provider::set_provider_path(r, path);
                    crate::provider::refresh_current_directory(r);
                } else {
                    crate::provider::set_provider_path(r, path);
                }
            }
            r.current_id = to_id.clone();
        }
        TimelineEntry::TextChunk { id, after, .. } => {
            replace_element_at_id(r, id, after.clone());
            r.current_id = id.clone();
            sync_compose_body_if_body_element(r, id);
        }
        TimelineEntry::Structural { id, op, payload } => match (op, payload) {
            (
                StructuralOp::Append | StructuralOp::Insert,
                StructuralPayload::Inserted(elem),
            ) => {
                clear_sole_i_placeholder_if_body_element(r, id);
                insert_element_at_id(r, id, elem.clone());
                r.current_id = id.clone();
                sync_compose_body_if_body_element(r, id);
            }
            (StructuralOp::Delete | StructuralOp::Cut, _) => {
                r.current_id = id.clone();
                crate::handlers::handle_delete(r, History::Redo);
                upgrade_body_bare_placeholder(r, id);
                sync_compose_body_if_body_element(r, id);
                let count = get_parent_len(&r.ffon, id);
                if let Some(idx) = r.current_id.last() {
                    if idx >= count && count > 0 {
                        r.current_id.set_last(count - 1);
                    }
                }
            }
            (StructuralOp::Paste, StructuralPayload::Pasted { after, .. }) => {
                replace_element_at_id(r, id, after.clone());
                r.current_id = id.clone();
            }
            (StructuralOp::Replace, StructuralPayload::Replaced { after, .. }) => {
                replace_element_at_id(r, id, after.clone());
                r.current_id = cursor_inside_radio(id, after);
            }
            _ => {}
        },
        TimelineEntry::FsOp {
            provider_idx,
            id,
            op,
            before,
            after,
            ..
        } => {
            // Mirror the undo side: provider.redo handles the disk-side
            // re-application for Delete/Move (re-trashes a restored file,
            // re-moves a message).
            if matches!(
                op,
                sicompass_sdk::timeline::FsOpKind::Delete
                    | sicompass_sdk::timeline::FsOpKind::Move
            ) {
                let mut error = String::new();
                if let Some(p) = r.providers.get_mut(*provider_idx) {
                    p.redo(entry, &mut error);
                }
                if !error.is_empty() {
                    r.error_message = error;
                }
            }
            apply_fs_op_redo(r, id, *op, before.as_ref(), after.as_ref());
        }
        TimelineEntry::ImapOp { provider_idx, .. }
        | TimelineEntry::ChatOp { provider_idx, .. }
        | TimelineEntry::ProviderOp { provider_idx, .. } => {
            dispatch_provider_redo(r, *provider_idx, entry);
        }
    }
}

fn apply_fs_op_undo(
    r: &mut AppRenderer,
    id: &IdArray,
    op: sicompass_sdk::timeline::FsOpKind,
    before: Option<&FfonElement>,
    after: Option<&FfonElement>,
) {
    use sicompass_sdk::timeline::FsOpKind;
    match op {
        FsOpKind::Create => {
            if let Some(elem) = after {
                let name = elem_key_str(elem);
                let is_dir = matches!(elem, FfonElement::Obj(_));
                if is_dir {
                    let cur_path = crate::provider::current_path(r).to_owned();
                    let tail = format!("/{}", name);
                    if cur_path.ends_with(&tail) || cur_path == name {
                        crate::provider::pop_path(r);
                    }
                }
                while r.current_id.depth() > id.depth() && r.current_id.depth() > 1 {
                    crate::provider::pop_path(r);
                    r.current_id.pop();
                }
                r.current_id = id.clone();
                crate::provider::delete_item_by_name(r, &name);
                crate::provider::refresh_current_directory(r);
                let parent_len = get_parent_len(&r.ffon, id);
                if parent_len == 0 {
                    insert_at(&mut r.ffon, id, 0, FfonElement::new_str("<input></input>"));
                    r.current_id.set_last(0);
                } else if let Some(idx) = r.current_id.last() {
                    if idx >= parent_len {
                        r.current_id.set_last(parent_len - 1);
                    }
                }
            }
        }
        FsOpKind::Rename => {
            if let (Some(prev_elem), Some(new_elem)) = (before, after) {
                let old_str = elem_key_str(new_elem);
                let new_str = elem_key_str(prev_elem);
                r.current_id = id.clone();
                crate::provider::commit_edit(r, &old_str, &new_str);
                replace_element_at_id(r, id, prev_elem.clone());
                crate::provider::refresh_current_directory(r);
                r.current_id = id.clone();
            }
        }
        FsOpKind::Paste => {
            if let Some(new_elem) = after {
                let name =
                    sicompass_sdk::tags::strip_display(&elem_key_str(new_elem)).to_owned();
                r.current_id = id.clone();
                crate::provider::delete_item_by_name(r, &name);
                crate::provider::refresh_current_directory(r);
                let parent_len = get_parent_len(&r.ffon, id);
                if parent_len == 0 {
                    insert_at(&mut r.ffon, id, 0, FfonElement::new_str("<input></input>"));
                    r.current_id.set_last(0);
                } else if let Some(idx) = r.current_id.last() {
                    if idx >= parent_len {
                        r.current_id.set_last(parent_len - 1);
                    }
                }
            }
        }
        FsOpKind::Delete | FsOpKind::Move => {
            // Full restore-from-trash logic lands in step 6 alongside the
            // lib_filebrowser emission. For now, just reinsert `before` into
            // FFON so tests can exercise the dispatcher.
            if let Some(prev_elem) = before {
                clear_sole_i_placeholder_if_body_element(r, id);
                insert_element_at_id(r, id, prev_elem.clone());
                r.current_id = id.clone();
            }
        }
    }
}

fn apply_fs_op_redo(
    r: &mut AppRenderer,
    id: &IdArray,
    op: sicompass_sdk::timeline::FsOpKind,
    before: Option<&FfonElement>,
    after: Option<&FfonElement>,
) {
    use sicompass_sdk::timeline::FsOpKind;
    match op {
        FsOpKind::Create => {
            if let Some(elem) = after {
                let name = elem_key_str(elem);
                let is_dir = matches!(elem, FfonElement::Obj(_));
                r.current_id = id.clone();
                if is_dir {
                    crate::provider::create_directory(r, &name);
                    crate::provider::push_path(r, &name);
                    crate::provider::refresh_current_directory(r);
                    let provider_idx = r.current_id.get(0).unwrap_or(0);
                    if let Some(root) = r.ffon.get_mut(provider_idx) {
                        if let Some(obj) = root.as_obj_mut() {
                            if obj.children.is_empty() {
                                obj.children
                                    .push(FfonElement::new_str("<input></input>"));
                            }
                        }
                    }
                    let mut new_id = IdArray::new();
                    new_id.push(provider_idx);
                    new_id.push(0);
                    r.current_id = new_id;
                } else {
                    crate::provider::create_file(r, &name);
                    crate::provider::refresh_current_directory(r);
                }
            }
        }
        FsOpKind::Rename => {
            if let (Some(prev_elem), Some(new_elem)) = (before, after) {
                let old_str = elem_key_str(prev_elem);
                let new_str = elem_key_str(new_elem);
                r.current_id = id.clone();
                crate::provider::commit_edit(r, &old_str, &new_str);
                replace_element_at_id(r, id, new_elem.clone());
                crate::provider::refresh_current_directory(r);
                r.current_id = id.clone();
            }
        }
        FsOpKind::Paste => {
            if let (Some(src_elem), Some(dest_elem)) = (before, after) {
                let src_path = elem_key_str(src_elem);
                let dest_name =
                    sicompass_sdk::tags::strip_display(&elem_key_str(dest_elem)).to_owned();
                let slash = src_path.rfind('/').unwrap_or(0);
                let src_dir = &src_path[..slash];
                let src_name = &src_path[slash + 1..];
                r.current_id = id.clone();
                let dest_dir = crate::provider::current_path(r).to_owned();
                crate::provider::copy_item(r, src_dir, src_name, &dest_dir, &dest_name);
                crate::provider::refresh_current_directory(r);
                r.current_id = id.clone();
            }
        }
        FsOpKind::Delete | FsOpKind::Move => {
            // Step 6 fills these arms once lib_filebrowser emits them.
            let _ = (id, before, after);
        }
    }
}

fn dispatch_provider_undo(r: &mut AppRenderer, provider_idx: usize, entry: &TimelineEntry) {
    let mut error = String::new();
    let pname = r
        .providers
        .get(provider_idx)
        .map(|p| p.display_name().to_owned());
    if let Some(p) = r.providers.get_mut(provider_idx) {
        p.undo(entry, &mut error);
    }
    if !error.is_empty() {
        r.error_message = error;
    } else if r.current_id.get(0) == Some(provider_idx) {
        crate::provider::refresh_current_directory(r);
    } else {
        r.error_message = format!(
            "undid on {}",
            pname.as_deref().unwrap_or("provider")
        );
    }
}

fn dispatch_provider_redo(r: &mut AppRenderer, provider_idx: usize, entry: &TimelineEntry) {
    let mut error = String::new();
    let pname = r
        .providers
        .get(provider_idx)
        .map(|p| p.display_name().to_owned());
    if let Some(p) = r.providers.get_mut(provider_idx) {
        p.redo(entry, &mut error);
    }
    if !error.is_empty() {
        r.error_message = error;
    } else if r.current_id.get(0) == Some(provider_idx) {
        crate::provider::refresh_current_directory(r);
    } else {
        r.error_message = format!(
            "redid on {}",
            pname.as_deref().unwrap_or("provider")
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers — FFON mutation primitives
// ---------------------------------------------------------------------------

/// Extract the key/string from an FfonElement (used by FsRename/FsPaste undo/redo).
fn elem_key_str(elem: &sicompass_sdk::ffon::FfonElement) -> String {
    match elem {
        sicompass_sdk::ffon::FfonElement::Str(s) => s.clone(),
        sicompass_sdk::ffon::FfonElement::Obj(o) => o.key.clone(),
    }
}

/// True if the last character of `line` is `':'`.
pub fn is_line_key(line: &str) -> bool {
    line.ends_with(':')
}

/// Strip a trailing `':'` from a string (for object key naming).
pub fn strip_trailing_colon(s: &str) -> String {
    if s.ends_with(':') {
        s[..s.len() - 1].to_owned()
    } else {
        s.to_owned()
    }
}


/// Get the element at `id` (immutable).
fn get_element_at<'a>(ffon: &'a [FfonElement], id: &IdArray) -> Option<&'a FfonElement> {
    let arr = navigate_to_slice(ffon, id)?;
    let idx = id.last()?;
    arr.get(idx)
}

/// Navigate to the parent slice (the array containing the element pointed to by `id`).
/// Returns `None` if navigation fails.
fn navigate_to_slice<'a>(ffon: &'a [FfonElement], id: &IdArray) -> Option<&'a [FfonElement]> {
    if id.depth() == 0 {
        return None;
    }
    let mut current = ffon;
    for d in 0..id.depth() - 1 {
        let idx = id.get(d)?;
        match current.get(idx)? {
            FfonElement::Obj(obj) => current = &obj.children,
            _ => return None,
        }
    }
    Some(current)
}

/// Navigate to the parent Vec (mutable) — public for use by handlers.
pub fn navigate_to_slice_pub<'a>(
    ffon: &'a mut Vec<FfonElement>,
    id: &IdArray,
) -> Option<&'a mut Vec<FfonElement>> {
    navigate_to_slice_mut(ffon, id)
}

/// Navigate to the parent Vec (mutable).
fn navigate_to_slice_mut<'a>(
    ffon: &'a mut Vec<FfonElement>,
    id: &IdArray,
) -> Option<&'a mut Vec<FfonElement>> {
    if id.depth() == 0 {
        return None;
    }
    let mut current = ffon;
    for d in 0..id.depth() - 1 {
        let idx = id.get(d)?;
        match current.get_mut(idx)? {
            FfonElement::Obj(obj) => current = &mut obj.children,
            _ => return None,
        }
    }
    Some(current)
}

/// Get the length of the parent array at `id`.
fn get_parent_len(ffon: &[FfonElement], id: &IdArray) -> usize {
    navigate_to_slice(ffon, id)
        .map(|s| s.len())
        .unwrap_or(0)
}

/// Maximum valid index at the current navigation path.
fn get_ffon_max_id_at_path(ffon: &[FfonElement], id: &IdArray) -> usize {
    navigate_to_slice(ffon, id)
        .map(|s| s.len().saturating_sub(1))
        .unwrap_or(0)
}

/// Replace element at `(parent_id, idx)` with `new_elem`.
fn replace_at(ffon: &mut Vec<FfonElement>, parent_id: &IdArray, idx: usize, new_elem: FfonElement) {
    if let Some(arr) = navigate_to_slice_mut(ffon, parent_id) {
        if idx < arr.len() {
            arr[idx] = new_elem;
        }
    }
}

/// Remove element at `(parent_id, idx)`.
fn remove_at(ffon: &mut Vec<FfonElement>, parent_id: &IdArray, idx: usize) -> Option<FfonElement> {
    let arr = navigate_to_slice_mut(ffon, parent_id)?;
    if idx < arr.len() {
        Some(arr.remove(idx))
    } else {
        None
    }
}

/// Insert element at `(parent_id, idx)`.
fn insert_at(ffon: &mut Vec<FfonElement>, parent_id: &IdArray, idx: usize, elem: FfonElement) {
    if let Some(arr) = navigate_to_slice_mut(ffon, parent_id) {
        let idx = idx.min(arr.len());
        arr.insert(idx, elem);
    }
}

/// Re-key an existing Obj at `(parent_id, idx)`.
fn rekey_obj_at(ffon: &mut Vec<FfonElement>, parent_id: &IdArray, idx: usize, new_key: String) {
    if let Some(arr) = navigate_to_slice_mut(ffon, parent_id) {
        if let Some(FfonElement::Obj(obj)) = arr.get_mut(idx) {
            obj.key = new_key;
        }
    }
}

/// Insert a cloned element at `id` (used by undo/redo).
fn insert_element_at_id(r: &mut AppRenderer, id: &IdArray, elem: FfonElement) {
    let insert_idx = id.last().unwrap_or(0);
    insert_at(&mut r.ffon, id, insert_idx, elem);
}

/// Replace element at `id` with a clone (used by undo/redo).
fn replace_element_at_id(r: &mut AppRenderer, id: &IdArray, elem: FfonElement) {
    let idx = id.last().unwrap_or(0);
    replace_at(&mut r.ffon, id, idx, elem);
}

/// Cursor position to land on when undoing/redoing a `Structural::Replace`
/// over a radio-group Obj. If `elem` is a `<radio>` Obj, returns `id` with
/// the index of the now-checked child pushed — so the user sees the
/// restored selection inside the radio children rather than being thrown
/// up to the radio group's parent slot. For non-radio replacements, falls
/// back to `id` itself.
fn cursor_inside_radio(id: &IdArray, elem: &FfonElement) -> IdArray {
    use sicompass_sdk::tags;
    if let FfonElement::Obj(o) = elem {
        if tags::has_radio(&o.key) {
            if let Some(checked_idx) = o.children.iter().position(|c| {
                c.as_str().map_or(false, |s| tags::has_checked(s))
            }) {
                let mut cursor = id.clone();
                cursor.push(checked_idx);
                return cursor;
            }
        }
    }
    id.clone()
}

// ---------------------------------------------------------------------------
// get_current_line — build the "line" for updateFfon
// ---------------------------------------------------------------------------

fn get_current_line(r: &AppRenderer) -> (String, bool) {
    if matches!(r.coordinate, Coordinate::Insert) {
        return (r.input_buffer.clone(), false);
    }

    let arr = match navigate_to_slice(&r.ffon, &r.current_id) {
        Some(a) => a,
        None => return (String::new(), false),
    };
    let idx = r.current_id.last().unwrap_or(0);
    match arr.get(idx) {
        Some(FfonElement::Str(s)) => (s.clone(), false),
        Some(FfonElement::Obj(obj)) => (obj.key.clone(), true),
        None => (String::new(), false),
    }
}

/// Before re-inserting a real element into a body that currently holds only `I_PLACEHOLDER`,
/// clear the placeholder so the re-inserted element is the sole child.
///
/// Must be called BEFORE `insert_element_at_id` so the placeholder does not end up
/// sitting alongside the restored element after a redo or undo-of-delete.
fn clear_sole_i_placeholder_if_body_element(r: &mut AppRenderer, id: &sicompass_sdk::ffon::IdArray) {
    if id.depth() < 3 { return; }
    let is_sole = sicompass_sdk::ffon::get_ffon_at_id(&r.ffon, id)
        .map(|arr| arr.len() == 1 && matches!(&arr[0], sicompass_sdk::ffon::FfonElement::Str(s)
                   if s == sicompass_sdk::placeholders::I_PLACEHOLDER))
        .unwrap_or(false);
    if !is_sole { return; }
    if let Some(arr) = navigate_to_slice_pub(&mut r.ffon, id) {
        arr.clear();
    }
}

/// After a delete leaves a bare `"<input></input>"` as the sole body child, upgrade it
/// to `I_PLACEHOLDER` so the user sees "i" (inviting typed insertion) rather than "-i ".
///
/// Must be called BEFORE `sync_compose_body_if_body_element` so the upgraded placeholder
/// propagates into `compose.draft.body` via the subsequent sync.
fn upgrade_body_bare_placeholder(r: &mut AppRenderer, id: &sicompass_sdk::ffon::IdArray) {
    if id.depth() < 3 { return; }
    let is_bare = sicompass_sdk::ffon::get_ffon_at_id(&r.ffon, id)
        .map(|arr| arr.len() == 1 && matches!(&arr[0], sicompass_sdk::ffon::FfonElement::Str(s) if s == "<input></input>"))
        .unwrap_or(false);
    if !is_bare { return; }
    if let Some(arr) = navigate_to_slice_pub(&mut r.ffon, id) {
        arr[0] = sicompass_sdk::ffon::FfonElement::Str(
            sicompass_sdk::placeholders::I_PLACEHOLDER.to_owned(),
        );
    }
}

/// Notify the active provider that body children changed after a FFON-level delete/insert.
///
/// Only acts when `id` is at depth >= 3 (provider / body-Obj / element), which is the
/// depth used by body elements when `refresh_on_navigate` is false for compose paths.
pub fn sync_compose_body_if_body_element(r: &mut AppRenderer, id: &sicompass_sdk::ffon::IdArray) {
    if id.depth() < 3 {
        return;
    }
    let provider_idx = match id.get(0) { Some(i) => i, None => return };
    // get_ffon_at_id returns the *parent* array of the element at `id`.
    // For id = [p, body_obj_idx, elem_idx], that is the body Obj's children slice.
    let body_children = sicompass_sdk::ffon::get_ffon_at_id(&r.ffon, id)
        .map(|arr| arr.to_vec());
    if let Some(children) = body_children {
        if let Some(p) = r.providers.get_mut(provider_idx) {
            p.sync_ffon_body_children(&children);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::{AppRenderer, Coordinate, History, Task};
    use sicompass_sdk::ffon::{FfonElement, IdArray};
    use sicompass_sdk::Provider;

    /// Stand-in for the editor provider so `update_ffon` / `update_state`
    /// take the editor-semantics branch in tests that don't wire up a real
    /// provider stack.
    struct MockEditorProvider;
    impl Provider for MockEditorProvider {
        fn name(&self) -> &str { "mock_editor" }
        fn fetch(&mut self) -> Vec<FfonElement> { Vec::new() }
        fn has_editor_semantics(&self) -> bool { true }
    }

    fn make_renderer(ffon: Vec<FfonElement>) -> AppRenderer {
        let mut r = AppRenderer::new();
        r.ffon = ffon;
        r.current_id = { let mut id = IdArray::new(); id.push(0); id };
        r.providers.push(Box::new(MockEditorProvider));
        r
    }

    // --- is_line_key ---

    #[test]
    fn is_line_key_with_colon() {
        assert!(is_line_key("Section:"));
    }

    #[test]
    fn is_line_key_without_colon() {
        assert!(!is_line_key("plain text"));
    }

    #[test]
    fn is_line_key_empty() {
        assert!(!is_line_key(""));
    }

    // --- strip_trailing_colon ---

    #[test]
    fn strip_colon_removes_trailing() {
        assert_eq!(strip_trailing_colon("Section:"), "Section");
    }

    #[test]
    fn strip_colon_no_change() {
        assert_eq!(strip_trailing_colon("plain"), "plain");
    }

    // --- update_ids ---

    #[test]
    fn update_ids_arrow_up_decrements() {
        let mut r = make_renderer(vec![
            FfonElement::new_str("a"),
            FfonElement::new_str("b"),
        ]);
        r.current_id.set_last(1);
        update_ids(&mut r, false, Task::ArrowUp, History::None);
        assert_eq!(r.current_id.last(), Some(0));
    }

    #[test]
    fn update_ids_arrow_down_increments() {
        let mut r = make_renderer(vec![
            FfonElement::new_str("a"),
            FfonElement::new_str("b"),
        ]);
        r.current_id.set_last(0);
        update_ids(&mut r, false, Task::ArrowDown, History::None);
        assert_eq!(r.current_id.last(), Some(1));
    }

    #[test]
    fn update_ids_arrow_down_clamps_at_max() {
        let mut r = make_renderer(vec![FfonElement::new_str("only")]);
        r.current_id.set_last(0);
        update_ids(&mut r, false, Task::ArrowDown, History::None);
        assert_eq!(r.current_id.last(), Some(0)); // no change, already at max
    }

    // --- update_ffon (basic) ---

    #[test]
    fn update_ffon_input_string_value() {
        let mut r = make_renderer(vec![FfonElement::new_str("old")]);
        r.coordinate = Coordinate::Insert;
        r.input_buffer = "new value".to_owned();
        r.previous_id = r.current_id.clone();
        update_ffon(&mut r, "new value", false, Task::Input, History::None);
        assert_eq!(r.ffon[0].as_str(), Some("new value"));
    }

    #[test]
    fn update_ffon_input_empty_root_creates_string() {
        let mut r = make_renderer(vec![]);
        r.coordinate = Coordinate::Insert;
        r.previous_id = r.current_id.clone();
        update_ffon(&mut r, "hello", false, Task::Input, History::None);
        assert_eq!(r.ffon.len(), 1);
        assert_eq!(r.ffon[0].as_str(), Some("hello"));
    }

    #[test]
    fn update_ffon_input_empty_root_creates_obj_for_key() {
        let mut r = make_renderer(vec![]);
        r.coordinate = Coordinate::Insert;
        r.previous_id = r.current_id.clone();
        update_ffon(&mut r, "Section:", true, Task::Input, History::None);
        assert_eq!(r.ffon.len(), 1);
        assert!(r.ffon[0].is_obj());
        assert_eq!(r.ffon[0].as_obj().unwrap().key, "Section");
    }

    #[test]
    fn update_ffon_delete_string_removes_element() {
        let mut r = make_renderer(vec![
            FfonElement::new_str("keep"),
            FfonElement::new_str("remove"),
        ]);
        r.coordinate = Coordinate::General;
        r.current_id.set_last(1);
        r.previous_id = r.current_id.clone();
        update_ffon(&mut r, "remove", false, Task::Delete, History::None);
        assert_eq!(r.ffon.len(), 1);
        assert_eq!(r.ffon[0].as_str(), Some("keep"));
    }

    // --- update_state integration ---

    #[test]
    fn update_state_input_commits_value() {
        let mut r = make_renderer(vec![FfonElement::new_str("old")]);
        r.coordinate = Coordinate::Insert;
        r.input_buffer = "updated".to_owned();
        update_state(&mut r, Task::Input, History::None);
        assert_eq!(r.ffon[0].as_str(), Some("updated"));
    }

    #[test]
    fn update_state_records_undo_for_input() {
        let mut r = make_renderer(vec![FfonElement::new_str("before")]);
        r.coordinate = Coordinate::Insert;
        r.input_buffer = "after".to_owned();
        update_state(&mut r, Task::Input, History::None);
        let tl = r.active_timeline();
        assert_eq!(tl.entries.len(), 1);
        assert!(matches!(
            tl.entries[0],
            TimelineEntry::TextChunk { .. }
        ));
    }

    // --- is_line_key (additional) ---

    #[test]
    fn is_line_key_just_colon() {
        assert!(is_line_key(":"));
    }

    #[test]
    fn is_line_key_colon_in_middle() {
        assert!(!is_line_key("key:value"));
    }

    #[test]
    fn is_line_key_multiple_colons() {
        assert!(is_line_key("a:b:c:"));
    }

    #[test]
    fn is_line_key_spaces_before_colon() {
        assert!(is_line_key("my key :"));
    }

    // --- strip_trailing_colon (additional) ---

    #[test]
    fn strip_colon_just_colon() {
        assert_eq!(strip_trailing_colon(":"), "");
    }

    #[test]
    fn strip_colon_empty_string() {
        assert_eq!(strip_trailing_colon(""), "");
    }

    // --- record_entry ---

    #[test]
    fn record_entry_pushes_text_chunk() {
        let mut r = AppRenderer::new();
        let entry = TimelineEntry::TextChunk {
            id: {
                let mut id = IdArray::new();
                id.push(0);
                id
            },
            before: FfonElement::Str("a".into()),
            after: FfonElement::Str("ab".into()),
            chunk_seq: 0,
        };
        record_entry(&mut r, entry);
        assert_eq!(r.active_timeline().entries.len(), 1);
    }

    // --- update_ids (additional boundary cases) ---

    #[test]
    fn update_ids_move_up_at_top_stays() {
        let mut r = make_renderer(vec![FfonElement::new_str("only")]);
        r.current_id.set_last(0);
        update_ids(&mut r, false, Task::ArrowUp, History::None);
        assert_eq!(r.current_id.last(), Some(0));
    }

    #[test]
    fn update_ids_move_down_at_bottom_stays() {
        let mut r = make_renderer(vec![
            FfonElement::new_str("a"),
            FfonElement::new_str("b"),
        ]);
        r.current_id.set_last(1);
        update_ids(&mut r, false, Task::ArrowDown, History::None);
        assert_eq!(r.current_id.last(), Some(1));
    }

    // --- update_ffon (object key modification) ---

    #[test]
    fn update_ffon_input_key_modifies_object_key() {
        let mut r = make_renderer(vec![{
            let mut root = FfonElement::new_obj("root:");
            let child = FfonElement::new_obj("old key:");
            root.as_obj_mut().unwrap().push(child);
            root
        }]);
        r.coordinate = Coordinate::Insert;
        r.current_id = {
            let mut id = IdArray::new();
            id.push(0);
            id.push(0);
            id
        };
        r.previous_id = r.current_id.clone();
        update_ffon(&mut r, "new key:", true, Task::Input, History::None);
        // The child at [0][0] should now have key "new key"
        let root_obj = r.ffon[0].as_obj().unwrap();
        assert_eq!(root_obj.children[0].as_obj().unwrap().key, "new key");
    }
}
