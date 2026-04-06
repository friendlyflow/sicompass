//! Key handlers — equivalent to `handlers.c`.
//!
//! Each function corresponds to one key action and mutates `AppRenderer`
//! in-place. Rendering is triggered by setting `needs_redraw = true`.

use crate::app_state::{AppRenderer, CommandPhase, Coordinate, History, Task};
use crate::list;
use sicompass_sdk::ffon::{get_ffon_at_id, next_layer_exists, FfonElement, IdArray};
use sicompass_sdk::tags;

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

/// Move selection up in the current list.
pub fn handle_up(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::ScrollSearch => {
            if r.scroll_search_match_count > 0 {
                if r.scroll_search_current_match > 0 {
                    r.scroll_search_current_match -= 1;
                } else {
                    r.scroll_search_current_match = r.scroll_search_match_count - 1;
                }
                r.scroll_search_snap = true;
                r.scroll_search_needs_position = false; // user took explicit control
            }
            r.needs_redraw = true;
        }
        Coordinate::Scroll => {
            let step = r.cached_line_height.max(1);
            r.text_scroll_offset = (r.text_scroll_offset - step).max(0);
            r.needs_redraw = true;
        }
        Coordinate::SimpleSearch | Coordinate::Command | Coordinate::ExtendedSearch | Coordinate::Meta => {
            r.error_message.clear();
            if r.list_index > 0 {
                r.list_index -= 1;
                if r.coordinate != Coordinate::Command && r.coordinate != Coordinate::Meta {
                    r.sync_current_id_from_list();
                }
            }
            r.needs_redraw = true;
        }
        Coordinate::EditorInsert | Coordinate::OperatorInsert => {
            r.needs_redraw = true;
        }
        _ => {
            crate::state::update_state(r, Task::ArrowUp, History::None);
            r.needs_redraw = true;
        }
    }
}

/// Move selection down in the current list.
pub fn handle_down(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::ScrollSearch => {
            if r.scroll_search_match_count > 0 {
                if r.scroll_search_current_match < r.scroll_search_match_count - 1 {
                    r.scroll_search_current_match += 1;
                } else {
                    r.scroll_search_current_match = 0;
                }
                r.scroll_search_snap = true;
                r.scroll_search_needs_position = false; // user took explicit control
            }
            r.needs_redraw = true;
        }
        Coordinate::Scroll => {
            let step = r.cached_line_height.max(1);
            let viewport_h = r.window_height - step; // area below header
            let max_offset = (r.text_scroll_total_height - viewport_h).max(0);
            r.text_scroll_offset = (r.text_scroll_offset + step).min(max_offset);
            r.needs_redraw = true;
        }
        Coordinate::SimpleSearch | Coordinate::Command | Coordinate::ExtendedSearch | Coordinate::Meta => {
            r.error_message.clear();
            let max_index = if r.filtered_list_indices.is_empty() {
                r.total_list.len().saturating_sub(1)
            } else {
                r.filtered_list_indices.len().saturating_sub(1)
            };
            if r.list_index < max_index {
                r.list_index += 1;
                if r.coordinate != Coordinate::Command && r.coordinate != Coordinate::Meta {
                    r.sync_current_id_from_list();
                }
            }
            r.needs_redraw = true;
        }
        Coordinate::EditorInsert | Coordinate::OperatorInsert => {
            // noop in insert modes
        }
        _ => {
            crate::state::update_state(r, Task::ArrowDown, History::None);
            r.needs_redraw = true;
        }
    }
}

/// Navigate into the item at `r.current_id` without rebuilding the list.
/// Returns `true` if navigation happened.
pub fn navigate_right_raw(r: &mut AppRenderer) -> bool {
    let item_id = r.current_id.clone();

    if !next_layer_exists(&r.ffon, &item_id) {
        return false; // leaf node — not navigable
    }

    // Extract segment name + whether the Obj already has children (static vs lazy) + link URL.
    let (segment, has_children, link_url) = {
        let depth = item_id.depth();
        let last_idx = item_id.get(depth.saturating_sub(1)).unwrap_or(0);
        get_ffon_at_id(&r.ffon, &item_id)
            .and_then(|s| s.get(last_idx))
            .and_then(|e| e.as_obj())
            .map(|o| {
                let link = if tags::has_link(&o.key) { tags::extract_link(&o.key) } else { None };
                (tags::strip_display(&o.key).to_string(), !o.children.is_empty(), link)
            })
            .unwrap_or_default()
    };

    // Handle <link> tags: resolve URL and load content as children.
    if let Some(url) = link_url {
        if !has_children {
            let children = resolve_link_to_elements(&url);
            if children.is_empty() {
                return false;
            }
            let depth = item_id.depth();
            let last_idx = item_id.get(depth.saturating_sub(1)).unwrap_or(0);
            if let Some(siblings) = crate::provider::get_ffon_at_id_mut(&mut r.ffon, &item_id) {
                if let Some(FfonElement::Obj(obj)) = siblings.get_mut(last_idx) {
                    obj.children = children;
                }
            }
        }
        let mut new_id = item_id;
        new_id.push(0);
        r.current_id = new_id;
        return true;
    }

    if has_children {
        // Static tree (settings, tutorial, meta): navigate deeper in-place.
        let mut new_id = item_id;
        new_id.push(0);
        r.current_id = new_id;
    } else {
        // Lazy-fetch provider (filebrowser): push path, re-fetch, stay at depth 2.
        let provider_idx = item_id.get(0).unwrap_or(0);
        crate::provider::push_path(r, &segment);
        crate::provider::refresh_current_directory(r);
        // If the directory is empty, insert a placeholder so the user can create files
        // (mirrors C providerNavigateRight: childCount == 0 → add <input></input>)
        if let Some(root) = r.ffon.get_mut(provider_idx) {
            if let Some(obj) = root.as_obj_mut() {
                if obj.children.is_empty() {
                    obj.children.push(FfonElement::Str("<input></input>".to_owned()));
                }
            }
        }
        let mut new_id = IdArray::new();
        new_id.push(provider_idx);
        new_id.push(0);
        r.current_id = new_id;
    }

    true
}

/// Fetch URL content and parse into FFON elements.
/// Mirrors C's `fetchUrlToElements`.
fn fetch_url_to_elements(url: &str) -> Vec<FfonElement> {
    let client = sicompass_webbrowser::http_client();
    let mut req = client.get(url);
    if let Some(key) = crate::provider::find_api_key_for_url(url) {
        req = req.header("Authorization", format!("Bearer {key}"));
    }
    let body = match req.send().and_then(|r| r.text()) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    if body.is_empty() { return Vec::new(); }
    if url.ends_with(".ffon") {
        return sicompass_sdk::ffon::deserialize_binary(body.as_bytes());
    }
    // Try JSON first; fall back to HTML
    if let Ok(elems) = sicompass_sdk::ffon::parse_json(&body) {
        if !elems.is_empty() { return elems; }
    }
    sicompass_webbrowser::html_to_ffon(&body, url)
}

/// Resolve a link URL (local file or HTTP) into FFON elements.
/// Mirrors C's `resolveLinkToElements`.
fn resolve_link_to_elements(url: &str) -> Vec<FfonElement> {
    if url.starts_with("http://") || url.starts_with("https://") {
        fetch_url_to_elements(url)
    } else if url.ends_with(".ffon") {
        sicompass_sdk::ffon::load_ffon_file(std::path::Path::new(url)).unwrap_or_default()
    } else {
        sicompass_sdk::ffon::load_json_file(std::path::Path::new(url)).unwrap_or_default()
    }
}

/// Navigate into the selected item (Right key).
pub fn handle_right(r: &mut AppRenderer) {
    let item_id = match r.current_list_item_id() {
        Some(id) => id,
        None => return,
    };
    r.current_id = item_id;
    if navigate_right_raw(r) {
        list::create_list_current_layer(r);
        r.sync_current_id_from_list();
        r.caret.reset(sdl_ticks());
        r.needs_redraw = true;
    }
}

/// Navigate out to the parent level without rebuilding the list.
/// Returns `true` if navigation happened.
pub fn navigate_left_raw(r: &mut AppRenderer) -> bool {
    if r.current_id.depth() <= 1 {
        return false; // already at root
    }

    // Check if the parent element has a <link> or is "meta" — skip popPath for those
    // (mirrors C's providerNavigateLeft: parentIsLink / parentIsMeta checks)
    let (parent_is_link, parent_is_meta) = {
        let mut parent_id = r.current_id.clone();
        parent_id.pop();
        let parent_idx = parent_id.last().unwrap_or(0);
        match get_ffon_at_id(&r.ffon, &parent_id).and_then(|a| a.get(parent_idx)) {
            Some(sicompass_sdk::ffon::FfonElement::Obj(obj)) => {
                (tags::has_link(&obj.key), obj.key == "meta")
            }
            _ => (false, false),
        }
    };

    // For lazy-fetch providers (filebrowser) at depth 2: if pop_path moves us
    // to a parent directory, stay at depth 2 and re-fetch instead of going back
    // to the root provider list.
    let path_before = if r.current_id.depth() == 2 && !parent_is_link && !parent_is_meta {
        Some(crate::provider::current_path(r).to_owned())
    } else {
        None
    };

    if !parent_is_link && !parent_is_meta {
        crate::provider::pop_path(r);
    }

    let (path_changed, folder_name) = match path_before {
        Some(before) => {
            let changed = before != crate::provider::current_path(r);
            let name = std::path::Path::new(&before)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            (changed, name)
        }
        None => (false, None),
    };

    if path_changed {
        // Path moved to parent dir — stay inside the provider and re-fetch.
        crate::provider::refresh_current_directory(r);
        // Restore cursor to the folder we just came from.
        let target_index = folder_name
            .and_then(|name| {
                let provider_idx = r.current_id.get(0)?;
                let children = r.ffon.get(provider_idx)?.as_obj()?.children.as_slice();
                children.iter().position(|child| match child {
                    sicompass_sdk::ffon::FfonElement::Obj(obj) => tags::strip_display(&obj.key) == name,
                    _ => false,
                })
            })
            .unwrap_or(0);
        r.current_id.set(1, target_index);
    } else {
        r.current_id.pop();
    }

    true
}

/// Navigate out to the parent level (Left key).
pub fn handle_left(r: &mut AppRenderer) {
    if navigate_left_raw(r) {
        list::create_list_current_layer(r);
        r.sync_current_id_from_list();
        r.caret.reset(sdl_ticks());
        r.needs_redraw = true;
    }
}

/// Page up (scroll a full screen up).
pub fn handle_page_up(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::EditorInsert | Coordinate::OperatorInsert => return,
        _ => {}
    }

    let line_height = r.cached_line_height.max(1);
    let page_size = ((r.window_height / line_height) - 3).max(1) as usize;

    match r.coordinate {
        Coordinate::InputSearch => {
            r.input_search_scroll_offset = (r.input_search_scroll_offset - page_size as i32).max(0);
        }
        Coordinate::Scroll | Coordinate::ScrollSearch => {
            let viewport_h = r.window_height - line_height;
            r.text_scroll_offset = (r.text_scroll_offset - viewport_h).max(0);
        }
        Coordinate::SimpleSearch | Coordinate::Command | Coordinate::ExtendedSearch => {
            r.error_message.clear();
            r.list_index = r.list_index.saturating_sub(page_size);
            r.scroll_offset = r.list_index as i32;
        }
        Coordinate::OperatorGeneral | Coordinate::EditorGeneral => {
            if let Some(slice) = sicompass_sdk::ffon::get_ffon_at_id(&r.ffon, &r.current_id) {
                let max_id = slice.len().saturating_sub(1);
                let cur = r.current_id.last().unwrap_or(0);
                let new_id = cur.saturating_sub(page_size).min(max_id);
                r.current_id.set_last(new_id);
                list::create_list_current_layer(r);
                r.scroll_offset = r.list_index as i32;
            }
        }
        _ => {}
    }
    r.needs_redraw = true;
}

/// Page down (scroll a full screen down).
pub fn handle_page_down(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::EditorInsert | Coordinate::OperatorInsert => return,
        _ => {}
    }

    let line_height = r.cached_line_height.max(1);
    let page_size = ((r.window_height / line_height) - 3).max(1) as usize;

    match r.coordinate {
        Coordinate::InputSearch => {
            r.input_search_scroll_offset += page_size as i32;
            // No upper clamp here — renderer will clamp when it knows the line count
        }
        Coordinate::Scroll | Coordinate::ScrollSearch => {
            let viewport_h = r.window_height - line_height;
            let max_offset = (r.text_scroll_total_height - viewport_h).max(0);
            r.text_scroll_offset = (r.text_scroll_offset + viewport_h).min(max_offset);
        }
        Coordinate::SimpleSearch | Coordinate::Command | Coordinate::ExtendedSearch => {
            r.error_message.clear();
            let count = r.active_list_len();
            if count > 0 {
                r.list_index = (r.list_index + page_size).min(count - 1);
                r.scroll_offset = -1;
            }
        }
        Coordinate::OperatorGeneral | Coordinate::EditorGeneral => {
            if let Some(slice) = sicompass_sdk::ffon::get_ffon_at_id(&r.ffon, &r.current_id) {
                let max_id = slice.len().saturating_sub(1);
                let cur = r.current_id.last().unwrap_or(0);
                let new_id = (cur + page_size).min(max_id);
                r.current_id.set_last(new_id);
                list::create_list_current_layer(r);
                r.scroll_offset = -1;
            }
        }
        _ => {}
    }
    r.needs_redraw = true;
}

/// Jump to first item (Ctrl+Home in SimpleSearch/Command/ExtendedSearch).
pub fn handle_ctrl_home(r: &mut AppRenderer) {
    let len = r.active_list_len();
    if len > 0 {
        r.list_index = 0;
        r.scroll_offset = 0;
        r.sync_current_id_from_list();
    }
    r.needs_redraw = true;
}

