//! desicompass — Wayland compositor entry point.
//!
//! Rust port of `src/desicompass/main.c` using smithay instead of wlroots.
//! Linux-only.
//!
//! ## Keybindings (Alt held)
//! * `Alt+Esc`  — quit the compositor
//! * `Alt+F1`   — cycle to the next window

#[cfg(target_os = "linux")]
mod focus;
#[cfg(target_os = "linux")]
mod keybindings;
#[cfg(target_os = "linux")]
mod state;

#[cfg(target_os = "linux")]
mod linux {
    use clap::Parser;
    use smithay::{
        backend::{
            input::{InputEvent, KeyboardKeyEvent},
            renderer::{gles::GlesRenderer, Color32F, Frame, Renderer},
            winit::{self, WinitEvent},
        },
        input::keyboard::FilterResult,
        reexports::{
            wayland_server::{Display, ListeningSocket},
            winit::platform::pump_events::PumpStatus,
        },
        utils::{Rectangle, Transform},
    };
    use state::{ClientState, State};
    use std::sync::Arc;
    use tracing::info;

    // ---------------------------------------------------------------------------
    // CLI
    // ---------------------------------------------------------------------------

    /// desicompass Wayland compositor.
    #[derive(Parser, Debug)]
    #[command(version, about)]
    struct Args {
        /// Command to execute after the compositor starts (like tinywl -s).
        #[arg(short = 's', long)]
        startup_cmd: Option<String>,
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
        let mut display: Display<State> = Display::new()?;
        let dh = display.handle();

        let mut state = State::new(&dh);

        // Add keyboard and pointer to the seat.
        let keyboard = state.seat.add_keyboard(Default::default(), 200, 25)?;
        state.seat.add_pointer();

        // Open the Wayland socket.
        let socket_name = "wayland-desicompass";
        let listener = ListeningSocket::bind(socket_name)?;
        std::env::set_var("WAYLAND_DISPLAY", socket_name);
        info!("WAYLAND_DISPLAY={socket_name}");

        // Optionally launch a startup program.
        if let Some(cmd) = &args.startup_cmd {
            info!("launching startup command: {cmd}");
            std::process::Command::new("/bin/sh")
                .args(["-c", cmd])
                .spawn()
                .ok();
        }

        // Winit backend: creates an OS window we render into.
        let (mut backend, mut winit) = winit::init::<GlesRenderer>()?;
        let start_time = std::time::Instant::now();

        while state.running {
            let status = winit.dispatch_new_events(|event| match event {
                WinitEvent::Resized { .. } => {}
                WinitEvent::Input(input_event) => match input_event {
                    InputEvent::Keyboard { event } => {
                        use smithay::backend::input::KeyboardKeyEvent;
                        keyboard.input(
                            &mut state,
                            event.key_code(),
                            event.state(),
                            0.into(),
                            0,
                            |app_state, modifiers, keysym| {
                                if modifiers.alt {
                                    let sym = keysym.modified_sym().raw();
                                    return state::apply_keybinding(app_state, sym);
                                }
                                FilterResult::Forward
                            },
                        );
                    }
                    InputEvent::PointerMotionAbsolute { .. } => {
                        // Focus the first available toplevel on pointer activity.
                        if let Some(surface) = state
                            .xdg_shell_state
                            .toplevel_surfaces()
                            .iter()
                            .next()
                            .cloned()
                        {
                            let wl_surface = surface.wl_surface().clone();
                            keyboard.set_focus(&mut state, Some(wl_surface), 0.into());
                        }
                    }
                    _ => {}
                },
                WinitEvent::CloseRequested => {
                    state.running = false;
                }
                _ => {}
            });

            match status {
                PumpStatus::Continue => {}
                PumpStatus::Exit(_) => break,
            }

            // ---- Render frame ----
            let size = backend.window_size();
            let damage = Rectangle::from_size(size);

            {
                let (renderer, mut framebuffer) = backend.bind()?;
                let mut frame = renderer.render(&mut framebuffer, size, Transform::Flipped180)?;
                // Black background.
                frame.clear(Color32F::new(0.0, 0.0, 0.0, 1.0), &[damage])?;
                frame.finish()?;
            }

            // Accept new Wayland clients.
            if let Some(stream) = listener.accept()? {
                info!("new client connected");
                display
                    .handle()
                    .insert_client(stream, Arc::new(ClientState::default()))
                    .unwrap();
            }

            // Dispatch + flush Wayland events.
            display.dispatch_clients(&mut state)?;
            display.flush_clients()?;

            // Submit the rendered frame.
            backend.submit(Some(&[damage]))?;
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
