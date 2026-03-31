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
use crate::plugin_loader::{NativePlugin, ScriptProvider};
use crate::plugin_manifest::{PluginType, discover_user_plugins};
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;
use std::path::PathBuf;
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
// Settings FFON refresh
// ---------------------------------------------------------------------------

/// Rebuild the settings FFON element in-place (settings is always the last provider).
///
/// Call this after modifying the settings provider's sections at runtime so the
/// displayed tree reflects the new state. Mirrors C's `refreshSettingsFfon`.
fn rebuild_settings_ffon(renderer: &mut AppRenderer) {
    if renderer.ffon.is_empty() { return; }
    let settings_idx = renderer.ffon.len() - 1;
    if let Some(settings_provider) = renderer.providers.last_mut() {
        let children = settings_provider.fetch();
        let display_name = settings_provider.display_name().to_owned();
        let mut root = FfonElement::new_obj(&display_name);
        for child in children {
            root.as_obj_mut().unwrap().push(child);
        }
        renderer.ffon[settings_idx] = root;
    }
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
                register_provider(renderer, Box::new(TutorialProvider::new(&tutorial_assets_dir())));
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

    // ---- Register a settings section for each loaded program ---------------
    // Only programs that actually have a provider registered get a section.
    // Using renderer.providers (not `enabled`) as the source of truth ensures
    // that only checked programs with a working implementation appear in settings.
    // Use the PROGRAM_ENTRIES name (e.g. "chat client") rather than provider.name()
    // (e.g. "chatclient") so section names are consistent across initial load and
    // dynamic enable/disable.
    for p in renderer.providers.iter() {
        if let Some(&(entry_name, _, _)) = PROGRAM_ENTRIES.iter()
            .find(|&&(n, _, _)| name_matches_provider(n, p.name()))
        {
            settings.add_section(entry_name);
        }
    }

    // ---- Load user-installed plugins ----------------------------------------
    load_user_plugins(renderer, &mut settings);

    // ---- Sort all registered providers alphabetically ----------------------
    sort_providers_alphabetically(renderer);

    // ---- Register settings as the last provider ----------------------------
    register_provider(renderer, Box::new(settings));

    queue
}

/// Discover plugins in `~/.config/sicompass/plugins/`, inject their settings
/// entries, and register them as providers.
fn load_user_plugins(renderer: &mut AppRenderer, settings: &mut SettingsProvider) {
    for plugin in discover_user_plugins() {
        let m = &plugin.manifest;

        // Inject per-plugin settings into the settings provider.
        for s in &m.settings {
            use crate::plugin_manifest::SettingKind;
            match s.kind {
                SettingKind::Text => {
                    settings.add_text(&m.display_name, &s.label, &s.key, &s.default);
                }
                SettingKind::Checkbox => {
                    settings.add_checkbox(
                        &m.display_name, &s.label, &s.key, s.default_checked,
                    );
                }
                SettingKind::Radio => {
                    let opts: Vec<&str> = s.options.iter().map(String::as_str).collect();
                    settings.add_radio(
                        &m.display_name, &s.label, &s.key, &opts, &s.default,
                    );
                }
            }
        }

        // Construct and register the provider.
        let provider: Option<Box<dyn Provider>> = match m.plugin_type {
            PluginType::Native => NativePlugin::open(&plugin.entry_path)
                .map(|p| Box::new(p) as Box<dyn Provider>),
            PluginType::Script => Some(Box::new(ScriptProvider::new(
                &m.name,
                &m.display_name,
                plugin.entry_path.clone(),
            ))),
        };

        if let Some(p) = provider {
            register_provider(renderer, p);
        } else {
            eprintln!(
                "sicompass: failed to load plugin '{}' from {}",
                m.name,
                plugin.entry_path.display()
            );
        }
    }
}

/// Resolve the tutorial assets directory relative to the running executable.
/// Falls back to a cwd-relative path if the executable path is unavailable.
fn tutorial_assets_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("../../lib/lib_tutorial/assets")))
        .unwrap_or_else(|| PathBuf::from("lib/lib_tutorial/assets"))
}

