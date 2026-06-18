//! `AppState` — owns the SDL3 window, the entire Vulkan context, and the
//! application-level renderer state (`AppRenderer`).
//!
//! Equivalent to `SiCompassApplication` + `AppRenderer` in the C code.

use crate::render;
use crate::view;
use sicompass_sdk::ffon::{FfonElement, IdArray};
use sicompass_sdk::provider::Provider;
use sicompass_sdk::timeline::TimelineEntry;
use std::fmt;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const MAX_FRAMES_IN_FLIGHT: usize = 2;
pub const WINDOW_TITLE: &str = "sicompass";
pub const WINDOW_WIDTH: u32 = 800;
pub const WINDOW_HEIGHT: u32 = 600;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum SiError {
    Sdl(String),
    Vulkan(ash::vk::Result),
    VulkanLoad(ash::LoadingError),
    Other(String),
}

impl fmt::Display for SiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SiError::Sdl(s) => write!(f, "SDL: {s}"),
            SiError::Vulkan(r) => write!(f, "Vulkan: {r}"),
            SiError::VulkanLoad(e) => write!(f, "Vulkan load: {e}"),
            SiError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl From<ash::vk::Result> for SiError {
    fn from(r: ash::vk::Result) -> Self {
        SiError::Vulkan(r)
    }
}

impl From<ash::LoadingError> for SiError {
    fn from(e: ash::LoadingError) -> Self {
        SiError::VulkanLoad(e)
    }
}

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

/// Theme selection. Mirrors C `colorScheme` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaletteTheme {
    #[default]
    Dark,
    Light,
}

/// RGBA color values for all rendered elements (packed as 0xRRGGBBAA).
///
/// Mirrors C `ColorPalette` struct.
#[derive(Debug, Clone, Copy)]
pub struct ColorPalette {
    pub background:      u32,
    pub text:            u32,
    pub header_sep:      u32,
    pub selected:        u32,
    pub ext_search:      u32,
    pub scroll_search:   u32,
    pub error:           u32,
}

pub const PALETTE_DARK: ColorPalette = ColorPalette {
    background:    0x000000FF,
    text:          0xFFFFFFFF,
    header_sep:    0x333333FF,
    selected:      0x2D4A28FF,
    ext_search:    0x696969FF,
    scroll_search: 0x264F78FF,
    error:         0xFF0000FF,
};

pub const PALETTE_LIGHT: ColorPalette = ColorPalette {
    background:    0xFFFFFFFF,
    text:          0x000000FF,
    header_sep:    0xE0E0E0FF,
    selected:      0xC0ECB8FF,
    ext_search:    0x333333FF,
    scroll_search: 0xA8C7FAFF,
    error:         0xFF0000FF,
};

// ---------------------------------------------------------------------------

/// Navigation / edit mode. Vim-style modes (general/insert/normal/visual)
/// apply to every provider; operator vs editor behavior is decided by
/// `Provider::has_editor_semantics()`, not by the variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Coordinate {
    #[default]
    General,
    Insert,
    Normal,
    Visual,
    SimpleSearch,
    ExtendedSearch,
    Command,
    Scroll,
    ScrollSearch,
    ScrollPrefixSearch,
    InputSearch,
    Dashboard,
    Meta,
    TimelineView,
    /// Modal yes/no list shown when Ctrl+W would close a tab whose providers
    /// are still busy (e.g. a terminal running a foreground command). The list
    /// holds two `<button>` options; Enter activates the highlighted one.
    ConfirmCloseTab,
}

impl Coordinate {
    pub fn is_general(self) -> bool {
        matches!(self, Coordinate::General)
    }

    /// Stable English identifier — used in logs (`tracing::debug!`) and
    /// internal tracing where translation would hurt grep-ability. For
    /// user-facing UI (screen-reader announcements, header status line,
    /// window title) use [`Coordinate::display_label`] instead.
    pub fn as_str(self) -> &'static str {
        match self {
            Coordinate::General => "general mode",
            Coordinate::Insert => "insert mode",
            Coordinate::Normal => "normal mode",
            Coordinate::Visual => "visual mode",
            Coordinate::SimpleSearch => "search",
            Coordinate::ExtendedSearch => "extended search",
            Coordinate::Command => "command",
            Coordinate::Scroll => "scroll mode",
            Coordinate::ScrollSearch => "scroll search",
            Coordinate::ScrollPrefixSearch => "scroll prefix search",
            Coordinate::InputSearch => "input search",
            Coordinate::Dashboard => "dashboard",
            Coordinate::Meta => "meta",
            Coordinate::TimelineView => "timeline",
            Coordinate::ConfirmCloseTab => "confirm close tab",
        }
    }

    /// Localized mode name for user-facing surfaces (screen reader, header
    /// status line, window title). Resolves through the SDK localizer with
    /// `mode-<id>` keys; falls back to the English `as_str()` literal if no
    /// translation is registered.
    pub fn display_label(self) -> String {
        crate::shortcuts::register_translations();
        let key = match self {
            Coordinate::General => "mode-general",
            Coordinate::Insert => "mode-insert",
            Coordinate::Normal => "mode-normal",
            Coordinate::Visual => "mode-visual",
            Coordinate::SimpleSearch => "mode-search",
            Coordinate::ExtendedSearch => "mode-extended-search",
            Coordinate::Command => "mode-command",
            Coordinate::Scroll => "mode-scroll",
            Coordinate::ScrollSearch => "mode-scroll-search",
            Coordinate::ScrollPrefixSearch => "mode-scroll-prefix-search",
            Coordinate::InputSearch => "mode-input-search",
            Coordinate::Dashboard => "mode-dashboard",
            Coordinate::Meta => "mode-meta",
            Coordinate::TimelineView => "mode-timeline",
            Coordinate::ConfirmCloseTab => "mode-confirm-close-tab",
        };
        let resolved = sicompass_sdk::localize::t(key);
        if resolved == key { self.as_str().to_owned() } else { resolved }
    }
}

/// Pending edit task — mirrors the C `Task` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Task {
    #[default]
    None,
    Input,
    Append,
    AppendAppend,
    Insert,
    InsertInsert,
    Delete,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Cut,
    Copy,
    Paste,
}

impl Task {
    pub fn as_str(self) -> &'static str {
        match self {
            Task::None => "none",
            Task::Input => "input",
            Task::Append => "append",
            Task::AppendAppend => "append append",
            Task::Insert => "insert",
            Task::InsertInsert => "insert insert",
            Task::Delete => "delete",
            Task::ArrowUp => "up",
            Task::ArrowDown => "down",
            Task::ArrowLeft => "left",
            Task::ArrowRight => "right",
            Task::Cut => "cut",
            Task::Copy => "copy",
            Task::Paste => "paste",
        }
    }
}

/// Undo/redo direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum History {
    #[default]
    None,
    Undo,
    Redo,
}

