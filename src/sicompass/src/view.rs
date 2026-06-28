//! Main event loop — mirrors `mainLoop()` / `updateView()` in `view.c`.
//!
//! Routes SDL events to key handlers, updates the window title with
//! navigation state, and drives the Vulkan render loop.

use crate::app_state::{AppRenderer, AppState, CommandPhase, Coordinate, History, Task, WindowAction};
use crate::handlers;
use crate::render;
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::{Keycode, Mod};
use sdl3::mouse::MouseButton;
use tracing;

// Modes where the caret blinks and we need continuous redraw
fn is_insert_mode(c: Coordinate) -> bool {
    matches!(
        c,
        Coordinate::Insert
            | Coordinate::Normal
            | Coordinate::Visual
            | Coordinate::SimpleSearch
            | Coordinate::ExtendedSearch
            | Coordinate::Command
            | Coordinate::ScrollSearch
            | Coordinate::ScrollPrefixSearch
            | Coordinate::InputSearch
            // Dashboard's interactive variant takes typed characters too;
            // without this SDL would not fire TextInput events while it owns
            // the screen. The image variant ignores text input, so always
            // enabling it in Dashboard is harmless.
            | Coordinate::Dashboard
            // The tab switcher has a type-to-filter search field (like the
            // colon command palette), so it needs SDL text input too. This only
            // enables text events — the overlay still renders as a list, not
            // element-editing (see the separate render-local `in_insert_mode`).
            | Coordinate::TabSwitcher
    )
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the application until the user quits.
pub fn main_loop(app: &mut AppState) {
    update_window_title(app);

    while app.running {
        // Timestamp the start of the iteration. A provider operation (notably a
        // webbrowser page load, which blocks on Chrome over CDP) can freeze this
        // loop for several seconds. While frozen we cannot service AT-SPI, so a
        // screen reader drops focus tracking on our window — after the load the
        // user's arrow keys go silent until they alt-tab away and back. We detect
        // such a long frame below and re-assert window focus to recover, doing
        // programmatically what that alt-tab does. See the end of the loop.
        let frame_start = handlers::sdl_ticks();

        // ---- Runtime maximize/restore (checkbox toggle) ---------------------
        // pending_maximized is set by a live settings change and must fire
        // every iteration.  At startup this is always None (the startup drain
        // skips "maximized"; the window builder flag covers initial state).
        if let Some(maximize) = app.renderer.pending_maximized.take() {
            if maximize {
                app.window.maximize();
            } else {
                app.window.restore();
            }
        }

        // ---- Custom titlebar window actions ---------------------------------
        // Set by the `c` controls palette or a titlebar-button click; performed
        // here because only the main loop owns the SDL window.
        if let Some(action) = app.renderer.pending_window_action.take() {
            match action {
                WindowAction::Minimize => {
                    app.window.minimize();
                }
                WindowAction::MaximizeToggle => {
                    if app.window.is_maximized() {
                        app.window.restore();
                    } else {
                        app.window.maximize();
                    }
                }
                WindowAction::Close => {
                    app.running = false;
                }
            }
        }

        // ---- First-iteration startup: wait for AT-SPI, then show window -----
        // The window is created hidden (render.rs) with the correct maximized
        // state already baked into the window builder flags.  We defer show()
        // so AccessKit can register with AT-SPI on Linux before the window is
        // mapped, ensuring Orca announces sicompass immediately on focus.
        // The !maximized_ready gate also ensures we don't write a stale value
        // to settings.json from the Restored event SDL fires during window
        // creation before the window is fully mapped.
        if !app.maximized_ready {
            // Wait until AT-SPI has called request_initial_tree (meaning the
            // accessibility tree is already registered on D-Bus) before making
            // the window visible.  The 400 ms timeout covers the case where no
            // screen reader is running.
            if let Some(adapter) = app.accesskit_adapter.as_ref() {
                adapter.wait_for_registration(std::time::Duration::from_millis(400));
            }
            app.window.show();
            app.maximized_ready = true;
        }

        // ---- Collect all pending SDL events (avoids split borrow) -----------
        let events: Vec<Event> = app.event_pump.poll_iter().collect();

        for event in events {
            match event {
                Event::Quit { .. } => {
                    app.running = false;
                }

                Event::KeyDown { keycode, keymod, window_id, .. } => {
                    if window_id != app.window.id() {
                        continue;
                    }
                    tracing::debug!(
                        ?keycode, ?keymod,
                        mode = app.renderer.coordinate.as_str(),
                        "keydown"
                    );
                    handle_keydown(app, keycode, keymod);
                    // Enable/disable SDL text input based on new mode (mirrors C view.c).
                    if is_insert_mode(app.renderer.coordinate) {
                        app._video.text_input().start(&app.window);
                    } else {
                        app._video.text_input().stop(&app.window);
                    }
                }

                Event::KeyUp { keycode, window_id, .. } => {
                    if window_id != app.window.id() {
                        continue;
                    }
                    // Releasing Ctrl commits the held MRU tab switcher (VS Code
                    // style). No-op in every other mode / for the sticky `t`
                    // palette, which commits on Enter instead.
                    if matches!(keycode, Some(Keycode::LCtrl) | Some(Keycode::RCtrl)) {
                        handlers::handle_tab_switcher_commit(&mut app.renderer);
                        if is_insert_mode(app.renderer.coordinate) {
                            app._video.text_input().start(&app.window);
                        } else {
                            app._video.text_input().stop(&app.window);
                        }
                    }
                }

                Event::Window { win_event, window_id, .. } => {
                    if window_id != app.window.id() {
                        continue;
                    }
                    match win_event {
                        WindowEvent::Resized(..)
                        | WindowEvent::PixelSizeChanged(..)
                        | WindowEvent::Exposed => {
                            app.framebuffer_resized = true;
                            // Keep the borderless hit-test region (draggable
                            // strip + resize borders) in sync with the new size.
                            let (wp, hp) = app.window.size();
                            use std::sync::atomic::Ordering;
                            app.hit_test_win_pt.0.store(wp as i32, Ordering::Relaxed);
                            app.hit_test_win_pt.1.store(hp as i32, Ordering::Relaxed);
                        }
                        WindowEvent::Maximized | WindowEvent::Restored => {
                            app.framebuffer_resized = true;
                            let is_maximized = matches!(win_event, WindowEvent::Maximized);
                            // Track maximized state for the `c` palette's
                            // maximize/restore label.
                            app.renderer.window_is_maximized = is_maximized;
                            // Persist so the window reopens in the same state.
                            // Gated on `maximized_ready` so the Restored event SDL
                            // fires during startup window-creation doesn't write a
                            // stale value. `write_maximized` is a no-op when the
                            // value is unchanged, so it never needlessly rewrites
                            // settings.json.
                            if app.maximized_ready {
                                crate::programs::write_maximized(is_maximized);
                            }
                        }
                        WindowEvent::CloseRequested => {
                            app.running = false;
                        }
                        WindowEvent::FocusGained => {
                            if let Some(adapter) = app.accesskit_adapter.as_mut() {
                                adapter.update_window_focus(true);
                            }
                        }
                        WindowEvent::FocusLost => {
                            if let Some(adapter) = app.accesskit_adapter.as_mut() {
                                adapter.update_window_focus(false);
                            }
                        }
                        _ => {}
                    }
                }

                Event::TextInput { text, window_id, .. } => {
                    if window_id != app.window.id() {
                        continue;
                    }
                    tracing::debug!(
                        text = %text,
                        mode = app.renderer.coordinate.as_str(),
                        "text_input"
                    );
                    handlers::handle_input(&mut app.renderer, &text);
                }

                // ---- Custom titlebar buttons (mouse) ------------------------
                // Button rects are filled by the renderer each frame in logical
                // point space — the same space SDL reports mouse coords in.
                Event::MouseMotion { x, y, window_id, .. } => {
                    if window_id != app.window.id() {
                        continue;
                    }
                    let hover = crate::app_state::window_button_at(
                        &app.renderer.window_button_rects, x, y);
                    if hover != app.renderer.window_button_hover {
                        app.renderer.window_button_hover = hover;
                        app.renderer.needs_redraw = true;
                    }
                }

                Event::MouseButtonDown { x, y, mouse_btn, window_id, .. } => {
                    if window_id != app.window.id() {
                        continue;
                    }
                    if mouse_btn == MouseButton::Left {
                        if let Some(idx) = crate::app_state::window_button_at(
                            &app.renderer.window_button_rects, x, y)
                        {
                            app.renderer.pending_window_action = Some(match idx {
                                0 => WindowAction::Minimize,
                                1 => WindowAction::MaximizeToggle,
                                _ => WindowAction::Close,
                            });
                        }
                    }
                }

                _ => {}
            }
        }

        if !app.running {
            break;
        }

        // ---- Drain settings apply-callback events ---------------------------
        if let Some(q) = app.settings_queue.clone() {
            crate::programs::apply_pending_settings(&mut app.renderer, &q, false);
        }

        // ---- Drain updater events + refresh "update available" banner ------
        // Hot-reload events run between frames so no provider call is in
        // flight when we drop the old library and load the new one.
        // FUTURE NOTIFICATION SYSTEM: the banner write inside this call is
        // interim — see programs::process_update_events.
        crate::programs::process_update_events(&mut app.renderer);

        // ---- Rebuild font renderer when fontScale changes -------------------
        if app.renderer.rebuild_font_renderer {
            app.renderer.rebuild_font_renderer = false;
            unsafe {
                app.device.device_wait_idle().unwrap();
                if let Some(old_fr) = app.font_renderer.take() {
                    old_fr.destroy(&app.device);
                }
                let content_scale = app
                    .window
                    .get_display()
                    .and_then(|d| d.get_content_scale())
                    .unwrap_or(1.0);
                let font_scale = crate::programs::read_font_scale();
                let effective_dpi = (96.0_f32 * content_scale * font_scale)
                    .round()
                    .max(48.0) as u32;
                match crate::text::FontRenderer::new(
                    &app.device, &app.instance, app.physical_device,
                    app.command_pool, app.graphics_queue, app.render_pass,
                    effective_dpi,
                ) {
                    Ok(fr) => { app.font_renderer = Some(fr); }
                    Err(e) => { app.renderer.error_message = format!("font reload failed: {e}"); }
                }
            }
            app.renderer.needs_redraw = true;
        }

        // ---- Let providers drive background state (e.g. async OAuth login) --
        // `active_tick_update` is scoped to the *active* provider: a background
        // provider (e.g. an enabled-but-unfocused terminal polling its shell)
        // ticks every frame, and refreshing the active view on its behalf
        // corrupts navigation in whatever the user is looking at.
        let (active_tick_update, dashboard_requests) =
            crate::events::run_provider_ticks(&mut app.renderer);
        // Honor only requests from the *active* provider — never yank the user
        // out of one tab into another tab's dashboard.
        for (i, req) in dashboard_requests {
            if app.renderer.current_id.get(0) != Some(i) {
                continue;
            }
            match req {
                sicompass_sdk::DashboardRequest::Enter
                    if app.renderer.coordinate != Coordinate::Dashboard =>
                {
                    // Reset the baseline to General before entering. The user
                    // typed a command at the input slot (likely in Insert
                    // mode); without this, auto-leave restores Insert and
                    // `i`/`a` would type literally instead of re-entering
                    // Insert. Bypass the manual-entry guard — the provider
                    // asked for this directly via take_dashboard_request.
                    app.renderer.coordinate = Coordinate::General;
                    app.renderer.input_buffer.clear();
                    app.renderer.cursor_position = 0;
                    handlers::enter_dashboard_for_active(&mut app.renderer);
                }
                sicompass_sdk::DashboardRequest::Leave => {
                    handlers::handle_dashboard_leave(&mut app.renderer);
                }
                _ => {}
            }
        }
        // Sync SDL text-input state with the coordinate after the dispatch.
        // Without this, the dashboard's text-input-enabled state lingers
        // through auto-leave; the next `i` keypress would fire BOTH the
        // mode-switch (General+i → Insert) AND a queued TextInput("i")
        // event, typing the literal `i` into the just-entered Insert mode.
        if is_insert_mode(app.renderer.coordinate) {
            app._video.text_input().start(&app.window);
        } else {
            app._video.text_input().stop(&app.window);
        }
        if active_tick_update {
            // Clear any stale status, then let providers re-assert their error.
            app.renderer.error_message.clear();
            for p in app.renderer.providers.iter_mut() {
                if let Some(err) = p.take_error() {
                    app.renderer.error_message = err;
                }
            }
            // Detect whether the cursor is parked on a terminal/chat-style
            // `<input></input>` slot. Streaming output (e.g. `ls` results) shifts
            // the trailing input slot's index in the rebuilt FFON; without this
            // snap, the cursor would silently drift onto an output line.
            // The terminal/claude `+i` live input slot is an Obj; recognise it
            // only when the active provider actually exposes such a slot.
            let on_live_input_provider = matches!(
                crate::provider::get_active_provider_ref(&app.renderer)
                    .map(|p| p.name()),
                Some("terminal") | Some("claude")
            );
            let was_on_input = sicompass_sdk::ffon::get_ffon_at_id(
                &app.renderer.ffon, &app.renderer.current_id,
            )
            .and_then(|arr| {
                let idx = app.renderer.current_id.last()?;
                match arr.get(idx)? {
                    sicompass_sdk::ffon::FfonElement::Str(s) if s.ends_with("<input></input>") => Some(()),
                    sicompass_sdk::ffon::FfonElement::Obj(o)
                        if on_live_input_provider
                            && sicompass_sdk::tags::has_input(&o.key) => Some(()),
                    _ => None,
                }
            })
            .is_some();

            crate::provider::refresh_current_directory(&mut app.renderer);

            if was_on_input {
                if let Some(arr) = sicompass_sdk::ffon::get_ffon_at_id(
                    &app.renderer.ffon, &app.renderer.current_id,
                ) {
                    if let Some(idx) = arr.iter().rposition(|e| match e {
                        sicompass_sdk::ffon::FfonElement::Str(s) => s.ends_with("<input></input>"),
                        sicompass_sdk::ffon::FfonElement::Obj(o) =>
                            on_live_input_provider
                                && sicompass_sdk::tags::has_input(&o.key),
                    }) {
                        app.renderer.current_id.set_last(idx);
                        app.renderer.scroll_offset = -1;
                    }
                }
            }
            // Rebuild the rendered list from the updated ffon tree — same as
            // what handlers.rs does after notify_button_pressed.
            crate::list::create_list_current_layer(&mut app.renderer);
            app.renderer.list_index = app.renderer.current_id.last().unwrap_or(0);
            app.renderer.needs_redraw = true;
        }

        // ---- Drain needs_refresh signals (e.g. async folder load, IMAP IDLE) -
        // Only act on the *active* provider's flag so we don't clear another
        // provider's pending signal before the user has navigated there.
        // Skip while the user is in insert mode — the caret must not jump mid-typing.
        let in_insert = is_insert_mode(app.renderer.coordinate);
        let active_refresh = !in_insert && app.renderer.current_id.get(0)
            .and_then(|i| app.renderer.providers.get(i))
            .map(|p| p.needs_refresh())
            .unwrap_or(false);
        if active_refresh {
            // Save the current list-item label so we can restore the cursor after rebuild.
            let saved_label = app.renderer.current_list_item().map(|it| it.label.clone());

            // Clear flag before rebuild so a signal that arrives *during*
            // rebuild (e.g. IDLE push arriving mid-frame) is preserved.
            if let Some(i) = app.renderer.current_id.get(0) {
                if let Some(p) = app.renderer.providers.get_mut(i) {
                    p.clear_needs_refresh();
                }
            }
            app.renderer.error_message.clear();
            for p in app.renderer.providers.iter_mut() {
                if let Some(err) = p.take_error() {
                    app.renderer.error_message = err;
                }
            }
            crate::provider::refresh_current_directory(&mut app.renderer);
            crate::list::create_list_current_layer(&mut app.renderer);

            // Restore cursor to the same labelled item when possible.
            if let Some(label) = saved_label {
                if let Some(pos) = app.renderer.total_list.iter().position(|it| it.label == label) {
                    if let Some(id) = app.renderer.total_list.get(pos).map(|it| it.id.clone()) {
                        app.renderer.current_id = id;
                        app.renderer.list_index = pos;
                    }
                } else {
                    app.renderer.list_index = app.renderer.current_id.last().unwrap_or(0);
                }
            } else {
                app.renderer.list_index = app.renderer.current_id.last().unwrap_or(0);
            }
            app.renderer.needs_redraw = true;
        }

        // ---- Advance caret blink state --------------------------------------
        let now_ms = handlers::sdl_ticks();
        app.renderer.caret.update(now_ms);

        // ---- Continuous redraw in insert/search modes (caret blink) ---------
        if is_insert_mode(app.renderer.coordinate) {
            app.renderer.needs_redraw = true;
        }

        // ---- Update window title when needed --------------------------------
        if app.renderer.needs_redraw {
            update_window_title(app);
            app.renderer.needs_redraw = false;
        }

        // ---- Fill vertex buffers for this frame ----------------------------
        update_view(app);
        // Overlay the custom titlebar controls on top of whatever update_view
        // rendered (every render path has already begun the passes; the submit
        // happens below in draw_frame).
        draw_window_controls(app);

        // ---- Update accessibility tree (no-op when no AT is active) ---------
        if let Some(adapter) = app.accesskit_adapter.as_mut() {
            adapter.update_if_active(&app.renderer);

            // A blocking provider op (e.g. a webbrowser page load) can freeze
            // this loop long enough that a screen reader stops tracking focus on
            // our window — the user's arrow keys then go silent until they alt-tab
            // away and back. Detect that long frame and re-assert window focus
            // (toggle off→on, the same transition alt-tab produces) so the screen
            // reader re-enters focus mode on its own. The tree rebuild alone is
            // not enough: with the focused node unchanged, no focus event fires.
            // Normal frames are sub-frame-time; only blocking ops (or a one-off
            // swapchain/font rebuild) cross this threshold, and re-asserting focus
            // there is harmless. Windows handles this in the provider via a
            // foreground bounce, and `update_window_focus` is a no-op there.
            const LONG_FRAME_MS: u64 = 750;
            if handlers::sdl_ticks().saturating_sub(frame_start) >= LONG_FRAME_MS {
                adapter.update_window_focus(false);
                adapter.update_window_focus(true);
            }
        }
        // Note: pending_announcement is NOT cleared here. It persists until a
        // handler overwrites it with the next announcement, so the AT has
        // unlimited time to query the live-region node. This matches the C
        // behaviour where the announcement text stays in the tree between speaks.

        // ---- Recreate swapchain if needed -----------------------------------
        if app.framebuffer_resized {
            app.framebuffer_resized = false;
            render::recreate_swapchain(app);
        }

        // ---- Sync clear colour from active palette --------------------------
        app.clear_color = rgba_u32_to_f32(app.renderer.palette().background);

        // ---- Draw frame ---------------------------------------------------
        render::draw_frame(app);

        // ~60 FPS cap
        std::thread::sleep(std::time::Duration::from_millis(16));
    }

    unsafe {
        let _ = app.device.device_wait_idle();
    }
}

// ---------------------------------------------------------------------------
// updateView — fill CPU vertex buffers for this frame
// ---------------------------------------------------------------------------

struct ParentInfo {
    display_text: String,
    is_radio: bool,
    radio_summary: Option<String>,
}

/// Append the custom titlebar controls (minimize / maximize / close) to the
/// current frame's vertex buffers and record their hit-test rects.
///
/// Called once per frame after `update_view`, so it covers every render path
/// (each has already called `begin_*_rendering`; the submit happens afterwards
/// in `draw_frame`). Geometry is computed in logical point space — the space SDL
/// reports mouse coords in — and scaled to physical pixels for drawing via the
/// device pixel ratio. All colours come from the active palette, so the controls
/// follow light/dark theme switching automatically.
fn draw_window_controls(app: &mut AppState) {
    let p = *app.renderer.palette();
    let win_w_px = app.swapchain_extent.width as f32;
    let (sz_w, _) = app.window.size();
    let device_scale = if sz_w > 0 { win_w_px / sz_w as f32 } else { 1.0 };
    let win_w_pt = if device_scale > 0.0 { win_w_px / device_scale } else { win_w_px };
    let on_left = crate::app_state::controls_on_left();

    let rects = crate::app_state::window_button_rects(win_w_pt, on_left);
    app.renderer.window_button_rects = rects;
    let hover = app.renderer.window_button_hover;

    let maximized = app.renderer.window_is_maximized;

    let Some(rr) = app.rect_renderer.as_mut() else {
        return;
    };
    // All three icons are drawn as strokes (no font glyph), so they look
    // consistent and don't depend on the font's glyph coverage (Consolas has
    // no square/✕ glyphs at all).
    for (i, &(bx, by, bw, bh)) in rects.iter().enumerate() {
        let px = bx * device_scale;
        let py = by * device_scale;
        let pw = bw * device_scale;
        let ph = bh * device_scale;

        // Hovered button face: red for close, selection highlight otherwise.
        if hover == Some(i) {
            let bg = if i == 2 { p.error } else { p.selected };
            rr.prepare_rectangle(px, py, pw, ph, bg, 0.0);
        }

        let cx = px + pw / 2.0;
        let cy = py + ph / 2.0;
        // Icon half-extent and stroke thickness, scaled with DPI.
        let h = (ph * 0.17).round().max(3.0);
        let t = (1.5 * device_scale).round().max(1.0);

        match i {
            0 => {
                // Minimize: a horizontal line.
                rr.prepare_line(cx - h, cy, cx + h, cy, t, p.text);
            }
            1 => {
                // Maximize / restore: a square outline drawn as four strokes.
                // When maximized, draw two offset squares to read as "restore".
                let square = |rr: &mut crate::rectangle::RectangleRenderer, ox: f32, oy: f32, he: f32| {
                    let (x0, y0, x1, y1) = (cx - he + ox, cy - he + oy, cx + he + ox, cy + he + oy);
                    rr.prepare_line(x0, y0, x1, y0, t, p.text); // top
                    rr.prepare_line(x0, y1, x1, y1, t, p.text); // bottom
                    rr.prepare_line(x0, y0, x0, y1, t, p.text); // left
                    rr.prepare_line(x1, y0, x1, y1, t, p.text); // right
                };
                if maximized {
                    let off = (h * 0.55).round();
                    square(rr, off, -off, h - off / 2.0);
                    square(rr, -off, off, h - off / 2.0);
                } else {
                    square(rr, 0.0, 0.0, h);
                }
            }
            _ => {
                // Close: an X drawn as two diagonal strokes.
                rr.prepare_line(cx - h, cy - h, cx + h, cy + h, t, p.text);
                rr.prepare_line(cx - h, cy + h, cx + h, cy - h, t, p.text);
            }
        }
    }
}

fn update_view(app: &mut AppState) {
    // Speak any error that reached the header since the last frame. Done once
    // here, before the header is drawn, so every error path is covered.
    app.renderer.announce_error_if_new();

    // ---- Snapshot palette before mutable borrows ---------------------------
    let p = *app.renderer.palette();

    // ---- Collect rendering data (before borrowing font/rect renderers) ----
    let scale;
    let line_height;
    let ascender;
    let em_width;

    {
        let fr = match app.font_renderer.as_ref() {
            Some(f) => f,
            None => return,
        };
        scale = fr.get_text_scale(crate::text::FONT_SIZE_PT);
        line_height = fr.get_line_height(scale, crate::text::TEXT_PADDING) as i32;
        ascender = fr.ascender;
        em_width = fr.get_width_em(scale);
    }

    // The visual tabs band has been removed; content starts at the top.
    let top_offset: f32 = 0.0;

    // Snapshot the display state so we can borrow font_renderer mutably after
    let header = build_header_text(&app.renderer, line_height);
    let win_w = app.swapchain_extent.width as f32;
    let win_h = app.swapchain_extent.height as f32;
    let list_items: Vec<(String, Option<String>, bool, Vec<u32>, Option<String>)> = collect_list_items(&app.renderer);
    let list_has_indicators = list_items.iter().any(|(label, _, _, _, _)| {
        get_radio_type(label) != RadioType::None || get_checkbox_type(label) != CheckboxType::None
    });

    // Meta/timeline items are plain strings without a type-indicator prefix, so
    // skip column alignment there (matches C render.c which renders all items at a
    // fixed itemX with no prefix-column offset). The colon command palette
    // (`Command` + `CommandPhase::None`) is a button list: its items carry a `-b `
    // prefix and render through the normal prefix-column path. The secondary
    // command list (`CommandPhase::Provider`, e.g. "open with") has no prefix and
    // stays flat.
    let is_flat_list = matches!(
        app.renderer.coordinate,
        Coordinate::Meta | Coordinate::TimelineView
    ) || (app.renderer.coordinate == Coordinate::Command
        && app.renderer.current_command == CommandPhase::Provider);

    // Compute indent and max prefix width before centering so the full visual
    // width (indent + prefix + content) can be centered in the window.
    let (list_indent_px, max_prefix_px) = {
        let fr = match app.font_renderer.as_ref() {
            Some(f) => f,
            None => return,
        };
        let indent = fr.measure_text_width("    ", scale);
        let prefix = if is_flat_list {
            0.0_f32
        } else {
            list_items.iter()
                .map(|(label, _, _, _, _)| {
                    let (p, _) = split_label(label);
                    let text_w = fr.measure_text_width(p, scale);
                    // When any item has an indicator, all items reserve the same indicator width
                    let indicator_w = if list_has_indicators { indicator_width(line_height as f32, em_width) } else { 0.0 };
                    text_w + indicator_w
                })
                .fold(0.0_f32, f32::max)
        };
        (indent, prefix)
    };
    let left_inset = 10.0 + list_indent_px + max_prefix_px;
    let content_w = (120.0 * em_width).min(win_w);
    let content_x = ((win_w - content_w - left_inset) / 2.0).max(0.0);
    let text_x = content_x + 10.0;
    let max_content_w = content_w.min(win_w - text_x - left_inset);

    // ---- Scroll / ScrollSearch / ScrollPrefixSearch early dispatch -----------
    if matches!(app.renderer.coordinate,
        Coordinate::Scroll | Coordinate::ScrollSearch | Coordinate::ScrollPrefixSearch)
    {
        // Cache layout metrics for handlers
        app.renderer.window_height = win_h as i32;
        app.renderer.cached_line_height = line_height;

        // Snapshot state needed before mutable borrows
        let text_scroll_offset = app.renderer.text_scroll_offset;
        let list_index = app.renderer.list_index;
        let search_query = app.renderer.input_buffer.clone();
        let search_cursor = app.renderer.cursor_position;
        let search_caret_visible = app.renderer.caret.visible;
        let search_selection = app.renderer.selection_anchor
            .filter(|&a| a != search_cursor)
            .map(|a| (a.min(search_cursor), a.max(search_cursor)));
        let search_current_match = app.renderer.scroll_search_current_match;
        let search_needs_position = app.renderer.scroll_search_needs_position;
        let search_snap = app.renderer.scroll_search_snap;
        let search_corpus = match app.renderer.coordinate {
            Coordinate::ScrollSearch => Some(ScrollSearchCorpus::Content),
            Coordinate::ScrollPrefixSearch => Some(ScrollSearchCorpus::Prefix),
            _ => None,
        };
        // Content viewport = window minus tabs band + header (+ search bar in
        // search modes). Cached so scroll handlers clamp to the exact area the
        // renderer uses — otherwise the last line/image bottom is unreachable.
        let scroll_clip_y = if search_corpus.is_some() {
            line_height as f32 * 2.0 + top_offset
        } else {
            line_height as f32 + top_offset
        };
        app.renderer.text_scroll_viewport_h = (win_h - scroll_clip_y).max(0.0) as i32;
        let error_msg = app.renderer.error_message.clone();

        // Begin render passes
        let fr = match app.font_renderer.as_mut() { Some(f) => f, None => return };
        fr.begin_text_rendering();
        if let Some(rr) = app.rect_renderer.as_mut() {
            rr.begin_rect_rendering();
        }
        if let Some(ir) = app.image_renderer.as_mut() {
            ir.begin_image_rendering();
        }

        // Header separator and text
        if let Some(rr) = app.rect_renderer.as_mut() {
            rr.prepare_rectangle(0.0, line_height as f32 + top_offset, win_w, 1.0, p.header_sep, 0.0);
        }
        let header_baseline = (ascender * scale + crate::text::TEXT_PADDING) as f32 + top_offset;
        app.font_renderer.as_mut().unwrap().prepare_text_for_rendering(&header, text_x, header_baseline, scale, p.text);
        if !error_msg.is_empty() {
            let fr = app.font_renderer.as_mut().unwrap();
            let err_x = text_x + (header.len() as f32 * fr.get_width_em(scale)) + 10.0;
            fr.prepare_text_for_rendering(&error_msg, err_x, header_baseline, scale, p.error);
        }

        // Render scroll content — full list with pixel-smooth scrolling
        if let Some(corpus) = search_corpus {
            let result = render_scroll_search_full(
                app.font_renderer.as_mut().unwrap(),
                app.rect_renderer.as_mut(),
                app.image_renderer.as_mut(),
                &list_items,
                list_index,
                text_scroll_offset,
                &search_query,
                search_cursor,
                search_caret_visible,
                search_selection,
                search_current_match,
                search_needs_position,
                search_snap,
                corpus,
                scale, line_height, ascender, em_width, text_x, max_prefix_px, max_content_w, win_h, top_offset, &p,
            );
            app.renderer.text_scroll_total_height = result.total_height;
            app.renderer.scroll_search_match_count = result.match_count;
            app.renderer.scroll_search_current_match = result.current_match;
            // Keep list_index on the current match so Enter navigates to it.
            app.renderer.list_index = result.current_item;
            app.renderer.text_scroll_offset = result.scroll_offset;
            // needs_position is cleared by Up/Down navigation (not by the renderer);
            // while the user is still typing it stays true so viewport-aware selection
            // re-fires on every frame with an updated match set.
            app.renderer.scroll_search_snap = false;
        } else {
            let (total_height, resolved_offset) = render_scroll_full(
                app.font_renderer.as_mut().unwrap(),
                app.rect_renderer.as_mut(),
                app.image_renderer.as_mut(),
                &list_items,
                list_index,
                text_scroll_offset,
                scale, line_height, ascender, em_width, text_x, max_prefix_px, max_content_w, win_h, top_offset, &p,
            );
            app.renderer.text_scroll_total_height = total_height;
            app.renderer.text_scroll_offset = resolved_offset;
        }
        return;
    }

    // ---- Dashboard early dispatch --------------------------------------------
    // Single coordinate for both flavors. Image vs interactive (cell-grid) is
    // decided by the active provider's `dashboard_kind()`.
    if app.renderer.coordinate == Coordinate::Dashboard {
        let is_interactive = crate::provider::get_active_provider_ref(&app.renderer)
            .map(|p| p.dashboard_kind() == sicompass_sdk::DashboardKind::Interactive)
            .unwrap_or(false);

        if is_interactive {
            let cell_w = em_width.max(1.0);
            let cell_h = line_height.max(1) as f32;
            let header_h = line_height as f32 + top_offset;
            let grid_top = header_h;
            let avail_h = (win_h - grid_top).max(0.0);
            let cols = (win_w / cell_w).floor().max(1.0) as u16;
            let rows = (avail_h / cell_h).floor().max(1.0) as u16;

            // Forward resize once whenever the cell-grid size changes (incl. on
            // first entry, since `dashboard_cell_size` starts at (0, 0)).
            let prev_size = app.renderer.dashboard_cell_size;
            let frame = match crate::provider::get_active_provider(&mut app.renderer) {
                Some(prov) => {
                    if prev_size != (cols, rows) {
                        prov.dashboard_resize(rows, cols);
                    }
                    prov.dashboard_render(cols, rows)
                }
                None => return,
            };
            app.renderer.dashboard_cell_size = (cols, rows);

            // Begin render passes
            let fr = match app.font_renderer.as_mut() { Some(f) => f, None => return };
            fr.begin_text_rendering();
            if let Some(rr) = app.rect_renderer.as_mut() {
                rr.begin_rect_rendering();
            }
            if let Some(ir) = app.image_renderer.as_mut() {
                ir.begin_image_rendering();
            }

            // Header separator + title
            if let Some(rr) = app.rect_renderer.as_mut() {
                rr.prepare_rectangle(0.0, line_height as f32 + top_offset, win_w, 1.0, p.header_sep, 0.0);
            }
            let header_baseline = (ascender * scale + crate::text::TEXT_PADDING) as f32 + top_offset;
            app.font_renderer.as_mut().unwrap().prepare_text_for_rendering(
                &header, text_x, header_baseline, scale, p.text,
            );

            // Pass 1: cell backgrounds (and the cursor block).
            if let Some(rr) = app.rect_renderer.as_mut() {
                for row in 0..rows {
                    for col in 0..cols {
                        let cell = frame.cell(col, row);
                        let is_cursor = frame.cursor == Some((col, row));
                        let bg = if is_cursor { cell.fg } else { cell.bg };
                        if (bg & 0xFF) == 0 { continue; }
                        let x = col as f32 * cell_w;
                        let y = grid_top + row as f32 * cell_h;
                        rr.prepare_rectangle(x, y, cell_w, cell_h, bg, 0.0);
                    }
                }
            }

            // Pass 2: glyphs. Cursor cell is rendered with fg/bg swapped so the
            // character stays legible against the cursor block.
            let fr = app.font_renderer.as_mut().unwrap();
            let mut utf8 = [0u8; 4];
            for row in 0..rows {
                let baseline = grid_top + row as f32 * cell_h
                    + (ascender * scale + crate::text::TEXT_PADDING) as f32;
                for col in 0..cols {
                    let cell = frame.cell(col, row);
                    if cell.ch == ' ' { continue; }
                    let is_cursor = frame.cursor == Some((col, row));
                    let fg = if is_cursor { cell.bg } else { cell.fg };
                    if (fg & 0xFF) == 0 { continue; }
                    let s: &str = cell.ch.encode_utf8(&mut utf8);
                    let x = col as f32 * cell_w;
                    fr.prepare_text_for_rendering(s, x, baseline, scale, fg);
                }
            }
        } else {
            // Image (or legacy `None` + dashboard_image_path) flavor.
            let dashboard_path = app.renderer.dashboard_image_path.clone();

            // Begin render passes
            let fr = match app.font_renderer.as_mut() { Some(f) => f, None => return };
            fr.begin_text_rendering();
            if let Some(rr) = app.rect_renderer.as_mut() {
                rr.begin_rect_rendering();
            }
            if let Some(ir) = app.image_renderer.as_mut() {
                ir.begin_image_rendering();
            }

            // Header separator and text
            if let Some(rr) = app.rect_renderer.as_mut() {
                rr.prepare_rectangle(0.0, line_height as f32 + top_offset, win_w, 1.0, p.header_sep, 0.0);
            }
            let header_baseline = (ascender * scale + crate::text::TEXT_PADDING) as f32 + top_offset;
            app.font_renderer.as_mut().unwrap().prepare_text_for_rendering(&header, text_x, header_baseline, scale, p.text);
            let error_msg = app.renderer.error_message.clone();
            if !error_msg.is_empty() {
                let fr = app.font_renderer.as_mut().unwrap();
                let err_x = text_x + (header.len() as f32 * fr.get_width_em(scale)) + 10.0;
                fr.prepare_text_for_rendering(&error_msg, err_x, header_baseline, scale, p.error);
            }

            // Render the dashboard image (tabs band + header occupy two line_heights)
            if !dashboard_path.is_empty() {
                if let Some(ir) = app.image_renderer.as_mut() {
                    let avail_w = win_w - 100.0;
                    let avail_h = win_h - line_height as f32 * 2.0 - top_offset;
                    if let Some((img_w, img_h)) = unsafe { ir.texture_size(&dashboard_path) } {
                        let img_w = img_w as f32;
                        let img_h = img_h as f32;
                        let mut display_scale = 1.0_f32;
                        if img_w > avail_w { display_scale = avail_w / img_w; }
                        if img_h * display_scale > avail_h { display_scale = avail_h / img_h; }
                        let display_w = img_w * display_scale;
                        let display_h = img_h * display_scale;
                        let img_x = (win_w - display_w) / 2.0;
                        let img_y = line_height as f32 + top_offset + (avail_h - display_h) / 2.0;
                        unsafe { ir.prepare_image(&dashboard_path, img_x, img_y, display_w, display_h); }
                    }
                }
            }
        }

        return;
    }

    // Snapshot insert-mode state before mutable borrows
    let in_insert_mode = matches!(
        app.renderer.coordinate,
        Coordinate::Insert | Coordinate::Normal | Coordinate::Visual
    );
    // For a `<password>` field, render the buffer as one `*` per character so
    // the secret never reaches the glyph pipeline. Cursor/selection are byte
    // offsets into the real buffer; remap them to the masked string (each
    // mask char is one byte) so the caret and highlight stay aligned. Done
    // once here so every downstream use (glyphs, caret, selection, line
    // counting) sees the same masked text.
    let mask_password = in_insert_mode && app.renderer.input_is_password;
    let (insert_buf, insert_cursor, insert_sel) = if mask_password {
        let raw = &app.renderer.input_buffer;
        let masked: String = raw.chars().map(|c| if c == '\n' { '\n' } else { '*' }).collect();
        let to_masked = |b: usize| raw.get(..b).map(|s| s.chars().count())
            .unwrap_or_else(|| raw.chars().count());
        (
            masked,
            to_masked(app.renderer.cursor_position),
            app.renderer.selection_anchor.map(to_masked),
        )
    } else {
        (
            app.renderer.input_buffer.clone(),
            app.renderer.cursor_position,
            app.renderer.selection_anchor,
        )
    };
    // `input_prefix`/`input_suffix` are kept raw (they reconstruct the FFON
    // key on commit), but an input slot puts a dangling `<input>` in the
    // prefix and `</input>` in the suffix — strip all tag tokens for display.
    let insert_prefix = sicompass_sdk::tags::strip_tags(&app.renderer.input_prefix);
    let insert_suffix = sicompass_sdk::tags::strip_tags(&app.renderer.input_suffix);
    let caret_visible = app.renderer.caret.visible;
    let search_str = if app.renderer.coordinate == Coordinate::ConfirmCloseTab {
        // Modal prompt above the two `-b` button options.
        Some("This tab has a running program. Close it?".to_string())
    } else if matches!(
        app.renderer.coordinate,
        Coordinate::SimpleSearch | Coordinate::ExtendedSearch | Coordinate::Command
            | Coordinate::TabSwitcher
    ) {
        let (prefix, text) = match app.renderer.coordinate {
            Coordinate::Command => ("search: ", app.renderer.input_buffer.as_str()),
            Coordinate::ExtendedSearch => ("ext search: ", app.renderer.input_buffer.as_str()),
            Coordinate::TabSwitcher => ("switch tab: ", app.renderer.input_buffer.as_str()),
            _ => ("search: ", app.renderer.search_string.as_str()),
        };
        // Tab (ExtendedSearch) and Ctrl-F (SimpleSearch) append a result count,
        // matching the `[N items]` readout shown by scroll-mode searches; the
        // tab switcher shows a tab count.
        let count_suffix = match app.renderer.coordinate {
            Coordinate::SimpleSearch | Coordinate::ExtendedSearch => {
                format!(" [{} items]", list_items.len())
            }
            Coordinate::TabSwitcher => format!(" [{} tabs]", list_items.len()),
            _ => String::new(),
        };
        Some(format!("{}{}{}", prefix, text, count_suffix))
    } else {
        None
    };
    let error_msg = app.renderer.error_message.clone();

    // ---- Parent element snapshot -------------------------------------------
    // Always present (empty at root level), so list items are consistently
    // indented one level below the parent line.
    //
    // At depth 2 (just inside a provider) the parent is always the provider's
    // root Obj. For providers that pin their root key (`stable_root_key=true`,
    // e.g. editor) that key never changes as the user navigates within the
    // provider, so deriving the label from the active provider's
    // `current_path()` basename gives a label that follows the list. For other
    // providers the basename and the FFON root key happen to coincide, so the
    // result is unchanged.
    let parent_info: ParentInfo = if app.renderer.current_id.depth() > 1 {
        let mut parent_id = app.renderer.current_id.clone();
        parent_id.pop();
        let parent_idx = parent_id.last();
        let parent_slice = sicompass_sdk::ffon::get_ffon_at_id(&app.renderer.ffon, &parent_id);
        parent_slice.zip(parent_idx).and_then(|(slice, idx)| {
            let elem = slice.get(idx)?;
            let raw_text = match elem {
                sicompass_sdk::ffon::FfonElement::Obj(obj) => obj.key.as_str(),
                sicompass_sdk::ffon::FfonElement::Str(s) => s.as_str(),
            };
            let mut display_text = sicompass_sdk::tags::strip_display(raw_text);
            if app.renderer.current_id.depth() == 2 {
                let provider_idx = app.renderer.current_id.get(0).unwrap_or(0);
                if let Some(p) = app.renderer.providers.get(provider_idx) {
                    if !p.at_root() {
                        if let Some(name) = std::path::Path::new(p.current_path())
                            .file_name()
                            .and_then(|n| n.to_str())
                            .filter(|s| !s.is_empty())
                        {
                            display_text = name.to_owned();
                        }
                    }
                }
            }
            let (is_radio, radio_summary) = if let sicompass_sdk::ffon::FfonElement::Obj(obj) = elem {
                if sicompass_sdk::tags::has_radio(&obj.key) {
                    let checked = obj.children.iter().find_map(|child| {
                        if let sicompass_sdk::ffon::FfonElement::Str(s) = child {
                            if sicompass_sdk::tags::has_checked(s) {
                                return sicompass_sdk::tags::extract_checked(s);
                            }
                        }
                        None
                    });
                    (true, checked)
                } else {
                    (false, None)
                }
            } else {
                (false, None)
            };
            Some(ParentInfo { display_text, is_radio, radio_summary })
        }).unwrap_or(ParentInfo { display_text: String::new(), is_radio: false, radio_summary: None })
    } else {
        ParentInfo { display_text: String::new(), is_radio: false, radio_summary: None }
    };

    // ---- Scroll-into-view: compute start_index from scroll_offset/list_index --
    // Pre-compute per-item line counts (needed before item_metrics so the scroll
    // algorithm can run first, matching the C render.c viewport logic).
    let extra_lines = if search_str.is_some() {
        1
    } else {
        1 + if parent_info.radio_summary.is_some() { 1 } else { 0 }
    };
    let first_item_y = (line_height as f32) * (1.0 + extra_lines as f32) + ascender * scale + crate::text::TEXT_PADDING + top_offset;
    let available_lines = ((win_h - first_item_y) / line_height as f32).max(1.0) as usize;
    let item_max_w = max_content_w.max(1.0);

    let is_extended_search = app.renderer.coordinate == Coordinate::ExtendedSearch;
    let count = list_items.len();
    let list_index = if count > 0 { app.renderer.list_index.min(count - 1) } else { 0 };
    let line_counts: Vec<usize> = {
        let fr = match app.font_renderer.as_ref() { Some(f) => f, None => return };
        list_items.iter().enumerate().map(|(idx, (label, img_data, _, _, ext_prefix))| {
            if in_insert_mode && idx == list_index {
                return insert_buf.split('\n').count().max(1);
            }
            if !is_extended_search {
                if let Some(path) = img_data {
                    let (prefix, suffix, has_prefix) = split_image_label(label, path);
                    let prefix_lines = if has_prefix { count_text_lines(prefix) } else { 0 };
                    let suffix_lines = if suffix.is_empty() { 0 } else { count_text_lines(suffix) };
                    let header_lines = (1 + extra_lines) as f32;
                    let lh = line_height as f32;
                    let max_h_raw = win_h - lh * (header_lines + prefix_lines as f32 + suffix_lines as f32);
                    let max_h = if suffix_lines > 0 {
                        ((max_h_raw / lh).floor() * lh).max(lh)
                    } else {
                        (max_h_raw - crate::text::TEXT_PADDING).max(lh)
                    };
                    let raw_img_h = app.image_renderer.as_mut()
                        .and_then(|ir| unsafe { ir.texture_size(path) })
                        .map(|(tw, th)| if tw == 0 { item_max_w } else { item_max_w * th as f32 / tw as f32 })
                        .unwrap_or(item_max_w);
                    let img_h = raw_img_h.min(max_h);
                    let image_lines = ((img_h / line_height as f32).ceil() as usize).max(1);
                    return prefix_lines + image_lines + suffix_lines;
                }
                let (_, content) = split_label(label);
                return fr.count_wrapped_lines(content, scale, item_max_w);
            }
            // ExtendedSearch: breadcrumb + prefix precede content — reduce available width.
            // Use 4.0 * em_width (= item_prefix_x offset) not list_indent_px (space-based)
            // so that the available_w matches what the rendering loop actually uses.
            // Also subtract indicator_width when any item has an indicator, since text_prefix_x
            // is shifted right by that amount for ALL items (for alignment).
            let indicator_w = if list_has_indicators { indicator_width(line_height as f32, em_width) } else { 0.0 };
            // Image result: one breadcrumb/prefix line, then the image below.
            if let Some(path) = ext_prefix {
                let img_w = (max_content_w - 4.0 * em_width - indicator_w).max(1.0);
                let img_h = app.image_renderer.as_mut()
                    .and_then(|ir| unsafe { ir.texture_size(path) })
                    .map(|(tw, th)| if tw == 0 { img_w } else { img_w * th as f32 / tw as f32 })
                    .unwrap_or(img_w);
                let image_lines = ((img_h / line_height as f32).ceil() as usize).max(1);
                let (_, suffix, _) = split_image_label(label, path);
                let suffix_lines = if suffix.is_empty() { 0 }
                    else { fr.count_wrapped_lines(suffix, scale, img_w) };
                // 1 breadcrumb/prefix line + image + trailing text below it.
                return 1 + image_lines + suffix_lines;
            }
            let bc_w = img_data.as_deref().filter(|s| !s.is_empty())
                .map(|bc| fr.measure_text_width(bc, scale)).unwrap_or(0.0);
            let (prefix_str, content) = split_label(label);
            let prefix_w = fr.measure_text_width(prefix_str, scale);
            // First line trails the breadcrumb + prefix; continuation lines
            // wrap full-width (left margin → column edge) below them.
            let rest_w = (max_content_w - 4.0 * em_width - indicator_w).max(1.0);
            let first_w = (rest_w - bc_w - prefix_w).max(1.0);
            fr.wrap_lines_with_offsets_hanging(content, scale, first_w, rest_w).len().max(1)
        }).collect()
    };

    let start_index: usize = if count == 0 {
        0
    } else {
        let scroll_offset = app.renderer.scroll_offset;
        // Whole-list fit: when every item visibly fits in the viewport,
        // always anchor at index 0 — no point in honoring a non-zero
        // scroll_offset (e.g. set by walk_back to list_index) that would
        // hide items above the cursor.
        let total_lines: usize = line_counts.iter().sum();
        if total_lines <= available_lines {
            0
        } else if scroll_offset < 0 {
            // Sentinel -1: position list_index as last visible (renderer shows one extra item below).
            let mut lines_from_bottom = line_counts.get(list_index).copied().unwrap_or(1);
            let mut si = list_index;
            while si > 0 {
                let prev = line_counts.get(si - 1).copied().unwrap_or(1);
                if lines_from_bottom + prev > available_lines { break; }
                lines_from_bottom += prev;
                si -= 1;
            }
            si
        } else {
            let mut si = (scroll_offset as usize).min(list_index);
            // Snap forward until list_index is within the visible area.
            let mut lines_to_sel: usize = line_counts[si..=list_index].iter().sum();
            while lines_to_sel > available_lines && si < list_index {
                lines_to_sel -= line_counts.get(si).copied().unwrap_or(1);
                si += 1;
            }
            // Scrolloff: try to show 1 item above the selection.
            if si > 0 && si == list_index {
                let prev_lines = line_counts.get(si - 1).copied().unwrap_or(1);
                if lines_to_sel + prev_lines <= available_lines {
                    si -= 1;
                }
            }
            si
        }
    };
    app.renderer.scroll_offset = start_index as i32;

    // ---- Per-item layout metrics (immutable font borrow) ------------------
    // Each entry: (item_y, content_start_x, lines_used, highlight_w)
    // Starts from start_index so only visible items are measured/rendered.
    // image_layouts is a parallel vec with Some(ImageLayout) for image items.
    let (item_metrics, image_layouts): (Vec<(f32, f32, usize, f32)>, Vec<Option<ImageLayout>>) = {
        let fr = match app.font_renderer.as_ref() {
            Some(f) => f,
            None => return,
        };
        let item_prefix_x = text_x + list_indent_px;
        let content_start_x = item_prefix_x + max_prefix_px;
        let mut y = first_item_y;
        let cap = list_items.len().saturating_sub(start_index);
        let mut metrics = Vec::with_capacity(cap);
        let mut layouts: Vec<Option<ImageLayout>> = Vec::with_capacity(cap);
        for (global_idx, (label, img_data, _, _, ext_prefix)) in list_items.iter().enumerate().skip(start_index) {
            if y > win_h { break; }
            let (lines, img_layout) = if in_insert_mode && global_idx == list_index {
                (insert_buf.split('\n').count().max(1), None)
            } else if !is_extended_search {
                if let Some(path) = img_data {
                    let (prefix, suffix, has_prefix) = split_image_label(label, path);
                    let prefix_lines = if has_prefix { count_text_lines(prefix) } else { 0 };
                    let suffix_lines = if suffix.is_empty() { 0 } else { count_text_lines(suffix) };
                    let header_lines = (1 + extra_lines) as f32;
                    let lh = line_height as f32;
                    let max_h_raw = win_h - lh * (header_lines + prefix_lines as f32 + suffix_lines as f32);
                    let max_h = if suffix_lines > 0 {
                        ((max_h_raw / lh).floor() * lh).max(lh)
                    } else {
                        (max_h_raw - crate::text::TEXT_PADDING).max(lh)
                    };
                    let img_w = item_max_w;
                    let img_h = if let Some(ir) = app.image_renderer.as_mut() {
                        unsafe { ir.texture_size(path) }
                            .map(|(tw, th)| {
                                if tw == 0 { img_w } else { (img_w * th as f32 / tw as f32).min(max_h) }
                            })
                            .unwrap_or(img_w)
                    } else {
                        img_w
                    };
                    let image_lines = ((img_h / line_height as f32).ceil() as usize).max(1);
                    let total_lines = prefix_lines + image_lines + suffix_lines;
                    (total_lines, Some(ImageLayout { prefix_lines, suffix_lines, image_lines, img_w, img_h }))
                } else if is_flat_list {
                    // Command/Meta items: no prefix split — measure the full label
                    (fr.count_wrapped_lines(label, scale, item_max_w), None)
                } else {
                    let (_, content) = split_label(label);
                    (fr.count_wrapped_lines(content, scale, item_max_w), None)
                }
            } else {
                // ExtendedSearch: breadcrumb + prefix precede content — reduce available width.
                let indicator_w = if list_has_indicators { indicator_width(line_height as f32, em_width) } else { 0.0 };
                if let Some(path) = ext_prefix {
                    // Image result: one breadcrumb/prefix line, then the image.
                    let img_w = (max_content_w - 4.0 * em_width - indicator_w).max(1.0);
                    let img_h = app.image_renderer.as_mut()
                        .and_then(|ir| unsafe { ir.texture_size(path) })
                        .map(|(tw, th)| if tw == 0 { img_w } else { img_w * th as f32 / tw as f32 })
                        .unwrap_or(img_w);
                    let image_lines = ((img_h / line_height as f32).ceil() as usize).max(1);
                    let (_, suffix, _) = split_image_label(label, path);
                    let suffix_lines = if suffix.is_empty() { 0 }
                        else { fr.count_wrapped_lines(suffix, scale, img_w) };
                    (1 + image_lines + suffix_lines,
                     Some(ImageLayout { prefix_lines: 1, suffix_lines, image_lines, img_w, img_h }))
                } else {
                    let bc_w = img_data.as_deref().filter(|s| !s.is_empty())
                        .map(|bc| fr.measure_text_width(bc, scale)).unwrap_or(0.0);
                    let (prefix_str, content) = split_label(label);
                    let prefix_w = fr.measure_text_width(prefix_str, scale);
                    let rest_w = (max_content_w - 4.0 * em_width - indicator_w).max(1.0);
                    let first_w = (rest_w - bc_w - prefix_w).max(1.0);
                    (fr.wrap_lines_with_offsets_hanging(content, scale, first_w, rest_w).len().max(1), None)
                }
            };
            let highlight_w = (max_prefix_px + item_max_w + 20.0).min(win_w - content_x - list_indent_px);
            metrics.push((y, content_start_x, lines, highlight_w));
            layouts.push(img_layout);
            y += lines as f32 * line_height as f32;
        }
        (metrics, layouts)
    };

    // Cache layout metrics for handler use
    app.renderer.window_height = win_h as i32;
    app.renderer.cached_line_height = line_height;
    app.renderer.cached_line_counts = line_counts.clone();

    // ---- Begin rendering --------------------------------------------------
    let fr = match app.font_renderer.as_mut() { Some(f) => f, None => return };
    let mut rr_opt = app.rect_renderer.as_mut();

    fr.begin_text_rendering();
    if let Some(rr) = rr_opt.as_deref_mut() {
        rr.begin_rect_rendering();
    }
    if let Some(ir) = app.image_renderer.as_mut() {
        ir.begin_image_rendering();
    }

    // ---- Header separator line (between header and content) --------------
    if let Some(rr) = app.rect_renderer.as_mut() {
        rr.prepare_rectangle(0.0, line_height as f32 + top_offset, win_w, 1.0, p.header_sep, 0.0);
    }

    // ---- Header text -----------------------------------------------------
    let header_baseline = (ascender * scale + crate::text::TEXT_PADDING) as f32 + top_offset;
    fr.prepare_text_for_rendering(&header, text_x, header_baseline, scale, p.text);

    // ---- Error message (right of header) ---------------------------------
    if !error_msg.is_empty() {
        let err_x = text_x + (header.len() as f32 * fr.get_width_em(scale)) + 10.0;
        fr.prepare_text_for_rendering(&error_msg, err_x, header_baseline, scale, p.error);
    }

    // ---- Parent element (when navigated into a child level) ---------------
    if !parent_info.display_text.is_empty() && search_str.is_none() {
        let parent_y = line_height as f32 + ascender * scale + crate::text::TEXT_PADDING + top_offset;
        let parent_display = if parent_info.is_radio {
            let state = parent_info.radio_summary.as_deref().unwrap_or("");
            format!("{} [{}]", parent_info.display_text, state)
        } else {
            parent_info.display_text.clone()
        };
        fr.prepare_text_for_rendering(&parent_display, text_x, parent_y, scale, p.text);
        if let Some(ref summary) = parent_info.radio_summary {
            let indent = fr.measure_text_width("    ", scale);
            let summary_x = text_x + indent;
            let summary_y = parent_y + line_height as f32;
            let display = format!("-rc {}", summary);
            let indicator_offset = if let Some(rr) = app.rect_renderer.as_mut() {
                render_radio_indicator(rr, &RadioType::Checked, summary_x, summary_y, scale, ascender, line_height as f32, em_width, &p)
            } else { 0.0 };
            fr.prepare_text_for_rendering(&display, summary_x + indicator_offset, summary_y, scale, p.text);
        }
    }

    // ---- Search / command line -------------------------------------------
    if let Some(ref s) = search_str {
        let search_y = line_height as f32 + ascender * scale + crate::text::TEXT_PADDING + top_offset;
        fr.prepare_text_for_rendering(s, text_x, search_y, scale, p.text);
    }

    // ---- List items — selection highlight rectangles ----------------------
    for (i, (_, _, is_selected, _, _)) in list_items[start_index..].iter().take(item_metrics.len()).enumerate() {
        if !is_selected { continue; }
        // In insert mode the highlight is deferred: only the input buffer portion
        // is highlighted (drawn in the text pass below), not the full row.
        if in_insert_mode { continue; }
        let (item_y, content_start_x, lines, highlight_w) = item_metrics[i];
        if let Some(layout) = &image_layouts[i] {
            // Tight-fitting selection background around image + prefix/suffix text.
            let bg_top = item_y - ascender * scale - crate::text::TEXT_PADDING
                - if layout.prefix_lines == 0 { crate::text::TEXT_PADDING } else { 0.0 };
            let img_y = if layout.prefix_lines > 0 {
                bg_top + layout.prefix_lines as f32 * line_height as f32
            } else {
                item_y - ascender * scale - crate::text::TEXT_PADDING
            };
            let bg_left = content_x + 4.0 * em_width;
            let bg_right = content_start_x + layout.img_w + crate::text::TEXT_PADDING;
            let img_display_h = layout.image_lines as f32 * line_height as f32;
            let bg_bottom = if layout.suffix_lines > 0 {
                img_y + img_display_h + layout.suffix_lines as f32 * line_height as f32
            } else {
                img_y + layout.img_h + crate::text::TEXT_PADDING
            };
            let bg_w = bg_right - bg_left;
            let bg_h = bg_bottom - bg_top;
            if let Some(rr) = app.rect_renderer.as_mut() {
                rr.prepare_rectangle(bg_left, bg_top, bg_w, bg_h, p.selected, 5.0);
                // Square off top-right corner when no prefix text
                if layout.prefix_lines == 0 {
                    rr.prepare_rectangle(bg_right - 5.0, bg_top, 5.0, 5.0, p.selected, 0.0);
                }
                // Square off bottom-right corner when no suffix text
                if layout.suffix_lines == 0 {
                    rr.prepare_rectangle(bg_right - 5.0, bg_top + bg_h - 5.0, 5.0, 5.0, p.selected, 0.0);
                }
            }
        } else {
            let rect_y = item_y - ascender * scale - crate::text::TEXT_PADDING;
            let rect_h = lines as f32 * line_height as f32;
            if let Some(rr) = app.rect_renderer.as_mut() {
                rr.prepare_rectangle(content_x + 4.0 * em_width, rect_y, highlight_w, rect_h, p.selected, 5.0);
            }
        }
    }

    // ---- List items — text / images ----------------------------------------
    let item_prefix_x = text_x + 4.0 * em_width;
    // Positions written during insert-mode rendering, read by caret/selection renderers below.
    let mut captured_elem_x: f32 = 0.0;
    let mut captured_elem_base_x: f32 = 0.0;
    let mut captured_elem_y: f32 = 0.0;
    for (i, (label, item_data, is_selected, match_pos, ext_prefix)) in list_items[start_index..].iter().take(item_metrics.len()).enumerate() {
        let (item_y, content_start_x, _, _) = item_metrics[i];

        // Draw graphical indicator (radio/checkbox) and compute x shift.
        // When any item has an indicator, all items shift their text right by the
        // same indicator_width so prefixes align across the entire list.
        let radio = get_radio_type(label);
        let checkbox = get_checkbox_type(label);
        if radio != RadioType::None {
            if let Some(rr) = app.rect_renderer.as_mut() {
                render_radio_indicator(rr, &radio, item_prefix_x, item_y, scale, ascender, line_height as f32, em_width, &p);
            }
        } else if checkbox != CheckboxType::None {
            if let Some(rr) = app.rect_renderer.as_mut() {
                render_checkbox_indicator(rr, &checkbox, item_prefix_x, item_y, scale, ascender, line_height as f32, em_width, &p);
            }
        }
        let text_prefix_x = if list_has_indicators {
            item_prefix_x + indicator_width(line_height as f32, em_width)
        } else {
            item_prefix_x
        };

        if is_extended_search {
            // item_data is a breadcrumb, not an image path; ext_prefix carries
            // an image path for image elements.
            let right_edge = text_x + max_content_w;
            let (prefix_str, content) = split_label(label.as_str());
            if let Some(path) = ext_prefix {
                // Image result, laid out like general mode: breadcrumb + the
                // image's prefix text (tag + caption) on line 1, the image
                // below, then any trailing (suffix) text under it.
                let (prefix_text, suffix_text, _) = split_image_label(label.as_str(), path);
                let layout = image_layouts[i].as_ref();
                // Match positions run over the whole label; split them around
                // the image path into the caption-prefix and caption-suffix
                // ranges so the search term highlights in both.
                let prefix_char_count = prefix_text.chars().count() as u32;
                let path_char_count = path.chars().count() as u32;
                let prefix_positions: Vec<u32> = match_pos.iter()
                    .copied().filter(|&pp| pp < prefix_char_count).collect();
                let suffix_positions: Vec<u32> = match_pos.iter()
                    .filter(|&&pp| pp >= prefix_char_count + path_char_count)
                    .map(|&pp| pp - prefix_char_count - path_char_count)
                    .collect();
                if let Some(fr) = app.font_renderer.as_mut() {
                    let mut label_x = text_prefix_x;
                    if let Some(breadcrumb) = item_data.as_deref().filter(|s: &&str| !s.is_empty()) {
                        let bc_w = fr.measure_text_width(breadcrumb, scale);
                        fr.prepare_text_for_rendering(breadcrumb, label_x, item_y, scale, p.ext_search);
                        label_x += bc_w;
                    }
                    if prefix_positions.is_empty() {
                        fr.prepare_text_for_rendering(prefix_text, label_x, item_y, scale, p.text);
                    } else {
                        let rr = app.rect_renderer.as_mut();
                        render_with_highlights(fr, rr, prefix_text, label_x, item_y, scale,
                            ascender, line_height as f32, p.text, p.scroll_search,
                            &prefix_positions, None);
                    }
                }
                // Image one line below the breadcrumb/prefix row.
                let img_top_y = item_y + line_height as f32;
                if let (Some(ir), Some(layout)) = (app.image_renderer.as_mut(), layout) {
                    let img_y = img_top_y - ascender * scale - crate::text::TEXT_PADDING;
                    let border = 2.0_f32;
                    unsafe {
                        ir.prepare_image(path, text_prefix_x + border, img_y + border,
                                         layout.img_w - 2.0 * border, layout.img_h - 2.0 * border);
                    }
                }
                // Trailing text below the image.
                if !suffix_text.is_empty() {
                    if let (Some(fr), Some(layout)) = (app.font_renderer.as_mut(), layout) {
                        let suffix_y = img_top_y + layout.image_lines as f32 * line_height as f32;
                        let img_w = layout.img_w.max(1.0);
                        if suffix_positions.is_empty() {
                            fr.prepare_text_wrapped(suffix_text, text_prefix_x, suffix_y, scale,
                                                    img_w, line_height as f32, p.text);
                        } else {
                            let rr = app.rect_renderer.as_mut();
                            render_with_highlights(fr, rr, suffix_text, text_prefix_x, suffix_y,
                                scale, ascender, line_height as f32, p.text, p.scroll_search,
                                &suffix_positions,
                                Some(WrapLayout { first_width: img_w, rest_x: text_prefix_x, rest_width: img_w }));
                        }
                    }
                }
            } else if let Some(fr) = app.font_renderer.as_mut() {
                // Render: [breadcrumb in ext_search color][prefix][content].
                let mut label_x = text_prefix_x;
                if let Some(breadcrumb) = item_data.as_deref().filter(|s: &&str| !s.is_empty()) {
                    let bc_w = fr.measure_text_width(breadcrumb, scale);
                    fr.prepare_text_for_rendering(breadcrumb, label_x, item_y, scale, p.ext_search);
                    label_x += bc_w;
                }
                fr.prepare_text_for_rendering(prefix_str, label_x, item_y, scale, p.text);
                let content_x = label_x + fr.measure_text_width(prefix_str, scale);
                let first_w = (right_edge - content_x).max(1.0);
                let rest_w = (right_edge - text_prefix_x).max(1.0);
                // Adjust match positions to be relative to content (subtract prefix char count)
                let prefix_char_count = prefix_str.chars().count() as u32;
                let content_positions: Vec<u32> = match_pos.iter()
                    .filter(|&&p| p >= prefix_char_count)
                    .map(|&p| p - prefix_char_count)
                    .collect();
                let rr = app.rect_renderer.as_mut();
                // First content line trails the breadcrumb + prefix; wrapped
                // continuation lines run full-width below, at the left margin.
                render_with_highlights(
                    fr, rr, content, content_x, item_y, scale, ascender, line_height as f32,
                    p.text, p.scroll_search, &content_positions,
                    Some(WrapLayout { first_width: first_w, rest_x: text_prefix_x, rest_width: rest_w }),
                );
            }
        } else if let Some(path) = item_data {
            let (prefix_text, suffix_text, has_prefix) = split_image_label(label, path);
            let (prefix_lines, img_h_precomp) = image_layouts[i]
                .as_ref()
                .map(|l| (l.prefix_lines, l.img_h))
                .unwrap_or((0, 0.0));
            // Match positions run over the whole label; the path splits them
            // into caption-prefix and caption-suffix ranges.
            let prefix_char_count = prefix_text.chars().count() as u32;
            let path_char_count = path.chars().count() as u32;

            // Render prefix text above image (or bare "-p" when no meaningful prefix).
            // The "-p" list tag always renders at text_prefix_x; content text at content_start_x.
            let mut current_y = item_y;
            if has_prefix {
                let (tag, content) = split_label(prefix_text);
                // Content trails the "-p " tag — shift match positions past it.
                let tag_char_count = tag.chars().count() as u32;
                let content_positions: Vec<u32> = match_pos.iter()
                    .filter(|&&pp| pp >= tag_char_count && pp < prefix_char_count)
                    .map(|&pp| pp - tag_char_count)
                    .collect();
                if let Some(fr) = app.font_renderer.as_mut() {
                    fr.prepare_text_for_rendering(tag, text_prefix_x, current_y, scale, p.text);
                    if !content.is_empty() {
                        if content_positions.is_empty() {
                            fr.prepare_text_for_rendering(content, content_start_x, current_y, scale, p.text);
                        } else {
                            let rr = app.rect_renderer.as_mut();
                            render_with_highlights(fr, rr, content, content_start_x, current_y,
                                scale, ascender, line_height as f32, p.text, p.scroll_search,
                                &content_positions, None);
                        }
                    }
                }
                current_y += prefix_lines as f32 * line_height as f32;
            } else {
                if let Some(fr) = app.font_renderer.as_mut() {
                    fr.prepare_text_for_rendering("-p", text_prefix_x, current_y, scale, p.text);
                }
            }

            // Render image with 2px border inset
            if let Some(ir) = app.image_renderer.as_mut() {
                let img_w = max_content_w.max(1.0);
                let img_h = if img_h_precomp > 0.0 {
                    img_h_precomp
                } else {
                    unsafe { ir.texture_size(path) }
                        .map(|(tw, th)| if tw == 0 { img_w } else { img_w * th as f32 / tw as f32 })
                        .unwrap_or(img_w)
                };
                let img_y = current_y - ascender * scale - crate::text::TEXT_PADDING;
                let border = 2.0_f32;
                unsafe {
                    ir.prepare_image(path, content_start_x + border, img_y + border,
                                     img_w - 2.0 * border, img_h - 2.0 * border);
                }
                current_y += (img_h / line_height as f32).ceil() as f32 * line_height as f32;
            }

            // Render suffix text below image
            if !suffix_text.is_empty() {
                let suffix_positions: Vec<u32> = match_pos.iter()
                    .filter(|&&pp| pp >= prefix_char_count + path_char_count)
                    .map(|&pp| pp - prefix_char_count - path_char_count)
                    .collect();
                if let Some(fr) = app.font_renderer.as_mut() {
                    if suffix_positions.is_empty() {
                        fr.prepare_text_for_rendering(suffix_text, content_start_x, current_y, scale, p.text);
                    } else {
                        let rr = app.rect_renderer.as_mut();
                        render_with_highlights(fr, rr, suffix_text, content_start_x, current_y,
                            scale, ascender, line_height as f32, p.text, p.scroll_search,
                            &suffix_positions, None);
                    }
                }
            }
        } else if let Some(fr) = app.font_renderer.as_mut() {
            if *is_selected && in_insert_mode {
                // Render prefix (non-editable, no highlight)
                let pfx_w = if !insert_prefix.is_empty() {
                    let w = fr.measure_text_width(&insert_prefix, scale);
                    fr.prepare_text_for_rendering(&insert_prefix, text_prefix_x, item_y, scale, p.text);
                    w
                } else {
                    0.0
                };
                let after_prefix_x = text_prefix_x + pfx_w;
                // Store positions for caret/selection rendering
                captured_elem_x = after_prefix_x;
                captured_elem_base_x = text_prefix_x;
                captured_elem_y = item_y;

                // Render input buffer — multiline-aware, with highlight only on the buffer
                let buf = insert_buf.as_str();
                let lh = line_height as f32;
                if let Some(nl_pos) = buf.find('\n') {
                    let first_line = &buf[..nl_pos];
                    let rest = &buf[nl_pos + 1..];
                    let first_text = if first_line.is_empty() { " " } else { first_line };
                    // Highlight first line of buffer
                    let first_w = fr.measure_text_width(first_text, scale);
                    if let Some(rr) = app.rect_renderer.as_mut() {
                        rr.prepare_rectangle(
                            after_prefix_x - crate::text::TEXT_PADDING,
                            item_y - ascender * scale - crate::text::TEXT_PADDING,
                            first_w + 2.0 * crate::text::TEXT_PADDING,
                            lh,
                            p.selected, 5.0,
                        );
                    }
                    fr.prepare_text_for_rendering(first_text, after_prefix_x, item_y, scale, p.text);
                    let mut rest_y = item_y + lh;
                    let mut last_segment = "";
                    for segment in rest.split('\n') {
                        let seg_text = if segment.is_empty() { " " } else { segment };
                        // Highlight each continuation line of buffer
                        let seg_w = fr.measure_text_width(seg_text, scale);
                        if let Some(rr) = app.rect_renderer.as_mut() {
                            rr.prepare_rectangle(
                                text_prefix_x - crate::text::TEXT_PADDING,
                                rest_y - ascender * scale - crate::text::TEXT_PADDING,
                                seg_w + 2.0 * crate::text::TEXT_PADDING,
                                lh,
                                p.selected, 5.0,
                            );
                        }
                        fr.prepare_text_for_rendering(seg_text, text_prefix_x, rest_y, scale, p.text);
                        last_segment = segment;
                        rest_y += lh;
                    }
                    if !insert_suffix.is_empty() {
                        let last_y = rest_y - lh;
                        let last_w = fr.measure_text_width(
                            if last_segment.is_empty() { " " } else { last_segment }, scale,
                        );
                        fr.prepare_text_for_rendering(&insert_suffix, text_prefix_x + last_w, last_y, scale, p.text);
                    }
                } else {
                    let buf_text = if buf.is_empty() { " " } else { buf };
                    let buf_w = fr.measure_text_width(buf_text, scale);
                    // Highlight only the buffer portion
                    if let Some(rr) = app.rect_renderer.as_mut() {
                        rr.prepare_rectangle(
                            after_prefix_x - crate::text::TEXT_PADDING,
                            item_y - ascender * scale - crate::text::TEXT_PADDING,
                            buf_w + 2.0 * crate::text::TEXT_PADDING,
                            lh,
                            p.selected, 5.0,
                        );
                    }
                    fr.prepare_text_for_rendering(buf_text, after_prefix_x, item_y, scale, p.text);
                    if !insert_suffix.is_empty() {
                        fr.prepare_text_for_rendering(&insert_suffix, after_prefix_x + buf_w, item_y, scale, p.text);
                    }
                }
            } else if is_flat_list {
                // Command/Meta: no prefix split — render the full label.
                // Use fuzzy highlights when match positions are available.
                if match_pos.is_empty() {
                    fr.prepare_text_wrapped(label.as_str(), text_prefix_x, item_y, scale, max_content_w.max(1.0), line_height as f32, p.text);
                } else {
                    let rr = app.rect_renderer.as_mut();
                    render_with_highlights(fr, rr, label.as_str(), text_prefix_x, item_y, scale, ascender, line_height as f32, p.text, p.scroll_search, &match_pos, None);
                }
            } else {
                let (prefix, content) = split_label(label.as_str());
                let prefix_char_count = prefix.chars().count() as u32;
                fr.prepare_text_for_rendering(prefix, text_prefix_x, item_y, scale, p.text);
                let content_positions: Vec<u32> = match_pos.iter()
                    .filter(|&&p| p >= prefix_char_count)
                    .map(|&p| p - prefix_char_count)
                    .collect();
                if content_positions.is_empty() {
                    fr.prepare_text_wrapped(content, content_start_x, item_y, scale, max_content_w.max(1.0), line_height as f32, p.text);
                } else {
                    let rr = app.rect_renderer.as_mut();
                    render_with_highlights(fr, rr, content, content_start_x, item_y, scale, ascender, line_height as f32, p.text, p.scroll_search, &content_positions, None);
                }
            }
        }
    }

    // Write back element positions captured during insert-mode rendering
    app.renderer.current_element_x = captured_elem_x;
    app.renderer.current_element_base_x = captured_elem_base_x;
    app.renderer.current_element_y = captured_elem_y;

    // ---- Selection highlight rectangles (behind text, rendered now) ----------
    // Selection highlights for search/command/insert modes.
    let has_sel = handlers::has_selection(&app.renderer);
    if has_sel {
        if let Some((sel_start, sel_end)) = handlers::selection_range(&app.renderer) {
            // base_y is the cell-top Y (not the baseline) so selection rects align with glyphs
            let (base_x, base_y) = if in_insert_mode {
                (captured_elem_x, captured_elem_y - ascender * scale)
            } else {
                let prefix = match app.renderer.coordinate {
                    Coordinate::ExtendedSearch => "ext search: ",
                    Coordinate::Command => "search: ",
                    _ => "search: ",
                };
                let pfx_w = app.font_renderer.as_ref()
                    .map(|fr| fr.measure_text_width(prefix, scale))
                    .unwrap_or(0.0);
                // search baseline is (line_height + ascender*scale + TEXT_PADDING); shift to cell top
                (text_x + pfx_w, line_height as f32 + crate::text::TEXT_PADDING + top_offset)
            };
            let sel_height = line_height as f32 - 2.0 * crate::text::TEXT_PADDING;
            let search_buf;
            let buf = if app.renderer.coordinate == Coordinate::SimpleSearch {
                search_buf = app.renderer.search_string.clone();
                search_buf.as_str()
            } else {
                insert_buf.as_str()
            };

            // Build line-start offsets
            let mut line_starts: Vec<usize> = vec![0];
            for (i, c) in buf.char_indices() {
                if c == '\n' { line_starts.push(i + 1); }
            }
            let num_lines = line_starts.len();

            // Find start/end lines
            let start_line = line_starts.partition_point(|&s| s <= sel_start).saturating_sub(1);
            let end_line = line_starts.partition_point(|&s| s <= sel_end).saturating_sub(1);

            if let Some(fr) = app.font_renderer.as_ref() {
                for line in start_line..=end_line {
                    let line_start_off = line_starts[line];
                    let line_end_off = if line + 1 < num_lines { line_starts[line + 1] - 1 } else { buf.len() };
                    let clamp_start = sel_start.max(line_start_off);
                    let clamp_end = sel_end.min(line_end_off);
                    let line_x = if in_insert_mode && line > 0 { captured_elem_base_x } else { base_x };
                    let line_y = base_y + line as f32 * line_height as f32;

                    let x_start = if clamp_start > line_start_off {
                        line_x + fr.measure_text_width(&buf[line_start_off..clamp_start], scale)
                    } else {
                        line_x
                    };
                    let x_end = if clamp_end > line_start_off {
                        line_x + fr.measure_text_width(&buf[line_start_off..clamp_end], scale)
                    } else {
                        line_x
                    };
                    let sel_w = x_end - x_start;
                    if sel_w > 0.0 {
                        if let Some(rr) = app.rect_renderer.as_mut() {
                            rr.prepare_rectangle(x_start, line_y, sel_w, sel_height, p.scroll_search, 0.0);
                        }
                    }
                }
            }
        }
    }

    // ---- Caret rectangle (on top of text) ------------------------------------
    if caret_visible {
        let lh = line_height as f32;
        let caret_h = lh - 2.0 * crate::text::TEXT_PADDING;

        if in_insert_mode {
            // Insert mode caret using stored element position
            let buf = insert_buf.as_str();
            let pos = insert_cursor.min(buf.len());
            // Count newlines before cursor
            let cursor_line = buf[..pos].chars().filter(|&c| c == '\n').count();
            let line_start_off = buf[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let line_x = if cursor_line == 0 { captured_elem_x } else { captured_elem_base_x };
            let col_text = &buf[line_start_off..pos];
            let caret_x = if let Some(fr) = app.font_renderer.as_ref() {
                line_x + fr.measure_text_width(col_text, scale)
            } else {
                line_x
            };
            // Shift from baseline to cell top + padding so the caret sits inside the row
            let caret_y = captured_elem_y - ascender * scale
                + cursor_line as f32 * lh;
            if let Some(rr) = app.rect_renderer.as_mut() {
                rr.prepare_rectangle(caret_x, caret_y, 2.0, caret_h, p.text, 0.0);
            }
        } else if matches!(
            app.renderer.coordinate,
            Coordinate::SimpleSearch | Coordinate::ExtendedSearch | Coordinate::Command
                | Coordinate::ScrollSearch | Coordinate::InputSearch | Coordinate::TabSwitcher
        ) {
            // Search/command caret after the prefix
            let (prefix, buf, cursor) = match app.renderer.coordinate {
                Coordinate::Command => ("search: ", insert_buf.as_str(), insert_cursor),
                Coordinate::ExtendedSearch => ("ext search: ", insert_buf.as_str(), insert_cursor),
                Coordinate::TabSwitcher => ("switch tab: ", insert_buf.as_str(), insert_cursor),
                _ => ("search: ", app.renderer.search_string.as_str(), insert_cursor),
            };
            // search_y is the baseline — shift to cell top + padding
            let search_y = line_height as f32 + ascender * scale + crate::text::TEXT_PADDING + top_offset;
            let caret_top_y = search_y - ascender * scale;
            if let Some(fr) = app.font_renderer.as_ref() {
                let pfx_w = fr.measure_text_width(prefix, scale);
                let base_x = text_x + pfx_w;
                let col_text = &buf[..cursor.min(buf.len())];
                let caret_x = base_x + fr.measure_text_width(col_text, scale);
                if let Some(rr) = app.rect_renderer.as_mut() {
                    rr.prepare_rectangle(caret_x, caret_top_y, 2.0, caret_h, p.text, 0.0);
                }
            }
        }
    }

    // Suppress unused-variable warnings for snapshots only used by caret/selection
    let _ = (insert_sel, insert_prefix, insert_suffix);
}

// ---------------------------------------------------------------------------
// Scroll mode rendering
// ---------------------------------------------------------------------------

/// Per-item vertical layout for scroll mode. Each item renders its extended
/// prefix line(s) (`layer: X list: Y/Z`) followed by the element content —
/// wrapped text for text items, a scaled image (with caption text above and
/// below it) for image items.
struct ScrollItemLayout {
    /// Virtual-space (pre-scroll) pixel top of the item.
    top: i32,
    /// Full item height in pixels (prefix + content).
    height: i32,
    /// Number of wrapped lines the extended prefix occupies.
    prefix_lines: usize,
    /// Number of wrapped lines the content occupies — text rows for text
    /// items, or caption-prefix + image + caption-suffix rows for image items.
    content_lines: usize,
    /// Image layout for image items; `None` for text items.
    img: Option<ScrollImg>,
}

/// Image-item layout within a scroll-mode row: the scaled image plus the
/// wrapped caption text rendered above (prefix) and below (suffix) it.
struct ScrollImg {
    /// `(display_w, display_h)` of the scaled image.
    img_w: f32,
    img_h: f32,
    /// Number of line-height rows the image itself occupies.
    image_lines: usize,
    /// Wrapped lines of caption text rendered above the image.
    caption_prefix_lines: usize,
    /// Wrapped lines of caption text rendered below the image.
    caption_suffix_lines: usize,
}

/// Display caption text wrapped around an image item's path: `(prefix, suffix)`,
/// each trimmed. `prefix` is the text between the `-p` list tag and the path;
/// `suffix` is the text after the path. Either may be empty.
fn scroll_image_caption<'a>(label: &'a str, path: &str) -> (&'a str, &'a str) {
    let (prefix_text, suffix_text, _) = split_image_label(label, path);
    let (_, caption_prefix) = split_label(prefix_text);
    (caption_prefix.trim(), suffix_text.trim())
}

/// Element content searched by Ctrl-F (`Content`) vs Tab (`Prefix`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScrollSearchCorpus {
    Content,
    Prefix,
}

