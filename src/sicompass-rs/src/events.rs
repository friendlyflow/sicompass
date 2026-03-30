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
