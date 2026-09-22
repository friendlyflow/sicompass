//! Compositor state — all Wayland protocol state in one struct, implementing
//! the smithay delegate traits.
//!
//! Mirrors the `tinywl_server` struct in `src/desicompass-c/main.c` but
//! expressed through smithay's typed API instead of raw wlroots calls, and
//! without the cursor half: desicompass is keyboard-driven and advertises no
//! `wl_pointer` at all.

use std::time::Instant;

use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    backend::{allocator::dmabuf::Dmabuf, renderer::ImportDma},
    delegate_compositor, delegate_data_device, delegate_dmabuf, delegate_output, delegate_seat,
    delegate_shm, delegate_xdg_shell,
    desktop::{PopupKind, PopupManager, Space, Window},
    input::{keyboard::FilterResult, pointer::CursorImageStatus, Seat, SeatHandler, SeatState},
    output::Output,
    reexports::{
        calloop::LoopSignal,
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_buffer::WlBuffer, wl_seat::WlSeat, wl_surface::WlSurface},
            Client, DisplayHandle, Resource,
        },
    },
    utils::{Logical, Rectangle, Serial, Size},
    wayland::{
        buffer::BufferHandler,
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        compositor::{
            get_parent, is_sync_subsurface, with_states, CompositorClientState, CompositorHandler,
            CompositorState,
        },
        output::OutputManagerState,
        selection::{
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
            },
            SelectionHandler,
        },
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
            XdgToplevelSurfaceData,
        },
        shm::{ShmHandler, ShmState},
    },
};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use tracing::{debug, error, info, warn};

use crate::focus::{FocusStack, WindowId};
use crate::gpu::Gpu;
use crate::keybindings::{self, BindingAction, Mods};
use crate::layout::{Dir, Tiler};

// ---------------------------------------------------------------------------
// ClientState
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// All compositor state, analogous to `tinywl_server` in the C version.
pub struct State {
    // ---- Plumbing ----
    pub display_handle: DisplayHandle,
    pub loop_signal: LoopSignal,
    /// Clock origin for frame callback timestamps.
    pub start_time: Instant,

    // ---- Protocol state ----
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    /// Held only to keep the `wl_output` / `xdg_output` globals alive for
    /// the lifetime of the compositor; nothing reads it back.
    #[allow(dead_code)]
    pub output_manager_state: OutputManagerState,
    pub data_device_state: DataDeviceState,

    // ---- Input ----
    pub seat_state: SeatState<State>,
    pub seat: Seat<State>,

    // ---- Desktop ----
    /// Cleared by the quit binding; the main loop checks it each pass.
    pub running: bool,

    pub space: Space<Window>,
    pub popups: PopupManager,
    pub output: Output,

    // ---- Window management ----
    /// Compositor-assigned ids, paired with their window. The id is what the
    /// layout and the focus stack speak in; the `Window` never leaves here.
    pub windows: Vec<(WindowId, Window)>,
    /// Stable spatial order and geometry.
    pub tiler: Tiler,
    /// Most-recently-used order, for "what gets focus now" questions.
    pub focus: FocusStack,
    next_id: usize,
    /// Set by the first quit chord, cleared by any other binding.
    quit_armed: bool,

    // ---- Rendering ----
    /// The GPU side. It lives here rather than in the main loop because
    /// `DmabufHandler::dmabuf_imported` is handed only `&mut State` and has
    /// to reach the renderer to validate a client's buffer.
    pub backend: Gpu,
    pub dmabuf_state: DmabufState,

    // ---- Spawning ----
    /// Command run by the spawn binding.
    pub spawn_cmd: String,
    /// Our socket name, handed to spawned children.
    pub socket_name: String,
}