/// Two-phase command execution state — mirrors C `currentCommand`.
///
/// `None` → showing the command list.
/// `Provider` → showing the secondary selection list for `provider_command_name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommandPhase {
    #[default]
    None,
    Provider,
}

// ---------------------------------------------------------------------------
// AppRenderer supporting types
// ---------------------------------------------------------------------------

/// A single item in the right-panel list.
#[derive(Debug, Clone)]
pub struct RenderListItem {
    /// Navigation path to this element in the FFON tree.
    pub id: IdArray,
    /// Display label including prefix (e.g. `"- item"`, `"+ Section"`).
    pub label: String,
    /// Breadcrumb text for extended search results.
    pub data: Option<String>,
    /// Non-None for deep-search results: the provider-level navigation path.
    pub nav_path: Option<String>,
    /// Extended prefix (`layer: X list: Y/Z`) for scroll mode; `None` elsewhere.
    pub ext_prefix: Option<String>,
}

// ---------------------------------------------------------------------------
// Timeline (unified undo/redo, per-tab)
// ---------------------------------------------------------------------------

/// Per-tab undo/redo timeline.
///
/// Coalescing rules live in `state::record_entry`:
/// - `TimelineEntry::TextChunk` entries on the same `id` within
///   `TEXT_CHUNK_IDLE_MS` collapse into the tail entry.
/// - `TimelineEntry::Navigate` entries collapse with the immediately
///   preceding `Navigate` so a burst of arrow keys is one undo step.
#[derive(Debug, Default, Clone)]
pub struct Timeline {
    pub entries: Vec<TimelineEntry>,
    /// 0 = at HEAD. Walking back increments; walking forward decrements.
    pub position: usize,
    pub last_text_edit_at: Option<Instant>,
    pub last_text_id: Option<IdArray>,
    pub next_chunk_seq: u32,
    /// Set by `walk_back` / `walk_forward` to prevent the next `record_entry`
    /// from coalescing the next user action with the entry that ended up at
    /// the new HEAD. Without this flag, undoing and then pressing another
    /// arrow key would silently extend the pre-undo Navigate entry instead
    /// of recording a new branch.
    pub coalesce_break: bool,
}

impl Timeline {
    pub fn new() -> Self {
        Self::default()
    }
}

pub const TIMELINE_CAPACITY: usize = 1024;
pub const TEXT_CHUNK_IDLE_MS: u64 = 500;

// ---------------------------------------------------------------------------
// PlaceholderCancel
// ---------------------------------------------------------------------------

/// Stashed state for cancelling an in-progress placeholder insertion via Escape.
///
/// Set by `insert_placeholder_typed`, `insert_general_placeholder`, and the
/// insert path in `handle_enter_command`. Consumed (and cleared) by
/// `handle_escape` when the user presses Escape during `Insert`.
#[derive(Clone)]
pub struct PlaceholderCancel {
    /// Full navigation id of the inserted (or replaced) placeholder element.
    pub insertion_id: IdArray,
    /// The original element that was replaced in place, if any.
    /// `None` means a fresh slot was inserted and should be removed on cancel.
    pub replaced_element: Option<FfonElement>,
    /// The `current_id` to restore after undoing the insertion.
    pub return_id: IdArray,
}

// ---------------------------------------------------------------------------
// InsertSession
// ---------------------------------------------------------------------------

/// State captured when entering insert mode on an `<input>` tag, used to
/// drive per-keystroke FFON mutation + TextChunk recording during typing.
///
/// Each buffer-mutating handler in Insert mode (handle_input, handle_backspace,
/// handle_delete_in_insert, paste, etc.) calls `apply_insert_session_chunk` to:
/// 1. Rewrite the current FFON element's key to wrap `input_buffer` in
///    `<input>...</input>` (preserving prefix/suffix).
/// 2. Record a TextChunk timeline entry. The merge logic in
///    `state::record_entry` (TEXT_CHUNK_IDLE_MS) collapses consecutive
///    keystrokes within 500 ms into a single entry; an idle gap > 500 ms
///    starts a fresh chunk so ctrl-Z reverts one typing-burst at a time.
///
/// `handle_escape` restores `original_element` and truncates the timeline
/// back to `timeline_position_at_start`, fully discarding the edit session.
#[derive(Debug, Clone)]
pub struct InsertSession {
    /// FFON element snapshot taken at insert-mode entry, before any
    /// per-keystroke mutation. Restored on Escape.
    pub original_element: FfonElement,
    /// FFON id of the element being edited.
    pub original_id: IdArray,
    /// `active_timeline().entries.len()` at insert-mode entry. On Escape,
    /// the timeline is truncated back to this length to drop per-keystroke
    /// TextChunks recorded during the abandoned edit.
    pub timeline_position_at_start: usize,
}

// ---------------------------------------------------------------------------
// AppRenderer
// ---------------------------------------------------------------------------

/// Application-level renderer / state. Equivalent to the C `AppRenderer`.
///
/// Owns FFON data, providers, navigation state, list state, input buffer,
/// undo history, and all UI flags.
pub struct AppRenderer {
    // ---- FFON data & providers ---------------------------------------------
    /// Root element per provider (Obj with key=displayName, children=fetched).
    pub ffon: Vec<FfonElement>,
    /// Providers in parallel with `ffon`.
    pub providers: Vec<Box<dyn Provider>>,

    // ---- Navigation state --------------------------------------------------
    /// Current navigation path (depth=1 means at root, depth≥2 inside a provider).
    pub current_id: IdArray,
    /// Previous path (restored on back navigation).
    pub previous_id: IdArray,
    /// Path used when entering insert/append mode.
    pub current_insert_id: IdArray,
    /// Current editing/navigation mode.
    pub coordinate: Coordinate,
    /// Previous mode (restored on Escape).
    pub previous_coordinate: Coordinate,

    // ---- List state --------------------------------------------------------
    pub total_list: Vec<RenderListItem>,
    /// Indices into `total_list` matching the current search string.
    /// Empty = no filter active, use `total_list` directly.
    pub filtered_list_indices: Vec<usize>,
    /// Per-filtered-item matched character positions (parallel to `filtered_list_indices`).
    /// Each inner Vec contains the character indices within the label that matched.
    pub fuzzy_match_positions: Vec<Vec<u32>>,
    /// Currently selected item in the active list.
    pub list_index: usize,

    // ---- Input editing state -----------------------------------------------
    /// Text being edited (UTF-8).
    pub input_buffer: String,
    /// Byte offset of the cursor within `input_buffer`.
    pub cursor_position: usize,
    /// Byte offset of the selection anchor, or `None` if no selection.
    pub selection_anchor: Option<usize>,
    /// Non-editable text displayed before the input widget.
    pub input_prefix: String,
    /// Non-editable text displayed after the input widget.
    pub input_suffix: String,

    /// Active per-keystroke edit session on an `<input>` tag (see
    /// [`InsertSession`]). `Some` from insert-mode entry until Enter or Escape.
    pub insert_session: Option<InsertSession>,