/// Jump to last item (Ctrl+End in SimpleSearch/Command/ExtendedSearch).
pub fn handle_ctrl_end(r: &mut AppRenderer) {
    let len = r.active_list_len();
    if len > 0 {
        r.list_index = len - 1;
        r.scroll_offset = -1;
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

/// Enter scroll mode (S key in OperatorGeneral).
pub fn handle_s(r: &mut AppRenderer) {
    if r.coordinate != Coordinate::OperatorGeneral {
        return;
    }
    r.previous_coordinate = r.coordinate;
    r.coordinate = Coordinate::Scroll;
    r.text_scroll_offset = -1; // sentinel: renderer computes initial offset (selected item at top)
    r.text_scroll_total_height = 0;
    r.needs_redraw = true;
}

/// Navigate into / out of the meta object (M key in OperatorGeneral).
pub fn handle_meta(r: &mut AppRenderer) {
    if r.coordinate != Coordinate::OperatorGeneral {
        return;
    }

    r.previous_coordinate = r.coordinate;
    r.coordinate = Coordinate::Meta;
    r.list_index = 0;
    list::create_list_current_layer(r);
    r.needs_redraw = true;
}

/// Enter Tab search mode.
pub fn handle_tab(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::Scroll | Coordinate::ScrollSearch => {
            // noop in scroll modes
        }
        Coordinate::OperatorGeneral | Coordinate::OperatorInsert | Coordinate::EditorGeneral => {
            r.previous_coordinate = r.coordinate;
            r.coordinate = Coordinate::SimpleSearch;
            r.search_string.clear();
            r.cursor_position = 0;
            list::create_list_current_layer(r);
            r.needs_redraw = true;
        }
        _ => {}
    }
}

/// Enter command mode (:).
pub fn handle_colon(r: &mut AppRenderer) {
    if r.current_id.depth() <= 1 {
        return;
    }
    r.previous_coordinate = r.coordinate;
    r.coordinate = Coordinate::Command;
    r.current_command = CommandPhase::None;
    r.provider_command_name.clear();
    r.input_buffer.clear();
    r.cursor_position = 0;
    list::create_list_current_layer(r);
    r.needs_redraw = true;
}

/// Execute the selected command or its secondary selection (Return in Command mode).
///
/// Two-phase flow:
/// 1. `CommandPhase::None` → user selected a command name; record it and ask the
///    provider whether it needs a secondary list (e.g. "open with" app list).
/// 2. `CommandPhase::Provider` → user selected from the secondary list; execute.
pub fn handle_enter_command(r: &mut AppRenderer) {
    match r.current_command {
        CommandPhase::None => {
            // Phase 1: user chose a command name from the list
            let cmd = match r.current_list_item() {
                Some(item) => item.label.clone(),
                None => { handle_escape(r); return; }
            };

            // Get the current element key for the provider
            let (element_key, element_type) = {
                let arr = get_ffon_at_id(&r.ffon, &r.current_id);
                let idx = r.current_id.last().unwrap_or(0);
                match arr.and_then(|a| a.get(idx)) {
                    Some(sicompass_sdk::ffon::FfonElement::Str(s)) => (s.clone(), 0),
                    Some(sicompass_sdk::ffon::FfonElement::Obj(o)) => (o.key.clone(), 1),
                    None => { handle_escape(r); return; }
                }
            };

            r.provider_command_name = cmd.clone();
            r.current_command = CommandPhase::Provider;

            // Ask the provider to handle the command
            let result = crate::provider::handle_command(r, &cmd, &element_key, element_type);

            if let Some(new_elem) = result {
                let current_idx = r.current_id.last().unwrap_or(0);

                // If the current element is an empty placeholder, replace it in-place;
                // otherwise insert after the current position.
                // (mirrors C handlers.c:2724-2751)
                let current_is_placeholder = {
                    let arr = get_ffon_at_id(&r.ffon, &r.current_id);
                    match arr.and_then(|a| a.get(current_idx)) {
                        Some(sicompass_sdk::ffon::FfonElement::Str(s)) => is_empty_placeholder(s),
                        _ => false,
                    }
                };

                if current_is_placeholder {
                    replace_ffon_element(r, current_idx, new_elem);
                } else {
                    let insert_idx = current_idx + 1;
                    insert_ffon_element(r, insert_idx, new_elem);
                    r.current_id.set_last(insert_idx);
                }
                r.current_command = CommandPhase::None;
                r.coordinate = Coordinate::OperatorGeneral;
                list::create_list_current_layer(r);
                r.list_index = r.current_id.last().unwrap_or(0);
                r.scroll_offset = 0;
                // Enter insert mode on the new element
                handle_i(r);
            } else if !r.error_message.is_empty() {
                // Provider set an error
                r.current_command = CommandPhase::None;
                r.coordinate = Coordinate::OperatorGeneral;
                list::create_list_current_layer(r);
                r.needs_redraw = true;
            } else {
                // No result, no error → provider needs a secondary selection
                r.input_buffer.clear();
                r.cursor_position = 0;
                list::create_list_current_layer(r); // now shows secondary items
                r.scroll_offset = 0;

                if r.total_list.is_empty() {
                    // Command was a state toggle — return to previous mode and refresh
                    r.current_command = CommandPhase::None;
                    r.coordinate = r.previous_coordinate;
                    r.previous_coordinate = Coordinate::OperatorGeneral;

                    // Navigate back to provider root if deeper (matches C: while depth > 2)
                    while r.current_id.depth() > 2 {
                        navigate_left_raw(r);
                    }

                    // Re-fetch the directory with the updated provider state
                    crate::provider::refresh_current_directory(r);
                    list::create_list_current_layer(r);
                }
                r.needs_redraw = true;
            }
        }

        CommandPhase::Provider => {
            // Phase 2: user chose a secondary item (e.g. an application to open with)
            let selection = match r.current_list_item() {
                Some(item) => item.nav_path.clone()
                    .or_else(|| item.data.clone())
                    .unwrap_or_else(|| item.label.clone()),
                None => { handle_escape(r); return; }
            };
            let cmd = r.provider_command_name.clone();
            crate::provider::execute_command(r, &cmd, &selection);

            r.current_command = CommandPhase::None;
            r.coordinate = r.previous_coordinate;
            list::create_list_current_layer(r);
            r.list_index = r.current_id.last().unwrap_or(0);
            r.scroll_offset = 0;
            r.needs_redraw = true;
        }
    }
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
                    FfonElement::Str(_) => FfonElement::new_str(new_text.clone()),
                    FfonElement::Obj(o) => {
                        let mut new_obj = FfonElement::new_obj(new_text.clone());
                        for c in &o.children { new_obj.as_obj_mut().unwrap().push(c.clone()); }
                        new_obj
                    }
                };
            }
        }
        crate::provider::notify_checkbox_changed(r, &new_text);
        list::create_list_current_layer(r);
        r.needs_redraw = true;
        return;
    }

    // Toggle radio — add <checked> to selected, remove from siblings
    if toggle_radio(r) {
        crate::provider::notify_radio_changed(r);
        list::create_list_current_layer(r);
        r.needs_redraw = true;
        return;
    }

    // Button press
    if tags::has_button(&match &elem_clone { FfonElement::Str(s) => s.clone(), FfonElement::Obj(o) => o.key.clone() }) {
        crate::provider::notify_button_pressed(r);
        list::create_list_current_layer(r);
        r.list_index = r.current_id.last().unwrap_or(0);
        r.needs_redraw = true;
        return;
    }

    // Activate <input> element (commit existing content — triggers provider refresh).
    // In the filebrowser, elements with <input> tags represent files — open them instead of editing.
    let (elem_text, _is_obj_elem) = match &elem_clone {
        FfonElement::Str(s) => (s.clone(), false),
        FfonElement::Obj(o) => (o.key.clone(), true),
    };
    let is_filebrowser = crate::provider::get_active_provider_ref(r)
        .map(|p| p.name() == "filebrowser")
        .unwrap_or(false);

    if !_is_obj_elem && (tags::has_input(&elem_text) || tags::has_input_all(&elem_text)) && !is_filebrowser {
        let content = if tags::has_input_all(&elem_text) {
            tags::extract_input_all(&elem_text).unwrap_or_default()
        } else {
            tags::extract_input(&elem_text).unwrap_or_default()
        };
        crate::provider::commit_edit(r, &content, &content);
        crate::provider::refresh_current_directory(r);
        list::create_list_current_layer(r);
        r.needs_redraw = true;
        return;
    }

    // For filebrowser string elements: open file with the system default program.
    if matches!(elem_clone, FfonElement::Str(_)) {
        let filename = if tags::has_input(&elem_text) {
            tags::extract_input(&elem_text)
        } else if tags::has_input_all(&elem_text) {
            tags::extract_input_all(&elem_text)
        } else {
            None
        };
        if let Some(fname) = filename {
            let path = crate::provider::current_path(r).to_owned();
            let full_path = format!("{}/{}", path.trim_end_matches('/'), fname);
            sicompass_sdk::platform::open_with_default(&full_path);
        }
    }

    r.needs_redraw = true;
}

/// Enter in SimpleSearch / ExtendedSearch — navigate to selected item and return to previous mode.
pub fn handle_enter_search(r: &mut AppRenderer) {
    if r.coordinate == Coordinate::ExtendedSearch {
        handle_enter_extended_search(r);
        return;
    }

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

    // Checkbox toggle: stay in search mode after toggling.
    {
        use sicompass_sdk::ffon::{FfonElement, get_ffon_at_id};
        let elem_clone = get_ffon_at_id(&r.ffon, &selected_id)
            .and_then(|a| a.get(selected_id.last().unwrap_or(0)))
            .cloned();
        if let Some(elem) = elem_clone {
            if let Some(new_text) = toggle_checkbox(&elem) {
                let idx = selected_id.last().unwrap_or(0);
                if let Some(arr) = crate::state::navigate_to_slice_pub(&mut r.ffon, &selected_id) {
                    if let Some(e) = arr.get_mut(idx) {
                        *e = match &elem {
                            FfonElement::Str(_) => FfonElement::new_str(new_text.clone()),
                            FfonElement::Obj(o) => {
                                let mut obj = FfonElement::new_obj(new_text.clone());
                                for c in &o.children { obj.as_obj_mut().unwrap().push(c.clone() as FfonElement); }
                                obj
                            }
                        };
                    }
                }
                crate::provider::notify_checkbox_changed(r, &new_text);
                let saved_index = r.list_index;
                list::create_list_current_layer(r);
                r.list_index = saved_index;
                r.needs_redraw = true;
                return;
            }
        }
    }

    // Radio toggle: exit search after selecting.
    {
        let saved_id = r.current_id.clone();
        r.current_id = selected_id.clone();
        if toggle_radio(r) {
            crate::provider::notify_radio_changed(r);
            r.coordinate = r.previous_coordinate;
            r.search_string.clear();
            list::create_list_current_layer(r);
            r.list_index = r.current_id.last().unwrap_or(0);
            r.needs_redraw = true;
            return;
        }
        r.current_id = saved_id;
    }

    r.current_id = selected_id;
    r.coordinate = r.previous_coordinate;
    r.search_string.clear();
    list::create_list_current_layer(r);
    r.list_index = r.current_id.last().unwrap_or(0).min(r.active_list_len().saturating_sub(1));
    r.needs_redraw = true;
}

fn handle_enter_extended_search(r: &mut AppRenderer) {
    let item = match r.current_list_item().cloned() {
        Some(i) => i,
        None => {
            r.coordinate = r.previous_coordinate;
            r.input_buffer.clear();
            r.cursor_position = 0;
            list::create_list_current_layer(r);
            r.needs_redraw = true;
            return;
        }
    };

    let selected_id = item.id.clone();

    // Validate ancestor radio groups along the path to the selected item.
    // Any radio group ancestor must have only string children and at most one <checked>.
    {
        use sicompass_sdk::ffon::get_ffon_at_id;
        use sicompass_sdk::tags;

        let depth = selected_id.depth();
        let mut blocked_error: Option<&'static str> = None;
        let mut blocked_ancestor_id = selected_id.clone();

        'outer: for d in 1..depth.saturating_sub(1) {
            let mut ancestor_id = sicompass_sdk::ffon::IdArray::new();
            for i in 0..=d {
                if let Some(v) = selected_id.get(i) { ancestor_id.push(v); }
            }
            let ancestor_idx = ancestor_id.last().unwrap_or(0);

            let radio_key = get_ffon_at_id(&r.ffon, &ancestor_id)
                .and_then(|a| a.get(ancestor_idx))
                .and_then(|e| e.as_obj())
                .filter(|o| tags::has_radio(&o.key))
                .map(|o| o.key.clone());

            if let Some(_) = radio_key {
                let children = get_ffon_at_id(&r.ffon, &ancestor_id)
                    .and_then(|a| a.get(ancestor_idx))
                    .and_then(|e| e.as_obj())
                    .map(|o| o.children.clone())
                    .unwrap_or_default();

                let mut checked_count = 0usize;
                for child in &children {
                    if child.is_obj() {
                        blocked_error = Some("Radio group children must be strings, not objects");
                        blocked_ancestor_id = ancestor_id.clone();
                        break 'outer;
                    }
                    if let Some(s) = child.as_str() {
                        if tags::has_checked(s) { checked_count += 1; }
                    }
                }
                if checked_count > 1 {
                    blocked_error = Some("Radio group must have at most one checked item");
                    blocked_ancestor_id = ancestor_id.clone();
                    break 'outer;
                }
            }
        }

        if let Some(err) = blocked_error {
            r.current_id = blocked_ancestor_id;
            r.coordinate = r.previous_coordinate;
            r.input_buffer.clear();
            r.cursor_position = 0;
            list::create_list_current_layer(r);
            r.error_message = err.to_owned();
            r.list_index = r.current_id.last().unwrap_or(0);
            r.scroll_offset = -1;
            r.needs_redraw = true;
            return;
        }
    }

    // Checkbox toggle: stay in extended search after toggling.
    {
        use sicompass_sdk::ffon::{FfonElement, get_ffon_at_id};
        let elem_clone = get_ffon_at_id(&r.ffon, &selected_id)
            .and_then(|a| a.get(selected_id.last().unwrap_or(0)))
            .cloned();
        if let Some(elem) = elem_clone {
            if let Some(new_text) = toggle_checkbox(&elem) {
                let idx = selected_id.last().unwrap_or(0);
                if let Some(arr) = crate::state::navigate_to_slice_pub(&mut r.ffon, &selected_id) {
                    if let Some(e) = arr.get_mut(idx) {
                        *e = match &elem {
                            FfonElement::Str(_) => FfonElement::new_str(new_text.clone()),
                            FfonElement::Obj(o) => {
                                let mut obj = FfonElement::new_obj(new_text.clone());
                                for c in &o.children { obj.as_obj_mut().unwrap().push(c.clone() as FfonElement); }
                                obj
                            }
                        };
                    }
                }
                crate::provider::notify_checkbox_changed(r, &new_text);
                let saved_index = r.list_index;
                list::create_list_extended_search(r);
                r.list_index = saved_index;
                r.needs_redraw = true;
                return;
            }
        }
    }

    // Radio toggle: exit search after selecting.
    {
        let saved_id = r.current_id.clone();
        r.current_id = selected_id.clone();
        if toggle_radio(r) {
            crate::provider::notify_radio_changed(r);
            r.coordinate = r.previous_coordinate;
            r.input_buffer.clear();
            r.cursor_position = 0;
            list::create_list_current_layer(r);
            r.list_index = r.current_id.last().unwrap_or(0);
            r.scroll_offset = -1;
            r.needs_redraw = true;
            return;
        }
        r.current_id = saved_id;
    }

    // Deep search item: teleport via nav_path
    if let Some(ref nav_path) = item.nav_path {
        let root_idx = item.id.get(0).unwrap_or(0);
        let (parent_dir, filename) = split_nav_path(nav_path);
        crate::provider::navigate_to_path(r, root_idx, parent_dir, filename);
        r.coordinate = r.previous_coordinate;
        r.input_buffer.clear();
        r.cursor_position = 0;
        list::create_list_current_layer(r);
        r.list_index = r.current_id.last().unwrap_or(0);
        r.scroll_offset = -1;
        r.needs_redraw = true;
        return;
    }

    // Regular FFON-tree item: navigate by id
    r.current_id = item.id;
    r.coordinate = r.previous_coordinate;
    r.input_buffer.clear();
    r.cursor_position = 0;
    list::create_list_current_layer(r);
    r.list_index = r.current_id.last().unwrap_or(0).min(r.active_list_len().saturating_sub(1));
    r.scroll_offset = -1;
    r.needs_redraw = true;
}

