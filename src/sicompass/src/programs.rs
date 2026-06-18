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
use crate::plugin_manifest::{DiscoveredPlugin, PluginManifest, PluginType, discover_user_plugins};
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;
use sicompass_updater::UpdateEvent;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A pending setting event from the settings apply callback.
pub type SettingEvent = (String, String); // (key, value)

/// Shared queue populated by the settings `ApplyFn`.
pub type SettingsQueue = Arc<Mutex<Vec<SettingEvent>>>;

// ---------------------------------------------------------------------------
// User plugin cache (mirrors C's s_userPlugins)
// ---------------------------------------------------------------------------

/// Cache of all discovered user plugins, populated once at startup by
/// `load_user_plugins`. Used by `enable_provider` to hot-load user plugins
/// at runtime without re-scanning the filesystem.
///
/// **Locking rule**: clone a `DiscoveredPlugin` out of the lock, drop the
/// guard, THEN call `instantiate_user_plugin` or `register_provider`.
/// Never hold this mutex across a call that mutates `AppRenderer`.
static USER_PLUGIN_CACHE: OnceLock<Mutex<Vec<DiscoveredPlugin>>> = OnceLock::new();

fn user_plugin_cache() -> &'static Mutex<Vec<DiscoveredPlugin>> {
    USER_PLUGIN_CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Seed the cache (test helper). Only the first call to `OnceLock::get_or_init`
/// wins; tests that need a fresh cache use this to replace the contents.
#[cfg(test)]
pub(crate) fn _reset_user_plugin_cache(plugins: Vec<DiscoveredPlugin>) {
    // The OnceLock may already be initialized; update the Vec inside the Mutex.
    let cache = user_plugin_cache();
    *cache.lock().unwrap() = plugins;
}

// ---------------------------------------------------------------------------
// Register a provider
// ---------------------------------------------------------------------------

/// Register a `Box<dyn Provider>` into the renderer: calls `init()`, fetches
/// the initial tree, and creates the root `FfonElement::Obj`.
pub fn register_provider(renderer: &mut AppRenderer, provider: Box<dyn Provider>) {
    let (provider, root) = init_provider_root(provider, &mut renderer.error_message);
    renderer.ffon.push(root);
    renderer.providers.push(provider);
}

/// `init()` a provider and build its FFON root Obj (key = display name,
/// children = first `fetch()`). Any fetch error is surfaced through `err_sink`.
/// Shared by `register_provider` and the per-tab content rebuild.
fn init_provider_root(
    mut provider: Box<dyn Provider>,
    err_sink: &mut String,
) -> (Box<dyn Provider>, FfonElement) {
    provider.init();
    let children = provider.fetch();
    if let Some(err) = provider.take_error() {
        eprintln!("provider '{}' fetch error on register: {err}", provider.display_name());
        *err_sink = err;
    }
    let display_name = provider.display_name().to_owned();
    let mut root = FfonElement::new_obj(&display_name);
    for child in children {
        root.as_obj_mut().unwrap().push(child);
    }
    (provider, root)
}

/// Re-instantiate a fresh provider instance by name, mirroring `enable_provider`'s
/// resolution order: built-ins first, then the user-plugin cache, then a remote
/// FFON service. Returns `None` only when the name matches none of these — the
/// caller substitutes a placeholder so per-tab provider indices stay aligned.
fn reinstantiate_provider(name: &str) -> Option<Box<dyn Provider>> {
    if let Some(p) = instantiate_builtin(name) {
        return Some(p);
    }
    let cached: Option<DiscoveredPlugin> = {
        let guard = user_plugin_cache().lock().unwrap();
        guard.iter()
            .find(|p| p.manifest.name == name || p.manifest.display_name == name)
            .cloned()
    };
    if let Some(plugin) = cached {
        if let Some(p) = instantiate_user_plugin(&plugin) {
            return Some(p);
        }
    }
    if let Some((remote_url, api_key)) = read_remote_config(name) {
        if !api_key.is_empty() {
            crate::provider::register_auth(&remote_url, &api_key);
        }
        return Some(sicompass_builtins::create_remote(name, remote_url, api_key));
    }
    None
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
    // Run one-time migrations of obsolete config keys.
    if let Some(path) = sicompass_sdk::platform::main_config_path() {
        migrate_programs_to_load(&path);
        migrate_editor_to_text_editor(&path);
    }

    // Set the active locale BEFORE any provider is constructed, so every
    // first `display_name()` / `fetch()` already resolves in the user's
    // chosen language — no English flash on startup. We bypass the settings
    // provider (not built yet) and read straight from settings.json.
    if let Some(path) = sicompass_sdk::platform::main_config_path() {
        if let Some(lang) = read_language_from_config(&path) {
            sicompass_sdk::localize::set_locale(&lang);
        }
    }

    let queue: SettingsQueue = Arc::new(Mutex::new(Vec::new()));
    let queue_clone = Arc::clone(&queue);

    // ---- Build the settings provider via factory registry -------------------
    let mut settings: Box<dyn Provider> = sicompass_sdk::create_provider_by_name("settings")
        .expect("settings factory must be registered — call sicompass_builtins::register_all() first");
    settings.set_apply_callback(Box::new(move |k, v| {
        queue_clone.lock().unwrap().push((k.to_owned(), v.to_owned()));
    }));
    if let Some(path) = sicompass_sdk::platform::main_config_path() {
        settings.set_config_path(path);
    }

    // Surface the sicompass app version as a child of the "sicompass" section.
    settings.set_section_version("sicompass", env!("CARGO_PKG_VERSION"));

    // Core sicompass settings. Labels are Fluent message IDs so the settings
    // panel renders them in the user's chosen language; lib_settings reverses
    // the translation when the user clicks a toggle so settings.json stays
    // language-neutral.
    settings.add_radio_setting(
        "sicompass", "color scheme", "colorScheme",
        &["dark", "light"], "dark",
    );
    settings.add_checkbox_setting("sicompass", "settings-checkbox-maximized", "maximized", false);
    settings.add_checkbox_setting("sicompass", "settings-checkbox-shoulder-surfing-protection", "shoulderSurfingProtection", false);
    // Checked at startup by `read_auto_update_check_setting` in main.rs.
    // Toggling at runtime only affects the next launch — we don't spawn /
    // cancel the updater thread on the fly.
    settings.add_checkbox_setting("sicompass", "settings-checkbox-auto-update-check", "autoUpdateCheck", true);
    settings.add_radio_setting(
        "sicompass", "settings-radio-font-scale", "fontScale",
        &["1.00", "1.25", "1.50", "1.75", "2.00", "2.25", "2.50"],
        "1.00",
    );
    settings.add_radio_setting(
        "sicompass", "settings-radio-language", "language",
        &["en-US", "nl-BE", "fr-BE", "de-BE"], "en-US",
    );

    // File-browser settings
    settings.add_radio_setting(
        "file browser", "settings-radio-sort-order", "sortOrder",
        &["alphanumerically", "chronologically"], "alphanumerically",
    );

    // "Available programs:" priority section.
    // Built-in program checkboxes are added first; user-plugin checkboxes are
    // added by load_user_plugins() below (after discovery).
    settings.add_priority_section("Available programs:");
    for m in sicompass_sdk::builtin_manifests() {
        if !m.always_enabled {
            let config_key = format!("enable_{}", m.display_name);
            settings.add_checkbox_setting("Available programs:", &m.display_name, &config_key, m.enable_default);
        }
    }

    // ---- Build the content providers + configure their settings sections ----
    load_content_providers(renderer, Some(settings.as_mut()));

    // ---- Register settings as the last provider ----------------------------
    register_provider(renderer, settings);

    queue
}