impl State {
    pub fn new(
        display: &DisplayHandle,
        loop_signal: LoopSignal,
        output: Output,
        backend: Gpu,
    ) -> Self {
        let compositor_state = CompositorState::new::<Self>(display);
        let xdg_shell_state = XdgShellState::new::<Self>(display);
        let shm_state = ShmState::new::<Self>(display, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(display);
        let data_device_state = DataDeviceState::new::<Self>(display);
        let mut seat_state = SeatState::new();
        let seat = seat_state.new_wl_seat(display, "seat0");

        let mut space = Space::default();
        space.map_output(&output, (0, 0));

        State {
            display_handle: display.clone(),
            loop_signal,
            start_time: Instant::now(),
            compositor_state,
            xdg_shell_state,
            shm_state,
            output_manager_state,
            data_device_state,
            seat_state,
            seat,
            running: true,
            space,
            popups: PopupManager::default(),
            windows: Vec::new(),
            tiler: Tiler::new(),
            focus: FocusStack::new(),
            next_id: 0,
            quit_armed: false,
            backend,
            dmabuf_state: DmabufState::new(),
            spawn_cmd: String::new(),
            socket_name: String::new(),
            output,
        }
    }

    /// The current output size in logical coordinates.
    ///
    /// Never zero: a 0x0 `xdg_toplevel.configure` is not a slow start for an
    /// SDL/Vulkan client, it is an unrecoverable spin — sicompass's
    /// `recreate_swapchain` loops on `size_in_pixels()` with no exit
    /// condition. Every configure this compositor sends goes through here.
    pub fn output_size(&self) -> Size<i32, Logical> {
        self.output
            .current_mode()
            .map(|mode| {
                self.output
                    .current_transform()
                    .transform_size(mode.size)
                    .to_logical(self.output.current_scale().integer_scale())
            })
            .filter(|size: &Size<i32, Logical>| size.w > 0 && size.h > 0)
            .unwrap_or_else(|| (800, 600).into())
    }

    /// The window owning `surface`, if any.
    pub fn window_for_surface(&self, surface: &WlSurface) -> Option<Window> {
        self.space
            .elements()
            .find(|w| {
                w.toplevel()
                    .is_some_and(|t| t.wl_surface() == surface)
            })
            .cloned()
    }

    /// The window owning `id`.
    pub fn window(&self, id: WindowId) -> Option<&Window> {
        self.windows.iter().find(|(w, _)| *w == id).map(|(_, win)| win)
    }

    /// The id of the window owning `surface`.
    pub fn id_for_surface(&self, surface: &WlSurface) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|(_, w)| w.toplevel().is_some_and(|t| t.wl_surface() == surface))
            .map(|(id, _)| *id)
    }

    /// Register a freshly created toplevel and give it the focus.
    pub fn add_window(&mut self, window: Window) -> WindowId {
        let id = WindowId(self.next_id);
        self.next_id += 1;

        self.tiler.insert_after_focused(id, self.focus.focused());
        self.windows.push((id, window.clone()));
        self.space.map_element(window, (0, 0), false);

        self.relayout();
        self.focus_id(id);
        id
    }

    /// Forget a window that has gone away, and hand focus to the next in the
    /// most-recently-used order.
    pub fn remove_window(&mut self, id: WindowId) {
        if let Some(window) = self.window(id).cloned() {
            self.space.unmap_elem(&window);
        }
        self.windows.retain(|(w, _)| *w != id);
        self.tiler.remove(id);
        self.focus.remove(id);

        self.relayout();
        if let Some(next) = self.focus.focused() {
            self.focus_id(next);
        }
    }

    /// Give every window the geometry the layout assigns it.
    ///
    /// Every configure this compositor sends passes through here, which is
    /// why the size can never be zero: sicompass's `recreate_swapchain`
    /// loops on `size_in_pixels()` with a 16ms sleep and no exit condition,
    /// so a 0x0 configure would spin its main thread forever.
    pub fn relayout(&mut self) {
        if self.tiler.is_empty() {
            return;
        }
        let area = Rectangle::new((0, 0).into(), self.output_size());
        debug!(
            "relayout: {:?}, {} window(s), order {:?}",
            self.tiler.layout(),
            self.tiler.len(),
            self.tiler.order()
        );
        for (id, rect) in self.tiler.arrange(area) {
            let Some(window) = self.window(id).cloned() else {
                continue;
            };
            if let Some(toplevel) = window.toplevel() {
                toplevel.with_pending_state(|state| {
                    state.size = Some(rect.size);
                    state.bounds = Some(area.size);
                    // Tell the client its edges are not its own, which is the
                    // standard way to say "do not draw resize handles here".
                    state.states.set(xdg_toplevel::State::TiledLeft);
                    state.states.set(xdg_toplevel::State::TiledRight);
                    state.states.set(xdg_toplevel::State::TiledTop);
                    state.states.set(xdg_toplevel::State::TiledBottom);
                });
                toplevel.send_pending_configure();
            }
            self.space.map_element(window, rect.loc, false);
        }
    }

    /// Focus a window: mark it active, raise it, hand it the keyboard.
    pub fn focus_id(&mut self, id: WindowId) {
        let Some(target) = self.window(id).cloned() else {
            return;
        };

        let windows: Vec<(WindowId, Window)> = self.windows.clone();
        for (other_id, window) in &windows {
            if let Some(toplevel) = window.toplevel() {
                toplevel.with_pending_state(|state| {
                    if *other_id == id {
                        state.states.set(xdg_toplevel::State::Activated);
                    } else {
                        state.states.unset(xdg_toplevel::State::Activated);
                    }
                });
                toplevel.send_pending_configure();
            }
        }

        self.space.raise_element(&target, true);
        self.focus.push_front(id);

        if let Some(keyboard) = self.seat.get_keyboard() {
            let surface = target.toplevel().map(|t| t.wl_surface().clone());
            keyboard.set_focus(self, surface, Serial::from(0));
        }
    }

    /// Ask the focused window to close. The client decides what that means,
    /// and may refuse.
    fn close_focused(&mut self) {
        if let Some(toplevel) = self
            .focus
            .focused()
            .and_then(|id| self.window(id))
            .and_then(|w| w.toplevel())
        {
            toplevel.send_close();
        }
    }

    /// Launch the configured command as a new client.
    fn spawn(&self) {
        if self.spawn_cmd.is_empty() {
            warn!("spawn binding pressed but no command is configured");
            return;
        }
        info!("spawning {}", self.spawn_cmd);
        match std::process::Command::new("/bin/sh")
            .args(["-c", &self.spawn_cmd])
            .env("WAYLAND_DISPLAY", &self.socket_name)
            .env("SICOMPASS_SESSION", "1")
            // See the same call in main.rs: a client that inherits the host's
            // DISPLAY may render into the desktop we are nested in instead of
            // into us, silently.
            .env_remove("DISPLAY")
            .spawn()
        {
            Ok(child) => debug!("spawned pid {}", child.id()),
            Err(e) => error!("failed to spawn {:?}: {e}", self.spawn_cmd),
        }
    }

    /// Two-step quit: the first chord arms, the second ends the session.
    ///
    /// A single chord is too easy to hit by accident on a keyboard-only
    /// shell, where an accidental logout costs the user everything unsaved
    /// and there is no pointer to undo with.
    ///
    /// The arming is currently only logged. On a real session the user needs
    /// to *hear* it, which needs the announcement channel that arrives with
    /// the accessibility work.
    fn request_quit(&mut self) {
        if self.quit_armed {
            info!("quit confirmed");
            self.running = false;
            self.loop_signal.stop();
        } else {
            self.quit_armed = true;
            warn!("press the quit chord again to end the session");
        }
    }
}