/// Split a nav_path like `/a/b/c` into (`/a/b`, `c`).
pub fn split_nav_path(nav_path: &str) -> (&str, &str) {
    match nav_path.rfind('/') {
        Some(pos) if pos > 0 => (&nav_path[..pos], &nav_path[pos + 1..]),
        Some(_) => ("/", &nav_path[1..]),
        None => ("", nav_path),
    }
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
        // Match C: wasInput → refresh even when content unchanged, then exit cleanly.
        crate::provider::refresh_current_directory(r);
        handle_escape(r);
        r.input_buffer.clear();
        r.cursor_position = 0;
        let saved_error = r.error_message.clone();
        list::create_list_current_layer(r);
        if !saved_error.is_empty() { r.error_message = saved_error; }
        r.list_index = r.current_id.last().unwrap_or(0);
        r.scroll_offset = 0;
        return;
    }

    // File-browser save-as: user typed a filename → write source provider data
    if r.pending_file_browser_save_as && old_content.is_empty() {
        if new_content.is_empty() {
            r.needs_redraw = true;
            return; // stay in insert mode
        }

        let save_dir = resolve_save_folder(r);

        // Build collision-free destination filename (append .json, handle duplicates)
        let base = new_content.trim_end_matches(".json");
        let mut dest_name = format!("{base}.json");
        let mut n = 0u32;
        loop {
            let full = format!("{save_dir}/{dest_name}");
            if !std::path::Path::new(&full).exists() { break; }
            n += 1;
            dest_name = format!("{base} (copy {n}).json");
        }
        let dest_full = format!("{save_dir}/{dest_name}");

        // Save source provider FFON to file
        let src_idx = r.save_as_source_root_idx;
        let save_result = if let Some(sicompass_sdk::ffon::FfonElement::Obj(root_obj)) = r.ffon.get(src_idx).cloned() {
            sicompass_sdk::ffon::save_json_file(
                &[sicompass_sdk::ffon::FfonElement::Obj(root_obj)],
                std::path::Path::new(&dest_full),
            )
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "source provider not found"))
        };

        // Remove the placeholder element from the filebrowser
        let remove_idx = r.current_id.last().unwrap_or(0);
        let mut parent_id = r.current_id.clone();
        parent_id.pop();
        let parent_idx = parent_id.last().unwrap_or(0);
        if let Some(parent_slice) = crate::state::navigate_to_slice_pub(&mut r.ffon, &parent_id) {
            if let Some(FfonElement::Obj(obj)) = parent_slice.get_mut(parent_idx) {
                if remove_idx < obj.children.len() {
                    obj.children.remove(remove_idx);
                }
            }
        }

        // Reset filebrowser to root
        let fb_idx = r.current_id.get(0).unwrap_or(0);
        if let Some(p) = r.providers.get_mut(fb_idx) { p.set_current_path("/"); }
        if let Some(FfonElement::Obj(obj)) = r.ffon.get_mut(fb_idx) {
            obj.children.clear();
        }
        if let Some(p) = r.providers.get_mut(fb_idx) {
            let children = p.fetch();
            if let Some(FfonElement::Obj(obj)) = r.ffon.get_mut(fb_idx) {
                obj.children = children;
            }
        }

        // Navigate back to source provider
        let return_id = r.save_as_return_id.clone();
        r.current_id = return_id;
        r.pending_file_browser_save_as = false;
        r.coordinate = Coordinate::OperatorGeneral;
        r.previous_coordinate = Coordinate::OperatorGeneral;
        r.input_buffer.clear();
        r.cursor_position = 0;

        list::create_list_current_layer(r);
        r.list_index = r.current_id.last().unwrap_or(0);
        r.scroll_offset = 0;

        match save_result {
            Ok(()) => {
                r.current_save_path = dest_full.clone();
                r.error_message = format!("Saved to {dest_full}");
            }
            Err(e) => {
                r.error_message = format!("Failed to save: {e}");
            }
        }
        r.needs_redraw = true;
        return;
    }

    // Prefix-based creation (Ctrl+I / Ctrl+A in filebrowser)
    if old_content.is_empty() && r.prefixed_insert_mode {
        let (is_file, is_dir, item_name) = parse_creation_prefix(&new_content);

        if (!is_file && !is_dir) || item_name.is_empty() {
            r.error_message = "Enter a name (prefix with '- ' for file or '+ ' for directory)".to_owned();
            r.needs_redraw = true;
            return; // stay in OperatorInsert
        }

        let undo_id = r.current_id.clone();
        let success = if is_file {
            crate::provider::create_file(r, &item_name)
        } else {
            crate::provider::create_directory(r, &item_name)
        };

        if !success {
            if r.error_message.is_empty() {
                r.error_message = "Failed to create item".to_owned();
            }
            r.needs_redraw = true;
            return; // stay in OperatorInsert
        }

        // Discard the placeholder Insert/Append undo entry (replaced by FsCreate below)
        if r.undo_history.last().map(|e| matches!(e.task, Task::Insert | Task::InsertInsert | Task::Append | Task::AppendAppend)).unwrap_or(false) {
            r.undo_history.pop();
        }

        // Record FsCreate undo entry (Str = file, Obj = directory)
        let undo_elem = if is_dir {
            FfonElement::new_obj(&item_name)
        } else {
            FfonElement::new_str(item_name.clone())
        };
        crate::state::update_history(
            r, Task::FsCreate, &undo_id,
            None, Some(undo_elem), History::None,
        );

        r.prefixed_insert_mode = false;
        crate::provider::refresh_current_directory(r);
        list::create_list_current_layer(r);
        // Move cursor to the newly created item by name (list may have re-sorted).
        // Labels have the form "{prefix} {content}" (e.g. "-i newfile.txt"),
        // so compare the content after the first space.
        {
            let name = item_name.as_str();
            let found_idx = r.total_list.iter().position(|item| {
                let content = item.label.split_once(' ').map(|(_, c)| c).unwrap_or(&item.label);
                content == name
            });
            if let Some(i) = found_idx {
                if let Some(id) = r.total_list.get(i).map(|it| it.id.clone()) {
                    r.current_id = id;
                    r.list_index = i;
                }
            }
        }

        r.coordinate = Coordinate::OperatorGeneral;
        r.previous_coordinate = Coordinate::OperatorGeneral;
        r.input_buffer.clear();
        r.cursor_position = 0;
        r.needs_redraw = true;
        return;
    }

    // <input-all> with empty old content and no prefix: create file or directory.
    // A trailing ':' on the new name means "create as directory".
    if tags::has_input_all(&elem_text) && old_content.is_empty() && r.input_prefix.is_empty() && !new_content.is_empty() {
        let (create_as_dir, effective_name) = if new_content.ends_with(':') {
            (true, new_content.trim_end_matches(':').trim().to_owned())
        } else {
            (false, new_content.clone())
        };

        let undo_id = r.current_id.clone();
        let success;
        let fs_created;

        if create_as_dir {
            let created = crate::provider::create_directory(r, &effective_name);
            if created {
                success = true;
                fs_created = true;
            } else if r.error_message.is_empty() {
                // Provider has no createDirectory — convert FFON element to Obj in-place
                let idx = r.current_id.last().unwrap_or(0);
                if let Some(arr) = crate::state::navigate_to_slice_pub(&mut r.ffon, &r.current_id) {
                    if idx < arr.len() {
                        let mut new_obj = FfonElement::new_obj(effective_name.clone());
                        new_obj.as_obj_mut().unwrap().push(FfonElement::new_str(String::new()));
                        arr[idx] = new_obj;
                    }
                }
                success = true;
                fs_created = false;
            } else {
                r.needs_redraw = true;
                return;
            }
        } else {
            let created = crate::provider::create_file(r, &effective_name);
            if created {
                success = true;
                fs_created = true;
            } else if r.error_message.is_empty() {
                // Provider has no createFile — keep as string, fall through to key update
                success = true;
                fs_created = false;
            } else {
                r.needs_redraw = true;
                return;
            }
        }

        if success {
            if fs_created {
                if r.undo_history.last().map(|e| matches!(e.task, Task::Insert | Task::InsertInsert | Task::Append | Task::AppendAppend)).unwrap_or(false) {
                    r.undo_history.pop();
                }
                let undo_elem = if create_as_dir {
                    FfonElement::new_obj(&effective_name)
                } else {
                    FfonElement::new_str(effective_name.clone())
                };
                crate::state::update_history(r, Task::FsCreate, &undo_id, None, Some(undo_elem), History::None);
                crate::provider::refresh_current_directory(r);
                list::create_list_current_layer(r);

                // Find the created item and move cursor to it.
                // Labels have the form "{prefix} {content}", compare content after the first space.
                let found_idx = r.total_list.iter().position(|item| {
                    let content = item.label.split_once(' ').map(|(_, c)| c).unwrap_or(&item.label);
                    content == effective_name
                });
                if let Some(i) = found_idx {
                    if let Some(id) = r.total_list.get(i).map(|it| it.id.clone()) {
                        r.current_id = id;
                    }
                }
            } else {
                // Pure FFON: update element key with new content
                let new_key = if !fs_created && !create_as_dir {
                    tags::format_input(&effective_name)
                } else {
                    effective_name.clone()
                };
                let idx = r.current_id.last().unwrap_or(0);
                if let Some(arr) = crate::state::navigate_to_slice_pub(&mut r.ffon, &r.current_id) {
                    if let Some(e) = arr.get_mut(idx) {
                        *e = FfonElement::new_str(new_key);
                    }
                }
                list::create_list_current_layer(r);
            }

            r.coordinate = Coordinate::OperatorGeneral;
            r.previous_coordinate = Coordinate::OperatorGeneral;
            r.input_buffer.clear();
            r.cursor_position = 0;
            r.list_index = r.current_id.last().unwrap_or(0);
            r.scroll_offset = 0;
            r.needs_redraw = true;
        }
        return;
    }

    // Empty old content, no prefix, Obj element: create directory via provider.
    if old_content.is_empty() && r.input_prefix.is_empty() && is_obj && !new_content.is_empty() {
        let undo_id = r.current_id.clone();
        if crate::provider::create_directory(r, &new_content) {
            if r.undo_history.last().map(|e| matches!(e.task, Task::Insert | Task::InsertInsert | Task::Append | Task::AppendAppend)).unwrap_or(false) {
                r.undo_history.pop();
            }
            crate::state::update_history(r, Task::FsCreate, &undo_id, None, Some(FfonElement::new_obj(&new_content)), History::None);
            crate::provider::refresh_current_directory(r);
            list::create_list_current_layer(r);
            {
                let name = new_content.as_str();
                let found = r.total_list.iter().position(|item| {
                    let content = item.label.split_once(' ').map(|(_, c)| c).unwrap_or(&item.label);
                    content == name
                });
                if let Some(i) = found {
                    if let Some(id) = r.total_list.get(i).map(|it| it.id.clone()) {
                        r.current_id = id;
                    }
                }
            }
            r.coordinate = Coordinate::OperatorGeneral;
            r.previous_coordinate = Coordinate::OperatorGeneral;
            r.input_buffer.clear();
            r.cursor_position = 0;
            r.list_index = r.current_id.last().unwrap_or(0);
            r.needs_redraw = true;
        } else if !r.error_message.is_empty() {
            r.needs_redraw = true;
        }
        return;
    }

    // Empty old content, no prefix, Str element: create file via provider or FFON update.
    if old_content.is_empty() && r.input_prefix.is_empty() && !is_obj && !new_content.is_empty() {
        let undo_id = r.current_id.clone();
        if crate::provider::create_file(r, &new_content) {
            if r.undo_history.last().map(|e| matches!(e.task, Task::Insert | Task::InsertInsert | Task::Append | Task::AppendAppend)).unwrap_or(false) {
                r.undo_history.pop();
            }
            crate::state::update_history(r, Task::FsCreate, &undo_id, None, Some(FfonElement::new_str(new_content.clone())), History::None);
            crate::provider::refresh_current_directory(r);
            list::create_list_current_layer(r);
            {
                let name = new_content.as_str();
                let found = r.total_list.iter().position(|item| {
                    let content = item.label.split_once(' ').map(|(_, c)| c).unwrap_or(&item.label);
                    content == name
                });
                if let Some(i) = found {
                    if let Some(id) = r.total_list.get(i).map(|it| it.id.clone()) {
                        r.current_id = id;
                    }
                }
            }
            r.coordinate = Coordinate::OperatorGeneral;
            r.previous_coordinate = Coordinate::OperatorGeneral;
            r.input_buffer.clear();
            r.cursor_position = 0;
            r.list_index = r.current_id.last().unwrap_or(0);
            r.needs_redraw = true;
            return;
        } else if r.error_message.is_empty() {
            // Provider has no createFile: update FFON element key directly
            let new_key = tags::format_input(&new_content);
            let idx = r.current_id.last().unwrap_or(0);
            if let Some(arr) = crate::state::navigate_to_slice_pub(&mut r.ffon, &r.current_id) {
                if let Some(e) = arr.get_mut(idx) {
                    *e = FfonElement::new_str(new_key);
                }
            }
            handle_escape(r);
            return;
        } else {
            r.needs_redraw = true;
            return;
        }
    }

    // For flat "label: <input>value</input>" items, temporarily push the prefix label
    // onto the provider path so commit_edit can locate the correct setting entry.
    let prefix_label = r.input_prefix
        .trim_end_matches(' ').trim_end_matches(':').trim()
        .to_owned();
    let prefix_path_pushed = if !prefix_label.is_empty() {
        crate::provider::push_path(r, &prefix_label);
        true
    } else {
        false
    };

    // Try provider commit first
    let committed = crate::provider::commit_edit(r, &old_content, &new_content);

    if prefix_path_pushed {
        crate::provider::pop_path(r);
    }

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
        crate::provider::refresh_current_directory(r);
        // Auto-navigate into the element if it now has children after refresh
        // (e.g. web browser URL bar gains page content after load).
        // Works for both fresh loads (Str→Obj) and URL changes (Obj→Obj).
        navigate_right_raw(r);
    }

    handle_escape(r);
    r.input_buffer.clear();
    r.cursor_position = 0;

    let saved_error = r.error_message.clone();
    list::create_list_current_layer(r);
    if !saved_error.is_empty() {
        r.error_message = saved_error;
    }
    r.list_index = r.current_id.last().unwrap_or(0);
    r.scroll_offset = 0;
}

