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
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use sicompass_filebrowser::FilebrowserProvider;
use sicompass_settings::SettingsProvider;
use sicompass_tutorial::TutorialProvider;
use sicompass_chatclient::ChatClientProvider;
use sicompass_emailclient::EmailClientProvider;
use sicompass_webbrowser::WebbrowserProvider;
use sicompass_remote::RemoteProvider;

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
pub fn register_provider(renderer: &mut AppRenderer, mut provider: Box<dyn Provider>) {
    provider.init();
    let children = provider.fetch();
    if let Some(err) = provider.take_error() {
        eprintln!("provider '{}' fetch error on register: {err}", provider.display_name());
        renderer.error_message = err;
    }
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
    // Run one-time migration of obsolete programsToLoad config key.
    if let Some(path) = sicompass_sdk::platform::main_config_path() {
        migrate_programs_to_load(&path);
    }

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
    settings.add_radio(
        "sicompass", "font scale", "fontScale",
        &[
            "0.500", "0.625", "0.750", "0.875",
            "1.000", "1.125", "1.250", "1.375",
            "1.500", "1.625", "1.750", "1.875",
            "2.000",
        ],
        "1.000",
    );

    // File-browser settings
    settings.add_radio(
        "file browser", "sort order", "sortOrder",
        &["alphanumerically", "chronologically"], "alphanumerically",
    );

    // "Available programs:" priority section.
    // Built-in program checkboxes are added first; user-plugin checkboxes are
    // added by load_user_plugins() below (after discovery).
    settings.add_priority_section("Available programs:");
    for &(name, config_key, default) in PROGRAM_ENTRIES {
        settings.add_checkbox("Available programs:", name, config_key, default);
    }

    // ---- Always register file browser first --------------------------------
    register_provider(renderer, Box::new(FilebrowserProvider::new()));

    // ---- Load enabled content providers (before registering settings) -------
    let enabled = enabled_programs();
    for name in &enabled {
        if let Some(p) = instantiate_builtin(name.as_str()) {
            inject_builtin_settings(&mut settings, name.as_str());
            register_provider(renderer, p);
        } else {
            eprintln!("sicompass: unknown program '{name}' — skipping");
        }
    }

    // ---- Register a settings section for each loaded program ---------------
    for p in renderer.providers.iter() {
        if let Some(&(entry_name, _, _)) = PROGRAM_ENTRIES.iter()
            .find(|&&(n, _, _)| name_matches_provider(n, p.name()))
        {
            settings.add_section(entry_name);
        }
    }

    // ---- Load user-installed plugins ----------------------------------------
    load_user_plugins(renderer, &mut settings);

    // ---- Load remote service providers from Available programs: config ------
    // Scans for enable_*=true entries that don't match any known program and
    // routes them through RemoteProvider (mirrors C's loadProgram remote branch).
    load_remote_programs(renderer, &mut settings);

    // ---- Sort all registered providers alphabetically ----------------------
    sort_providers_alphabetically(renderer);

    // ---- Register settings as the last provider ----------------------------
    register_provider(renderer, Box::new(settings));

    queue
}

/// Inject the text settings for a built-in program. Mirrors C's
/// `applyManifestSettings` over `BUILTIN_MANIFESTS` (src/sicompass/programs.c:187).
/// Called from both the startup load loop and `enable_provider` so hot-enable
/// registers identical settings to startup-enable.
fn inject_builtin_settings(settings: &mut dyn Provider, name: &str) {
    match name {
        "sales demo" => {
            settings.add_text_setting("sales demo",
                "save folder (product configuration)", "saveFolder", "Downloads");
        }
        "chat client" => {
            settings.add_text_setting("chat client", "homeserver URL",    "chatHomeserver", "https://matrix.org");
            settings.add_text_setting("chat client", "access token",      "chatAccessToken", "");
            settings.add_text_setting("chat client", "username",          "chatUsername",    "");
            settings.add_text_setting("chat client", "password",          "chatPassword",    "");
        }
        "email client" => {
            settings.add_text_setting("email client", "IMAP URL",
                "emailImapUrl", "imaps://imap.gmail.com");
            settings.add_text_setting("email client", "SMTP URL",
                "emailSmtpUrl", "smtps://smtp.gmail.com");
            settings.add_text_setting("email client", "username",              "emailUsername",     "");
            settings.add_text_setting("email client", "password",              "emailPassword",     "");
            settings.add_text_setting("email client", "client ID (OAuth)",     "emailClientId",     "");
            settings.add_text_setting("email client", "client secret (OAuth)", "emailClientSecret", "");
        }
        _ => {}
    }
}