    // ---- Scroll state ------------------------------------------------------
    pub scroll_offset: i32,
    pub text_scroll_offset: i32,
    pub text_scroll_total_height: i32,
    /// Pixel height of the scroll-mode content viewport (window minus the tabs
    /// band, header, and — in search sub-modes — the search bar). Cached by the
    /// renderer each frame so scroll handlers clamp to the exact visible area.
    pub text_scroll_viewport_h: i32,
    pub input_search_scroll_offset: i32,

    // ---- Scroll search state -----------------------------------------------
    pub scroll_search_match_count: usize,
    pub scroll_search_current_match: usize,
    /// When true the renderer picks the first match in/after the current viewport
    /// on the next frame, then clears this flag. Set only by Ctrl+F entry.
    pub scroll_search_needs_position: bool,
    /// When true the renderer snaps the viewport to the current match on the
    /// next frame, then clears this flag. Set by Up/Down navigation in ScrollSearch.
    pub scroll_search_snap: bool,

    // ---- Dashboard state ---------------------------------------------------
    pub dashboard_image_path: String,
    /// Last (cols, rows) the active provider was told about while in
    /// `Coordinate::Dashboard` with `DashboardKind::Interactive`. Used by
    /// view.rs to fire `dashboard_resize` only on actual size changes.
    /// Reset to `(0, 0)` on enter/leave.
    pub dashboard_cell_size: (u16, u16),
    /// `sdl_ticks()` of the last Ctrl+C pressed inside the interactive
    /// dashboard. A second Ctrl+C within `DASHBOARD_CTRL_C_EXIT_MS` leaves the
    /// dashboard; the first press still forwards `0x03` to the program so a
    /// lone Ctrl+C interrupts what's running inside. `0` means no pending
    /// first press. Reset to `0` on dashboard enter/leave.
    pub dashboard_last_ctrl_c: u64,

    // ---- Keypress timing (for double-tap detection) ------------------------
    pub last_keypress_time: u64,

    // ---- Search string (Tab search) ----------------------------------------
    pub search_string: String,
    /// `current_id` at the moment SimpleSearch/ExtendedSearch was entered.
    /// Restored on Escape so search navigation doesn't permanently move the cursor.
    pub search_origin_id: IdArray,

    // ---- Caret (cursor blink) ----------------------------------------------
    pub caret: crate::caret::CaretState,

    // ---- Clipboard ---------------------------------------------------------
    pub clipboard: Option<FfonElement>,
    pub file_clipboard_path: String,
    pub file_clipboard_is_cut: bool,

    // ---- Flags -------------------------------------------------------------
    pub needs_redraw: bool,
    pub input_down: bool,
    /// True when the current Insert session is for a `*` placeholder —
    /// the typed text is interpreted by `parse_placeholder_prefix` at commit time
    /// to resolve to either a `Str` or an `Obj`.
    pub placeholder_insert_mode: bool,

    // ---- Cached layout (filled by render loop) ----------------------------
    pub window_height: i32,
    pub cached_line_height: i32,
    /// Visual line count per list item — index matches `current_id.last()`.
    /// Populated after each render; empty until first render of a given list.
    pub cached_line_counts: Vec<usize>,
    /// X position after the input prefix — first-line caret origin.
    pub current_element_x: f32,
    /// X position before the input prefix — continuation-line caret origin.
    pub current_element_base_x: f32,
    /// Y position of the editable element.
    pub current_element_y: f32,

    // ---- Error display -----------------------------------------------------
    pub error_message: String,
    /// The last error text already announced via `pending_announcement`. Lets
    /// `announce_error_if_new` speak each error exactly once (and re-speak it
    /// if the same text is set again after being cleared).
    pub last_spoken_error: String,

    // ---- Command execution state ------------------------------------------
    pub current_command: CommandPhase,
    pub provider_command_name: String,

    // ---- Palette -----------------------------------------------------------
    pub palette_theme: PaletteTheme,

    // ---- Pending window commands (set by settings apply, consumed by main loop) --
    /// `Some(true)` → maximize window, `Some(false)` → restore, `None` → no-op.
    pub pending_maximized: Option<bool>,
    /// Set by `apply_setting("fontScale")` — consumed by the main loop to
    /// rebuild the font renderer without a restart.
    pub rebuild_font_renderer: bool,

    // ---- Save/load path (set by save-as dialog, used by Ctrl+S) ------------
    /// The filesystem path last used for Ctrl+S / save-as. Empty = no path set yet.
    pub current_save_path: String,

    // ---- Placeholder cancel state -------------------------------------------
    /// Set when a placeholder element is freshly inserted for a create session
    /// (Ctrl+I/A or `:create` command). Consumed by `handle_escape` to undo the
    /// insertion and restore the prior selection. `None` when no cancel is pending
    /// (e.g. pressing `i` on a persistent `I_PLACEHOLDER` — nothing was inserted).
    pub placeholder_cancel: Option<PlaceholderCancel>,

    // ---- Save-as / open dialog state ----------------------------------------
    /// True while navigating the filebrowser to pick a save-as destination.
    pub pending_file_browser_save_as: bool,
    /// True while navigating the filebrowser to pick a file to open/load.
    pub pending_file_browser_open: bool,
    /// Provider index of the data source when doing save-as/open (not the filebrowser).
    pub save_as_source_root_idx: usize,
    /// Navigation state to restore after save-as/open completes.
    pub save_as_return_id: IdArray,
    /// Configured save folder (relative to home, absolute, or empty → Downloads).
    pub save_folder_path: String,

    // ---- Current URI -------------------------------------------------------
    pub current_uri: String,

    // ---- Accessibility announcements --------------------------------------
    /// Text to announce via the live-region node on the next `update_if_active`
    /// call. Persists until overwritten by the next announcement — the main
    /// loop no longer clears it per-frame, so the AT has unlimited time to
    /// query the node. Set by `speak_mode_change` and `announce_char`.
    pub pending_announcement: Option<String>,
    /// Toggled on every announcement to force an AccessKit tree diff even when
    /// the announced text is the same as the previous one. A zero-width space
    /// (\u{200B}) is appended when the toggle is true; screen readers ignore it.
    pub announcement_parity: bool,

    // ---- Privacy -----------------------------------------------------------
    /// When true the visual output is suppressed (blank screen). Navigation,
    /// FFON state, and AccessKit/screen-reader output continue to work normally.
    pub privacy_blank: bool,

    // ---- Tabs --------------------------------------------------------------
    /// Saved navigation snapshots, one per tab. `tabs[active_tab]` mirrors
    /// the live `current_id` + active provider's path between tab switches.
    pub tabs: Vec<TabSnapshot>,
    /// Index into `tabs` of the currently active tab.
    pub active_tab: usize,
    /// Per-tab undo/redo timeline, parallel to `tabs`. `tab_timelines.len()`
    /// is always equal to `tabs.len()`. `tab_timelines[active_tab]` is the
    /// timeline that ctrl-Z / ctrl-Shift-Z operate on.
    pub tab_timelines: Vec<Timeline>,

