//! The TTY backend: DRM/KMS for output, libinput for input, libseat for the
//! seat.
//!
//! This is what makes desicompass a session rather than a window. It takes
//! the display directly instead of nesting in someone else's compositor, and
//! it is what runs when you log in on a bare VT.
//!
//! ## Scope
//!
//! Deliberately one GPU and one connector. A laptop with an external screen,
//! or hotplugging a monitor, needs a `UdevBackend` source and a compositor
//! per CRTC; that is a larger piece of work and it is not on the path to
//! "log in on tty2 and see sicompass". Both simplifications are noted at
//! their sites.
//!
//! ## Getting a seat
//!
//! `LibSeatSession` asks logind over D-Bus for the devices, which is why
//! nothing here needs the user to be in the `video` group and why `seatd`
//! the daemon does not have to be running. What it *does* need is a real
//! logind session that is **active**: log in at the VT's own prompt. Running
//! this from a terminal inside a graphical session inherits that session,
//! which already holds DRM master, and the device open fails.
//!
//! ## Switching away
//!
//! Handling `SessionEvent::PauseSession` / `ActivateSession` is not optional.
//! When the user presses Ctrl+Alt+F1, logind revokes the device; a compositor
//! that keeps rendering into a revoked DRM master leaves a wedged VT that can
//! only be recovered by killing it blind from another machine.

use std::time::Duration;

use smithay::{
    backend::{
        allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        drm::{
            compositor::{DrmCompositor, FrameFlags},
            exporter::gbm::GbmFramebufferExporter,
            DrmDevice, DrmDeviceFd, DrmEvent, DrmNode,
        },
        egl::{EGLContext, EGLDevice, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            damage::OutputDamageTracker, element::surface::WaylandSurfaceRenderElement,
            gles::GlesRenderer, ImportDma, ImportEgl,
        },
        session::{libseat::LibSeatSession, Event as SessionEvent, Session},
        udev::primary_gpu,
    },
    desktop::space::{space_render_elements, SpaceRenderElements},
    input::keyboard::XkbConfig,
    output::{Mode, Output, OutputModeSource, PhysicalProperties, Scale, Subpixel},
    reexports::{
        calloop::{
            generic::Generic, EventLoop, Interest, Mode as CalloopMode, PostAction,
        },
        drm::control::{connector, crtc, Device as _, ModeTypeFlags},
        input::Libinput,
        rustix::fs::OFlags,
        wayland_server::Display,
    },
    utils::{DeviceFd, Transform},
    wayland::dmabuf::DmabufFeedbackBuilder,
};
use tracing::{debug, error, info, warn};

use crate::{
    gpu::Gpu,
    keybindings,
    state::{ClientState, State},
};

/// The subset of the command line the TTY backend needs.
pub struct TtyArgs {
    pub startup_cmd: Option<String>,
    pub xkb_layout: Option<String>,
    pub xkb_variant: Option<String>,
    pub terminal: String,
}

/// The DRM side, owned by [`crate::gpu::Gpu::Tty`].
pub struct TtyGpu {
    /// Kept here so the keyboard handler can reach it: VT switching is a
    /// session operation and the handler is only handed `&mut State`.
    session: LibSeatSession,
    renderer: GlesRenderer,
    compositor: GbmDrmCompositor,
    damage_tracker: OutputDamageTracker,
    /// Cleared while the session is in the background. Rendering into a
    /// revoked DRM master is what wedges a VT.
    active: bool,
}

/// The concrete `DrmCompositor` this backend uses.
type GbmDrmCompositor = DrmCompositor<
    GbmAllocator<DrmDeviceFd>,
    GbmFramebufferExporter<DrmDeviceFd>,
    (),
    DrmDeviceFd,
>;

impl TtyGpu {
    pub fn renderer(&mut self) -> &mut GlesRenderer {
        &mut self.renderer
    }
}