/// Build and register the content providers (everything except the shared
/// settings provider) into `renderer`, in canonical order: always-enabled
/// builtins first, then enabled opt-in builtins, then user plugins, then remote
/// services, all sorted alphabetically.
///
/// When `settings` is `Some`, also configures the settings provider (injects
/// per-provider setting entries, registers sections, adds plugin/remote
/// checkboxes) — done once, at initial app load (`load_programs`). New tabs
/// pass `None`: they reuse the already-configured shared settings provider and
/// only need fresh provider instances, so they must NOT mutate settings (which
/// would duplicate sections/checkboxes). The registered provider set is
/// identical either way, so provider indices stay stable across tabs.
pub fn load_content_providers(
    renderer: &mut AppRenderer,
    mut settings: Option<&mut dyn Provider>,
) {
    // Always-enabled providers first (e.g. file browser).
    for m in sicompass_sdk::builtin_manifests() {
        if m.always_enabled {
            if let Some(p) = instantiate_builtin(&m.name) {
                register_provider(renderer, p);
            }
            if let Some(s) = settings.as_deref_mut() {
                if !m.settings.is_empty() {
                    inject_builtin_manifest_settings(s, &m);
                }
            }
        }
    }

    // Enabled opt-in content providers.
    let enabled = enabled_programs();
    for name in &enabled {
        if let Some(p) = instantiate_builtin(name.as_str()) {
            if let Some(s) = settings.as_deref_mut() {
                let manifest = sicompass_sdk::builtin_manifests()
                    .into_iter()
                    .find(|m| m.display_name == *name || m.name == *name);
                if let Some(m) = &manifest {
                    inject_builtin_manifest_settings(s, m);
                }
            }
            register_provider(renderer, p);
        } else {
            eprintln!("sicompass: unknown program '{name}' — skipping");
        }
    }

    // Register a settings section for each loaded builtin program.
    if let Some(s) = settings.as_deref_mut() {
        for p in renderer.providers.iter() {
            if let Some(m) = sicompass_sdk::builtin_manifests()
                .into_iter()
                .find(|m| name_matches_provider(&m.display_name, p.name()))
            {
                s.add_settings_section(&m.display_name);
            }
        }
    }

    // User-installed plugins, then remote services.
    load_user_plugins(renderer, settings.as_deref_mut());
    load_remote_programs(renderer, settings.as_deref_mut());

    // Sort content providers alphabetically (settings is appended afterwards).
    sort_providers_alphabetically(renderer);
}

/// Build a fresh, independent set of content providers + their ffon roots for a
/// new tab by re-instantiating each provider in `names` (the active tab's
/// content providers, everything except the trailing shared settings provider).
///
/// Mirroring the live set name-for-name (rather than re-running the discovery
/// pipeline) guarantees the new tab has the exact same provider count and order,
/// so `current_id[0]` stays valid across tabs. A provider that can't be
/// re-instantiated is replaced by an inert [`GenericProvider`] placeholder
/// carrying its name, so indices never shift.
///
/// Callers must capture `names` *before* detaching the live content (a detach
/// leaves only the settings provider behind). `renderer` is borrowed only as an
/// error sink.
pub fn build_content_set_from_names(
    renderer: &mut AppRenderer,
    names: &[String],
) -> (Vec<Box<dyn Provider>>, Vec<FfonElement>) {
    let mut fresh_p: Vec<Box<dyn Provider>> = Vec::with_capacity(names.len());
    let mut fresh_f: Vec<FfonElement> = Vec::with_capacity(names.len());
    for name in names {
        let provider = reinstantiate_provider(name).unwrap_or_else(|| {
            eprintln!("sicompass: cannot re-instantiate provider '{name}' for new tab — using placeholder");
            Box::new(sicompass_sdk::provider::GenericProvider::new(
                name.clone(), name.clone(), |_| Vec::new(),
            ))
        });
        let (provider, root) = init_provider_root(provider, &mut renderer.error_message);
        fresh_p.push(provider);
        fresh_f.push(root);
    }
    (fresh_p, fresh_f)
}

/// Inject setting entries from a `BuiltinManifest` into the settings provider.
/// Called from both the startup load loop and `enable_provider` so hot-enable
/// registers identical settings to startup-enable.
fn inject_builtin_manifest_settings(settings: &mut dyn Provider, manifest: &sicompass_sdk::BuiltinManifest) {
    use sicompass_sdk::SettingKind;
    for s in &manifest.settings {
        match s.kind {
            SettingKind::Text => {
                settings.add_text_setting(&s.section, &s.label, &s.key, &s.default);
            }
            SettingKind::Checkbox => {
                settings.add_checkbox_setting(&s.section, &s.label, &s.key, s.default_checked);
            }
            SettingKind::Radio => {
                let opts: Vec<&str> = s.options.iter().map(String::as_str).collect();
                settings.add_radio_setting(&s.section, &s.label, &s.key, &opts, &s.default);
            }
        }
    }
}

/// Instantiate a built-in provider by name via the SDK factory registry.
///
/// Accepts both display names with spaces (e.g. `"chat client"`) and the
/// compact factory keys (e.g. `"chatclient"`).  Tries the exact name first,
/// then strips spaces as a fallback so callers from `instantiate_user_plugin`
/// (which uses `manifest.name`) work without conversion.
fn instantiate_builtin(name: &str) -> Option<Box<dyn Provider>> {
    // Try exact name first (e.g. "sales demo", "filebrowser").
    if let Some(p) = sicompass_sdk::create_provider_by_name(name) {
        return Some(p);
    }
    // Fallback: try with spaces stripped (e.g. "chat client" → "chatclient").
    let compact: String = name.chars().filter(|&c| c != ' ').collect();
    sicompass_sdk::create_provider_by_name(&compact)
}

/// Instantiate a user plugin (Script, Native, or Factory) from its discovered manifest.
///
/// Rejects plugins whose `minAppVersion` exceeds the running app version —
/// the plugin file may already be on disk from an update, but if it
/// declares it needs a newer app than this one, loading it would risk a
/// missing-symbol crash. Logging makes the skip visible.
fn instantiate_user_plugin(plugin: &DiscoveredPlugin) -> Option<Box<dyn Provider>> {
    let m = &plugin.manifest;

    if let Some(min) = m.min_app_version.as_deref() {
        let current = env!("CARGO_PKG_VERSION");
        match (semver::Version::parse(min.trim_start_matches('v')),
               semver::Version::parse(current)) {
            (Ok(min_v), Ok(cur_v)) if min_v > cur_v => {
                eprintln!(
                    "sicompass: skipping plugin '{}' — needs app >= {min} but running {current}",
                    m.name
                );
                return None;
            }
            _ => {}
        }
    }

    match m.plugin_type {
        PluginType::Native => NativePlugin::open(&plugin.entry_path)
            .map(|p| Box::new(p) as Box<dyn Provider>),
        PluginType::Script => Some(Box::new(ScriptProvider::new(
            &m.name,
            &m.display_name,
            plugin.entry_path.clone(),
        ).with_supports_config_files(m.supports_config_files))),
        PluginType::Factory => instantiate_builtin(&m.name),
    }
}

/// Inject a plugin manifest's settings entries into the settings provider.
fn inject_plugin_settings(settings: &mut dyn Provider, manifest: &PluginManifest) {
    use crate::plugin_manifest::SettingKind;
    for s in &manifest.settings {
        match s.kind {
            SettingKind::Text => {
                settings.add_text_setting(&manifest.display_name, &s.label, &s.key, &s.default);
            }
            SettingKind::Checkbox => {
                settings.add_checkbox_setting(
                    &manifest.display_name, &s.label, &s.key, s.default_checked,
                );
            }
            SettingKind::Radio => {
                let opts: Vec<&str> = s.options.iter().map(String::as_str).collect();
                settings.add_radio_setting(
                    &manifest.display_name, &s.label, &s.key, &opts, &s.default,
                );
            }
        }
    }
}

