//! Registers all built-in sicompass providers with the SDK factory and manifest
//! registries.
//!
//! This is the **only** crate in the workspace that has direct dependencies on
//! all the individual `lib_*` crates.  The app (`src/`) depends only
//! on this crate and `sicompass-sdk` — never on individual lib crates.
//!
//! ## Usage
//!
//! Call [`register_all`] once at the very start of `main`, before
//! [`load_programs`](sicompass) runs:
//!
//! ```no_run
//! sicompass_builtins::register_all();
//! ```

use std::sync::OnceLock;

static REGISTERED: OnceLock<()> = OnceLock::new();

/// Register all built-in providers with the SDK factory and manifest registries.
///
/// Idempotent — safe to call multiple times (only the first call has effect).
pub fn register_all() {
    REGISTERED.get_or_init(|| {
        sicompass_tutorial::register();
        sicompass_settings::register();
        sicompass_store::register();
    });
}

/// The plugin updates waiting for this computer, for the app's update message
/// (the app may not reach the Store crate itself). See
/// `sicompass_store::updates`.
pub mod plugin_updates {
    pub use sicompass_store::updates::PendingUpdate;
    use sicompass_store::updates::{self, Sources};

    /// Every installed plugin with a newer release. Downloads: call it off the
    /// UI thread.
    pub fn check() -> Vec<PendingUpdate> {
        Sources::here()
            .map(|s| updates::check(&s))
            .unwrap_or_default()
    }

    /// Install one, recording its approval. Refused when it asks for more
    /// access. Downloads: call it off the UI thread.
    pub fn install(update: &PendingUpdate) -> Result<String, String> {
        let sources = Sources::here().ok_or("no plugins folder on this system")?;
        updates::install(&sources, update)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_all_is_idempotent() {
        register_all();
        register_all(); // second call must not panic
    }

    #[test]
    fn tutorial_factory_is_registered() {
        register_all();
        let p = sicompass_sdk::create_provider_by_name("tutorial");
        assert!(p.is_some(), "tutorial factory should be registered");
    }

    #[test]
    fn store_is_registered_and_always_present() {
        register_all();
        let p = sicompass_sdk::create_provider_by_name("store");
        assert_eq!(p.map(|p| p.name().to_owned()).as_deref(), Some("store"));
        let m = sicompass_sdk::builtin_manifests()
            .into_iter()
            .find(|m| m.name == "store")
            .expect("store manifest");
        assert!(
            m.always_enabled,
            "the Store is not optional: it installs the others"
        );
    }

    #[test]
    fn settings_factory_is_registered() {
        register_all();
        let p = sicompass_sdk::create_provider_by_name("settings");
        assert!(p.is_some(), "settings factory should be registered");
    }
}
