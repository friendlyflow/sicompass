//! Key event routing — public entry point for SDL key dispatch.
//!
//! The authoritative key-binding table lives in [`crate::shortcuts::SHORTCUTS`].
//! This module exposes [`dispatch_key`], a thin tracing wrapper around
//! [`crate::shortcuts::dispatch_key`], used by `view::handle_keydown` and the
//! integration test suite.

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
    fn tab_in_general_switches_to_simple_search() {
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
    fn colon_in_general_switches_to_command() {
        let mut r = make_renderer_inside_provider();
        dispatch_key(&mut r, Some(Keycode::Semicolon), shift());
        assert_eq!(r.coordinate, Coordinate::Command);
    }

    #[test]
    fn colon_blocked_at_root() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::Semicolon), shift());
        assert_eq!(r.coordinate, Coordinate::General);
    }

    #[test]
    fn colon_blocked_in_insert() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::Insert;
        dispatch_key(&mut r, Some(Keycode::Semicolon), shift());
        assert_eq!(r.coordinate, Coordinate::Insert);
    }

    // --- Ctrl+F → SimpleSearch (from General) ---

    #[test]
    fn ctrl_f_in_general_switches_to_extended_search() {
        // C spec: Ctrl+F from General enters ExtendedSearch (not SimpleSearch)
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::F), ctrl());
        assert_eq!(r.coordinate, Coordinate::ExtendedSearch);
    }

    // --- Escape ---

    #[test]
    fn escape_in_insert_returns_to_general() {
        // C spec: Insert → updateState(Input) → General
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::Insert;
        dispatch_key(&mut r, Some(Keycode::Escape), no_mod());
        assert_eq!(r.coordinate, Coordinate::General);
    }

    #[test]
    fn dispatch_key_returns_false() {
        let mut r = AppRenderer::new();
        assert!(!dispatch_key(&mut r, Some(Keycode::Escape), no_mod()));
        assert!(!dispatch_key(&mut r, Some(Keycode::Q), no_mod()));
        assert!(!dispatch_key(&mut r, Some(Keycode::Tab), no_mod()));
    }

    // --- Ctrl+A select all in Insert ---

    #[test]
    fn ctrl_a_in_insert_selects_all() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::Insert;
        r.input_buffer = "hello".to_string();
        r.cursor_position = 0;
        dispatch_key(&mut r, Some(Keycode::A), ctrl());
        assert_eq!(r.selection_anchor, Some(0));
        assert_eq!(r.cursor_position, 5);
    }

    // --- Shift+Left in Insert ---

    #[test]
    fn shift_left_in_insert_starts_selection() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::Insert;
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

    // --- Ctrl+I in General → handle_insert ---

    #[test]
    fn ctrl_i_in_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::General;
        dispatch_key(&mut r, Some(Keycode::I), ctrl());
        assert!(r.needs_redraw);
    }

    // --- Ctrl+D in General → no crash ---

    #[test]
    fn ctrl_d_in_general_no_crash() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::D), ctrl());
        // handle_file_delete with no providers — no crash
    }

    // --- Delete key in General → no crash ---

    #[test]
    fn delete_key_in_general_no_crash() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::Delete), no_mod());
    }

    // --- K/J/Up in navigation modes set needs_redraw ---

    #[test]
    fn k_in_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::K), no_mod());
        assert!(r.needs_redraw);
    }

    #[test]
    fn j_in_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::General;
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
    fn ctrl_z_in_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::General;
        dispatch_key(&mut r, Some(Keycode::Z), ctrl());
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_shift_z_in_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::General;
        dispatch_key(&mut r, Some(Keycode::Z), ctrl_shift());
        assert!(r.needs_redraw);
    }

    // --- Ctrl+A in General → stays General ---

    #[test]
    fn ctrl_a_in_general_stays_general() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::A), ctrl());
        assert_eq!(r.coordinate, Coordinate::General);
    }

    // --- Ctrl+A in General → handle_append → sets needs_redraw ---

    #[test]
    fn ctrl_a_in_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::General;
        dispatch_key(&mut r, Some(Keycode::A), ctrl());
        assert!(r.needs_redraw);
    }

    // --- Return in General → handle_append → sets needs_redraw ---

    #[test]
    fn enter_in_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::Return), no_mod());
        assert!(r.needs_redraw);
    }

    // --- Ctrl+I in General → no crash ---

    #[test]
    fn ctrl_i_in_general_no_crash() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::I), ctrl());
        // empty ffon — no crash
    }

    // --- Ctrl+X/C/V in General → sets needs_redraw ---

    #[test]
    fn ctrl_x_in_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::General;
        dispatch_key(&mut r, Some(Keycode::X), ctrl());
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_c_in_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::General;
        dispatch_key(&mut r, Some(Keycode::C), ctrl());
        assert!(r.needs_redraw);
    }

    #[test]
    fn ctrl_v_in_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::General;
        dispatch_key(&mut r, Some(Keycode::V), ctrl());
        assert!(r.needs_redraw);
    }

    // --- H moves left / L moves right ---

    #[test]
    fn h_moves_left_in_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::H), no_mod());
        assert!(r.needs_redraw);
    }

    #[test]
    fn l_moves_right_in_general_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::General;
        dispatch_key(&mut r, Some(Keycode::L), no_mod());
        assert!(r.needs_redraw);
    }

    // --- D in General → handle_dashboard ---

    #[test]
    fn d_in_general_no_dashboard_provider_stays_general() {
        let mut r = AppRenderer::new();
        // No provider — handle_dashboard returns early, but dispatch routes to it
        dispatch_key(&mut r, Some(Keycode::D), no_mod());
        // Coordinate stays General when no provider has a dashboard image
        assert_eq!(r.coordinate, Coordinate::General);
    }

    // --- Ctrl+Enter in Insert → insert newline ---

    #[test]
    fn ctrl_enter_in_insert_inserts_newline() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::Insert;
        r.input_buffer = "hello".to_string();
        r.cursor_position = 5;
        dispatch_key(&mut r, Some(Keycode::Return), ctrl());
        assert!(r.input_buffer.contains('\n'));
        assert!(r.needs_redraw);
    }
}

