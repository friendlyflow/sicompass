//! Main event loop — mirrors `mainLoop()` / `updateView()` in `view.c`.
//!
//! Routes SDL events to key handlers, updates the window title with
//! navigation state, and drives the Vulkan render loop.

use crate::app_state::{AppRenderer, AppState, Coordinate, History, Task};
use crate::handlers;
use crate::render;
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::{Keycode, Mod};

// Modes where the caret blinks and we need continuous redraw
fn is_insert_mode(c: Coordinate) -> bool {
    matches!(
        c,
        Coordinate::EditorInsert
            | Coordinate::EditorNormal
            | Coordinate::EditorVisual
            | Coordinate::OperatorInsert
            | Coordinate::SimpleSearch
            | Coordinate::Command
    )
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the application until the user quits.
pub fn main_loop(app: &mut AppState) {
    update_window_title(app);

    while app.running {
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
                    handle_keydown(app, keycode, keymod);
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
                        }
                        WindowEvent::CloseRequested => {
                            app.running = false;
                        }
                        _ => {}
                    }
                }

                Event::TextInput { text, window_id, .. } => {
                    if window_id != app.window.id() {
                        continue;
                    }
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

        // ---- Recreate swapchain if needed -----------------------------------
        if app.framebuffer_resized {
            app.framebuffer_resized = false;
            render::recreate_swapchain(app);
        }

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

// Dark palette colours (ARGB-packed as RRGGBBAA)
const COLOR_BG: u32      = 0x000000FF; // black background (same as clear)
const COLOR_TEXT: u32    = 0xFFFFFFFF; // white text
const COLOR_SELECTED: u32 = 0x2D4A28FF; // dark green selection
const COLOR_SEPARATOR: u32 = 0x333333FF; // dark gray separator
const COLOR_ERROR: u32   = 0xFF0000FF; // red

fn update_view(app: &mut AppState) {
    // ---- Collect rendering data (before borrowing font/rect renderers) ----
    let scale;
    let line_height;
    let ascender;

    {
        let fr = match app.font_renderer.as_ref() {
            Some(f) => f,
            None => return,
        };
        scale = fr.get_text_scale(crate::text::FONT_SIZE_PT);
        line_height = fr.get_line_height(scale, crate::text::TEXT_PADDING) as i32;
        ascender = fr.ascender;
    }

    // Snapshot the display state so we can borrow font_renderer mutably after
    let header = build_header_text(&app.renderer, line_height);
    let win_w = app.swapchain_extent.width as f32;
    let win_h = app.swapchain_extent.height as f32;
    let list_items: Vec<(String, bool)> = collect_list_items(&app.renderer);
    // In insert mode the selected item shows prefix + buffer (with cursor marker)
    let insert_display: Option<String> = build_insert_display(&app.renderer);
    let search_str = if matches!(
        app.renderer.coordinate,
        Coordinate::SimpleSearch | Coordinate::Command
    ) {
        let prefix = match app.renderer.coordinate {
            Coordinate::Command => ":",
            _ => "search: ",
        };
        Some(format!("{}{}", prefix, app.renderer.search_string))
    } else {
        None
    };
    let error_msg = app.renderer.error_message.clone();

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
        rr.prepare_rectangle(0.0, line_height as f32, win_w, 1.0, COLOR_SEPARATOR, 0.0);
    }

    // ---- Header text -----------------------------------------------------
    let header_baseline = (ascender * scale + crate::text::TEXT_PADDING) as f32;
    fr.prepare_text_for_rendering(&header, 10.0, header_baseline, scale, COLOR_TEXT);

    // ---- Error message (right of header) ---------------------------------
    if !error_msg.is_empty() {
        let err_x = (header.len() as f32 * fr.get_width_em(scale)) + 20.0;
        fr.prepare_text_for_rendering(&error_msg, err_x, header_baseline, scale, COLOR_ERROR);
    }

    // ---- Search / command line -------------------------------------------
    if let Some(ref s) = search_str {
        let search_y = line_height as f32 * 2.0 - crate::text::TEXT_PADDING;
        fr.prepare_text_for_rendering(s, 10.0, search_y, scale, COLOR_TEXT);
    }

    // ---- List items -------------------------------------------------------
    let first_item_y = (line_height as f32) * 2.0 + ascender * scale;
    let visible_items = ((win_h - first_item_y) / line_height as f32).ceil() as usize + 1;

    for (i, (label, is_selected)) in list_items.iter().enumerate() {
        if i >= visible_items { break; }
        let item_y = first_item_y + i as f32 * line_height as f32;

        // Selection highlight rectangle
        if *is_selected {
            let rect_y = item_y - ascender * scale - crate::text::TEXT_PADDING;
            if let Some(rr) = app.rect_renderer.as_mut() {
                rr.prepare_rectangle(0.0, rect_y, win_w, line_height as f32, COLOR_SELECTED, 0.0);
            }
        }

        // Item text (re-borrow fr after possible rect_renderer borrow)
        if let Some(fr) = app.font_renderer.as_mut() {
            let display = if *is_selected {
                insert_display.as_deref().unwrap_or(label.as_str())
            } else {
                label.as_str()
            };
            fr.prepare_text_for_rendering(display, 10.0, item_y, scale, COLOR_TEXT);
        }
    }
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
fn collect_list_items(r: &AppRenderer) -> Vec<(String, bool)> {
    let len = r.active_list_len();
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let item = if r.filtered_list_indices.is_empty() {
            r.total_list.get(i)
        } else {
            r.filtered_list_indices.get(i).and_then(|&raw| r.total_list.get(raw))
        };
        if let Some(item) = item {
            out.push((item.label.clone(), i == r.list_index));
        }
    }
    out
}

