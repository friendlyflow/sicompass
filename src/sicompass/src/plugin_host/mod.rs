//! Host for plugins: a plugin is a program of its own, which the app starts and
//! talks to over its stdin and stdout ([`sicompass_sdk::plugin_ipc`]). See
//! `docs/process-plugins.md`.
//!
//! | Piece | Where |
//! |---|---|
//! | Starting it, the channel, letting it go | [`channel`] |
//! | What it asks the app, and the answers | [`services`] |
//! | The desktop it asks for | [`desktop`] |
//! | Its own files, as `asset:` URIs | [`assets`] |
//! | The `Provider` it wears | [`provider`] |
//!
//! A plugin runs with the user's rights: its `plugin.json` permissions say what
//! it means to do, and the user approves that before it runs, but nothing here
//! enforces them.

use std::path::PathBuf;

pub mod assets;
pub mod channel;
pub mod desktop;
pub mod provider;
pub mod services;

pub use assets::{ASSET_SUBDIR, confine_in, read_confined_asset, register_plugin_assets};
pub use provider::{ProcessProvider, Spec};

// A plugin's `locales/*.ftl` are registered by the SDK, which the tutorial also
// calls to describe the installed plugins, so one once-per-process registry
// covers both.
pub use sicompass_sdk::installed_plugins::{
    LOCALE_SUBDIR, register_locales as register_plugin_locales,
};

/// What the app gives a plugin, from its manifest
/// (`plugin_manifest::grants_for`).
#[derive(Debug, Clone, Default)]
pub struct Grants {
    /// Its own folder, `app_data_dir()/<name>`, when `plugin.json` asks for
    /// `"storage": true`. The app creates it.
    pub storage_dir: Option<PathBuf>,
    /// Keys of the settings the plugin declared in `plugin.json`: the only
    /// ones it is told about and can read. The app broadcasts every settings
    /// change to every provider, and other programs' settings hold API keys
    /// and passwords.
    pub settings: Vec<String>,
    /// Each declared setting's default, `~` already expanded: what the plugin
    /// reads until the user has saved a value.
    pub setting_defaults: Vec<(String, String)>,
    /// The tier the plugin's manifest names as its `service`: the only tier
    /// `license.token` answers for.
    pub service_tier: Option<String>,
    /// It renders the web pages other programs link to (`"rendersPages"`).
    pub renders_pages: bool,
}
