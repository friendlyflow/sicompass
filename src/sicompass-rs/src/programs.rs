//! Provider loading — equivalent to `programs.c`.
//!
//! Reads the settings config, determines which providers are enabled, and
//! registers them in the `AppRenderer`. The settings provider is always
//! registered last so it appears at the bottom of the root list.
//!
//! ## Apply callback pattern
//!
//! The settings provider fires `ApplyFn(key, value)` during `init()` and
//! whenever the user edits a setting.  Because those calls happen inside the
//! provider's own `&mut self` method, they cannot directly mutate
//! `AppRenderer`.  Instead the callback pushes events into a shared
//! `Arc<Mutex<Vec<...>>>` queue that the main loop drains each frame via
//! [`apply_pending_settings`].

use crate::app_state::AppRenderer;
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;
use sicompass_filebrowser::FilebrowserProvider;
use sicompass_settings::SettingsProvider;
use sicompass_tutorial::TutorialProvider;
use sicompass_chatclient::ChatClientProvider;
use sicompass_emailclient::EmailClientProvider;
use sicompass_webbrowser::WebbrowserProvider;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A pending setting event from the settings apply callback.
pub type SettingEvent = (String, String); // (key, value)

/// Shared queue populated by the settings `ApplyFn`.
pub type SettingsQueue = Arc<Mutex<Vec<SettingEvent>>>;

// ---------------------------------------------------------------------------
// Register a provider
// ---------------------------------------------------------------------------