/// Discover plugins in `~/.config/sicompass/plugins/`, add their checkboxes to
/// "Available programs:", and register those that are enabled.
///
/// Mirrors `discoverUserPlugins` + `registerProgramsSection` (user half) +
/// the user-plugin loading loop in `programsLoad` from `src/sicompass/programs.c`.
/// Load enabled user plugins into `renderer`. When `settings` is `Some`, also
/// configures the settings provider (checkboxes, per-plugin settings, sections,
/// versions) — done once at initial load. New tabs pass `None`: they reuse the
/// already-configured shared settings provider and must only instantiate fresh
/// provider instances, so settings is left untouched. The set of registered
/// providers is identical regardless of `settings`, keeping provider indices
/// stable across tabs.
fn load_user_plugins(renderer: &mut AppRenderer, mut settings: Option<&mut dyn Provider>) {
    let discovered = discover_user_plugins();

    // Populate the global cache so hot-enable can find manifests later.
    *user_plugin_cache().lock().unwrap() = discovered.clone();

    for plugin in &discovered {
        let m = &plugin.manifest;

        // Add the enable checkbox to "Available programs:" (same as C's registerProgramsSection).
        let config_key = format!("enable_{}", m.name);
        let currently_enabled = is_plugin_enabled_in_config(&m.name);
        if let Some(s) = settings.as_deref_mut() {
            s.add_checkbox_setting("Available programs:", &m.display_name, &config_key, currently_enabled);
        }

        // Skip disabled plugins (mirrors C's isEnabledInConfig check in programsLoad).
        if !currently_enabled {
            continue;
        }

        if let Some(s) = settings.as_deref_mut() {
            // Inject per-plugin settings and register a section.
            inject_plugin_settings(s, m);
            s.add_settings_section(&m.display_name);
        }

        // Construct and register the provider.
        match instantiate_user_plugin(plugin) {
            Some(p) => {
                register_provider(renderer, p);
                if let Some(s) = settings.as_deref_mut() {
                    // Third-party plugin version: prefer plugin.json's `version`
                    // field; fall back to the provider's own `Provider::version()`.
                    let v: Option<String> = m.version.clone().or_else(|| {
                        renderer
                            .providers
                            .last()
                            .and_then(|p| p.version().map(|s| s.to_owned()))
                    });
                    if let Some(v) = v {
                        s.set_section_version(&m.display_name, &v);
                    }
                }
            }
            None => eprintln!(
                "sicompass: failed to load plugin '{}' from {}",
                m.name,
                plugin.entry_path.display()
            ),
        }
    }
}

/// Check whether a user plugin (by manifest `name`) is enabled in `settings.json`.
/// Returns `false` if the file doesn't exist, the section is absent, or the key
/// is missing (user plugins are opt-in, default disabled — matches C behavior).
fn is_plugin_enabled_in_config(name: &str) -> bool {
    let Some(path) = sicompass_sdk::platform::main_config_path() else {
        return false;
    };
    let Ok(data) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&data) else {
        return false;
    };
    let config_key = format!("enable_{}", name);
    root.get("Available programs:")
        .and_then(|s| s.get(&config_key))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Migrate obsolete `sicompass.programsToLoad` array to individual
/// Read `sicompass.language` from settings.json. Returns `Some(lang)` only
/// when the value is one of the locales the language radio actually offers,
/// so a typo / stale value can't lock the UI to a missing bundle. Called
/// during startup before any provider is constructed, so the active locale
/// is set in time for the first `display_name()` / `fetch()` of every
/// provider — no English flash on launch.
fn read_language_from_config(path: &Path) -> Option<String> {
    const ALLOWED: &[&str] = &["en-US", "nl-BE", "fr-BE", "de-BE"];
    let data = std::fs::read_to_string(path).ok()?;
    let root: serde_json::Value = serde_json::from_str(&data).ok()?;
    let lang = root
        .get("sicompass")
        .and_then(|v| v.as_object())
        .and_then(|sc| sc.get("language"))
        .and_then(|v| v.as_str())?;
    if ALLOWED.contains(&lang) {
        Some(lang.to_owned())
    } else {
        None
    }
}

/// `Available programs:.enable_<name> = true` entries.
///
/// Mirrors `programs.c:422-448`. Runs once at startup; if the key is absent
/// the function is a no-op.
fn migrate_programs_to_load(path: &Path) {
    let Ok(data) = std::fs::read_to_string(path) else { return; };
    let Ok(mut root) = serde_json::from_str::<serde_json::Value>(&data) else { return; };

    let programs_to_load: Vec<String> = {
        let Some(sc) = root.get("sicompass").and_then(|v| v.as_object()) else { return; };
        let Some(ptl) = sc.get("programsToLoad").and_then(|v| v.as_array()) else { return; };
        ptl.iter()
            .filter_map(|v| v.as_str().map(|s| s.to_owned()))
            .filter(|s| !s.is_empty())
            .collect()
    };

    // Insert enable_<name> = true into "Available programs:"
    {
        let available = root
            .as_object_mut().unwrap()
            .entry("Available programs:")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let map = available.as_object_mut().unwrap();
        for name in &programs_to_load {
            let key = format!("enable_{name}");
            map.entry(key).or_insert(serde_json::Value::Bool(true));
        }
    }

    // Remove programsToLoad
    if let Some(sc) = root.get_mut("sicompass").and_then(|v| v.as_object_mut()) {
        sc.remove("programsToLoad");
    }

    // Write back atomically so a concurrent sicompass instance never reads a
    // truncated settings.json (which it would then rebuild from an empty map).
    if let Ok(json) = serde_json::to_string_pretty(&root) {
        let _ = sicompass_sdk::platform::atomic_write(path, &json);
    }
}

/// Migrate the renamed "editor" plugin to "text editor".
///
/// Renames the top-level `"editor"` settings section to `"text editor"` (and
/// its inner `"editorPath"` key to `"textEditorPath"`), and the
/// `Available programs:.enable_editor` toggle to `enable_text editor`. Runs
/// once at startup; if no old keys are present the function is a no-op.
fn migrate_editor_to_text_editor(path: &Path) {
    let Ok(data) = std::fs::read_to_string(path) else { return; };
    let Ok(mut root) = serde_json::from_str::<serde_json::Value>(&data) else { return; };
    let Some(obj) = root.as_object_mut() else { return; };

    let mut changed = false;

    // Move the "editor" section → "text editor", renaming "editorPath" inside.
    if let Some(mut section) = obj.remove("editor") {
        if let Some(sec) = section.as_object_mut() {
            if let Some(v) = sec.remove("editorPath") {
                sec.entry("textEditorPath").or_insert(v);
            }
        }
        // Merge into an existing "text editor" section rather than clobbering.
        match obj.get_mut("text editor").and_then(|v| v.as_object_mut()) {
            Some(existing) => {
                if let Some(sec) = section.as_object() {
                    for (k, v) in sec {
                        existing.entry(k.clone()).or_insert(v.clone());
                    }
                }
            }
            None => {
                obj.insert("text editor".to_owned(), section);
            }
        }
        changed = true;
    }

    // Rename the "Available programs:" enable toggle.
    if let Some(available) = obj.get_mut("Available programs:").and_then(|v| v.as_object_mut()) {
        if let Some(v) = available.remove("enable_editor") {
            available.entry("enable_text editor".to_owned()).or_insert(v);
            changed = true;
        }
    }

    if !changed {
        return;
    }

    // Write back atomically (see migrate_programs_to_load).
    if let Ok(json) = serde_json::to_string_pretty(&root) {
        let _ = sicompass_sdk::platform::atomic_write(path, &json);
    }
}

/// Read `sicompass.maximized` from settings.json.
/// Returns `false` if absent, unparseable, or file missing.
pub fn read_maximized() -> bool {
    let Some(path) = sicompass_sdk::platform::main_config_path() else { return false };
    let Ok(data) = std::fs::read_to_string(&path) else { return false };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&data) else { return false };
    let val = root.get("sicompass")
        .and_then(|v| v.as_object())
        .and_then(|s| s.get("maximized"));
    match val {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) => s == "true",
        _ => false,
    }
}

