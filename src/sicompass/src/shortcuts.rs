//! Central shortcut table — single source of truth for key dispatch and hint display.
//!
//! Each [`Shortcut`] row carries:
//! - which key (+ optional alias, ctrl flag, shift flag) triggers the action,
//! - which [`Coordinate`] modes the shortcut is active in,
//! - a display label (empty → dispatch-only, not shown in the M hint screen),
//! - an `is_available` predicate that must also pass, and
//! - the handler function to call.
//!
//! [`dispatch_key`] iterates the table and calls the first matching handler.
//! [`hints_for`] iterates the table and returns formatted strings for all entries
//! whose `label` is non-empty and whose `is_available` passes in the current state.
//!
//! This unifies three previously disconnected systems:
//! - `Provider::meta()` / `list_actions()` (hint declarations),
//! - the per-mode `match keycode` dispatcher in `events.rs`, and
//! - ad-hoc `provider_allows_shortcut` guards inside handler functions.

use crate::app_state::{AppRenderer, Coordinate};
use crate::handlers;
use sdl3::keyboard::{Keycode, Mod};
use sicompass_sdk::ffon::get_ffon_at_id;
use sicompass_sdk::tags;

// ---------------------------------------------------------------------------
// Mode group constants
// ---------------------------------------------------------------------------

const GENERAL: &[Coordinate] = &[Coordinate::General];

const INSERT: &[Coordinate] = &[
    Coordinate::Insert,
    Coordinate::Normal,
    Coordinate::Visual,
];

const SEARCH: &[Coordinate] = &[Coordinate::SimpleSearch, Coordinate::ExtendedSearch];

const TEXT: &[Coordinate] = &[
    Coordinate::Insert,
    Coordinate::Normal,
    Coordinate::Visual,
    Coordinate::SimpleSearch,
    Coordinate::ExtendedSearch,
    Coordinate::Command,
];

// Modes where Up/Down navigate the list (not text cursor movement)
const NAV_UP_DOWN: &[Coordinate] = &[
    Coordinate::General,
    Coordinate::SimpleSearch,
    Coordinate::ExtendedSearch,
    Coordinate::Command,
    Coordinate::Scroll,
    Coordinate::ScrollSearch,
    Coordinate::Meta,
];

// Modes where Undo/Redo are active
const UNDO_MODES_ALL: &[Coordinate] = &[
    Coordinate::General,
    Coordinate::SimpleSearch,
    Coordinate::ExtendedSearch,
    Coordinate::Insert,
    Coordinate::Normal,
    Coordinate::Visual,
    Coordinate::Command,
    Coordinate::Scroll,
    Coordinate::ScrollSearch,
];

// ---------------------------------------------------------------------------
// Shortcut struct
// ---------------------------------------------------------------------------

/// One row in the SHORTCUTS table.
pub struct Shortcut {
    /// Primary Keycode.
    pub key: Keycode,
    /// Optional alias (e.g. `KpEnter` for `Return`, `K` for `Up`).
    pub key2: Option<Keycode>,
    /// True if Ctrl must be held.
    pub ctrl: bool,
    /// True if Shift must be held.
    pub shift: bool,
    /// Modes in which this shortcut is active.
    pub modes: &'static [Coordinate],
    /// Display label for the M hint screen. Empty string → dispatch-only.
    pub label: &'static str,
    /// Extra availability predicate (mode + ctrl + shift + modes already checked).
    pub is_available: fn(&AppRenderer) -> bool,
    /// Handler to invoke.
    pub handle: fn(&mut AppRenderer),
}

// ---------------------------------------------------------------------------
// Predicates
// ---------------------------------------------------------------------------

fn always(_: &AppRenderer) -> bool { true }

fn not_at_root(r: &AppRenderer) -> bool {
    r.current_id.depth() > 1
}

fn at_root(r: &AppRenderer) -> bool {
    r.current_id.depth() <= 1
}

/// True when the focused element is an Obj with children (can navigate into with Right).
fn focused_has_children(r: &AppRenderer) -> bool {
    use sicompass_sdk::ffon::FfonElement;
    let Some(slice) = get_ffon_at_id(&r.ffon, &r.current_id) else { return false };
    let idx = r.current_id.last().unwrap_or(0);
    matches!(slice.get(idx), Some(FfonElement::Obj(o)) if !o.children.is_empty())
}

fn not_at_root_and_no_input_children(r: &AppRenderer) -> bool {
    not_at_root(r) && !children_have_input(r)
}

fn active_provider_name(r: &AppRenderer) -> Option<&str> {
    r.current_id.get(0)
        .and_then(|i| r.providers.get(i))
        .map(|p| p.name())
}

fn is_filebrowser(r: &AppRenderer) -> bool {
    active_provider_name(r) == Some("filebrowser")
}

fn is_editor(r: &AppRenderer) -> bool {
    r.current_id.get(0)
        .and_then(|i| r.providers.get(i))
        .map(|p| p.has_editor_semantics())
        .unwrap_or(false)
}

fn has_dashboard(r: &AppRenderer) -> bool {
    r.current_id.get(0)
        .and_then(|i| r.providers.get(i))
        .and_then(|p| p.dashboard_image_path())
        .is_some()
}

fn supports_config_files(r: &AppRenderer) -> bool {
    r.current_id.get(0)
        .and_then(|i| r.providers.get(i))
        .map(|p| p.supports_config_files())
        .unwrap_or(false)
}

/// Show config-file hints only when inside a provider (not at root provider list).
fn supports_config_files_hint(r: &AppRenderer) -> bool {
    not_at_root(r) && supports_config_files(r)
}

/// True when the active provider's command list includes `"delete"`.
/// Used to gate Ctrl+D / Delete key so they only act as message-delete in the
/// email client and remain inert (or fall through to other handlers) elsewhere.
fn avail_provider_has_delete(r: &AppRenderer) -> bool {
    crate::provider::get_commands(r).iter().any(|c| c == "delete")
}

