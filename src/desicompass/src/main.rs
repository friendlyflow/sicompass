//! desicompass — Wayland compositor entry point.
//!
//! Rust port of `src/desicompass-c/main.c` using smithay instead of wlroots.
//! Linux-only.
//!
//! Unlike the tinywl original this compositor is **cursorless**: it advertises
//! no `wl_pointer`, windows are placed by the compositor, and every
//! interaction is a key.
//!
//! ## Keybindings (Super held)
//!
//! Super, not Alt: sicompass consumes Alt itself, and the right Alt is AltGr
//! on most non-US layouts. See [`keybindings`] for the full reasoning.
//!
//! | Binding | Action |
//! |---|---|
//! | `Super+J` / `Super+K` | focus next / previous window |
//! | `Super+H` / `Super+L` | focus left / right |
//! | `Super+Shift+{H,J,K,L}` | move the focused window |
//! | `Super+Tab` | focus the least recently used window |
//! | `Super+M` | toggle Columns / Monocle |
//! | `Super+Return` | spawn the terminal |
//! | `Super+Shift+Q` | close the focused window |
//! | `Super+Shift+E` | end the session (press twice) |

#[cfg(target_os = "linux")]
mod focus;
#[cfg(target_os = "linux")]
mod gpu;
#[cfg(target_os = "linux")]
mod keybindings;
#[cfg(target_os = "linux")]
mod layout;
#[cfg(target_os = "linux")]
mod state;
#[cfg(all(target_os = "linux", feature = "tty"))]
mod tty;

#[cfg(target_os = "linux")]
mod linux {
    use std::{sync::Arc, time::Duration};

    use clap::Parser;
    use smithay::{
        backend::{
            input::{InputEvent, KeyboardKeyEvent},
            renderer::{
                damage::OutputDamageTracker,
                element::surface::WaylandSurfaceRenderElement,
                gles::GlesRenderer,
            },
            winit::{self, WinitEvent},
        },
        desktop::space::render_output,
        backend::{
            egl::EGLDevice,
            renderer::ImportDma,
        },
        input::keyboard::{FilterResult, XkbConfig},
        wayland::dmabuf::DmabufFeedbackBuilder,
        output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
        reexports::{
            calloop::{
                generic::Generic, EventLoop, Interest, Mode as CalloopMode, PostAction,
            },
            wayland_server::Display,
        },
        utils::Transform,
        wayland::socket::ListeningSocketSource,
    };
    use tracing::{error, info, warn};

    use crate::gpu::{BackendChoice, Gpu, ResolvedBackend};
    use crate::keybindings::Mods;
    use crate::state::{ClientState, State};

    // -----------------------------------------------------------------------
    // CLI
    // -----------------------------------------------------------------------

    /// desicompass Wayland compositor.
    #[derive(Parser, Debug)]
    #[command(version, about)]
    struct Args {
        /// Command to execute after the compositor starts (like tinywl -s).
        #[arg(short = 's', long)]
        startup_cmd: Option<String>,

        /// XKB layout handed to every client, e.g. `be`, `us`, `fr`.
        ///
        /// Defaults to XKB_DEFAULT_LAYOUT, then to `us`. This is not
        /// cosmetic: the compositor owns the keymap, so getting it wrong
        /// leaves every client on a US layout no matter what the console or
        /// the desktop is set to, and on a Belgian keyboard that costs you
        /// `@`, `#`, `[`, `]`, `{` and `}`.
        #[arg(long)]
        xkb_layout: Option<String>,

        /// XKB variant to pair with the layout.
        #[arg(long)]
        xkb_variant: Option<String>,

        /// Command launched by the spawn binding (Super+Return).
        #[arg(long, default_value = "foot")]
        terminal: String,

        /// Which backend to run: nested in a session, or the real display.
        ///
        /// `auto` nests when WAYLAND_DISPLAY or DISPLAY is set and takes the
        /// display otherwise, which is what lets one fixed command line work
        /// both from a desktop while developing and from a TTY at boot.
        #[arg(long, value_enum, default_value_t = BackendChoice::Auto)]
        backend: BackendChoice,
    }