/// Display-stripped `(list_prefix, content)` parts of a scroll-mode label.
/// `list_prefix` is the type tag (`-`, `+`, `-c`, …) with no trailing space;
/// `content` is the element text. Mirrors general-mode column alignment.
fn scroll_item_parts(label: &str) -> (String, String) {
    let display = sicompass_sdk::tags::strip_display(label);
    let (prefix, content) = split_label(&display);
    (prefix.trim_end().to_string(), content.to_string())
}

/// Compute per-item layout for the scroll-mode list. Shared by the plain
/// scroll renderer and the scroll search renderers.
fn measure_scroll_items(
    fr: &crate::text::FontRenderer,
    ir: Option<&mut crate::image::ImageRenderer>,
    list_items: &[(String, Option<String>, bool, Vec<u32>, Option<String>)],
    scale: f32,
    line_height: i32,
    max_w: f32,
) -> (Vec<ScrollItemLayout>, i32) {
    let mut ir = ir;
    let mut layouts = Vec::with_capacity(list_items.len());
    let mut y_accum: i32 = 0;
    for (label, img_data, _, _, ext_prefix) in list_items.iter() {
        let prefix_text = ext_prefix.as_deref().unwrap_or("");
        let prefix_lines = fr.count_wrapped_lines(prefix_text, scale, max_w).max(1);
        let (img, content_lines) = if let Some(path) = img_data {
            let img_w = max_w;
            let img_h = ir
                .as_deref_mut()
                .and_then(|ir| unsafe { ir.texture_size(path) })
                .map(|(tw, th)| if tw == 0 { img_w } else { img_w * th as f32 / tw as f32 })
                .unwrap_or(img_w);
            let image_lines = ((img_h / line_height as f32).ceil() as usize).max(1);
            let (caption_prefix, caption_suffix) = scroll_image_caption(label, path);
            let caption_prefix_lines = if caption_prefix.is_empty() {
                0
            } else {
                fr.count_wrapped_lines(caption_prefix, scale, max_w).max(1)
            };
            let caption_suffix_lines = if caption_suffix.is_empty() {
                0
            } else {
                fr.count_wrapped_lines(caption_suffix, scale, max_w).max(1)
            };
            let content_lines = caption_prefix_lines + image_lines + caption_suffix_lines;
            (
                Some(ScrollImg { img_w, img_h, image_lines, caption_prefix_lines, caption_suffix_lines }),
                content_lines,
            )
        } else {
            let (_, content) = scroll_item_parts(label);
            (None, fr.count_wrapped_lines(&content, scale, max_w).max(1))
        };
        let height = (prefix_lines + content_lines) as i32 * line_height;
        layouts.push(ScrollItemLayout { top: y_accum, height, prefix_lines, content_lines, img });
        y_accum += height;
    }
    (layouts, y_accum)
}

