//! Key event routing — mirrors `events.c` `handleKeys()`.
//!
//! In the C version `events.c` contained a monolithic `handleKeys()` function
//! that dispatched SDL key events to individual handlers based on the current
//! `Coordinate` mode.  In the Rust port that logic lives in
//! `view::handle_keydown` (which has direct access to both `AppState` and
//! `AppRenderer`).  This module documents the full key-binding table and
//! re-exports the handler entry points so the table is easy to audit.
//!
//! # Key bindings summary
//!
//! | Key           | Modifier | Mode(s)                            | Handler                    |
//! |---------------|----------|------------------------------------|----------------------------|
//! | Up / K        | —        | OperatorGeneral, EditorGeneral,    | handle_up                  |
//! |               |          | SimpleSearch, Scroll                |                            |
//! | Down / J      | —        | same                               | handle_down                |
//! | Right / L     | —        | OperatorGeneral, EditorGeneral      | handle_right               |
//! | Left / H      | —        | same                               | handle_left                |
//! | Up            | —        | EditorInsert, OperatorInsert       | handle_up_insert           |
//! | Down          | —        | same                               | handle_down_insert         |
//! | Up            | Shift    | same                               | handle_shift_up_insert     |
//! | Down          | Shift    | same                               | handle_shift_down_insert   |
//! | Left          | Shift    | insert/search/command              | handle_shift_left          |
//! | Right         | Shift    | same                               | handle_shift_right         |
//! | Home          | —        | OperatorGeneral, EditorGeneral      | handle_home (first item)   |
//! | Home          | —        | insert/search/command              | handle_home (line start)   |
//! | End           | —        | OperatorGeneral, EditorGeneral      | handle_end (last item)     |
//! | End           | —        | insert/search/command              | handle_end (line end)      |
//! | Home          | Shift    | insert/search/command              | handle_shift_home          |
//! | End           | Shift    | same                               | handle_shift_end           |
//! | Home          | Ctrl     | search/command                     | handle_ctrl_home           |
//! | End           | Ctrl     | same                               | handle_ctrl_end            |
//! | PageUp        | —        | navigation modes                   | handle_page_up             |
//! | PageDown      | —        | same                               | handle_page_down           |
//! | Tab           | —        | most modes                         | handle_tab                 |
//! | Return        | —        | OperatorGeneral                    | handle_enter_operator      |
//! | Return        | —        | EditorGeneral                      | handle_append              |
//! | Return        | —        | SimpleSearch                       | handle_enter_search        |
//! | Return        | —        | EditorInsert / EditorNormal        | update_state(Input)        |
//! | Return        | —        | OperatorInsert                     | handle_enter_operator_ins  |
//! | Return        | —        | Command                            | handle_enter_command       |
//! | I             | —        | OperatorGeneral, EditorGeneral      | handle_i                   |
//! | A             | —        | same                               | handle_a                   |
//! | A             | Ctrl     | OperatorGeneral                    | handle_ctrl_a_operator     |
//! | I             | Ctrl     | OperatorGeneral                    | handle_ctrl_i_operator     |
//! | A             | Ctrl     | EditorGeneral                      | handle_append              |
//! | I             | Ctrl     | EditorGeneral                      | handle_insert              |
//! | A             | Ctrl     | insert/search/command              | handle_select_all          |
//! | D             | Ctrl     | EditorGeneral, OperatorGeneral      | handle_delete              |
//! | Delete        | —        | OperatorGeneral                    | handle_delete              |
//! | Delete        | —        | insert/search/command              | handle_delete_forward      |
//! | S             | —        | OperatorGeneral                    | handle_s (enter Scroll)    |
//! | M             | —        | OperatorGeneral                    | handle_meta                |
//! | Space         | —        | OperatorGeneral, EditorGeneral      | handle_space               |
//! | Z             | Ctrl     | navigation modes                   | handle_undo                |
//! | Z             | Ctrl+Shift | same                             | handle_redo                |
//! | X             | Ctrl     | most modes                         | handle_ctrl_x              |
//! | C             | Ctrl     | same                               | handle_ctrl_c              |
//! | V             | Ctrl     | same                               | handle_ctrl_v              |
//! | F             | Ctrl     | most modes                         | handle_ctrl_f              |
//! | F5            | —        | OperatorGeneral, EditorGeneral      | handle_f5                  |
//! | Backspace     | —        | all editing modes                  | handle_backspace           |
//! | Escape        | —        | all modes                          | handle_escape              |
//! | Shift+;       | —        | navigation modes                   | handle_colon               |