/// True when the focused container's children contain an `<input>` element.
fn children_have_input(r: &AppRenderer) -> bool {
    let Some(slice) = get_ffon_at_id(&r.ffon, &r.current_id) else { return false };
    slice.iter().any(|elem| {
        let key = match elem {
            sicompass_sdk::ffon::FfonElement::Str(s) => s.as_str(),
            sicompass_sdk::ffon::FfonElement::Obj(o) => o.key.as_str(),
        };
        tags::has_input(key)
    })
}

/// True when the focused element is a button.
fn focused_is_button(r: &AppRenderer) -> bool {
    let Some(slice) = get_ffon_at_id(&r.ffon, &r.current_id) else { return false };
    let idx = r.current_id.last().unwrap_or(0);
    slice.get(idx).is_some_and(|e| {
        let k = match e {
            sicompass_sdk::ffon::FfonElement::Str(s) => s.as_str(),
            sicompass_sdk::ffon::FfonElement::Obj(o) => o.key.as_str(),
        };
        tags::has_button(k)
    })
}

/// True when the focused element is a hyperlink.
fn focused_is_link(r: &AppRenderer) -> bool {
    let Some(slice) = get_ffon_at_id(&r.ffon, &r.current_id) else { return false };
    let idx = r.current_id.last().unwrap_or(0);
    slice.get(idx).is_some_and(|e| {
        let k = match e {
            sicompass_sdk::ffon::FfonElement::Str(s) => s.as_str(),
            sicompass_sdk::ffon::FfonElement::Obj(o) => o.key.as_str(),
        };
        tags::has_link(k)
    })
}

/// True when the focused element itself carries an `<input>` or `<input-all>` tag.
fn focused_is_input(r: &AppRenderer) -> bool {
    let Some(slice) = get_ffon_at_id(&r.ffon, &r.current_id) else { return false };
    let idx = r.current_id.last().unwrap_or(0);
    slice.get(idx).is_some_and(|e| {
        let k = match e {
            sicompass_sdk::ffon::FfonElement::Str(s) => s.as_str(),
            sicompass_sdk::ffon::FfonElement::Obj(o) => o.key.as_str(),
        };
        tags::has_input(k)
    })
}

/// I/A (generic): not at root and the focused element is an `<input>`.
fn avail_insert_on_input(r: &AppRenderer) -> bool {
    not_at_root(r) && focused_is_input(r)
}

/// A (filebrowser / editor): mirror of `avail_i_edit_hint` so rename via A shows the hint.
fn avail_a_edit_hint(r: &AppRenderer) -> bool {
    not_at_root(r) && (is_filebrowser(r) || is_editor(r))
}

/// True when we're navigated inside an email compose / reply / forward body section.
fn in_email_compose_body(r: &AppRenderer) -> bool {
    crate::provider::is_in_email_compose_body(r)
}

/// True when we're navigated inside any email compose / reply / forward context
/// (header fields OR body).
fn in_email_compose(r: &AppRenderer) -> bool {
    let path = crate::provider::current_path(r);
    let compose_roots = ["compose", "reply", "reply all", "forward"];
    let has_compose = path.trim_matches('/').split('/').any(|s| compose_roots.contains(&s));
    has_compose && not_at_root(r)
}

/// True when the current parent container has an "Add element:" sibling
/// (createElement provider pattern).
fn has_add_element_sibling(r: &AppRenderer) -> bool {
    use sicompass_sdk::ffon::FfonElement;
    let siblings: &[FfonElement] = if r.current_id.depth() <= 1 {
        &r.ffon
    } else {
        let Some(s) = get_ffon_at_id(&r.ffon, &r.current_id) else { return false };
        s
    };
    siblings.iter().any(|e| matches!(e, FfonElement::Obj(o) if o.key == "Add element:"))
}

/// Structural editing is available for filebrowser, email compose body,
/// or createElement providers. The editor provider has dedicated rows
/// (avail_editor_edit) — keeping it out of this predicate stops the
/// generic operator-style handlers from racing the editor-specific ones
/// now that operator/editor coordinates are unified.
fn avail_structural_edit(r: &AppRenderer) -> bool {
    not_at_root(r) && (is_filebrowser(r) || in_email_compose_body(r) || has_add_element_sibling(r))
}

/// FFON-tree delete is only meaningful in compose body and createElement
/// providers — filebrowser/editor route Ctrl+D to file/disk delete.
fn avail_ffon_delete(r: &AppRenderer) -> bool {
    not_at_root(r) && (in_email_compose_body(r) || has_add_element_sibling(r))
}

/// Predicates for keys that are operator-only after the coordinate collapse —
/// the editor provider opts out (no scroll-mode / meta hint screen).
fn avail_scroll_key(r: &AppRenderer) -> bool {
    not_at_root(r) && !is_editor(r)
}

fn avail_meta_key(r: &AppRenderer) -> bool {
    !is_editor(r)
}

/// Structural edit available in email compose body (General).
fn avail_compose_body_edit(r: &AppRenderer) -> bool {
    not_at_root(r) && in_email_compose_body(r)
}

/// File-level delete (General Delete / Ctrl+D) — filebrowser or editor provider.
fn avail_file_delete(r: &AppRenderer) -> bool {
    not_at_root(r) && (is_filebrowser(r) || is_editor(r))
}

/// File clipboard (Ctrl+X/C/V) in General — only for filebrowser.
fn avail_file_clipboard(r: &AppRenderer) -> bool {
    not_at_root(r) && is_filebrowser(r)
}

/// Enter "Activate" — focused item is a button.
fn avail_enter_activate(r: &AppRenderer) -> bool {
    not_at_root(r) && focused_is_button(r)
}

/// Enter "Follow link" — focused item is a link.
fn avail_enter_follow_link(r: &AppRenderer) -> bool {
    not_at_root(r) && focused_is_link(r)
}