/// Render one scroll-mode item: the list prefix in the left column (a graphical
/// radio/checkbox indicator plus its `-c`/`-r` tag, or a plain `-`/`+` tag), the
/// extended prefix (`layer: X list: Y/Z`) on the same first line, then the
/// element content below — wrapped text, or an image's caption prefix text,
/// the scaled image, and its caption suffix text — all aligned at `content_x`
/// like general mode. Lines/images outside `[clip_y, win_h)` are clipped.
#[allow(clippy::too_many_arguments)]
fn render_scroll_item(
    fr: &mut crate::text::FontRenderer,
    mut rr: Option<&mut crate::rectangle::RectangleRenderer>,
    ir: Option<&mut crate::image::ImageRenderer>,
    label: &str,
    img_data: &Option<String>,
    ext_prefix: &Option<String>,
    layout: &ScrollItemLayout,
    item_top_screen: f32,
    scale: f32,
    ascender: f32,
    line_height: i32,
    em_width: f32,
    list_has_indicators: bool,
    prefix_x: f32,
    content_x: f32,
    content_w: f32,
    clip_y: f32,
    win_h: f32,
    p: &crate::app_state::ColorPalette,
) {
    let lh = line_height as f32;
    let (list_prefix, content) = scroll_item_parts(label);
    let radio = get_radio_type(label);
    let checkbox = get_checkbox_type(label);
    // When any item in the list carries an indicator, every item reserves the
    // same indicator-width gutter so the text prefixes stay aligned.
    let text_prefix_x = if list_has_indicators {
        prefix_x + indicator_width(lh, em_width)
    } else {
        prefix_x
    };

    // First row: list prefix in the left column, extended prefix at content_x.
    let prefix_text = ext_prefix.as_deref().unwrap_or("");
    for (n, (line, _)) in fr.wrap_lines_with_offsets(prefix_text, scale, content_w).iter().enumerate() {
        let line_top = item_top_screen + n as f32 * lh;
        if line_top + lh <= clip_y { continue; }
        if line_top >= win_h { break; }
        let baseline = line_top + ascender * scale + crate::text::TEXT_PADDING;
        if n == 0 {
            // Graphical radio/checkbox indicator (matches general mode).
            if let Some(rr) = rr.as_deref_mut() {
                if radio != RadioType::None {
                    render_radio_indicator(rr, &radio, prefix_x, baseline, scale, ascender, lh, em_width, p);
                } else if checkbox != CheckboxType::None {
                    render_checkbox_indicator(rr, &checkbox, prefix_x, baseline, scale, ascender, lh, em_width, p);
                }
            }
            if !list_prefix.is_empty() {
                fr.prepare_text_for_rendering(&list_prefix, text_prefix_x, baseline, scale, p.text);
            }
        }
        fr.prepare_text_for_rendering(line, content_x, baseline, scale, p.text);
    }

    let content_top = item_top_screen + layout.prefix_lines as f32 * lh;
    if let (Some(path), Some(img)) = (img_data, layout.img.as_ref()) {
        let (caption_prefix, caption_suffix) = scroll_image_caption(label, path);
        // Caption text rendered above the image, at the content column.
        if !caption_prefix.is_empty() {
            for (n, (line, _)) in fr.wrap_lines_with_offsets(caption_prefix, scale, content_w).iter().enumerate() {
                let line_top = content_top + n as f32 * lh;
                if line_top + lh <= clip_y { continue; }
                if line_top >= win_h { break; }
                let baseline = line_top + ascender * scale + crate::text::TEXT_PADDING;
                fr.prepare_text_for_rendering(line, content_x, baseline, scale, p.text);
            }
        }
        let img_top = content_top + img.caption_prefix_lines as f32 * lh;
        if img_top + img.img_h > clip_y && img_top < win_h {
            if let Some(ir) = ir {
                let border = 2.0_f32;
                // Clip vertically so a scrolled image never bleeds over the
                // header / tabs band (or below the window).
                unsafe {
                    ir.prepare_image_clipped(path, content_x + border, img_top + border,
                                     img.img_w - 2.0 * border, img.img_h - 2.0 * border, clip_y, win_h);
                }
            }
        }
        // Caption text rendered below the image.
        if !caption_suffix.is_empty() {
            let suffix_top = img_top + img.image_lines as f32 * lh;
            for (n, (line, _)) in fr.wrap_lines_with_offsets(caption_suffix, scale, content_w).iter().enumerate() {
                let line_top = suffix_top + n as f32 * lh;
                if line_top + lh <= clip_y { continue; }
                if line_top >= win_h { break; }
                let baseline = line_top + ascender * scale + crate::text::TEXT_PADDING;
                fr.prepare_text_for_rendering(line, content_x, baseline, scale, p.text);
            }
        }
    } else {
        for (n, (line, _)) in fr.wrap_lines_with_offsets(&content, scale, content_w).iter().enumerate() {
            let line_top = content_top + n as f32 * lh;
            if line_top + lh <= clip_y { continue; }
            if line_top >= win_h { break; }
            let baseline = line_top + ascender * scale + crate::text::TEXT_PADDING;
            fr.prepare_text_for_rendering(line, content_x, baseline, scale, p.text);
        }
    }
}