    pub fn main_impl() -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
            tracing_subscriber::fmt().with_env_filter(env_filter).init();
        } else {
            tracing_subscriber::fmt().init();
        }

        let args = Args::parse();
        run(args)
    }

    fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
        match args
            .backend
            .resolve(|name| std::env::var_os(name).is_some())
        {
            ResolvedBackend::Winit => run_winit(args),
            #[cfg(feature = "tty")]
            ResolvedBackend::Tty => crate::tty::run(args_to_tty(&args)),
            #[cfg(not(feature = "tty"))]
            ResolvedBackend::Tty => Err(
                "this build has no TTY backend: rebuild with `--features tty`, \
                 or pass `--backend winit` to run nested in an existing session"
                    .into(),
            ),
        }
    }

    /// The shared settings both backends need, so the TTY module does not
    /// have to depend on the `Args` type.
    #[cfg(feature = "tty")]
    fn args_to_tty(args: &Args) -> crate::tty::TtyArgs {
        crate::tty::TtyArgs {
            startup_cmd: args.startup_cmd.clone(),
            xkb_layout: args.xkb_layout.clone(),
            xkb_variant: args.xkb_variant.clone(),
            terminal: args.terminal.clone(),
        }
    }

    fn run_winit(args: Args) -> Result<(), Box<dyn std::error::Error>> {
        let mut event_loop: EventLoop<State> = EventLoop::try_new()?;
        let display: Display<State> = Display::new()?;
        let dh = display.handle();

        // Winit backend first.
        //
        // This must happen before the socket name reaches any child's
        // environment. winit reads WAYLAND_DISPLAY to find the host
        // compositor to open its window on, so pointing that at our own
        // socket makes winit connect to us — and we only start serving
        // clients once this returns. The compositor then looks healthy,
        // process alive and socket present, and answers nobody.
        let (backend, mut winit) = winit::init::<GlesRenderer>()?;

        // One output, sized to the winit window.
        //
        // An output global is not optional even nested: a client built on
        // smithay-client-toolkit (loginsicompass is) waits on `wl_output`
        // before it will draw anything at all.
        let mode = Mode {
            size: backend.window_size(),
            refresh: 60_000,
        };
        let output = Output::new(
            "desicompass-0".to_string(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "desicompass".into(),
                model: "winit".into(),
            },
        );
        let _output_global = output.create_global::<State>(&dh);
        // Flipped180 is what the winit backend renders with; the output has
        // to agree or everything lands upside down.
        output.change_current_state(
            Some(mode),
            Some(Transform::Flipped180),
            Some(Scale::Integer(1)),
            Some((0, 0).into()),
        );
        output.set_preferred(mode);

        let mut damage_tracker = OutputDamageTracker::from_output(&output);
        let mut state = State::new(&dh, event_loop.get_signal(), output.clone(), Gpu::Winit(backend));

        // Advertise zwp_linux_dmabuf_v1 with per-surface feedback.
        //
        // This is what lets a hardware Vulkan client present. Mesa's Wayland
        // WSI only falls back to wl_shm for software drivers, so without this
        // global a real ICD reports every surface unsupported: vkcube
        // segfaults and sicompass exits. Version 4 (what
        // `create_global_with_default_feedback` creates) covers both the
        // modifier negotiation wsi_wl wants from v3 and the per-surface
        // feedback it wants from v4.
        //
        // The device we name has to be the one the client will allocate on,
        // which on a single-GPU machine is trivially the same node we render
        // with. On a hybrid laptop this is the first thing to get wrong.
        let render_node = EGLDevice::device_for_display(state.backend.renderer().egl_context().display())
            .ok()
            .and_then(|device| device.try_get_render_node().ok().flatten());
        match render_node {
            Some(node) => {
                let formats: Vec<_> = state.backend.renderer().dmabuf_formats().iter().copied().collect();
                info!(
                    "advertising zwp_linux_dmabuf_v1 on {:?} with {} formats",
                    node.dev_path().unwrap_or_default(),
                    formats.len()
                );
                let feedback = DmabufFeedbackBuilder::new(node.dev_id(), formats).build()?;
                let _dmabuf_global = state
                    .dmabuf_state
                    .create_global_with_default_feedback::<State>(&dh, &feedback);
            }
            // Not fatal: shm clients still work, which is most of the test
            // suite. Hardware Vulkan clients will not.
            None => warn!("no DRM render node found; dmabuf is unavailable and GPU clients cannot present"),
        }

        // The keymap the compositor compiles is the keymap every client
        // gets; `XkbConfig::default()` means US, silently, on any machine.
        let layout = args
            .xkb_layout
            .clone()
            .or_else(|| std::env::var("XKB_DEFAULT_LAYOUT").ok())
            .unwrap_or_default();
        let variant = args
            .xkb_variant
            .clone()
            .or_else(|| std::env::var("XKB_DEFAULT_VARIANT").ok())
            .unwrap_or_default();
        let xkb_config = XkbConfig {
            layout: &layout,
            variant: &variant,
            ..Default::default()
        };
        info!(
            "xkb layout={:?} variant={:?}",
            if layout.is_empty() { "us (default)" } else { &layout },
            variant
        );
        let keyboard = state.seat.add_keyboard(xkb_config, 200, 25)?;
        // No `add_pointer()`. A seat with no wl_pointer is the honest
        // advertisement of a cursorless compositor, and it means a client's
        // pointer-driven paths (sicompass's SDL hit test, for one) can never
        // fire in the first place.

        // Wayland socket, as a calloop source.
        //
        // `new_auto` picks the first free wayland-N rather than a fixed name,
        // so a leftover socket file from a killed run cannot block startup.
        let socket = ListeningSocketSource::new_auto()?;
        let socket_name = socket.socket_name().to_string_lossy().into_owned();
        event_loop
            .handle()
            .insert_source(socket, |stream, _, state: &mut State| {
                if let Err(err) = state
                    .display_handle
                    .insert_client(stream, Arc::new(ClientState::default()))
                {
                    warn!("failed to insert client: {err}");
                }
            })?;

        // Client requests, as a calloop source.
        event_loop.handle().insert_source(
            Generic::new(display, Interest::READ, CalloopMode::Level),
            |_, display, state: &mut State| {
                // SAFETY: the display is never dropped or moved out of the
                // source for as long as the loop runs.
                unsafe {
                    display.get_mut().dispatch_clients(state)?;
                }
                Ok(PostAction::Continue)
            },
        )?;

        info!("WAYLAND_DISPLAY={socket_name}");
        state.socket_name = socket_name.clone();
        state.spawn_cmd = args.terminal.clone();

        // Optionally launch a startup program.
        //
        // The socket name is handed to the child explicitly instead of being
        // exported into our own environment: a process-wide `set_var` is
        // `unsafe` under edition 2024 and would repoint every library in
        // *this* process at our socket.
        if let Some(cmd) = &args.startup_cmd {
            info!("launching startup command: {cmd}");
            match std::process::Command::new("/bin/sh")
                .args(["-c", cmd])
                .env("WAYLAND_DISPLAY", &socket_name)
                // sicompass checks this and drops its self-drawn titlebar,
                // which is unreachable here: no pointer exists to click it.
                // Any other client ignores it.
                .env("SICOMPASS_SESSION", "1")
                // Drop the *host* session's DISPLAY. Without this a
                // toolkit that can speak both protocols may quietly pick X11
                // and render into the desktop we are nested in, instead of
                // into us: the client looks healthy, the compositor stays
                // empty, and nothing anywhere reports an error. SDL3 does
                // exactly this. On a real TTY session there is no DISPLAY to
                // begin with, so this only ever matters while developing
                // nested - which is when it costs the most time.
                .env_remove("DISPLAY")
                .spawn()
            {
                Ok(child) => info!("startup command running as pid {}", child.id()),
                // Never silent: on a bare TTY a startup command that failed to
                // exec is a black screen with no other diagnosis available.
                Err(e) => error!("startup command {cmd:?} failed to start: {e}"),
            }
        }

        let mut running = true;
        while running {
            let status = winit.dispatch_new_events(|event| match event {
                WinitEvent::Resized { size, .. } => {
                    let mode = Mode {
                        size,
                        refresh: 60_000,
                    };
                    state.output.change_current_state(Some(mode), None, None, None);
                    state.output.set_preferred(mode);
                    state.relayout();
                }
                WinitEvent::Input(InputEvent::Keyboard { event }) => {
                    keyboard.input(
                        &mut state,
                        event.key_code(),
                        event.state(),
                        0.into(),
                        0,
                        |app_state, modifiers, keysym| {
                            // The *Latin* sym for the physical key, not the
                            // modified one: Shift would turn `j` into `J`,
                            // and a non-Latin layout into something else
                            // again, so matching the modified sym would make
                            // every binding layout-dependent.
                            let Some(sym) = keysym.raw_latin_sym_or_raw_current_sym() else {
                                return FilterResult::Forward;
                            };
                            let mods = Mods {
                                logo: modifiers.logo,
                                shift: modifiers.shift,
                            };
                            crate::state::apply_keybinding(app_state, mods, sym.raw())
                        },
                    );
                }
                WinitEvent::CloseRequested => {
                    running = false;
                }
                _ => {}
            });

            if let smithay::reexports::winit::platform::pump_events::PumpStatus::Exit(_) = status {
                break;
            }

            // ---- Render ----
            //
            // Destructured rather than reached through `state.`, so the
            // renderer (&mut) and the space (&) are borrows of two disjoint
            // fields instead of two borrows of the whole struct.
            {
                let State {
                    backend,
                    space,
                    output,
                    start_time,
                    ..
                } = &mut state;

                // Irrefutable without the `tty` feature, where `Gpu` has a
                // single variant — but not with it.
                #[allow(irrefutable_let_patterns)]
                let Gpu::Winit(backend) = backend
                else {
                    unreachable!("run_winit only ever builds Gpu::Winit")
                };
                let age = backend.buffer_age().unwrap_or(0);
                let render_result = {
                    let (renderer, mut framebuffer) = backend.bind()?;
                    render_output::<_, WaylandSurfaceRenderElement<GlesRenderer>, _, _>(
                        output,
                        renderer,
                        &mut framebuffer,
                        1.0,
                        age,
                        [&*space],
                        &[],
                        &mut damage_tracker,
                        [0.0, 0.0, 0.0, 1.0],
                    )
                    .map(|res| res.damage.cloned())
                };

                match render_result {
                    Ok(damage) => {
                        backend.submit(damage.as_deref())?;

                        // Frame callbacks. Without these a client draws
                        // exactly one frame and then waits forever for
                        // permission to draw the next — the compositor looks
                        // like it has frozen every window on screen.
                        let now = start_time.elapsed();
                        let out = output.clone();
                        space.elements().for_each(|window| {
                            window.send_frame(&out, now, Some(Duration::ZERO), |_, _| {
                                Some(out.clone())
                            });
                        });
                    }
                    Err(err) => error!("render failed: {err}"),
                }
            }

            state.space.refresh();
            state.popups.cleanup();
            state.display_handle.flush_clients()?;

            // Wayland sources: new clients and client requests.
            event_loop.dispatch(Some(Duration::from_millis(16)), &mut state)?;

            // The quit binding clears this from inside a key handler.
            if !state.running {
                running = false;
            }
        }

        info!("desicompass exiting");
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    return linux::main_impl();

    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("desicompass is Linux-only (Wayland compositor)");
        std::process::exit(1);
    }
}
