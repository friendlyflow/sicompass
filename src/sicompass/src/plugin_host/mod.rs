//! Host for plugin processes: a plugin is a program of its own, which the app
//! starts and talks to over its stdin and stdout
//! ([`sicompass_sdk::plugin_ipc`]).
//!
//! | Piece | Where |
//! |---|---|
//! | Starting it, the channel, deadlines, letting it go | [`channel`] |
//! | What it asks the app, and the answers | [`services`] |
//! | The `Provider` it wears | [`provider`] |
//!
//! A plugin process runs with the user's rights: its `plugin.json`
//! permissions say what it means to do, and the user approves that before it
//! runs, but nothing here enforces them. That is the difference from
//! [`crate::wasm_host`], whose guests can do only what is linked into them.

pub mod channel;
pub mod provider;
pub mod services;

pub use provider::{ProcessProvider, Spec};