use crate::app_state::{AppRenderer, Coordinate, History, Task};
use crate::handlers;
use crate::list;
use sdl3::keyboard::{Keycode, Mod};
use tracing;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::AppRenderer;

    fn no_mod() -> Mod { Mod::empty() }
    fn ctrl()   -> Mod { Mod::LCTRLMOD }
    fn shift()  -> Mod { Mod::LSHIFTMOD }
    fn ctrl_shift() -> Mod { Mod::LCTRLMOD | Mod::LSHIFTMOD }

    // --- Tab ---

    #[test]
    fn tab_in_operator_switches_to_simple_search() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::Tab), no_mod());
        assert_eq!(r.coordinate, Coordinate::SimpleSearch);
    }

    // --- Colon (Shift+Semicolon) ---

    fn make_renderer_inside_provider() -> AppRenderer {
        use sicompass_sdk::ffon::{FfonElement, IdArray};
        let mut root = FfonElement::new_obj("provider");
        root.as_obj_mut().unwrap().push(FfonElement::new_str("item"));
        let mut r = AppRenderer::new();
        r.ffon = vec![root];
        r.current_id = { let mut id = IdArray::new(); id.push(0); id.push(0); id };
        list::create_list_current_layer(&mut r);
        r
    }

    #[test]
    fn colon_in_operator_switches_to_command() {
        let mut r = make_renderer_inside_provider();
        dispatch_key(&mut r, Some(Keycode::Semicolon), shift());
        assert_eq!(r.coordinate, Coordinate::Command);
    }

    #[test]
    fn colon_blocked_at_root() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::Semicolon), shift());
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
    }

    #[test]
    fn colon_blocked_in_editor_insert() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorInsert;
        dispatch_key(&mut r, Some(Keycode::Semicolon), shift());
        assert_eq!(r.coordinate, Coordinate::EditorInsert);
    }

    // --- Space toggle ---

    #[test]
    fn space_in_operator_switches_to_editor() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::Space), no_mod());
        assert_eq!(r.coordinate, Coordinate::EditorGeneral);
        assert_eq!(r.previous_coordinate, Coordinate::OperatorGeneral);
        assert!(r.needs_redraw);
    }

    #[test]
    fn space_in_editor_switches_to_operator() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        dispatch_key(&mut r, Some(Keycode::Space), no_mod());
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
        assert_eq!(r.previous_coordinate, Coordinate::EditorGeneral);
        assert!(r.needs_redraw);
    }

    #[test]
    fn space_noop_in_editor_insert() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorInsert;
        dispatch_key(&mut r, Some(Keycode::Space), no_mod());
        assert_eq!(r.coordinate, Coordinate::EditorInsert);
    }

    // --- Ctrl+F → SimpleSearch (from OperatorGeneral) ---

    #[test]
    fn ctrl_f_in_operator_switches_to_extended_search() {
        // C spec: Ctrl+F from OperatorGeneral enters ExtendedSearch (not SimpleSearch)
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::F), ctrl());
        assert_eq!(r.coordinate, Coordinate::ExtendedSearch);
    }

    // --- Escape ---

    #[test]
    fn escape_in_editor_insert_returns_to_editor_general() {
        // C spec: EditorInsert → updateState(Input) → EditorGeneral
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorInsert;
        dispatch_key(&mut r, Some(Keycode::Escape), no_mod());
        assert_eq!(r.coordinate, Coordinate::EditorGeneral);
    }

    #[test]
    fn dispatch_key_returns_false() {
        let mut r = AppRenderer::new();
        assert!(!dispatch_key(&mut r, Some(Keycode::Escape), no_mod()));
        assert!(!dispatch_key(&mut r, Some(Keycode::Q), no_mod()));
        assert!(!dispatch_key(&mut r, Some(Keycode::Tab), no_mod()));
    }

    // --- Ctrl+A select all in EditorInsert ---

    #[test]
    fn ctrl_a_in_editor_insert_selects_all() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorInsert;
        r.input_buffer = "hello".to_string();
        r.cursor_position = 0;
        dispatch_key(&mut r, Some(Keycode::A), ctrl());
        assert_eq!(r.selection_anchor, Some(0));
        assert_eq!(r.cursor_position, 5);
    }

    // --- Shift+Left in EditorInsert ---

    #[test]
    fn shift_left_in_editor_insert_starts_selection() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorInsert;
        r.input_buffer = "hello".to_string();
        r.cursor_position = 3;
        dispatch_key(&mut r, Some(Keycode::Left), shift());
        assert_eq!(r.selection_anchor, Some(3));
        assert_eq!(r.cursor_position, 2);
    }

    // --- Shift+Right in SimpleSearch ---

    #[test]
    fn shift_right_in_simple_search_starts_selection() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::SimpleSearch;
        r.search_string = "hello".to_string();
        r.cursor_position = 2;
        dispatch_key(&mut r, Some(Keycode::Right), shift());
        assert_eq!(r.selection_anchor, Some(2));
        assert_eq!(r.cursor_position, 3);
    }

    // --- Ctrl+I in EditorGeneral → handle_insert ---

    #[test]
    fn ctrl_i_in_editor_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        dispatch_key(&mut r, Some(Keycode::I), ctrl());
        assert!(r.needs_redraw);
    }

    // --- Ctrl+D in OperatorGeneral → no crash ---

    #[test]
    fn ctrl_d_in_operator_general_no_crash() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::D), ctrl());
        // handle_file_delete with no providers — no crash
    }

    // --- Ctrl+D in EditorGeneral → no crash ---

    #[test]
    fn ctrl_d_in_editor_general_no_crash() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        dispatch_key(&mut r, Some(Keycode::D), ctrl());
    }

    // --- Delete key in OperatorGeneral → no crash ---

    #[test]
    fn delete_key_in_operator_general_no_crash() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::Delete), no_mod());
    }

    // --- K/J/Up in navigation modes set needs_redraw ---

    #[test]
    fn k_in_operator_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::K), no_mod());
        assert!(r.needs_redraw);
    }

    #[test]
    fn j_in_editor_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        dispatch_key(&mut r, Some(Keycode::J), no_mod());
        assert!(r.needs_redraw);
    }

    #[test]
    fn up_arrow_in_simple_search_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::SimpleSearch;
        dispatch_key(&mut r, Some(Keycode::Up), no_mod());
        assert!(r.needs_redraw);
    }

    // --- Ctrl+Z / Ctrl+Shift+Z set needs_redraw ---

    #[test]
    fn ctrl_z_in_editor_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        dispatch_key(&mut r, Some(Keycode::Z), ctrl());
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_shift_z_in_editor_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        dispatch_key(&mut r, Some(Keycode::Z), ctrl_shift());
        assert!(r.needs_redraw);
    }

    // --- D in EditorGeneral → not handled (no needs_redraw, no coordinate change) ---

    #[test]
    fn d_in_editor_general_not_handled() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        dispatch_key(&mut r, Some(Keycode::D), no_mod());
        assert_eq!(r.coordinate, Coordinate::EditorGeneral);
    }

    // --- Ctrl+A in OperatorGeneral → handle_ctrl_a_operator → stays OperatorGeneral ---

    #[test]
    fn ctrl_a_in_operator_general_stays_operator() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::A), ctrl());
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
    }

    // --- Ctrl+A in EditorGeneral → handle_append → sets needs_redraw ---

    #[test]
    fn ctrl_a_in_editor_general_appends() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        dispatch_key(&mut r, Some(Keycode::A), ctrl());
        assert!(r.needs_redraw);
    }

    // --- Return in EditorGeneral → handle_append → sets needs_redraw ---

    #[test]
    fn enter_in_editor_general_appends() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        dispatch_key(&mut r, Some(Keycode::Return), no_mod());
        assert!(r.needs_redraw);
    }

    // --- Return in OperatorGeneral → handle_enter_operator → sets needs_redraw ---

    #[test]
    fn enter_in_operator_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::Return), no_mod());
        assert!(r.needs_redraw);
    }

    // --- Ctrl+I in OperatorGeneral → handle_ctrl_i_operator → no crash ---

    #[test]
    fn ctrl_i_in_operator_general_no_crash() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::I), ctrl());
        // handle_ctrl_i_operator with empty ffon — no crash
    }

    // --- Ctrl+X/C/V in EditorGeneral → sets needs_redraw ---

    #[test]
    fn ctrl_x_in_editor_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        dispatch_key(&mut r, Some(Keycode::X), ctrl());
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_c_in_editor_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        dispatch_key(&mut r, Some(Keycode::C), ctrl());
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_v_in_editor_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        dispatch_key(&mut r, Some(Keycode::V), ctrl());
        assert!(r.needs_redraw);
    }

    // --- H moves left / L moves right ---

    #[test]
    fn h_moves_left_in_operator_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::H), no_mod());
        assert!(r.needs_redraw);
    }

    #[test]
    fn l_moves_right_in_editor_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        dispatch_key(&mut r, Some(Keycode::L), no_mod());
        assert!(r.needs_redraw);
    }

    // --- D in OperatorGeneral → handle_dashboard ---

    #[test]
    fn d_in_operator_general_goes_to_dashboard() {
        let mut r = AppRenderer::new();
        // No provider — handle_dashboard returns early, but dispatch routes to it
        dispatch_key(&mut r, Some(Keycode::D), no_mod());
        // Coordinate stays OperatorGeneral when no provider has a dashboard image
        assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
    }

    #[test]
    fn d_in_editor_general_is_not_handled() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorGeneral;
        let before = r.coordinate;
        dispatch_key(&mut r, Some(Keycode::D), no_mod());
        assert_eq!(r.coordinate, before);
    }

    // --- Ctrl+Enter in EditorInsert → insert newline ---

    #[test]
    fn ctrl_enter_in_editor_insert_inserts_newline() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::EditorInsert;
        r.input_buffer = "hello".to_string();
        r.cursor_position = 5;
        dispatch_key(&mut r, Some(Keycode::Return), ctrl());
        assert!(r.input_buffer.contains('\n'));
        assert!(r.needs_redraw);
    }
}

