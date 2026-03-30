//! SCTK application state — all Wayland protocol state + login logic.
//!
//! Implements the smithay-client-toolkit delegate traits for the login window,
//! keyboard input, seat, and output handling.
//!
//! The authentication flow mirrors `main.c` in `src/loginsicompass/`:
//!   create_session → auth_message → post_response → start_session → exit.

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_registry, delegate_seat,
    delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Modifiers},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        xdg::{
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgShell,
        },
        WaylandSurface,
    },
    shm::{
        slot::{Buffer, SlotPool},
        Shm, ShmHandler,
    },
};
use wayland_client::{
    globals::GlobalList,
    protocol::{wl_keyboard, wl_output, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use xkeysym::Keysym;

use crate::{
    entry::{InputMode, PasswordEntry},
    greetd::{AuthMessageType, GreetdClient, Response},
    renderer::{render_frame, RenderConfig},
};

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

pub struct AppState {
    // ---- SCTK state ----
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub shm: Shm,
    pub compositor_state: CompositorState,
    pub xdg_shell: XdgShell,

    // ---- Window ----
    pub window: Window,
    pub pool: SlotPool,
    pub buffer: Option<Buffer>,
    pub width: u32,
    pub height: u32,
    pub first_configure: bool,

    // ---- Keyboard ----
    pub keyboard: Option<wl_keyboard::WlKeyboard>,

    // ---- Render config (set from CLI) ----
    pub render_config: RenderConfig,

    // ---- Login state ----
    pub entry: PasswordEntry,
    pub greetd: Option<GreetdClient>,
    pub username: String,
    pub command: String,

    /// Set to `true` when Enter is pressed — auth submission happens in the
    /// next event loop iteration outside the keyboard handler.
    pub submit_pending: bool,

    /// Set to `true` when greetd signals the session should start (exit loop).
    pub exit: bool,

    /// Error message to display (cleared on next keypress).
    pub error: Option<String>,
}

impl AppState {
    pub fn new(
        conn: &Connection,
        globals: &GlobalList,
        qh: &QueueHandle<Self>,
        render_config: RenderConfig,
        username: String,
        command: String,
    ) -> Self {
        let compositor_state =
            CompositorState::bind(globals, qh).expect("wl_compositor not available");
        let xdg_shell = XdgShell::bind(globals, qh).expect("xdg_shell not available");
        let shm = Shm::bind(globals, qh).expect("wl_shm not available");

        let surface = compositor_state.create_surface(qh);
        let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, qh);
        window.set_title("loginsicompass");
        window.set_app_id("io.sicompass.login");
        window.commit();

        let w = render_config.width;
        let h = render_config.height;
        let pool = SlotPool::new((w * h * 4) as usize, &shm).expect("failed to create shm pool");

        AppState {
            registry_state: RegistryState::new(globals),
            seat_state: SeatState::new(globals, qh),
            output_state: OutputState::new(globals, qh),
            shm,
            compositor_state,
            xdg_shell,
            window,
            pool,
            buffer: None,
            width: w,
            height: h,
            first_configure: true,
            keyboard: None,
            render_config,
            entry: PasswordEntry::new(),
            greetd: None,
            username,
            command,
            submit_pending: false,
            exit: false,
            error: None,
        }
    }

    /// Draw the current frame using the software renderer and attach it to the window.
    pub fn draw(&mut self, qh: &QueueHandle<Self>) {
        let w = self.width;
        let h = self.height;

        if w == 0 || h == 0 {
            return;
        }

        // Resize the pool if needed.
        let needed = (w * h * 4) as usize;
        if self.pool.len() < needed {
            self.pool.resize(needed).ok();
        }

        let (buffer, canvas) = self
            .pool
            .create_buffer(
                w as i32,
                h as i32,
                (w * 4) as i32,
                wl_shm::Format::Argb8888,
            )
            .expect("failed to create buffer");

        // Render.
        let mut cfg = self.render_config.clone();
        cfg.width = w;
        cfg.height = h;

        let pixels = render_frame(&cfg, &self.entry);
        for (dst, src) in canvas.chunks_exact_mut(4).zip(pixels.iter()) {
            // wl_shm ARGB8888 on little-endian is stored as [B, G, R, A].
            dst[0] = (*src & 0xFF) as u8;           // B
            dst[1] = ((*src >> 8) & 0xFF) as u8;    // G
            dst[2] = ((*src >> 16) & 0xFF) as u8;   // R
            dst[3] = ((*src >> 24) & 0xFF) as u8;   // A
        }

        self.window.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
        self.window
            .wl_surface()
            .damage_buffer(0, 0, w as i32, h as i32);
        self.window.wl_surface().commit();
        self.buffer = Some(buffer);
    }

    /// Submit the password to greetd and handle the response.
    ///
    /// Mirrors `post_auth_message_response` + `handle_response` in the C version.
    pub fn submit(&mut self) {
        let Some(greetd) = self.greetd.as_mut() else {
            return;
        };
        let password = self.entry.as_string();
        self.entry.clear();

        let resp = match greetd.post_auth_message_response(Some(&password)) {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(format!("IPC error: {e}"));
                return;
            }
        };

        self.handle_response(resp);
    }

    pub fn handle_response_pub(&mut self, resp: Response) {
        self.handle_response(resp);
    }

    fn handle_response(&mut self, resp: Response) {
        let Some(greetd) = self.greetd.as_mut() else {
            return;
        };
        match resp {
            Response::Success => {
                // Auth passed — start the session.
                let command = self.command.clone();
                match greetd.start_session(&command) {
                    Ok(Response::Success) => {
                        self.exit = true;
                    }
                    Ok(Response::Error { description, .. }) => {
                        self.error = Some(description);
                        self.restart_session();
                    }
                    Ok(other) => {
                        self.error = Some(format!("unexpected: {other:?}"));
                    }
                    Err(e) => {
                        self.error = Some(format!("IPC error: {e}"));
                    }
                }
            }
            Response::Error { description, .. } => {
                self.error = Some(description);
                self.restart_session();
            }
            Response::AuthMessage {
                auth_message_type,
                auth_message: _,
            } => {
                match auth_message_type {
                    AuthMessageType::Secret => {
                        self.entry.mode = InputMode::Secret;
                    }
                    AuthMessageType::Visible => {
                        self.entry.mode = InputMode::Visible;
                    }
                    AuthMessageType::Info | AuthMessageType::Error => {
                        // Acknowledge info/error messages with a null response.
                        let ack_resp = greetd.post_auth_message_response(None);
                        if let Ok(r) = ack_resp {
                            self.handle_response(r);
                        }
                    }
                }
            }
        }
    }

    fn restart_session(&mut self) {
        let Some(greetd) = self.greetd.as_mut() else {
            return;
        };
        let username = self.username.clone();
        let _ = greetd.cancel_session();
        let resp = greetd.create_session(&username);
        if let Ok(r) = resp {
            self.handle_response(r);
        }
    }
}