/// Enter in General: not at root, not a link/button hint
fn avail_enter_op(r: &AppRenderer) -> bool {
    not_at_root(r)
}

/// I key (edit/enter insert) visible as "Edit" for filebrowser, invisible for others.
fn avail_i_edit_hint(r: &AppRenderer) -> bool {
    not_at_root(r) && is_filebrowser(r)
}

fn avail_editor_edit(r: &AppRenderer) -> bool {
    not_at_root(r) && is_editor(r)
}


// ---------------------------------------------------------------------------
// Handler wrappers (for History param or mode-specific disambiguation)
// ---------------------------------------------------------------------------

fn delete_editor(r: &mut AppRenderer) {
    handlers::handle_delete(r, crate::app_state::History::None);
}

// ---------------------------------------------------------------------------
// SHORTCUTS table
// ---------------------------------------------------------------------------

pub static SHORTCUTS: &[Shortcut] = &[

    // ---- Escape (all meaningful modes) -----------------------------------
    // Hint only inside providers; dispatch fires everywhere.
    Shortcut { key: Keycode::Escape, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General,
                 Coordinate::Insert, Coordinate::Normal,
                 Coordinate::Visual,
                 Coordinate::SimpleSearch, Coordinate::ExtendedSearch,
                 Coordinate::Command, Coordinate::Scroll, Coordinate::ScrollSearch,
                 Coordinate::InputSearch, Coordinate::Meta, Coordinate::Dashboard,
                 Coordinate::DashboardInteractive],
        label: "Esc    Back", is_available: not_at_root, handle: handlers::handle_escape },
    Shortcut { key: Keycode::Escape, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General,
                 Coordinate::Insert, Coordinate::Normal,
                 Coordinate::Visual,
                 Coordinate::SimpleSearch, Coordinate::ExtendedSearch,
                 Coordinate::Command, Coordinate::Scroll, Coordinate::ScrollSearch,
                 Coordinate::InputSearch, Coordinate::Meta, Coordinate::Dashboard,
                 Coordinate::DashboardInteractive],
        label: "", is_available: always, handle: handlers::handle_escape },

    // ---- Up / K ----------------------------------------------------------
    Shortcut { key: Keycode::Up, key2: Some(Keycode::K), ctrl: false, shift: false,
        modes: NAV_UP_DOWN,
        label: "Up     Previous", is_available: always, handle: handlers::handle_up },
    Shortcut { key: Keycode::Up, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::InputSearch],
        label: "Up     Scroll up", is_available: always, handle: handlers::handle_input_search_up },
    Shortcut { key: Keycode::Up, key2: None, ctrl: false, shift: false,
        modes: INSERT,
        label: "Up     Previous", is_available: always, handle: handlers::handle_up_insert },
    Shortcut { key: Keycode::Up, key2: None, ctrl: false, shift: true,
        modes: INSERT,
        label: "Shift+Up Select up", is_available: always, handle: handlers::handle_shift_up_insert },

    // ---- Down / J --------------------------------------------------------
    Shortcut { key: Keycode::Down, key2: Some(Keycode::J), ctrl: false, shift: false,
        modes: NAV_UP_DOWN,
        label: "Down   Next", is_available: always, handle: handlers::handle_down },
    Shortcut { key: Keycode::Down, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::InputSearch],
        label: "Down   Scroll dn", is_available: always, handle: handlers::handle_input_search_down },
    Shortcut { key: Keycode::Down, key2: None, ctrl: false, shift: false,
        modes: INSERT,
        label: "Down   Next", is_available: always, handle: handlers::handle_down_insert },
    Shortcut { key: Keycode::Down, key2: None, ctrl: false, shift: true,
        modes: INSERT,
        label: "Shift+Down Select dn", is_available: always, handle: handlers::handle_shift_down_insert },

    // ---- Right / L -------------------------------------------------------
    Shortcut { key: Keycode::Right, key2: Some(Keycode::L), ctrl: false, shift: false,
        modes: GENERAL,
        label: "Right  Open", is_available: focused_has_children, handle: handlers::handle_right },
    Shortcut { key: Keycode::Right, key2: Some(Keycode::L), ctrl: false, shift: false,
        modes: GENERAL,
        label: "", is_available: always, handle: handlers::handle_right },
    Shortcut { key: Keycode::Right, key2: None, ctrl: false, shift: false,
        modes: SEARCH,
        label: "Right  Navigate", is_available: always, handle: handlers::handle_search_right },
    Shortcut { key: Keycode::Right, key2: None, ctrl: false, shift: false,
        modes: INSERT,
        label: "Right  Cursor right", is_available: always, handle: handlers::handle_text_cursor_right },
    Shortcut { key: Keycode::Right, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::Command],
        label: "Right  Cursor right", is_available: always, handle: handlers::handle_command_right },
    Shortcut { key: Keycode::Right, key2: None, ctrl: false, shift: true,
        modes: TEXT,
        label: "Shift+Right Select right", is_available: always, handle: handlers::handle_shift_right },

    // ---- Left / H --------------------------------------------------------
    Shortcut { key: Keycode::Left, key2: Some(Keycode::H), ctrl: false, shift: false,
        modes: GENERAL,
        label: "Left   Back", is_available: not_at_root, handle: handlers::handle_left },
    Shortcut { key: Keycode::Left, key2: Some(Keycode::H), ctrl: false, shift: false,
        modes: GENERAL,
        label: "", is_available: always, handle: handlers::handle_left },
    Shortcut { key: Keycode::Left, key2: None, ctrl: false, shift: false,
        modes: SEARCH,
        label: "Left   Navigate", is_available: always, handle: handlers::handle_search_left },
    Shortcut { key: Keycode::Left, key2: None, ctrl: false, shift: false,
        modes: INSERT,
        label: "Left   Cursor left", is_available: always, handle: handlers::handle_text_cursor_left },
    Shortcut { key: Keycode::Left, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::Command],
        label: "Left   Cursor left", is_available: always, handle: handlers::handle_text_cursor_left },
    Shortcut { key: Keycode::Left, key2: None, ctrl: false, shift: true,
        modes: TEXT,
        label: "Shift+Left Select left", is_available: always, handle: handlers::handle_shift_left },

    // ---- PageUp / PageDown -----------------------------------------------
    // Hint only inside providers; dispatch fires everywhere.
    Shortcut { key: Keycode::PageUp, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General,
                 Coordinate::SimpleSearch, Coordinate::ExtendedSearch,
                 Coordinate::Command, Coordinate::Scroll, Coordinate::ScrollSearch,
                 Coordinate::InputSearch],
        label: "PgUp   Page up", is_available: not_at_root, handle: handlers::handle_page_up },
    Shortcut { key: Keycode::PageUp, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General,
                 Coordinate::SimpleSearch, Coordinate::ExtendedSearch,
                 Coordinate::Command, Coordinate::Scroll, Coordinate::ScrollSearch,
                 Coordinate::InputSearch],
        label: "", is_available: always, handle: handlers::handle_page_up },
    Shortcut { key: Keycode::PageDown, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General,
                 Coordinate::SimpleSearch, Coordinate::ExtendedSearch,
                 Coordinate::Command, Coordinate::Scroll, Coordinate::ScrollSearch,
                 Coordinate::InputSearch],
        label: "PgDn   Page dn", is_available: not_at_root, handle: handlers::handle_page_down },
    Shortcut { key: Keycode::PageDown, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General,
                 Coordinate::SimpleSearch, Coordinate::ExtendedSearch,
                 Coordinate::Command, Coordinate::Scroll, Coordinate::ScrollSearch,
                 Coordinate::InputSearch],
        label: "", is_available: always, handle: handlers::handle_page_down },

    // ---- Home / End (no modifier) ----------------------------------------
    // Hint only inside providers; dispatch fires everywhere.
    Shortcut { key: Keycode::Home, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General,
                 Coordinate::Insert, Coordinate::Normal,
                 Coordinate::Visual,
                 Coordinate::SimpleSearch, Coordinate::ExtendedSearch,
                 Coordinate::Command, Coordinate::Scroll, Coordinate::ScrollSearch,
                 Coordinate::InputSearch],
        label: "Home   First", is_available: not_at_root, handle: handlers::handle_home },
    Shortcut { key: Keycode::Home, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General,
                 Coordinate::Insert, Coordinate::Normal,
                 Coordinate::Visual,
                 Coordinate::SimpleSearch, Coordinate::ExtendedSearch,
                 Coordinate::Command, Coordinate::Scroll, Coordinate::ScrollSearch,
                 Coordinate::InputSearch],
        label: "", is_available: always, handle: handlers::handle_home },
    Shortcut { key: Keycode::End, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General,
                 Coordinate::Insert, Coordinate::Normal,
                 Coordinate::Visual,
                 Coordinate::SimpleSearch, Coordinate::ExtendedSearch,
                 Coordinate::Command, Coordinate::Scroll, Coordinate::ScrollSearch,
                 Coordinate::InputSearch],
        label: "End    Last", is_available: not_at_root, handle: handlers::handle_end },
    Shortcut { key: Keycode::End, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General,
                 Coordinate::Insert, Coordinate::Normal,
                 Coordinate::Visual,
                 Coordinate::SimpleSearch, Coordinate::ExtendedSearch,
                 Coordinate::Command, Coordinate::Scroll, Coordinate::ScrollSearch,
                 Coordinate::InputSearch],
        label: "", is_available: always, handle: handlers::handle_end },

    // ---- Home / End (Ctrl) -----------------------------------------------
    Shortcut { key: Keycode::Home, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::SimpleSearch, Coordinate::ExtendedSearch, Coordinate::Command],
        label: "Ctrl+Home Line start", is_available: always, handle: handlers::handle_ctrl_home },
    Shortcut { key: Keycode::End, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::SimpleSearch, Coordinate::ExtendedSearch, Coordinate::Command],
        label: "Ctrl+End  Line end", is_available: always, handle: handlers::handle_ctrl_end },

    // ---- Home / End (Shift) ---------------------------------------------
    Shortcut { key: Keycode::Home, key2: None, ctrl: false, shift: true,
        modes: TEXT,
        label: "Shift+Home Sel. start", is_available: always, handle: handlers::handle_shift_home },
    Shortcut { key: Keycode::End, key2: None, ctrl: false, shift: true,
        modes: TEXT,
        label: "Shift+End  Sel. end", is_available: always, handle: handlers::handle_shift_end },

    // ---- Tab -------------------------------------------------------------
    Shortcut { key: Keycode::Tab, key2: None, ctrl: false, shift: false,
        modes: GENERAL,
        label: "Tab    Search", is_available: always, handle: handlers::handle_tab },
    // Insert Tab
    Shortcut { key: Keycode::Tab, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::Insert],
        label: "Tab    Next field", is_available: always, handle: handlers::handle_tab },

    // ---- Backspace -------------------------------------------------------
    Shortcut { key: Keycode::Backspace, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "Bspc   Backspace", is_available: not_at_root, handle: handlers::handle_backspace },
    Shortcut { key: Keycode::Backspace, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General,
                 Coordinate::Insert, Coordinate::Normal,
                 Coordinate::Visual,
                 Coordinate::SimpleSearch, Coordinate::ExtendedSearch,
                 Coordinate::Command, Coordinate::ScrollSearch, Coordinate::InputSearch],
        label: "Bspc   Backspace", is_available: always, handle: handlers::handle_backspace },

    // ---- Delete (forward) ------------------------------------------------
    Shortcut { key: Keycode::Delete, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::Insert, Coordinate::Normal,
                 Coordinate::Visual,
                 Coordinate::SimpleSearch, Coordinate::ExtendedSearch,
                 Coordinate::Command, Coordinate::ScrollSearch, Coordinate::InputSearch],
        label: "Del    Delete fwd", is_available: always, handle: handlers::handle_delete_forward },

    // ---- Return ----------------------------------------------------------
    // General: Enter → activate element
    Shortcut { key: Keycode::Return, key2: Some(Keycode::KpEnter), ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "Enter  Follow link", is_available: avail_enter_follow_link,
        handle: handlers::handle_enter_general },
    Shortcut { key: Keycode::Return, key2: Some(Keycode::KpEnter), ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "Enter  Activate", is_available: avail_enter_activate,
        handle: handlers::handle_enter_general },
    Shortcut { key: Keycode::Return, key2: Some(Keycode::KpEnter), ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "Enter  Open", is_available: avail_enter_op,
        handle: handlers::handle_enter_general },
    // General: Enter → append
    Shortcut { key: Keycode::Return, key2: Some(Keycode::KpEnter), ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "Enter  Append", is_available: always, handle: handlers::handle_append },
    // Search: Enter → commit search
    Shortcut { key: Keycode::Return, key2: Some(Keycode::KpEnter), ctrl: false, shift: false,
        modes: SEARCH,
        label: "Enter  Confirm", is_available: always, handle: handlers::handle_enter_search },
    // Command: Enter → execute command
    Shortcut { key: Keycode::Return, key2: Some(Keycode::KpEnter), ctrl: false, shift: false,
        modes: &[Coordinate::Command],
        label: "Enter  Execute", is_available: always, handle: handlers::handle_enter_command },
    // Insert modes: Ctrl+Return → newline
    Shortcut { key: Keycode::Return, key2: Some(Keycode::KpEnter), ctrl: true, shift: false,
        modes: &[Coordinate::Insert],
        label: "Ctrl+Enter Newline", is_available: always, handle: handlers::handle_ctrl_enter_insert },
    // Normal: Return → commit + escape (no disk write)
    Shortcut { key: Keycode::Return, key2: Some(Keycode::KpEnter), ctrl: false, shift: false,
        modes: &[Coordinate::Normal],
        label: "Enter  Confirm", is_available: always, handle: handlers::handle_return_in_normal },
    // Insert / Insert: Return → commit edit (disk write via provider)
    Shortcut { key: Keycode::Return, key2: Some(Keycode::KpEnter), ctrl: false, shift: false,
        modes: &[Coordinate::Insert],
        label: "Enter  Confirm", is_available: always, handle: handlers::handle_enter_insert },

    // ---- Colon / Semicolon+Shift (command mode entry) --------------------
    Shortcut { key: Keycode::Semicolon, key2: None, ctrl: false, shift: true,
        modes: &[Coordinate::General],
        label: ":      Command", is_available: always, handle: handlers::handle_colon },
    Shortcut { key: Keycode::Colon, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: ":      Command", is_available: always, handle: handlers::handle_colon },

    // ---- I / A (enter insert/append mode) --------------------------------
    // Editor provider: i/a enter Insert so commit_edit fires on Enter → writes to disk.
    // These rows must precede the generic avail_insert_on_input rows to take priority.
    Shortcut { key: Keycode::I, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "I      Edit", is_available: avail_editor_edit, handle: handlers::handle_editor_provider_i },
    Shortcut { key: Keycode::A, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "A      Append", is_available: avail_editor_edit, handle: handlers::handle_editor_provider_a },
    // Generic i/a for filebrowser and other providers with <input> elements.
    Shortcut { key: Keycode::I, key2: None, ctrl: false, shift: false,
        modes: GENERAL,
        label: "I      Edit input", is_available: avail_i_edit_hint, handle: handlers::handle_i },
    Shortcut { key: Keycode::I, key2: None, ctrl: false, shift: false,
        modes: GENERAL,
        label: "I      Edit input", is_available: avail_insert_on_input, handle: handlers::handle_i },
    Shortcut { key: Keycode::A, key2: None, ctrl: false, shift: false,
        modes: GENERAL,
        label: "A      Append", is_available: avail_a_edit_hint, handle: handlers::handle_a },
    Shortcut { key: Keycode::A, key2: None, ctrl: false, shift: false,
        modes: GENERAL,
        label: "A      Append", is_available: avail_insert_on_input, handle: handlers::handle_a },

    // ---- Ctrl+I / Ctrl+A (structural insert/append) ----------------------
    // General: insert/append placeholder
    Shortcut { key: Keycode::I, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+I Insert before", is_available: avail_structural_edit,
        handle: handlers::handle_ctrl_i_general },
    Shortcut { key: Keycode::A, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+A Insert after", is_available: avail_structural_edit,
        handle: handlers::handle_ctrl_a_general },
    // General, editor provider: custom insert/append (file-line or dir-placeholder).
    // Must precede the generic ctrl_i_editor / ctrl_a_editor rows.
    Shortcut { key: Keycode::I, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+I Insert before", is_available: avail_editor_edit,
        handle: handlers::handle_editor_ctrl_i },
    Shortcut { key: Keycode::A, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+A Insert after", is_available: avail_editor_edit,
        handle: handlers::handle_editor_ctrl_a },
    // Insert: Ctrl+Shift+I/A — escape, insert/append, re-enter insert
    Shortcut { key: Keycode::I, key2: None, ctrl: true, shift: true,
        modes: &[Coordinate::Insert],
        label: "Ctrl+Shift+I Insert before", is_available: avail_structural_edit,
        handle: handlers::handle_ctrl_shift_i_insert },
    Shortcut { key: Keycode::A, key2: None, ctrl: true, shift: true,
        modes: &[Coordinate::Insert],
        label: "Ctrl+Shift+A Insert after", is_available: avail_structural_edit,
        handle: handlers::handle_ctrl_shift_a_insert },
    // Search/Insert/Command: Ctrl+A → select all
    Shortcut { key: Keycode::A, key2: None, ctrl: true, shift: false,
        modes: TEXT,
        label: "Ctrl+A Select all", is_available: always, handle: handlers::handle_select_all },

    // ---- D ---------------------------------------------------------------
    Shortcut { key: Keycode::D, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "D      Dashboard", is_available: has_dashboard,
        handle: handlers::handle_dashboard },
    Shortcut { key: Keycode::D, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "", is_available: always, handle: handlers::handle_dashboard },

    // ---- S (enter scroll mode) -------------------------------------------
    // Suppressed for editor providers (which manage their own buffer view).
    Shortcut { key: Keycode::S, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "S      Scroll", is_available: avail_scroll_key, handle: handlers::handle_s },

    // ---- M (enter meta/hint screen) --------------------------------------
    // Suppressed for editor providers (vim conventions reserve M for jumps).
    Shortcut { key: Keycode::M, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "M      Meta", is_available: avail_meta_key, handle: handlers::handle_meta },

    // ---- Ctrl+D / Delete key → provider delete command (email message delete) ----
    // These must come before the editor/filebrowser Ctrl+D rows so they win when
    // the active provider advertises "delete" (e.g. email client in folder/message view).
    Shortcut { key: Keycode::D, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+D Delete", is_available: avail_provider_has_delete,
        handle: handlers::invoke_provider_delete },
    Shortcut { key: Keycode::Delete, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "Del    Delete", is_available: avail_provider_has_delete,
        handle: handlers::invoke_provider_delete },

    // ---- Ctrl+D / Delete in General for editor provider -----------
    // Routes to handle_file_delete so the provider's delete_item is called (writes to disk).
    // Must precede the generic delete_editor rows so editor provider wins.
    Shortcut { key: Keycode::D, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+D Delete", is_available: avail_editor_edit,
        handle: handlers::handle_file_delete },
    Shortcut { key: Keycode::Delete, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "Del    Delete", is_available: avail_editor_edit,
        handle: handlers::handle_file_delete },

    // ---- Ctrl+D (delete FFON element in General) ------------------
    // Only fires for compose body / has_add_element_sibling providers.
    // Filebrowser/editor have dedicated file-delete rows above.
    Shortcut { key: Keycode::D, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+D Delete", is_available: avail_ffon_delete,
        handle: delete_editor },
    // General Ctrl+D → compose body element delete (before filebrowser entry)
    Shortcut { key: Keycode::D, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+D Delete", is_available: avail_compose_body_edit,
        handle: handlers::handle_delete_body_element },
    // General Ctrl+D → file delete
    Shortcut { key: Keycode::D, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+D Delete", is_available: avail_file_delete, handle: handlers::handle_file_delete },

    // ---- Delete key (file delete in General) --------------------
    // Compose body delete (before filebrowser entry)
    Shortcut { key: Keycode::Delete, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "Del    Delete", is_available: avail_compose_body_edit,
        handle: handlers::handle_delete_body_element },
    Shortcut { key: Keycode::Delete, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "Del    Delete", is_available: avail_file_delete,
        handle: handlers::handle_file_delete },

    // ---- Ctrl+X / C / V -------------------------------------------------
    // General: compose body clipboard (before filebrowser entries)
    Shortcut { key: Keycode::X, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+X Cut", is_available: avail_compose_body_edit,
        handle: handlers::handle_ctrl_x },
    Shortcut { key: Keycode::C, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+C Copy", is_available: avail_compose_body_edit,
        handle: handlers::handle_ctrl_c },
    Shortcut { key: Keycode::V, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+V Paste", is_available: avail_compose_body_edit,
        handle: handlers::handle_ctrl_v },
    // General: filebrowser file clipboard (show hint)
    Shortcut { key: Keycode::X, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+X Cut", is_available: avail_file_clipboard,
        handle: handlers::handle_ctrl_x },
    Shortcut { key: Keycode::C, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+C Copy", is_available: avail_file_clipboard,
        handle: handlers::handle_ctrl_c },
    Shortcut { key: Keycode::V, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+V Paste", is_available: avail_file_clipboard,
        handle: handlers::handle_ctrl_v },
    // General + text modes: clipboard (dispatch always, hint for structural contexts)
    Shortcut { key: Keycode::X, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+X Cut", is_available: avail_structural_edit,
        handle: handlers::handle_ctrl_x },
    Shortcut { key: Keycode::X, key2: None, ctrl: true, shift: false,
        modes: TEXT,
        label: "Ctrl+X Cut", is_available: always, handle: handlers::handle_ctrl_x },
    Shortcut { key: Keycode::C, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+C Copy", is_available: avail_structural_edit,
        handle: handlers::handle_ctrl_c },
    Shortcut { key: Keycode::C, key2: None, ctrl: true, shift: false,
        modes: TEXT,
        label: "Ctrl+C Copy", is_available: always, handle: handlers::handle_ctrl_c },
    Shortcut { key: Keycode::V, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+V Paste", is_available: avail_structural_edit,
        handle: handlers::handle_ctrl_v },
    Shortcut { key: Keycode::V, key2: None, ctrl: true, shift: false,
        modes: TEXT,
        label: "Ctrl+V Paste", is_available: always, handle: handlers::handle_ctrl_v },

    // ---- Ctrl+F (extended search) ----------------------------------------
    Shortcut { key: Keycode::F, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General,
                 Coordinate::Insert, Coordinate::Normal,
                 Coordinate::Visual,
                 Coordinate::SimpleSearch, Coordinate::ExtendedSearch,
                 Coordinate::Command, Coordinate::Scroll, Coordinate::ScrollSearch,
                 Coordinate::InputSearch],
        label: "Ctrl+F Extended search", is_available: always,
        handle: handlers::handle_ctrl_f },

    // ---- Ctrl+Z / Ctrl+Shift+Z (undo / redo) ----------------------------
    Shortcut { key: Keycode::Z, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+Z Undo", is_available: not_at_root, handle: handlers::handle_undo },
    Shortcut { key: Keycode::Z, key2: None, ctrl: true, shift: false,
        modes: UNDO_MODES_ALL,
        label: "Ctrl+Z Undo", is_available: always, handle: handlers::handle_undo },
    Shortcut { key: Keycode::Z, key2: None, ctrl: true, shift: true,
        modes: &[Coordinate::General],
        label: "Ctrl+Shift+Z Redo", is_available: not_at_root, handle: handlers::handle_redo },
    Shortcut { key: Keycode::Z, key2: None, ctrl: true, shift: true,
        modes: UNDO_MODES_ALL,
        label: "Ctrl+Shift+Z Redo", is_available: always, handle: handlers::handle_redo },

    // ---- F5 (refresh) ----------------------------------------------------
    Shortcut { key: Keycode::F5, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "F5     Refresh", is_available: always, handle: handlers::handle_f5 },
    Shortcut { key: Keycode::F5, key2: None, ctrl: false, shift: false,
        modes: &[Coordinate::General],
        label: "F5     Refresh", is_available: always, handle: handlers::handle_f5 },

    // ---- Ctrl+S / Ctrl+Shift+S / Ctrl+O (config file ops) ---------------
    // Hints only inside providers; dispatch fires anywhere the provider supports it.
    Shortcut { key: Keycode::S, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+S Save", is_available: supports_config_files_hint,
        handle: handlers::handle_save_provider_config },
    Shortcut { key: Keycode::S, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "", is_available: supports_config_files,
        handle: handlers::handle_save_provider_config },
    Shortcut { key: Keycode::S, key2: None, ctrl: true, shift: true,
        modes: &[Coordinate::General],
        label: "Ctrl+Shift+S Save as", is_available: supports_config_files_hint,
        handle: handlers::handle_save_as_provider_config },
    Shortcut { key: Keycode::S, key2: None, ctrl: true, shift: true,
        modes: &[Coordinate::General],
        label: "", is_available: supports_config_files,
        handle: handlers::handle_save_as_provider_config },
    Shortcut { key: Keycode::O, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "Ctrl+O Open", is_available: supports_config_files_hint,
        handle: handlers::handle_file_browser_open },
    Shortcut { key: Keycode::O, key2: None, ctrl: true, shift: false,
        modes: &[Coordinate::General],
        label: "", is_available: supports_config_files,
        handle: handlers::handle_file_browser_open },

];

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Dispatch a key event using the SHORTCUTS table.
///
/// Returns `true` if the application should quit (same semantics as `events::dispatch_key`).
pub fn dispatch_key(r: &mut AppRenderer, keycode: Option<Keycode>, keymod: Mod) -> bool {
    let Some(k) = keycode else { return false };
    let ctrl  = keymod.intersects(Mod::LCTRLMOD  | Mod::RCTRLMOD);
    let shift = keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
    let alt   = keymod.intersects(Mod::LALTMOD   | Mod::RALTMOD);

    // Interactive-dashboard fast-path: forward every key *except* Escape to the
    // active provider. Escape still falls through to the SHORTCUTS table so the
    // user can always bail out via `handle_escape`.
    if r.coordinate == Coordinate::DashboardInteractive && k != Keycode::Escape {
        if let Some(keysym) = sdl_keycode_to_dashboard_keysym(k) {
            let key = sicompass_sdk::DashboardKey { keysym, ctrl, shift, alt };
            if let Some(p) = crate::provider::get_active_provider(r) {
                if p.dashboard_key(key) {
                    r.needs_redraw = true;
                }
            }
        }
        return false;
    }

    // During the file-browser-open dialog restrict General/General
    // to navigation + selection only (same semantics as the original pre-filter).
    if r.pending_file_browser_open {
        match r.coordinate {
            Coordinate::General => {
                const ALLOWED: &[Keycode] = &[
                    Keycode::Up, Keycode::K, Keycode::Down, Keycode::J,
                    Keycode::Right, Keycode::L, Keycode::Left, Keycode::H,
                    Keycode::PageUp, Keycode::PageDown, Keycode::Home, Keycode::End,
                    Keycode::Return, Keycode::KpEnter, Keycode::Escape,
                    Keycode::Backspace, Keycode::Tab, Keycode::F,
                ];
                if !ALLOWED.contains(&k) { return false; }
                // Ctrl+F allowed; other ctrl combos blocked.
                if ctrl && k != Keycode::F { return false; }
            }
            Coordinate::SimpleSearch | Coordinate::ExtendedSearch => {
                if ctrl && matches!(k, Keycode::X | Keycode::V | Keycode::Z) {
                    return false;
                }
            }
            _ => {}
        }
    }

    for s in SHORTCUTS {
        if s.ctrl != ctrl || s.shift != shift { continue; }
        if s.key != k && s.key2 != Some(k) { continue; }
        if !s.modes.contains(&r.coordinate) { continue; }
        if !(s.is_available)(r) { continue; }
        (s.handle)(r);
        return false;
    }
    false
}