    /// Set by `walk_back` / `walk_forward` while applying an undo/redo so
    /// `record_entry` can ignore side-effect emissions (e.g. a Create undo
    /// invokes `delete_item_by_name`, which would otherwise enqueue a fresh
    /// Delete entry and become the next redo target).
    pub in_history_action: bool,

    // ---- Self-update state -------------------------------------------------
    /// Latest snapshot from the background `sicompass-updater` thread.
    /// `None` when the check is disabled or has not run yet.
    pub update_state: Option<std::sync::Arc<std::sync::Mutex<sicompass_updater::UpdateStatus>>>,
    /// Receives `HotReload` events from the updater thread. The main loop
    /// drains this each frame (see `crate::programs::hot_reload_plugin`).
    pub update_event_rx: Option<std::sync::mpsc::Receiver<sicompass_updater::UpdateEvent>>,
    /// True after `error_message` has been clobbered with an "Update
    /// available" banner so the per-frame writer doesn't re-clobber real
    /// errors. Reset whenever the underlying status changes.
    pub update_message_active: bool,
}

/// One tab's state.
///
/// Each tab owns an independent set of *content* providers (terminal, file
/// browser, …) and their FFON roots — its own shell process, own navigation,
/// "like separate windows". The single shared `settings` provider is the one
/// exception: it is never duplicated and always lives as the last entry of the
/// live `AppRenderer.providers`.
///
/// Invariant: the **active** tab's providers/ffon live in `AppRenderer`'s
/// `providers`/`ffon` (the working set), so its parked `providers`/`ffon` here
/// are empty. **Inactive** tabs park their content providers/ffon in these
/// fields. Switching tabs is a swap of the content slices (see
/// `AppRenderer::switch_to_tab`) — no tree rebuild, because the provider
/// instances retain their own live state.
///
/// `current_id` + `provider_path` are also what gets persisted to disk so the
/// layout can be reconstructed across restarts (`persist_tabs`).
pub struct TabSnapshot {
    pub current_id: IdArray,
    /// `current_path()` of the provider at `current_id[0]` at snapshot time.
    /// Empty when there is no provider (shouldn't happen in practice).
    pub provider_path: String,
    /// Parked content providers for an INACTIVE tab (everything except the
    /// shared settings provider, in the same order). Empty for the active tab.
    pub providers: Vec<Box<dyn Provider>>,
    /// FFON roots parallel to `providers`. Empty for the active tab.
    pub ffon: Vec<FfonElement>,
}

impl TabSnapshot {
    /// A fresh tab record holding the given navigation, with no parked
    /// providers (used for the tab that is about to become active, whose
    /// providers live in `AppRenderer`).
    pub fn nav_only(current_id: IdArray, provider_path: String) -> Self {
        TabSnapshot { current_id, provider_path, providers: Vec::new(), ffon: Vec::new() }
    }
}

/// `current_path()` of the active provider (`current_id[0]`) in `r`, or empty.
pub fn active_provider_path(r: &AppRenderer) -> String {
    r.current_id.get(0)
        .and_then(|i| r.providers.get(i))
        .map(|p| p.current_path().to_owned())
        .unwrap_or_default()
}

impl AppRenderer {
    pub fn new() -> Self {
        let mut current_id = IdArray::new();
        current_id.push(0);
        let current_id_clone = current_id.clone();

        AppRenderer {
            ffon: Vec::new(),
            providers: Vec::new(),
            current_id,
            previous_id: IdArray::new(),
            current_insert_id: IdArray::new(),
            coordinate: Coordinate::General,
            previous_coordinate: Coordinate::General,
            total_list: Vec::new(),
            filtered_list_indices: Vec::new(),
            fuzzy_match_positions: Vec::new(),
            list_index: 0,
            input_buffer: String::new(),
            cursor_position: 0,
            selection_anchor: None,
            input_prefix: String::new(),
            input_suffix: String::new(),
            insert_session: None,
            scroll_offset: 0,
            text_scroll_offset: 0,
            text_scroll_total_height: 0,
            text_scroll_viewport_h: 0,
            input_search_scroll_offset: 0,
            scroll_search_match_count: 0,
            scroll_search_current_match: 0,
            scroll_search_needs_position: false,
            scroll_search_snap: false,
            dashboard_image_path: String::new(),
            dashboard_cell_size: (0, 0),
            dashboard_last_ctrl_c: 0,
            last_keypress_time: 0,
            search_string: String::new(),
            search_origin_id: IdArray::new(),
            caret: crate::caret::CaretState::new(),
            clipboard: None,
            file_clipboard_path: String::new(),
            file_clipboard_is_cut: false,
            needs_redraw: true,
            input_down: false,
            placeholder_insert_mode: false,
            window_height: WINDOW_HEIGHT as i32,
            cached_line_height: 20,
            cached_line_counts: Vec::new(),
            current_element_x: 0.0,
            current_element_base_x: 0.0,
            current_element_y: 0.0,
            error_message: String::new(),
            last_spoken_error: String::new(),
            current_command: CommandPhase::None,
            provider_command_name: String::new(),
            palette_theme: PaletteTheme::Dark,
            pending_maximized: None,
            rebuild_font_renderer: false,
            current_save_path: String::new(),
            placeholder_cancel: None,
            pending_file_browser_save_as: false,
            pending_file_browser_open: false,
            save_as_source_root_idx: 0,
            save_as_return_id: IdArray::new(),
            save_folder_path: String::new(),
            current_uri: String::new(),
            pending_announcement: None,
            announcement_parity: false,
            privacy_blank: false,
            tabs: vec![TabSnapshot::nav_only(current_id_clone, String::new())],
            active_tab: 0,
            tab_timelines: vec![Timeline::new()],
            in_history_action: false,
            update_state: None,
            update_event_rx: None,
            update_message_active: false,
        }
    }

    /// Borrow the active tab's timeline. Panics if `active_tab` is out of
    /// bounds — but the constructor and tab handlers maintain the invariant
    /// that `tab_timelines.len() == tabs.len() > 0`.
    pub fn active_timeline(&self) -> &Timeline {
        &self.tab_timelines[self.active_tab]
    }

    pub fn active_timeline_mut(&mut self) -> &mut Timeline {
        &mut self.tab_timelines[self.active_tab]
    }

    /// Detach the active tab's *content* providers/ffon (everything except the
    /// trailing shared settings provider) out of the live working set, leaving
    /// only settings in `self.providers`/`self.ffon`. Returns the detached
    /// content vectors so the caller can park them in a tab slot.
    pub fn detach_content(&mut self) -> (Vec<Box<dyn Provider>>, Vec<FfonElement>) {
        // Settings is always the last provider; keep it in place.
        let settings_p = self.providers.pop();
        let settings_f = self.ffon.pop();
        let content_p = std::mem::take(&mut self.providers);
        let content_f = std::mem::take(&mut self.ffon);
        if let Some(p) = settings_p { self.providers.push(p); }
        if let Some(f) = settings_f { self.ffon.push(f); }
        (content_p, content_f)
    }