/// Dispatch a key event to the appropriate handler based on the current mode.
///
/// Returns `true` if the application should quit (Escape/Q in OperatorGeneral),
/// `false` otherwise.  The caller is responsible for acting on the quit signal.
///
/// This function is the central key dispatcher — used by the main event loop
/// (via `view::handle_keydown`) and directly by the integration test harness.
pub fn dispatch_key(r: &mut AppRenderer, keycode: Option<Keycode>, keymod: Mod) -> bool {
    tracing::debug!(
        ?keycode, ?keymod,
        mode = r.coordinate.as_str(),
        "dispatch_key"
    );
    let ctrl  = keymod.intersects(Mod::LCTRLMOD  | Mod::RCTRLMOD);
    let shift = keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
    let at_root = r.current_id.depth() <= 1;

    // In the Ctrl+O open-file flow the file browser is a read-only .json picker.
    // Restrict OperatorGeneral/EditorGeneral to navigation+selection only.
    // SimpleSearch/ExtendedSearch fall through to normal dispatch so the user
    // can still type search queries; only clipboard/undo ops are blocked there.
    if r.pending_file_browser_open {
        match r.coordinate {
            Coordinate::OperatorGeneral | Coordinate::EditorGeneral => {
                match keycode {
                    Some(Keycode::Up) | Some(Keycode::K) if !ctrl && !shift => handlers::handle_up(r),
                    Some(Keycode::Down) | Some(Keycode::J) if !ctrl && !shift => handlers::handle_down(r),
                    Some(Keycode::Right) | Some(Keycode::L) if !ctrl && !shift => handlers::handle_right(r),
                    Some(Keycode::Left) | Some(Keycode::H) if !ctrl && !shift => handlers::handle_left(r),
                    Some(Keycode::PageUp) if !ctrl && !shift => handlers::handle_page_up(r),
                    Some(Keycode::PageDown) if !ctrl && !shift => handlers::handle_page_down(r),
                    Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
                    Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
                    Some(Keycode::Return) | Some(Keycode::KpEnter) if !ctrl => handlers::handle_enter_operator(r),
                    Some(Keycode::Escape) => handlers::handle_escape(r),
                    Some(Keycode::Backspace) => handlers::handle_backspace(r),
                    Some(Keycode::Tab) => handlers::handle_tab(r),
                    Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
                    _ => {}
                }
                return false;
            }
            Coordinate::SimpleSearch | Coordinate::ExtendedSearch => {
                // Block clipboard and undo; let all other search-mode keys through.
                match keycode {
                    Some(Keycode::X) if ctrl => return false,
                    Some(Keycode::V) if ctrl => return false,
                    Some(Keycode::Z) if ctrl => return false,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    match r.coordinate {
        // ---- Operator general -----------------------------------------------
        Coordinate::OperatorGeneral => match keycode {
            Some(Keycode::Up) | Some(Keycode::K) if !ctrl && !shift => handlers::handle_up(r),
            Some(Keycode::Down) | Some(Keycode::J) if !ctrl && !shift => handlers::handle_down(r),
            Some(Keycode::Right) | Some(Keycode::L) if !ctrl && !shift => handlers::handle_right(r),
            Some(Keycode::Left) | Some(Keycode::H) if !ctrl && !shift => handlers::handle_left(r),
            Some(Keycode::PageUp) if !ctrl && !shift => handlers::handle_page_up(r),
            Some(Keycode::PageDown) if !ctrl && !shift => handlers::handle_page_down(r),
            Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
            Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
            Some(Keycode::Tab) => handlers::handle_tab(r),
            Some(Keycode::Colon) if !ctrl && !shift && !at_root => handlers::handle_colon(r),
            Some(Keycode::Semicolon) if shift && !at_root => handlers::handle_colon(r),
            Some(Keycode::Return) | Some(Keycode::KpEnter) if !ctrl && !at_root => {
                handlers::handle_enter_operator(r);
            }
            Some(Keycode::I) if !ctrl && !shift && !at_root => handlers::handle_i(r),
            Some(Keycode::A) if !ctrl && !shift && !at_root => handlers::handle_a(r),
            Some(Keycode::A) if ctrl && !shift && !at_root => handlers::handle_ctrl_a_operator(r),
            Some(Keycode::I) if ctrl && !shift && !at_root => handlers::handle_ctrl_i_operator(r),
            Some(Keycode::D) if ctrl && !shift && !at_root => handlers::handle_file_delete(r),
            Some(Keycode::Delete) if !ctrl && !shift && !at_root => handlers::handle_file_delete(r),
            Some(Keycode::D) if !ctrl && !shift => handlers::handle_dashboard(r),
            Some(Keycode::S) if !ctrl && !shift && !at_root => handlers::handle_s(r),
            Some(Keycode::M) if !ctrl && !shift => handlers::handle_meta(r),
            Some(Keycode::Space) if !ctrl && !shift => handlers::handle_space(r),
            Some(Keycode::Z) if ctrl && !shift && !at_root => handlers::handle_undo(r),
            Some(Keycode::Z) if ctrl && shift && !at_root => handlers::handle_redo(r),
            Some(Keycode::X) if ctrl && !shift && !at_root => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift && !at_root => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift && !at_root => handlers::handle_ctrl_v(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
            Some(Keycode::S) if ctrl && !shift => handlers::handle_save_provider_config(r),
            Some(Keycode::S) if ctrl && shift => {
                if r.providers.get(r.current_id.get(0).unwrap_or(0))
                    .map(|p| p.supports_config_files()).unwrap_or(false) {
                    handlers::handle_save_as_provider_config(r);
                }
            }
            Some(Keycode::O) if ctrl && !shift => {
                if r.providers.get(r.current_id.get(0).unwrap_or(0))
                    .map(|p| p.supports_config_files()).unwrap_or(false) {
                    handlers::handle_file_browser_open(r);
                }
            }
            Some(Keycode::Escape) => handlers::handle_escape(r),
            Some(Keycode::F5) if !at_root => handlers::handle_f5(r),
            Some(Keycode::Backspace) if !at_root => handlers::handle_backspace(r),
            _ => {}
        },

        // ---- Editor general -------------------------------------------------
        Coordinate::EditorGeneral => match keycode {
            Some(Keycode::Up) | Some(Keycode::K) if !ctrl && !shift => handlers::handle_up(r),
            Some(Keycode::Down) | Some(Keycode::J) if !ctrl && !shift => handlers::handle_down(r),
            Some(Keycode::Right) | Some(Keycode::L) if !ctrl && !shift => handlers::handle_right(r),
            Some(Keycode::Left) | Some(Keycode::H) if !ctrl && !shift => handlers::handle_left(r),
            Some(Keycode::PageUp) if !ctrl && !shift => handlers::handle_page_up(r),
            Some(Keycode::PageDown) if !ctrl && !shift => handlers::handle_page_down(r),
            Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
            Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
            Some(Keycode::Tab) => handlers::handle_tab(r),
            Some(Keycode::Colon) if !ctrl && !shift => handlers::handle_colon(r),
            Some(Keycode::Semicolon) if shift => handlers::handle_colon(r),
            Some(Keycode::Return) | Some(Keycode::KpEnter) if !ctrl => handlers::handle_append(r),
            Some(Keycode::I) if !ctrl && !shift => handlers::handle_i(r),
            Some(Keycode::A) if !ctrl && !shift => handlers::handle_a(r),
            Some(Keycode::A) if ctrl && !shift => handlers::handle_ctrl_a(r, History::None),
            Some(Keycode::I) if ctrl && !shift => handlers::handle_ctrl_i(r, History::None),
            Some(Keycode::D) if ctrl && !shift => handlers::handle_delete(r, History::None),
            Some(Keycode::Space) if !ctrl && !shift => handlers::handle_space(r),
            Some(Keycode::Z) if ctrl && !shift => handlers::handle_undo(r),
            Some(Keycode::Z) if ctrl && shift => handlers::handle_redo(r),
            Some(Keycode::X) if ctrl && !shift => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift => handlers::handle_ctrl_v(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
            Some(Keycode::F5) => handlers::handle_f5(r),
            Some(Keycode::Backspace) => handlers::handle_backspace(r),
            Some(Keycode::Escape) => handlers::handle_escape(r),
            _ => {}
        },

        // ---- Simple search / extended search --------------------------------
        Coordinate::SimpleSearch | Coordinate::ExtendedSearch => match keycode {
            Some(Keycode::Up) | Some(Keycode::K) if !ctrl && !shift => handlers::handle_up(r),
            Some(Keycode::Down) | Some(Keycode::J) if !ctrl && !shift => handlers::handle_down(r),
            Some(Keycode::PageUp) if !ctrl && !shift => handlers::handle_page_up(r),
            Some(Keycode::PageDown) if !ctrl && !shift => handlers::handle_page_down(r),
            Some(Keycode::Home) if ctrl => handlers::handle_ctrl_home(r),
            Some(Keycode::End) if ctrl => handlers::handle_ctrl_end(r),
            Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
            Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
            Some(Keycode::Home) if shift => handlers::handle_shift_home(r),
            Some(Keycode::End) if shift => handlers::handle_shift_end(r),
            Some(Keycode::Left) if shift => handlers::handle_shift_left(r),
            Some(Keycode::Right) if shift => handlers::handle_shift_right(r),
            Some(Keycode::Left) if !ctrl && !shift => {
                // If selection is active: collapse to selection start and clear.
                if handlers::has_selection(r) {
                    if let Some((start, _)) = handlers::selection_range(r) {
                        r.cursor_position = start;
                    }
                    handlers::clear_selection(r);
                    r.caret.reset(handlers::sdl_ticks());
                    r.needs_redraw = true;
                    return false;
                }
                let buf_len = if r.coordinate == Coordinate::ExtendedSearch {
                    r.input_buffer.len()
                } else {
                    r.search_string.len()
                };
                if r.cursor_position > 0 {
                    let buf = if r.coordinate == Coordinate::ExtendedSearch {
                        &r.input_buffer
                    } else {
                        &r.search_string
                    };
                    let before = &buf[..r.cursor_position.min(buf_len)];
                    if let Some((i, ch)) = before.char_indices().rev().next() {
                        r.cursor_position = i;
                        handlers::announce_char(r, ch);
                    } else {
                        r.cursor_position = 0;
                    }
                    r.caret.reset(handlers::sdl_ticks());
                    r.needs_redraw = true;
                } else if handlers::navigate_left_raw(r) {
                    // Cursor at start — navigate up in tree.
                    if r.coordinate == Coordinate::ExtendedSearch {
                        r.input_buffer.clear();
                        r.cursor_position = 0;
                        list::create_list_extended_search(r);
                    } else {
                        r.search_string.clear();
                        r.cursor_position = 0;
                        list::create_list_current_layer(r);
                    }
                    r.list_index = r.current_id.last().unwrap_or(0)
                        .min(r.active_list_len().saturating_sub(1));
                    r.needs_redraw = true;
                }
            }
            Some(Keycode::Right) if !ctrl && !shift => {
                // If selection is active: collapse to selection end and clear.
                if handlers::has_selection(r) {
                    if let Some((_, end)) = handlers::selection_range(r) {
                        r.cursor_position = end;
                    }
                    handlers::clear_selection(r);
                    r.caret.reset(handlers::sdl_ticks());
                    r.needs_redraw = true;
                    return false;
                }
                let buf = if r.coordinate == Coordinate::ExtendedSearch {
                    r.input_buffer.clone()
                } else {
                    r.search_string.clone()
                };
                if r.cursor_position < buf.len() {
                    let ch = buf[r.cursor_position..].chars().next().unwrap();
                    r.cursor_position += ch.len_utf8();
                    handlers::announce_char(r, ch);
                    r.caret.reset(handlers::sdl_ticks());
                    r.needs_redraw = true;
                } else if r.coordinate == Coordinate::ExtendedSearch {
                    // Cursor at end in extended search — navigate into selected item.
                    if let Some(item) = r.current_list_item().cloned() {
                        if let Some(ref nav_path) = item.nav_path {
                            let root_idx = item.id.get(0).unwrap_or(0);
                            let (parent_dir, filename) = handlers::split_nav_path(nav_path);
                            crate::provider::navigate_to_path(r, root_idx, parent_dir, filename);
                        } else {
                            r.current_id = item.id;
                        }
                        if handlers::navigate_right_raw(r) {
                            r.input_buffer.clear();
                            r.cursor_position = 0;
                            list::create_list_extended_search(r);
                            r.list_index = r.current_id.last().unwrap_or(0)
                                .min(r.active_list_len().saturating_sub(1));
                            r.scroll_offset = r.list_index as i32;
                            r.needs_redraw = true;
                        }
                    }
                } else {
                    // Cursor at end in simple search — navigate into selected item.
                    r.search_string.clear();
                    r.cursor_position = 0;
                    if let Some(item_id) = r.current_list_item_id() {
                        r.current_id = item_id;
                    }
                    if handlers::navigate_right_raw(r) {
                        list::create_list_current_layer(r);
                        r.list_index = r.current_id.last().unwrap_or(0)
                            .min(r.active_list_len().saturating_sub(1));
                        r.scroll_offset = r.list_index as i32;
                        r.needs_redraw = true;
                    }
                }
            }
            Some(Keycode::Tab) => handlers::handle_tab(r),
            Some(Keycode::Return) | Some(Keycode::KpEnter) => handlers::handle_enter_search(r),
            Some(Keycode::A) if ctrl && !shift => handlers::handle_select_all(r),
            Some(Keycode::X) if ctrl && !shift => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift => handlers::handle_ctrl_v(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
            Some(Keycode::Z) if ctrl && !shift => handlers::handle_undo(r),
            Some(Keycode::Z) if ctrl && shift => handlers::handle_redo(r),
            Some(Keycode::Backspace) => handlers::handle_backspace(r),
            Some(Keycode::Delete) if !ctrl && !shift => handlers::handle_delete_forward(r),
            Some(Keycode::Escape) => handlers::handle_escape(r),
            _ => {}
        },

        // ---- Insert / normal / visual / operator-insert modes ---------------
        Coordinate::EditorInsert | Coordinate::EditorNormal
        | Coordinate::EditorVisual | Coordinate::OperatorInsert => match keycode {
            Some(Keycode::Escape) => handlers::handle_escape(r),
            Some(Keycode::Backspace) => handlers::handle_backspace(r),
            Some(Keycode::Delete) if !ctrl && !shift => handlers::handle_delete_forward(r),
            Some(Keycode::Up) if !ctrl && !shift => handlers::handle_up_insert(r),
            Some(Keycode::Down) if !ctrl && !shift => handlers::handle_down_insert(r),
            Some(Keycode::Up) if shift => handlers::handle_shift_up_insert(r),
            Some(Keycode::Down) if shift => handlers::handle_shift_down_insert(r),
            Some(Keycode::Left) if shift => handlers::handle_shift_left(r),
            Some(Keycode::Right) if shift => handlers::handle_shift_right(r),
            Some(Keycode::Home) if shift => handlers::handle_shift_home(r),
            Some(Keycode::End) if shift => handlers::handle_shift_end(r),
            Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
            Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
            Some(Keycode::Left) if !ctrl && !shift => {
                if handlers::has_selection(r) {
                    if let Some((start, _)) = handlers::selection_range(r) {
                        r.cursor_position = start;
                    }
                    handlers::clear_selection(r);
                    r.caret.reset(handlers::sdl_ticks());
                    r.needs_redraw = true;
                } else if r.cursor_position > 0 {
                    let before = &r.input_buffer[..r.cursor_position];
                    if let Some((i, ch)) = before.char_indices().rev().next() {
                        r.cursor_position = i;
                        handlers::announce_char(r, ch);
                    } else {
                        r.cursor_position = 0;
                    }
                    r.caret.reset(handlers::sdl_ticks());
                    r.needs_redraw = true;
                }
            }
            Some(Keycode::Right) if !ctrl && !shift => {
                if handlers::has_selection(r) {
                    if let Some((_, end)) = handlers::selection_range(r) {
                        r.cursor_position = end;
                    }
                    handlers::clear_selection(r);
                    r.caret.reset(handlers::sdl_ticks());
                    r.needs_redraw = true;
                } else {
                    let pos = r.cursor_position;
                    if pos < r.input_buffer.len() {
                        let ch = r.input_buffer[pos..].chars().next().unwrap();
                        r.cursor_position = pos + ch.len_utf8();
                        handlers::announce_char(r, ch);
                        r.caret.reset(handlers::sdl_ticks());
                        r.needs_redraw = true;
                    }
                }
            }
            Some(Keycode::Return) | Some(Keycode::KpEnter)
                if ctrl && matches!(r.coordinate, Coordinate::EditorInsert | Coordinate::OperatorInsert) =>
            {
                handlers::handle_input(r, "\n");
            }
            Some(Keycode::Return) | Some(Keycode::KpEnter)
                if matches!(r.coordinate, Coordinate::EditorInsert | Coordinate::EditorNormal) =>
            {
                crate::state::update_state(r, Task::Input, History::None);
                handlers::handle_escape(r);
            }
            Some(Keycode::Return) | Some(Keycode::KpEnter)
                if r.coordinate == Coordinate::OperatorInsert =>
            {
                handlers::handle_enter_operator_insert(r);
            }
            Some(Keycode::A) if ctrl && shift && r.coordinate == Coordinate::EditorInsert => {
                handlers::handle_escape(r);
                handlers::handle_ctrl_a(r, History::None);
                handlers::handle_a(r);
            }
            Some(Keycode::I) if ctrl && shift && r.coordinate == Coordinate::EditorInsert => {
                handlers::handle_escape(r);
                handlers::handle_ctrl_i(r, History::None);
                handlers::handle_i(r);
            }
            Some(Keycode::A) if ctrl && !shift => handlers::handle_select_all(r),
            Some(Keycode::X) if ctrl && !shift => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift => handlers::handle_ctrl_v(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
            Some(Keycode::Z) if ctrl && !shift => handlers::handle_undo(r),
            Some(Keycode::Z) if ctrl && shift => handlers::handle_redo(r),
            _ => {}
        },

        // ---- Command mode ---------------------------------------------------
        Coordinate::Command => match keycode {
            Some(Keycode::Up) | Some(Keycode::K) if !ctrl && !shift => handlers::handle_up(r),
            Some(Keycode::Down) | Some(Keycode::J) if !ctrl && !shift => handlers::handle_down(r),
            Some(Keycode::PageUp) if !ctrl && !shift => handlers::handle_page_up(r),
            Some(Keycode::PageDown) if !ctrl && !shift => handlers::handle_page_down(r),
            Some(Keycode::Escape) => handlers::handle_escape(r),
            Some(Keycode::Backspace) => handlers::handle_backspace(r),
            Some(Keycode::Delete) if !ctrl && !shift => handlers::handle_delete_forward(r),
            Some(Keycode::Home) if ctrl => handlers::handle_ctrl_home(r),
            Some(Keycode::End) if ctrl => handlers::handle_ctrl_end(r),
            Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
            Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
            Some(Keycode::Home) if shift => handlers::handle_shift_home(r),
            Some(Keycode::End) if shift => handlers::handle_shift_end(r),
            Some(Keycode::Left) if shift => handlers::handle_shift_left(r),
            Some(Keycode::Right) if shift => handlers::handle_shift_right(r),
            Some(Keycode::Left) if !ctrl && !shift => {
                if handlers::has_selection(r) {
                    if let Some((start, _)) = handlers::selection_range(r) {
                        r.cursor_position = start;
                    }
                    handlers::clear_selection(r);
                    r.caret.reset(handlers::sdl_ticks());
                    r.needs_redraw = true;
                } else if r.cursor_position > 0 {
                    let before = &r.input_buffer[..r.cursor_position];
                    if let Some((i, ch)) = before.char_indices().rev().next() {
                        r.cursor_position = i;
                        handlers::announce_char(r, ch);
                    } else {
                        r.cursor_position = 0;
                    }
                    r.caret.reset(handlers::sdl_ticks());
                    r.needs_redraw = true;
                }
                // cursor == 0: no-op (matches C)
            }
            Some(Keycode::Right) if !ctrl && !shift => {
                if handlers::has_selection(r) {
                    if let Some((_, end)) = handlers::selection_range(r) {
                        r.cursor_position = end;
                    }
                    handlers::clear_selection(r);
                    r.caret.reset(handlers::sdl_ticks());
                    r.needs_redraw = true;
                } else {
                    let pos = r.cursor_position;
                    if pos < r.input_buffer.len() {
                        let ch = r.input_buffer[pos..].chars().next().unwrap();
                        r.cursor_position = pos + ch.len_utf8();
                        handlers::announce_char(r, ch);
                        r.caret.reset(handlers::sdl_ticks());
                        r.needs_redraw = true;
                    } else {
                        // Cursor at end — attempt tree navigation right (mirrors C).
                        handlers::handle_right(r);
                    }
                }
            }
            Some(Keycode::Return) | Some(Keycode::KpEnter) => {
                handlers::handle_enter_command(r);
            }
            Some(Keycode::A) if ctrl && !shift => handlers::handle_select_all(r),
            Some(Keycode::X) if ctrl && !shift => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift => handlers::handle_ctrl_v(r),
            Some(Keycode::Z) if ctrl && !shift => handlers::handle_undo(r),
            Some(Keycode::Z) if ctrl && shift => handlers::handle_redo(r),
            _ => {}
        },

        // ---- Scroll / scroll-search / input-search modes --------------------
        Coordinate::Scroll | Coordinate::ScrollSearch | Coordinate::InputSearch => match keycode {
            Some(Keycode::Escape) => handlers::handle_escape(r),
            Some(Keycode::Up) | Some(Keycode::K) if !ctrl && !shift => {
                if matches!(r.coordinate, Coordinate::InputSearch) {
                    r.text_scroll_offset = (r.text_scroll_offset - 1).max(0);
                    r.needs_redraw = true;
                } else {
                    handlers::handle_up(r);
                }
            }
            Some(Keycode::Down) | Some(Keycode::J) if !ctrl && !shift => {
                if matches!(r.coordinate, Coordinate::InputSearch) {
                    r.text_scroll_offset += 1;
                    r.needs_redraw = true;
                } else {
                    handlers::handle_down(r);
                }
            }
            Some(Keycode::PageUp) if !ctrl && !shift => handlers::handle_page_up(r),
            Some(Keycode::PageDown) if !ctrl && !shift => handlers::handle_page_down(r),
            Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
            Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
            Some(Keycode::Z) if ctrl && !shift => handlers::handle_undo(r),
            Some(Keycode::Z) if ctrl && shift => handlers::handle_redo(r),
            Some(Keycode::Backspace) if matches!(r.coordinate,
                Coordinate::ScrollSearch | Coordinate::InputSearch) =>
            {
                handlers::handle_backspace(r);
            }
            Some(Keycode::Delete) if matches!(r.coordinate,
                Coordinate::ScrollSearch | Coordinate::InputSearch) =>
            {
                handlers::handle_delete_forward(r);
            }
            _ => {}
        },

        // ---- Meta mode -------------------------------------------------------
        Coordinate::Meta => match keycode {
            Some(Keycode::Escape) => handlers::handle_escape(r),
            Some(Keycode::Up) | Some(Keycode::K) if !ctrl && !shift => handlers::handle_up(r),
            Some(Keycode::Down) | Some(Keycode::J) if !ctrl && !shift => handlers::handle_down(r),
            _ => {}
        },

        // ---- Dashboard mode ---------------------------------------------------
        Coordinate::Dashboard => match keycode {
            Some(Keycode::Escape) => handlers::handle_escape(r),
            _ => {}
        },

    }

    false
}
