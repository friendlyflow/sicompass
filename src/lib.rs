//! sicompass — the application.
//!
//! The renderer itself lives in the `sicompass-ui` crate; what is left here is
//! everything an *application* has and a login screen does not: the provider
//! catalogue and the settings file that selects from it (`programs`), the
//! plugin host (`plugin_host`, `plugin_manifest`), the self-updater, and the
//! Windows Start Menu entry (`start_menu`).
//!
//! That split is what keeps `loginsicompass` from linking the plugin host, the
//! updater's HTTP client and the built-in providers. See sicompass-ui's
//! `Cargo.toml`.
//!
//! The renderer calls back into this crate through
//! [`sicompass_ui::registry::HostHooks`], implemented by [`boot::ProgramsHooks`].

#![allow(dead_code, unused_imports)]

pub mod boot;
/// A debug build runs the plugins of the checkouts beside it.
pub mod dev_plugins;
/// Host for plugin processes: a plugin is a program the app starts and talks
/// to over its stdin and stdout.
pub mod plugin_host;
pub mod plugin_manifest;
pub mod programs;
pub mod start_menu;

// The renderer, re-exported so `tests/integration.rs` and the binary can reach
// it by the same paths they used before the split.
pub use sicompass_ui::{
    accesskit_sdl, app_state, caret, checkmark, events, fonts, handlers, http, icon, image, list,
    provider, rectangle, registry, render, session_mode, shaders, shortcuts, state, text,
    unicode_search, view,
};