/// Render the recursively-flattened list with pixel-smooth scrolling.
/// Each item shows its extended prefix line then its content (text or image).
/// Returns `(total_height_px, resolved_scroll_offset_px)`.
#[allow(clippy::too_many_arguments)]
fn render_scroll_full(
    fr: &mut crate::text::FontRenderer,
    mut rr: Option<&mut crate::rectangle::RectangleRenderer>,
    mut ir: Option<&mut crate::image::ImageRenderer>,
    list_items: &[(String, Option<String>, bool, Vec<u32>, Option<String>)],
    list_index: usize,
    text_scroll_offset: i32,
    scale: f32,
    line_height: i32,
    ascender: f32,
    em_width: f32,
    text_x: f32,
    max_prefix_px: f32,
    max_content_w: f32,
    win_h: f32,
    top_offset: f32,
    p: &crate::app_state::ColorPalette,
) -> (i32, i32) {
    let clip_y = line_height as f32 + top_offset;
    let viewport_h = win_h - clip_y;
    let max_w = max_content_w.max(1.0);
    // List prefix sits in a left column; content + extended prefix align here.
    let content_x = text_x + max_prefix_px;
    let list_has_indicators = scroll_list_has_indicators(list_items);

    let (layouts, total_height) =
        measure_scroll_items(fr, ir.as_deref_mut(), list_items, scale, line_height, max_w);

    // Resolve sentinel (-1): place the selected item at viewport top.
    let max_offset = (total_height - viewport_h as i32).max(0);
    let scroll_offset = if text_scroll_offset < 0 {
        layouts.get(list_index).map(|l| l.top).unwrap_or(0).min(max_offset)
    } else {
        text_scroll_offset.clamp(0, max_offset)
    };

    // Plain scroll mode is a continuous reading view — no focused-element
    // highlight (search sub-modes highlight matches instead).

    // Render visible items.
    for (i, (label, img_data, _, _, ext_prefix)) in list_items.iter().enumerate() {
        let l = &layouts[i];
        let item_top_screen = clip_y + (l.top - scroll_offset) as f32;
        if item_top_screen + l.height as f32 <= clip_y { continue; } // above viewport
        if item_top_screen >= win_h { break; }                        // below viewport
        render_scroll_item(fr, rr.as_deref_mut(), ir.as_deref_mut(), label, img_data, ext_prefix, l,
            item_top_screen, scale, ascender, line_height, em_width, list_has_indicators,
            text_x, content_x, max_w, clip_y, win_h, p);
    }

    (total_height, scroll_offset)
}

