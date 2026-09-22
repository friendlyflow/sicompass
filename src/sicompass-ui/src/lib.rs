//! sicompass-ui — the list renderer.
//!
//! Everything that turns an FFON tree into pixels and into an AccessKit tree:
//! the SDL3 window, the Vulkan device, font rasterisation, the list layout, the
//! key handlers, the insert-mode text field, and the screen-reader bridge.
//!
//! It knows about [`sicompass_sdk::Provider`] and nothing else. It does not
//! know that `settings.json` exists, that there is an updater, or that plugins
//! can be loaded — those belong to whoever embeds it, and are reached through
//! [`HostHooks`].
//!
//! Two embedders today: the `sicompass` application and the `loginsicompass`
//! greeter.
//!
//! # The boundary, as a rule
//!
//! Nothing here may depend on `sicompass-builtins`, `sicompass-updater`,
//! `wasmtime` or `reqwest`, and nothing here may import a `lib_*` provider
//! crate. Those are what a login screen must not have to link. When the
//! renderer needs an answer only the embedder has, it asks through
//! [`HostHooks`].

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
pub mod http;
pub mod icon;
pub mod image;
pub mod list;
pub mod provider;
pub mod rectangle;
pub mod registry;
pub mod session_mode;
pub mod shaders;
pub mod shortcuts;
pub mod state;
pub mod text;
pub mod unicode_search;

pub use app_state::{AppConfig, AppRenderer, AppState, SiError};
pub use registry::{HostHooks, NoHooks, SettingsQueue, register_provider};