/// Read `sicompass.tabs` and `sicompass.activeTab` from settings.json and
/// apply them to `r`. Falls back to the existing single-tab default if either
/// key is absent or unparseable.
///
/// `tabs` is a JSON-encoded array of `{"id":[u, ...], "path":"…"}` objects.
/// Tabs whose first index points to a provider that is no longer registered
/// (e.g. the program was disabled) are dropped; if everything is filtered out,
/// the existing default is preserved.
pub fn load_tabs_state(r: &mut crate::app_state::AppRenderer) {
    let Some(path) = sicompass_sdk::platform::main_config_path() else { return };
    let Ok(data) = std::fs::read_to_string(&path) else { return };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&data) else { return };
    let Some(sec) = root.get("sicompass").and_then(|v| v.as_object()) else { return };
    apply_tabs_section(r, sec);
}

/// Apply the parsed `sicompass` settings section to `r`. Split out from
/// [`load_tabs_state`] so tests can exercise the reconciliation logic without
/// depending on the global config path.
pub fn apply_tabs_section(
    r: &mut crate::app_state::AppRenderer,
    sec: &serde_json::Map<String, serde_json::Value>,
) {
    use sicompass_sdk::ffon::IdArray;
    use crate::app_state::TabSnapshot;

    // The bootstrap live set is `[content…, settings]`; every tab shares the
    // same provider ordering, so this count gates persisted `current_id[0]`
    // values for all of them.
    let provider_count = r.providers.len();

    // Parse the persisted nav entries (id + active provider path) per tab,
    // dropping any whose provider index no longer exists.
    let parsed: Vec<(IdArray, String)> = sec.get("tabs").and_then(|v| v.as_str())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| match v { serde_json::Value::Array(a) => Some(a), _ => None })
        .map(|arr| arr.into_iter().filter_map(|v| {
            let obj = v.as_object()?;
            let ids = obj.get("id")?.as_array()?;
            let path = obj.get("path")?.as_str()?.to_owned();
            let mut id = IdArray::new();
            for n in ids {
                id.push(n.as_u64()? as usize);
            }
            match id.get(0) {
                Some(pi) if pi < provider_count && id.depth() > 0 => Some((id, path)),
                _ => None,
            }
        }).collect())
        .unwrap_or_default();

    if !parsed.is_empty() {
        // Capture the bootstrap content provider names BEFORE detaching, so
        // each freshly built tab mirrors them exactly (count + order).
        let content_n = r.providers.len().saturating_sub(1);
        let content_names: Vec<String> = r.providers[..content_n]
            .iter()
            .map(|p| p.name().to_owned())
            .collect();

        // Each restored tab gets its own independent content-provider set.
        // The first tab reuses the already-live bootstrap set; the rest are
        // built fresh. For each we attach the set, rebuild its saved provider
        // path, then park it back out into the tab record.
        let mut tabs: Vec<TabSnapshot> = Vec::with_capacity(parsed.len());
        for (i, (id, path)) in parsed.into_iter().enumerate() {
            let (cp, cf) = if i == 0 {
                r.detach_content()
            } else {
                build_content_set_from_names(r, &content_names)
            };
            r.attach_content(cp, cf);
            r.rebuild_and_clamp(&path, id);
            let current_id = r.current_id.clone();
            let (cp, cf) = r.detach_content();
            tabs.push(TabSnapshot { current_id, provider_path: path, providers: cp, ffon: cf });
        }
        r.tabs = tabs;
        // Keep `tab_timelines` parallel to `tabs` (invariant relied on by
        // `active_timeline_mut()`).
        r.tab_timelines.resize_with(r.tabs.len(), crate::app_state::Timeline::new);

        // Resolve the active tab and make its parked content the live set.
        let mut active = 0;
        if let Some(active_str) = sec.get("activeTab").and_then(|v| v.as_str()) {
            if let Ok(n) = active_str.parse::<usize>() {
                if n < r.tabs.len() { active = n; }
            }
        }
        r.active_tab = active;
        // Seed the MRU order to a default front-first sequence (no real visit
        // history exists across restarts; the active tab leads).
        r.reset_mru_default(active);
        let cp = std::mem::take(&mut r.tabs[active].providers);
        let cf = std::mem::take(&mut r.tabs[active].ffon);
        r.attach_content(cp, cf);
        r.current_id = r.tabs[active].current_id.clone();
        r.list_index = r.current_id.last().unwrap_or(0);
        return;
    }

    // No persisted tabs: keep the single bootstrap tab. Keep timelines parallel
    // and apply the active tab's saved nav (path is empty by default → no-op).
    r.tab_timelines.resize_with(r.tabs.len(), crate::app_state::Timeline::new);
    if r.active_tab >= r.tabs.len() { r.active_tab = 0; }
    r.reset_mru_default(r.active_tab);
    r.load_active_tab();
}

/// Read `sicompass.fontScale` from settings.json.
/// Returns 1.0 if absent or unparseable. Clamped to [1.0, 2.5].
pub fn read_font_scale() -> f32 {
    let Some(path) = sicompass_sdk::platform::main_config_path() else { return 1.0 };
    let Ok(data) = std::fs::read_to_string(&path) else { return 1.0 };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&data) else { return 1.0 };
    let raw = root.get("sicompass")
        .and_then(|v| v.as_object())
        .and_then(|s| s.get("fontScale"))
        .and_then(|v| {
            v.as_str().map(|s| s.to_owned())
                .or_else(|| v.as_f64().map(|f| f.to_string()))
        });
    raw.and_then(|s| s.parse::<f32>().ok())
        .map(|f| f.clamp(1.0, 2.5))
        .unwrap_or(1.0)
}

/// Read `remoteUrl` and `apiKey` from settings.json for the given section.
/// Returns `None` if the file or section is absent, or if `remoteUrl` is empty.
fn read_remote_config(section: &str) -> Option<(String, String)> {
    let path = sicompass_sdk::platform::main_config_path()?;
    let data = std::fs::read_to_string(&path).ok()?;
    let root = serde_json::from_str::<serde_json::Value>(&data).ok()?;
    let sec = root.get(section)?.as_object()?;
    let remote_url = sec.get("remoteUrl")?.as_str()?.to_owned();
    if remote_url.is_empty() { return None; }
    let api_key = sec.get("apiKey").and_then(|v| v.as_str()).unwrap_or("").to_owned();
    Some((remote_url, api_key))
}

/// Scan `Available programs:` for `enable_*=true` entries whose names don't
/// match any known built-in or user plugin, and register them as remote FFON
/// providers.  Mirrors the "unknown program → remote service" branch of C's
/// `loadProgram` (src/sicompass/programs.c:247-273) but applied at startup so
/// remote services are reachable without requiring a hot-enable action.
fn load_remote_programs(renderer: &mut AppRenderer, mut settings: Option<&mut dyn Provider>) {
    let path = match sicompass_sdk::platform::main_config_path() {
        Some(p) => p,
        None => return,
    };
    let data = match std::fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => return,
    };
    let root = match serde_json::from_str::<serde_json::Value>(&data) {
        Ok(v) => v,
        Err(_) => return,
    };
    let available = match root.get("Available programs:").and_then(|v| v.as_object()) {
        Some(m) => m,
        None => return,
    };

    let builtin_manifests = sicompass_sdk::builtin_manifests();
    for (key, val) in available {
        // Only process enable_*=true keys.
        let name = match key.strip_prefix("enable_") {
            Some(n) => n,
            None => continue,
        };
        if val.as_bool() != Some(true) { continue; }

        // Skip known builtins and already-registered providers.
        if builtin_manifests.iter().any(|m| m.display_name == name || m.name == name) { continue; }
        if renderer.providers.iter().any(|p| name_matches_provider(name, p.name())) { continue; }

        // Read remoteUrl; skip if absent.
        let (remote_url, api_key) = match read_remote_config(name) {
            Some(cfg) => cfg,
            None => continue,
        };

        if !api_key.is_empty() {
            crate::provider::register_auth(&remote_url, &api_key);
        }

        let provider: Box<dyn Provider> =
            sicompass_builtins::create_remote(name, remote_url, api_key);
        register_provider(renderer, provider);

        // Register the two settings text entries for this remote service.
        if let Some(s) = settings.as_deref_mut() {
            s.add_text_setting(name, "remote URL", "remoteUrl", "");
            s.add_text_setting(name, "API key",    "apiKey",    "");
            s.add_settings_section(name);
        }
    }
}

