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
        sicompass_filebrowser::register();
        sicompass_editor::register();
        sicompass_tutorial::register();
        sicompass_webbrowser::register();
        sicompass_chatclient::register();
        sicompass_emailclient::register();
        sicompass_sales_demo::register();
        sicompass_remote::register();
        sicompass_settings::register();
    });
}

/// Instantiate a `RemoteProvider` for a named remote service.
///
/// `RemoteProvider` cannot fit the zero-arg factory signature, so it is
/// exposed as a named helper here.  The app calls this from `load_remote_programs`
/// and `enable_provider` instead of constructing `RemoteProvider` directly.
pub fn create_remote(
    name: &str,
    remote_url: String,
    api_key: String,
) -> Box<dyn sicompass_sdk::Provider> {
    sicompass_remote::create_remote(name, remote_url, api_key)
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
    fn filebrowser_factory_is_registered() {
        register_all();
        let p = sicompass_sdk::create_provider_by_name("filebrowser");
        assert!(p.is_some(), "filebrowser factory should be registered");
        assert_eq!(p.unwrap().name(), "filebrowser");
    }

    #[test]
    fn tutorial_factory_is_registered() {
        register_all();
        let p = sicompass_sdk::create_provider_by_name("tutorial");
        assert!(p.is_some(), "tutorial factory should be registered");
    }

    #[test]
    fn chatclient_factory_is_registered() {
        register_all();
        let p = sicompass_sdk::create_provider_by_name("chatclient");
        assert!(p.is_some(), "chatclient factory should be registered");
    }

    #[test]
    fn emailclient_factory_is_registered() {
        register_all();
        let p = sicompass_sdk::create_provider_by_name("emailclient");
        assert!(p.is_some(), "emailclient factory should be registered");
    }

    #[test]
    fn editor_factory_is_registered() {
        register_all();
        let p = sicompass_sdk::create_provider_by_name("editor");
        assert!(p.is_some(), "editor factory should be registered");
        assert_eq!(p.unwrap().name(), "editor");
    }

    #[test]
    fn settings_factory_is_registered() {
        register_all();
        let p = sicompass_sdk::create_provider_by_name("settings");
        assert!(p.is_some(), "settings factory should be registered");
    }

    #[test]
    fn create_remote_returns_provider() {
        let p = create_remote("myservice", "http://example.com".to_owned(), String::new());
        assert_eq!(p.name(), "myservice");
    }

    #[test]
    fn builtin_manifests_include_filebrowser_always_enabled() {
        register_all();
        let manifests = sicompass_sdk::builtin_manifests();
        let fb = manifests.iter().find(|m| m.name == "filebrowser");
        assert!(fb.is_some(), "filebrowser manifest should be registered");
        assert!(fb.unwrap().always_enabled, "filebrowser should be always_enabled");
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