/// In insert modes, build the display text for the selected item:
/// `input_prefix` + text-before-cursor + `|` + text-after-cursor + `input_suffix`.
/// Returns `None` when not in an insert mode.
fn build_insert_display(r: &AppRenderer) -> Option<String> {
    if !matches!(
        r.coordinate,
        Coordinate::EditorInsert
            | Coordinate::EditorNormal
            | Coordinate::EditorVisual
            | Coordinate::OperatorInsert
    ) {
        return None;
    }

    let buf = &r.input_buffer;
    let pos = r.cursor_position.min(buf.len());
    let before = &buf[..pos];
    let after = &buf[pos..];

    Some(format!("{}{}|{}{}", r.input_prefix, before, after, r.input_suffix))
}

// ---------------------------------------------------------------------------
// Key dispatch
// ---------------------------------------------------------------------------

fn handle_keydown(app: &mut AppState, keycode: Option<Keycode>, keymod: Mod) {
    let r = &mut app.renderer;
    let ctrl  = keymod.intersects(Mod::LCTRLMOD  | Mod::RCTRLMOD);
    let shift = keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);

    match r.coordinate {
        // ---- Operator general -----------------------------------------------
        Coordinate::OperatorGeneral => match keycode {
            Some(Keycode::Up) | Some(Keycode::K) if !ctrl && !shift => handlers::handle_up(r),
            Some(Keycode::Down) | Some(Keycode::J) if !ctrl && !shift => handlers::handle_down(r),
            Some(Keycode::Right) | Some(Keycode::L) if !ctrl && !shift => handlers::handle_right(r),
            Some(Keycode::Left) | Some(Keycode::H) if !ctrl && !shift => handlers::handle_left(r),
            Some(Keycode::PageUp) if !ctrl && !shift => handlers::handle_page_up(r),
            Some(Keycode::PageDown) if !ctrl && !shift => handlers::handle_page_down(r),
            Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
            Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
            Some(Keycode::Tab) => handlers::handle_tab(r),
            Some(Keycode::Semicolon) if shift => handlers::handle_colon(r),
            Some(Keycode::Return) | Some(Keycode::KpEnter) if !ctrl => {
                handlers::handle_enter_operator(r);
            }
            Some(Keycode::I) if !ctrl && !shift => handlers::handle_i(r),
            Some(Keycode::A) if !ctrl && !shift => handlers::handle_a(r),
            Some(Keycode::A) if ctrl && !shift => handlers::handle_ctrl_a_operator(r),
            Some(Keycode::I) if ctrl && !shift => handlers::handle_ctrl_i_operator(r),
            Some(Keycode::D) if ctrl && !shift => handlers::handle_delete(r, History::None),
            Some(Keycode::Delete) if !ctrl && !shift => handlers::handle_delete(r, History::None),
            Some(Keycode::M) if !ctrl && !shift => handlers::handle_meta(r),
            Some(Keycode::Space) if !ctrl && !shift => handlers::handle_space(r),
            Some(Keycode::Z) if ctrl && !shift => handlers::handle_undo(r),
            Some(Keycode::Z) if ctrl && shift => handlers::handle_redo(r),
            Some(Keycode::X) if ctrl && !shift => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift => handlers::handle_ctrl_v(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
            Some(Keycode::F5) => handlers::handle_f5(r),
            Some(Keycode::Backspace) => handlers::handle_backspace(r),
            Some(Keycode::Escape) | Some(Keycode::Q) => {
                app.running = false;
                return;
            }
            _ => {}
        },

        // ---- Editor general -------------------------------------------------
        Coordinate::EditorGeneral => match keycode {
            Some(Keycode::Up) | Some(Keycode::K) if !ctrl && !shift => handlers::handle_up(r),
            Some(Keycode::Down) | Some(Keycode::J) if !ctrl && !shift => handlers::handle_down(r),
            Some(Keycode::Right) | Some(Keycode::L) if !ctrl && !shift => handlers::handle_right(r),
            Some(Keycode::Left) | Some(Keycode::H) if !ctrl && !shift => handlers::handle_left(r),
            Some(Keycode::PageUp) if !ctrl && !shift => handlers::handle_page_up(r),
            Some(Keycode::PageDown) if !ctrl && !shift => handlers::handle_page_down(r),
            Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
            Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
            Some(Keycode::Tab) => handlers::handle_tab(r),
            Some(Keycode::Semicolon) if shift => handlers::handle_colon(r),
            Some(Keycode::Return) | Some(Keycode::KpEnter) if !ctrl => handlers::handle_append(r),
            Some(Keycode::I) if !ctrl && !shift => handlers::handle_i(r),
            Some(Keycode::A) if !ctrl && !shift => handlers::handle_a(r),
            Some(Keycode::A) if ctrl && !shift => handlers::handle_append(r),
            Some(Keycode::I) if ctrl && !shift => handlers::handle_insert(r),
            Some(Keycode::D) if ctrl && !shift => handlers::handle_delete(r, History::None),
            Some(Keycode::Space) if !ctrl && !shift => handlers::handle_space(r),
            Some(Keycode::Z) if ctrl && !shift => handlers::handle_undo(r),
            Some(Keycode::Z) if ctrl && shift => handlers::handle_redo(r),
            Some(Keycode::X) if ctrl && !shift => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift => handlers::handle_ctrl_v(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
            Some(Keycode::F5) => handlers::handle_f5(r),
            Some(Keycode::Backspace) => handlers::handle_backspace(r),
            Some(Keycode::Escape) => handlers::handle_escape(r),
            _ => {}
        },

        // ---- Simple search --------------------------------------------------
        Coordinate::SimpleSearch => match keycode {
            Some(Keycode::Up) | Some(Keycode::K) if !ctrl && !shift => handlers::handle_up(r),
            Some(Keycode::Down) | Some(Keycode::J) if !ctrl && !shift => handlers::handle_down(r),
            Some(Keycode::PageUp) if !ctrl && !shift => handlers::handle_page_up(r),
            Some(Keycode::PageDown) if !ctrl && !shift => handlers::handle_page_down(r),
            Some(Keycode::Home) if ctrl => handlers::handle_ctrl_home(r),
            Some(Keycode::End) if ctrl => handlers::handle_ctrl_end(r),
            Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
            Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
            Some(Keycode::Home) if shift => handlers::handle_shift_home(r),
            Some(Keycode::End) if shift => handlers::handle_shift_end(r),
            Some(Keycode::Left) if shift => handlers::handle_shift_left(r),
            Some(Keycode::Right) if shift => handlers::handle_shift_right(r),
            Some(Keycode::Left) if !ctrl && !shift => {
                if r.cursor_position > 0 {
                    let before = &r.search_string[..r.cursor_position.min(r.search_string.len())];
                    r.cursor_position = before.char_indices().rev().next().map(|(i,_)| i).unwrap_or(0);
                    r.needs_redraw = true;
                }
            }
            Some(Keycode::Right) if !ctrl && !shift => {
                let pos = r.cursor_position;
                let slen = r.search_string.len();
                if pos < slen {
                    let ch = r.search_string[pos..].chars().next().unwrap();
                    r.cursor_position = pos + ch.len_utf8();
                    r.needs_redraw = true;
                }
            }
            Some(Keycode::Tab) => handlers::handle_tab(r),
            Some(Keycode::Return) | Some(Keycode::KpEnter) => handlers::handle_enter_search(r),
            Some(Keycode::A) if ctrl && !shift => handlers::handle_select_all(r),
            Some(Keycode::X) if ctrl && !shift => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift => handlers::handle_ctrl_v(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
            Some(Keycode::Backspace) => handlers::handle_backspace(r),
            Some(Keycode::Delete) if !ctrl && !shift => handlers::handle_delete_forward(r),
            Some(Keycode::Escape) => handlers::handle_escape(r),
            _ => {}
        },

        // ---- Insert / normal / visual modes ---------------------------------
        Coordinate::EditorInsert | Coordinate::EditorNormal
        | Coordinate::EditorVisual | Coordinate::OperatorInsert => match keycode {
            Some(Keycode::Escape) => handlers::handle_escape(r),
            Some(Keycode::Backspace) => handlers::handle_backspace(r),
            Some(Keycode::Delete) if !ctrl && !shift => handlers::handle_delete_forward(r),
            Some(Keycode::Up) if !ctrl && !shift => handlers::handle_up_insert(r),
            Some(Keycode::Down) if !ctrl && !shift => handlers::handle_down_insert(r),
            Some(Keycode::Up) if shift => handlers::handle_shift_up_insert(r),
            Some(Keycode::Down) if shift => handlers::handle_shift_down_insert(r),
            Some(Keycode::Left) if shift => handlers::handle_shift_left(r),
            Some(Keycode::Right) if shift => handlers::handle_shift_right(r),
            Some(Keycode::Home) if shift => handlers::handle_shift_home(r),
            Some(Keycode::End) if shift => handlers::handle_shift_end(r),
            Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
            Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
            Some(Keycode::Left) if !ctrl && !shift => {
                if r.cursor_position > 0 {
                    let before = &r.input_buffer[..r.cursor_position];
                    r.cursor_position = before.char_indices().rev()
                        .next().map(|(i, _)| i).unwrap_or(0);
                    r.needs_redraw = true;
                }
            }
            Some(Keycode::Right) if !ctrl && !shift => {
                let pos = r.cursor_position;
                if pos < r.input_buffer.len() {
                    let ch = r.input_buffer[pos..].chars().next().unwrap();
                    r.cursor_position = pos + ch.len_utf8();
                    r.needs_redraw = true;
                }
            }
            Some(Keycode::Return) | Some(Keycode::KpEnter)
                if matches!(r.coordinate, Coordinate::EditorInsert | Coordinate::EditorNormal) =>
            {
                crate::state::update_state(r, Task::Input, History::None);
                handlers::handle_escape(r);
            }
            Some(Keycode::Return) | Some(Keycode::KpEnter)
                if r.coordinate == Coordinate::OperatorInsert =>
            {
                handlers::handle_enter_operator_insert(r);
            }
            Some(Keycode::A) if ctrl && !shift => handlers::handle_select_all(r),
            Some(Keycode::X) if ctrl && !shift => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift => handlers::handle_ctrl_v(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
            _ => {}
        },

        // ---- Command mode ---------------------------------------------------
        Coordinate::Command => match keycode {
            Some(Keycode::Escape) => handlers::handle_escape(r),
            Some(Keycode::Backspace) => handlers::handle_backspace(r),
            Some(Keycode::Delete) if !ctrl && !shift => handlers::handle_delete_forward(r),
            Some(Keycode::Home) if ctrl => handlers::handle_ctrl_home(r),
            Some(Keycode::End) if ctrl => handlers::handle_ctrl_end(r),
            Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
            Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
            Some(Keycode::Home) if shift => handlers::handle_shift_home(r),
            Some(Keycode::End) if shift => handlers::handle_shift_end(r),
            Some(Keycode::Left) if shift => handlers::handle_shift_left(r),
            Some(Keycode::Right) if shift => handlers::handle_shift_right(r),
            Some(Keycode::Left) if !ctrl && !shift => {
                if r.cursor_position > 0 {
                    let before = &r.input_buffer[..r.cursor_position];
                    r.cursor_position = before.char_indices().rev()
                        .next().map(|(i, _)| i).unwrap_or(0);
                    r.needs_redraw = true;
                }
            }
            Some(Keycode::Right) if !ctrl && !shift => {
                let pos = r.cursor_position;
                if pos < r.input_buffer.len() {
                    let ch = r.input_buffer[pos..].chars().next().unwrap();
                    r.cursor_position = pos + ch.len_utf8();
                    r.needs_redraw = true;
                }
            }
            Some(Keycode::Return) | Some(Keycode::KpEnter) => {
                // TODO Phase 4+: execute command
                handlers::handle_escape(r);
            }
            Some(Keycode::A) if ctrl && !shift => handlers::handle_select_all(r),
            Some(Keycode::X) if ctrl && !shift => handlers::handle_ctrl_x(r),
            Some(Keycode::C) if ctrl && !shift => handlers::handle_ctrl_c(r),
            Some(Keycode::V) if ctrl && !shift => handlers::handle_ctrl_v(r),
            _ => {}
        },

        // ---- Scroll / scroll-search modes -----------------------------------
        Coordinate::Scroll | Coordinate::ScrollSearch | Coordinate::InputSearch => match keycode {
            Some(Keycode::Escape) | Some(Keycode::Tab) => handlers::handle_escape(r),
            Some(Keycode::Up) | Some(Keycode::K) if !ctrl && !shift => {
                r.text_scroll_offset = (r.text_scroll_offset - 1).max(0);
                r.needs_redraw = true;
            }
            Some(Keycode::Down) | Some(Keycode::J) if !ctrl && !shift => {
                r.text_scroll_offset += 1;
                r.needs_redraw = true;
            }
            Some(Keycode::Home) if !ctrl && !shift => handlers::handle_home(r),
            Some(Keycode::End) if !ctrl && !shift => handlers::handle_end(r),
            Some(Keycode::F) if ctrl && !shift => handlers::handle_ctrl_f(r),
            Some(Keycode::Backspace) if matches!(r.coordinate,
                Coordinate::ScrollSearch | Coordinate::InputSearch) =>
            {
                handlers::handle_backspace(r);
            }
            Some(Keycode::Delete) if matches!(r.coordinate,
                Coordinate::ScrollSearch | Coordinate::InputSearch) =>
            {
                handlers::handle_delete_forward(r);
            }
            _ => {}
        },

        _ => {}
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
