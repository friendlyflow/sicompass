#![allow(dead_code, unused_imports)]
//! sicompass — Rust port of the Vulkan/SDL3 modal UI application.
//!
//! Module layout mirrors the C source in `src/sicompass/`:
//!
//! | Rust module     | C source         | Phase |
//! |-----------------|------------------|-------|
//! | `app_state`     | app_state.h      | 3     |
//! | `render`        | main.c / render.c| 3     |
//! | `view`          | view.c           | 3     |
//! | `unicode_search`| unicode_search.c | 3 ✓   |
//! | `caret`         | caret.c          | 3 ✓   |
//! | `checkmark`     | checkmark.c      | 3 ✓   |
//! | `text`          | text.c           | 4     |
//! | `rectangle`     | rectangle.c      | 4     |
//! | `image`         | image.c          | 4     |
//! | `events`        | events.c         | 4     |
//! | `handlers`      | handlers.c       | 4     |
//! | `list`          | list.c           | 4     |
//! | `provider`      | provider.c       | 4     |
//! | `programs`      | programs.c       | 4     |
//! | `state`         | state.c          | 4     |
//! | `accesskit_sdl` | accesskit_sdl.c  | 5     |

mod app_state;
mod render;
mod view;

mod accesskit_sdl;
mod caret;
mod checkmark;
mod events;
mod handlers;
mod image;
mod list;
mod programs;
mod provider;
mod rectangle;
mod state;
mod text;
mod unicode_search;

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