/// Enable a provider by name at runtime (hot-load).
///
/// Checks built-in names first, then looks up the `USER_PLUGIN_CACHE` for
/// user-installed plugins. Unknown names are tried as remote FFON services if
/// `settings.json` contains a `remoteUrl` for them. Mirrors C's
/// `programsEnableProvider` + `findManifest`.
///
/// The new provider is inserted alphabetically by name between the filebrowser
/// (always index 0) and settings (always last). If the current root navigation
/// index points at or after the insertion point, it is incremented so the
/// selection stays on the same provider.
pub fn enable_provider(renderer: &mut AppRenderer, name: &str) {
    // Never double-load an already-registered provider.
    if renderer.providers.iter().any(|p| name_matches_provider(name, p.name())) { return; }

    // Try built-ins first.
    if let Some(provider) = instantiate_builtin(name) {
        let manifest = sicompass_sdk::builtin_manifests()
            .into_iter()
            .find(|m| m.display_name == name || m.name == name);
        insert_provider_alphabetically(
            renderer,
            provider,
            Some(Box::new(move |settings: &mut dyn Provider| {
                if let Some(ref m) = manifest {
                    inject_builtin_manifest_settings(settings, m);
                }
            })),
            None,
        );
        return;
    }

    // Try user-plugin cache (clone to avoid holding the lock across provider init).
    let cached: Option<DiscoveredPlugin> = {
        let guard = user_plugin_cache().lock().unwrap();
        guard.iter().find(|p| p.manifest.name == name || p.manifest.display_name == name).cloned()
    };
    if let Some(plugin) = cached {
        if let Some(provider) = instantiate_user_plugin(&plugin) {
            insert_provider_alphabetically(
                renderer,
                provider,
                None,
                plugin.manifest.version.clone(),
            );
            return;
        }
    }

    // Unknown name: try remote FFON service fallback. Mirrors the
    // loadProgram remote branch in src/sicompass/programs.c:247-273.
    if let Some((remote_url, api_key)) = read_remote_config(name) {
        if !api_key.is_empty() {
            crate::provider::register_auth(&remote_url, &api_key);
        }
        let provider: Box<dyn Provider> =
            sicompass_builtins::create_remote(name, remote_url, api_key);
        let section_name = name.to_owned();
        insert_provider_alphabetically(renderer, provider, Some(Box::new(move |settings: &mut dyn Provider| {
            settings.add_text_setting(&section_name, "remote URL", "remoteUrl", "");
            settings.add_text_setting(&section_name, "API key",    "apiKey",    "");
        })), None);
        return;
    }

    eprintln!("sicompass: cannot enable unknown provider '{name}'");
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
///
/// `extra_settings` — optional closure called on the settings provider (the
/// last entry) after the section is registered.  Used by the remote-service
/// fallback to inject `remoteUrl` / `apiKey` text entries.
fn insert_provider_alphabetically(
    renderer: &mut AppRenderer,
    mut provider: Box<dyn Provider>,
    extra_settings: Option<Box<dyn FnOnce(&mut dyn Provider)>>,
    manifest_version: Option<String>,
) {
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
    // Determine the canonical settings section name before consuming `provider`.
    let section_name = sicompass_sdk::builtin_manifests()
        .into_iter()
        .find(|m| name_matches_provider(&m.display_name, provider.name()))
        .map(|m| m.display_name)
        .unwrap_or_else(|| display_name.clone());
    let mut insert_idx = settings_idx; // default: just before settings
    for i in 0..settings_idx {
        if renderer.providers[i].name().to_ascii_lowercase() > new_name_lower {
            insert_idx = i;
            break;
        }
    }

    let provider_version: Option<String> = manifest_version
        .or_else(|| provider.version().map(|s| s.to_owned()));

    renderer.ffon.insert(insert_idx, root);
    renderer.providers.insert(insert_idx, provider);
    if let Some(settings) = renderer.providers.last_mut() {
        settings.add_settings_section(&section_name);
        if let Some(v) = &provider_version {
            settings.set_section_version(&section_name, v);
        }
        if let Some(cb) = extra_settings {
            cb(settings.as_mut());
        }
    }
    rebuild_settings_ffon(renderer);

    // Adjust root navigation index if it points at or after the insertion point.
    if let Some(root_idx) = renderer.current_id.get(0) {
        if root_idx >= insert_idx {
            renderer.current_id.set(0, root_idx + 1);
        }
    }
}

/// Drain any pending `UpdateEvent`s from the updater thread and refresh
/// the "update available" banner. Called once per frame from the main
/// loop. Never panics.
///
/// FUTURE NOTIFICATION SYSTEM: when sicompass grows a real in-app
/// notification surface, replace the `error_message` writes below with
/// calls to the new notification API. The `update_state` Arc upstream
/// and the Ctrl+U keybind downstream stay unchanged — only this
/// rendering site moves. Grep for "FUTURE NOTIFICATION SYSTEM" to find
/// all interim shims.
pub fn process_update_events(renderer: &mut AppRenderer) {
    // ---- Drain HotReload events ----
    let events: Vec<UpdateEvent> = match renderer.update_event_rx.as_ref() {
        Some(rx) => rx.try_iter().collect(),
        None => Vec::new(),
    };

    for evt in events {
        match evt {
            UpdateEvent::HotReload { plugin_name, new_entry_path } => {
                match hot_reload_plugin(renderer, &plugin_name, &new_entry_path) {
                    Ok(()) => {
                        tracing::info!("hot-reloaded plugin '{plugin_name}'");
                        // Force banner refresh so user sees the result.
                        renderer.update_message_active = false;
                    }
                    Err(e) => {
                        tracing::warn!("hot reload '{plugin_name}': {e}");
                        renderer.error_message =
                            format!("plugin '{plugin_name}': {e}");
                    }
                }
            }
        }
    }

    // ---- Refresh banner ----
    let state = match renderer.update_state.as_ref() {
        Some(s) => Arc::clone(s),
        None => return,
    };
    let snap = state.lock().unwrap().clone();

    let plugin_applied: Vec<_> = snap.plugin_updates.iter().filter(|p| p.applied).collect();
    let app_pending = snap.app_update.is_some();
    let has_news = app_pending || !plugin_applied.is_empty();

    // FUTURE NOTIFICATION SYSTEM: this is the interim surface for update
    // notifications. The error-slot is borrowed only when it is empty or
    // contains our own previous banner, so real provider errors never get
    // clobbered.
    if !has_news {
        if renderer.update_message_active {
            renderer.error_message.clear();
            renderer.update_message_active = false;
        }
        return;
    }

    let can_write =
        renderer.error_message.is_empty() || renderer.update_message_active;
    if !can_write {
        return;
    }

    let mut msg = String::new();
    if let Some(au) = snap.app_update.as_ref() {
        msg = format!(
            "Update available: app v{} (Ctrl+U to install)",
            au.new_version
        );
    }
    if !plugin_applied.is_empty() {
        if !msg.is_empty() {
            msg.push_str(" — ");
        }
        if plugin_applied.len() == 1 {
            let p = plugin_applied[0];
            msg.push_str(&format!("Plugin updated: {} v{}", p.plugin_name, p.new_version));
        } else {
            msg.push_str(&format!("{} plugin updates applied", plugin_applied.len()));
        }
    }

    renderer.error_message = msg;
    renderer.update_message_active = true;
}

/// Apply the staged app update (Ctrl+U). Spawns the platform installer
/// and, on Windows, terminates the process. Surfaces failures via
/// `error_message`.
pub fn handle_apply_app_update(renderer: &mut AppRenderer) {
    let Some(state) = renderer.update_state.as_ref() else {
        return;
    };
    let snap = state.lock().unwrap().clone();
    let Some(app_update) = snap.app_update else {
        renderer.error_message = "no app update staged".to_string();
        return;
    };

    let checker = sicompass_updater::UpdateChecker::new(
        sicompass_updater::parse_version(env!("CARGO_PKG_VERSION"))
            .unwrap_or_else(|_| semver::Version::new(0, 0, 0)),
        std::path::PathBuf::new(),
        "",
        "",
    );
    match checker.apply_app_update(&app_update) {
        Ok(()) => {
            // Unreachable on Windows (we exit inside apply_app_update);
            // on other platforms this branch isn't reached either (the
            // call returns Err with Unsupported).
        }
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
            sicompass_sdk::platform::open_with_default(&app_update.release_url);
            renderer.error_message = "Opened release page in browser".to_string();
        }
        Err(e) => {
            renderer.error_message = format!("apply update failed: {e}");
        }
    }
}