/// Enable a provider by name at runtime (hot-load).
///
/// The new provider is inserted alphabetically by name between the filebrowser
/// (always index 0) and settings (always last). If the current root navigation
/// index points at or after the insertion point, it is incremented so the
/// selection stays on the same provider.
pub fn enable_provider(renderer: &mut AppRenderer, name: &str) {
    // Never double-load an already-registered provider.
    // Use name_matches_provider so "chat client" matches provider.name() "chatclient".
    if renderer.providers.iter().any(|p| name_matches_provider(name, p.name())) { return; }
    let provider: Box<dyn Provider> = match name {
        "filebrowser" => Box::new(FilebrowserProvider::new()),
        "tutorial"    => Box::new(TutorialProvider::new(&tutorial_assets_dir())),
        "web browser" => Box::new(WebbrowserProvider::new()),
        "chat client" => Box::new(ChatClientProvider::new()),
        "email client"=> Box::new(EmailClientProvider::new()),
        other => {
            eprintln!("sicompass: cannot enable unknown provider '{other}'");
            return;
        }
    };
    insert_provider_alphabetically(renderer, provider);
}

/// Sort all currently registered providers (and their ffon entries) alphabetically
/// by name (case-insensitive). Call this before registering settings so settings
/// stays last.
fn sort_providers_alphabetically(renderer: &mut AppRenderer) {
    let len = renderer.providers.len();
    if len <= 1 {
        return;
    }
    let mut pairs: Vec<(Box<dyn Provider>, FfonElement)> = renderer.providers
        .drain(..)
        .zip(renderer.ffon.drain(..))
        .collect();
    pairs.sort_by(|(a, _), (b, _)| {
        a.name().to_ascii_lowercase().cmp(&b.name().to_ascii_lowercase())
    });
    let (providers, ffon): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
    renderer.providers = providers;
    renderer.ffon = ffon;
}

/// Insert a provider at the alphabetically correct position.
///
/// Settings (last index) is never displaced. All other providers (including
/// filebrowser) participate in alphabetical ordering.
/// The insertion point is found by scanning indices `0..len-1` and picking
/// the first slot where the existing provider's name sorts after the new name
/// (case-insensitive ASCII). Falls back to just before settings.
fn insert_provider_alphabetically(renderer: &mut AppRenderer, mut provider: Box<dyn Provider>) {
    provider.init();
    let children = provider.fetch();
    let display_name = provider.display_name().to_owned();

    let mut root = FfonElement::new_obj(&display_name);
    for child in children {
        root.as_obj_mut().unwrap().push(child);
    }

    // Find insertion index: anywhere before settings (last).
    let settings_idx = renderer.providers.len().saturating_sub(1);
    let new_name_lower = provider.name().to_ascii_lowercase();
    // Determine the canonical settings section name (PROGRAM_ENTRIES name, e.g. "chat client")
    // before consuming `provider` below.
    let section_name = PROGRAM_ENTRIES.iter()
        .find(|&&(n, _, _)| name_matches_provider(n, provider.name()))
        .map(|&(n, _, _)| n.to_owned())
        .unwrap_or_else(|| display_name.clone());
    let mut insert_idx = settings_idx; // default: just before settings
    for i in 0..settings_idx {
        if renderer.providers[i].name().to_ascii_lowercase() > new_name_lower {
            insert_idx = i;
            break;
        }
    }

    renderer.ffon.insert(insert_idx, root);
    renderer.providers.insert(insert_idx, provider);
    if let Some(settings) = renderer.providers.last_mut() {
        settings.add_settings_section(&section_name);
    }
    rebuild_settings_ffon(renderer);

    // Adjust root navigation index if it points at or after the insertion point.
    if let Some(root_idx) = renderer.current_id.get(0) {
        if root_idx >= insert_idx {
            renderer.current_id.set(0, root_idx + 1);
        }
    }
}