// ---------------------------------------------------------------------------
// Keybinding handling
// ---------------------------------------------------------------------------

/// Process a compositor-level keybinding.
///
/// Returns `FilterResult::Intercept` when the compositor consumed the key and
/// `FilterResult::Forward` when it belongs to the focused client.
pub fn apply_keybinding(state: &mut State, mods: Mods, keysym: u32) -> FilterResult<()> {
    let action = keybindings::evaluate(mods, keysym);

    // Any binding other than the quit chord disarms a pending quit, so the
    // two presses have to be consecutive.
    if !matches!(action, BindingAction::Quit | BindingAction::PassThrough) {
        state.quit_armed = false;
    }

    let focused = state.focus.focused();

    match action {
        BindingAction::PassThrough => return FilterResult::Forward,

        BindingAction::FocusNext => {
            if let Some(next) = focused.and_then(|id| state.tiler.next(id)) {
                state.focus_id(next);
            }
        }
        BindingAction::FocusPrev => {
            if let Some(prev) = focused.and_then(|id| state.tiler.prev(id)) {
                state.focus_id(prev);
            }
        }
        BindingAction::FocusLeft => {
            if let Some(id) = focused.and_then(|id| state.tiler.neighbour(id, Dir::Left)) {
                state.focus_id(id);
            }
        }
        BindingAction::FocusRight => {
            if let Some(id) = focused.and_then(|id| state.tiler.neighbour(id, Dir::Right)) {
                state.focus_id(id);
            }
        }

        BindingAction::MoveNext => swap_focused_with(state, focused.and_then(|id| state.tiler.next(id))),
        BindingAction::MovePrev => swap_focused_with(state, focused.and_then(|id| state.tiler.prev(id))),
        BindingAction::MoveLeft => {
            swap_focused_with(state, focused.and_then(|id| state.tiler.neighbour(id, Dir::Left)))
        }
        BindingAction::MoveRight => {
            swap_focused_with(state, focused.and_then(|id| state.tiler.neighbour(id, Dir::Right)))
        }

        BindingAction::CycleRecent => {
            if let Some(id) = state.focus.cycle() {
                state.focus_id(id);
            }
        }

        BindingAction::ToggleLayout => {
            let layout = state.tiler.toggle_layout();
            info!("layout: {layout:?}");
            state.relayout();
            // Monocle stacks every window in the same place, so the focused
            // one has to come back to the top.
            if let Some(id) = state.focus.focused() {
                state.focus_id(id);
            }
        }

        BindingAction::CloseWindow => state.close_focused(),
        BindingAction::Spawn => state.spawn(),
        BindingAction::Quit => state.request_quit(),
    }

    FilterResult::Intercept(())
}