/// Register a `Box<dyn Provider>` into the renderer: calls `init()`, fetches
/// the initial tree, and creates the root `FfonElement::Obj`.
pub fn register_provider(renderer: &mut AppRenderer, mut provider: Box<dyn Provider>) {
    provider.init();
    let children = provider.fetch();
    let display_name = provider.display_name().to_owned();

    let mut root = FfonElement::new_obj(&display_name);
    for child in children {
        root.as_obj_mut().unwrap().push(child);
    }

    renderer.ffon.push(root);
    renderer.providers.push(provider);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load all providers according to the settings config.
///
/// Returns a [`SettingsQueue`] that receives apply-callback events while the
/// settings provider is live.  Pass it to [`apply_pending_settings`] to
/// process those events against `AppRenderer`.
pub fn load_programs(renderer: &mut AppRenderer) -> SettingsQueue {
    let queue: SettingsQueue = Arc::new(Mutex::new(Vec::new()));
    let queue_clone = Arc::clone(&queue);

    // ---- Build the settings provider ----------------------------------------
    let mut settings = SettingsProvider::new(move |k, v| {
        queue_clone.lock().unwrap().push((k.to_owned(), v.to_owned()));
    });

    // Core sicompass settings
    settings.add_radio(
        "sicompass", "color scheme", "colorScheme",
        &["dark", "light"], "dark",
    );
    settings.add_checkbox("sicompass", "maximized", "maximized", false);

    // File-browser settings
    settings.add_radio(
        "file browser", "sort order", "sortOrder",
        &["alphanumerically", "chronologically"], "alphanumerically",
    );

    // "Available programs:" priority section (registered before loading)
    settings.add_priority_section("Available programs:");
    for &(name, config_key, default) in PROGRAM_ENTRIES {
        settings.add_checkbox("Available programs:", name, config_key, default);
    }

    // ---- Always register file browser first --------------------------------
    register_provider(renderer, Box::new(FilebrowserProvider::new()));

    // ---- Load enabled content providers (before registering settings) -------
    let enabled = enabled_programs();
    for name in &enabled {
        match name.as_str() {
            "tutorial" => {
                register_provider(renderer, Box::new(TutorialProvider::new_headless()));
            }
            "web browser" => {
                register_provider(renderer, Box::new(WebbrowserProvider::new()));
            }
            "chat client" => {
                register_provider(renderer, Box::new(ChatClientProvider::new()));
            }
            "email client" => {
                register_provider(renderer, Box::new(EmailClientProvider::new()));
            }
            other => {
                eprintln!("sicompass: unknown program '{other}' — skipping");
            }
        }
    }

    // ---- Register settings as the last provider ----------------------------
    register_provider(renderer, Box::new(settings));

    queue
}

/// Enable a provider by name at runtime (hot-load).
pub fn enable_provider(renderer: &mut AppRenderer, name: &str) {
    // Never double-load an already-registered provider
    if renderer.providers.iter().any(|p| p.name() == name) { return; }
    match name {
        "filebrowser" => {
            register_provider(renderer, Box::new(FilebrowserProvider::new()));
        }
        "tutorial" => {
            register_provider(renderer, Box::new(TutorialProvider::new_headless()));
        }
        "web browser" => {
            register_provider(renderer, Box::new(WebbrowserProvider::new()));
        }
        "chat client" => {
            register_provider(renderer, Box::new(ChatClientProvider::new()));
        }
        "email client" => {
            register_provider(renderer, Box::new(EmailClientProvider::new()));
        }
        other => {
            eprintln!("sicompass: cannot enable unknown provider '{other}'");
        }
    }
}

/// Disable and remove a provider by name.
pub fn disable_provider(renderer: &mut AppRenderer, name: &str) {
    let Some(idx) = renderer.providers.iter().position(|p| p.name() == name) else {
        return;
    };

    renderer.ffon.remove(idx);
    renderer.providers.remove(idx);

    // Clamp root navigation index
    let max_root = renderer.ffon.len().saturating_sub(1);
    if let Some(root_idx) = renderer.current_id.get(0) {
        if root_idx > max_root {
            renderer.current_id.set_last(max_root);
        }
    }
}

// ---------------------------------------------------------------------------
// Apply pending settings from the queue
// ---------------------------------------------------------------------------

/// Drain the settings queue and apply each (key, value) pair to `app`.
///
/// `skip_enable` — when `true`, `enable_*` events are ignored (used during
/// the initial drain to avoid double-loading providers that were already
/// registered by [`load_programs`]).
pub fn apply_pending_settings(
    renderer: &mut AppRenderer,
    queue: &SettingsQueue,
    skip_enable: bool,
) {
    let events: Vec<SettingEvent> = {
        let mut q = queue.lock().unwrap();
        q.drain(..).collect()
    };

    for (key, value) in events {
        apply_setting(renderer, &key, &value, skip_enable);
    }
}

fn apply_setting(
    renderer: &mut AppRenderer,
    key: &str,
    value: &str,
    skip_enable: bool,
) {
    if let Some(name) = key.strip_prefix("enable_") {
        if skip_enable { return; }
        if name == "file browser" { return; } // always present
        if value == "true" {
            enable_provider(renderer, name);
        } else {
            disable_provider(renderer, name);
        }
        return;
    }

    match key {
        "colorScheme" => {
            renderer.palette_theme = if value == "light" {
                crate::app_state::PaletteTheme::Light
            } else {
                crate::app_state::PaletteTheme::Dark
            };
        }
        "maximized" => {
            renderer.pending_maximized = Some(value == "true");
        }
        "saveFolder" => {
            renderer.save_folder_path = value.to_owned();
        }
        _ => {}
    }

    // Broadcast to all providers so they can react to settings that affect them
    // (e.g. chatHomeserver → ChatClientProvider, sortOrder → FilebrowserProvider).
    for provider in &mut renderer.providers {
        provider.on_setting_change(key, value);
    }
}

// ---------------------------------------------------------------------------
// Config helpers
// ---------------------------------------------------------------------------

/// (name, config_key, default_enabled) for the Available programs: section.
const PROGRAM_ENTRIES: &[(&str, &str, bool)] = &[
    ("tutorial",     "enable_tutorial",     true),
    ("sales demo",   "enable_sales demo",   false),
    ("chat client",  "enable_chat client",  false),
    ("email client", "enable_email client", false),
    ("web browser",  "enable_web browser",  false),
];

/// Return true if `display_name` matches `provider_name` when spaces are ignored.
/// e.g., "chat client" matches "chatclient".
pub fn name_matches_provider(display_name: &str, provider_name: &str) -> bool {
    if display_name == provider_name { return true; }
    let stripped: String = display_name.chars().filter(|&c| c != ' ').collect();
    stripped == provider_name
}

fn enabled_programs() -> Vec<String> {
    if let Some(path) = sicompass_sdk::platform::main_config_path() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(root) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(section) = root.get("Available programs:") {
                    let mut result = Vec::new();
                    for &(name, config_key, default) in PROGRAM_ENTRIES {
                        let enabled = section
                            .get(config_key)
                            .and_then(|v| v.as_bool())
                            .unwrap_or(default);
                        if enabled {
                            result.push(name.to_owned());
                        }
                    }
                    if !result.is_empty() {
                        return result;
                    }
                }
            }
        }
    }

    PROGRAM_ENTRIES
        .iter()
        .filter(|&&(_, _, default)| default)
        .map(|&(name, _, _)| name.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::AppRenderer;
    use sicompass_sdk::ffon::FfonElement;
    use sicompass_sdk::provider::Provider;

    struct MockProv { name: String }
    impl MockProv { fn new(n: &str) -> Self { MockProv { name: n.to_owned() } } }
    impl Provider for MockProv {
        fn name(&self) -> &str { &self.name }
        fn fetch(&mut self) -> Vec<FfonElement> { vec![] }
    }

    // --- name_matches_provider ---

    #[test]
    fn name_matches_exact() {
        assert!(name_matches_provider("tutorial", "tutorial"));
    }

    #[test]
    fn name_matches_with_spaces() {
        assert!(name_matches_provider("chat client", "chatclient"));
    }

    #[test]
    fn name_no_match() {
        assert!(!name_matches_provider("chat client", "emailclient"));
    }

    #[test]
    fn name_matches_web_browser() {
        assert!(name_matches_provider("web browser", "webbrowser"));
    }

    #[test]
    fn name_matches_empty_strings() {
        assert!(name_matches_provider("", ""));
    }

    #[test]
    fn name_matches_trailing_spaces() {
        assert!(name_matches_provider("chat ", "chat"));
    }

    // --- register_provider ---

    #[test]
    fn register_provider_adds_to_renderer() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("test")));
        assert_eq!(r.providers.len(), 1);
        assert_eq!(r.ffon.len(), 1);
    }

    #[test]
    fn register_provider_multiple() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("a")));
        register_provider(&mut r, Box::new(MockProv::new("b")));
        register_provider(&mut r, Box::new(MockProv::new("c")));
        assert_eq!(r.providers.len(), 3);
        assert_eq!(r.providers[1].name(), "b");
    }

    // --- disable_provider ---

    #[test]
    fn disable_provider_removes_by_name() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("keep")));
        register_provider(&mut r, Box::new(MockProv::new("remove")));
        disable_provider(&mut r, "remove");
        assert_eq!(r.providers.len(), 1);
        assert_eq!(r.providers[0].name(), "keep");
    }

    #[test]
    fn disable_provider_removes_first() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("first")));
        register_provider(&mut r, Box::new(MockProv::new("second")));
        disable_provider(&mut r, "first");
        assert_eq!(r.providers.len(), 1);
        assert_eq!(r.providers[0].name(), "second");
    }

    #[test]
    fn disable_provider_not_found_is_noop() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("keep")));
        disable_provider(&mut r, "nonexistent");
        assert_eq!(r.providers.len(), 1);
    }

    #[test]
    fn disable_provider_removes_middle() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("a")));
        register_provider(&mut r, Box::new(MockProv::new("b")));
        register_provider(&mut r, Box::new(MockProv::new("c")));
        disable_provider(&mut r, "b");
        assert_eq!(r.providers.len(), 2);
        assert_eq!(r.providers[0].name(), "a");
        assert_eq!(r.providers[1].name(), "c");
    }

    // --- apply_setting (colorScheme) ---

    #[test]
    fn apply_setting_color_scheme_light() {
        let mut r = AppRenderer::new();
        apply_setting(&mut r, "colorScheme", "light", false);
        assert_eq!(r.palette_theme, crate::app_state::PaletteTheme::Light);
    }

    #[test]
    fn apply_setting_color_scheme_dark() {
        let mut r = AppRenderer::new();
        r.palette_theme = crate::app_state::PaletteTheme::Light;
        apply_setting(&mut r, "colorScheme", "dark", false);
        assert_eq!(r.palette_theme, crate::app_state::PaletteTheme::Dark);
    }

    #[test]
    fn apply_setting_color_scheme_unknown_defaults_dark() {
        let mut r = AppRenderer::new();
        apply_setting(&mut r, "colorScheme", "solarized", false);
        assert_eq!(r.palette_theme, crate::app_state::PaletteTheme::Dark);
    }

    #[test]
    fn palette_dark_background_is_black() {
        use crate::app_state::{PALETTE_DARK, PALETTE_LIGHT};
        assert_eq!(PALETTE_DARK.background, 0x000000FF);
        assert_eq!(PALETTE_LIGHT.background, 0xFFFFFFFF);
    }

    #[test]
    fn palette_accessor_returns_dark_by_default() {
        use crate::app_state::{PaletteTheme, PALETTE_DARK};
        let r = AppRenderer::new();
        assert_eq!(r.palette_theme, PaletteTheme::Dark);
        assert_eq!(r.palette().background, PALETTE_DARK.background);
    }

    // --- apply_setting (maximized) ---

    #[test]
    fn apply_setting_maximized_true_sets_pending() {
        let mut r = AppRenderer::new();
        apply_setting(&mut r, "maximized", "true", false);
        assert_eq!(r.pending_maximized, Some(true));
    }

    #[test]
    fn apply_setting_maximized_false_sets_pending() {
        let mut r = AppRenderer::new();
        apply_setting(&mut r, "maximized", "false", false);
        assert_eq!(r.pending_maximized, Some(false));
    }

    #[test]
    fn apply_setting_unknown_key_is_noop() {
        let mut r = AppRenderer::new();
        apply_setting(&mut r, "unknownKey", "someValue", false);
        assert_eq!(r.pending_maximized, None);
        assert_eq!(r.palette_theme, crate::app_state::PaletteTheme::Dark);
    }
}
