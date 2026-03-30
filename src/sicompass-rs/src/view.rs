//! Main event loop and mode-based render dispatch.
//!
//! Mirrors `mainLoop()` in `view.c` and the SDL event loop in `view.c`.
//! Phase 3: SDL events → quit/resize handling + clear-colour frame draw.
//! Phase 4: provider init, key handlers, updateView dispatch.

use crate::app_state::{AppState, Coordinate};
use crate::render;
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::Keycode;

/// Run the application until the user quits.
/// Mirrors the `while (app->running)` loop inside `mainLoop()`.
pub fn main_loop(app: &mut AppState) {
    while app.running {
        // ---- Process all pending SDL events ---------------------------------
        for event in app.event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    app.running = false;
                }

                Event::KeyDown { keycode, window_id, .. } => {
                    if window_id != app.window.id() {
                        continue;
                    }
                    match keycode {
                        Some(Keycode::Escape) | Some(Keycode::Q) => {
                            // TODO Phase 4: only quit when in OPERATOR_GENERAL
                            // For now, any Escape/q exits
                            app.running = false;
                        }
                        _ => {
                            // TODO Phase 4: handleKeys(app, &event)
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
                        }
                        WindowEvent::Maximized | WindowEvent::Restored => {
                            app.framebuffer_resized = true;
                        }
                        WindowEvent::FocusGained => {
                            // TODO Phase 5: accesskitUpdateWindowFocus(app, true)
                        }
                        WindowEvent::FocusLost => {
                            // TODO Phase 5: accesskitUpdateWindowFocus(app, false)
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
                    // TODO Phase 4: handleInput(app, text)
                    let _ = text;
                }

                _ => {}
            }
        }

        if !app.running {
            break;
        }

        // ---- Recreate swapchain if needed -----------------------------------
        if app.framebuffer_resized {
            app.framebuffer_resized = false;
            render::recreate_swapchain(app);
        }

        // ---- Draw frame -----------------------------------------------------
        render::draw_frame(app);

        // ~60 FPS cap (mirrors SDL_Delay(16) in C)
        std::thread::sleep(std::time::Duration::from_millis(16));
    }

    // Wait for GPU to finish before cleanup (called via AppState::drop)
    unsafe {
        let _ = app.device.device_wait_idle();
    }
}