// ---------------------------------------------------------------------------
// SCTK delegate implementations
// ---------------------------------------------------------------------------

impl CompositorHandler for AppState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.draw(qh);
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

delegate_compositor!(AppState);

// ---- Output ----

impl OutputHandler for AppState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

delegate_output!(AppState);

// ---- Seat ----

impl SeatHandler for AppState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = Some(
                self.seat_state
                    .get_keyboard(qh, &seat, None)
                    .expect("failed to get keyboard"),
            );
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(kbd) = self.keyboard.take() {
                kbd.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

delegate_seat!(AppState);

// ---- Keyboard ----

impl KeyboardHandler for AppState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.error = None;
        match event.keysym {
            Keysym::Return | Keysym::KP_Enter => {
                self.submit_pending = true;
            }
            Keysym::BackSpace => {
                self.entry.backspace();
                self.draw(qh);
            }
            Keysym::Escape => {
                self.entry.clear();
                self.draw(qh);
            }
            _ => {
                if let Some(s) = &event.utf8 {
                    for ch in s.chars().filter(|c| !c.is_control()) {
                        self.entry.push(ch);
                    }
                    self.draw(qh);
                }
            }
        }
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: Modifiers,
        _layout: u32,
    ) {
    }
}

delegate_keyboard!(AppState);

// ---- Shm ----

impl ShmHandler for AppState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_shm!(AppState);

// ---- XDG shell ----

impl WindowHandler for AppState {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        if let Some(w) = configure.new_size.0 {
            self.width = w.get();
        }
        if let Some(h) = configure.new_size.1 {
            self.height = h.get();
        }

        if self.first_configure {
            self.first_configure = false;
            self.draw(qh);
        }
    }
}

delegate_xdg_shell!(AppState);
delegate_xdg_window!(AppState);

// ---- Registry ----

impl ProvidesRegistryState for AppState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

delegate_registry!(AppState);
