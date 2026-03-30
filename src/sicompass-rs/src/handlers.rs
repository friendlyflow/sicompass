//! Key handlers — equivalent to `handlers.c`.
//!
//! Each function corresponds to one key action and mutates `AppRenderer`
//! in-place. Rendering is triggered by setting `needs_redraw = true`.

use crate::app_state::{AppRenderer, Coordinate};
use crate::list;
use sicompass_sdk::ffon::next_layer_exists;

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

/// Enter insert mode on the current item.
pub fn handle_i(r: &mut AppRenderer) {
    // TODO Phase 4+: populate input_buffer from selected element's input tag
    r.previous_coordinate = r.coordinate;
    r.coordinate = Coordinate::EditorInsert;
    r.caret.reset(sdl_ticks());
    r.needs_redraw = true;
}

/// Enter append mode on the current item.
pub fn handle_a(r: &mut AppRenderer) {
    r.previous_coordinate = r.coordinate;
    r.coordinate = Coordinate::EditorInsert;
    // TODO Phase 4+: place cursor at end
    r.caret.reset(sdl_ticks());
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
        Coordinate::Command | Coordinate::EditorInsert => {
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
        Coordinate::Command | Coordinate::EditorInsert => {
            if r.cursor_position > 0 {
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

/// Handle F5 — refresh current provider.
pub fn handle_f5(r: &mut AppRenderer) {
    crate::provider::refresh_current_directory(r);
    list::create_list_current_layer(r);
    r.needs_redraw = true;
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

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
