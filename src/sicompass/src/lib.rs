//! sicompass library interface — re-exports modules for integration tests.
//!
//! The binary (`main.rs`) declares all modules.  This library crate re-exports
//! them so that `tests/integration.rs` can access `sicompass::events`,
//! `sicompass::app_state`, etc. without duplicating module declarations.

#![allow(dead_code, unused_imports)]

pub mod app_state;
pub mod render;
pub mod view;

pub mod accesskit_sdl;
pub mod caret;
pub mod checkmark;
pub mod events;
pub mod fonts;
pub mod handlers;
pub mod icon;
pub mod image;
pub mod list;
pub mod plugin_manifest;
pub mod programs;
pub mod provider;
pub mod rectangle;
pub mod shaders;
pub mod shortcuts;
pub mod start_menu;
pub mod state;
pub mod text;
pub mod unicode_search;
/// Host for sandboxed WASM plugins — the replacement for `dlopen`ed native plugins
/// and `bun`-spawned script plugins, neither of which can ship on Apple's stores.
pub mod wasm_host;