/// True when any item in the scroll list is a radio or checkbox — then every
/// item reserves an indicator-width gutter so prefixes stay column-aligned.
fn scroll_list_has_indicators(
    list_items: &[(String, Option<String>, bool, Vec<u32>, Option<String>)],
) -> bool {
    list_items.iter().any(|(label, _, _, _, _)| {
        get_radio_type(label) != RadioType::None || get_checkbox_type(label) != CheckboxType::None
    })
}

struct ScrollSearchResult {
    total_height: i32,
    match_count: usize,
    current_match: usize,
    /// List index of the item containing the current match — written back to
    /// `list_index` so Enter navigates to the right element.
    current_item: usize,
    scroll_offset: i32,
}

/// One searchable text region within a scroll-search row. Text items and the
/// Prefix corpus contribute a single segment per item; an image item under the
/// Content corpus contributes two — the caption text above the image and the
/// caption text below it — since the image splits them into separate regions.
struct SearchSegment {
    /// Index into `list_items` of the row this segment belongs to.
    item_idx: usize,
    /// The searchable text.
    text: String,
    /// Wrapped lines of `text`: `(line, byte_offset_into_text)`.
    wrap: Vec<(String, usize)>,
    /// Virtual-space (pre-scroll) pixel top of the segment's first line.
    virtual_top: i32,
}

