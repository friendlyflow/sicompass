//! `AppState` — owns the SDL3 window, the entire Vulkan context, and the
//! application-level renderer state (`AppRenderer`).
//!
//! Equivalent to `SiCompassApplication` + `AppRenderer` in the C code.

use crate::render;
use crate::view;
use sicompass_sdk::ffon::{FfonElement, IdArray};
use sicompass_sdk::provider::Provider;
use std::fmt;

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

/// Navigation / edit mode — mirrors the C `Coordinate` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Coordinate {
    #[default]
    OperatorGeneral,
    OperatorInsert,
    EditorGeneral,
    EditorInsert,
    EditorNormal,
    EditorVisual,
    SimpleSearch,
    ExtendedSearch,
    Command,
    Scroll,
    ScrollSearch,
    InputSearch,
    Dashboard,
}

impl Coordinate {
    pub fn as_str(self) -> &'static str {
        match self {
            Coordinate::OperatorGeneral => "operator",
            Coordinate::OperatorInsert => "operator insert",
            Coordinate::EditorGeneral => "editor",
            Coordinate::EditorInsert => "editor insert",
            Coordinate::EditorNormal => "editor normal",
            Coordinate::EditorVisual => "editor visual",
            Coordinate::SimpleSearch => "search",
            Coordinate::ExtendedSearch => "extended search",
            Coordinate::Command => "command",
            Coordinate::Scroll => "scroll",
            Coordinate::ScrollSearch => "scroll search",
            Coordinate::InputSearch => "input search",
            Coordinate::Dashboard => "dashboard",
        }
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
    FsCreate,
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
            Task::FsCreate => "fs create",
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
}

/// An undo history entry.
#[derive(Debug, Clone)]
pub struct UndoEntry {
    pub id: IdArray,
    pub task: Task,
    pub prev_element: Option<FfonElement>,
    pub new_element: Option<FfonElement>,
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

    // ---- Scroll state ------------------------------------------------------
    pub scroll_offset: i32,
    pub text_scroll_offset: i32,
    pub text_scroll_line_count: i32,

    // ---- Scroll search state -----------------------------------------------
    pub scroll_search_match_count: usize,
    pub scroll_search_current_match: usize,

    // ---- Dashboard state ---------------------------------------------------
    pub dashboard_image_path: String,

    // ---- Keypress timing (for double-tap detection) ------------------------
    pub last_keypress_time: u64,

    // ---- Search string (Tab search) ----------------------------------------
    pub search_string: String,

    // ---- Undo history ------------------------------------------------------
    pub undo_history: Vec<UndoEntry>,
    pub undo_position: usize,

    // ---- Caret (cursor blink) ----------------------------------------------
    pub caret: crate::caret::CaretState,

    // ---- Clipboard ---------------------------------------------------------
    pub clipboard: Option<FfonElement>,
    pub file_clipboard_path: String,
    pub file_clipboard_is_cut: bool,

    // ---- Flags -------------------------------------------------------------
    pub needs_redraw: bool,
    pub input_down: bool,
    pub prefixed_insert_mode: bool,
    pub show_meta_menu: bool,
    pub inside_meta: bool,
    pub meta_return_id: IdArray,
    pub meta_return_list_index: usize,

    // ---- Cached layout (filled by render loop) ----------------------------
    pub window_height: i32,
    pub cached_line_height: i32,

    // ---- Error display -----------------------------------------------------
    pub error_message: String,

    // ---- Command execution state ------------------------------------------
    pub current_command: CommandPhase,
    pub provider_command_name: String,

    // ---- Palette -----------------------------------------------------------
    pub palette_theme: PaletteTheme,

    // ---- Pending window commands (set by settings apply, consumed by main loop) --
    /// `Some(true)` → maximize window, `Some(false)` → restore, `None` → no-op.
    pub pending_maximized: Option<bool>,

    // ---- Save/load path (set by save-as dialog, used by Ctrl+S) ------------
    /// The filesystem path last used for Ctrl+S / save-as. Empty = no path set yet.
    pub current_save_path: String,

    // ---- Current URI -------------------------------------------------------
    pub current_uri: String,
}

impl AppRenderer {
    pub fn new() -> Self {
        let mut current_id = IdArray::new();
        current_id.push(0);

        AppRenderer {
            ffon: Vec::new(),
            providers: Vec::new(),
            current_id,
            previous_id: IdArray::new(),
            current_insert_id: IdArray::new(),
            coordinate: Coordinate::OperatorGeneral,
            previous_coordinate: Coordinate::OperatorGeneral,
            total_list: Vec::new(),
            filtered_list_indices: Vec::new(),
            list_index: 0,
            input_buffer: String::new(),
            cursor_position: 0,
            selection_anchor: None,
            input_prefix: String::new(),
            input_suffix: String::new(),
            scroll_offset: 0,
            text_scroll_offset: 0,
            text_scroll_line_count: 0,
            scroll_search_match_count: 0,
            scroll_search_current_match: 0,
            dashboard_image_path: String::new(),
            last_keypress_time: 0,
            search_string: String::new(),
            undo_history: Vec::new(),
            undo_position: 0,
            caret: crate::caret::CaretState::new(),
            clipboard: None,
            file_clipboard_path: String::new(),
            file_clipboard_is_cut: false,
            needs_redraw: true,
            input_down: false,
            prefixed_insert_mode: false,
            show_meta_menu: false,
            inside_meta: false,
            meta_return_id: IdArray::new(),
            meta_return_list_index: 0,
            window_height: WINDOW_HEIGHT as i32,
            cached_line_height: 20,
            error_message: String::new(),
            current_command: CommandPhase::None,
            provider_command_name: String::new(),
            palette_theme: PaletteTheme::Dark,
            pending_maximized: None,
            current_save_path: String::new(),
            current_uri: String::new(),
        }
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
}

impl AppState {
    /// Initialise SDL3, create a Vulkan window and device, set up the
    /// render pipeline, then load providers into the AppRenderer.
    pub fn new() -> Result<Self, SiError> {
        let mut state = render::build_app()?;

        // Initialise rendering sub-systems
        unsafe {
            let fr = crate::text::FontRenderer::new(
                &state.device,
                &state.instance,
                state.physical_device,
                state.command_pool,
                state.graphics_queue,
                state.render_pass,
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

    // --- Coordinate::as_str ---

    #[test]
    fn coordinate_as_str_operator_general() {
        assert_eq!(Coordinate::OperatorGeneral.as_str(), "operator");
    }

    #[test]
    fn coordinate_as_str_operator_insert() {
        assert_eq!(Coordinate::OperatorInsert.as_str(), "operator insert");
    }

    #[test]
    fn coordinate_as_str_editor_general() {
        assert_eq!(Coordinate::EditorGeneral.as_str(), "editor");
    }

    #[test]
    fn coordinate_as_str_editor_insert() {
        assert_eq!(Coordinate::EditorInsert.as_str(), "editor insert");
    }

    #[test]
    fn coordinate_as_str_editor_normal() {
        assert_eq!(Coordinate::EditorNormal.as_str(), "editor normal");
    }

    #[test]
    fn coordinate_as_str_editor_visual() {
        assert_eq!(Coordinate::EditorVisual.as_str(), "editor visual");
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
        assert_eq!(Coordinate::Scroll.as_str(), "scroll");
    }

    #[test]
    fn coordinate_as_str_scroll_search() {
        assert_eq!(Coordinate::ScrollSearch.as_str(), "scroll search");
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