/// No-op provider used as a placeholder in the registry slot while a
/// native plugin is being hot-reloaded. Must exist for one instant
/// (between `drop(old)` and `place(new)`) so that the OS dynamic linker
/// can free the old `.so`/`.dll`/`.dylib` before we `dlopen` the new
/// file at the same path. Without this drop-first dance, dlopen returns
/// the cached old library and the update has no effect.
struct HotReloadPlaceholder;
impl Provider for HotReloadPlaceholder {
    fn name(&self) -> &str { "" }
    fn fetch(&mut self) -> Vec<FfonElement> { Vec::new() }
}

/// Tear down a running user plugin and re-instantiate it from the
/// already-swapped-in entry file. Called from the main loop when an
/// `UpdateEvent::HotReload` event arrives from the updater thread.
///
/// The plugin's directory on disk has already been atomically replaced
/// by `sicompass-updater`; this function only handles the in-memory
/// provider swap. Returns an error string suitable for `error_message`
/// surfacing on failure.
///
/// Order matters for native plugins (see `HotReloadPlaceholder`): the
/// old `Box<dyn Provider>` must be dropped before the new library is
/// loaded, otherwise the dynamic linker reuses the cached handle and
/// the update is invisible. Script plugins are immune (they spawn a
/// subprocess), but we keep the same order for both for simplicity.
pub fn hot_reload_plugin(
    renderer: &mut AppRenderer,
    plugin_name: &str,
    new_entry: &Path,
) -> Result<(), String> {
    let idx = renderer
        .providers
        .iter()
        .position(|p| name_matches_provider(plugin_name, p.name()))
        .ok_or_else(|| format!("plugin '{plugin_name}' not currently loaded"))?;

    let plugins_root = sicompass_sdk::platform::plugins_dir()
        .ok_or_else(|| "no plugins dir on this platform".to_string())?;
    let manifest_path = plugins_root.join(plugin_name).join("plugin.json");
    let manifest = crate::plugin_manifest::load_manifest(&manifest_path)
        .ok_or_else(|| format!("read updated manifest {}", manifest_path.display()))?;

    if !manifest.hot_reload {
        return Err(format!(
            "plugin '{plugin_name}' opted out of hot reload — restart to activate update"
        ));
    }

    let plugin = DiscoveredPlugin {
        manifest,
        entry_path: new_entry.to_path_buf(),
    };

    // Drop-first dance — see HotReloadPlaceholder doc.
    let placeholder: Box<dyn Provider> = Box::new(HotReloadPlaceholder);
    let old = std::mem::replace(&mut renderer.providers[idx], placeholder);
    drop(old);

    let mut new_provider = match instantiate_user_plugin(&plugin) {
        Some(p) => p,
        None => {
            return Err(format!(
                "failed to instantiate updated plugin '{plugin_name}' \
                 — provider disabled until restart"
            ));
        }
    };

    new_provider.init();
    let children = new_provider.fetch();
    let display_name = new_provider.display_name().to_owned();
    let mut root = FfonElement::new_obj(&display_name);
    for child in children {
        root.as_obj_mut().unwrap().push(child);
    }
    renderer.ffon[idx] = root;
    renderer.providers[idx] = new_provider;

    // Refresh the version shown in the settings tree for this plugin.
    if let Some(v) = plugin.manifest.version.as_deref() {
        if let Some(settings) = renderer.providers.last_mut() {
            settings.set_section_version(&plugin.manifest.display_name, v);
        }
        rebuild_settings_ffon(renderer);
    }

    Ok(())
}