    /// Inverse of [`detach_content`]: splice `content_p`/`content_f` back in
    /// front of the trailing shared settings provider, making them the live
    /// working set. Assumes `self.providers`/`self.ffon` currently hold only the
    /// settings provider (i.e. a prior `detach_content`).
    pub fn attach_content(
        &mut self,
        mut content_p: Vec<Box<dyn Provider>>,
        mut content_f: Vec<FfonElement>,
    ) {
        let settings_p = self.providers.pop();
        let settings_f = self.ffon.pop();
        if let Some(p) = settings_p { content_p.push(p); }
        if let Some(f) = settings_f { content_f.push(f); }
        self.providers = content_p;
        self.ffon = content_f;
    }

    /// Switch to the tab at `target`. Parks the active tab's content providers
    /// into its slot (saving its navigation) and swaps in the target tab's
    /// parked providers. No FFON rebuild is needed — each tab's provider
    /// instances retain their own live state. No-op if `target` is out of range
    /// or already active.
    pub fn switch_to_tab(&mut self, target: usize) {
        if target >= self.tabs.len() || target == self.active_tab { return; }
        let active = self.active_tab;
        // Save outgoing navigation, then park its content providers.
        self.tabs[active].current_id = self.current_id.clone();
        self.tabs[active].provider_path = active_provider_path(self);
        let (cp, cf) = self.detach_content();
        self.tabs[active].providers = cp;
        self.tabs[active].ffon = cf;
        // Swap in the target tab's parked content.
        self.active_tab = target;
        let cp = std::mem::take(&mut self.tabs[target].providers);
        let cf = std::mem::take(&mut self.tabs[target].ffon);
        self.attach_content(cp, cf);
        self.current_id = self.tabs[target].current_id.clone();
        self.list_index = self.current_id.last().unwrap_or(0);
    }

    /// Rebuild the active tab's saved provider FFON tree (for the cold-start
    /// restore path, where providers were freshly instantiated and need their
    /// saved navigation re-grafted) and clamp `current_id` to what actually
    /// resolves. The active tab's providers must already be the live working
    /// set. Used by `apply_tabs_section` and `handle_tab_close`.
    pub fn load_active_tab(&mut self) {
        let provider_path = self.tabs[self.active_tab].provider_path.clone();
        let current_id = self.tabs[self.active_tab].current_id.clone();
        self.rebuild_and_clamp(&provider_path, current_id);
    }

    /// Deep-rebuild the provider tree at `provider_path` (if it differs from the
    /// provider's current path) and set `self.current_id` to `current_id`
    /// clamped to the rebuilt tree.
    pub fn rebuild_and_clamp(&mut self, provider_path: &str, current_id: IdArray) {
        // Rebuild the saved provider's FFON tree at full depth so a deep
        // `current_id` still resolves into it (see `deep_rebuild_provider_tree`).
        if let Some(provider_idx) = current_id.get(0) {
            if provider_idx < self.providers.len()
                && provider_idx < self.ffon.len()
                && !provider_path.is_empty()
                && self.providers[provider_idx].current_path() != provider_path
            {
                self.deep_rebuild_provider_tree(
                    provider_idx,
                    provider_path,
                    current_id.depth(),
                );
            }
        }

        // Clamp the cursor position to the actual children count at the
        // restored path. Providers with ephemeral content (terminal
        // scrollback, chat backlog) often have fewer elements after restart
        // than when last saved, so a stale `current_id.last()` would point
        // past the end. Without this clamp, downstream code that propagates
        // `current_id.last()` into `list_index` (see view.rs after a tick)
        // would leave focus on a non-existent row.
        let mut current_id = current_id;
        // Pop trailing indices while the path no longer resolves through the
        // rebuilt FFON tree — handles providers whose tree shrinks at any
        // depth after restart, e.g. the webbrowser, which does not persist
        // its loaded page so a cursor saved inside the previous page tree
        // must collapse back onto the URL bar.
        while current_id.depth() > 0
            && sicompass_sdk::ffon::get_ffon_at_id(&self.ffon, &current_id).is_none()
        {
            current_id.pop();
        }
        if let Some(parent_slice) =
            sicompass_sdk::ffon::get_ffon_at_id(&self.ffon, &current_id)
        {
            let last_idx = current_id.last().unwrap_or(0);
            let max_idx = parent_slice.len().saturating_sub(1);
            if last_idx > max_idx {
                current_id.set_last(max_idx);
            }
        }
        self.current_id = current_id;
        self.list_index = self.current_id.last().unwrap_or(0);
    }