/// Exchange the focused window with `other`, then re-tile.
fn swap_focused_with(state: &mut State, other: Option<WindowId>) {
    let (Some(focused), Some(other)) = (state.focus.focused(), other) else {
        return;
    };
    if state.tiler.swap(focused, other) {
        state.relayout();
    }
}

// ---------------------------------------------------------------------------
// CompositorHandler
// ---------------------------------------------------------------------------

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);

        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self.window_for_surface(&root) {
                window.on_commit();
            }
        }

        ensure_initial_configure(self, surface);
        self.popups.commit(surface);
    }
}

/// Send the first configure for a freshly created toplevel or popup.
///
/// A client is not allowed to attach a buffer before it has been configured,
/// so until this runs the surface stays blank and nothing is ever rendered.
fn ensure_initial_configure(state: &mut State, surface: &WlSurface) {
    if let Some(window) = state.window_for_surface(surface) {
        if let Some(toplevel) = window.toplevel() {
            let already_sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .map(|data| data.lock().unwrap().initial_configure_sent)
                    .unwrap_or(true)
            });
            if !already_sent {
                let size = state.output_size();
                toplevel.with_pending_state(|s| {
                    s.size = Some(size);
                    s.bounds = Some(size);
                });
                toplevel.send_configure();
            }
        }
        return;
    }

    if let Some(PopupKind::Xdg(popup)) = state.popups.find_popup(surface) {
        let already_sent = with_states(surface, |states| {
            states
                .data_map
                .get::<smithay::wayland::shell::xdg::XdgPopupSurfaceData>()
                .map(|data| data.lock().unwrap().initial_configure_sent)
                .unwrap_or(true)
        });
        if !already_sent {
            // Cannot fail: the surface is not yet configured.
            let _ = popup.send_configure();
        }
    }
}

