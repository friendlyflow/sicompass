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
// Enums
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

/// Undo/redo direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum History {
    #[default]
    None,
    Undo,
    Redo,
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
            current_uri: String::new(),
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
        }

        // Load providers (tutorial by default)
        crate::programs::load_programs(&mut state.renderer);
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