    /// Rebuild a provider's FFON tree at full depth so a saved deep `current_id`
    /// resolves after a tab switch / app restart.
    ///
    /// Navigation is uniformly in-memory: each `Right` grafts a fetched level
    /// onto the Obj it descends into. Re-fetching only the deepest path would
    /// collapse every ancestor and clamp a deep `current_id` back to depth 2.
    ///
    /// `cursor_depth` (the saved `current_id` depth) tells us how many path
    /// segments were *navigated* — `current_id` depth 2 sits at the provider
    /// root, and each level beyond that is one navigated segment. The tree root
    /// is the path prefix above those navigated segments: this matters for the
    /// file browser, whose `current_path` is an absolute path rooted wherever
    /// the session started, not `"/"`. We fetch the root at that prefix and
    /// then graft each navigated segment's children onto the matching Obj
    /// (matched by the stripped display text of the key). If a segment cannot
    /// be matched the descent stops there; the `load_active_tab` clamp then
    /// trims `current_id` to the depth that was rebuilt.
    fn deep_rebuild_provider_tree(&mut self, provider_idx: usize, path: &str, cursor_depth: usize) {
        use sicompass_sdk::ffon::FfonElement;
        use sicompass_sdk::tags::strip_display;
        use std::path::PathBuf;

        // Whole-tree providers (currently `settings`) return their entire
        // navigable structure from a single `fetch()` regardless of
        // `current_path`. The per-segment re-fetch + graft pattern below
        // would replace each ancestor's children with the WHOLE top-level
        // tree (sections nested inside sections), so a deep cursor like
        // `[settings, sicompass_idx, language_radio_idx, option_idx]` would
        // resolve into a *sibling section* (e.g. email client) rather than
        // back into the language radio. Handle them specially: one fetch,
        // descend by key-match without re-fetching.
        if self.providers[provider_idx].name() == "settings" {
            let display_name = self.providers[provider_idx].display_name();
            self.providers[provider_idx].set_current_path(path);
            let root_children = self.providers[provider_idx].fetch();
            let mut root = FfonElement::new_obj(&display_name);
            if let Some(obj) = root.as_obj_mut() {
                for c in root_children {
                    obj.push(c);
                }
            }
            self.ffon[provider_idx] = root;
            return;
        }

        // Filesystem providers (filebrowser, texteditor) use native paths —
        // on Windows those carry backslash separators and drive prefixes
        // (`C:\…`). Splitting them on `/` collapses the whole path into one
        // opaque segment and rebases the root to `"/"`, which on Windows is
        // the drive-list sentinel — every navigated tab restored on Windows
        // ended up showing `C:\, D:\` instead of its real directory. Route
        // those paths through `std::path::PathBuf` so file-name extraction
        // and joining stay platform-correct.
        let is_fs = self.providers[provider_idx].path_is_filesystem();
        let navigated_count = cursor_depth.saturating_sub(2);

        let (root_prefix, segments): (String, Vec<String>) = if is_fs {
            let mut buf = PathBuf::from(path);
            let mut leaf: Vec<String> = Vec::with_capacity(navigated_count);
            for _ in 0..navigated_count {
                let Some(name) = buf
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                else { break };
                leaf.push(name);
                if !buf.pop() { break; }
            }
            leaf.reverse();
            (buf.to_string_lossy().into_owned(), leaf)
        } else {
            let all: Vec<String> = path
                .split('/')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            let n = navigated_count.min(all.len());
            let split = all.len() - n;
            let joined = all[..split].join("/");
            let root = if joined.is_empty() { "/".to_owned() } else { format!("/{joined}") };
            (root, all[split..].to_vec())
        };

        let display_name = self.providers[provider_idx].display_name().to_owned();

        // Fetch the children at the root prefix and at each navigated segment.
        let mut levels: Vec<Vec<FfonElement>> = Vec::with_capacity(segments.len() + 1);
        self.providers[provider_idx].set_current_path(&root_prefix);
        levels.push(self.providers[provider_idx].fetch());

        let mut indices: Vec<usize> = Vec::with_capacity(segments.len());
        let mut prefix = root_prefix.clone();
        for seg in &segments {
            let found = levels.last().unwrap().iter().position(|e| {
                matches!(e, FfonElement::Obj(o) if strip_display(&o.key) == *seg)
            });
            let idx = match found {
                Some(i) => i,
                None => break, // can't descend further — graft what matched
            };
            indices.push(idx);
            prefix = if is_fs {
                let mut buf = PathBuf::from(&prefix);
                buf.push(seg);
                buf.to_string_lossy().into_owned()
            } else if prefix == "/" {
                format!("/{seg}")
            } else {
                format!("{prefix}/{seg}")
            };
            self.providers[provider_idx].set_current_path(&prefix);
            levels.push(self.providers[provider_idx].fetch());
        }

        // Graft each fetched child level onto its parent Obj, deepest first.
        for k in (0..indices.len()).rev() {
            let child_level = levels.pop().unwrap();
            if let FfonElement::Obj(o) = &mut levels[k][indices[k]] {
                o.children = child_level;
            }
        }
        let root_children = levels.pop().unwrap_or_default();

        let mut root = FfonElement::new_obj(&display_name);
        if let Some(obj) = root.as_obj_mut() {
            for c in root_children {
                obj.push(c);
            }
        }
        self.ffon[provider_idx] = root;

        // Live provider path = the full restored path, matching the deep cursor.
        self.providers[provider_idx].set_current_path(path);
    }

    /// Set the screen-reader announcement to "tab N/M: <label>". Toggles the
    /// parity sentinel so back-to-back identical announcements still produce
    /// an AccessKit tree diff.
    pub fn speak_tab_change(&mut self, label: &str) {
        crate::shortcuts::register_translations();
        let mut args = sicompass_sdk::localize::Args::new();
        args.set("idx", (self.active_tab + 1) as i64);
        args.set("total", self.tabs.len() as i64);
        args.set("label", label.to_owned());
        let text = sicompass_sdk::localize::t_args("speak-tab-change", &args);
        // Fallback: if no translation is registered the message ID is
        // returned verbatim. Use the legacy English template in that case
        // so the screen reader still announces something readable.
        let text = if text == "speak-tab-change" {
            format!("tab {}/{}: {}", self.active_tab + 1, self.tabs.len(), label)
        } else {
            text
        };
        self.announcement_parity = !self.announcement_parity;
        let sentinel = if self.announcement_parity { "\u{200B}" } else { "" };
        self.pending_announcement = Some(format!("{text}{sentinel}"));
    }

    /// Set the screen-reader announcement to the current coordinate's spoken
    /// name, optionally suffixed with a context string.
    ///
    /// Mirrors `accesskitSpeakModeChange` from the C source. The announcement
    /// persists in `pending_announcement` until the next call overwrites it —
    /// the main loop no longer clears it per-frame.
    ///
    /// `announcement_parity` is toggled on every call so that identical
    /// consecutive announcements still produce an AccessKit tree diff (a
    /// zero-width space is appended when parity is true — screen readers
    /// universally ignore it in speech output).
    pub fn speak_mode_change(&mut self, context: Option<String>) {
        let mode = self.coordinate.display_label();
        let text = match context {
            Some(ctx) if !ctx.is_empty() => format!("{mode} - {ctx}"),
            _ => mode,
        };
        self.announcement_parity = !self.announcement_parity;
        let sentinel = if self.announcement_parity { "\u{200B}" } else { "" };
        self.pending_announcement = Some(format!("{text}{sentinel}"));
    }

    /// If `error_message` holds a new error, announce it via the live region.
    /// Called once per render frame so any error reaching the header is spoken
    /// exactly once, regardless of which code path set it. Clears the tracker
    /// when the error clears, so the same text spoken again is re-announced.
    /// Toggles `announcement_parity` so a repeat still produces a tree diff.
    pub fn announce_error_if_new(&mut self) {
        if self.error_message.is_empty() {
            self.last_spoken_error.clear();
            return;
        }
        if self.error_message == self.last_spoken_error {
            return;
        }
        self.last_spoken_error = self.error_message.clone();
        self.announcement_parity = !self.announcement_parity;
        let sentinel = if self.announcement_parity { "\u{200B}" } else { "" };
        self.pending_announcement = Some(format!("{}{sentinel}", self.error_message));
    }

    /// Set the pending screen-reader announcement to the currently selected
    /// list item's spoken label. Used by filter modes (search/command/extended
    /// search) after any action that changes which item is selected — mode
    /// entry, typing, backspace, up/down/page/ctrl-home/ctrl-end navigation.
    ///
    /// Mirrors `accesskitSpeakCurrentElement` in C. No-op if the list is empty.
    /// Toggles `announcement_parity` so back-to-back identical items still
    /// produce an AccessKit tree diff.
    pub fn speak_current_element(&mut self) {
        let Some(item) = self.current_list_item() else { return; };
        let text = crate::accesskit_sdl::label_to_speech(&item.label);
        self.announcement_parity = !self.announcement_parity;
        let sentinel = if self.announcement_parity { "\u{200B}" } else { "" };
        self.pending_announcement = Some(format!("{text}{sentinel}"));
    }