/// Disable and remove a provider by name.
pub fn disable_provider(renderer: &mut AppRenderer, name: &str) {
    // Use name_matches_provider so "chat client" matches provider.name() "chatclient".
    let Some(idx) = renderer.providers.iter().position(|p| name_matches_provider(name, p.name())) else {
        return;
    };

    let removed_provider_name = renderer.providers[idx].name().to_owned();
    // Use the PROGRAM_ENTRIES canonical name for section removal (e.g. "chat client"
    // not "chatclient") to match what was added during load or dynamic enable.
    let removed_section_name = PROGRAM_ENTRIES.iter()
        .find(|&&(n, _, _)| name_matches_provider(n, &removed_provider_name))
        .map(|&(n, _, _)| n.to_owned())
        .unwrap_or_else(|| removed_provider_name.clone());
    renderer.ffon.remove(idx);
    renderer.providers.remove(idx);

    // Remove the settings section for the disabled program and rebuild.
    if let Some(settings) = renderer.providers.last_mut() {
        settings.remove_settings_section(&removed_section_name);
    }
    rebuild_settings_ffon(renderer);

    // Clamp root navigation index
    let max_root = renderer.ffon.len().saturating_sub(1);
    if let Some(root_idx) = renderer.current_id.get(0) {
        if root_idx > max_root {
            renderer.current_id.set(0, max_root);
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

    // --- sort_providers_alphabetically ---

    #[test]
    fn sort_providers_alphabetically_orders_by_name() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("tutorial")));
        register_provider(&mut r, Box::new(MockProv::new("chat client")));
        register_provider(&mut r, Box::new(MockProv::new("filebrowser")));
        sort_providers_alphabetically(&mut r);
        assert_eq!(r.providers[0].name(), "chat client");
        assert_eq!(r.providers[1].name(), "filebrowser");
        assert_eq!(r.providers[2].name(), "tutorial");
    }

    #[test]
    fn sort_providers_alphabetically_single_provider_noop() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("only")));
        sort_providers_alphabetically(&mut r);
        assert_eq!(r.providers.len(), 1);
        assert_eq!(r.providers[0].name(), "only");
    }

    // --- insert_provider_alphabetically ---

    #[test]
    fn insert_alphabetically_between_filebrowser_and_settings() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("filebrowser")));
        register_provider(&mut r, Box::new(MockProv::new("settings")));
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("tutorial")));
        // Expected: filebrowser, tutorial, settings
        assert_eq!(r.providers.len(), 3);
        assert_eq!(r.providers[0].name(), "filebrowser");
        assert_eq!(r.providers[1].name(), "tutorial");
        assert_eq!(r.providers[2].name(), "settings");
    }

    #[test]
    fn insert_alphabetically_sorts_among_existing_providers() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("filebrowser")));
        register_provider(&mut r, Box::new(MockProv::new("tutorial")));
        register_provider(&mut r, Box::new(MockProv::new("settings")));
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("chat client")));
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("email client")));
        // Expected: chat client, email client, filebrowser, tutorial, settings
        assert_eq!(r.providers.len(), 5);
        assert_eq!(r.providers[0].name(), "chat client");
        assert_eq!(r.providers[1].name(), "email client");
        assert_eq!(r.providers[2].name(), "filebrowser");
        assert_eq!(r.providers[3].name(), "tutorial");
        assert_eq!(r.providers[4].name(), "settings");
    }

    #[test]
    fn insert_alphabetically_adjusts_current_id_when_before() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("filebrowser")));
        register_provider(&mut r, Box::new(MockProv::new("tutorial")));
        register_provider(&mut r, Box::new(MockProv::new("settings")));
        // Simulate user navigated inside tutorial (depth 2): current_id = [1, 3]
        r.current_id.set_last(1); // root → tutorial (index 1)
        r.current_id.push(3);     // navigated one level deeper
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("chat client")));
        // chat client inserts at index 1, tutorial shifts to index 2
        // Root index (depth 0) must be incremented; deeper index (depth 1) must be unchanged
        assert_eq!(r.current_id.get(0), Some(2));
        assert_eq!(r.current_id.get(1), Some(3));
    }

    #[test]
    fn insert_alphabetically_no_adjust_when_after() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("filebrowser")));
        register_provider(&mut r, Box::new(MockProv::new("chat client")));
        register_provider(&mut r, Box::new(MockProv::new("settings")));
        // Stay at filebrowser (index 0) — AppRenderer::new() already sets current_id to [0]
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("tutorial")));
        // tutorial inserts at index 2, filebrowser stays at index 0
        assert_eq!(r.current_id.get(0), Some(0));
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