pub fn run(args: TtyArgs) -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop: EventLoop<State> = EventLoop::try_new()?;
    let display: Display<State> = Display::new()?;
    let dh = display.handle();

    // ---- Seat -----------------------------------------------------------
    let (session, session_notifier) = LibSeatSession::new()?;
    let seat_name = session.seat();
    info!("libseat session on seat {seat_name}");

    // ---- GPU ------------------------------------------------------------
    let primary = primary_gpu(&seat_name)?
        .and_then(|path| DrmNode::from_path(path).ok())
        .ok_or("no primary GPU found for this seat")?;
    info!("primary GPU: {:?}", primary.dev_path());

    let mut session_for_open = session.clone();
    let primary_path = primary.dev_path().ok_or("primary GPU has no device path")?;
    let fd = session_for_open.open(
        &primary_path,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;
    let device_fd = DrmDeviceFd::new(DeviceFd::from(fd));

    // `true` asks for atomic modesetting; smithay falls back to legacy when
    // the driver cannot do it.
    let (mut drm, drm_notifier) = DrmDevice::new(device_fd.clone(), true)?;
    let gbm = GbmDevice::new(device_fd)?;

    let egl_display = unsafe { EGLDisplay::new(gbm.clone())? };
    let egl_context = EGLContext::new(&egl_display)?;
    let mut renderer = unsafe { GlesRenderer::new(egl_context)? };
    // Lets clients hand us wl_drm/EGL buffers as well as dmabufs.
    if let Err(err) = renderer.bind_wl_display(&dh) {
        warn!("could not bind the EGL display for wl_drm: {err}");
    }

    // ---- Output ---------------------------------------------------------
    let (connector_info, crtc) = first_connected_output(&drm)?;
    let drm_mode = preferred_mode(&connector_info)?;
    let output_name = format!(
        "{}-{}",
        connector_info.interface().as_str(),
        connector_info.interface_id()
    );
    let (phys_w, phys_h) = connector_info.size().unwrap_or((0, 0));
    let output = Output::new(
        output_name.clone(),
        PhysicalProperties {
            size: (phys_w as i32, phys_h as i32).into(),
            subpixel: Subpixel::Unknown,
            make: "desicompass".into(),
            model: connector_info.interface().as_str().to_string(),
        },
    );
    let _output_global = output.create_global::<State>(&dh);
    let mode: Mode = drm_mode.into();
    output.change_current_state(
        Some(mode),
        Some(Transform::Normal),
        Some(Scale::Integer(1)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    info!(
        "output {output_name}: {}x{} @ {}mHz",
        mode.size.w, mode.size.h, mode.refresh
    );

    let surface = drm.create_surface(crtc, drm_mode, &[connector_info.handle()])?;
    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let compositor = GbmDrmCompositor::new(
        OutputModeSource::Auto(output.clone()),
        surface,
        None,
        allocator,
        GbmFramebufferExporter::new(gbm.clone(), Some(primary)),
        [
            smithay::reexports::drm::buffer::DrmFourcc::Xrgb8888,
            smithay::reexports::drm::buffer::DrmFourcc::Argb8888,
        ],
        renderer.dmabuf_formats().iter().copied(),
        drm.cursor_size(),
        Some(gbm.clone()),
    )?;

    let damage_tracker = OutputDamageTracker::from_output(&output);
    let tty = TtyGpu {
        session: session.clone(),
        renderer,
        compositor,
        damage_tracker,
        active: true,
    };

    let mut state = State::new(
        &dh,
        event_loop.get_signal(),
        output.clone(),
        Gpu::Tty(Box::new(tty)),
    );

    // ---- Keyboard -------------------------------------------------------
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
    info!("xkb layout={layout:?} variant={variant:?}");
    state.seat.add_keyboard(
        XkbConfig {
            layout: &layout,
            variant: &variant,
            ..Default::default()
        },
        200,
        25,
    )?;

    // ---- dmabuf ---------------------------------------------------------
    match EGLDevice::device_for_display(&egl_display)
        .ok()
        .and_then(|device| device.try_get_render_node().ok().flatten())
    {
        Some(node) => {
            let formats: Vec<_> = state.backend.renderer().dmabuf_formats().iter().copied().collect();
            info!(
                "advertising zwp_linux_dmabuf_v1 on {:?} with {} formats",
                node.dev_path().unwrap_or_default(),
                formats.len()
            );
            let feedback = DmabufFeedbackBuilder::new(node.dev_id(), formats).build()?;
            let _global = state
                .dmabuf_state
                .create_global_with_default_feedback::<State>(&dh, &feedback);
        }
        None => warn!("no render node: GPU clients will not be able to present"),
    }

    // ---- Wayland socket -------------------------------------------------
    let socket = smithay::wayland::socket::ListeningSocketSource::new_auto()?;
    let socket_name = socket.socket_name().to_string_lossy().into_owned();
    event_loop
        .handle()
        .insert_source(socket, |stream, _, state: &mut State| {
            if let Err(err) = state
                .display_handle
                .insert_client(stream, std::sync::Arc::new(ClientState::default()))
            {
                warn!("failed to insert client: {err}");
            }
        })?;
    event_loop.handle().insert_source(
        Generic::new(display, Interest::READ, CalloopMode::Level),
        |_, display, state: &mut State| {
            // SAFETY: the display is never moved out of the source.
            unsafe {
                display.get_mut().dispatch_clients(state)?;
            }
            Ok(PostAction::Continue)
        },
    )?;
    info!("WAYLAND_DISPLAY={socket_name}");
    state.socket_name = socket_name.clone();
    state.spawn_cmd = args.terminal.clone();

    // ---- Input ----------------------------------------------------------
    let mut libinput = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(
        session.clone().into(),
    );
    libinput
        .udev_assign_seat(&seat_name)
        .map_err(|()| "libinput refused the seat")?;
    event_loop.handle().insert_source(
        LibinputInputBackend::new(libinput.clone()),
        move |event, _, state: &mut State| {
            handle_input(state, event);
        },
    )?;

    // ---- Session changes ------------------------------------------------
    //
    // This is the piece that keeps Ctrl+Alt+F1 from wedging the VT.
    let mut libinput_for_session = libinput;
    event_loop
        .handle()
        .insert_source(session_notifier, move |event, _, state: &mut State| {
            match event {
                SessionEvent::PauseSession => {
                    info!("session paused (VT switched away)");
                    libinput_for_session.suspend();
                    if let Gpu::Tty(tty) = &mut state.backend {
                        tty.active = false;
                    }
                }
                SessionEvent::ActivateSession => {
                    info!("session activated (VT switched back)");
                    if libinput_for_session.resume().is_err() {
                        error!("failed to resume libinput after VT switch");
                    }
                    if let Gpu::Tty(tty) = &mut state.backend {
                        tty.active = true;
                        // Everything on screen belongs to the session that
                        // had the VT in the meantime, so nothing of ours can
                        // be assumed still valid: repaint the lot.
                        tty.damage_tracker = OutputDamageTracker::from_output(&state.output);
                    }
                    state.relayout();
                }
            }
        })?;

    // ---- Page flips -----------------------------------------------------
    event_loop
        .handle()
        .insert_source(drm_notifier, move |event, meta, state: &mut State| {
            match event {
                DrmEvent::VBlank(_crtc) => {
                    if let Gpu::Tty(tty) = &mut state.backend {
                        let _ = tty.compositor.frame_submitted();
                    }
                    let _ = meta;
                }
                DrmEvent::Error(err) => error!("DRM error: {err}"),
            }
        })?;

    // ---- Startup command ------------------------------------------------
    if let Some(cmd) = &args.startup_cmd {
        info!("launching startup command: {cmd}");
        match std::process::Command::new("/bin/sh")
            .args(["-c", cmd])
            .env("WAYLAND_DISPLAY", &socket_name)
            .env("SICOMPASS_SESSION", "1")
            .env_remove("DISPLAY")
            .spawn()
        {
            Ok(child) => info!("startup command running as pid {}", child.id()),
            Err(e) => error!("startup command {cmd:?} failed to start: {e}"),
        }
    }

    // ---- Main loop ------------------------------------------------------
    while state.running {
        render(&mut state);
        state.space.refresh();
        state.popups.cleanup();
        state.display_handle.flush_clients()?;
        event_loop.dispatch(Some(Duration::from_millis(16)), &mut state)?;
    }

    info!("desicompass exiting");
    Ok(())
}

/// Draw one frame and queue it for the next page flip.
fn render(state: &mut State) {
    let output = state.output.clone();
    let now = state.start_time.elapsed();

    let State { backend, space, .. } = state;
    let Gpu::Tty(tty) = backend else {
        return;
    };
    if !tty.active {
        return;
    }

    type SpaceElements = SpaceRenderElements<
        GlesRenderer,
        WaylandSurfaceRenderElement<GlesRenderer>,
    >;
    let elements: Vec<SpaceElements> =
        match space_render_elements(&mut tty.renderer, [&*space], &output, 1.0) {
            Ok(elements) => elements,
            Err(err) => {
                error!("failed to collect render elements: {err}");
                return;
            }
        };

    match tty.compositor.render_frame::<_, _>(
        &mut tty.renderer,
        &elements,
        [0.0, 0.0, 0.0, 1.0],
        FrameFlags::DEFAULT,
    ) {
        Ok(frame) => {
            if frame.is_empty {
                return;
            }
            if let Err(err) = tty.compositor.queue_frame(()) {
                error!("failed to queue frame: {err}");
                return;
            }
            // Only once the frame is actually on its way: a client told it
            // may draw again before we have submitted anything will race us.
            space.elements().for_each(|window| {
                window.send_frame(&output, now, Some(Duration::ZERO), |_, _| {
                    Some(output.clone())
                });
            });
        }
        Err(err) => error!("render failed: {err}"),
    }
}

/// Feed a libinput event into the seat.
fn handle_input(state: &mut State, event: InputEvent<LibinputInputBackend>) {
    use smithay::backend::input::{Event, KeyboardKeyEvent};
    use smithay::input::keyboard::FilterResult;

    if let InputEvent::Keyboard { event } = event {
        let Some(keyboard) = state.seat.get_keyboard() else {
            return;
        };
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let pressed = event.state() == smithay::backend::input::KeyState::Pressed;
        keyboard.input(
            state,
            event.key_code(),
            event.state(),
            serial,
            event.time_msec(),
            |app_state, modifiers, keysym| {
                // Diagnostics for chords only — never for ordinary typing.
                //
                // A TTY has no debugger and no scrollback, so when a chord
                // does nothing the log is the only way to tell "the key never
                // arrived" from "it arrived as a different keysym". Gated on
                // a modifier combination that could be a compositor binding,
                // so that passwords and prose are never written to a log.
                if modifiers.logo || (modifiers.ctrl && modifiers.alt) {
                    debug!(
                        "chord: raw={:?} modified={:?} logo={} ctrl={} alt={} shift={} pressed={}",
                        keysym.raw_latin_sym_or_raw_current_sym().map(|s| s.raw()),
                        keysym.modified_sym().raw(),
                        modifiers.logo,
                        modifiers.ctrl,
                        modifiers.alt,
                        modifiers.shift,
                        pressed,
                    );
                }

                // Ctrl+Alt+F1..F12: hand the display to another session.
                //
                // Nothing below a Wayland compositor implements this, so a
                // compositor that does not act on it leaves the user with no
                // way off the screen but killing it. Matched on the
                // *modified* sym: XF86Switch_VT_n exists only at the Ctrl+Alt
                // level of the function keys, and the raw Latin sym used for
                // our own bindings would only ever see plain F1.
                if let Some(vt) = keybindings::vt_switch_target(keysym.modified_sym().raw()) {
                    if !pressed {
                        // Swallow the release; switching twice is harmless but
                        // the client must not see a stray key either.
                        return FilterResult::Intercept(());
                    }
                    if let Gpu::Tty(tty) = &mut app_state.backend {
                        match tty.session.change_vt(vt) {
                            Ok(()) => info!("switching to vt {vt}"),
                            Err(err) => warn!("could not switch to vt {vt}: {err}"),
                        }
                    }
                    return FilterResult::Intercept(());
                }

                let Some(sym) = keysym.raw_latin_sym_or_raw_current_sym() else {
                    return FilterResult::Forward;
                };
                let mods = keybindings::Mods {
                    logo: modifiers.logo,
                    shift: modifiers.shift,
                };
                crate::state::apply_keybinding(app_state, mods, sym.raw(), pressed)
            },
        );
    }
    // Pointer, touch and tablet events are dropped on purpose: desicompass
    // advertises no wl_pointer, so there is nowhere to send them.
}

/// The first connected connector, and a CRTC that can drive it.
fn first_connected_output(
    drm: &DrmDevice,
) -> Result<(connector::Info, crtc::Handle), Box<dyn std::error::Error>> {
    let resources = drm.resource_handles()?;
    let connectors: Vec<connector::Info> = resources
        .connectors()
        .iter()
        .filter_map(|handle| drm.get_connector(*handle, true).ok())
        .filter(|info| info.state() == connector::State::Connected)
        .collect();

    for connector_info in connectors {
        for &encoder_handle in connector_info.encoders() {
            let Ok(encoder) = drm.get_encoder(encoder_handle) else {
                continue;
            };
            if let Some(crtc) = resources
                .filter_crtcs(encoder.possible_crtcs())
                .into_iter()
                .next()
            {
                return Ok((connector_info, crtc));
            }
        }
    }
    Err("no connected output with a usable CRTC".into())
}

/// The connector's preferred mode, else its largest.
fn preferred_mode(
    connector_info: &connector::Info,
) -> Result<smithay::reexports::drm::control::Mode, Box<dyn std::error::Error>> {
    connector_info
        .modes()
        .iter()
        .find(|mode| mode.mode_type().contains(ModeTypeFlags::PREFERRED))
        .or_else(|| {
            connector_info
                .modes()
                .iter()
                .max_by_key(|mode| mode.size().0 as u64 * mode.size().1 as u64)
        })
        .copied()
        .ok_or_else(|| "connector reports no modes".into())
}
