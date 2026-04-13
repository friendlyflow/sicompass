//! Main event loop — mirrors `mainLoop()` / `updateView()` in `view.c`.
//!
//! Routes SDL events to key handlers, updates the window title with
//! navigation state, and drives the Vulkan render loop.

use crate::app_state::{AppRenderer, AppState, Coordinate, History, Task};
use crate::handlers;
use crate::render;
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::{Keycode, Mod};
use tracing;

// Modes where the caret blinks and we need continuous redraw
fn is_insert_mode(c: Coordinate) -> bool {
    matches!(
        c,
        Coordinate::EditorInsert
            | Coordinate::EditorNormal
            | Coordinate::EditorVisual
            | Coordinate::OperatorInsert
            | Coordinate::SimpleSearch
            | Coordinate::ExtendedSearch
            | Coordinate::Command
            | Coordinate::ScrollSearch
            | Coordinate::InputSearch
    )
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the application until the user quits.
pub fn main_loop(app: &mut AppState) {
    update_window_title(app);

    while app.running {
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

                Event::Window { win_event, window_id, .. } => {
                    if window_id != app.window.id() {
                        continue;
                    }
                    match win_event {
                        WindowEvent::Resized(..)
                        | WindowEvent::PixelSizeChanged(..)
                        | WindowEvent::Exposed => {
                            app.framebuffer_resized = true;
                        }
                        WindowEvent::Maximized | WindowEvent::Restored => {
                            app.framebuffer_resized = true;
                            // Only update the setting once the initial pending_maximized
                            // has been applied; ignoring earlier events prevents writing
                            // a stale value during the startup window-creation sequence.
                            if app.maximized_ready {
                                let is_maximized = matches!(win_event, WindowEvent::Maximized);
                                if let Some(s) = app.renderer.providers.iter_mut().find(|p| p.name() == "settings") {
                                    s.on_checkbox_change("maximized", is_maximized);
                                }
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

        // ---- Rebuild font renderer when fontScale changes -------------------
        if app.renderer.rebuild_font_renderer {
            app.renderer.rebuild_font_renderer = false;
            unsafe {
                app.device.device_wait_idle().unwrap();
                if let Some(old_fr) = app.font_renderer.take() {
                    old_fr.destroy(&app.device);
                }
                let display_id = app.window.display_index().unwrap_or(1) as u32;
                let content_scale = app.window.display_content_scale(display_id);
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
        let any_tick_update = app.renderer.providers.iter_mut().any(|p| p.tick());
        if any_tick_update {
            // Clear any stale status, then let providers re-assert their error.
            app.renderer.error_message.clear();
            for p in app.renderer.providers.iter_mut() {
                if let Some(err) = p.take_error() {
                    app.renderer.error_message = err;
                }
            }
            crate::provider::refresh_current_directory(&mut app.renderer);
            // Rebuild the rendered list from the updated ffon tree — same as
            // what handlers.rs does after notify_button_pressed.
            crate::list::create_list_current_layer(&mut app.renderer);
            app.renderer.list_index = app.renderer.current_id.last().unwrap_or(0);
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

        // ---- Update accessibility tree (no-op when no AT is active) ---------
        if let Some(adapter) = app.accesskit_adapter.as_mut() {
            adapter.update_if_active(&app.renderer);
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
    radio_summary: Option<String>,
}

fn update_view(app: &mut AppState) {
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

    // Snapshot the display state so we can borrow font_renderer mutably after
    let header = build_header_text(&app.renderer, line_height);
    let win_w = app.swapchain_extent.width as f32;
    let win_h = app.swapchain_extent.height as f32;
    let list_items: Vec<(String, Option<String>, bool, Vec<u32>)> = collect_list_items(&app.renderer);
    let list_has_indicators = list_items.iter().any(|(label, _, _, _)| {
        get_radio_type(label) != RadioType::None || get_checkbox_type(label) != CheckboxType::None
    });

    // Command/meta mode items are plain strings without a type-indicator prefix,
    // so skip column alignment there (matches C render.c which renders all items
    // at a fixed itemX with no prefix-column offset).
    let is_flat_list = matches!(
        app.renderer.coordinate,
        Coordinate::Command | Coordinate::Meta
    );

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
                .map(|(label, _, _, _)| {
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

    // ---- Scroll / ScrollSearch early dispatch --------------------------------
    if matches!(app.renderer.coordinate, Coordinate::Scroll | Coordinate::ScrollSearch) {
        // Cache layout metrics for handlers
        app.renderer.window_height = win_h as i32;
        app.renderer.cached_line_height = line_height;

        // Snapshot state needed before mutable borrows
        let text_scroll_offset = app.renderer.text_scroll_offset;
        let list_index = app.renderer.list_index;
        let search_query = app.renderer.input_buffer.clone();
        let search_match_count = app.renderer.scroll_search_match_count;
        let search_current_match = app.renderer.scroll_search_current_match;
        let search_needs_position = app.renderer.scroll_search_needs_position;
        let search_snap = app.renderer.scroll_search_snap;
        let is_scroll_search = app.renderer.coordinate == Coordinate::ScrollSearch;
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

        // Header separator and text (same as list mode)
        if let Some(rr) = app.rect_renderer.as_mut() {
            rr.prepare_rectangle(0.0, line_height as f32, win_w, 1.0, p.header_sep, 0.0);
        }
        let header_baseline = (ascender * scale + crate::text::TEXT_PADDING) as f32;
        app.font_renderer.as_mut().unwrap().prepare_text_for_rendering(&header, text_x, header_baseline, scale, p.text);
        if !error_msg.is_empty() {
            let fr = app.font_renderer.as_mut().unwrap();
            let err_x = text_x + (header.len() as f32 * fr.get_width_em(scale)) + 10.0;
            fr.prepare_text_for_rendering(&error_msg, err_x, header_baseline, scale, p.error);
        }

        // Render scroll content — full list with pixel-smooth scrolling
        if is_scroll_search {
            let result = render_scroll_search_full(
                app.font_renderer.as_mut().unwrap(),
                app.rect_renderer.as_mut(),
                &list_items,
                list_index,
                text_scroll_offset,
                &search_query,
                search_match_count,
                search_current_match,
                search_needs_position,
                search_snap,
                scale, line_height, ascender, em_width, text_x, max_content_w, win_h, &p,
            );
            app.renderer.text_scroll_total_height = result.total_height;
            app.renderer.scroll_search_match_count = result.match_count;
            app.renderer.scroll_search_current_match = result.current_match;
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
                scale, line_height, ascender, em_width, text_x, max_content_w, win_h, &p,
            );
            app.renderer.text_scroll_total_height = total_height;
            app.renderer.text_scroll_offset = resolved_offset;
        }
        return;
    }

    // ---- Dashboard early dispatch --------------------------------------------
    if app.renderer.coordinate == Coordinate::Dashboard {
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
            rr.prepare_rectangle(0.0, line_height as f32, win_w, 1.0, p.header_sep, 0.0);
        }
        let header_baseline = (ascender * scale + crate::text::TEXT_PADDING) as f32;
        app.font_renderer.as_mut().unwrap().prepare_text_for_rendering(&header, text_x, header_baseline, scale, p.text);
        let error_msg = app.renderer.error_message.clone();
        if !error_msg.is_empty() {
            let fr = app.font_renderer.as_mut().unwrap();
            let err_x = text_x + (header.len() as f32 * fr.get_width_em(scale)) + 10.0;
            fr.prepare_text_for_rendering(&error_msg, err_x, header_baseline, scale, p.error);
        }

        // Render the dashboard image
        if !dashboard_path.is_empty() {
            if let Some(ir) = app.image_renderer.as_mut() {
                let avail_w = win_w - 100.0;
                let avail_h = win_h - line_height as f32 * 2.0;
                if let Some((img_w, img_h)) = unsafe { ir.texture_size(&dashboard_path) } {
                    let img_w = img_w as f32;
                    let img_h = img_h as f32;
                    let mut display_scale = 1.0_f32;
                    if img_w > avail_w { display_scale = avail_w / img_w; }
                    if img_h * display_scale > avail_h { display_scale = avail_h / img_h; }
                    let display_w = img_w * display_scale;
                    let display_h = img_h * display_scale;
                    let img_x = (win_w - display_w) / 2.0;
                    let img_y = line_height as f32 + (avail_h - display_h) / 2.0;
                    unsafe { ir.prepare_image(&dashboard_path, img_x, img_y, display_w, display_h); }
                }
            }
        }

        return;
    }

    // Snapshot insert-mode state before mutable borrows
    let in_insert_mode = matches!(
        app.renderer.coordinate,
        Coordinate::EditorInsert | Coordinate::EditorNormal | Coordinate::EditorVisual | Coordinate::OperatorInsert
    );
    let insert_buf = app.renderer.input_buffer.clone();
    let insert_prefix = app.renderer.input_prefix.clone();
    let insert_suffix = app.renderer.input_suffix.clone();
    let insert_cursor = app.renderer.cursor_position;
    let insert_sel = app.renderer.selection_anchor;
    let caret_visible = app.renderer.caret.visible;
    let search_str = if matches!(
        app.renderer.coordinate,
        Coordinate::SimpleSearch | Coordinate::ExtendedSearch | Coordinate::Command
    ) {
        let (prefix, text) = match app.renderer.coordinate {
            Coordinate::Command => ("search: ", app.renderer.input_buffer.as_str()),
            Coordinate::ExtendedSearch => ("ext search: ", app.renderer.input_buffer.as_str()),
            _ => ("search: ", app.renderer.search_string.as_str()),
        };
        Some(format!("{}{}", prefix, text))
    } else {
        None
    };
    let error_msg = app.renderer.error_message.clone();

    // ---- Parent element snapshot -------------------------------------------
    // Always present (empty at root level), so list items are consistently
    // indented one level below the parent line.
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
            let display_text = sicompass_sdk::tags::strip_display(raw_text);
            let radio_summary = if let sicompass_sdk::ffon::FfonElement::Obj(obj) = elem {
                if sicompass_sdk::tags::has_radio(&obj.key) {
                    obj.children.iter().find_map(|child| {
                        if let sicompass_sdk::ffon::FfonElement::Str(s) = child {
                            if sicompass_sdk::tags::has_checked(s) {
                                return sicompass_sdk::tags::extract_checked(s);
                            }
                        }
                        None
                    })
                } else {
                    None
                }
            } else {
                None
            };
            Some(ParentInfo { display_text, radio_summary })
        }).unwrap_or(ParentInfo { display_text: String::new(), radio_summary: None })
    } else {
        ParentInfo { display_text: String::new(), radio_summary: None }
    };

    // ---- Scroll-into-view: compute start_index from scroll_offset/list_index --
    // Pre-compute per-item line counts (needed before item_metrics so the scroll
    // algorithm can run first, matching the C render.c viewport logic).
    let extra_lines = if search_str.is_some() {
        1
    } else {
        1 + if parent_info.radio_summary.is_some() { 1 } else { 0 }
    };
    let first_item_y = (line_height as f32) * (1.0 + extra_lines as f32) + ascender * scale + crate::text::TEXT_PADDING;
    let available_lines = ((win_h - first_item_y) / line_height as f32).max(1.0) as usize;
    let item_max_w = max_content_w.max(1.0);

    let is_extended_search = app.renderer.coordinate == Coordinate::ExtendedSearch;
    let count = list_items.len();
    let list_index = if count > 0 { app.renderer.list_index.min(count - 1) } else { 0 };
    let line_counts: Vec<usize> = {
        let fr = match app.font_renderer.as_ref() { Some(f) => f, None => return };
        list_items.iter().enumerate().map(|(idx, (label, img_data, _, _))| {
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
            let bc_w = img_data.as_deref().filter(|s| !s.is_empty())
                .map(|bc| fr.measure_text_width(bc, scale)).unwrap_or(0.0);
            let (prefix_str, content) = split_label(label);
            let prefix_w = fr.measure_text_width(prefix_str, scale);
            let available_w = (max_content_w - 4.0 * em_width - indicator_w - bc_w - prefix_w).max(1.0);
            fr.count_wrapped_lines(content, scale, available_w)
        }).collect()
    };

    let start_index: usize = if count == 0 {
        0
    } else {
        let scroll_offset = app.renderer.scroll_offset;
        if scroll_offset < 0 {
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
        for (global_idx, (label, img_data, _, _)) in list_items.iter().enumerate().skip(start_index) {
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
                let bc_w = img_data.as_deref().filter(|s| !s.is_empty())
                    .map(|bc| fr.measure_text_width(bc, scale)).unwrap_or(0.0);
                let (prefix_str, content) = split_label(label);
                let prefix_w = fr.measure_text_width(prefix_str, scale);
                let available_w = (max_content_w - 4.0 * em_width - indicator_w - bc_w - prefix_w).max(1.0);
                (fr.count_wrapped_lines(content, scale, available_w), None)
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

    // ---- Header separator line -------------------------------------------
    if let Some(rr) = app.rect_renderer.as_mut() {
        rr.prepare_rectangle(0.0, line_height as f32, win_w, 1.0, p.header_sep, 0.0);
    }

    // ---- Header text -----------------------------------------------------
    let header_baseline = (ascender * scale + crate::text::TEXT_PADDING) as f32;
    fr.prepare_text_for_rendering(&header, text_x, header_baseline, scale, p.text);

    // ---- Error message (right of header) ---------------------------------
    if !error_msg.is_empty() {
        let err_x = text_x + (header.len() as f32 * fr.get_width_em(scale)) + 10.0;
        fr.prepare_text_for_rendering(&error_msg, err_x, header_baseline, scale, p.error);
    }

    // ---- Parent element (when navigated into a child level) ---------------
    if !parent_info.display_text.is_empty() && search_str.is_none() {
        let parent_y = line_height as f32 + ascender * scale + crate::text::TEXT_PADDING;
        fr.prepare_text_for_rendering(&parent_info.display_text, text_x, parent_y, scale, p.text);
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
        let search_y = line_height as f32 + ascender * scale + crate::text::TEXT_PADDING;
        fr.prepare_text_for_rendering(s, text_x, search_y, scale, p.text);
    }

    // ---- List items — selection highlight rectangles ----------------------
    for (i, (_, _, is_selected, _)) in list_items[start_index..].iter().take(item_metrics.len()).enumerate() {
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
    for (i, (label, item_data, is_selected, match_pos)) in list_items[start_index..].iter().take(item_metrics.len()).enumerate() {
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
            // item_data is a breadcrumb, not an image path.
            // Render: [breadcrumb in ext_search color][prefix][content], all within 120 em column.
            let right_edge = text_x + max_content_w;
            let (prefix_str, content) = split_label(label.as_str());
            if let Some(fr) = app.font_renderer.as_mut() {
                let mut label_x = text_prefix_x;
                if let Some(breadcrumb) = item_data.as_deref().filter(|s: &&str| !s.is_empty()) {
                    let bc_w = fr.measure_text_width(breadcrumb, scale);
                    fr.prepare_text_for_rendering(breadcrumb, label_x, item_y, scale, p.ext_search);
                    label_x += bc_w;
                }
                fr.prepare_text_for_rendering(prefix_str, label_x, item_y, scale, p.text);
                let content_x = label_x + fr.measure_text_width(prefix_str, scale);
                let available_w = (right_edge - content_x).max(1.0);
                // Adjust match positions to be relative to content (subtract prefix char count)
                let prefix_char_count = prefix_str.chars().count() as u32;
                let content_positions: Vec<u32> = match_pos.iter()
                    .filter(|&&p| p >= prefix_char_count)
                    .map(|&p| p - prefix_char_count)
                    .collect();
                let rr = app.rect_renderer.as_mut();
                render_with_highlights(fr, rr, content, content_x, item_y, scale, ascender, line_height as f32, p.text, p.scroll_search, &content_positions);
                let _ = available_w;
            }
        } else if let Some(ref path) = item_data {
            let (prefix_text, suffix_text, has_prefix) = split_image_label(label, path);
            let (prefix_lines, img_h_precomp) = image_layouts[i]
                .as_ref()
                .map(|l| (l.prefix_lines, l.img_h))
                .unwrap_or((0, 0.0));

            // Render prefix text above image (or bare "-p" when no meaningful prefix).
            // The "-p" list tag always renders at text_prefix_x; content text at content_start_x.
            let mut current_y = item_y;
            if has_prefix {
                let (tag, content) = split_label(prefix_text);
                if let Some(fr) = app.font_renderer.as_mut() {
                    fr.prepare_text_for_rendering(tag, text_prefix_x, current_y, scale, p.text);
                    if !content.is_empty() {
                        fr.prepare_text_for_rendering(content, content_start_x, current_y, scale, p.text);
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
                if let Some(fr) = app.font_renderer.as_mut() {
                    fr.prepare_text_for_rendering(suffix_text, content_start_x, current_y, scale, p.text);
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
                    render_with_highlights(fr, rr, label.as_str(), text_prefix_x, item_y, scale, ascender, line_height as f32, p.text, p.scroll_search, &match_pos);
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
                    render_with_highlights(fr, rr, content, content_start_x, item_y, scale, ascender, line_height as f32, p.text, p.scroll_search, &content_positions);
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
                (text_x + pfx_w, line_height as f32 + crate::text::TEXT_PADDING)
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
            // Insert/editor mode caret using stored element position
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
                | Coordinate::ScrollSearch | Coordinate::InputSearch
        ) {
            // Search/command caret after the prefix
            let (prefix, buf, cursor) = match app.renderer.coordinate {
                Coordinate::Command => ("search: ", insert_buf.as_str(), insert_cursor),
                Coordinate::ExtendedSearch => ("ext search: ", insert_buf.as_str(), insert_cursor),
                _ => ("search: ", app.renderer.search_string.as_str(), insert_cursor),
            };
            // search_y is the baseline — shift to cell top + padding
            let search_y = line_height as f32 + ascender * scale + crate::text::TEXT_PADDING;
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

/// Compute the pixel height of one list item in scroll mode.
/// Uses the full label (including prefix) for text measurement.
fn scroll_item_height(
    fr: &crate::text::FontRenderer,
    label: &str,
    scale: f32,
    line_height: i32,
    max_w: f32,
) -> i32 {
    let stripped = sicompass_sdk::tags::strip_display(label);
    let lines = fr.count_wrapped_lines(&stripped, scale, max_w).max(1);
    lines as i32 * line_height
}

/// Render the full list with pixel-smooth scrolling.
/// Returns `(total_height_px, resolved_scroll_offset_px)`.
#[allow(clippy::too_many_arguments)]
fn render_scroll_full(
    fr: &mut crate::text::FontRenderer,
    mut rr: Option<&mut crate::rectangle::RectangleRenderer>,
    _ir: Option<&mut crate::image::ImageRenderer>,
    list_items: &[(String, Option<String>, bool, Vec<u32>)],
    list_index: usize,
    text_scroll_offset: i32,
    scale: f32,
    line_height: i32,
    ascender: f32,
    em_width: f32,
    text_x: f32,
    max_content_w: f32,
    win_h: f32,
    p: &crate::app_state::ColorPalette,
) -> (i32, i32) {
    let _ = em_width;
    let clip_y = line_height as f32;
    let viewport_h = win_h - clip_y;
    let max_w = max_content_w.max(1.0);
    let lh = line_height as f32;

    // Phase 1: compute per-item heights and cumulative tops
    let mut item_tops: Vec<i32> = Vec::with_capacity(list_items.len());
    let mut y_accum: i32 = 0;
    for (label, _, _, _) in list_items.iter() {
        item_tops.push(y_accum);
        y_accum += scroll_item_height(fr, label, scale, line_height, max_w);
    }
    let total_height = y_accum;

    // Resolve sentinel (-1): place selected item at viewport top
    let max_offset = (total_height - viewport_h as i32).max(0);
    let scroll_offset = if text_scroll_offset < 0 {
        item_tops.get(list_index).copied().unwrap_or(0).min(max_offset)
    } else {
        text_scroll_offset.clamp(0, max_offset)
    };

    // Phase 2: selection highlight rectangle for the selected item
    if let Some(item_top) = item_tops.get(list_index) {
        let item_top_screen = clip_y + (item_top - scroll_offset) as f32;
        let item_h = (item_tops.get(list_index + 1).copied().unwrap_or(total_height) - item_top) as f32;
        let rect_top = item_top_screen.max(clip_y);
        let rect_bottom = (item_top_screen + item_h).min(win_h);
        if rect_bottom > rect_top {
            if let Some(rr) = rr.as_deref_mut() {
                rr.prepare_rectangle(text_x - 4.0, rect_top, max_w + 8.0, rect_bottom - rect_top, p.selected, 5.0);
            }
        }
    }

    // Phase 3: render text for all visible items (full label including prefix)
    for (i, (label, _, _, _)) in list_items.iter().enumerate() {
        let item_top_screen = clip_y + (item_tops[i] - scroll_offset) as f32;
        let item_h = (item_tops.get(i + 1).copied().unwrap_or(total_height) - item_tops[i]) as f32;
        if item_top_screen + item_h <= clip_y { continue; } // above viewport
        if item_top_screen >= win_h { break; }               // below viewport

        let stripped = sicompass_sdk::tags::strip_display(label);
        let wrapped = fr.wrap_lines_with_offsets(&stripped, scale, max_w);
        for (n, (line, _)) in wrapped.iter().enumerate() {
            let line_top = item_top_screen + n as f32 * lh;
            if line_top + lh <= clip_y { continue; }
            if line_top >= win_h { break; }
            let line_baseline = line_top + ascender * scale + crate::text::TEXT_PADDING;
            fr.prepare_text_for_rendering(line, text_x, line_baseline, scale, p.text);
        }
    }

    (total_height, scroll_offset)
}

struct ScrollSearchResult {
    total_height: i32,
    match_count: usize,
    current_match: usize,
    scroll_offset: i32,
}

/// Find all case-insensitive occurrences of `query` in `text`.
/// Returns `(byte_offset_in_text, match_len_in_lowercased_text)` pairs.
fn find_matches_ci(text: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    let text_lower = text.to_lowercase();
    let query_lower = query.to_lowercase();
    let q_len = query_lower.len();
    let mut matches = Vec::new();
    let mut pos = 0usize;
    while pos + q_len <= text_lower.len() {
        if text_lower[pos..].starts_with(&query_lower) {
            matches.push((pos, q_len));
            pos += 1;
        } else {
            pos += 1;
        }
    }
    matches
}

/// Render scroll-search mode across all list items. Returns computed state for write-back.
#[allow(clippy::too_many_arguments)]
fn render_scroll_search_full(
    fr: &mut crate::text::FontRenderer,
    mut rr: Option<&mut crate::rectangle::RectangleRenderer>,
    list_items: &[(String, Option<String>, bool, Vec<u32>)],
    list_index: usize,
    text_scroll_offset: i32,
    search_query: &str,
    _search_match_count: usize,
    search_current_match: usize,
    needs_position: bool,
    snap: bool,
    scale: f32,
    line_height: i32,
    ascender: f32,
    em_width: f32,
    text_x: f32,
    max_content_w: f32,
    win_h: f32,
    p: &crate::app_state::ColorPalette,
) -> ScrollSearchResult {
    let _ = em_width;
    // Search bar occupies line 1 (below header), content starts at line 2
    let clip_y = line_height as f32 * 2.0;
    let viewport_h = win_h - clip_y;
    let max_w = max_content_w.max(1.0);
    let lh = line_height as f32;

    // Phase 1: per-item heights, cumulative tops, texts, and pre-computed wrapped lines.
    // Wrapping once here avoids re-wrapping in later phases.
    let mut item_tops: Vec<i32> = Vec::with_capacity(list_items.len());
    let mut item_texts: Vec<String> = Vec::with_capacity(list_items.len());
    let mut item_wraps: Vec<Vec<(String, usize)>> = Vec::with_capacity(list_items.len());
    let mut y_accum: i32 = 0;
    for (label, _, _, _) in list_items.iter() {
        item_tops.push(y_accum);
        let stripped = sicompass_sdk::tags::strip_display(label);
        let wrap = fr.wrap_lines_with_offsets(&stripped, scale, max_w);
        y_accum += (wrap.len().max(1) as i32) * line_height;
        item_texts.push(stripped);
        item_wraps.push(wrap);
    }
    let total_height = y_accum;

    // Phase 2: collect all matches across all items.
    // Each entry: (item_idx, byte_off, match_len, match_virtual_y)
    // match_virtual_y is the virtual-space top of the wrapped line containing the match.
    let mut all_matches: Vec<(usize, usize, usize, i32)> = Vec::new();
    for (item_idx, text) in item_texts.iter().enumerate() {
        let wrap = &item_wraps[item_idx];
        for (byte_off, mlen) in find_matches_ci(text, search_query) {
            let li = wrap.partition_point(|(_, off)| *off <= byte_off).saturating_sub(1);
            let li = li.min(wrap.len().saturating_sub(1));
            let virtual_y = item_tops[item_idx] + li as i32 * line_height;
            all_matches.push((item_idx, byte_off, mlen, virtual_y));
        }
    }
    let match_count = all_matches.len();

    // Resolve the current viewport top from the input scroll offset.
    let viewport_top = if text_scroll_offset < 0 {
        item_tops.get(list_index).copied().unwrap_or(0)
    } else {
        text_scroll_offset
    };

    // Select current match.
    // needs_position (first entry with matches): pick the first match whose line
    //   is visible in the current viewport (match_virtual_y + line_height > viewport_top),
    //   falling back to match 0 if none qualify.
    // Otherwise: use clamped search_current_match directly (explicit navigation or typing).
    let current_match = if match_count == 0 {
        0
    } else {
        let clamped = search_current_match.min(match_count - 1);
        if needs_position {
            // Always find the first match whose line is in/after the viewport top.
            // Never short-circuit via clamped — it may not be the first visible match.
            all_matches.iter().enumerate()
                .find(|(_, &(_, _, _, vy))| vy + line_height > viewport_top)
                .map(|(mi, _)| mi)
                .unwrap_or(0)
        } else {
            clamped
        }
    };

    // Snap the viewport only on explicit Up/Down navigation (snap flag).
    // On entry (needs_position) and while typing, keep the viewport where it is.
    let max_offset = (total_height - viewport_h as i32).max(0);
    let scroll_offset = if snap && match_count > 0 {
        let match_item = all_matches[current_match].0;
        item_tops.get(match_item).copied().unwrap_or(0).clamp(0, max_offset)
    } else {
        viewport_top.clamp(0, max_offset)
    };

    // Render search bar at line 1 (immediately below header separator)
    let search_bar_y = line_height as f32 + ascender * scale + crate::text::TEXT_PADDING;
    let search_bar = format!("search: {} [{} items]", search_query, match_count);
    fr.prepare_text_for_rendering(&search_bar, text_x, search_bar_y, scale, p.text);

    // Phase 3: render visible items with match highlights
    for (i, (text, wrap_lines)) in item_texts.iter().zip(item_wraps.iter()).enumerate() {
        let item_top_screen = clip_y + (item_tops[i] - scroll_offset) as f32;
        let item_h = (item_tops.get(i + 1).copied().unwrap_or(total_height) - item_tops[i]) as f32;
        if item_top_screen + item_h <= clip_y { continue; }
        if item_top_screen >= win_h { break; }

        // Collect matches within this item mapped to their wrapped line index
        let mut matches_per_line: Vec<Vec<(usize, usize, bool)>> = vec![Vec::new(); wrap_lines.len()];
        for (mi, &(match_item, byte_off, mlen, _)) in all_matches.iter().enumerate() {
            if match_item != i { continue; }
            let li = wrap_lines.partition_point(|(_, off)| *off <= byte_off).saturating_sub(1);
            let li = li.min(wrap_lines.len().saturating_sub(1));
            let line_byte_off = wrap_lines[li].1;
            let local_start = byte_off.saturating_sub(line_byte_off);
            matches_per_line[li].push((local_start, mlen, mi == current_match));
        }

        for (n, (line_text, _)) in wrap_lines.iter().enumerate() {
            let line_top = item_top_screen + n as f32 * lh;
            if line_top + lh <= clip_y { continue; }
            if line_top >= win_h { break; }
            let line_baseline = line_top + ascender * scale + crate::text::TEXT_PADDING;

            if let Some(rr) = rr.as_deref_mut() {
                for &(local_start, mlen, is_current) in &matches_per_line[n] {
                    let safe_start = local_start.min(line_text.len());
                    let safe_end = (local_start + mlen).min(line_text.len());
                    let match_x = text_x + fr.measure_text_width(&line_text[..safe_start], scale);
                    let match_w = fr.measure_text_width(&line_text[safe_start..safe_end], scale).max(2.0);
                    let color = if is_current { p.scroll_search } else { p.selected };
                    rr.prepare_rectangle(match_x, line_top, match_w, lh, color, 3.0);
                }
            }

            fr.prepare_text_for_rendering(line_text, text_x, line_baseline, scale, p.text);
        }
        let _ = text;
    }

    ScrollSearchResult { total_height, match_count, current_match, scroll_offset }
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
    let mode = r.coordinate.as_str();
    let depth = r.current_id.depth().saturating_sub(1);
    let last_id = r.current_id.last().unwrap_or(0);
    let total = r.active_list_len();
    format!("{mode}, layer: {depth} list: {}/{total}", last_id + 1)
}

/// Snapshot the active list for rendering (avoids mixed borrows later).
/// Returns `(label, item_data, is_selected, fuzzy_match_positions)`.
fn collect_list_items(r: &AppRenderer) -> Vec<(String, Option<String>, bool, Vec<u32>)> {
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
            out.push((item.label.clone(), item.data.clone(), i == r.list_index, positions));
        }
    }
    out
}

/// Render `text` at `(x, y)` with background highlight rectangles behind
/// characters at `match_positions`, matching scroll-search style. Text is
/// rendered in `text_color`; highlights use `highlight_color` as background.
fn render_with_highlights(
    fr: &mut crate::text::FontRenderer,
    rr: Option<&mut crate::rectangle::RectangleRenderer>,
    text: &str,
    x: f32,
    y: f32,
    scale: f32,
    ascender: f32,
    line_height: f32,
    text_color: u32,
    highlight_color: u32,
    match_positions: &[u32],
) {
    if let Some(rr) = rr {
        // Draw background rectangles behind matched character runs
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0usize;
        let mut byte_off = 0usize;
        while i < chars.len() {
            if match_positions.binary_search(&(i as u32)).is_ok() {
                let start_byte = byte_off;
                while i < chars.len() && match_positions.binary_search(&(i as u32)).is_ok() {
                    byte_off += chars[i].len_utf8();
                    i += 1;
                }
                let match_x = x + fr.measure_text_width(&text[..start_byte], scale);
                let match_w = fr.measure_text_width(&text[start_byte..byte_off], scale);
                let rect_y = y - ascender * scale - crate::text::TEXT_PADDING;
                rr.prepare_rectangle(match_x, rect_y, match_w, line_height, highlight_color, 3.0);
            } else {
                byte_off += chars[i].len_utf8();
                i += 1;
            }
        }
    }
    fr.prepare_text_for_rendering(text, x, y, scale, text_color);
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
    let mode = r.coordinate.as_str();
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
}
