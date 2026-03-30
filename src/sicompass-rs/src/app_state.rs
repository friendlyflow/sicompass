//! `AppState` — owns the SDL3 window and the entire Vulkan context.
//!
//! Equivalent to `SiCompassApplication` + `AppRenderer` in the C code.

use crate::render;
use crate::view;
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
}

impl AppState {
    /// Initialise SDL3, create a Vulkan window and device, set up the
    /// render pipeline.  Mirrors `initWindow()` + `initVulkan()` in C.
    pub fn new() -> Result<Self, SiError> {
        render::build_app()
    }

    /// Run the main event loop until the window is closed.
    /// Mirrors `startApp()` + the loop inside `mainLoop()` in C.
    pub fn run(&mut self) {
        view::main_loop(self);
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        render::cleanup(self);
    }
}
