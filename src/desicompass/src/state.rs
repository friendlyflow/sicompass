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
    delegate_compositor, delegate_data_device, delegate_output, delegate_seat, delegate_shm,
    delegate_xdg_shell,
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
    utils::{Logical, Serial, Size},
    wayland::{
        buffer::BufferHandler,
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
use tracing::debug;

use crate::keybindings::{self, BindingAction};

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
}

impl State {
    pub fn new(display: &DisplayHandle, loop_signal: LoopSignal, output: Output) -> Self {
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

    /// Give every mapped window the whole output.
    ///
    /// Phase 2 is deliberately monocle: one window fills the screen and the
    /// rest sit behind it. The real layout arrives with `layout.rs`; what
    /// matters here is that a configure is always sent and is always
    /// non-zero.
    pub fn relayout(&mut self) {
        let size = self.output_size();
        let windows: Vec<Window> = self.space.elements().cloned().collect();
        for window in windows {
            if let Some(toplevel) = window.toplevel() {
                toplevel.with_pending_state(|state| {
                    state.size = Some(size);
                    state.bounds = Some(size);
                    // Tell the client its edges are not its own. Without
                    // this a well-behaved client assumes it may pick its own
                    // size and draws decorations for edges it cannot move.
                    state.states.set(xdg_toplevel::State::TiledLeft);
                    state.states.set(xdg_toplevel::State::TiledRight);
                    state.states.set(xdg_toplevel::State::TiledTop);
                    state.states.set(xdg_toplevel::State::TiledBottom);
                });
                toplevel.send_pending_configure();
            }
            self.space.map_element(window, (0, 0), false);
        }
    }

    /// Focus `window`: raise it, mark it active, hand it the keyboard.
    pub fn focus_window(&mut self, window: &Window) {
        for other in self.space.elements() {
            if let Some(toplevel) = other.toplevel() {
                let active = other == window;
                toplevel.with_pending_state(|state| {
                    if active {
                        state.states.set(xdg_toplevel::State::Activated);
                    } else {
                        state.states.unset(xdg_toplevel::State::Activated);
                    }
                });
            }
        }
        let pending: Vec<Window> = self.space.elements().cloned().collect();
        for w in pending {
            if let Some(toplevel) = w.toplevel() {
                toplevel.send_pending_configure();
            }
        }

        self.space.raise_element(window, true);
        if let Some(keyboard) = self.seat.get_keyboard() {
            let surface = window.toplevel().map(|t| t.wl_surface().clone());
            keyboard.set_focus(self, surface, Serial::from(0));
        }
    }
}

// ---------------------------------------------------------------------------
// Keybinding handling
// ---------------------------------------------------------------------------

/// Process a compositor-level keybinding.  Returns `FilterResult::Intercept`
/// when the compositor consumed the key, `FilterResult::Forward` otherwise.
pub fn apply_keybinding(state: &mut State, keysym: u32) -> FilterResult<()> {
    match keybindings::evaluate(keysym) {
        BindingAction::Quit => {
            state.running = false;
            state.loop_signal.stop();
            FilterResult::Intercept(())
        }
        BindingAction::CycleWindows => {
            cycle_focus(state);
            FilterResult::Intercept(())
        }
        BindingAction::PassThrough => FilterResult::Forward,
    }
}

/// Rotate focus to the window that was least recently focused.
fn cycle_focus(state: &mut State) {
    // `elements()` is bottom-to-top, so the first entry is the window that
    // has been buried longest.
    let target = state.space.elements().next().cloned();
    if state.space.elements().len() < 2 {
        return;
    }
    if let Some(window) = target {
        state.focus_window(&window);
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
        self.space.map_element(window.clone(), (0, 0), false);
        self.relayout();
        self.focus_window(&window);
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for_surface(surface.wl_surface()) {
            self.space.unmap_elem(&window);
        }
        self.relayout();
        // Hand focus to whatever is left on top.
        if let Some(next) = self.space.elements().last().cloned() {
            self.focus_window(&next);
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
