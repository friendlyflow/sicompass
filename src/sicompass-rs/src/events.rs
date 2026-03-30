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
//! | Return        | —        | Command                            | (execute command, TODO)    |
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
//! | Escape        | —        | OperatorGeneral                    | quit                       |
//! | Escape        | —        | all other modes                    | handle_escape              |
//! | Q             | —        | OperatorGeneral                    | quit                       |
//! | Shift+;       | —        | navigation modes                   | handle_colon               |

use crate::app_state::{AppRenderer, Coordinate, History, Task};
use crate::handlers;
use sdl3::keyboard::{Keycode, Mod};

/// Dispatch a key event to the appropriate handler based on the current mode.
///
/// Returns `true` if the application should quit (Escape/Q in OperatorGeneral),
/// `false` otherwise.  The caller is responsible for acting on the quit signal.
///
/// This function is the central key dispatcher — used by the main event loop
/// (via `view::handle_keydown`) and directly by the integration test harness.
pub fn dispatch_key(r: &mut AppRenderer, keycode: Option<Keycode>, keymod: Mod) -> bool {
    let ctrl  = keymod.intersects(Mod::LCTRLMOD  | Mod::RCTRLMOD);
    let shift = keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);

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
            Some(Keycode::Semicolon) if shift => handlers::handle_colon(r),
            Some(Keycode::Return) | Some(Keycode::KpEnter) if !ctrl => {
                handlers::handle_enter_operator(r);
            }
            Some(Keycode::I) if !ctrl && !shift => handlers::handle_i(r),
            Some(Keycode::A) if !ctrl && !shift => handlers::handle_a(r),
            Some(Keycode::A) if ctrl && !shift => handlers::handle_ctrl_a_operator(r),
            Some(Keycode::I) if ctrl && !shift => handlers::handle_ctrl_i_operator(r),
            Some(Keycode::D) if ctrl && !shift => handlers::handle_file_delete(r),
            Some(Keycode::Delete) if !ctrl && !shift => handlers::handle_file_delete(r),
            Some(Keycode::M) if !ctrl && !shift => handlers::handle_meta(r),
            Some(Keycode::Space) if !ctrl && !shift => handlers::handle_space(r),
            Some(Keycode::Z) if ctrl && !shift => handlers::handle_undo(r),
            Some(Keycode::Z) if ctrl && shift => handlers::handle_redo(r),
            Some(Keycode::X) if ctrl && !shift => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift => handlers::handle_ctrl_v(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
            Some(Keycode::F5) => handlers::handle_f5(r),
            Some(Keycode::Backspace) => handlers::handle_backspace(r),
            Some(Keycode::Escape) | Some(Keycode::Q) => return true,
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
            Some(Keycode::Semicolon) if shift => handlers::handle_colon(r),
            Some(Keycode::Return) | Some(Keycode::KpEnter) if !ctrl => handlers::handle_append(r),
            Some(Keycode::I) if !ctrl && !shift => handlers::handle_i(r),
            Some(Keycode::A) if !ctrl && !shift => handlers::handle_a(r),
            Some(Keycode::A) if ctrl && !shift => handlers::handle_append(r),
            Some(Keycode::I) if ctrl && !shift => handlers::handle_insert(r),
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

        // ---- Simple search --------------------------------------------------
        Coordinate::SimpleSearch => match keycode {
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
                if r.cursor_position > 0 {
                    let before = &r.search_string[..r.cursor_position.min(r.search_string.len())];
                    r.cursor_position = before.char_indices().rev().next().map(|(i,_)| i).unwrap_or(0);
                    r.needs_redraw = true;
                }
            }
            Some(Keycode::Right) if !ctrl && !shift => {
                let pos = r.cursor_position;
                let slen = r.search_string.len();
                if pos < slen {
                    let ch = r.search_string[pos..].chars().next().unwrap();
                    r.cursor_position = pos + ch.len_utf8();
                    r.needs_redraw = true;
                }
            }
            Some(Keycode::Tab) => handlers::handle_tab(r),
            Some(Keycode::Return) | Some(Keycode::KpEnter) => handlers::handle_enter_search(r),
            Some(Keycode::A) if ctrl && !shift => handlers::handle_select_all(r),
            Some(Keycode::X) if ctrl && !shift => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift => handlers::handle_ctrl_v(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
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
                if r.cursor_position > 0 {
                    let before = &r.input_buffer[..r.cursor_position];
                    r.cursor_position = before.char_indices().rev()
                        .next().map(|(i, _)| i).unwrap_or(0);
                    r.needs_redraw = true;
                }
            }
            Some(Keycode::Right) if !ctrl && !shift => {
                let pos = r.cursor_position;
                if pos < r.input_buffer.len() {
                    let ch = r.input_buffer[pos..].chars().next().unwrap();
                    r.cursor_position = pos + ch.len_utf8();
                    r.needs_redraw = true;
                }
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
            Some(Keycode::A) if ctrl && !shift => handlers::handle_select_all(r),
            Some(Keycode::X) if ctrl && !shift => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift => handlers::handle_ctrl_v(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
            _ => {}
        },

        // ---- Command mode ---------------------------------------------------
        Coordinate::Command => match keycode {
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
                if r.cursor_position > 0 {
                    let before = &r.input_buffer[..r.cursor_position];
                    r.cursor_position = before.char_indices().rev()
                        .next().map(|(i, _)| i).unwrap_or(0);
                    r.needs_redraw = true;
                }
            }
            Some(Keycode::Right) if !ctrl && !shift => {
                let pos = r.cursor_position;
                if pos < r.input_buffer.len() {
                    let ch = r.input_buffer[pos..].chars().next().unwrap();
                    r.cursor_position = pos + ch.len_utf8();
                    r.needs_redraw = true;
                }
            }
            Some(Keycode::Return) | Some(Keycode::KpEnter) => {
                handlers::handle_escape(r);
            }
            Some(Keycode::A) if ctrl && !shift => handlers::handle_select_all(r),
            Some(Keycode::X) if ctrl && !shift => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift => handlers::handle_ctrl_v(r),
            _ => {}
        },

        // ---- Scroll / scroll-search / input-search modes --------------------
        Coordinate::Scroll | Coordinate::ScrollSearch | Coordinate::InputSearch => match keycode {
            Some(Keycode::Escape) => handlers::handle_escape(r),
            Some(Keycode::Up) | Some(Keycode::K) if !ctrl && !shift => {
                r.text_scroll_offset = (r.text_scroll_offset - 1).max(0);
                r.needs_redraw = true;
            }
            Some(Keycode::Down) | Some(Keycode::J) if !ctrl && !shift => {
                r.text_scroll_offset += 1;
                r.needs_redraw = true;
            }
            Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
            Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
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

        _ => {}
    }

    false
}