/// Disable and remove a provider by name.
pub fn disable_provider(renderer: &mut AppRenderer, name: &str) {
    let Some(idx) = renderer.providers.iter().position(|p| name_matches_provider(name, p.name())) else {
        return;
    };

    let removed_provider_name = renderer.providers[idx].name().to_owned();
    // Use the builtin manifest display_name for section removal when available.
    // For user plugins, fall back to the provider name itself (which equals manifest.display_name).
    let removed_section_name = sicompass_sdk::builtin_manifests()
        .into_iter()
        .find(|m| name_matches_provider(&m.display_name, &removed_provider_name))
        .map(|m| m.display_name)
        .unwrap_or_else(|| {
            // Check user plugin cache for display_name
            let guard = user_plugin_cache().lock().unwrap();
            guard.iter()
                .find(|p| p.manifest.name == removed_provider_name || p.manifest.display_name == removed_provider_name)
                .map(|p| p.manifest.display_name.clone())
                .unwrap_or(removed_provider_name.clone())
        });
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

/// Propagate a just-enabled provider to every INACTIVE tab's parked content set
/// so the enabled-program list stays identical across tabs. The active tab is
/// skipped (already handled by [`enable_provider`], whose changes live in
/// `renderer.providers`). Each parked tab gets its OWN fresh instance, so other
/// tabs' running shells are left untouched.
fn propagate_enable_to_parked_tabs(renderer: &mut AppRenderer, name: &str) {
    let active = renderer.active_tab;
    for i in 0..renderer.tabs.len() {
        if i == active { continue; }
        // Skip tabs that somehow already have it (idempotent).
        if renderer.tabs[i].providers.iter().any(|p| name_matches_provider(name, p.name())) {
            continue;
        }
        let Some(provider) = reinstantiate_provider(name) else { continue; };
        let (provider, root) = init_provider_root(provider, &mut renderer.error_message);
        let new_name_lower = provider.name().to_ascii_lowercase();

        let tab = &mut renderer.tabs[i];
        let mut insert_idx = tab.providers.len();
        for j in 0..tab.providers.len() {
            if tab.providers[j].name().to_ascii_lowercase() > new_name_lower {
                insert_idx = j;
                break;
            }
        }
        tab.ffon.insert(insert_idx, root);
        tab.providers.insert(insert_idx, provider);
        // Keep the parked cursor on the same provider it pointed at.
        if let Some(root_idx) = tab.current_id.get(0) {
            if root_idx >= insert_idx {
                tab.current_id.set(0, root_idx + 1);
            }
        }
    }
}

/// Propagate a just-disabled provider's removal to every INACTIVE tab's parked
/// content set. Dropping the parked instance fires its Drop (killing that tab's
/// shell for a terminal), which is the intended teardown.
fn propagate_disable_to_parked_tabs(renderer: &mut AppRenderer, name: &str) {
    let active = renderer.active_tab;
    for i in 0..renderer.tabs.len() {
        if i == active { continue; }
        let tab = &mut renderer.tabs[i];
        let Some(idx) = tab.providers.iter().position(|p| name_matches_provider(name, p.name()))
        else { continue; };
        tab.ffon.remove(idx);
        tab.providers.remove(idx);
        // Fix the parked cursor's provider index.
        let max_root = tab.ffon.len().saturating_sub(1);
        if let Some(root_idx) = tab.current_id.get(0) {
            if root_idx == idx {
                // The provider the cursor was in is gone — collapse to a shallow
                // id at the clamped position.
                let mut id = sicompass_sdk::ffon::IdArray::new();
                id.push(idx.min(max_root));
                tab.current_id = id;
            } else if root_idx > idx {
                tab.current_id.set(0, root_idx - 1);
            }
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
            propagate_enable_to_parked_tabs(renderer, name);
        } else {
            disable_provider(renderer, name);
            propagate_disable_to_parked_tabs(renderer, name);
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
            // During the startup settings drain (skip_enable=true) the window
            // builder flag already handles the initial maximize state, so skip
            // queuing a redundant runtime request.  Only set pending_maximized
            // for live checkbox toggles (skip_enable=false).
            if !skip_enable {
                renderer.pending_maximized = Some(value == "true");
            }
        }
        "shoulderSurfingProtection" => {
            renderer.privacy_blank = value == "true";
        }
        "saveFolder" => {
            renderer.save_folder_path = value.to_owned();
        }
        "fontScale" => {
            renderer.rebuild_font_renderer = true;
        }
        "language" => {
            // Switch the active locale on every t() / t_args() call from now
            // on, then (a) re-key every provider's root Obj so the root
            // program list flips immediately (display_name() is translation-
            // backed for every provider) and (b) re-fetch the active
            // provider so its children flip too. Inactive providers' deeper
            // children re-fetch lazily on next navigation / F5.
            sicompass_sdk::localize::set_locale(value);
            crate::provider::refresh_all_provider_root_keys(renderer);
            crate::provider::refresh_current_directory(renderer);
        }
        _ => {}
    }

    // Broadcast to all providers so they can react to settings that affect them
    // (e.g. chatHomeserver → ChatClientProvider, sortOrder → FilebrowserProvider).
    // Cover BOTH the active tab's live providers AND every inactive tab's parked
    // providers, so a setting change reaches all tabs, not just the visible one.
    // (The active tab's own slot has empty parked vectors, so no double-apply.)
    for provider in &mut renderer.providers {
        provider.on_setting_change(key, value);
    }
    for tab in &mut renderer.tabs {
        for provider in &mut tab.providers {
            provider.on_setting_change(key, value);
        }
    }
}

// ---------------------------------------------------------------------------
// Config helpers
// ---------------------------------------------------------------------------

/// Return true if `display_name` matches `provider_name` when spaces are ignored.
/// e.g., "chat client" matches "chatclient".
pub fn name_matches_provider(display_name: &str, provider_name: &str) -> bool {
    if display_name == provider_name { return true; }
    let stripped: String = display_name.chars().filter(|&c| c != ' ').collect();
    stripped == provider_name
}

fn enabled_programs() -> Vec<String> {
    let manifests = sicompass_sdk::builtin_manifests();
    let non_always: Vec<_> = manifests.iter().filter(|m| !m.always_enabled).collect();

    if let Some(path) = sicompass_sdk::platform::main_config_path() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(root) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(section) = root.get("Available programs:") {
                    let mut result = Vec::new();
                    for m in &non_always {
                        let config_key = format!("enable_{}", m.display_name);
                        let enabled = section
                            .get(&config_key)
                            .and_then(|v| v.as_bool())
                            .unwrap_or(m.enable_default);
                        if enabled {
                            result.push(m.display_name.clone());
                        }
                    }
                    if !result.is_empty() {
                        return result;
                    }
                }
            }
        }
    }

    non_always
        .iter()
        .filter(|m| m.enable_default)
        .map(|m| m.display_name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::AppRenderer;
    use crate::plugin_manifest::{PluginManifest, PluginType};
    use sicompass_sdk::ffon::FfonElement;
    use sicompass_sdk::provider::Provider;
    use std::io::Write;

    fn write_config(json: &str) -> tempfile::NamedTempFile {
        let f = tempfile::NamedTempFile::new().expect("temp file");
        std::fs::write(f.path(), json).expect("write");
        f
    }

    #[test]
    fn read_language_returns_allowed_locale() {
        let f = write_config(r#"{"sicompass":{"language":"nl-BE"}}"#);
        assert_eq!(read_language_from_config(f.path()), Some("nl-BE".to_owned()));
    }

    #[test]
    fn read_language_ignores_unknown_locale() {
        // Typo / stale value must NOT propagate to set_locale — otherwise the
        // UI would lock to a bundle that doesn't exist.
        let f = write_config(r#"{"sicompass":{"language":"klingon"}}"#);
        assert_eq!(read_language_from_config(f.path()), None);
    }

    #[test]
    fn read_language_returns_none_when_missing() {
        let f = write_config(r#"{"sicompass":{"colorScheme":"dark"}}"#);
        assert_eq!(read_language_from_config(f.path()), None);
    }

    #[test]
    fn read_language_returns_none_when_file_absent() {
        let missing = std::env::temp_dir().join("sicompass-no-such-file-xyz.json");
        let _ = std::fs::remove_file(&missing);
        assert_eq!(read_language_from_config(&missing), None);
    }

    #[test]
    fn read_language_returns_none_when_malformed() {
        let f = write_config("{not json");
        assert_eq!(read_language_from_config(f.path()), None);
    }

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
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("tutorial")), None, None);
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
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("chat client")), None, None);
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("email client")), None, None);
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
        r.current_id.set_last(1);
        r.current_id.push(3);
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("chat client")), None, None);
        assert_eq!(r.current_id.get(0), Some(2));
        assert_eq!(r.current_id.get(1), Some(3));
    }

    #[test]
    fn insert_alphabetically_no_adjust_when_after() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("filebrowser")));
        register_provider(&mut r, Box::new(MockProv::new("chat client")));
        register_provider(&mut r, Box::new(MockProv::new("settings")));
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("tutorial")), None, None);
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
    fn apply_setting_maximized_skipped_during_startup_drain() {
        let mut r = AppRenderer::new();
        // skip_enable=true simulates the startup drain; pending_maximized must stay None.
        apply_setting(&mut r, "maximized", "true", true);
        assert_eq!(r.pending_maximized, None);
    }

    #[test]
    fn apply_setting_font_scale_triggers_rebuild() {
        let mut r = AppRenderer::new();
        assert!(!r.rebuild_font_renderer);
        apply_setting(&mut r, "fontScale", "1.250", false);
        assert!(r.rebuild_font_renderer);
    }

    #[test]
    fn apply_setting_unknown_key_is_noop() {
        let mut r = AppRenderer::new();
        apply_setting(&mut r, "unknownKey", "someValue", false);
        assert_eq!(r.pending_maximized, None);
        assert_eq!(r.palette_theme, crate::app_state::PaletteTheme::Dark);
    }

    // --- migrate_programs_to_load ---

    #[test]
    fn migrate_programs_to_load_creates_enable_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{
            "sicompass": {
                "programsToLoad": ["tutorial", "web browser"],
                "colorScheme": "dark"
            }
        }"#).unwrap();

        migrate_programs_to_load(&path);

        let data = std::fs::read_to_string(&path).unwrap();
        let root: serde_json::Value = serde_json::from_str(&data).unwrap();

        // enable keys should be set
        let available = root.get("Available programs:").unwrap();
        assert_eq!(available.get("enable_tutorial").unwrap().as_bool(), Some(true));
        assert_eq!(available.get("enable_web browser").unwrap().as_bool(), Some(true));

        // programsToLoad should be removed
        assert!(root["sicompass"].get("programsToLoad").is_none());
        // colorScheme should still be present
        assert_eq!(root["sicompass"]["colorScheme"].as_str(), Some("dark"));
    }

    #[test]
    fn migrate_programs_to_load_no_programs_to_load_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let original = r#"{"sicompass":{"colorScheme":"dark"}}"#;
        std::fs::write(&path, original).unwrap();

        migrate_programs_to_load(&path);

        let data = std::fs::read_to_string(&path).unwrap();
        // File should be unchanged (no programsToLoad key means no migration needed)
        assert!(data.contains("colorScheme"));
        assert!(!data.contains("Available programs:"));
    }

    // --- migrate_editor_to_text_editor ---

    #[test]
    fn migrate_editor_to_text_editor_renames_section_and_toggle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{
            "editor": { "editorPath": "/home/nico/Dropbox" },
            "Available programs:": { "enable_editor": true }
        }"#).unwrap();

        migrate_editor_to_text_editor(&path);

        let data = std::fs::read_to_string(&path).unwrap();
        let root: serde_json::Value = serde_json::from_str(&data).unwrap();

        // Old keys are gone.
        assert!(root.get("editor").is_none());
        assert!(root["Available programs:"].get("enable_editor").is_none());
        // New keys present with carried-over values.
        assert_eq!(
            root["text editor"]["textEditorPath"].as_str(),
            Some("/home/nico/Dropbox")
        );
        assert_eq!(
            root["Available programs:"]["enable_text editor"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn migrate_editor_to_text_editor_no_old_keys_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let original = r#"{"sicompass":{"colorScheme":"dark"}}"#;
        std::fs::write(&path, original).unwrap();

        migrate_editor_to_text_editor(&path);

        let data = std::fs::read_to_string(&path).unwrap();
        assert_eq!(data, original, "file must be left byte-identical");
    }

    // --- enable_provider with user plugin cache ---

    fn make_test_manifest(name: &str) -> PluginManifest {
        PluginManifest {
            name: name.to_owned(),
            display_name: name.to_owned(),
            plugin_type: PluginType::Script,
            entry: "plugin.ts".to_owned(),
            supports_config_files: false,
            settings: vec![],
            version: None,
            update_url: None,
            min_app_version: None,
            pubkey: None,
            hot_reload: true,
        }
    }

    fn make_discovered_plugin(name: &str) -> DiscoveredPlugin {
        DiscoveredPlugin {
            manifest: make_test_manifest(name),
            entry_path: PathBuf::from("/nonexistent/plugin.ts"),
        }
    }

    /// Serializes tests that mutate the process-wide `USER_PLUGIN_CACHE`. The
    /// cache's own `Mutex` guards individual accesses but not the
    /// set-then-read invariant each test relies on, so without this lock a
    /// parallel test can reset the cache between another test's set and read.
    static PLUGIN_CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn enable_provider_unknown_name_logs_and_returns() {
        let _cache_guard = PLUGIN_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut r = AppRenderer::new();
        // Seed empty cache so we don't accidentally pick up real plugins
        _reset_user_plugin_cache(vec![]);
        register_provider(&mut r, Box::new(MockProv::new("settings")));
        let len_before = r.providers.len();
        enable_provider(&mut r, "completely-unknown-plugin");
        // No provider should be added (ScriptProvider init would fail loading
        // /nonexistent/plugin.ts, but with an empty cache the early return fires first)
        assert_eq!(r.providers.len(), len_before);
    }

    #[test]
    fn disable_then_reenable_user_plugin_via_cache() {
        // This test validates that enable_provider finds the user plugin in the cache.
        // ScriptProvider doesn't actually run `bun` in tests (init() is a no-op,
        // fetch() calls bun which will fail silently returning []).
        let _cache_guard = PLUGIN_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut r = AppRenderer::new();
        let plugin = make_discovered_plugin("my-demo");
        _reset_user_plugin_cache(vec![plugin]);

        // Pre-register a settings sentinel at the end
        register_provider(&mut r, Box::new(MockProv::new("settings")));
        let before = r.providers.len();

        enable_provider(&mut r, "my-demo");
        // ScriptProvider is created (even if bun fails, the provider object is inserted)
        assert_eq!(r.providers.len(), before + 1);
        assert!(r.providers.iter().any(|p| p.name() == "my-demo"));
    }

    // --- inject_builtin_settings registers text entries on hot-enable ---

    /// Helper: register a headless SettingsProvider last, call enable_provider for
    /// `name`, then return the FFON fetch output from the settings provider.
    fn settings_ffon_after_enable(name: &str) -> Vec<FfonElement> {
        use sicompass_settings::SettingsProvider;
        let _cache_guard = PLUGIN_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Ensure the factory registry is populated (normally done in main()).
        sicompass_builtins::register_all();
        let mut r = AppRenderer::new();
        _reset_user_plugin_cache(vec![]);
        register_provider(&mut r, Box::new(SettingsProvider::new_headless()));
        enable_provider(&mut r, name);
        r.providers.last_mut().unwrap().fetch()
    }

    fn section_children<'a>(ffon: &'a [FfonElement], section_name: &str) -> Option<&'a Vec<FfonElement>> {
        ffon.iter()
            .find_map(|e| e.as_obj().filter(|o| o.key == section_name))
            .map(|o| &o.children)
    }

    #[test]
    fn hot_enable_email_client_registers_settings() {
        let ffon = settings_ffon_after_enable("email client");
        let children = section_children(&ffon, "email client")
            .expect("email client section should be present");
        // Should have 6 text entries, not the fallback "no settings"
        assert!(
            !children.iter().any(|e| e.as_str() == Some("no settings")),
            "email client section should not show 'no settings'"
        );
        let inputs: Vec<_> = children.iter()
            .filter_map(|e| e.as_str())
            .filter(|s| s.contains("<input>"))
            .collect();
        assert_eq!(inputs.len(), 6, "expected 6 text settings, got {}: {:?}", inputs.len(), inputs);
    }

    #[test]
    fn hot_enable_chat_client_registers_settings() {
        let ffon = settings_ffon_after_enable("chat client");
        let children = section_children(&ffon, "chat client")
            .expect("chat client section should be present");
        assert!(
            !children.iter().any(|e| e.as_str() == Some("no settings")),
            "chat client section should not show 'no settings'"
        );
        let inputs: Vec<_> = children.iter()
            .filter_map(|e| e.as_str())
            .filter(|s| s.contains("<input>"))
            .collect();
        assert_eq!(inputs.len(), 5, "expected 5 text settings, got {}: {:?}", inputs.len(), inputs);
    }

    #[test]
    fn hot_enable_sales_demo_registers_settings() {
        let ffon = settings_ffon_after_enable("sales demo");
        let children = section_children(&ffon, "sales demo")
            .expect("sales demo section should be present");
        assert!(
            !children.iter().any(|e| e.as_str() == Some("no settings")),
            "sales demo section should not show 'no settings'"
        );
        let inputs: Vec<_> = children.iter()
            .filter_map(|e| e.as_str())
            .filter(|s| s.contains("<input>"))
            .collect();
        assert_eq!(inputs.len(), 1, "expected 1 text setting, got {}: {:?}", inputs.len(), inputs);
    }

    #[test]
    fn hot_enable_text_editor_registers_settings() {
        let ffon = settings_ffon_after_enable("text editor");
        let children = section_children(&ffon, "text editor")
            .expect("text editor section should be present after hot-enable");
        assert!(
            !children.iter().any(|e| e.as_str() == Some("no settings")),
            "text editor section should not show 'no settings'"
        );
        let inputs: Vec<_> = children.iter()
            .filter_map(|e| e.as_str())
            .filter(|s| s.contains("<input>"))
            .collect();
        assert_eq!(inputs.len(), 1, "expected 1 text setting (text editor path), got {}: {:?}", inputs.len(), inputs);
        assert!(
            inputs[0].contains("text editor path"),
            "the single input should be 'text editor path', got: {}", inputs[0]
        );
    }
}
