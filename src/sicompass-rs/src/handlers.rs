//! Key handlers — equivalent to `handlers.c`.
//!
//! Each function corresponds to one key action and mutates `AppRenderer`
//! in-place. Rendering is triggered by setting `needs_redraw = true`.

use crate::app_state::{AppRenderer, Coordinate, History, Task};
use crate::list;
use sicompass_sdk::ffon::{get_ffon_at_id, next_layer_exists};
use sicompass_sdk::tags;

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

/// Move selection up in the current list.
pub fn handle_up(r: &mut AppRenderer) {
    if r.list_index > 0 {
        r.list_index -= 1;
        r.sync_current_id_from_list();
        r.caret.reset(sdl_ticks());
        r.needs_redraw = true;
    }
}

/// Move selection down in the current list.
pub fn handle_down(r: &mut AppRenderer) {
    let len = r.active_list_len();
    if len == 0 { return; }
    if r.list_index < len - 1 {
        r.list_index += 1;
        r.sync_current_id_from_list();
        r.caret.reset(sdl_ticks());
        r.needs_redraw = true;
    }
}

/// Navigate into the selected item (Right key).
pub fn handle_right(r: &mut AppRenderer) {
    let item_id = match r.current_list_item_id() {
        Some(id) => id,
        None => return,
    };

    if !next_layer_exists(&r.ffon, &item_id) {
        return; // leaf node — not navigable
    }

    let mut new_id = item_id.clone();
    new_id.push(0);
    r.current_id = new_id;

    // Refresh from provider if it uses lazy fetching (no_cache providers)
    crate::provider::refresh_if_needed(r);

    list::create_list_current_layer(r);
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

/// Navigate out to the parent level (Left key).
pub fn handle_left(r: &mut AppRenderer) {
    if r.current_id.depth() <= 1 {
        return; // already at root
    }

    r.current_id.pop();

    // Refresh parent level if needed
    crate::provider::refresh_if_needed(r);

    list::create_list_current_layer(r);
    // current_id.last() now points at the item we came from — list_index was
    // already set correctly by create_list_current_layer.
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

/// Page up (scroll a full screen up).
pub fn handle_page_up(r: &mut AppRenderer) {
    let page = (r.window_height / r.cached_line_height.max(1)).max(1) as usize;
    r.list_index = r.list_index.saturating_sub(page);
    r.sync_current_id_from_list();
    r.needs_redraw = true;
}

/// Page down (scroll a full screen down).
pub fn handle_page_down(r: &mut AppRenderer) {
    let len = r.active_list_len();
    if len == 0 { return; }
    let page = (r.window_height / r.cached_line_height.max(1)).max(1) as usize;
    r.list_index = (r.list_index + page).min(len - 1);
    r.sync_current_id_from_list();
    r.needs_redraw = true;
}

/// Jump to first item.
pub fn handle_ctrl_home(r: &mut AppRenderer) {
    r.list_index = 0;
    r.sync_current_id_from_list();
    r.needs_redraw = true;
}

/// Jump to last item.
pub fn handle_ctrl_end(r: &mut AppRenderer) {
    let len = r.active_list_len();
    if len > 0 {
        r.list_index = len - 1;
        r.sync_current_id_from_list();
    }
    r.needs_redraw = true;
}

// ---------------------------------------------------------------------------
// Mode transitions
// ---------------------------------------------------------------------------

/// Enter insert mode (cursor at start) on the current item.
pub fn handle_i(r: &mut AppRenderer) {
    if !matches!(r.coordinate, Coordinate::EditorGeneral | Coordinate::OperatorGeneral) {
        return;
    }
    r.previous_coordinate = r.coordinate;
    r.coordinate = if r.coordinate == Coordinate::OperatorGeneral {
        Coordinate::OperatorInsert
    } else {
        Coordinate::EditorInsert
    };
    populate_input_buffer(r);
    r.cursor_position = 0;
    r.selection_anchor = None;
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

/// Enter append mode (cursor at end) on the current item.
pub fn handle_a(r: &mut AppRenderer) {
    if !matches!(r.coordinate, Coordinate::EditorGeneral | Coordinate::OperatorGeneral) {
        return;
    }
    r.previous_coordinate = r.coordinate;
    r.coordinate = if r.coordinate == Coordinate::OperatorGeneral {
        Coordinate::OperatorInsert
    } else {
        Coordinate::EditorInsert
    };
    populate_input_buffer(r);
    r.cursor_position = r.input_buffer.len();
    r.selection_anchor = None;
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

/// Toggle between OperatorGeneral and EditorGeneral (Space key).
pub fn handle_space(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::OperatorGeneral => {
            r.previous_coordinate = r.coordinate;
            r.coordinate = Coordinate::EditorGeneral;
        }
        Coordinate::EditorGeneral => {
            r.previous_coordinate = r.coordinate;
            r.coordinate = Coordinate::OperatorGeneral;
        }
        _ => return,
    }
    r.needs_redraw = true;
}

/// Navigate into / out of the meta object (M key in OperatorGeneral).
pub fn handle_meta(r: &mut AppRenderer) {
    if r.coordinate != Coordinate::OperatorGeneral {
        return;
    }

    if r.inside_meta {
        // Restore saved position
        r.inside_meta = false;
        r.show_meta_menu = false;
        r.current_id = r.meta_return_id.clone();
        r.list_index = r.meta_return_list_index;
        list::create_list_current_layer(r);
    } else {
        // Navigate into meta's children (meta is always at index 0 at current level)
        let meta_exists = {
            let arr = get_ffon_at_id(&r.ffon, &r.current_id);
            arr.and_then(|a| a.first())
                .and_then(|e| e.as_obj())
                .map_or(false, |o| o.key == "meta" && !o.children.is_empty())
        };
        if meta_exists {
            r.meta_return_id = r.current_id.clone();
            r.meta_return_list_index = r.list_index;
            r.current_id.set_last(0);
            r.current_id.push(0);
            r.inside_meta = true;
            r.show_meta_menu = false;
            r.list_index = 0;
            list::create_list_current_layer(r);
        }
    }
    r.needs_redraw = true;
}

/// Enter Tab search mode.
pub fn handle_tab(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::OperatorGeneral | Coordinate::OperatorInsert => {
            r.previous_coordinate = r.coordinate;
            r.coordinate = Coordinate::SimpleSearch;
            r.search_string.clear();
            list::create_list_current_layer(r);
        }
        Coordinate::SimpleSearch => {
            // Tab again → scroll mode
            r.coordinate = Coordinate::Scroll;
            r.scroll_offset = 0;
        }
        _ => {}
    }
    r.needs_redraw = true;
}

/// Enter command mode (:).
pub fn handle_colon(r: &mut AppRenderer) {
    r.previous_coordinate = r.coordinate;
    r.coordinate = Coordinate::Command;
    r.input_buffer.clear();
    r.cursor_position = 0;
    list::create_list_current_layer(r);
    r.needs_redraw = true;
}

/// Append a new empty element after the current one (Ctrl+A / Enter in EditorGeneral).
pub fn handle_append(r: &mut AppRenderer) {
    crate::state::update_state(r, Task::Append, History::None);
    r.needs_redraw = true;
}

/// Insert a new empty element before the current one (Ctrl+I in EditorGeneral).
pub fn handle_insert(r: &mut AppRenderer) {
    crate::state::update_state(r, Task::Insert, History::None);
    r.needs_redraw = true;
}

/// Enter in OperatorGeneral — toggle checkbox/radio, activate input, or open file.
pub fn handle_enter_operator(r: &mut AppRenderer) {
    use sicompass_sdk::ffon::{FfonElement, get_ffon_at_id};
    use sicompass_sdk::tags;

    let elem_clone = {
        let arr = match get_ffon_at_id(&r.ffon, &r.current_id) {
            Some(a) => a,
            None => { r.needs_redraw = true; return; }
        };
        let idx = r.current_id.last().unwrap_or(0);
        match arr.get(idx) {
            Some(e) => e.clone(),
            None => { r.needs_redraw = true; return; }
        }
    };

    // Toggle checkbox
    if let Some(new_text) = toggle_checkbox(&elem_clone) {
        let idx = r.current_id.last().unwrap_or(0);
        if let Some(arr) = crate::state::navigate_to_slice_pub(&mut r.ffon, &r.current_id) {
            if let Some(elem) = arr.get_mut(idx) {
                *elem = match &elem_clone {
                    FfonElement::Str(_) => FfonElement::new_str(new_text),
                    FfonElement::Obj(o) => {
                        let mut new_obj = FfonElement::new_obj(new_text);
                        for c in &o.children { new_obj.as_obj_mut().unwrap().push(c.clone()); }
                        new_obj
                    }
                };
            }
        }
        list::create_list_current_layer(r);
        r.needs_redraw = true;
        return;
    }

    // Toggle radio — add <checked> to selected, remove from siblings
    if toggle_radio(r) {
        list::create_list_current_layer(r);
        r.needs_redraw = true;
        return;
    }

    // Activate <input> element (commit existing content — triggers provider refresh)
    let (elem_text, is_obj_elem) = match &elem_clone {
        FfonElement::Str(s) => (s.clone(), false),
        FfonElement::Obj(o) => (o.key.clone(), true),
    };
    if tags::has_input(&elem_text) || tags::has_input_all(&elem_text) {
        let content = if tags::has_input_all(&elem_text) {
            tags::extract_input_all(&elem_text).unwrap_or_default()
        } else {
            tags::extract_input(&elem_text).unwrap_or_default()
        };
        crate::provider::commit_edit(r, &content, &content);
        crate::provider::refresh_if_needed(r);
        list::create_list_current_layer(r);
        r.needs_redraw = true;
        return;
    }

    r.needs_redraw = true;
}

/// Enter in SimpleSearch — navigate to selected item and return to previous mode.
pub fn handle_enter_search(r: &mut AppRenderer) {
    let selected_id = match r.current_list_item_id() {
        Some(id) => id,
        None => {
            r.coordinate = r.previous_coordinate;
            r.search_string.clear();
            list::create_list_current_layer(r);
            r.needs_redraw = true;
            return;
        }
    };
    r.current_id = selected_id;
    r.coordinate = r.previous_coordinate;
    r.search_string.clear();
    list::create_list_current_layer(r);
    r.list_index = r.current_id.last().unwrap_or(0).min(r.active_list_len().saturating_sub(1));
    r.needs_redraw = true;
}

// ---------------------------------------------------------------------------
// Checkbox / radio helpers
// ---------------------------------------------------------------------------

fn toggle_checkbox(elem: &sicompass_sdk::ffon::FfonElement) -> Option<String> {
    use sicompass_sdk::tags;
    let text = match elem {
        sicompass_sdk::ffon::FfonElement::Str(s) => s.as_str(),
        sicompass_sdk::ffon::FfonElement::Obj(o) => o.key.as_str(),
    };
    if tags::has_checkbox_checked(text) {
        let content = tags::extract_checkbox_checked(text).unwrap_or_default();
        Some(tags::format_checkbox(&content))
    } else if tags::has_checkbox(text) {
        let content = tags::extract_checkbox(text).unwrap_or_default();
        Some(tags::format_checkbox_checked(&content))
    } else {
        None
    }
}

fn toggle_radio(r: &mut AppRenderer) -> bool {
    use sicompass_sdk::ffon::{FfonElement, get_ffon_at_id};
    use sicompass_sdk::tags;

    // Validate: must be depth ≥ 2 and parent must have <radio> tag
    if r.current_id.depth() < 2 {
        return false;
    }
    let mut parent_id = r.current_id.clone();
    let child_idx = parent_id.pop().unwrap_or(0);

    // Check parent has radio tag
    let parent_is_radio = {
        let arr = match get_ffon_at_id(&r.ffon, &parent_id) {
            Some(a) => a,
            None => return false,
        };
        let pidx = parent_id.last().unwrap_or(0);
        arr.get(pidx)
            .and_then(|e| e.as_obj())
            .map_or(false, |o| tags::has_radio(&o.key))
    };
    if !parent_is_radio {
        return false;
    }

    // Navigate to parent's children, uncheck all, check the selected one
    let children_len = {
        let arr = match get_ffon_at_id(&r.ffon, &r.current_id) {
            Some(a) => a,
            None => return false,
        };
        arr.len()
    };

    // Uncheck all siblings, check the target
    let parent_id_for_children = r.current_id.clone();
    for i in 0..children_len {
        let mut sib_id = parent_id_for_children.clone();
        sib_id.set_last(i);
        let has_checked_tag = {
            let arr = get_ffon_at_id(&r.ffon, &sib_id);
            arr.and_then(|a| a.get(i))
                .and_then(|e| e.as_str())
                .map_or(false, |s| tags::has_checked(s))
        };
        if has_checked_tag {
            // Strip <checked>
            let text = {
                let arr = get_ffon_at_id(&r.ffon, &sib_id).unwrap();
                arr[i].as_str().unwrap().to_owned()
            };
            let content = tags::extract_checked(&text).unwrap_or(text);
            if let Some(arr) = crate::state::navigate_to_slice_pub(&mut r.ffon, &sib_id) {
                arr[i] = FfonElement::new_str(content);
            }
        }
    }
    // Add <checked> to selected
    let target_text = {
        let arr = get_ffon_at_id(&r.ffon, &parent_id_for_children).unwrap();
        arr.get(child_idx).and_then(|e| e.as_str()).map(|s| s.to_owned())
    };
    if let Some(text) = target_text {
        let new_text = tags::format_checked(&tags::strip_display(&text));
        if let Some(arr) = crate::state::navigate_to_slice_pub(&mut r.ffon, &r.current_id) {
            arr[child_idx] = FfonElement::new_str(new_text);
        }
    }

    true
}

/// Commit the input buffer to the active provider (Enter in OperatorInsert).
///
/// For elements with `<input>` or `<input-all>` tags, calls `commit_edit` on
/// the provider with old/new content, then updates the FFON element and exits
/// insert mode. Falls back to a direct FFON update for providers without
/// `commit_edit` support.
pub fn handle_enter_operator_insert(r: &mut AppRenderer) {
    use sicompass_sdk::ffon::{FfonElement, get_ffon_at_id};
    use sicompass_sdk::tags;

    // Get current element's raw key/value
    let (elem_text, is_obj) = {
        let arr = match get_ffon_at_id(&r.ffon, &r.current_id) {
            Some(a) => a,
            None => { handle_escape(r); return; }
        };
        let idx = r.current_id.last().unwrap_or(0);
        match arr.get(idx) {
            Some(FfonElement::Str(s)) => (s.clone(), false),
            Some(FfonElement::Obj(o)) => (o.key.clone(), true),
            None => { handle_escape(r); return; }
        }
    };

    // Extract the old content from the tag
    let old_content = if tags::has_input_all(&elem_text) {
        tags::extract_input_all(&elem_text)
    } else if tags::has_input(&elem_text) {
        tags::extract_input(&elem_text)
    } else {
        // No input tag — just escape
        handle_escape(r);
        return;
    };
    let old_content = old_content.unwrap_or_default();
    let new_content = r.input_buffer.clone();

    if old_content == new_content {
        handle_escape(r);
        return;
    }

    // Try provider commit first
    let committed = crate::provider::commit_edit(r, &old_content, &new_content);

    // Build replacement element text (re-wrap in tag format)
    let new_elem_text = if tags::has_input_all(&elem_text) {
        let prefix = &r.input_prefix;
        let suffix = &r.input_suffix;
        format!("{prefix}<input-all>{new_content}</input-all>{suffix}")
    } else {
        let prefix = &r.input_prefix;
        let suffix = &r.input_suffix;
        format!("{prefix}<input>{new_content}</input>{suffix}")
    };

    // Update FFON element regardless of commit result (provider may not implement commit_edit)
    let idx = r.current_id.last().unwrap_or(0);
    if let Some(arr) = crate::state::navigate_to_slice_pub(&mut r.ffon, &r.current_id) {
        if let Some(elem) = arr.get_mut(idx) {
            *elem = if is_obj {
                FfonElement::new_obj(new_elem_text)
            } else {
                FfonElement::new_str(new_elem_text)
            };
        }
    }

    if committed {
        crate::provider::refresh_if_needed(r);
    }

    handle_escape(r);
}

/// Undo the last edit action.
pub fn handle_undo(r: &mut AppRenderer) {
    crate::state::handle_history_action(r, History::Undo);
    r.needs_redraw = true;
}

/// Redo the last undone action.
pub fn handle_redo(r: &mut AppRenderer) {
    crate::state::handle_history_action(r, History::Redo);
    r.needs_redraw = true;
}

/// Return to the previous/operator mode (Escape).
pub fn handle_escape(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::OperatorGeneral => {
            // Already at base mode — nothing to do
        }
        Coordinate::SimpleSearch | Coordinate::ExtendedSearch => {
            r.coordinate = r.previous_coordinate;
            r.search_string.clear();
            list::create_list_current_layer(r);
        }
        Coordinate::Command => {
            r.coordinate = r.previous_coordinate;
            r.input_buffer.clear();
            r.cursor_position = 0;
            list::create_list_current_layer(r);
        }
        Coordinate::EditorInsert | Coordinate::EditorGeneral
        | Coordinate::EditorNormal | Coordinate::EditorVisual => {
            r.coordinate = Coordinate::OperatorGeneral;
            r.input_buffer.clear();
            r.cursor_position = 0;
            r.selection_anchor = None;
        }
        Coordinate::Scroll | Coordinate::ScrollSearch => {
            r.coordinate = r.previous_coordinate;
            r.scroll_offset = 0;
        }
        _ => {
            r.coordinate = r.previous_coordinate;
        }
    }
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

// ---------------------------------------------------------------------------
// Text input (while in an editing mode)
// ---------------------------------------------------------------------------

/// Handle a printable text input event.
pub fn handle_input(r: &mut AppRenderer, text: &str) {
    match r.coordinate {
        Coordinate::SimpleSearch => {
            r.search_string.push_str(text);
            let search = r.search_string.clone();
            list::create_list_current_layer(r);
            list::populate_list_current_layer(r, &search);
            r.needs_redraw = true;
        }
        Coordinate::Command | Coordinate::EditorInsert | Coordinate::OperatorInsert => {
            // Replace selection if active
            if has_selection(r) { delete_selection(r); }
            // Insert text at cursor position (byte offset)
            let pos = r.cursor_position.min(r.input_buffer.len());
            r.input_buffer.insert_str(pos, text);
            r.cursor_position = pos + text.len();
            r.caret.reset(sdl_ticks());
            r.needs_redraw = true;
        }
        _ => {}
    }
}

/// Handle Backspace in editing modes.
pub fn handle_backspace(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::SimpleSearch => {
            if !r.search_string.is_empty() {
                // Remove last char (UTF-8 aware)
                let new_len = r.search_string
                    .char_indices()
                    .rev()
                    .next()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                r.search_string.truncate(new_len);
                let search = r.search_string.clone();
                list::create_list_current_layer(r);
                list::populate_list_current_layer(r, &search);
                r.needs_redraw = true;
            }
        }
        Coordinate::Command | Coordinate::EditorInsert | Coordinate::OperatorInsert => {
            if has_selection(r) {
                delete_selection(r);
                r.caret.reset(sdl_ticks());
                r.needs_redraw = true;
            } else if r.cursor_position > 0 {
                // Find the char boundary before cursor
                let before = &r.input_buffer[..r.cursor_position];
                let new_end = before.char_indices().rev().next()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                r.input_buffer.replace_range(new_end..r.cursor_position, "");
                r.cursor_position = new_end;
                r.caret.reset(sdl_ticks());
                r.needs_redraw = true;
            }
        }
        _ => {}
    }
}

/// Delete the currently selected item.
pub fn handle_delete(r: &mut AppRenderer, history: crate::app_state::History) {
    crate::state::update_state(r, crate::app_state::Task::Delete, history);
    r.needs_redraw = true;
}

/// Handle F5 — refresh current provider.
pub fn handle_f5(r: &mut AppRenderer) {
    crate::provider::refresh_current_directory(r);
    list::create_list_current_layer(r);
    r.needs_redraw = true;
}

// ---------------------------------------------------------------------------
// Input buffer population
// ---------------------------------------------------------------------------

/// Populate `input_buffer`, `input_prefix`, `input_suffix` from the current element.
///
/// Called by `handle_i` and `handle_a` before entering insert mode.
fn populate_input_buffer(r: &mut AppRenderer) {
    r.input_buffer.clear();
    r.input_prefix.clear();
    r.input_suffix.clear();

    let arr = match get_ffon_at_id(&r.ffon, &r.current_id) {
        Some(a) => a,
        None => return,
    };
    let idx = r.current_id.last().unwrap_or(0);
    let elem = match arr.get(idx) {
        Some(e) => e,
        None => return,
    };

    let element_key: &str = match elem {
        sicompass_sdk::ffon::FfonElement::Str(s) => s,
        sicompass_sdk::ffon::FfonElement::Obj(o) => &o.key,
    };

    // Try <input-all> first, then <input>
    let extracted = if tags::has_input_all(element_key) {
        tags::extract_input_all(element_key)
    } else if tags::has_input(element_key) {
        tags::extract_input(element_key)
    } else {
        None
    };

    if let Some(content) = extracted {
        r.input_buffer = content;

        // Extract prefix (text before the opening tag) and suffix (text after closing tag)
        let (open_tag, close_tag) = if tags::has_input_all(element_key) {
            ("<input-all>", "</input-all>")
        } else {
            ("<input>", "</input>")
        };
        if let Some(open_pos) = element_key.find(open_tag) {
            r.input_prefix = element_key[..open_pos].to_owned();
            let after_open = open_pos + open_tag.len();
            if let Some(close_pos) = element_key[after_open..].find(close_tag) {
                let after_close = after_open + close_pos + close_tag.len();
                r.input_suffix = element_key[after_close..].to_owned();
            }
        }
    } else {
        match elem {
            sicompass_sdk::ffon::FfonElement::Str(s) => {
                r.input_buffer = s.clone();
            }
            sicompass_sdk::ffon::FfonElement::Obj(o) => {
                r.input_buffer = format!("{}:", o.key);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Text selection helpers
// ---------------------------------------------------------------------------

fn has_selection(r: &AppRenderer) -> bool {
    r.selection_anchor.map_or(false, |a| a != r.cursor_position)
}

fn clear_selection(r: &mut AppRenderer) {
    r.selection_anchor = None;
}

fn selection_range(r: &AppRenderer) -> Option<(usize, usize)> {
    let a = r.selection_anchor?;
    let b = r.cursor_position;
    if a <= b { Some((a, b)) } else { Some((b, a)) }
}

/// Delete the selected text, placing cursor at the start of the deleted range.
fn delete_selection(r: &mut AppRenderer) {
    if let Some((start, end)) = selection_range(r) {
        r.input_buffer.replace_range(start..end, "");
        r.cursor_position = start;
        r.selection_anchor = None;
    }
}

// ---------------------------------------------------------------------------
// Multiline line-boundary helpers
// ---------------------------------------------------------------------------

fn find_line_start(buf: &str, pos: usize) -> usize {
    let bytes = buf.as_bytes();
    for i in (0..pos).rev() {
        if bytes[i] == b'\n' { return i + 1; }
    }
    0
}

fn find_line_end(buf: &str, pos: usize) -> usize {
    let bytes = buf.as_bytes();
    for i in pos..bytes.len() {
        if bytes[i] == b'\n' { return i; }
    }
    buf.len()
}

/// Count UTF-8 characters in `buf[from..to]`.
fn utf8_count_chars(buf: &str, from: usize, to: usize) -> usize {
    buf[from..to].chars().count()
}

/// Advance `col` UTF-8 chars from `from`, clamped to `limit`.
fn utf8_advance_n(buf: &str, from: usize, n: usize, limit: usize) -> usize {
    let mut pos = from;
    for _ in 0..n {
        if pos >= limit { break; }
        let ch = buf[pos..].chars().next().unwrap();
        pos += ch.len_utf8();
    }
    pos.min(limit)
}

// ---------------------------------------------------------------------------
// Selection-extending handlers
// ---------------------------------------------------------------------------

/// Ctrl+A in text-input modes — select all.
pub fn handle_select_all(r: &mut AppRenderer) {
    if r.input_buffer.is_empty() { return; }
    r.selection_anchor = Some(0);
    r.cursor_position = r.input_buffer.len();
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

/// Shift+Left — extend selection one character to the left.
pub fn handle_shift_left(r: &mut AppRenderer) {
    if r.cursor_position == 0 { return; }
    if r.selection_anchor.is_none() {
        r.selection_anchor = Some(r.cursor_position);
    }
    let before = &r.input_buffer[..r.cursor_position];
    r.cursor_position = before.char_indices().rev().next().map(|(i, _)| i).unwrap_or(0);
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

/// Shift+Right — extend selection one character to the right.
pub fn handle_shift_right(r: &mut AppRenderer) {
    if r.cursor_position >= r.input_buffer.len() { return; }
    if r.selection_anchor.is_none() {
        r.selection_anchor = Some(r.cursor_position);
    }
    let ch = r.input_buffer[r.cursor_position..].chars().next().unwrap();
    r.cursor_position += ch.len_utf8();
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

/// Home — go to first list item (OperatorGeneral/EditorGeneral) or line start (insert/search).
pub fn handle_home(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::Scroll => {
            r.text_scroll_offset = 0;
            r.needs_redraw = true;
        }
        Coordinate::OperatorGeneral | Coordinate::EditorGeneral => {
            r.current_id.set_last(0);
            list::create_list_current_layer(r);
            r.list_index = 0;
            r.scroll_offset = 0;
            r.needs_redraw = true;
        }
        _ => {
            // Text cursor: start of current line
            let pos = r.cursor_position;
            clear_selection(r);
            r.cursor_position = find_line_start(&r.input_buffer, pos);
            r.caret.reset(sdl_ticks());
            r.needs_redraw = true;
        }
    }
}

/// End — go to last list item (OperatorGeneral/EditorGeneral) or line end (insert/search).
pub fn handle_end(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::Scroll => {
            // scroll to bottom — approximated; full impl needs line count
            r.text_scroll_offset = i32::MAX;
            r.needs_redraw = true;
        }
        Coordinate::OperatorGeneral | Coordinate::EditorGeneral => {
            use sicompass_sdk::ffon::get_ffon_max_id;
            if let Some(slice) = sicompass_sdk::ffon::get_ffon_at_id(&r.ffon, &r.current_id) {
                let max_id = slice.len().saturating_sub(1);
                r.current_id.set_last(max_id);
                list::create_list_current_layer(r);
                r.list_index = max_id;
                r.scroll_offset = -1;
            }
            r.needs_redraw = true;
        }
        _ => {
            let pos = r.cursor_position;
            let buf_len = r.input_buffer.len();
            clear_selection(r);
            r.cursor_position = find_line_end(&r.input_buffer, pos).min(buf_len);
            r.caret.reset(sdl_ticks());
            r.needs_redraw = true;
        }
    }
}

/// Shift+Home — extend selection to start of current line.
pub fn handle_shift_home(r: &mut AppRenderer) {
    if r.selection_anchor.is_none() {
        r.selection_anchor = Some(r.cursor_position);
    }
    let pos = r.cursor_position;
    r.cursor_position = find_line_start(&r.input_buffer, pos);
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

/// Shift+End — extend selection to end of current line.
pub fn handle_shift_end(r: &mut AppRenderer) {
    if r.selection_anchor.is_none() {
        r.selection_anchor = Some(r.cursor_position);
    }
    let pos = r.cursor_position;
    let buf_len = r.input_buffer.len();
    r.cursor_position = find_line_end(&r.input_buffer, pos).min(buf_len);
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

// ---------------------------------------------------------------------------
// Multiline insert-mode navigation
// ---------------------------------------------------------------------------

/// Up in insert mode — move cursor to same column on the previous line.
pub fn handle_up_insert(r: &mut AppRenderer) {
    let pos = r.cursor_position;
    let cur_line_start = find_line_start(&r.input_buffer, pos);
    if cur_line_start == 0 { return; }
    let col = utf8_count_chars(&r.input_buffer, cur_line_start, pos);
    let prev_line_end = cur_line_start - 1; // the '\n'
    let prev_line_start = find_line_start(&r.input_buffer, prev_line_end);
    r.cursor_position = utf8_advance_n(&r.input_buffer, prev_line_start, col, prev_line_end);
    clear_selection(r);
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

/// Down in insert mode — move cursor to same column on the next line.
pub fn handle_down_insert(r: &mut AppRenderer) {
    let pos = r.cursor_position;
    let buf_len = r.input_buffer.len();
    let cur_line_end = find_line_end(&r.input_buffer, pos);
    if cur_line_end >= buf_len { return; }
    let cur_line_start = find_line_start(&r.input_buffer, pos);
    let col = utf8_count_chars(&r.input_buffer, cur_line_start, pos);
    let next_line_start = cur_line_end + 1;
    let next_line_end = find_line_end(&r.input_buffer, next_line_start);
    r.cursor_position = utf8_advance_n(&r.input_buffer, next_line_start, col, next_line_end);
    clear_selection(r);
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

/// Shift+Up in insert mode — extend selection to same column on previous line.
pub fn handle_shift_up_insert(r: &mut AppRenderer) {
    let pos = r.cursor_position;
    let cur_line_start = find_line_start(&r.input_buffer, pos);
    if cur_line_start == 0 { return; }
    if r.selection_anchor.is_none() { r.selection_anchor = Some(pos); }
    let col = utf8_count_chars(&r.input_buffer, cur_line_start, pos);
    let prev_line_end = cur_line_start - 1;
    let prev_line_start = find_line_start(&r.input_buffer, prev_line_end);
    r.cursor_position = utf8_advance_n(&r.input_buffer, prev_line_start, col, prev_line_end);
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

/// Shift+Down in insert mode — extend selection to same column on next line.
pub fn handle_shift_down_insert(r: &mut AppRenderer) {
    let pos = r.cursor_position;
    let buf_len = r.input_buffer.len();
    let cur_line_end = find_line_end(&r.input_buffer, pos);
    if cur_line_end >= buf_len { return; }
    if r.selection_anchor.is_none() { r.selection_anchor = Some(pos); }
    let cur_line_start = find_line_start(&r.input_buffer, pos);
    let col = utf8_count_chars(&r.input_buffer, cur_line_start, pos);
    let next_line_start = cur_line_end + 1;
    let next_line_end = find_line_end(&r.input_buffer, next_line_start);
    r.cursor_position = utf8_advance_n(&r.input_buffer, next_line_start, col, next_line_end);
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

// ---------------------------------------------------------------------------
// Delete-forward (Delete key in insert modes)
// ---------------------------------------------------------------------------

/// Delete the character at the cursor (or selected range) in insert modes.
pub fn handle_delete_forward(r: &mut AppRenderer) {
    if has_selection(r) {
        delete_selection(r);
        r.caret.reset(sdl_ticks());
        maybe_update_search(r);
        r.needs_redraw = true;
        return;
    }
    let pos = r.cursor_position;
    if pos < r.input_buffer.len() {
        let ch = r.input_buffer[pos..].chars().next().unwrap();
        r.input_buffer.remove(pos);
        let _ = ch; // char already removed via remove() which is byte-correct
        r.caret.reset(sdl_ticks());
        maybe_update_search(r);
        r.needs_redraw = true;
    }
}

/// Re-filter the list when editing in search/command modes.
fn maybe_update_search(r: &mut AppRenderer) {
    if matches!(r.coordinate, Coordinate::SimpleSearch | Coordinate::Command) {
        let s = r.search_string.clone();
        list::create_list_current_layer(r);
        list::populate_list_current_layer(r, &s);
    }
}

// ---------------------------------------------------------------------------
// Clipboard — Ctrl+X / Ctrl+C / Ctrl+V
// ---------------------------------------------------------------------------

fn sdl_set_clipboard(text: &str) {
    use std::ffi::CString;
    if let Ok(c) = CString::new(text) {
        unsafe { sdl3::sys::clipboard::SDL_SetClipboardText(c.as_ptr()); }
    }
}

fn sdl_get_clipboard() -> Option<String> {
    unsafe {
        if !sdl3::sys::clipboard::SDL_HasClipboardText() { return None; }
        let ptr = sdl3::sys::clipboard::SDL_GetClipboardText();
        if ptr.is_null() { return None; }
        let s = std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned();
        sdl3::sys::stdinc::SDL_free(ptr as *mut _);
        if s.is_empty() { None } else { Some(s) }
    }
}

fn is_text_edit_mode(r: &AppRenderer) -> bool {
    matches!(
        r.coordinate,
        Coordinate::EditorInsert
            | Coordinate::OperatorInsert
            | Coordinate::SimpleSearch
            | Coordinate::Command
    )
}

/// Ctrl+X — cut selected text (insert modes) or cut FFON element (editor general).
pub fn handle_ctrl_x(r: &mut AppRenderer) {
    if is_text_edit_mode(r) {
        if !has_selection(r) { return; }
        if let Some((start, end)) = selection_range(r) {
            sdl_set_clipboard(&r.input_buffer[start..end].to_owned());
        }
        delete_selection(r);
        r.caret.reset(sdl_ticks());
        maybe_update_search(r);
        r.needs_redraw = true;
        return;
    }
    if matches!(r.coordinate, Coordinate::EditorGeneral) {
        // Cut FFON element into internal clipboard
        if let Some(item) = r.current_list_item().cloned() {
            use sicompass_sdk::ffon::get_ffon_at_id;
            if let Some(slice) = get_ffon_at_id(&r.ffon, &item.id) {
                if let Some(idx) = item.id.last() {
                    r.clipboard = slice.get(idx).cloned();
                }
            }
        }
        crate::state::update_state(r, Task::Cut, History::None);
        r.needs_redraw = true;
    }
}

/// Ctrl+C — copy selected text (insert modes) or copy FFON element (editor general).
pub fn handle_ctrl_c(r: &mut AppRenderer) {
    if is_text_edit_mode(r) {
        if !has_selection(r) { return; }
        if let Some((start, end)) = selection_range(r) {
            sdl_set_clipboard(&r.input_buffer[start..end].to_owned());
        }
        r.needs_redraw = true;
        return;
    }
    if matches!(r.coordinate, Coordinate::EditorGeneral) {
        if let Some(item) = r.current_list_item().cloned() {
            use sicompass_sdk::ffon::get_ffon_at_id;
            if let Some(slice) = get_ffon_at_id(&r.ffon, &item.id) {
                if let Some(idx) = item.id.last() {
                    r.clipboard = slice.get(idx).cloned();
                }
            }
        }
        r.needs_redraw = true;
    }
}

/// Ctrl+V — paste from system clipboard (insert modes) or paste FFON element (editor general).
pub fn handle_ctrl_v(r: &mut AppRenderer) {
    if is_text_edit_mode(r) {
        let text = match sdl_get_clipboard() { Some(t) => t, None => return };
        if has_selection(r) { delete_selection(r); }
        let pos = r.cursor_position.min(r.input_buffer.len());
        r.input_buffer.insert_str(pos, &text);
        r.cursor_position = pos + text.len();
        r.caret.reset(sdl_ticks());
        maybe_update_search(r);
        r.needs_redraw = true;
        return;
    }
    if matches!(r.coordinate, Coordinate::EditorGeneral) {
        crate::state::update_state(r, Task::Paste, History::None);
        r.needs_redraw = true;
    }
}

// ---------------------------------------------------------------------------
// Ctrl+F — find / enter search mode
// ---------------------------------------------------------------------------

/// Ctrl+F — in Scroll mode enters ScrollSearch; in insert modes enters InputSearch;
/// otherwise enters SimpleSearch.
pub fn handle_ctrl_f(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::Scroll => {
            r.coordinate = Coordinate::ScrollSearch;
            r.input_buffer.clear();
            r.cursor_position = 0;
            r.selection_anchor = None;
            r.needs_redraw = true;
        }
        Coordinate::ScrollSearch | Coordinate::InputSearch => {}
        Coordinate::EditorInsert | Coordinate::OperatorInsert => {
            r.previous_coordinate = r.coordinate;
            r.coordinate = Coordinate::InputSearch;
            r.input_buffer.clear();
            r.cursor_position = 0;
            r.selection_anchor = None;
            r.needs_redraw = true;
        }
        _ => {
            r.previous_coordinate = r.coordinate;
            r.coordinate = Coordinate::SimpleSearch;
            r.search_string.clear();
            list::create_list_current_layer(r);
            r.needs_redraw = true;
        }
    }
}

// ---------------------------------------------------------------------------
// Operator-mode insert/append placeholders (Ctrl+I / Ctrl+A in OperatorGeneral)
// ---------------------------------------------------------------------------

/// Ctrl+I in OperatorGeneral — insert a `<input></input>` placeholder before the
/// current item and immediately enter insert mode.
pub fn handle_ctrl_i_operator(r: &mut AppRenderer) {
    if !matches!(r.coordinate, Coordinate::OperatorGeneral) { return; }
    let slice = match sicompass_sdk::ffon::get_ffon_at_id(&r.ffon, &r.current_id) {
        Some(s) => s,
        None => return,
    };
    let insert_idx = if slice.is_empty() {
        if r.current_id.depth() <= 1 { return; }
        0
    } else {
        r.current_id.last().unwrap_or(0)
    };
    insert_operator_placeholder(r, insert_idx);
}

/// Ctrl+A in OperatorGeneral — append a `<input></input>` placeholder after the
/// current item and immediately enter insert mode.
pub fn handle_ctrl_a_operator(r: &mut AppRenderer) {
    if !matches!(r.coordinate, Coordinate::OperatorGeneral) { return; }
    let slice = match sicompass_sdk::ffon::get_ffon_at_id(&r.ffon, &r.current_id) {
        Some(s) => s,
        None => return,
    };
    let insert_idx = if slice.is_empty() {
        if r.current_id.depth() <= 1 { return; }
        0
    } else {
        r.current_id.last().unwrap_or(0) + 1
    };
    insert_operator_placeholder(r, insert_idx);
}

/// Insert a `<input></input>` placeholder at `insert_idx` in the current parent,
/// navigate the cursor there, and immediately enter insert mode.
fn insert_operator_placeholder(r: &mut AppRenderer, insert_idx: usize) {
    use sicompass_sdk::ffon::FfonElement;
    let placeholder = FfonElement::Str("<input></input>".to_owned());

    let depth = r.current_id.depth();
    if depth == 1 {
        r.ffon.insert(insert_idx, placeholder);
    } else {
        let mut parent_id = r.current_id.clone();
        parent_id.pop();
        if let Some(parent_slice) = crate::state::navigate_to_slice_pub(&mut r.ffon, &parent_id) {
            let parent_idx = parent_id.last().unwrap_or(0);
            if let Some(FfonElement::Obj(obj)) = parent_slice.get_mut(parent_idx) {
                obj.children.insert(insert_idx, placeholder);
            } else {
                return;
            }
        } else {
            return;
        }
    }

    r.current_id.set_last(insert_idx);
    r.prefixed_insert_mode = false;
    list::create_list_current_layer(r);
    r.list_index = insert_idx;
    r.scroll_offset = 0;
    handle_i(r);
}

/// Get current time in milliseconds (used to reset caret blink).
fn sdl_ticks() -> u64 {
    // SDL3 provides SDL_GetTicks() returning u64 ms.
    // sdl3-rs exposes this via the timer subsystem, but since we don't carry
    // a TimerSubsystem here, we fall back to std::time.
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::AppRenderer;
    use sicompass_sdk::ffon::{FfonElement, IdArray};

    fn make_renderer() -> AppRenderer {
        let mut root = FfonElement::new_obj("provider");
        root.as_obj_mut().unwrap().push(FfonElement::new_str("item 0"));
        root.as_obj_mut().unwrap().push(FfonElement::new_str("item 1"));
        root.as_obj_mut().unwrap().push(FfonElement::new_str("item 2"));

        let mut section = FfonElement::new_obj("section");
        section.as_obj_mut().unwrap().push(FfonElement::new_str("child 0"));
        root.as_obj_mut().unwrap().push(section);

        let mut r = AppRenderer::new();
        r.ffon = vec![root];
        r.current_id = { let mut id = IdArray::new(); id.push(0); id.push(0); id };
        list::create_list_current_layer(&mut r);
        r
    }

    #[test]
    fn up_moves_index() {
        let mut r = make_renderer();
        r.list_index = 1;
        r.sync_current_id_from_list();
        handle_up(&mut r);
        assert_eq!(r.list_index, 0);
    }

    #[test]
    fn up_clamps_at_zero() {
        let mut r = make_renderer();
        r.list_index = 0;
        handle_up(&mut r);
        assert_eq!(r.list_index, 0);
    }

    #[test]
    fn down_moves_index() {
        let mut r = make_renderer();
        r.list_index = 0;
        handle_down(&mut r);
        assert_eq!(r.list_index, 1);
    }

    #[test]
    fn down_clamps_at_end() {
        let mut r = make_renderer();
        let last = r.active_list_len() - 1;
        r.list_index = last;
        handle_down(&mut r);
        assert_eq!(r.list_index, last);
    }

    #[test]
    fn right_navigates_into_section() {
        let mut r = make_renderer();
        // Select "section" (index 3)
        r.list_index = 3;
        r.sync_current_id_from_list();
        handle_right(&mut r);
        assert_eq!(r.current_id.depth(), 3);
        assert_eq!(r.total_list.len(), 1); // "child 0"
    }

    #[test]
    fn right_on_leaf_does_nothing() {
        let mut r = make_renderer();
        r.list_index = 0; // "item 0" — a leaf
        r.sync_current_id_from_list();
        let old_depth = r.current_id.depth();
        handle_right(&mut r);
        assert_eq!(r.current_id.depth(), old_depth);
    }

    #[test]
    fn left_goes_back() {
        let mut r = make_renderer();
        // Navigate into section
        r.list_index = 3;
        r.sync_current_id_from_list();
        handle_right(&mut r);
        assert_eq!(r.current_id.depth(), 3);
        handle_left(&mut r);
        assert_eq!(r.current_id.depth(), 2);
    }

    #[test]
    fn left_at_depth1_does_nothing() {
        let mut r = AppRenderer::new();
        r.ffon = vec![FfonElement::new_obj("p")];
        r.current_id = { let mut id = IdArray::new(); id.push(0); id };
        handle_left(&mut r);
        assert_eq!(r.current_id.depth(), 1);
    }

    #[test]
    fn escape_from_search_clears_and_returns() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.search_string = "query".to_owned();
        handle_escape(&mut r);
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
        assert!(r.search_string.is_empty());
    }

    #[test]
    fn handle_input_in_search() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        handle_input(&mut r, "item");
        assert_eq!(r.search_string, "item");
    }

    #[test]
    fn handle_backspace_in_search() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.search_string = "abc".to_owned();
        handle_backspace(&mut r);
        assert_eq!(r.search_string, "ab");
    }
}