/// A single case-insensitive query hit located within a `SearchSegment`.
struct ScrollMatch {
    seg_idx: usize,
    item_idx: usize,
    byte_off: usize,
    mlen: usize,
    /// Virtual-space pixel top of the wrapped line containing the hit.
    virtual_y: i32,
}

/// Find all case-insensitive occurrences of `query` in `text`.
/// Returns `(byte_offset, match_len)` pairs — both are byte indices into the
/// original `text` and always land on char boundaries (matching is done
/// char-by-char on the original text, so multi-byte characters such as `—` do
/// not produce invalid slice indices).
fn find_matches_ci(text: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    // Per original char: (byte offset, byte length, lowercased form).
    let chars: Vec<(usize, usize, String)> = text
        .char_indices()
        .map(|(i, c)| (i, c.len_utf8(), c.to_lowercase().collect::<String>()))
        .collect();
    let query_lower = query.to_lowercase();
    let mut matches = Vec::new();
    for start in 0..chars.len() {
        let mut q = query_lower.as_str();
        let mut ci = start;
        let mut ok = true;
        while !q.is_empty() {
            match chars.get(ci) {
                Some((_, _, lc)) => match q.strip_prefix(lc.as_str()) {
                    Some(rest) => { q = rest; ci += 1; }
                    None => { ok = false; break; }
                },
                None => { ok = false; break; }
            }
        }
        if ok && ci > start {
            let start_off = chars[start].0;
            let end_off = chars[ci - 1].0 + chars[ci - 1].1;
            matches.push((start_off, end_off - start_off));
        }
    }
    matches
}

