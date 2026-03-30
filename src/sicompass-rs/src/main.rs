//! sicompass — Rust port of the Vulkan/SDL3 modal UI application.
//!
//! Module layout mirrors the C source in `src/sicompass/`:
//!
//! | Rust module     | C source         | Phase | Status |
//! |-----------------|------------------|-------|--------|
//! | `app_state`     | app_state.h      | 3     | ✓      |
//! | `render`        | main.c / render.c| 3     | ✓      |
//! | `view`          | view.c           | 3     | ✓      |
//! | `unicode_search`| unicode_search.c | 3     | ✓      |
//! | `caret`         | caret.c          | 3     | ✓      |
//! | `checkmark`     | checkmark.c      | 3     | ✓      |
//! | `text`          | text.c           | 4     | ✓      |
//! | `rectangle`     | rectangle.c      | 4     | ✓      |
//! | `handlers`      | handlers.c       | 4     | ✓      |
//! | `list`          | list.c           | 4     | ✓      |
//! | `provider`      | provider.c       | 4     | ✓      |
//! | `programs`      | programs.c       | 4     | ✓      |
//! | `state`         | state.c          | 4     | ✓      |
//! | `events`        | events.c         | 4     | ✓      |
//! | `image`         | image.c          | 5     | ✓      |
//! | `accesskit_sdl` | accesskit_sdl.c  | 5     | ✓      |
//!
//! ## Library providers (Phase 5)
//!
//! | Crate                    | C library            | Status |
//! |--------------------------|----------------------|--------|
//! | `sicompass-settings`     | lib_settings         | ✓      |
//! | `sicompass-filebrowser`  | lib_filebrowser      | ✓      |
//! | `sicompass-tutorial`     | lib_tutorial (TS)    | ✓      |
//! | `sicompass-webbrowser`   | lib_webbrowser       | ✓      |
//! | `sicompass-chatclient`   | lib_chatclient       | ✓      |
//! | `sicompass-emailclient`  | lib_emailclient      | ✓      |

// All modules are declared in lib.rs; the binary just re-uses them.
use sicompass::app_state;
use sicompass::render;
use sicompass::view;
use std::process;

fn main() {
    if std::env::args().any(|a| a == "--check") {
        process::exit(render::check_runtime_files());
    }

    match app_state::AppState::new() {
        Ok(mut app) => app.run(),
        Err(e) => {
            eprintln!("sicompass: {e}");
            process::exit(1);
        }
    }
}