// ---------------------------------------------------------------------------
// Hints
// ---------------------------------------------------------------------------

/// Return formatted hint strings for all shortcuts whose label is non-empty
/// and whose `is_available` predicate passes in the current state.
///
/// Used by `get_meta` to populate the M-key hint screen.
pub fn hints_for(r: &AppRenderer) -> Vec<String> {
    // When showing the Meta screen, hints are derived from the coordinate we
    // came from (previous_coordinate), not from Coordinate::Meta itself.
    let coord = if r.coordinate == Coordinate::Meta {
        r.previous_coordinate
    } else {
        r.coordinate
    };

    // Collect entries: skip dispatch-only (empty label) and deduplicate by label.
    let mut seen_labels = std::collections::HashSet::new();
    SHORTCUTS.iter()
        .filter(|s| {
            !s.label.is_empty()
                && s.modes.contains(&coord)
                && (s.is_available)(r)
                && seen_labels.insert(s.label)
        })
        .map(|s| s.label.to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// SDL → DashboardKeysym mapping (interactive dashboard input)
// ---------------------------------------------------------------------------

/// Translate an SDL keycode into the SDK-side keysym a provider sees in
/// interactive-dashboard mode. Returns `None` for keys we don't want to
/// surface (modifier keys, lock keys, …).
fn sdl_keycode_to_dashboard_keysym(k: Keycode) -> Option<sicompass_sdk::DashboardKeysym> {
    use sicompass_sdk::DashboardKeysym as K;
    let s = match k {
        Keycode::Return | Keycode::KpEnter => K::Enter,
        Keycode::Backspace => K::Backspace,
        Keycode::Tab => K::Tab,
        Keycode::Up => K::Up,
        Keycode::Down => K::Down,
        Keycode::Left => K::Left,
        Keycode::Right => K::Right,
        Keycode::Home => K::Home,
        Keycode::End => K::End,
        Keycode::PageUp => K::PageUp,
        Keycode::PageDown => K::PageDown,
        Keycode::Insert => K::Insert,
        Keycode::Delete => K::Delete,
        Keycode::F1 => K::F(1),
        Keycode::F2 => K::F(2),
        Keycode::F3 => K::F(3),
        Keycode::F4 => K::F(4),
        Keycode::F5 => K::F(5),
        Keycode::F6 => K::F(6),
        Keycode::F7 => K::F(7),
        Keycode::F8 => K::F(8),
        Keycode::F9 => K::F(9),
        Keycode::F10 => K::F(10),
        Keycode::F11 => K::F(11),
        Keycode::F12 => K::F(12),
        Keycode::A => K::Char('a'), Keycode::B => K::Char('b'), Keycode::C => K::Char('c'),
        Keycode::D => K::Char('d'), Keycode::E => K::Char('e'), Keycode::F => K::Char('f'),
        Keycode::G => K::Char('g'), Keycode::H => K::Char('h'), Keycode::I => K::Char('i'),
        Keycode::J => K::Char('j'), Keycode::K => K::Char('k'), Keycode::L => K::Char('l'),
        Keycode::M => K::Char('m'), Keycode::N => K::Char('n'), Keycode::O => K::Char('o'),
        Keycode::P => K::Char('p'), Keycode::Q => K::Char('q'), Keycode::R => K::Char('r'),
        Keycode::S => K::Char('s'), Keycode::T => K::Char('t'), Keycode::U => K::Char('u'),
        Keycode::V => K::Char('v'), Keycode::W => K::Char('w'), Keycode::X => K::Char('x'),
        Keycode::Y => K::Char('y'), Keycode::Z => K::Char('z'),
        Keycode::_0 => K::Char('0'), Keycode::_1 => K::Char('1'), Keycode::_2 => K::Char('2'),
        Keycode::_3 => K::Char('3'), Keycode::_4 => K::Char('4'), Keycode::_5 => K::Char('5'),
        Keycode::_6 => K::Char('6'), Keycode::_7 => K::Char('7'), Keycode::_8 => K::Char('8'),
        Keycode::_9 => K::Char('9'),
        Keycode::Space => K::Char(' '),
        _ => return Some(K::Unknown),
    };
    Some(s)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::AppRenderer;

    fn no_mod() -> Mod { Mod::empty() }
    fn ctrl()   -> Mod { Mod::LCTRLMOD }
    fn shift()  -> Mod { Mod::LSHIFTMOD }

    // --- dispatch correctness ---

    #[test]
    fn tab_in_general_switches_to_simple_search() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::Tab), no_mod());
        assert_eq!(r.coordinate, Coordinate::SimpleSearch);
    }

    #[test]
    fn ctrl_f_in_general_switches_to_extended_search() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::F), ctrl());
        assert_eq!(r.coordinate, Coordinate::ExtendedSearch);
    }

    #[test]
    fn up_key_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::Up), no_mod());
        assert!(r.needs_redraw);
    }

    #[test]
    fn k_sets_needs_redraw() {
        let mut r = AppRenderer::new();
        dispatch_key(&mut r, Some(Keycode::K), no_mod());
        assert!(r.needs_redraw);
    }

    #[test]
    fn escape_in_insert_returns_to_general() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::Insert;
        dispatch_key(&mut r, Some(Keycode::Escape), no_mod());
        assert_eq!(r.coordinate, Coordinate::General);
    }

    // --- hints ---

    #[test]
    fn hints_for_at_root_contains_search_and_ctrl_f() {
        let r = AppRenderer::new();
        let hints = hints_for(&r);
        assert!(hints.iter().any(|h| h.contains("Search")));
        assert!(hints.iter().any(|h| h.contains("Ctrl+F")));
    }

    #[test]
    fn hints_for_no_duplicates() {
        let r = AppRenderer::new();
        let hints = hints_for(&r);
        let mut seen = std::collections::HashSet::new();
        for h in &hints {
            assert!(seen.insert(h.clone()), "duplicate hint: {h}");
        }
    }

    #[test]
    fn hints_for_in_insert_mode_has_navigation_and_editing() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::Insert;
        // Simulate being inside a provider (Insert always happens at depth > 1).
        r.current_id.push(0);
        r.current_id.push(0);
        let hints = hints_for(&r);
        // Insert mode should expose navigation + editing hints.
        assert!(hints.iter().any(|h| h.contains("Ctrl+F")), "Ctrl+F missing, got: {hints:?}");
        assert!(hints.iter().any(|h| h.contains("Up")),     "Up missing, got: {hints:?}");
        assert!(hints.iter().any(|h| h.contains("Esc")),    "Esc missing, got: {hints:?}");
    }
}