delegate_compositor!(State);

// ---------------------------------------------------------------------------
// XdgShellHandler
// ---------------------------------------------------------------------------

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface);
        self.add_window(window);
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.id_for_surface(surface.wl_surface()) {
            self.remove_window(id);
        }
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        if let Err(err) = self.popups.track_popup(PopupKind::Xdg(surface)) {
            debug!("failed to track popup: {err}");
        }
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {
        // Popup grabs need a pointer or a keyboard grab to be meaningful.
        // Revisited when the keyboard model lands.
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        surface.send_repositioned(token);
    }

    fn move_request(&mut self, surface: ToplevelSurface, _seat: WlSeat, _serial: Serial) {
        // Deliberately ignored, not overlooked. desicompass is cursorless and
        // the compositor owns every window's geometry, so there is nothing an
        // interactive move could do. sicompass asks for this from its
        // self-drawn titlebar via SDL's hit test.
        debug!(
            "ignoring xdg_toplevel.move from {:?}: compositor is cursorless and tiling",
            surface.wl_surface().id()
        );
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        _seat: WlSeat,
        _serial: Serial,
        _edges: xdg_toplevel::ResizeEdge,
    ) {
        debug!(
            "ignoring xdg_toplevel.resize from {:?}: compositor owns geometry",
            surface.wl_surface().id()
        );
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        // The default implementation configures whatever the client asked
        // for, which would let it size itself out of its tile. Answer with
        // our own geometry instead: every window is already maximised here.
        let size = self.output_size();
        surface.with_pending_state(|state| {
            state.size = Some(size);
            state.states.set(xdg_toplevel::State::Maximized);
        });
        surface.send_pending_configure();
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
    ) {
        let size = self.output_size();
        surface.with_pending_state(|state| {
            state.size = Some(size);
            state.states.set(xdg_toplevel::State::Fullscreen);
        });
        surface.send_pending_configure();
    }
}

delegate_xdg_shell!(State);

// ---------------------------------------------------------------------------
// ShmHandler
// ---------------------------------------------------------------------------

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

delegate_shm!(State);

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl smithay::wayland::output::OutputHandler for State {}

delegate_output!(State);

// ---------------------------------------------------------------------------
// SeatHandler
// ---------------------------------------------------------------------------

impl SeatHandler for State {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {}

    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {}
}

delegate_seat!(State);

// ---------------------------------------------------------------------------
// DataDevice (clipboard / DnD — required by many clients)
// ---------------------------------------------------------------------------

impl SelectionHandler for State {
    type SelectionUserData = ();
}

impl DataDeviceHandler for State {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for State {}

impl ServerDndGrabHandler for State {
    fn send(&mut self, _mime_type: String, _fd: std::os::unix::io::OwnedFd, _seat: Seat<Self>) {}
}

delegate_data_device!(State);

// ---------------------------------------------------------------------------
// Dmabuf
// ---------------------------------------------------------------------------

// Without `zwp_linux_dmabuf_v1` a hardware Vulkan client cannot present at
// all: Mesa's Wayland WSI only falls back to wl_shm for software drivers, so
// a real ICD reports the surface unsupported and the client dies. sicompass
// is exactly such a client, and so is vkcube, which segfaults outright rather
// than reporting it.
impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        // Import it now rather than at render time, so a client that hands us
        // a buffer we cannot use is told immediately instead of showing a
        // blank window.
        match self.backend.renderer().import_dmabuf(&dmabuf, None) {
            Ok(_) => {
                let _ = notifier.successful::<State>();
            }
            Err(err) => {
                debug!("rejecting client dmabuf: {err}");
                notifier.failed();
            }
        }
    }
}

delegate_dmabuf!(State);