    /// Return the active color palette.
    pub fn palette(&self) -> &'static ColorPalette {
        match self.palette_theme {
            PaletteTheme::Dark => &PALETTE_DARK,
            PaletteTheme::Light => &PALETTE_LIGHT,
        }
    }

    // ---- List helpers ------------------------------------------------------

    /// Number of items in the currently active list (filtered or total).
    pub fn active_list_len(&self) -> usize {
        if self.filtered_list_indices.is_empty() {
            self.total_list.len()
        } else {
            self.filtered_list_indices.len()
        }
    }

    /// Currently selected item.
    pub fn current_list_item(&self) -> Option<&RenderListItem> {
        if self.filtered_list_indices.is_empty() {
            self.total_list.get(self.list_index)
        } else {
            let raw_idx = *self.filtered_list_indices.get(self.list_index)?;
            self.total_list.get(raw_idx)
        }
    }

    /// Id of the currently selected item.
    pub fn current_list_item_id(&self) -> Option<IdArray> {
        self.current_list_item().map(|item| item.id.clone())
    }

    /// Update `current_id.last()` to match the selected list item.
    pub fn sync_current_id_from_list(&mut self) {
        if let Some(item_id) = self.current_list_item_id() {
            if let Some(last_idx) = item_id.last() {
                self.current_id.set_last(last_idx);
            }
        }
    }
}

impl Default for AppRenderer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

/// Owns the SDL3 window, Vulkan context, and application renderer state.
pub struct AppState {
    // ---- SDL ----------------------------------------------------------------
    /// Keep the SDL context alive for the duration of the app.
    pub(crate) _sdl: sdl3::Sdl,
    /// Keep the video subsystem alive; window is derived from it.
    pub(crate) _video: sdl3::VideoSubsystem,
    pub window: sdl3::video::Window,
    pub event_pump: sdl3::EventPump,

    // ---- Vulkan core --------------------------------------------------------
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    #[cfg(debug_assertions)]
    pub debug_utils: ash::ext::debug_utils::Instance,
    #[cfg(debug_assertions)]
    pub debug_messenger: ash::vk::DebugUtilsMessengerEXT,
    pub surface_loader: ash::khr::surface::Instance,
    pub surface: ash::vk::SurfaceKHR,
    pub physical_device: ash::vk::PhysicalDevice,
    pub device: ash::Device,
    pub graphics_queue: ash::vk::Queue,
    pub present_queue: ash::vk::Queue,
    pub graphics_family: u32,
    pub present_family: u32,

    // ---- Swapchain ----------------------------------------------------------
    pub swapchain_loader: ash::khr::swapchain::Device,
    pub swapchain: ash::vk::SwapchainKHR,
    pub swapchain_images: Vec<ash::vk::Image>,
    pub swapchain_format: ash::vk::Format,
    pub swapchain_extent: ash::vk::Extent2D,
    pub swapchain_image_views: Vec<ash::vk::ImageView>,

    // ---- Render pass + framebuffers -----------------------------------------
    pub render_pass: ash::vk::RenderPass,
    pub framebuffers: Vec<ash::vk::Framebuffer>,

    // ---- Command recording --------------------------------------------------
    pub command_pool: ash::vk::CommandPool,
    pub command_buffers: Vec<ash::vk::CommandBuffer>,

    // ---- Synchronisation ----------------------------------------------------
    pub image_available: [ash::vk::Semaphore; MAX_FRAMES_IN_FLIGHT],
    pub render_finished: [ash::vk::Semaphore; MAX_FRAMES_IN_FLIGHT],
    pub in_flight: [ash::vk::Fence; MAX_FRAMES_IN_FLIGHT],
    pub current_frame: usize,

    // ---- App render state ---------------------------------------------------
    pub framebuffer_resized: bool,
    pub running: bool,
    /// RGBA clear colour (background). Set from active palette.
    pub clear_color: [f32; 4],

    // ---- Application renderer (providers, FFON, navigation) ----------------
    pub renderer: AppRenderer,

    // ---- Rendering sub-systems (Phase 5) -----------------------------------
    pub font_renderer: Option<crate::text::FontRenderer>,
    pub rect_renderer: Option<crate::rectangle::RectangleRenderer>,
    pub image_renderer: Option<crate::image::ImageRenderer>,

    // ---- Accessibility -----------------------------------------------------
    pub accesskit_adapter: Option<crate::accesskit_sdl::AccessKitAdapter>,

    // ---- Settings apply queue ----------------------------------------------
    /// Receives (key, value) events fired by the settings provider's ApplyFn.
    /// Drained each frame in the main loop via `programs::apply_pending_settings`.
    pub settings_queue: Option<crate::programs::SettingsQueue>,

    // ---- Startup guard -----------------------------------------------------
    /// Set to `true` once the initial `pending_maximized` has been applied.
    /// Window Maximized/Restored events are ignored until then to prevent the
    /// startup race where SDL fires a Restored event before the window is
    /// maximized, which would write `maximized: false` to settings.json.
    pub maximized_ready: bool,
}

impl AppState {
    /// Initialise SDL3, create a Vulkan window and device, set up the
    /// render pipeline, then load providers into the AppRenderer.
    pub fn new() -> Result<Self, SiError> {
        let mut state = render::build_app()?;

        // Compute effective DPI: OS display scale × user font_scale override.
        let content_scale = state
            .window
            .get_display()
            .and_then(|d| d.get_content_scale())
            .unwrap_or(1.0);
        let font_scale = crate::programs::read_font_scale();
        let effective_dpi = (96.0_f32 * content_scale * font_scale)
            .round()
            .max(48.0) as u32;

        // Initialise rendering sub-systems
        unsafe {
            let fr = crate::text::FontRenderer::new(
                &state.device,
                &state.instance,
                state.physical_device,
                state.command_pool,
                state.graphics_queue,
                state.render_pass,
                effective_dpi,
            )?;
            state.font_renderer = Some(fr);

            let rr = crate::rectangle::RectangleRenderer::new(
                &state.device,
                &state.instance,
                state.physical_device,
                state.render_pass,
            )?;
            state.rect_renderer = Some(rr);

            let ir = crate::image::ImageRenderer::new(
                &state.device,
                &state.instance,
                state.physical_device,
                state.command_pool,
                state.graphics_queue,
                state.render_pass,
            )?;
            state.image_renderer = Some(ir);
        }

        // Initialise accessibility adapter (no-op if no AT is active)
        state.accesskit_adapter =
            crate::accesskit_sdl::AccessKitAdapter::new(&state.window, &state.renderer);

        // Load providers (tutorial + settings by default)
        let queue = crate::programs::load_programs(&mut state.renderer);
        // Apply initial settings (skip enable_* — providers already loaded above)
        crate::programs::apply_pending_settings(&mut state.renderer, &queue, true);
        state.settings_queue = Some(queue);

        // Restore persisted tab layout (no-op if none stored).
        // Must run AFTER providers are loaded so provider-index validation works.
        crate::programs::load_tabs_state(&mut state.renderer);

        crate::list::create_list_current_layer(&mut state.renderer);
        Ok(state)
    }

