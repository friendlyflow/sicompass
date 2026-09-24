//! Registers all built-in sicompass providers with the SDK factory and manifest
//! registries.
//!
//! This is the **only** crate in the workspace that has direct dependencies on
//! all the individual `lib_*` crates.  The app (`src/sicompass`) depends only
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
        sicompass_webbrowser::register();
        sicompass_emailclient::register();
        sicompass_settings::register();
        sicompass_store::register();
    });
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
    fn emailclient_factory_is_registered() {
        register_all();
        let p = sicompass_sdk::create_provider_by_name("emailclient");
        assert!(p.is_some(), "emailclient factory should be registered");
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

    #[test]
    fn builtin_manifests_include_email_settings() {
        register_all();
        let manifests = sicompass_sdk::builtin_manifests();
        let email = manifests.iter().find(|m| m.name == "emailclient");
        assert!(email.is_some());
        let settings = &email.unwrap().settings;
        assert_eq!(settings.len(), 6, "email client should declare 6 settings");
    }
}