/// Render a scroll search across the flattened list. `corpus` selects whether
/// the query matches element content (Ctrl-F) or extended prefixes (Tab).
/// Returns computed state for write-back.
#[allow(clippy::too_many_arguments)]
fn render_scroll_search_full(
    fr: &mut crate::text::FontRenderer,
    mut rr: Option<&mut crate::rectangle::RectangleRenderer>,
    mut ir: Option<&mut crate::image::ImageRenderer>,
    list_items: &[(String, Option<String>, bool, Vec<u32>, Option<String>)],
    list_index: usize,
    text_scroll_offset: i32,
    search_query: &str,
    cursor_position: usize,
    caret_visible: bool,
    selection: Option<(usize, usize)>,
    search_current_match: usize,
    needs_position: bool,
    snap: bool,
    corpus: ScrollSearchCorpus,
    scale: f32,
    line_height: i32,
    ascender: f32,
    em_width: f32,
    text_x: f32,
    max_prefix_px: f32,
    max_content_w: f32,
    win_h: f32,
    top_offset: f32,
    p: &crate::app_state::ColorPalette,
) -> ScrollSearchResult {
    // Tabs band + header + search bar occupy three line_heights from the top.
    let clip_y = line_height as f32 * 2.0 + top_offset;
    let viewport_h = win_h - clip_y;
    let max_w = max_content_w.max(1.0);
    let lh = line_height as f32;
    // List prefix sits in a left column; content + extended prefix align here.
    let content_x = text_x + max_prefix_px;
    let list_has_indicators = scroll_list_has_indicators(list_items);

    let (layouts, total_height) =
        measure_scroll_items(fr, ir.as_deref_mut(), list_items, scale, line_height, max_w);

    // Build the searchable segments. Each segment's text is exactly what the
    // item renders at `content_x`, so wrap offsets and highlight rects line up.
    // Content corpus (Ctrl-F): element content for text items, or the caption
    // text above/below the image for image items. Prefix corpus (Tab): the
    // extended prefix. Segments are appended top-to-bottom so the resulting
    // match list is ordered by vertical position.
    let mut segments: Vec<SearchSegment> = Vec::with_capacity(list_items.len());
    for (item_idx, (label, img_data, _, _, ext_prefix)) in list_items.iter().enumerate() {
        let l = &layouts[item_idx];
        let mut push_segment = |text: String, virtual_top: i32, fr: &crate::text::FontRenderer| {
            let wrap = fr.wrap_lines_with_offsets(&text, scale, max_w);
            segments.push(SearchSegment { item_idx, text, wrap, virtual_top });
        };
        match corpus {
            ScrollSearchCorpus::Prefix => {
                push_segment(ext_prefix.clone().unwrap_or_default(), l.top, fr);
            }
            ScrollSearchCorpus::Content => {
                let content_top = l.top + l.prefix_lines as i32 * line_height;
                if let (Some(path), Some(img)) = (img_data, l.img.as_ref()) {
                    let (caption_prefix, caption_suffix) = scroll_image_caption(label, path);
                    if !caption_prefix.is_empty() {
                        push_segment(caption_prefix.to_string(), content_top, fr);
                    }
                    if !caption_suffix.is_empty() {
                        let suffix_top = content_top
                            + (img.caption_prefix_lines + img.image_lines) as i32 * line_height;
                        push_segment(caption_suffix.to_string(), suffix_top, fr);
                    }
                } else {
                    push_segment(scroll_item_parts(label).1, content_top, fr);
                }
            }
        }
    }

    // Collect all matches, ordered by vertical position (segments are already
    // ordered, and `find_matches_ci` returns hits left-to-right).
    let mut all_matches: Vec<ScrollMatch> = Vec::new();
    for (seg_idx, seg) in segments.iter().enumerate() {
        for (byte_off, mlen) in find_matches_ci(&seg.text, search_query) {
            let li = seg.wrap.partition_point(|(_, off)| *off <= byte_off).saturating_sub(1);
            let li = li.min(seg.wrap.len().saturating_sub(1));
            let virtual_y = seg.virtual_top + li as i32 * line_height;
            all_matches.push(ScrollMatch { seg_idx, item_idx: seg.item_idx, byte_off, mlen, virtual_y });
        }
    }
    let match_count = all_matches.len();

    let viewport_top = if text_scroll_offset < 0 {
        layouts.get(list_index).map(|l| l.top).unwrap_or(0)
    } else {
        text_scroll_offset
    };

    // Select current match (see original scroll-search docs): on entry/typing
    // pick the first match visible in the viewport; on explicit nav use clamped.
    let current_match = if match_count == 0 {
        0
    } else {
        let clamped = search_current_match.min(match_count - 1);
        if needs_position {
            all_matches.iter()
                .position(|m| m.virtual_y + line_height > viewport_top)
                .unwrap_or(0)
        } else {
            clamped
        }
    };
    let current_item = if match_count == 0 {
        list_index
    } else {
        all_matches[current_match].item_idx
    };

    // Snap the viewport only on explicit Up/Down navigation.
    let max_offset = (total_height - viewport_h as i32).max(0);
    let scroll_offset = if snap && match_count > 0 {
        let match_item = all_matches[current_match].item_idx;
        layouts.get(match_item).map(|l| l.top).unwrap_or(0).clamp(0, max_offset)
    } else {
        viewport_top.clamp(0, max_offset)
    };

    // Search bar at line 1 (below the header separator, under the tabs band).
    let search_bar_y = line_height as f32 + ascender * scale + crate::text::TEXT_PADDING + top_offset;
    let bar_label = match corpus {
        ScrollSearchCorpus::Content => "search",
        ScrollSearchCorpus::Prefix => "prefix search",
    };
    let search_bar = format!("{bar_label}: {search_query} [{match_count} items]");
    fr.prepare_text_for_rendering(&search_bar, text_x, search_bar_y, scale, p.text);

    // Text-selection highlight + blinking caret in the search input.
    if let Some(rr) = rr.as_deref_mut() {
        let base_x = text_x + fr.measure_text_width(&format!("{bar_label}: "), scale);
        let field_top = search_bar_y - ascender * scale;
        let field_h = lh - 2.0 * crate::text::TEXT_PADDING;
        if let Some((sel_start, sel_end)) = selection {
            let s = sel_start.min(search_query.len());
            let e = sel_end.min(search_query.len());
            if e > s {
                let sel_x = base_x + fr.measure_text_width(&search_query[..s], scale);
                let sel_w = fr.measure_text_width(&search_query[s..e], scale);
                rr.prepare_rectangle(sel_x, field_top, sel_w, field_h, p.scroll_search, 0.0);
            }
        }
        if caret_visible {
            let cur = cursor_position.min(search_query.len());
            let caret_x = base_x + fr.measure_text_width(&search_query[..cur], scale);
            rr.prepare_rectangle(caret_x, field_top, 2.0, field_h, p.text, 0.0);
        }
    }

    // Match highlights, drawn first so item text renders on top of them. Each
    // match is placed from its segment's virtual top, so highlights on an image
    // item's caption-suffix segment sit correctly below the image.
    if let Some(rr) = rr.as_deref_mut() {
        for (mi, m) in all_matches.iter().enumerate() {
            let seg = &segments[m.seg_idx];
            let li = seg.wrap.partition_point(|(_, off)| *off <= m.byte_off).saturating_sub(1);
            let li = li.min(seg.wrap.len().saturating_sub(1));
            let (line_text, line_byte_off) = match seg.wrap.get(li) {
                Some((t, o)) => (t.as_str(), *o),
                None => continue,
            };
            let line_top = clip_y + (m.virtual_y - scroll_offset) as f32;
            if line_top + lh <= clip_y || line_top >= win_h { continue; }
            let local_start = m.byte_off.saturating_sub(line_byte_off);
            let safe_start = local_start.min(line_text.len());
            let safe_end = (local_start + m.mlen).min(line_text.len());
            let match_x = content_x + fr.measure_text_width(&line_text[..safe_start], scale);
            let match_w = fr.measure_text_width(&line_text[safe_start..safe_end], scale).max(2.0);
            let color = if mi == current_match { p.scroll_search } else { p.selected };
            rr.prepare_rectangle(match_x, line_top, match_w, lh, color, 3.0);
        }
    }

    // Render visible items on top of the highlights.
    for (i, (label, img_data, _, _, ext_prefix)) in list_items.iter().enumerate() {
        let l = &layouts[i];
        let item_top_screen = clip_y + (l.top - scroll_offset) as f32;
        if item_top_screen + l.height as f32 <= clip_y { continue; }
        if item_top_screen >= win_h { break; }

        render_scroll_item(fr, rr.as_deref_mut(), ir.as_deref_mut(), label, img_data, ext_prefix, l,
            item_top_screen, scale, ascender, line_height, em_width, list_has_indicators,
            text_x, content_x, max_w, clip_y, win_h, p);
    }

    ScrollSearchResult { total_height, match_count, current_match, current_item, scroll_offset }
}

/// Convert a packed 0xRRGGBBAA color to `[r, g, b, a]` floats in 0.0..=1.0.
fn rgba_u32_to_f32(c: u32) -> [f32; 4] {
    [
        ((c >> 24) & 0xFF) as f32 / 255.0,
        ((c >> 16) & 0xFF) as f32 / 255.0,
        ((c >>  8) & 0xFF) as f32 / 255.0,
        ( c        & 0xFF) as f32 / 255.0,
    ]
}

/// Build the header status line (mirrors C updateView header format).
fn build_header_text(r: &AppRenderer, line_height: i32) -> String {
    let _ = line_height;
    r.header_text()
}