/// Instantiate a built-in provider by display name.
///
/// Factory match keys use display names (e.g. `"chat client"` with a space),
/// not internal names like `"chatclient"`. This same function is used from
/// both `load_programs` and the `Factory` branch of `instantiate_user_plugin`.
fn instantiate_builtin(name: &str) -> Option<Box<dyn Provider>> {
    match name {
        "filebrowser" => Some(Box::new(FilebrowserProvider::new())),
        "tutorial"    => Some(Box::new(TutorialProvider::new(&tutorial_assets_dir()))),
        "web browser" => Some(Box::new(WebbrowserProvider::new())),
        "chat client" => Some(Box::new(ChatClientProvider::new())),
        "email client"=> Some(Box::new(EmailClientProvider::new())),
        "sales demo"  => Some(Box::new(ScriptProvider::new(
            "sales demo", "sales demo", sales_demo_script_path(),
        ).with_supports_config_files(true))),
        _ => None,
    }
}

/// Instantiate a user plugin (Script, Native, or Factory) from its discovered manifest.
fn instantiate_user_plugin(plugin: &DiscoveredPlugin) -> Option<Box<dyn Provider>> {
    let m = &plugin.manifest;
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
fn inject_plugin_settings(settings: &mut SettingsProvider, manifest: &PluginManifest) {
    use crate::plugin_manifest::SettingKind;
    for s in &manifest.settings {
        match s.kind {
            SettingKind::Text => {
                settings.add_text(&manifest.display_name, &s.label, &s.key, &s.default);
            }
            SettingKind::Checkbox => {
                settings.add_checkbox(
                    &manifest.display_name, &s.label, &s.key, s.default_checked,
                );
            }
            SettingKind::Radio => {
                let opts: Vec<&str> = s.options.iter().map(String::as_str).collect();
                settings.add_radio(
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
fn load_user_plugins(renderer: &mut AppRenderer, settings: &mut SettingsProvider) {
    let discovered = discover_user_plugins();

    // Populate the global cache so hot-enable can find manifests later.
    *user_plugin_cache().lock().unwrap() = discovered.clone();

    for plugin in &discovered {
        let m = &plugin.manifest;

        // Add the enable checkbox to "Available programs:" (same as C's registerProgramsSection).
        let config_key = format!("enable_{}", m.name);
        let currently_enabled = is_plugin_enabled_in_config(&m.name);
        settings.add_checkbox("Available programs:", &m.display_name, &config_key, currently_enabled);

        // Skip disabled plugins (mirrors C's isEnabledInConfig check in programsLoad).
        if !currently_enabled {
            continue;
        }

        // Inject per-plugin settings.
        inject_plugin_settings(settings, m);

        // Register a section in settings for this plugin.
        settings.add_section(&m.display_name);

        // Construct and register the provider.
        match instantiate_user_plugin(plugin) {
            Some(p) => register_provider(renderer, p),
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

    // Write back
    if let Ok(json) = serde_json::to_string_pretty(&root) {
        let _ = std::fs::write(path, json);
    }
}

/// Resolve a repo-relative asset path to an absolute one. Tries candidates in order:
///   1. `<CARGO_MANIFEST_DIR>/../../<rel>`  — dev build (works with redirected target-dir,
///      e.g. AppControl-blocked C:\sicompass\target redirect)
///   2. `<exe_dir>/<rel>`                   — release: resources alongside the exe
///   3. `<exe_dir>/../<rel>`                — release: resources one level up
///   4. `<cwd>/<rel>`                       — running from the repo root
///
/// Returns the first existing candidate, or the manifest-anchored path as a last
/// resort so that error messages point somewhere meaningful.
fn resolve_repo_asset(rel: &str) -> PathBuf {
    let from_manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..").join(rel);
    if from_manifest.exists() {
        return from_manifest;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let a = dir.join(rel);
            if a.exists() { return a; }
            let b = dir.join("..").join(rel);
            if b.exists() { return b; }
        }
    }
    let cwd = PathBuf::from(rel);
    if cwd.exists() { return cwd; }
    from_manifest
}

fn tutorial_assets_dir() -> PathBuf {
    resolve_repo_asset("lib/lib_tutorial-rs/assets")
}

fn sales_demo_script_path() -> PathBuf {
    resolve_repo_asset("lib/lib_sales_demo/sales_demo.ts")
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

/// Read `sicompass.fontScale` from settings.json.
/// Returns 1.0 if absent or unparseable. Clamped to [0.5, 2.0].
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
        .map(|f| f.clamp(0.5, 2.0))
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
fn load_remote_programs(renderer: &mut AppRenderer, settings: &mut SettingsProvider) {
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

    for (key, val) in available {
        // Only process enable_*=true keys.
        let name = match key.strip_prefix("enable_") {
            Some(n) => n,
            None => continue,
        };
        if val.as_bool() != Some(true) { continue; }

        // Skip known builtins and already-registered providers.
        if PROGRAM_ENTRIES.iter().any(|&(n, _, _)| n == name) { continue; }
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
            Box::new(RemoteProvider::new(name, remote_url, api_key));
        register_provider(renderer, provider);

        // Register the two settings text entries for this remote service.
        settings.add_text(name, "remote URL", "remoteUrl", "");
        settings.add_text(name, "API key",    "apiKey",    "");
        settings.add_section(name);
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
        let name_owned = name.to_owned();
        insert_provider_alphabetically(
            renderer,
            provider,
            Some(Box::new(move |settings: &mut dyn Provider| {
                inject_builtin_settings(settings, &name_owned);
            })),
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
            insert_provider_alphabetically(renderer, provider, None);
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
            Box::new(RemoteProvider::new(name, remote_url, api_key));
        let section_name = name.to_owned();
        insert_provider_alphabetically(renderer, provider, Some(Box::new(move |settings: &mut dyn Provider| {
            settings.add_text_setting(&section_name, "remote URL", "remoteUrl", "");
            settings.add_text_setting(&section_name, "API key",    "apiKey",    "");
        })));
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

/// Disable and remove a provider by name.
pub fn disable_provider(renderer: &mut AppRenderer, name: &str) {
    let Some(idx) = renderer.providers.iter().position(|p| name_matches_provider(name, p.name())) else {
        return;
    };

    let removed_provider_name = renderer.providers[idx].name().to_owned();
    // Use the PROGRAM_ENTRIES canonical name for section removal when available.
    // For user plugins, fall back to the provider name itself (which equals manifest.display_name).
    let removed_section_name = PROGRAM_ENTRIES.iter()
        .find(|&&(n, _, _)| name_matches_provider(n, &removed_provider_name))
        .map(|&(n, _, _)| n.to_owned())
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
            // During the startup settings drain (skip_enable=true) the window
            // builder flag already handles the initial maximize state, so skip
            // queuing a redundant runtime request.  Only set pending_maximized
            // for live checkbox toggles (skip_enable=false).
            if !skip_enable {
                renderer.pending_maximized = Some(value == "true");
            }
        }
        "saveFolder" => {
            renderer.save_folder_path = value.to_owned();
        }
        "fontScale" => {
            renderer.rebuild_font_renderer = true;
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
    use crate::plugin_manifest::{PluginManifest, PluginType};
    use sicompass_sdk::ffon::FfonElement;
    use sicompass_sdk::provider::Provider;
    use std::io::Write;

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
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("tutorial")), None);
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
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("chat client")), None);
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("email client")), None);
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
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("chat client")), None);
        assert_eq!(r.current_id.get(0), Some(2));
        assert_eq!(r.current_id.get(1), Some(3));
    }

    #[test]
    fn insert_alphabetically_no_adjust_when_after() {
        let mut r = AppRenderer::new();
        register_provider(&mut r, Box::new(MockProv::new("filebrowser")));
        register_provider(&mut r, Box::new(MockProv::new("chat client")));
        register_provider(&mut r, Box::new(MockProv::new("settings")));
        insert_provider_alphabetically(&mut r, Box::new(MockProv::new("tutorial")), None);
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

    // --- enable_provider with user plugin cache ---

    fn make_test_manifest(name: &str) -> PluginManifest {
        PluginManifest {
            name: name.to_owned(),
            display_name: name.to_owned(),
            plugin_type: PluginType::Script,
            entry: "plugin.ts".to_owned(),
            supports_config_files: false,
            settings: vec![],
        }
    }

    fn make_discovered_plugin(name: &str) -> DiscoveredPlugin {
        DiscoveredPlugin {
            manifest: make_test_manifest(name),
            entry_path: PathBuf::from("/nonexistent/plugin.ts"),
        }
    }

    #[test]
    fn enable_provider_unknown_name_logs_and_returns() {
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
        assert_eq!(inputs.len(), 4, "expected 4 text settings, got {}: {:?}", inputs.len(), inputs);
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
}