    /// Run the main event loop until the window is closed.
    pub fn run(&mut self) {
        view::main_loop(self);
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        render::cleanup(self);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    // The display_label / speak_tab_change tests mutate the global active
    // locale; serialize them to avoid racing other tests in the binary.
    fn locale_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static L: OnceLock<Mutex<()>> = OnceLock::new();
        L.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|e| e.into_inner())
    }

    // --- Coordinate::display_label (i18n) ---

    #[test]
    fn coordinate_display_label_en_us_matches_as_str() {
        let _g = locale_test_lock();
        sicompass_sdk::localize::set_locale("en-US");
        assert_eq!(Coordinate::General.display_label(), "general mode");
        assert_eq!(Coordinate::Insert.display_label(), "insert mode");
        assert_eq!(Coordinate::SimpleSearch.display_label(), "search");
        sicompass_sdk::localize::set_locale("en-US");
    }

    #[test]
    fn coordinate_display_label_translates_to_belgian_locales() {
        let _g = locale_test_lock();

        sicompass_sdk::localize::set_locale("nl-BE");
        assert_eq!(Coordinate::General.display_label(), "algemene modus");
        assert_eq!(Coordinate::Insert.display_label(), "invoegmodus");

        sicompass_sdk::localize::set_locale("fr-BE");
        assert_eq!(Coordinate::General.display_label(), "mode général");
        assert_eq!(Coordinate::Insert.display_label(), "mode insertion");

        sicompass_sdk::localize::set_locale("de-BE");
        assert_eq!(Coordinate::General.display_label(), "allgemeiner Modus");
        assert_eq!(Coordinate::Insert.display_label(), "Einfügemodus");

        sicompass_sdk::localize::set_locale("en-US");
    }

    #[test]
    fn speak_tab_change_uses_translated_template() {
        let _g = locale_test_lock();
        let mut r = AppRenderer::new();
        sicompass_sdk::localize::set_locale("nl-BE");
        r.speak_tab_change("bestandsverkenner");
        // Strip parity sentinel (zero-width space) before asserting.
        let ann = r.pending_announcement.as_ref().expect("ann set");
        let ann = ann.trim_end_matches('\u{200B}');
        assert!(
            ann.starts_with("tabblad ") && ann.contains(": bestandsverkenner"),
            "nl-BE tab announcement should use 'tabblad' template, got: {ann:?}"
        );
        sicompass_sdk::localize::set_locale("en-US");
    }

    // --- Coordinate::as_str ---

    #[test]
    fn coordinate_as_str_general() {
        assert_eq!(Coordinate::General.as_str(), "general mode");
    }

    #[test]
    fn coordinate_as_str_insert() {
        assert_eq!(Coordinate::Insert.as_str(), "insert mode");
    }

    #[test]
    fn coordinate_as_str_normal() {
        assert_eq!(Coordinate::Normal.as_str(), "normal mode");
    }

    #[test]
    fn coordinate_as_str_visual() {
        assert_eq!(Coordinate::Visual.as_str(), "visual mode");
    }

    #[test]
    fn coordinate_as_str_simple_search() {
        assert_eq!(Coordinate::SimpleSearch.as_str(), "search");
    }

    #[test]
    fn coordinate_as_str_extended_search() {
        assert_eq!(Coordinate::ExtendedSearch.as_str(), "extended search");
    }

    #[test]
    fn coordinate_as_str_command() {
        assert_eq!(Coordinate::Command.as_str(), "command");
    }

    #[test]
    fn coordinate_as_str_scroll() {
        assert_eq!(Coordinate::Scroll.as_str(), "scroll mode");
    }

    #[test]
    fn coordinate_as_str_scroll_search() {
        assert_eq!(Coordinate::ScrollSearch.as_str(), "scroll search");
        assert_eq!(Coordinate::ScrollPrefixSearch.as_str(), "scroll prefix search");
    }

    #[test]
    fn coordinate_as_str_input_search() {
        assert_eq!(Coordinate::InputSearch.as_str(), "input search");
    }

    #[test]
    fn coordinate_as_str_dashboard() {
        assert_eq!(Coordinate::Dashboard.as_str(), "dashboard");
    }

    // --- Task::as_str ---

    #[test]
    fn task_as_str_none() { assert_eq!(Task::None.as_str(), "none"); }

    #[test]
    fn task_as_str_input() { assert_eq!(Task::Input.as_str(), "input"); }

    #[test]
    fn task_as_str_append() { assert_eq!(Task::Append.as_str(), "append"); }

    #[test]
    fn task_as_str_append_append() { assert_eq!(Task::AppendAppend.as_str(), "append append"); }

    #[test]
    fn task_as_str_insert() { assert_eq!(Task::Insert.as_str(), "insert"); }

    #[test]
    fn task_as_str_insert_insert() { assert_eq!(Task::InsertInsert.as_str(), "insert insert"); }

    #[test]
    fn task_as_str_delete() { assert_eq!(Task::Delete.as_str(), "delete"); }

    #[test]
    fn task_as_str_arrow_up() { assert_eq!(Task::ArrowUp.as_str(), "up"); }

    #[test]
    fn task_as_str_arrow_down() { assert_eq!(Task::ArrowDown.as_str(), "down"); }

    #[test]
    fn task_as_str_arrow_left() { assert_eq!(Task::ArrowLeft.as_str(), "left"); }

    #[test]
    fn task_as_str_arrow_right() { assert_eq!(Task::ArrowRight.as_str(), "right"); }

    #[test]
    fn task_as_str_cut() { assert_eq!(Task::Cut.as_str(), "cut"); }

    #[test]
    fn task_as_str_copy() { assert_eq!(Task::Copy.as_str(), "copy"); }

    #[test]
    fn task_as_str_paste() { assert_eq!(Task::Paste.as_str(), "paste"); }

    // --- AppRenderer.error_message ---

    #[test]
    fn error_message_simple() {
        let mut r = AppRenderer::new();
        r.error_message = "test error".to_owned();
        assert_eq!(r.error_message, "test error");
    }

    #[test]
    fn error_message_empty() {
        let mut r = AppRenderer::new();
        r.error_message = String::new();
        assert_eq!(r.error_message, "");
    }

    #[test]
    fn error_message_overwrites() {
        let mut r = AppRenderer::new();
        r.error_message = "first".to_owned();
        r.error_message = "second".to_owned();
        assert_eq!(r.error_message, "second");
    }
}
