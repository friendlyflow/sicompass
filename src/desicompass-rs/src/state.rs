//! Compositor state — all Wayland protocol state in one struct, implementing
//! the smithay delegate traits.
//!
//! Mirrors the `tinywl_server` struct in `src/desicompass/main.c` but
//! expressed through smithay's typed API instead of raw wlroots calls.

use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_data_device, delegate_seat, delegate_shm, delegate_xdg_shell,
    input::{keyboard::FilterResult, pointer::CursorImageStatus, Seat, SeatHandler, SeatState},
    reexports::wayland_server::{
        backend::{ClientData, ClientId, DisconnectReason},
        protocol::{wl_buffer::WlBuffer, wl_seat::WlSeat, wl_surface::WlSurface},
        Client,
    },
    utils::Serial,
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorClientState, CompositorHandler, CompositorState},
        selection::{
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
            },
            SelectionHandler,
        },
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
        },
        shm::{ShmHandler, ShmState},
    },
};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;

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
    // ---- Protocol state ----
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub data_device_state: DataDeviceState,

    // ---- Input ----
    pub seat_state: SeatState<State>,
    pub seat: Seat<State>,

    // ---- Quit flag ----
    pub running: bool,
}

impl State {
    pub fn new(display: &smithay::reexports::wayland_server::DisplayHandle) -> Self {
        let compositor_state = CompositorState::new::<Self>(display);
        let xdg_shell_state = XdgShellState::new::<Self>(display);
        let shm_state = ShmState::new::<Self>(display, vec![]);
        let data_device_state = DataDeviceState::new::<Self>(display);
        let mut seat_state = SeatState::new();
        let seat = seat_state.new_wl_seat(display, "seat0");

        State {
            compositor_state,
            xdg_shell_state,
            shm_state,
            data_device_state,
            seat_state,
            seat,
            running: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Keybinding handling
// ---------------------------------------------------------------------------

/// Process a compositor-level keybinding.  Returns `FilterResult::Intercept`
/// when the compositor consumed the key, `FilterResult::Forward` otherwise.
///
/// Only triggered while Alt is held (Alt serves as the compositor modifier,
/// matching the C compositor's `handle_keybinding`).
pub fn apply_keybinding(state: &mut State, keysym: u32) -> FilterResult<()> {
    match keybindings::evaluate(keysym) {
        BindingAction::Quit => {
            state.running = false;
            FilterResult::Intercept(())
        }
        BindingAction::CycleWindows => {
            cycle_focus(state);
            FilterResult::Intercept(())
        }
        BindingAction::PassThrough => FilterResult::Forward,
    }
}

/// Rotate focus to the window that was least recently focused (Alt+F1).
///
/// The XDG shell state provides `toplevel_surfaces()` in map order.  We take
/// the last surface and give it keyboard focus.
fn cycle_focus(state: &mut State) {
    let surfaces: Vec<_> = state
        .xdg_shell_state
        .toplevel_surfaces()
        .iter()
        .cloned()
        .collect();
    if surfaces.len() < 2 {
        return;
    }
    // Give focus to the last (least recently active) toplevel.
    let target = surfaces.last().unwrap();
    let wl_surface = target.wl_surface().clone();
    let keyboard = state.seat.get_keyboard().unwrap();
    keyboard.set_focus(state, Some(wl_surface), Serial::from(0u32));
}

// ---------------------------------------------------------------------------
// BufferHandler
// ---------------------------------------------------------------------------

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
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
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Activated);
        });
        surface.send_configure();
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}

    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
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
    fn send(
        &mut self,
        _mime_type: String,
        _fd: std::os::unix::io::OwnedFd,
        _seat: Seat<Self>,
    ) {
    }
}

delegate_data_device!(State);