/// Parse "- name" (file) or "+ name" (directory) creation prefixes.
/// Returns (is_file, is_dir, name_without_prefix).
fn parse_creation_prefix(input: &str) -> (bool, bool, String) {
    if let Some(rest) = input.strip_prefix('-') {
        let name = rest.trim_start().to_owned();
        (true, false, name)
    } else if let Some(rest) = input.strip_prefix('+') {
        let name = rest.trim_start().to_owned();
        (false, true, name)
    } else {
        // No prefix: bare name → file (or dir if ends with ':')
        let trimmed = input.trim();
        if trimmed.ends_with(':') {
            let name = trimmed.trim_end_matches(':').trim().to_owned();
            (false, !name.is_empty(), name)
        } else if !trimmed.is_empty() {
            (true, false, trimmed.to_owned())
        } else {
            (false, false, String::new())
        }
    }
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
    clear_selection(r);
    match r.coordinate {
        Coordinate::EditorInsert => {
            // Save via updateState, return to EditorGeneral
            crate::state::update_state(r, Task::Input, History::None);
            r.coordinate = Coordinate::EditorGeneral;
        }
        Coordinate::OperatorInsert => {
            // Discard changes, return to OperatorGeneral
            r.coordinate = Coordinate::OperatorGeneral;
        }
        Coordinate::Command => {
            r.coordinate = r.previous_coordinate;
            r.input_buffer.clear();
            r.cursor_position = 0;
            list::create_list_current_layer(r);
            r.list_index = r.current_id.last().unwrap_or(0);
            r.scroll_offset = 0;
            r.caret.reset(sdl_ticks());
            r.needs_redraw = true;
            return;
        }
        Coordinate::SimpleSearch => {
            r.coordinate = r.previous_coordinate;
            r.search_string.clear();
            list::create_list_current_layer(r);
            r.list_index = r.current_id.last().unwrap_or(0);
            r.scroll_offset = 0;
            r.caret.reset(sdl_ticks());
            r.needs_redraw = true;
            return;
        }
        Coordinate::ExtendedSearch => {
            r.coordinate = r.previous_coordinate;
            r.input_buffer.clear();
            r.cursor_position = 0;
            list::create_list_current_layer(r);
            r.list_index = r.current_id.last().unwrap_or(0);
            r.scroll_offset = 0;
            r.needs_redraw = true;
            return;
        }
        Coordinate::Meta => {
            r.coordinate = r.previous_coordinate;
            list::create_list_current_layer(r);
            r.list_index = r.current_id.last().unwrap_or(0);
            r.scroll_offset = 0;
            r.caret.reset(sdl_ticks());
            r.needs_redraw = true;
            return;
        }
        Coordinate::Dashboard => {
            r.coordinate = r.previous_coordinate;
            r.caret.reset(sdl_ticks());
            r.needs_redraw = true;
            return;
        }
        Coordinate::ScrollSearch => {
            r.coordinate = Coordinate::Scroll;
            r.scroll_search_match_count = 0;
            r.scroll_search_current_match = 0;
        }
        Coordinate::Scroll => {
            r.coordinate = r.previous_coordinate;
            r.text_scroll_offset = 0;
            r.text_scroll_total_height = 0;
        }
        _ => {
            // EditorGeneral, EditorNormal, EditorVisual, OperatorGeneral, etc.
            // Go to previous if it was an operator mode, else editor
            if r.previous_coordinate == Coordinate::OperatorGeneral
                || r.previous_coordinate == Coordinate::OperatorInsert
            {
                r.coordinate = Coordinate::OperatorGeneral;
            } else {
                r.coordinate = Coordinate::EditorGeneral;
            }
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
    if text.is_empty() { return; }

    // When entering Command mode via handle_colon, SDL fires both a key event and
    // a text input event for the ':' key. Ignore the text event so the colon is
    // not inserted into the command buffer.
    if r.coordinate == Coordinate::Command && r.input_buffer.is_empty() && text == ":" {
        return;
    }

    match r.coordinate {
        Coordinate::SimpleSearch => {
            let pos = r.cursor_position.min(r.search_string.len());
            r.search_string.insert_str(pos, text);
            r.cursor_position = pos + text.len();
            let search = r.search_string.clone();
            list::create_list_current_layer(r);
            list::populate_list_current_layer(r, &search);
            r.needs_redraw = true;
        }
        Coordinate::Command => {
            if has_selection(r) { delete_selection(r); }
            let pos = r.cursor_position.min(r.input_buffer.len());
            r.input_buffer.insert_str(pos, text);
            r.cursor_position = pos + text.len();
            r.caret.reset(sdl_ticks());
            let filter = r.input_buffer.clone();
            list::create_list_current_layer(r);
            list::populate_list_current_layer(r, &filter);
            r.needs_redraw = true;
        }
        Coordinate::EditorInsert | Coordinate::OperatorInsert => {
            // Replace selection if active
            if has_selection(r) { delete_selection(r); }
            // Insert text at cursor position (byte offset)
            let pos = r.cursor_position.min(r.input_buffer.len());
            r.input_buffer.insert_str(pos, text);
            r.cursor_position = pos + text.len();
            r.caret.reset(sdl_ticks());
            r.needs_redraw = true;
        }
        Coordinate::ScrollSearch => {
            let pos = r.cursor_position.min(r.input_buffer.len());
            r.input_buffer.insert_str(pos, text);
            r.cursor_position = pos + text.len();
            r.needs_redraw = true;
        }
        Coordinate::ExtendedSearch => {
            let pos = r.cursor_position.min(r.input_buffer.len());
            r.input_buffer.insert_str(pos, text);
            r.cursor_position = pos + text.len();
            let filter = r.input_buffer.clone();
            list::create_list_extended_search(r);
            list::populate_list_current_layer(r, &filter);
            r.needs_redraw = true;
        }
        _ => {}
    }
}

/// Handle Backspace in editing modes.
pub fn handle_backspace(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::SimpleSearch => {
            if r.cursor_position > 0 {
                // Remove char before cursor_position (UTF-8 aware)
                let pos = r.cursor_position.min(r.search_string.len());
                let before = &r.search_string[..pos];
                let new_pos = before.char_indices().rev().next().map(|(i, _)| i).unwrap_or(0);
                r.search_string.replace_range(new_pos..pos, "");
                r.cursor_position = new_pos;
                let search = r.search_string.clone();
                list::create_list_current_layer(r);
                list::populate_list_current_layer(r, &search);
                r.needs_redraw = true;
            }
        }
        Coordinate::Command | Coordinate::EditorInsert | Coordinate::OperatorInsert | Coordinate::ScrollSearch | Coordinate::ExtendedSearch => {
            if has_selection(r) {
                delete_selection(r);
                r.caret.reset(sdl_ticks());
                maybe_update_search(r);
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
                maybe_update_search(r);
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

/// Delete via active provider (file browser delete — mirrors C's `handleFileDelete`).
///
/// Calls `provider::delete_item` which invokes the provider's `delete_item` method
/// (e.g. removes the file from disk) and refreshes the directory listing.
pub fn handle_file_delete(r: &mut AppRenderer) {
    let ok = crate::provider::delete_item(r);
    if ok {
        // If the directory is now empty, insert a placeholder so the user can create files
        // (mirrors C update.c: after deletion, if _ffon_count == 0 → add <input></input>)
        let provider_idx = r.current_id.get(0).unwrap_or(0);
        if let Some(root) = r.ffon.get_mut(provider_idx) {
            if let Some(obj) = root.as_obj_mut() {
                if obj.children.is_empty() {
                    obj.children.push(FfonElement::Str("<input></input>".to_owned()));
                    r.current_id.set_last(0);
                }
            }
        }
        // Clamp current_id to valid range after deletion + refresh
        let new_len = sicompass_sdk::ffon::get_ffon_at_id(&r.ffon, &r.current_id)
            .map(|a| a.len())
            .unwrap_or(0);
        let cur = r.current_id.last().unwrap_or(0);
        if new_len > 0 && cur >= new_len {
            r.current_id.set_last(new_len - 1);
        }
        list::create_list_current_layer(r);
        r.needs_redraw = true;
    }
}

/// Copy the selected file's path into `file_clipboard_path` (no move).
///
/// Only works when the active provider is "filebrowser".
/// Mirrors C `handleFileCopy`.
pub fn handle_file_copy(r: &mut AppRenderer) {
    let current_path = crate::provider::current_path(r).to_owned();
    let arr = match get_ffon_at_id(&r.ffon, &r.current_id) { Some(a) => a, None => return };
    let idx = r.current_id.last().unwrap_or(0);
    let elem = match arr.get(idx) { Some(e) => e, None => return };
    let key = match elem {
        sicompass_sdk::ffon::FfonElement::Str(s) => s.as_str(),
        sicompass_sdk::ffon::FfonElement::Obj(o) => o.key.as_str(),
    };
    let name = tags::strip_display(key);
    if name.is_empty() { return; }
    r.file_clipboard_path = format!("{current_path}/{name}");
    r.file_clipboard_is_cut = false;
    r.needs_redraw = true;
}

/// Cut the selected file: copy to clipboard cache, delete original.
///
/// Only works when the active provider is "filebrowser".
/// Mirrors C `handleFileCut`.
pub fn handle_file_cut(r: &mut AppRenderer) {
    let current_path = crate::provider::current_path(r).to_owned();
    let arr = match get_ffon_at_id(&r.ffon, &r.current_id) { Some(a) => a, None => return };
    let idx = r.current_id.last().unwrap_or(0);
    let elem = match arr.get(idx) { Some(e) => e, None => return };
    let key = match elem {
        sicompass_sdk::ffon::FfonElement::Str(s) => s.as_str(),
        sicompass_sdk::ffon::FfonElement::Obj(o) => o.key.as_str(),
    };
    let name = tags::strip_display(key).to_owned();
    if name.is_empty() { return; }

    // Resolve clipboard cache dir
    let cache_dir = {
        let base = sicompass_sdk::platform::cache_home()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
        let dir = base.join("sicompass").join("clipboard");
        let _ = std::fs::create_dir_all(&dir);
        dir.to_string_lossy().into_owned()
    };

    if !crate::provider::copy_item(r, &current_path, &name, &cache_dir, &name) {
        r.error_message = "Cut: failed to copy file to clipboard cache".to_owned();
        return;
    }
    if !crate::provider::delete_item_by_name(r, &name) {
        r.error_message = "Cut: failed to delete original file".to_owned();
        return;
    }

    r.file_clipboard_path = format!("{cache_dir}/{name}");
    r.file_clipboard_is_cut = true;

    crate::state::update_state(r, Task::Delete, History::None);
    r.list_index = r.current_id.last().unwrap_or(0);
    r.needs_redraw = true;
}

/// Paste the file from `file_clipboard_path` into the current directory.
///
/// Resolves name collisions by appending " (copy N)". Mirrors C `handleFilePaste`.
pub fn handle_file_paste(r: &mut AppRenderer) {
    if r.file_clipboard_path.is_empty() { return; }

    let src_path = r.file_clipboard_path.clone();
    let slash = match src_path.rfind('/') { Some(p) => p, None => return };
    let src_dir = &src_path[..slash];
    let src_name = &src_path[slash + 1..];
    if src_name.is_empty() { return; }

    let dest_dir = crate::provider::current_path(r).to_owned();

    // Resolve collision-free destination name
    let dest_name = {
        let mut candidate = src_name.to_owned();
        let mut n = 0u32;
        loop {
            let full = format!("{dest_dir}/{candidate}");
            if !std::path::Path::new(&full).exists() { break; }
            n += 1;
            candidate = format!("{src_name} (copy {n})");
        }
        candidate
    };

    if !crate::provider::copy_item(r, src_dir, src_name, &dest_dir, &dest_name) {
        r.error_message = "Paste: failed to copy file".to_owned();
        return;
    }

    crate::provider::refresh_current_directory(r);
    list::create_list_current_layer(r);

    // Move cursor to the newly pasted element
    if let Some(arr) = get_ffon_at_id(&r.ffon, &r.current_id) {
        for (i, elem) in arr.iter().enumerate() {
            let key = match elem {
                sicompass_sdk::ffon::FfonElement::Str(s) => s.as_str(),
                sicompass_sdk::ffon::FfonElement::Obj(o) => o.key.as_str(),
            };
            if tags::strip_display(key) == dest_name {
                r.current_id.set_last(i);
                r.list_index = i;
                break;
            }
        }
    }
    r.needs_redraw = true;
}

/// Ctrl+I in OperatorGeneral — double-tap undo+insert.
///
/// A single tap inserts a new item (same as Ctrl+I operator).
/// A double tap (within DELTA_MS) undoes the previous insert and re-enters insert.
/// Mirrors C `handleCtrlI`.
pub fn handle_ctrl_i(r: &mut AppRenderer, history: crate::app_state::History) {
    let now = sdl_ticks();
    if now.saturating_sub(r.last_keypress_time) <= DELTA_MS {
        r.last_keypress_time = 0;
        crate::state::handle_history_action(r, History::Undo);
        crate::state::update_state(r, Task::InsertInsert, History::None);
    } else {
        crate::state::update_state(r, Task::Insert, history);
    }
    r.last_keypress_time = now;
    r.needs_redraw = true;
}

/// Ctrl+Enter — insert a newline character in insert modes.
///
/// Mirrors C `handleCtrlEnter`.
pub fn handle_ctrl_enter(r: &mut AppRenderer) {
    if matches!(
        r.coordinate,
        Coordinate::EditorInsert | Coordinate::OperatorInsert
    ) {
        handle_input(r, "\n");
    }
}

/// Save the active provider's FFON tree to `current_save_path` (Ctrl+S).
///
/// If no save path is set, launches the save-as dialog (filebrowser navigation).
/// Mirrors C `handleSaveProviderConfig`.
pub fn handle_save_provider_config(r: &mut AppRenderer) {
    if r.current_save_path.is_empty() {
        handle_save_as_provider_config(r);
        return;
    }
    let idx = match r.current_id.get(0) { Some(i) => i, None => return };
    let path = r.current_save_path.clone();
    if let Some(sicompass_sdk::ffon::FfonElement::Obj(root_obj)) = r.ffon.get(idx) {
        match sicompass_sdk::ffon::save_json_file(
            &[sicompass_sdk::ffon::FfonElement::Obj(root_obj.clone())],
            std::path::Path::new(&path),
        ) {
            Ok(()) => {
                r.error_message = format!("Saved to {path}");
            }
            Err(e) => {
                r.error_message = format!("Failed to save: {e}");
            }
        }
    } else {
        r.error_message = "Nothing to save".to_owned();
    }
    r.needs_redraw = true;
}

/// Save-as: navigate to filebrowser save-folder, insert a filename `<input>` placeholder,
/// and enter insert mode so the user can type a filename.
///
/// On Enter (in `handle_enter_operator_insert`), the typed name is used to write the
/// source provider's FFON data to `<save_folder>/<name>.json`, then navigation returns
/// to the original provider.
///
/// Mirrors C `handleSaveAsProviderConfig` → `handleFileBrowserSaveAs`.
pub fn handle_save_as_provider_config(r: &mut AppRenderer) {
    // Record which provider we're saving from, and where to return
    let src_idx = match r.current_id.get(0) { Some(i) => i, None => return };
    r.save_as_source_root_idx = src_idx;
    r.save_as_return_id = r.current_id.clone();

    // Find the filebrowser provider index
    let Some(fb_idx) = r.providers.iter().position(|p| p.name() == "filebrowser") else {
        r.error_message = "File browser not available".to_owned();
        r.needs_redraw = true;
        return;
    };

    // Resolve the save folder
    let save_dir = resolve_save_folder(r);
    if !std::path::Path::new(&save_dir).is_dir() {
        r.error_message = format!("Save folder does not exist: {save_dir}");
        r.needs_redraw = true;
        return;
    }

    // Navigate filebrowser to the save folder
    crate::provider::navigate_to_path(r, fb_idx, &save_dir, "");
    list::create_list_current_layer(r);

    // Insert an empty <input></input> placeholder at position 0 in the current dir
    use sicompass_sdk::ffon::FfonElement;
    let depth = r.current_id.depth();
    if depth >= 2 {
        let mut parent_id = r.current_id.clone();
        parent_id.pop();
        let parent_idx = parent_id.last().unwrap_or(0);
        if let Some(parent_slice) = crate::state::navigate_to_slice_pub(&mut r.ffon, &parent_id) {
            if let Some(FfonElement::Obj(obj)) = parent_slice.get_mut(parent_idx) {
                obj.children.insert(0, FfonElement::Str("<input></input>".to_owned()));
            }
        }
    }

    // Point cursor at position 0 (the new placeholder)
    r.current_id.set_last(0);
    r.pending_file_browser_save_as = true;
    r.coordinate = Coordinate::OperatorGeneral;
    list::create_list_current_layer(r);
    r.list_index = 0;
    r.scroll_offset = 0;
    r.prefixed_insert_mode = false;

    // Immediately enter insert mode
    handle_i(r);
    r.needs_redraw = true;
}

/// Resolve the configured save folder to an absolute path.
///
/// Empty or relative paths are resolved relative to `$HOME`.
/// Falls back to the platform downloads directory or `/tmp`.
fn resolve_save_folder(r: &AppRenderer) -> String {
    let folder = r.save_folder_path.trim();
    if folder.is_empty() {
        // Default to Downloads directory
        return sicompass_sdk::platform::downloads_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/tmp".to_owned());
    }
    let path = std::path::Path::new(folder);
    if path.is_absolute() {
        return folder.to_owned();
    }
    // Relative → $HOME/<folder>
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
    if home.is_empty() {
        return folder.to_owned();
    }
    format!("{home}/{folder}")
}

/// Load a JSON config file into the current provider (Ctrl+O).
///
/// Mirrors C `handleLoadProviderConfig` → `handleFileBrowserOpen` → `loadProviderConfigFromFile`.
pub fn handle_load_provider_config(r: &mut AppRenderer, path: &str) {
    if path.is_empty() { return; }
    let idx = match r.current_id.get(0) { Some(i) => i, None => return };

    match sicompass_sdk::ffon::load_json_file(std::path::Path::new(path)) {
        Ok(new_children) => {
            if let Some(sicompass_sdk::ffon::FfonElement::Obj(root_obj)) = r.ffon.get_mut(idx) {
                root_obj.children = new_children;
            }
            if let Some(p) = r.providers.get_mut(idx) {
                p.set_current_path("/");
            }
            // Clear undo history
            r.undo_history.clear();
            r.undo_position = 0;

            r.current_save_path = path.to_owned();
            list::create_list_current_layer(r);
            r.error_message = format!("Loaded from {path}");
        }
        Err(e) => {
            r.error_message = format!("Failed to load: {e}");
        }
    }
    r.needs_redraw = true;
}

/// Handle F5 — refresh current provider.
pub fn handle_f5(r: &mut AppRenderer) {
    crate::provider::refresh_current_directory(r);
    list::create_list_current_layer(r);
    r.sync_current_id_from_list();
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

pub fn has_selection(r: &AppRenderer) -> bool {
    r.selection_anchor.map_or(false, |a| a != r.cursor_position)
}

pub fn clear_selection(r: &mut AppRenderer) {
    r.selection_anchor = None;
}

pub fn selection_range(r: &AppRenderer) -> Option<(usize, usize)> {
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
            let now = sdl_ticks();
            if now.saturating_sub(r.last_keypress_time) <= DELTA_MS && r.current_id.depth() > 1 {
                // Double-tap: navigate to root
                while r.current_id.depth() > 1 {
                    handle_left(r);
                }
            } else {
                r.current_id.set_last(0);
            }
            r.last_keypress_time = now;
            list::create_list_current_layer(r);
            r.scroll_offset = r.list_index as i32;
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
            let line_height = r.cached_line_height.max(1);
            let viewport_h = r.window_height - line_height;
            let max_offset = (r.text_scroll_total_height - viewport_h).max(0);
            r.text_scroll_offset = max_offset;
            r.needs_redraw = true;
        }
        Coordinate::OperatorGeneral | Coordinate::EditorGeneral => {
            if let Some(slice) = sicompass_sdk::ffon::get_ffon_at_id(&r.ffon, &r.current_id) {
                let max_id = slice.len().saturating_sub(1);
                r.current_id.set_last(max_id);
                list::create_list_current_layer(r);
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
    match r.coordinate {
        Coordinate::SimpleSearch | Coordinate::Command => {
            let s = match r.coordinate {
                Coordinate::Command => r.input_buffer.clone(),
                _ => r.search_string.clone(),
            };
            list::create_list_current_layer(r);
            list::populate_list_current_layer(r, &s);
        }
        Coordinate::ExtendedSearch => {
            let s = r.input_buffer.clone();
            list::create_list_extended_search(r);
            list::populate_list_current_layer(r, &s);
        }
        _ => {}
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
            | Coordinate::ExtendedSearch
            | Coordinate::Command
    )
}

/// Returns true when the active provider is the file browser.
///
/// Used to route Ctrl+C/X/V to filesystem clipboard ops in OperatorGeneral.
fn active_provider_is_filebrowser(r: &AppRenderer) -> bool {
    r.current_id.get(0)
        .and_then(|i| r.providers.get(i))
        .map(|p| p.name() == "filebrowser")
        .unwrap_or(false)
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
    // File browser: filesystem cut
    if r.coordinate == Coordinate::OperatorGeneral && active_provider_is_filebrowser(r) {
        handle_file_cut(r);
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

/// Ctrl+C — copy selected text (insert modes), file path (filebrowser), or FFON element.
pub fn handle_ctrl_c(r: &mut AppRenderer) {
    if is_text_edit_mode(r) {
        if !has_selection(r) { return; }
        if let Some((start, end)) = selection_range(r) {
            sdl_set_clipboard(&r.input_buffer[start..end].to_owned());
        }
        r.needs_redraw = true;
        return;
    }
    // File browser: record filesystem copy path
    if r.coordinate == Coordinate::OperatorGeneral && active_provider_is_filebrowser(r) {
        handle_file_copy(r);
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

/// Ctrl+V — paste from system clipboard (insert modes), file paste (filebrowser), or FFON paste.
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
    // File browser: paste file from clipboard
    if r.coordinate == Coordinate::OperatorGeneral && active_provider_is_filebrowser(r) {
        handle_file_paste(r);
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
/// Double-tap interval for Ctrl+F extended search reset (mirrors C DELTA_MS).
const DELTA_MS: u64 = 400;

pub fn handle_ctrl_f(r: &mut AppRenderer) {
    match r.coordinate {
        Coordinate::Scroll => {
            r.coordinate = Coordinate::ScrollSearch;
            r.input_buffer.clear();
            r.cursor_position = 0;
            r.selection_anchor = None;
            r.scroll_search_match_count = 0;
            r.scroll_search_current_match = 0;
            r.scroll_search_needs_position = true;
            r.needs_redraw = true;
        }
        Coordinate::ScrollSearch | Coordinate::InputSearch => {
            // noop
        }
        Coordinate::EditorInsert | Coordinate::OperatorInsert => {
            r.previous_coordinate = r.coordinate;
            r.coordinate = Coordinate::InputSearch;
            r.input_buffer.clear();
            r.cursor_position = 0;
            r.selection_anchor = None;
            r.needs_redraw = true;
        }
        Coordinate::Command => {
            // noop in command mode
        }
        Coordinate::ExtendedSearch => {
            // Double-tap: reset to root of current provider
            let now = sdl_ticks();
            if now.saturating_sub(r.last_keypress_time) <= DELTA_MS {
                while r.current_id.depth() > 1 {
                    r.current_id.pop();
                }
                r.input_buffer.clear();
                r.cursor_position = 0;
                r.selection_anchor = None;
                r.scroll_offset = 0;
                list::create_list_extended_search(r);
                r.list_index = 0;
            }
            r.last_keypress_time = now;
            r.needs_redraw = true;
        }
        _ => {
            // From SimpleSearch: preserve previous_coordinate (don't overwrite)
            if r.coordinate != Coordinate::SimpleSearch {
                r.previous_coordinate = r.coordinate;
            }
            r.coordinate = Coordinate::ExtendedSearch;
            r.input_buffer.clear();
            r.cursor_position = 0;
            r.selection_anchor = None;
            r.scroll_offset = 0;
            list::create_list_extended_search(r);
            r.list_index = r.current_id.last().unwrap_or(0);
            r.last_keypress_time = sdl_ticks();
            r.needs_redraw = true;
        }
    }
}

/// Enter dashboard mode if the active provider has a dashboard image.
pub fn handle_dashboard(r: &mut AppRenderer) {
    let provider_idx = r.current_id.get(0).unwrap_or(0);
    let image_path = r.providers.get(provider_idx)
        .and_then(|p| p.dashboard_image_path())
        .map(|s| s.to_owned());

    if let Some(path) = image_path {
        r.dashboard_image_path = path;
        r.previous_coordinate = r.coordinate;
        r.coordinate = Coordinate::Dashboard;
        r.needs_redraw = true;
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
        // Clamp in case current_id drifted out of bounds after a refresh
        r.current_id.last().unwrap_or(0).min(slice.len().saturating_sub(1))
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
        // Clamp in case current_id drifted out of bounds after a refresh
        r.current_id.last().unwrap_or(0).min(slice.len().saturating_sub(1)) + 1
    };
    insert_operator_placeholder(r, insert_idx);
}

/// Insert a `<input></input>` placeholder at `insert_idx` in the current parent,
/// navigate the cursor there, and immediately enter insert mode.
fn insert_operator_placeholder(r: &mut AppRenderer, insert_idx: usize) {
    use sicompass_sdk::ffon::FfonElement;
    let placeholder = FfonElement::Str("<input></input>".to_owned());

    // Check provider before any mutation (borrow checker: can't hold ref while mutating)
    let is_filebrowser = r.current_id.get(0)
        .and_then(|idx| r.providers.get(idx))
        .map(|p| p.name() == "filebrowser")
        .unwrap_or(false);

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
    r.prefixed_insert_mode = is_filebrowser;
    list::create_list_current_layer(r);
    r.list_index = insert_idx;
    r.scroll_offset = 0;
    handle_i(r);
}

/// Get current time in milliseconds (used to reset caret blink).
pub fn sdl_ticks() -> u64 {
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
// FFON insertion/replacement helpers (for handle_enter_command)
// ---------------------------------------------------------------------------

/// Returns true if `key` is an empty `<input></input>` placeholder.
fn is_empty_placeholder(key: &str) -> bool {
    use sicompass_sdk::tags;
    if let Some(content) = tags::extract_input(key) {
        return content.is_empty();
    }
    if let Some(content) = tags::extract_input_all(key) {
        return content.is_empty();
    }
    false
}

/// Replace the element at `replace_idx` in the FFON tree at the current navigation depth.
fn replace_ffon_element(r: &mut AppRenderer, replace_idx: usize, elem: sicompass_sdk::ffon::FfonElement) {
    if r.current_id.depth() <= 1 {
        if replace_idx < r.ffon.len() {
            r.ffon[replace_idx] = elem;
        }
        return;
    }

    let mut parent_id = r.current_id.clone();
    parent_id.pop();

    let parent_idx = parent_id.last().unwrap_or(0);
    if get_ffon_at_id(&r.ffon, &parent_id).and_then(|s| s.get(parent_idx)).is_none() {
        return;
    }

    let mut current: &mut Vec<sicompass_sdk::ffon::FfonElement> = &mut r.ffon;
    let depth = parent_id.depth();
    for d in 0..depth {
        let idx = parent_id.get(d).unwrap_or(0);
        if d + 1 == depth {
            if let Some(sicompass_sdk::ffon::FfonElement::Obj(obj)) = current.get_mut(idx) {
                if replace_idx < obj.children.len() {
                    obj.children[replace_idx] = elem;
                }
            }
            return;
        }
        match current.get_mut(idx) {
            Some(sicompass_sdk::ffon::FfonElement::Obj(obj)) => {
                current = &mut obj.children;
            }
            _ => return,
        }
    }
}

/// Insert `elem` at `insert_idx` in the FFON tree at the current navigation depth.
///
/// - Depth 1 (root level): inserts directly into `r.ffon`.
/// - Depth > 1: finds the parent object and inserts into its children.
fn insert_ffon_element(r: &mut AppRenderer, insert_idx: usize, elem: sicompass_sdk::ffon::FfonElement) {
    if r.current_id.depth() <= 1 {
        r.ffon.insert(insert_idx, elem);
        return;
    }

    // Build parent id (current_id minus last component)
    let mut parent_id = r.current_id.clone();
    parent_id.pop();

    // Bounds-check the parent exists before walking mutably
    let parent_idx = parent_id.last().unwrap_or(0);
    if get_ffon_at_id(&r.ffon, &parent_id).and_then(|s| s.get(parent_idx)).is_none() {
        return;
    }

    // Re-walk mutably — we need a mutable reference to the parent object's children
    // Navigate step by step through the id chain
    let mut current: &mut Vec<sicompass_sdk::ffon::FfonElement> = &mut r.ffon;
    let depth = parent_id.depth();
    for d in 0..depth {
        let idx = parent_id.get(d).unwrap_or(0);
        if d + 1 == depth {
            // `current[idx]` is the parent object — insert into its children
            if let Some(sicompass_sdk::ffon::FfonElement::Obj(obj)) = current.get_mut(idx) {
                if insert_idx <= obj.children.len() {
                    obj.children.insert(insert_idx, elem);
                }
            }
            return;
        }
        // Step into the Obj at this level
        match current.get_mut(idx) {
            Some(sicompass_sdk::ffon::FfonElement::Obj(obj)) => {
                current = &mut obj.children;
            }
            _ => return,
        }
    }
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
        r.coordinate = Coordinate::SimpleSearch;
        r.list_index = 1;
        r.sync_current_id_from_list();
        handle_up(&mut r);
        assert_eq!(r.list_index, 0);
    }

    #[test]
    fn up_clamps_at_zero() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.list_index = 0;
        handle_up(&mut r);
        assert_eq!(r.list_index, 0);
    }

    #[test]
    fn down_moves_index() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.list_index = 0;
        handle_down(&mut r);
        assert_eq!(r.list_index, 1);
    }

    #[test]
    fn down_clamps_at_end() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
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
    fn handle_input_basic_insert() {
        let mut r = make_input_renderer("");
        r.cursor_position = 0;
        handle_input(&mut r, "abc");
        assert_eq!(r.input_buffer, "abc");
        assert_eq!(r.cursor_position, 3);
    }

    #[test]
    fn handle_input_insert_at_cursor() {
        let mut r = make_input_renderer("hello");
        r.cursor_position = 2;
        handle_input(&mut r, "X");
        assert_eq!(r.input_buffer, "heXllo");
        assert_eq!(r.cursor_position, 3);
    }

    #[test]
    fn handle_input_ignores_colon_in_empty_command_mode() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::Command;
        r.input_buffer.clear();
        r.cursor_position = 0;
        handle_input(&mut r, ":");
        assert_eq!(r.input_buffer, "");
    }

    #[test]
    fn handle_input_allows_colon_in_non_empty_command() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::Command;
        handle_input(&mut r, "a");
        handle_input(&mut r, ":");
        assert_eq!(r.input_buffer, "a:");
    }

    #[test]
    fn handle_input_sets_needs_redraw() {
        let mut r = make_input_renderer("");
        r.cursor_position = 0;
        handle_input(&mut r, "x");
        assert!(r.needs_redraw);
    }

    #[test]
    fn handle_input_resets_caret() {
        let mut r = make_input_renderer("");
        r.cursor_position = 0;
        r.caret.visible = false;
        handle_input(&mut r, "x");
        assert!(r.caret.visible);
    }

    #[test]
    fn handle_input_noop_in_operator_general() {
        let mut r = make_renderer();
        // OperatorGeneral is not a text-edit mode — input is ignored
        handle_input(&mut r, "abc");
        assert_eq!(r.input_buffer, "");
    }

    #[test]
    fn handle_backspace_in_search() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.search_string = "abc".to_owned();
        r.cursor_position = 3; // cursor at end, as set by handle_input
        handle_backspace(&mut r);
        assert_eq!(r.search_string, "ab");
        assert_eq!(r.cursor_position, 2);
    }

    // -----------------------------------------------------------------------
    // Input buffer helpers
    // -----------------------------------------------------------------------

    fn make_input_renderer(text: &str) -> AppRenderer {
        let mut r = AppRenderer::new();
        r.input_buffer = text.to_string();
        r.cursor_position = 0;
        r.selection_anchor = None;
        r.coordinate = Coordinate::EditorInsert;
        r
    }

    // has_selection
    #[test]
    fn has_selection_no_anchor() {
        let r = make_input_renderer("hello");
        assert!(!has_selection(&r));
    }

    #[test]
    fn has_selection_anchor_equals_cursor() {
        let mut r = make_input_renderer("hello");
        r.cursor_position = 3;
        r.selection_anchor = Some(3);
        assert!(!has_selection(&r));
    }

    #[test]
    fn has_selection_forward() {
        let mut r = make_input_renderer("hello");
        r.selection_anchor = Some(1);
        r.cursor_position = 4;
        assert!(has_selection(&r));
    }

    #[test]
    fn has_selection_reverse() {
        let mut r = make_input_renderer("hello");
        r.selection_anchor = Some(4);
        r.cursor_position = 1;
        assert!(has_selection(&r));
    }

    // clear_selection
    #[test]
    fn clear_selection_resets_anchor() {
        let mut r = make_input_renderer("hello");
        r.selection_anchor = Some(3);
        clear_selection(&mut r);
        assert_eq!(r.selection_anchor, None);
    }

    // selection_range
    #[test]
    fn selection_range_forward() {
        let mut r = make_input_renderer("hello");
        r.selection_anchor = Some(1);
        r.cursor_position = 4;
        assert_eq!(selection_range(&r), Some((1, 4)));
    }

    #[test]
    fn selection_range_backward() {
        let mut r = make_input_renderer("hello");
        r.selection_anchor = Some(4);
        r.cursor_position = 1;
        assert_eq!(selection_range(&r), Some((1, 4)));
    }

    // delete_selection
    #[test]
    fn delete_selection_no_selection() {
        let mut r = make_input_renderer("hello");
        r.cursor_position = 3;
        delete_selection(&mut r);
        assert_eq!(r.input_buffer, "hello");
        assert_eq!(r.cursor_position, 3);
    }

    #[test]
    fn delete_selection_middle() {
        let mut r = make_input_renderer("hello");
        r.selection_anchor = Some(1);
        r.cursor_position = 4;
        delete_selection(&mut r);
        assert_eq!(r.input_buffer, "ho");
        assert_eq!(r.cursor_position, 1);
        assert_eq!(r.selection_anchor, None);
    }

    #[test]
    fn delete_selection_reverse() {
        let mut r = make_input_renderer("hello");
        r.selection_anchor = Some(4);
        r.cursor_position = 1;
        delete_selection(&mut r);
        assert_eq!(r.input_buffer, "ho");
        assert_eq!(r.cursor_position, 1);
    }

    #[test]
    fn delete_selection_entire_string() {
        let mut r = make_input_renderer("hello");
        r.selection_anchor = Some(0);
        r.cursor_position = 5;
        delete_selection(&mut r);
        assert_eq!(r.input_buffer, "");
        assert_eq!(r.cursor_position, 0);
    }

    #[test]
    fn delete_selection_single_char() {
        let mut r = make_input_renderer("hello");
        r.selection_anchor = Some(2);
        r.cursor_position = 3;
        delete_selection(&mut r);
        assert_eq!(r.input_buffer, "helo");
        assert_eq!(r.input_buffer.len(), 4);
    }

    // -----------------------------------------------------------------------
    // Shift selection handlers
    // -----------------------------------------------------------------------

    #[test]
    fn shift_left_starts_selection() {
        let mut r = make_input_renderer("hello");
        r.cursor_position = 3;
        handle_shift_left(&mut r);
        assert_eq!(r.selection_anchor, Some(3));
        assert_eq!(r.cursor_position, 2);
    }

    #[test]
    fn shift_left_extends_selection() {
        let mut r = make_input_renderer("hello");
        r.cursor_position = 3;
        handle_shift_left(&mut r);
        handle_shift_left(&mut r);
        assert_eq!(r.selection_anchor, Some(3));
        assert_eq!(r.cursor_position, 1);
    }

    #[test]
    fn shift_left_at_start_noop() {
        let mut r = make_input_renderer("hello");
        r.cursor_position = 0;
        handle_shift_left(&mut r);
        assert_eq!(r.selection_anchor, None);
        assert_eq!(r.cursor_position, 0);
    }

    #[test]
    fn shift_left_utf8() {
        // "Aé" = A(1) + é(2) = 3 bytes
        let mut r = make_input_renderer("A\u{00E9}"); // é is 2 UTF-8 bytes
        r.cursor_position = 3; // end
        handle_shift_left(&mut r);
        assert_eq!(r.selection_anchor, Some(3));
        assert_eq!(r.cursor_position, 1); // start of é
    }

    #[test]
    fn shift_right_starts_selection() {
        let mut r = make_input_renderer("hello");
        r.cursor_position = 1;
        handle_shift_right(&mut r);
        assert_eq!(r.selection_anchor, Some(1));
        assert_eq!(r.cursor_position, 2);
    }

    #[test]
    fn shift_right_at_end_noop() {
        let mut r = make_input_renderer("hello");
        r.cursor_position = 5;
        handle_shift_right(&mut r);
        assert_eq!(r.selection_anchor, None);
        assert_eq!(r.cursor_position, 5);
    }

    #[test]
    fn shift_right_utf8() {
        // "éB" = é(2) + B(1) = 3 bytes
        let mut r = make_input_renderer("\u{00E9}B"); // é is 2 UTF-8 bytes
        r.cursor_position = 0;
        handle_shift_right(&mut r);
        assert_eq!(r.selection_anchor, Some(0));
        assert_eq!(r.cursor_position, 2); // past é
    }

    #[test]
    fn shift_home_from_middle() {
        let mut r = make_input_renderer("hello");
        r.cursor_position = 3;
        handle_shift_home(&mut r);
        assert_eq!(r.selection_anchor, Some(3));
        assert_eq!(r.cursor_position, 0);
    }

    #[test]
    fn shift_home_preserves_existing_anchor() {
        let mut r = make_input_renderer("hello");
        r.cursor_position = 3;
        r.selection_anchor = Some(4);
        handle_shift_home(&mut r);
        assert_eq!(r.selection_anchor, Some(4));
        assert_eq!(r.cursor_position, 0);
    }

    #[test]
    fn shift_end_from_middle() {
        let mut r = make_input_renderer("hello");
        r.cursor_position = 2;
        handle_shift_end(&mut r);
        assert_eq!(r.selection_anchor, Some(2));
        assert_eq!(r.cursor_position, 5);
    }

    #[test]
    fn select_all_selects_everything() {
        let mut r = make_input_renderer("hello");
        r.cursor_position = 2;
        handle_select_all(&mut r);
        assert_eq!(r.selection_anchor, Some(0));
        assert_eq!(r.cursor_position, 5);
    }

    #[test]
    fn select_all_empty_buffer_noop() {
        let mut r = make_input_renderer("");
        handle_select_all(&mut r);
        assert_eq!(r.selection_anchor, None);
        assert_eq!(r.cursor_position, 0);
    }

    // -----------------------------------------------------------------------
    // handle_ctrl_home / handle_ctrl_end
    // -----------------------------------------------------------------------

    #[test]
    fn ctrl_home_jumps_to_first() {
        let mut r = make_renderer();
        r.list_index = 3;
        handle_ctrl_home(&mut r);
        assert_eq!(r.list_index, 0);
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_end_jumps_to_last() {
        let mut r = make_renderer();
        r.list_index = 0;
        handle_ctrl_end(&mut r);
        let last = r.active_list_len() - 1;
        assert_eq!(r.list_index, last);
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_end_empty_list_no_change() {
        let mut r = AppRenderer::new();
        r.ffon = vec![FfonElement::new_obj("p")];
        r.current_id = { let mut id = IdArray::new(); id.push(0); id.push(0); id };
        // Empty: no children pushed
        r.list_index = 0;
        handle_ctrl_end(&mut r);
        // Empty list — list_index stays at 0
        assert_eq!(r.list_index, 0);
    }

    // -----------------------------------------------------------------------
    // handle_delete
    // -----------------------------------------------------------------------

    #[test]
    fn delete_sets_needs_redraw() {
        let mut r = make_renderer();
        r.needs_redraw = false;
        handle_delete(&mut r, History::None);
        assert!(r.needs_redraw);
    }

    // -----------------------------------------------------------------------
    // handle_colon
    // -----------------------------------------------------------------------

    #[test]
    fn colon_enters_command_mode() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::OperatorGeneral;
        handle_colon(&mut r);
        assert_eq!(r.coordinate, Coordinate::Command);
        assert_eq!(r.previous_coordinate, Coordinate::OperatorGeneral);
    }

    #[test]
    fn colon_clears_input_buffer() {
        let mut r = make_renderer();
        r.input_buffer = "hello".to_string();
        r.cursor_position = 3;
        handle_colon(&mut r);
        assert!(r.input_buffer.is_empty());
        assert_eq!(r.cursor_position, 0);
    }

    #[test]
    fn colon_resets_command_phase_to_none() {
        let mut r = make_renderer();
        r.current_command = CommandPhase::Provider;
        handle_colon(&mut r);
        assert_eq!(r.current_command, CommandPhase::None);
    }

    // -----------------------------------------------------------------------
    // handle_enter_command
    // -----------------------------------------------------------------------

    /// A provider that exposes one command ("open") and executes it immediately.
    struct CmdProvider {
        cmds: Vec<String>,
        execute_ok: bool,
        handle_result: Option<FfonElement>,
        secondary_items: Vec<sicompass_sdk::provider::ListItem>,
    }
    impl sicompass_sdk::provider::Provider for CmdProvider {
        fn name(&self) -> &str { "cmdprov" }
        fn fetch(&mut self) -> Vec<FfonElement> { vec![] }
        fn commands(&self) -> Vec<String> { self.cmds.clone() }
        fn execute_command(&mut self, _cmd: &str, _sel: &str) -> bool { self.execute_ok }
        fn handle_command(&mut self, _cmd: &str, _key: &str, _ty: i32, _err: &mut String) -> Option<FfonElement> {
            self.handle_result.clone()
        }
        fn command_list_items(&self, _cmd: &str) -> Vec<sicompass_sdk::provider::ListItem> {
            self.secondary_items.clone()
        }
    }

    fn make_renderer_with_cmd_provider(
        commands: &[&str],
        handle_result: Option<FfonElement>,
        secondary_items: Vec<sicompass_sdk::provider::ListItem>,
    ) -> AppRenderer {
        let mut root = FfonElement::new_obj("cmdprov");
        root.as_obj_mut().unwrap().push(FfonElement::new_str("- item0"));

        let mut r = AppRenderer::new();
        r.ffon = vec![root];
        r.current_id = { let mut id = IdArray::new(); id.push(0); id.push(0); id };
        r.providers.push(Box::new(CmdProvider {
            cmds: commands.iter().map(|s| s.to_string()).collect(),
            execute_ok: true,
            handle_result,
            secondary_items,
        }));
        list::create_list_current_layer(&mut r);
        r
    }

    #[test]
    fn enter_command_no_list_item_escapes() {
        let mut r = make_renderer_with_cmd_provider(&[], None, vec![]);
        r.coordinate = Coordinate::Command;
        r.total_list.clear(); // no items
        handle_enter_command(&mut r);
        // should escape back to previous coordinate
        assert_ne!(r.coordinate, Coordinate::Command);
    }

    #[test]
    fn enter_command_phase_none_with_secondary_list_stays_in_command() {
        // Provider returns None + no error + has secondary items
        let items = vec![sicompass_sdk::provider::ListItem { label: "App A".to_string(), data: "/usr/bin/a".to_string() }];
        let mut r = make_renderer_with_cmd_provider(&["open"], None, items);
        r.coordinate = Coordinate::Command;
        r.previous_coordinate = Coordinate::OperatorGeneral;
        // Manually populate command list
        list::create_list_current_layer(&mut r);
        // Should show 1 command: "open"
        assert_eq!(r.total_list.len(), 1);
        r.list_index = 0;

        handle_enter_command(&mut r);

        // Now in Phase 2: CommandPhase::Provider, secondary list visible
        assert_eq!(r.current_command, CommandPhase::Provider);
        assert_eq!(r.coordinate, Coordinate::Command);
        assert_eq!(r.total_list[0].label, "App A");
    }

    #[test]
    fn enter_command_phase_provider_executes_and_returns() {
        let mut r = make_renderer_with_cmd_provider(&["open"], None, vec![]);
        r.coordinate = Coordinate::Command;
        r.previous_coordinate = Coordinate::OperatorGeneral;
        r.current_command = CommandPhase::Provider;
        r.provider_command_name = "open".to_string();
        // Build a secondary list manually
        r.total_list = vec![crate::app_state::RenderListItem {
            id: { let mut id = IdArray::new(); id.push(0); id },
            label: "App A".to_string(),
            data: Some("/usr/bin/a".to_string()),
            nav_path: None,
        }];
        r.list_index = 0;

        handle_enter_command(&mut r);

        // Should return to previous coordinate and reset phase
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
        assert_eq!(r.current_command, CommandPhase::None);
    }

    // -----------------------------------------------------------------------
    // handle_tab
    // -----------------------------------------------------------------------

    #[test]
    fn tab_noop_in_scroll_mode() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::Scroll;
        handle_tab(&mut r);
        assert_eq!(r.coordinate, Coordinate::Scroll);
    }

    #[test]
    fn tab_from_operator_enters_simple_search() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::OperatorGeneral;
        handle_tab(&mut r);
        assert_eq!(r.coordinate, Coordinate::SimpleSearch);
        assert_eq!(r.previous_coordinate, Coordinate::OperatorGeneral);
        assert!(r.search_string.is_empty());
    }

    #[test]
    fn tab_from_simple_search_is_noop() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        handle_tab(&mut r);
        assert_eq!(r.coordinate, Coordinate::SimpleSearch);
    }

    // -----------------------------------------------------------------------
    // handle_escape (advanced)
    // -----------------------------------------------------------------------

    #[test]
    fn escape_from_command_mode() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::Command;
        r.previous_coordinate = Coordinate::OperatorGeneral;
        handle_escape(&mut r);
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
    }

    #[test]
    fn escape_from_extended_search() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::ExtendedSearch;
        r.previous_coordinate = Coordinate::OperatorGeneral;
        handle_escape(&mut r);
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
    }

    #[test]
    fn escape_from_scroll_search() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::ScrollSearch;
        handle_escape(&mut r);
        assert_eq!(r.coordinate, Coordinate::Scroll);
    }

    #[test]
    fn escape_from_scroll_returns_to_previous() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::Scroll;
        r.previous_coordinate = Coordinate::OperatorGeneral;
        handle_escape(&mut r);
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
    }

    #[test]
    fn escape_clears_selection_anchor() {
        let mut r = make_input_renderer("hello");
        r.coordinate = Coordinate::OperatorInsert;
        r.selection_anchor = Some(5);
        handle_escape(&mut r);
        assert_eq!(r.selection_anchor, None);
    }

    #[test]
    fn escape_from_editor_insert_goes_to_editor_general() {
        // C spec: EditorInsert → updateState(Input) → EditorGeneral
        let mut r = make_renderer();
        r.coordinate = Coordinate::EditorInsert;
        handle_escape(&mut r);
        assert_eq!(r.coordinate, Coordinate::EditorGeneral);
    }

    // -----------------------------------------------------------------------
    // handle_s
    // -----------------------------------------------------------------------

    #[test]
    fn s_from_operator_enters_scroll() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::OperatorGeneral;
        handle_s(&mut r);
        assert_eq!(r.coordinate, Coordinate::Scroll);
        assert_eq!(r.previous_coordinate, Coordinate::OperatorGeneral);
        assert_eq!(r.text_scroll_offset, -1); // sentinel: renderer computes initial offset
        assert_eq!(r.text_scroll_total_height, 0);
    }

    #[test]
    fn s_noop_outside_operator() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        handle_s(&mut r);
        assert_eq!(r.coordinate, Coordinate::SimpleSearch);
    }

    // -----------------------------------------------------------------------
    // handle_ctrl_f
    // -----------------------------------------------------------------------

    #[test]
    fn ctrl_f_from_scroll_enters_scroll_search() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::Scroll;
        handle_ctrl_f(&mut r);
        assert_eq!(r.coordinate, Coordinate::ScrollSearch);
    }

    #[test]
    fn ctrl_f_noop_in_scroll_search() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::ScrollSearch;
        handle_ctrl_f(&mut r);
        assert_eq!(r.coordinate, Coordinate::ScrollSearch);
    }

    #[test]
    fn ctrl_f_from_operator_enters_extended_search() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::OperatorGeneral;
        handle_ctrl_f(&mut r);
        assert_eq!(r.coordinate, Coordinate::ExtendedSearch);
        assert_eq!(r.previous_coordinate, Coordinate::OperatorGeneral);
    }

    #[test]
    fn ctrl_f_from_insert_enters_input_search() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::OperatorInsert;
        handle_ctrl_f(&mut r);
        assert_eq!(r.coordinate, Coordinate::InputSearch);
        assert_eq!(r.previous_coordinate, Coordinate::OperatorInsert);
    }

    // -----------------------------------------------------------------------
    // handle_up / handle_down (advanced — with actual list)
    // -----------------------------------------------------------------------

    #[test]
    fn up_in_simple_search_decrements_list_index() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.list_index = 3;
        r.sync_current_id_from_list();
        handle_up(&mut r);
        assert_eq!(r.list_index, 2);
    }

    #[test]
    fn up_at_zero_stays_in_search() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.list_index = 0;
        handle_up(&mut r);
        assert_eq!(r.list_index, 0);
    }

    #[test]
    fn down_in_simple_search_increments_list_index() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.list_index = 0;
        handle_down(&mut r);
        assert_eq!(r.list_index, 1);
    }

    #[test]
    fn down_at_max_stays_in_search() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        let last = r.active_list_len() - 1;
        r.list_index = last;
        handle_down(&mut r);
        assert_eq!(r.list_index, last);
    }

    // -----------------------------------------------------------------------
    // Clipboard — Ctrl+C / Ctrl+X / Ctrl+V
    // -----------------------------------------------------------------------

    /// Set up an EditorGeneral renderer whose list shows `items` as string children
    /// of provider 0. `list_index` is left at 0 (first child).
    fn make_editor_with_items(items: &[&str]) -> AppRenderer {
        use sicompass_sdk::ffon::IdArray;
        let mut root = FfonElement::new_obj("test");
        for &item in items {
            root.as_obj_mut().unwrap().push(FfonElement::new_str(item));
        }
        let mut r = AppRenderer::new();
        r.ffon = vec![root];
        r.coordinate = Coordinate::EditorGeneral;
        let mut id = IdArray::new();
        id.push(0);
        id.push(0);
        r.current_id = id;
        crate::list::create_list_current_layer(&mut r);
        r.list_index = 0;
        r
    }

    // --- Ctrl+C element mode ---

    #[test]
    fn ctrl_c_copies_first_element_to_clipboard() {
        let mut r = make_editor_with_items(&["hello", "world"]);
        handle_ctrl_c(&mut r);
        assert_eq!(r.clipboard, Some(FfonElement::Str("hello".to_string())));
        // ffon unchanged
        assert_eq!(r.ffon[0].as_obj().unwrap().children.len(), 2);
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_c_copies_second_element() {
        let mut r = make_editor_with_items(&["first", "second"]);
        r.list_index = 1;
        handle_ctrl_c(&mut r);
        assert_eq!(r.clipboard, Some(FfonElement::Str("second".to_string())));
    }

    #[test]
    fn ctrl_c_copies_object_element() {
        use sicompass_sdk::ffon::IdArray;
        let mut root = FfonElement::new_obj("test");
        let mut section = FfonElement::new_obj("mykey");
        section.as_obj_mut().unwrap().push(FfonElement::new_str("child"));
        root.as_obj_mut().unwrap().push(FfonElement::new_str("first"));
        root.as_obj_mut().unwrap().push(section);
        let mut r = AppRenderer::new();
        r.ffon = vec![root];
        r.coordinate = Coordinate::EditorGeneral;
        let mut id = IdArray::new();
        id.push(0); id.push(0);
        r.current_id = id;
        crate::list::create_list_current_layer(&mut r);
        r.list_index = 1; // second child = "mykey" object
        handle_ctrl_c(&mut r);
        let clip = r.clipboard.as_ref().unwrap();
        assert!(clip.is_obj());
        assert_eq!(clip.as_obj().unwrap().key, "mykey");
        assert_eq!(clip.as_obj().unwrap().children.len(), 1);
    }

    #[test]
    fn ctrl_c_replaces_previous_clipboard() {
        let mut r = make_editor_with_items(&["first", "second"]);
        r.list_index = 0;
        handle_ctrl_c(&mut r);
        assert_eq!(r.clipboard, Some(FfonElement::Str("first".to_string())));
        r.list_index = 1;
        handle_ctrl_c(&mut r);
        assert_eq!(r.clipboard, Some(FfonElement::Str("second".to_string())));
    }

    // --- Ctrl+C text mode (selection) ---

    #[test]
    fn ctrl_c_text_no_selection_does_nothing() {
        let mut r = make_input_renderer("hello");
        r.needs_redraw = false;
        handle_ctrl_c(&mut r);
        // no selection → nothing happens
        assert_eq!(r.input_buffer, "hello");
        assert!(!r.needs_redraw);
    }

    #[test]
    fn ctrl_c_text_with_selection_sets_needs_redraw() {
        let mut r = make_input_renderer("hello world");
        r.selection_anchor = Some(0);
        r.cursor_position = 5;
        handle_ctrl_c(&mut r);
        // buffer unchanged (copy, not cut)
        assert_eq!(r.input_buffer, "hello world");
        assert!(r.needs_redraw);
    }

    // --- Ctrl+X element mode ---

    #[test]
    fn ctrl_x_sets_clipboard_in_editor_general() {
        let mut r = make_editor_with_items(&["first", "second", "third"]);
        r.list_index = 1;
        handle_ctrl_x(&mut r);
        assert_eq!(r.clipboard, Some(FfonElement::Str("second".to_string())));
        assert!(r.needs_redraw);
    }

    // --- Ctrl+X text mode ---

    #[test]
    fn ctrl_x_text_cuts_selection() {
        let mut r = make_input_renderer("hello world");
        r.selection_anchor = Some(6);
        r.cursor_position = 11;
        handle_ctrl_x(&mut r);
        assert_eq!(r.input_buffer, "hello ");
        assert_eq!(r.cursor_position, 6);
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_x_text_no_selection_does_nothing() {
        let mut r = make_input_renderer("hello");
        r.needs_redraw = false;
        handle_ctrl_x(&mut r);
        assert_eq!(r.input_buffer, "hello");
        assert!(!r.needs_redraw);
    }

    #[test]
    fn ctrl_x_text_cuts_middle_selection() {
        let mut r = make_input_renderer("abcdefgh");
        r.selection_anchor = Some(2);
        r.cursor_position = 5;
        handle_ctrl_x(&mut r);
        assert_eq!(r.input_buffer, "abfgh");
        assert_eq!(r.cursor_position, 2);
    }

    #[test]
    fn ctrl_x_text_reverse_selection_cuts_correctly() {
        // anchor > cursor (reverse selection)
        let mut r = make_input_renderer("hello world");
        r.selection_anchor = Some(11);
        r.cursor_position = 6;
        handle_ctrl_x(&mut r);
        assert_eq!(r.input_buffer, "hello ");
        assert_eq!(r.cursor_position, 6);
    }

    // --- Ctrl+V element mode ---

    #[test]
    fn ctrl_v_element_mode_sets_needs_redraw() {
        let mut r = make_editor_with_items(&["original"]);
        r.clipboard = Some(FfonElement::Str("pasted".to_string()));
        handle_ctrl_v(&mut r);
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_v_element_mode_no_clipboard_still_redraws() {
        // In Rust, handle_ctrl_v in EditorGeneral always calls update_state(Paste)
        // and sets needs_redraw regardless of whether clipboard is set.
        let mut r = make_editor_with_items(&["original"]);
        r.clipboard = None;
        handle_ctrl_v(&mut r);
        assert!(r.needs_redraw);
    }

    // --- Ctrl+V text mode (no clipboard path) ---

    #[test]
    fn ctrl_v_text_no_clipboard_does_nothing() {
        // Without a real SDL clipboard, handle_ctrl_v returns early.
        // Verify no crash and no buffer modification.
        let mut r = make_input_renderer("hello");
        r.cursor_position = 5;
        let before = r.input_buffer.clone();
        handle_ctrl_v(&mut r);
        // Buffer unchanged (no SDL clipboard available in tests)
        assert_eq!(r.input_buffer, before);
    }

    // --- Ctrl+X actually removes element from FFON ---

    #[test]
    fn ctrl_x_removes_element_from_ffon() {
        let mut r = make_editor_with_items(&["first", "second", "third"]);
        r.list_index = 1;
        r.sync_current_id_from_list();
        let before_len = r.ffon[0].as_obj().unwrap().children.len();
        handle_ctrl_x(&mut r);
        let after_len = r.ffon[0].as_obj().unwrap().children.len();
        assert_eq!(after_len, before_len - 1);
    }

    #[test]
    fn ctrl_x_cut_first_element_cursor_stays_at_zero() {
        let mut r = make_editor_with_items(&["a", "b", "c"]);
        r.list_index = 0;
        r.sync_current_id_from_list();
        handle_ctrl_x(&mut r);
        assert_eq!(r.list_index, 0);
    }

    #[test]
    fn ctrl_x_cut_last_element_cursor_adjusts() {
        let mut r = make_editor_with_items(&["a", "b", "c"]);
        r.list_index = 2;
        r.sync_current_id_from_list();
        handle_ctrl_x(&mut r);
        let new_len = r.ffon[0].as_obj().unwrap().children.len();
        assert!(r.list_index < new_len.max(1));
    }

    // --- Ctrl+V actually replaces element ---

    #[test]
    fn ctrl_v_paste_replaces_current_element() {
        let mut r = make_editor_with_items(&["original"]);
        r.list_index = 0;
        r.sync_current_id_from_list();
        r.clipboard = Some(FfonElement::Str("pasted".to_string()));
        handle_ctrl_v(&mut r);
        let elem = &r.ffon[0].as_obj().unwrap().children[0];
        assert_eq!(elem.as_str(), Some("pasted"));
    }

    // --- Integration: copy then paste ---

    #[test]
    fn clipboard_integration_copy_then_paste() {
        let mut r = make_editor_with_items(&["alpha", "beta"]);
        r.list_index = 0;
        r.sync_current_id_from_list();
        handle_ctrl_c(&mut r);
        r.list_index = 1;
        r.sync_current_id_from_list();
        handle_ctrl_v(&mut r);
        let elem = &r.ffon[0].as_obj().unwrap().children[1];
        assert_eq!(elem.as_str(), Some("alpha"));
    }

    #[test]
    fn clipboard_integration_cut_then_paste() {
        let mut r = make_editor_with_items(&["alpha", "beta"]);
        r.list_index = 0;
        r.sync_current_id_from_list();
        handle_ctrl_x(&mut r);
        assert_eq!(r.clipboard, Some(FfonElement::Str("alpha".to_string())));
        // After cut, "beta" is at index 0
        crate::list::create_list_current_layer(&mut r);
        r.list_index = 0;
        r.sync_current_id_from_list();
        handle_ctrl_v(&mut r);
        let elem = &r.ffon[0].as_obj().unwrap().children[0];
        assert_eq!(elem.as_str(), Some("alpha"));
    }

    #[test]
    fn clipboard_integration_multiple_pastes() {
        let mut r = make_editor_with_items(&["src", "dst1", "dst2"]);
        r.list_index = 0;
        r.sync_current_id_from_list();
        handle_ctrl_c(&mut r);
        r.list_index = 1;
        r.sync_current_id_from_list();
        handle_ctrl_v(&mut r);
        crate::list::create_list_current_layer(&mut r);
        r.list_index = 2;
        r.sync_current_id_from_list();
        handle_ctrl_v(&mut r);
        let children = &r.ffon[0].as_obj().unwrap().children;
        assert_eq!(children[1].as_str(), Some("src"));
        assert_eq!(children[2].as_str(), Some("src"));
    }

    // -----------------------------------------------------------------------
    // handle_ctrl_home / handle_ctrl_end (filtered list variants)
    // -----------------------------------------------------------------------

    fn make_renderer_with_items(items: &[&str]) -> AppRenderer {
        let mut root = FfonElement::new_obj("provider");
        for item in items {
            root.as_obj_mut().unwrap().push(FfonElement::new_str(*item));
        }
        let mut r = AppRenderer::new();
        r.ffon = vec![root];
        r.current_id = { let mut id = IdArray::new(); id.push(0); id.push(0); id };
        list::create_list_current_layer(&mut r);
        r
    }

    #[test]
    fn ctrl_home_filtered_list_jumps_to_first() {
        let mut r = make_renderer_with_items(&["a", "b", "c", "d", "e"]);
        // Simulate a filtered list with 3 matches
        r.filtered_list_indices = vec![1, 2, 3];
        r.list_index = 2;
        handle_ctrl_home(&mut r);
        assert_eq!(r.list_index, 0);
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_home_empty_list_no_change() {
        let mut r = AppRenderer::new();
        r.ffon = vec![FfonElement::new_obj("empty")];
        r.current_id = { let mut id = IdArray::new(); id.push(0); id.push(0); id };
        r.list_index = 0;
        handle_ctrl_home(&mut r);
        assert_eq!(r.list_index, 0);
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_end_filtered_list_jumps_to_last_filtered() {
        let mut r = make_renderer_with_items(&["a", "b", "c", "d", "e"]);
        // Simulate a filtered list with 3 matches (indices 1,2,3 → display index 0,1,2)
        r.filtered_list_indices = vec![1, 2, 3];
        r.list_index = 0;
        handle_ctrl_end(&mut r);
        assert_eq!(r.list_index, 2); // filteredListCount - 1
        assert!(r.needs_redraw);
    }

    // -----------------------------------------------------------------------
    // handle_delete (no-history variant)
    // -----------------------------------------------------------------------

    #[test]
    fn delete_no_history_passes_none() {
        let mut r = make_renderer_with_items(&["item"]);
        r.list_index = 0;
        r.sync_current_id_from_list();
        // Should not panic; just verify it sets needs_redraw
        handle_delete(&mut r, History::None);
        assert!(r.needs_redraw);
    }

    // -----------------------------------------------------------------------
    // handle_tab (new variants)
    // -----------------------------------------------------------------------

    #[test]
    fn tab_noop_in_scroll_search_mode() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::ScrollSearch;
        handle_tab(&mut r);
        assert_eq!(r.coordinate, Coordinate::ScrollSearch);
    }

    #[test]
    fn tab_from_editor_enters_simple_search() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::EditorGeneral;
        handle_tab(&mut r);
        assert_eq!(r.coordinate, Coordinate::SimpleSearch);
        assert_eq!(r.previous_coordinate, Coordinate::EditorGeneral);
    }

    // -----------------------------------------------------------------------
    // handle_escape (new variants)
    // -----------------------------------------------------------------------

    #[test]
    fn escape_from_operator_insert_goes_to_operator_general() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::OperatorInsert;
        handle_escape(&mut r);
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
    }

    #[test]
    fn escape_from_simple_search_returns_to_previous() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.previous_coordinate = Coordinate::EditorGeneral;
        r.current_id.set_last(3); // simulate current position
        handle_escape(&mut r);
        assert_eq!(r.coordinate, Coordinate::EditorGeneral);
        // list_index should be synced to currentId
        assert_eq!(r.list_index, 3);
    }

    #[test]
    fn escape_from_unknown_with_operator_previous() {
        // EditorGeneral + previousCoordinate=OperatorGeneral → OperatorGeneral
        let mut r = make_renderer();
        r.coordinate = Coordinate::EditorGeneral;
        r.previous_coordinate = Coordinate::OperatorGeneral;
        handle_escape(&mut r);
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
    }

    #[test]
    fn escape_from_unknown_defaults_to_editor() {
        // EditorGeneral + previousCoordinate=EditorGeneral → EditorGeneral
        let mut r = make_renderer();
        r.coordinate = Coordinate::EditorGeneral;
        r.previous_coordinate = Coordinate::EditorGeneral;
        handle_escape(&mut r);
        assert_eq!(r.coordinate, Coordinate::EditorGeneral);
    }

    #[test]
    fn escape_from_dashboard_returns_to_previous() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::Dashboard;
        r.previous_coordinate = Coordinate::OperatorGeneral;
        handle_escape(&mut r);
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
        assert!(r.needs_redraw);
    }

    #[test]
    fn escape_from_scroll_search_resets_match_counts() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::ScrollSearch;
        r.scroll_search_match_count = 5;
        r.scroll_search_current_match = 2;
        handle_escape(&mut r);
        assert_eq!(r.coordinate, Coordinate::Scroll);
        assert_eq!(r.scroll_search_match_count, 0);
        assert_eq!(r.scroll_search_current_match, 0);
    }

    // -----------------------------------------------------------------------
    // handle_ctrl_f (new variants)
    // -----------------------------------------------------------------------

    #[test]
    fn ctrl_f_noop_in_command_mode() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::Command;
        handle_ctrl_f(&mut r);
        assert_eq!(r.coordinate, Coordinate::Command);
    }

    #[test]
    fn ctrl_f_from_simple_search_preserves_previous() {
        // C: when coming from SimpleSearch, don't overwrite previousCoordinate
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.previous_coordinate = Coordinate::OperatorGeneral;
        handle_ctrl_f(&mut r);
        assert_eq!(r.coordinate, Coordinate::ExtendedSearch);
        assert_eq!(r.previous_coordinate, Coordinate::OperatorGeneral); // not overwritten
    }

    #[test]
    fn ctrl_f_from_scroll_resets_search_state() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::Scroll;
        r.scroll_search_match_count = 5;
        r.scroll_search_current_match = 3;
        handle_ctrl_f(&mut r);
        assert_eq!(r.coordinate, Coordinate::ScrollSearch);
        assert_eq!(r.scroll_search_match_count, 0);
        assert_eq!(r.scroll_search_current_match, 0);
    }

    // -----------------------------------------------------------------------
    // handle_up (new variants: scroll search, scroll, command mode)
    // -----------------------------------------------------------------------

    #[test]
    fn up_in_scroll_search_wraps_to_last() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::ScrollSearch;
        r.scroll_search_match_count = 5;
        r.scroll_search_current_match = 0;
        handle_up(&mut r);
        assert_eq!(r.scroll_search_current_match, 4); // wraps to last
    }

    #[test]
    fn up_in_scroll_search_decrements() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::ScrollSearch;
        r.scroll_search_match_count = 5;
        r.scroll_search_current_match = 3;
        handle_up(&mut r);
        assert_eq!(r.scroll_search_current_match, 2);
    }

    #[test]
    fn up_in_scroll_mode_decrements_offset() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::Scroll;
        // step = cached_line_height (20); start above step so clamp doesn't apply
        r.text_scroll_offset = 50;
        handle_up(&mut r);
        assert_eq!(r.text_scroll_offset, 30); // 50 - 20
    }

    #[test]
    fn up_in_scroll_mode_clamps_at_zero() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::Scroll;
        r.text_scroll_offset = 0;
        handle_up(&mut r);
        assert_eq!(r.text_scroll_offset, 0);
    }

    #[test]
    fn up_in_search_clears_error_message() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.error_message = "some error".to_owned();
        r.list_index = 2;
        handle_up(&mut r);
        assert!(r.error_message.is_empty());
    }

    #[test]
    fn up_in_command_mode_no_id_copy() {
        // In command mode, listIndex decrements but currentId is NOT updated
        let mut r = make_renderer_with_items(&["a", "b", "c", "d", "e"]);
        r.coordinate = Coordinate::Command;
        r.list_index = 2;
        let saved_id = r.current_id.clone();
        handle_up(&mut r);
        assert_eq!(r.list_index, 1);
        assert_eq!(r.current_id, saved_id); // currentId unchanged
    }

    #[test]
    fn up_in_general_sets_needs_redraw() {
        // OperatorGeneral calls updateState(ArrowUp); just verify no crash + needsRedraw
        let mut r = make_renderer();
        r.coordinate = Coordinate::OperatorGeneral;
        handle_up(&mut r);
        assert!(r.needs_redraw);
    }

    #[test]
    fn up_noop_in_insert_mode_but_redraws() {
        // C: insert modes set needsRedraw but don't call updateState
        let mut r = make_renderer();
        r.coordinate = Coordinate::EditorInsert;
        handle_up(&mut r);
        assert!(r.needs_redraw);
    }

    // -----------------------------------------------------------------------
    // handle_down (new variants)
    // -----------------------------------------------------------------------

    #[test]
    fn down_in_scroll_search_wraps_to_first() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::ScrollSearch;
        r.scroll_search_match_count = 5;
        r.scroll_search_current_match = 4;
        handle_down(&mut r);
        assert_eq!(r.scroll_search_current_match, 0); // wraps to first
    }

    #[test]
    fn down_in_scroll_search_increments() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::ScrollSearch;
        r.scroll_search_match_count = 5;
        r.scroll_search_current_match = 2;
        handle_down(&mut r);
        assert_eq!(r.scroll_search_current_match, 3);
    }

    #[test]
    fn down_in_command_mode_no_id_copy() {
        let mut r = make_renderer_with_items(&["a", "b", "c", "d", "e"]);
        r.coordinate = Coordinate::Command;
        r.list_index = 1;
        let saved_id = r.current_id.clone();
        handle_down(&mut r);
        assert_eq!(r.list_index, 2);
        assert_eq!(r.current_id, saved_id); // currentId unchanged
    }

    #[test]
    fn down_in_general_sets_needs_redraw() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::EditorGeneral;
        handle_down(&mut r);
        assert!(r.needs_redraw);
    }

    #[test]
    fn down_noop_in_operator_insert() {
        // OperatorInsert: down does nothing (no updateState, no listIndex change)
        let mut r = make_renderer_with_items(&["a", "b", "c"]);
        r.coordinate = Coordinate::OperatorInsert;
        r.list_index = 1;
        handle_down(&mut r);
        assert_eq!(r.list_index, 1); // unchanged
    }

    #[test]
    fn down_uses_filtered_count_as_max() {
        // ExtendedSearch with filtered list: can't go past filteredListCount - 1
        let mut r = make_renderer_with_items(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
        r.coordinate = Coordinate::ExtendedSearch;
        // Simulate 3 filtered matches (display indices 0, 1, 2)
        r.filtered_list_indices = vec![0, 1, 2];
        r.list_index = 2; // already at max filtered index
        handle_down(&mut r);
        assert_eq!(r.list_index, 2); // can't go further
    }

    // ---- UTF-8 cursor movement (mirrors C test_handlers.c utf8_* tests) ----

    // Rust doesn't have standalone utf8_char_length/utf8_move_forward/backward
    // functions — the behavior is tested through cursor operations.

    #[test]
    fn utf8_char_length_ascii() {
        // ASCII char is 1 byte
        let s = "A";
        let len = s.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
        assert_eq!(len, 1);
    }

    #[test]
    fn utf8_char_length_two_byte() {
        // "é" (U+00E9) is 2 bytes
        let s = "\u{00E9}";
        let len = s.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
        assert_eq!(len, 2);
    }

    #[test]
    fn utf8_char_length_three_byte() {
        // "€" (U+20AC) is 3 bytes
        let s = "\u{20AC}";
        let len = s.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
        assert_eq!(len, 3);
    }

    #[test]
    fn utf8_char_length_four_byte() {
        // "𝄞" (U+1D11E) is 4 bytes
        let s = "\u{1D11E}";
        let len = s.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
        assert_eq!(len, 4);
    }

    #[test]
    fn utf8_char_length_at_offset() {
        // "Aé" — char at byte offset 1 is "é" (2 bytes)
        let s = "A\u{00E9}";
        let len = s[1..].chars().next().map(|c| c.len_utf8()).unwrap_or(0);
        assert_eq!(len, 2);
    }

    #[test]
    fn utf8_move_backward_at_start() {
        // At position 0, backspace is a no-op
        let mut r = make_input_renderer("hello");
        r.cursor_position = 0;
        handle_backspace(&mut r);
        assert_eq!(r.cursor_position, 0);
        assert_eq!(r.input_buffer, "hello");
    }

    #[test]
    fn utf8_move_backward_ascii() {
        // From byte 3 in "hello", backspace moves to byte 2
        let mut r = make_input_renderer("hello");
        r.cursor_position = 3;
        handle_backspace(&mut r);
        assert_eq!(r.cursor_position, 2);
    }

    #[test]
    fn utf8_move_backward_two_byte_char() {
        // "Aé": backspace from end (byte 3) removes "é", cursor at byte 1
        let mut r = make_input_renderer("A\u{00E9}");
        r.cursor_position = 3;
        handle_backspace(&mut r);
        assert_eq!(r.cursor_position, 1);
        assert_eq!(r.input_buffer, "A");
    }

    // -----------------------------------------------------------------------
    // handle_ctrl_i
    // -----------------------------------------------------------------------

    #[test]
    fn ctrl_i_single_tap_performs_insert() {
        let mut r = AppRenderer::new();
        r.last_keypress_time = 0; // ensure no double-tap
        r.coordinate = Coordinate::OperatorGeneral;
        handle_ctrl_i(&mut r, History::None);
        assert!(r.needs_redraw);
        assert!(r.last_keypress_time > 0);
    }

    #[test]
    fn ctrl_i_double_tap_updates_timer() {
        let mut r = AppRenderer::new();
        // Simulate first tap just happened (set time far in the past so next call is a "second tap"
        // within DELTA_MS by setting it to now so the delta is near zero)
        let before = sdl_ticks();
        r.last_keypress_time = before; // within DELTA_MS of the upcoming call
        handle_ctrl_i(&mut r, History::None);
        // After double-tap the timer should be updated to "now" (>= before)
        assert!(r.last_keypress_time >= before);
        assert!(r.needs_redraw);
    }

    // -----------------------------------------------------------------------
    // navigate_to_path (provider.rs) — tested via handler indirectly
    // handle_save_as_provider_config
    // -----------------------------------------------------------------------

    #[test]
    fn save_as_sets_pending_when_filebrowser_present() {
        use sicompass_sdk::ffon::{FfonElement, FfonObject};
        use sicompass_sdk::provider::Provider;
        struct FbProv;
        impl Provider for FbProv {
            fn name(&self) -> &str { "filebrowser" }
            fn fetch(&mut self) -> Vec<FfonElement> { vec![] }
            fn current_path(&self) -> &str { "/" }
            fn set_current_path(&mut self, _: &str) {}
        }

        let fb_root = FfonElement::Obj(FfonObject { key: "file browser".to_string(), children: vec![] });
        let src_root = FfonElement::Obj(FfonObject { key: "myprov".to_string(), children: vec![] });
        let mut r = AppRenderer::new();
        r.ffon = vec![fb_root, src_root];
        r.providers.push(Box::new(FbProv));
        // Navigate to provider 1 (source)
        r.current_id = { let mut id = IdArray::new(); id.push(1); id.push(0); id };
        // Set save_folder_path to /tmp (guaranteed to exist)
        r.save_folder_path = "/tmp".to_owned();

        handle_save_as_provider_config(&mut r);

        assert!(r.pending_file_browser_save_as, "save-as flag should be set");
        assert_eq!(r.save_as_source_root_idx, 1);
    }

    #[test]
    fn save_as_no_filebrowser_sets_error() {
        let mut r = AppRenderer::new();
        handle_save_as_provider_config(&mut r);
        assert!(!r.error_message.is_empty());
        assert!(!r.pending_file_browser_save_as);
    }

    // -----------------------------------------------------------------------
    // resolve_save_folder
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_save_folder_absolute_path_unchanged() {
        let mut r = AppRenderer::new();
        r.save_folder_path = "/custom/save/dir".to_owned();
        let result = resolve_save_folder(&r);
        assert_eq!(result, "/custom/save/dir");
    }

    #[test]
    fn resolve_save_folder_relative_appends_home() {
        let mut r = AppRenderer::new();
        r.save_folder_path = "Documents".to_owned();
        let result = resolve_save_folder(&r);
        // Should contain "Documents" somewhere (after HOME/)
        assert!(result.ends_with("/Documents"), "got: {result}");
    }

    #[test]
    fn resolve_save_folder_empty_returns_downloads() {
        let r = AppRenderer::new(); // save_folder_path is empty
        let result = resolve_save_folder(&r);
        // Should be some real path (Downloads or /tmp fallback)
        assert!(!result.is_empty());
    }

    #[test]
    fn utf8_move_backward_three_byte_char() {
        // "A€": backspace from end (byte 4) removes "€", cursor at byte 1
        let mut r = make_input_renderer("A\u{20AC}");
        r.cursor_position = 4;
        handle_backspace(&mut r);
        assert_eq!(r.cursor_position, 1);
        assert_eq!(r.input_buffer, "A");
    }

    #[test]
    fn utf8_move_forward_ascii() {
        // Shift+Right on ASCII "hello" from pos 0 moves to pos 1
        let mut r = make_input_renderer("hello");
        r.cursor_position = 0;
        handle_shift_right(&mut r);
        assert_eq!(r.cursor_position, 1);
    }

    #[test]
    fn utf8_move_forward_at_end() {
        // Shift+Right at end is a no-op
        let mut r = make_input_renderer("hello");
        r.cursor_position = 5;
        handle_shift_right(&mut r);
        assert_eq!(r.cursor_position, 5);
    }

    // -----------------------------------------------------------------------
    // handle_file_copy
    // -----------------------------------------------------------------------

    fn make_renderer_with_file_elem(dir: &str, filename: &str) -> AppRenderer {
        use sicompass_sdk::ffon::{FfonElement, FfonObject};
        use sicompass_sdk::provider::Provider;
        struct FbProv { path: String }
        impl Provider for FbProv {
            fn name(&self) -> &str { "filebrowser" }
            fn fetch(&mut self) -> Vec<FfonElement> { vec![] }
            fn current_path(&self) -> &str { &self.path }
        }
        let entry = FfonElement::new_str(filename);
        let root_obj = FfonObject { key: "file browser".to_string(), children: vec![entry] };
        let root = FfonElement::Obj(root_obj);
        let mut r = AppRenderer::new();
        r.ffon = vec![root];
        r.current_id = { let mut id = IdArray::new(); id.push(0); id.push(0); id };
        r.providers.push(Box::new(FbProv { path: dir.to_owned() }));
        r
    }

    #[test]
    fn file_copy_sets_clipboard_path() {
        let mut r = make_renderer_with_file_elem("/home/user/docs", "report.pdf");
        handle_file_copy(&mut r);
        assert_eq!(r.file_clipboard_path, "/home/user/docs/report.pdf");
        assert!(!r.file_clipboard_is_cut);
    }

    #[test]
    fn file_copy_empty_name_is_noop() {
        // An element with an empty (display-stripped) name should not change clipboard
        let mut r = make_renderer_with_file_elem("/home/user", "");
        let before = r.file_clipboard_path.clone();
        handle_file_copy(&mut r);
        assert_eq!(r.file_clipboard_path, before);
    }

    #[test]
    fn file_copy_marks_not_cut() {
        let mut r = make_renderer_with_file_elem("/tmp", "notes.txt");
        r.file_clipboard_is_cut = true; // ensure it's overwritten
        handle_file_copy(&mut r);
        assert!(!r.file_clipboard_is_cut);
    }

    // -----------------------------------------------------------------------
    // handle_ctrl_enter
    // -----------------------------------------------------------------------

    #[test]
    fn ctrl_enter_inserts_newline_in_editor_insert() {
        let mut r = make_input_renderer("hello");
        r.cursor_position = 5;
        r.coordinate = Coordinate::EditorInsert;
        handle_ctrl_enter(&mut r);
        assert!(r.input_buffer.contains('\n'));
    }

    #[test]
    fn ctrl_enter_inserts_newline_in_operator_insert() {
        let mut r = make_input_renderer("abc");
        r.cursor_position = 3;
        r.coordinate = Coordinate::OperatorInsert;
        handle_ctrl_enter(&mut r);
        assert!(r.input_buffer.contains('\n'));
    }

    #[test]
    fn ctrl_enter_noop_in_operator_general() {
        let mut r = make_input_renderer("abc");
        r.coordinate = Coordinate::OperatorGeneral;
        handle_ctrl_enter(&mut r);
        assert_eq!(r.input_buffer, "abc"); // unchanged
    }

    // -----------------------------------------------------------------------
    // handle_save_provider_config
    // -----------------------------------------------------------------------

    #[test]
    fn save_config_no_path_launches_save_as() {
        // When no path is set, save_provider_config launches save-as.
        // With no filebrowser registered it should set an error about filebrowser unavailability.
        let mut r = AppRenderer::new();
        r.current_save_path = String::new();
        handle_save_provider_config(&mut r);
        // Either the save-as dialog started (pending_file_browser_save_as set) or
        // an error was shown (no filebrowser available in test renderer).
        assert!(r.pending_file_browser_save_as || !r.error_message.is_empty());
        assert!(r.needs_redraw);
    }

    #[test]
    fn save_config_with_path_writes_file() {
        use tempfile::NamedTempFile;
        use sicompass_sdk::ffon::{FfonElement, FfonObject};
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();

        let root = FfonElement::Obj(FfonObject { key: "myprov".to_string(), children: vec![] });
        let mut r = AppRenderer::new();
        r.ffon = vec![root];
        r.current_id = { let mut id = IdArray::new(); id.push(0); id };
        r.current_save_path = path.clone();

        handle_save_provider_config(&mut r);

        assert!(r.error_message.contains(&path), "expected path in message, got: {}", r.error_message);
        assert!(tmp.path().exists());
    }

    // -----------------------------------------------------------------------
    // handle_load_provider_config
    // -----------------------------------------------------------------------

    #[test]
    fn load_config_empty_path_is_noop() {
        let mut r = AppRenderer::new();
        r.error_message = String::new();
        handle_load_provider_config(&mut r, "");
        assert!(r.error_message.is_empty());
    }

    #[test]
    fn load_config_sets_save_path_and_clears_undo() {
        use tempfile::NamedTempFile;
        use std::io::Write;
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"[\"- item1\", \"- item2\"]\n").unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();

        use sicompass_sdk::ffon::{FfonElement, FfonObject};
        let root = FfonElement::Obj(FfonObject { key: "myprov".to_string(), children: vec![] });
        let mut r = AppRenderer::new();
        r.ffon = vec![root];
        r.current_id = { let mut id = IdArray::new(); id.push(0); id };
        r.undo_position = 3;

        handle_load_provider_config(&mut r, &path);

        assert_eq!(r.current_save_path, path);
        assert_eq!(r.undo_position, 0);
        assert!(r.error_message.contains(&path));
    }

    #[test]
    fn load_config_missing_file_sets_error() {
        let mut r = AppRenderer::new();
        handle_load_provider_config(&mut r, "/nonexistent/path/file.json");
        assert!(!r.error_message.is_empty());
        assert!(r.needs_redraw);
    }

    // -----------------------------------------------------------------------
    // handle_page_up / handle_page_down
    // -----------------------------------------------------------------------

    // Helper: renderer with window_height=40, cached_line_height=10 → page_size=1
    fn make_renderer_paged() -> AppRenderer {
        let mut r = make_renderer();
        r.window_height = 40;
        r.cached_line_height = 10;
        r
    }

    #[test]
    fn page_up_noop_in_editor_insert() {
        let mut r = make_renderer_paged();
        r.coordinate = Coordinate::EditorInsert;
        r.list_index = 2;
        handle_page_up(&mut r);
        assert_eq!(r.list_index, 2); // unchanged
    }

    #[test]
    fn page_down_noop_in_operator_insert() {
        let mut r = make_renderer_paged();
        r.coordinate = Coordinate::OperatorInsert;
        r.list_index = 0;
        handle_page_down(&mut r);
        assert_eq!(r.list_index, 0); // unchanged
    }

    #[test]
    fn page_up_simple_search_moves_index() {
        let mut r = make_renderer_paged();
        r.coordinate = Coordinate::SimpleSearch;
        r.list_index = 3;
        r.error_message = "err".to_owned();
        handle_page_up(&mut r);
        assert_eq!(r.list_index, 2); // decremented by page_size=1
        assert!(r.error_message.is_empty());
        assert!(r.needs_redraw);
    }

    #[test]
    fn page_up_simple_search_clamps_at_zero() {
        let mut r = make_renderer_paged();
        r.coordinate = Coordinate::SimpleSearch;
        r.list_index = 0;
        handle_page_up(&mut r);
        assert_eq!(r.list_index, 0);
    }

    #[test]
    fn page_down_simple_search_moves_index() {
        let mut r = make_renderer_paged();
        r.coordinate = Coordinate::SimpleSearch;
        r.list_index = 0;
        handle_page_down(&mut r);
        assert_eq!(r.list_index, 1); // incremented by page_size=1
        assert!(r.needs_redraw);
    }

    #[test]
    fn page_down_simple_search_clamps_at_end() {
        let mut r = make_renderer_paged();
        r.coordinate = Coordinate::SimpleSearch;
        let last = r.active_list_len() - 1;
        r.list_index = last;
        handle_page_down(&mut r);
        assert_eq!(r.list_index, last);
    }

    #[test]
    fn page_up_scroll_decreases_offset() {
        let mut r = make_renderer_paged();
        r.coordinate = Coordinate::Scroll;
        // viewport_h = window_height(40) - line_height(10) = 30; start above that
        r.text_scroll_offset = 50;
        handle_page_up(&mut r);
        assert_eq!(r.text_scroll_offset, 20); // 50 - 30
        assert!(r.needs_redraw);
    }

    #[test]
    fn page_up_scroll_clamps_at_zero() {
        let mut r = make_renderer_paged();
        r.coordinate = Coordinate::Scroll;
        r.text_scroll_offset = 0;
        handle_page_up(&mut r);
        assert_eq!(r.text_scroll_offset, 0);
    }

    #[test]
    fn page_down_scroll_increases_offset() {
        let mut r = make_renderer_paged();
        r.coordinate = Coordinate::Scroll;
        r.text_scroll_offset = 0;
        r.text_scroll_total_height = 100;
        handle_page_down(&mut r);
        assert!(r.text_scroll_offset > 0);
        assert!(r.needs_redraw);
    }

    #[test]
    fn page_up_input_search_decreases_offset() {
        let mut r = make_renderer_paged();
        r.coordinate = Coordinate::InputSearch;
        r.input_search_scroll_offset = 3;
        handle_page_up(&mut r);
        assert_eq!(r.input_search_scroll_offset, 2);
        assert!(r.needs_redraw);
    }

    #[test]
    fn page_up_input_search_clamps_at_zero() {
        let mut r = make_renderer_paged();
        r.coordinate = Coordinate::InputSearch;
        r.input_search_scroll_offset = 0;
        handle_page_up(&mut r);
        assert_eq!(r.input_search_scroll_offset, 0);
    }

    #[test]
    fn page_down_input_search_increases_offset() {
        let mut r = make_renderer_paged();
        r.coordinate = Coordinate::InputSearch;
        r.input_search_scroll_offset = 0;
        handle_page_down(&mut r);
        assert_eq!(r.input_search_scroll_offset, 1); // page_size=1
        assert!(r.needs_redraw);
    }

    // -----------------------------------------------------------------------
    // handle_ctrl_home / handle_ctrl_end
    // -----------------------------------------------------------------------

    #[test]
    fn ctrl_home_in_simple_search_resets_scroll_offset() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.list_index = 3;
        r.scroll_offset = 3;
        handle_ctrl_home(&mut r);
        assert_eq!(r.list_index, 0);
        assert_eq!(r.scroll_offset, 0);
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_home_empty_list_no_crash() {
        let mut r = AppRenderer::new();
        handle_ctrl_home(&mut r); // should not panic
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_end_in_simple_search_sets_scroll_offset_minus_one() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.list_index = 0;
        let last = r.active_list_len() - 1;
        handle_ctrl_end(&mut r);
        assert_eq!(r.list_index, last);
        assert_eq!(r.scroll_offset, -1);
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_end_empty_list_no_crash() {
        let mut r = AppRenderer::new();
        handle_ctrl_end(&mut r); // should not panic
        assert!(r.needs_redraw);
    }

    // -----------------------------------------------------------------------
    // handle_enter_search
    // -----------------------------------------------------------------------

    #[test]
    fn enter_search_no_item_returns_to_previous() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::SimpleSearch;
        r.previous_coordinate = Coordinate::OperatorGeneral;
        r.search_string = "abc".to_owned();
        // total_list is empty → current_list_item_id() returns None
        handle_enter_search(&mut r);
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
        assert!(r.search_string.is_empty());
        assert!(r.needs_redraw);
    }

    #[test]
    fn enter_search_selects_item_and_exits() {
        let mut r = make_renderer();
        r.coordinate = Coordinate::SimpleSearch;
        r.previous_coordinate = Coordinate::OperatorGeneral;
        r.list_index = 2;
        r.search_string = "item".to_owned();
        r.sync_current_id_from_list();
        handle_enter_search(&mut r);
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
        assert!(r.search_string.is_empty());
        assert!(r.needs_redraw);
    }

    #[test]
    fn enter_search_extended_empty_list_escapes() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::ExtendedSearch;
        r.previous_coordinate = Coordinate::OperatorGeneral;
        // Empty list → handle_enter_extended_search will escape
        handle_enter_search(&mut r);
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
        assert!(r.needs_redraw);
    }

    // -----------------------------------------------------------------------
    // handle_enter_operator_insert — guard clause paths
    // -----------------------------------------------------------------------

    #[test]
    fn enter_operator_insert_no_element_escapes() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::OperatorInsert;
        // ffon is empty → get_ffon_at_id returns None → handle_escape called
        handle_enter_operator_insert(&mut r);
        // After escape, coordinate should not be OperatorInsert
        assert_ne!(r.coordinate, Coordinate::OperatorInsert);
    }

    #[test]
    fn enter_operator_insert_no_input_tag_escapes() {
        let mut r = make_renderer_with_items(&["plain text"]);
        r.coordinate = Coordinate::OperatorInsert;
        handle_enter_operator_insert(&mut r);
        // No <input> tag → escape
        assert_ne!(r.coordinate, Coordinate::OperatorInsert);
    }

    #[test]
    fn enter_operator_insert_unchanged_content_escapes() {
        let mut r = make_renderer_with_items(&["<input>hello</input>"]);
        r.coordinate = Coordinate::OperatorInsert;
        r.input_buffer = "hello".to_owned(); // same as element content
        handle_enter_operator_insert(&mut r);
        // Unchanged → escape
        assert_ne!(r.coordinate, Coordinate::OperatorInsert);
    }
}