/// Snapshot the active list for rendering (avoids mixed borrows later).
/// Returns `(label, item_data, is_selected, fuzzy_match_positions, ext_prefix)`.
fn collect_list_items(r: &AppRenderer) -> Vec<(String, Option<String>, bool, Vec<u32>, Option<String>)> {
    let len = r.active_list_len();
    let mut out = Vec::with_capacity(len);
    let has_filter = !r.filtered_list_indices.is_empty();
    for i in 0..len {
        let item = if has_filter {
            r.filtered_list_indices.get(i).and_then(|&raw| r.total_list.get(raw))
        } else {
            r.total_list.get(i)
        };
        if let Some(item) = item {
            let positions = if has_filter {
                r.fuzzy_match_positions.get(i).cloned().unwrap_or_default()
            } else {
                Vec::new()
            };
            out.push((item.label.clone(), item.data.clone(), i == r.list_index, positions, item.ext_prefix.clone()));
        }
    }
    out
}

/// Word-wrap layout for `render_with_highlights`. The first wrapped line keeps
/// the caller's `x` and wraps at `first_width`; continuation lines start at
/// `rest_x` and wrap at `rest_width`. Used for general-mode (ExtendedSearch)
/// results, where line 1 trails the breadcrumb + prefix and the remaining
/// lines run full-width below them at the left margin.
struct WrapLayout {
    first_width: f32,
    rest_x: f32,
    rest_width: f32,
}

/// Render `text` at `(x, y)` with background highlight rectangles behind
/// characters at `match_positions`, matching scroll-search style. Text is
/// rendered in `text_color`; highlights use `highlight_color` as background.
/// Newlines in `text` start a new visual row, matching `prepare_text_wrapped`.
/// When `wrap` is `Some`, the text is also word-wrapped per the `WrapLayout`.
fn render_with_highlights(
    fr: &mut crate::text::FontRenderer,
    mut rr: Option<&mut crate::rectangle::RectangleRenderer>,
    text: &str,
    x: f32,
    y: f32,
    scale: f32,
    ascender: f32,
    line_height: f32,
    text_color: u32,
    highlight_color: u32,
    match_positions: &[u32],
    wrap: Option<WrapLayout>,
) {
    // Split `text` into segments: (byte_start, byte_end, char_start).
    // With `wrap` set the text is word-wrapped — the first line narrowed by a
    // preceding breadcrumb/prefix, continuation lines full-width (explicit `\n`
    // still break lines); otherwise it splits only on `\n`.
    let segs: Vec<(usize, usize, u32)> = if let Some(w) = &wrap {
        fr.wrap_lines_with_offsets_hanging(text, scale, w.first_width, w.rest_width)
            .into_iter()
            .map(|(line, byte_start)| {
                let byte_end = byte_start + line.len();
                let char_start = text[..byte_start].chars().count() as u32;
                (byte_start, byte_end, char_start)
            })
            .collect()
    } else {
        let mut segs: Vec<(usize, usize, u32)> = Vec::new();
        let mut seg_byte_start = 0usize;
        let mut seg_char_start = 0u32;
        let mut char_count = 0u32;
        for (bi, c) in text.char_indices() {
            if c == '\n' {
                segs.push((seg_byte_start, bi, seg_char_start));
                seg_byte_start = bi + 1;
                seg_char_start = char_count + 1;
            }
            char_count += 1;
        }
        segs.push((seg_byte_start, text.len(), seg_char_start));
        segs
    };

    for (n, &(byte_start, byte_end, char_start)) in segs.iter().enumerate() {
        let seg_text = &text[byte_start..byte_end];
        let seg_y = y + n as f32 * line_height;
        // Wrapped continuation lines drop back to the left margin.
        let seg_x = match &wrap {
            Some(w) if n > 0 => w.rest_x,
            _ => x,
        };

        if let Some(rr) = rr.as_deref_mut() {
            let seg_chars: Vec<char> = seg_text.chars().collect();
            let seg_char_len = seg_chars.len() as u32;
            let local_positions: Vec<u32> = match_positions.iter()
                .filter(|&&p| p >= char_start && p < char_start + seg_char_len)
                .map(|&p| p - char_start)
                .collect();
            if !local_positions.is_empty() {
                let mut i = 0usize;
                let mut byte_off = 0usize;
                let rect_y = seg_y - ascender * scale - crate::text::TEXT_PADDING;
                while i < seg_chars.len() {
                    if local_positions.binary_search(&(i as u32)).is_ok() {
                        let start_byte = byte_off;
                        while i < seg_chars.len() && local_positions.binary_search(&(i as u32)).is_ok() {
                            byte_off += seg_chars[i].len_utf8();
                            i += 1;
                        }
                        let match_x = seg_x + fr.measure_text_width(&seg_text[..start_byte], scale);
                        let match_w = fr.measure_text_width(&seg_text[start_byte..byte_off], scale);
                        rr.prepare_rectangle(match_x, rect_y, match_w, line_height, highlight_color, 3.0);
                    } else {
                        byte_off += seg_chars[i].len_utf8();
                        i += 1;
                    }
                }
            }
        }

        fr.prepare_text_for_rendering(seg_text, seg_x, seg_y, scale, text_color);
    }
}


// ---------------------------------------------------------------------------
// Key dispatch
// ---------------------------------------------------------------------------

fn handle_keydown(app: &mut AppState, keycode: Option<Keycode>, keymod: Mod) {
    if crate::events::dispatch_key(&mut app.renderer, keycode, keymod) {
        app.running = false;
    }
}

// ---------------------------------------------------------------------------
// Window title
// ---------------------------------------------------------------------------

fn update_window_title(app: &mut AppState) {
    let r = &app.renderer;
    let mode = r.mode_display_label();
    let path = build_display_path(r);

    let selected = r.current_list_item()
        .map(|item| item.label.as_str())
        .unwrap_or("");
    let selected_short: String = selected.chars().take(50).collect();

    let title = if selected_short.is_empty() {
        format!("sicompass [{mode}] {path}")
    } else {
        format!("sicompass [{mode}] {path}  »  {selected_short}")
    };

    let _ = app.window.set_title(&title);
    app.renderer.needs_redraw = false;
}

fn build_display_path(r: &crate::app_state::AppRenderer) -> String {
    let depth = r.current_id.depth();
    if depth == 0 {
        return "/".to_owned();
    }

    let mut parts = Vec::new();
    let mut current = r.ffon.as_slice();

    for d in 0..depth {
        let idx = r.current_id.get(d).unwrap_or(0);
        match current.get(idx) {
            Some(sicompass_sdk::ffon::FfonElement::Obj(obj)) => {
                let name: String = obj.key.chars().take(24).collect();
                parts.push(name);
                current = &obj.children;
            }
            Some(sicompass_sdk::ffon::FfonElement::Str(s)) => {
                let name: String = s.chars().take(24).collect();
                parts.push(name);
                break;
            }
            None => break,
        }
    }

    if parts.is_empty() { "/".to_owned() } else { parts.join(" / ") }
}

// ---------------------------------------------------------------------------
// Checkbox / radio indicator helpers (mirrors render.c:263-338)
// ---------------------------------------------------------------------------

#[derive(PartialEq)]
enum RadioType { None, Unchecked, Checked }

#[derive(PartialEq)]
enum CheckboxType { None, Unchecked, Checked }

fn get_radio_type(label: &str) -> RadioType {
    if label.starts_with("-rc ") { RadioType::Checked }
    else if label.starts_with("-r ") { RadioType::Unchecked }
    else { RadioType::None }
}

fn get_checkbox_type(label: &str) -> CheckboxType {
    if label.starts_with("-cc ") || label.starts_with("+cc ") { CheckboxType::Checked }
    else if label.starts_with("-c ") || label.starts_with("+c ") { CheckboxType::Unchecked }
    else { CheckboxType::None }
}

/// Returns the pixel width consumed by the indicator (circle/box + one em gap).
fn indicator_width(line_h: f32, em_width: f32) -> f32 {
    line_h * 0.8 + em_width
}

/// Draw a radio indicator. Returns the x offset to add before drawing text.
fn render_radio_indicator(
    rr: &mut crate::rectangle::RectangleRenderer,
    radio_type: &RadioType,
    x: f32, item_y: f32,
    scale: f32, ascender: f32, line_h: f32, em_width: f32,
    p: &crate::app_state::ColorPalette,
) -> f32 {
    let size = line_h * 0.8;
    let line_top = item_y - ascender * scale - crate::text::TEXT_PADDING;
    let indicator_y = line_top + (line_h - size) / 2.0;

    // Outer circle
    rr.prepare_rectangle(x, indicator_y, size, size, p.text, size / 2.0);
    // Inner circle
    let inner_size = size * 0.55;
    let inner_offset = (size - inner_size) / 2.0;
    let inner_color = if *radio_type == RadioType::Checked { p.selected } else { p.background };
    rr.prepare_rectangle(x + inner_offset, indicator_y + inner_offset, inner_size, inner_size, inner_color, inner_size / 2.0);

    size + em_width
}

/// Draw a checkbox indicator. Returns the x offset to add before drawing text.
fn render_checkbox_indicator(
    rr: &mut crate::rectangle::RectangleRenderer,
    checkbox_type: &CheckboxType,
    x: f32, item_y: f32,
    scale: f32, ascender: f32, line_h: f32, em_width: f32,
    p: &crate::app_state::ColorPalette,
) -> f32 {
    let size = line_h * 0.8;
    let line_top = item_y - ascender * scale - crate::text::TEXT_PADDING;
    let box_y = line_top + (line_h - size) / 2.0;

    if *checkbox_type == CheckboxType::Checked {
        rr.prepare_rectangle(x, box_y, size, size, p.selected, 0.0);
        let pad = size * 0.02;
        rr.prepare_checkmark(x + pad, box_y + pad, size - pad * 2.0, p.text);
    } else {
        rr.prepare_rectangle(x, box_y, size, size, p.text, 0.0);
        let border = size * 0.07;
        let inner = size - border * 2.0;
        rr.prepare_rectangle(x + border, box_y + border, inner, inner, p.background, 0.0);
    }

    size + em_width
}

/// Split a list label at the first space into (prefix_with_space, content).
/// E.g. `"-c My item"` -> `("-c ", "My item")`.
fn split_label(label: &str) -> (&str, &str) {
    if let Some(i) = label.find(' ') {
        (&label[..=i], &label[i + 1..])
    } else {
        (label, "")
    }
}

/// For image items, splits the label around the image path into (prefix, suffix,
/// has_meaningful_prefix). Prefix is everything before the path (including "-p "),
/// suffix is everything after. has_meaningful_prefix is true when there is text
/// beyond the bare "-p " marker (prefix.len() > 3).
fn split_image_label<'a>(label: &'a str, path: &str) -> (&'a str, &'a str, bool) {
    if let Some(pos) = label.find(path) {
        let prefix = &label[..pos];
        let suffix = &label[pos + path.len()..];
        let has_prefix = prefix.len() > 3;
        (prefix, suffix, has_prefix)
    } else {
        ("-p ", "", false)
    }
}

/// Counts display lines in text using 120-character column wrapping.
/// Matches C render.c countTextLines().
fn count_text_lines(text: &str) -> usize {
    if text.is_empty() { return 1; }
    let mut lines = 0usize;
    for seg in text.split('\n') {
        let len = seg.len();
        if len <= 120 { lines += 1; } else { lines += (len + 119) / 120; }
    }
    if text.ends_with('\n') { lines += 1; }
    lines.max(1)
}

/// Layout data for image items, pre-computed alongside item_metrics.
struct ImageLayout {
    prefix_lines: usize,
    suffix_lines: usize,
    image_lines: usize,
    img_w: f32,
    img_h: f32,
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_matches_ci_basic_and_case_insensitive() {
        assert_eq!(find_matches_ci("Hello hello", "hello"), vec![(0, 5), (6, 5)]);
        assert_eq!(find_matches_ci("abc", "xyz"), vec![]);
        assert_eq!(find_matches_ci("abc", ""), vec![]);
    }

    #[test]
    fn find_matches_ci_handles_multibyte_chars() {
        // Regression: matching must not slice inside a multi-byte char (`—`).
        let text = "open the view — a read-only list";
        let m = find_matches_ci(text, "read");
        let idx = text.find("read").unwrap();
        assert_eq!(m, vec![(idx, 4)]);
        // A query that scans past the em-dash must not panic.
        assert!(find_matches_ci(text, "zzz").is_empty());
    }

    #[test]
    fn rgba_u32_to_f32_black() {
        let [r, g, b, a] = rgba_u32_to_f32(0x000000FF);
        assert!((r - 0.0).abs() < 1e-6);
        assert!((g - 0.0).abs() < 1e-6);
        assert!((b - 0.0).abs() < 1e-6);
        assert!((a - 1.0).abs() < 1e-6);
    }

    #[test]
    fn rgba_u32_to_f32_white() {
        let [r, g, b, a] = rgba_u32_to_f32(0xFFFFFFFF);
        assert!((r - 1.0).abs() < 1e-6);
        assert!((g - 1.0).abs() < 1e-6);
        assert!((b - 1.0).abs() < 1e-6);
        assert!((a - 1.0).abs() < 1e-6);
    }

    #[test]
    fn rgba_u32_to_f32_red() {
        let [r, g, b, a] = rgba_u32_to_f32(0xFF0000FF);
        assert!((r - 1.0).abs() < 1e-6);
        assert!((g - 0.0).abs() < 1e-6);
        assert!((b - 0.0).abs() < 1e-6);
        assert!((a - 1.0).abs() < 1e-6);
    }

    #[test]
    fn split_label_with_space() {
        let (prefix, content) = split_label("-c My item");
        assert_eq!(prefix, "-c ");
        assert_eq!(content, "My item");
    }

    #[test]
    fn split_label_no_space() {
        let (prefix, content) = split_label("nospace");
        assert_eq!(prefix, "nospace");
        assert_eq!(content, "");
    }

    #[test]
    fn split_label_obj_prefix() {
        let (prefix, content) = split_label("+ Section name");
        assert_eq!(prefix, "+ ");
        assert_eq!(content, "Section name");
    }

    #[test]
    fn scroll_image_caption_splits_prefix_and_suffix() {
        let label = "-p Caption above /img/pic.png trailing words";
        let (prefix, suffix) = scroll_image_caption(label, "/img/pic.png");
        assert_eq!(prefix, "Caption above");
        assert_eq!(suffix, "trailing words");
    }

    #[test]
    fn scroll_image_caption_bare_marker_has_no_caption() {
        let label = "-p /img/pic.png";
        let (prefix, suffix) = scroll_image_caption(label, "/img/pic.png");
        assert_eq!(prefix, "");
        assert_eq!(suffix, "");
    }

    #[test]
    fn scroll_image_caption_prefix_only() {
        let label = "-p A photo of the harbour /img/pic.png";
        let (prefix, suffix) = scroll_image_caption(label, "/img/pic.png");
        assert_eq!(prefix, "A photo of the harbour");
        assert_eq!(suffix, "");
    }
}