/// Dispatch a key event to the appropriate handler based on the current mode.
///
/// Returns `true` if the application should quit (Escape/Q in General),
/// `false` otherwise.  The caller is responsible for acting on the quit signal.
///
/// Delegates to [`crate::shortcuts::dispatch_key`] which iterates the central
/// SHORTCUTS table — one source of truth for both dispatch and hint display.
///
/// After dispatch, refresh the active tab's snapshot from the live navigation
/// state so the tab band and persisted config follow root-level program changes
/// and within-program navigation — not just explicit tab operations. Persistence
/// is skipped when the handler itself changed `active_tab` / `tabs.len()` (tab
/// switch/new/close handlers already call `persist_tabs` via `after_tab_change`).
pub fn dispatch_key(r: &mut AppRenderer, keycode: Option<Keycode>, keymod: Mod) -> bool {
    tracing::debug!(
        ?keycode, ?keymod,
        mode = r.coordinate.as_str(),
        "dispatch_key"
    );

    let prior_active = r.active_tab;
    let prior_len = r.tabs.len();

    let result = crate::shortcuts::dispatch_key(r, keycode, keymod);

    let new_snap = crate::app_state::TabSnapshot::capture(r);
    let snap_changed = r.tabs.get(r.active_tab).map_or(false, |s| s != &new_snap);
    if let Some(slot) = r.tabs.get_mut(r.active_tab) {
        *slot = new_snap;
    }
    let tab_structure_changed = r.active_tab != prior_active || r.tabs.len() != prior_len;
    if snap_changed && !tab_structure_changed {
        crate::handlers::persist_tabs(r);
    }

    result
}
