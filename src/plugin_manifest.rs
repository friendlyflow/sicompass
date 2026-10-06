//! Plugin manifest — parses `plugin.json` and discovers user plugins.
//!
//! User plugins live under `<config>/sicompass/plugins/<name>/plugin.json`.
//! A computer's configuration can provide more, in the folders
//! `SICOMPASS_PLUGIN_PATH` lists ([`sicompass_sdk::platform::system_plugin_dirs`]),
//! which win over the user's copy of the same name and need no approval.
//! Each manifest names the plugin's program (`entry`), what it declares it
//! does (`permissions`), and the settings to inject into the settings
//! provider.

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

// The `plugin.json` types are the SDK's (`sicompass_sdk::plugin_manifest`), shared
// with the `sicompass-plugin` release tool and the Store so the three never
// disagree about what a manifest says.
pub use sicompass_sdk::plugin_manifest::{
    Permissions, PluginManifest, PluginSetting, PluginType, RETIRED_TYPES, Service, SettingKind,
    parse_manifest,
};

/// The top-level `settings.json` key recording what the user approved, per
/// plugin: `{ "<name>": "<approval fingerprint>" }`. Written by the Store when the
/// user grants access; compared on every load, so an update asking for more is
/// held back until approved again.
pub const APPROVALS_KEY: &str = "pluginApprovals";

/// The user's approvals from `settings.json`, by plugin name.
pub fn read_approvals() -> std::collections::HashMap<String, String> {
    let Some(path) = sicompass_sdk::platform::main_config_path() else {
        return Default::default();
    };
    let Ok(data) = std::fs::read_to_string(path) else {
        return Default::default();
    };
    serde_json::from_str::<serde_json::Value>(&data)
        .ok()
        .and_then(|v| v.get(APPROVALS_KEY).cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// Record what the Store just installed or updated: the access the user
/// approved by pressing Install or Update (whatever the manifest asks, so a
/// later update asking for more is noticed).
pub fn record_store_install(m: &PluginManifest) -> Result<(), String> {
    edit_config(|root| {
        let approvals = object_at(root, APPROVALS_KEY);
        approvals.insert(
            m.name.clone(),
            serde_json::Value::String(sicompass_sdk::plugin_abi::approval_fingerprint(m)),
        );
    })
}

/// Forget an uninstalled plugin's approval. Its own settings section is kept,
/// like its data folder: reinstalling finds them again.
pub fn forget_store_install(name: &str) -> Result<(), String> {
    edit_config(|root| {
        object_at(root, APPROVALS_KEY).remove(name);
    })
}

fn object_at<'a>(
    root: &'a mut serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> &'a mut serde_json::Map<String, serde_json::Value> {
    let slot = root
        .entry(key.to_owned())
        .or_insert_with(|| serde_json::Value::Object(Default::default()));
    if !slot.is_object() {
        *slot = serde_json::Value::Object(Default::default());
    }
    slot.as_object_mut().expect("just made an object")
}

/// Read-modify-write `settings.json`, the way the settings provider does: a
/// file that exists but does not parse is left alone (another process may be
/// half-way through writing it), never rebuilt from nothing.
fn edit_config(
    f: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
) -> Result<(), String> {
    let path =
        sicompass_sdk::platform::main_config_path().ok_or("no settings folder on this platform")?;
    let mut root = match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => return Err(format!("{} does not parse, left as it is", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    f(&mut root);
    if let Some(parent) = path.parent() {
        sicompass_sdk::platform::make_dirs(parent);
    }
    let json = serde_json::to_string_pretty(&serde_json::Value::Object(root))
        .map_err(|e| e.to_string())?;
    if sicompass_sdk::platform::atomic_write(&path, &json) {
        Ok(())
    } else {
        Err(format!("{} could not be written", path.display()))
    }
}

/// What a plugin gets, from its manifest and the user's approval.
///
/// A plugin is a program that runs with the user's rights, so it loads only
/// once the user approved exactly this manifest
/// ([`sicompass_sdk::plugin_abi::approval_fingerprint`]): the Store records
/// that when they install it, and an update that declares more asks again.
/// Then it gets its own folder (`storage`, `app_data_dir()/<name>`, where the
/// notes and board built-ins kept their data), its settings, and its service.
///
/// `Err` says why the plugin cannot load.
pub fn grants_for(
    m: &PluginManifest,
    approvals: &std::collections::HashMap<String, String>,
) -> Result<crate::plugin_host::Grants, String> {
    if sicompass_sdk::plugin_abi::needs_approval(m)
        && approvals.get(&m.name) != Some(&sicompass_sdk::plugin_abi::approval_fingerprint(m))
    {
        return Err(
            "it runs as a program on this computer, and you have not approved this \
             version; approve it in the Store"
                .to_owned(),
        );
    }
    approved_grants(m)
}

/// What a plugin gets once it may run: [`grants_for`] without the approval
/// check, for a plugin this computer's configuration provides
/// ([`PluginOrigin::System`]), which nobody is asked about.
pub fn approved_grants(m: &PluginManifest) -> Result<crate::plugin_host::Grants, String> {
    let storage_dir = if m.permissions.storage {
        Some(
            sicompass_sdk::platform::app_data_dir()
                .ok_or("no data directory on this system")?
                .join(&m.name),
        )
    } else {
        None
    };
    Ok(crate::plugin_host::Grants {
        storage_dir,
        settings: m.settings.iter().map(|s| s.key.clone()).collect(),
        service_tier: m.service.as_ref().map(|s| s.tier.clone()),
        renders_pages: m.renders_pages,
        setting_defaults: m
            .settings
            .iter()
            .filter(|s| !s.default.is_empty())
            .map(|s| (s.key.clone(), expand_home(&s.default)))
            .collect(),
    })
}

/// `~` or `~/…` as the user's home folder, in a setting's value or default.
/// Anything else, and `~` on a system without a home, is left as it is.
pub fn expand_home(value: &str) -> String {
    let rest = match value.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => rest,
        _ => return value.to_owned(),
    };
    match sicompass_sdk::platform::home_dir() {
        Some(h) if rest.is_empty() => h.to_string_lossy().into_owned(),
        Some(h) => h.join(rest.trim_start_matches('/')).to_string_lossy().into_owned(),
        None => value.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Manifest loading
// ---------------------------------------------------------------------------

/// Parse a `plugin.json` from disk. Returns `None` on I/O or parse error.
///
/// A manifest naming a retired plugin type is reported rather than skipped in
/// silence. Otherwise a plugin that worked yesterday would simply stop appearing,
/// with nothing anywhere saying why.
pub fn load_manifest(path: &Path) -> Option<PluginManifest> {
    let data = std::fs::read_to_string(path).ok()?;
    match parse_manifest(&data) {
        Ok(manifest) => Some(manifest),
        Err(e) => {
            // `parse_manifest` names a retired plugin type itself.
            eprintln!("sicompass: ignoring {}: {e}", path.display());
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin discovery
// ---------------------------------------------------------------------------

pub use sicompass_sdk::installed_plugins::PluginOrigin;

/// A discovered plugin: the parsed manifest, where it is, and where it came
/// from.
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub manifest: PluginManifest,
    /// Absolute path to the program to start: `entry` in [`dir`](Self::dir).
    pub entry_path: PathBuf,
    /// The plugin's own folder, holding its `plugin.json`, `locales/` and
    /// `assets/`, and its working directory.
    pub dir: PathBuf,
    pub origin: PluginOrigin,
}

/// Every installed plugin: the folders this computer's configuration provides,
/// then `~/.config/sicompass/plugins/`, one per name
/// ([`sicompass_sdk::installed_plugins::discover_all`]). Returns all
/// successfully parsed manifests.
pub fn discover_user_plugins() -> Vec<DiscoveredPlugin> {
    from_discovered(sicompass_sdk::installed_plugins::discover_all())
}

/// [`discover_user_plugins`] for an explicit user plugins directory and no
/// system ones. The scan is the SDK's, shared with the tutorial, which lists
/// the same plugins.
pub fn discover_plugins_in(plugins_dir: &Path) -> Vec<DiscoveredPlugin> {
    from_discovered(sicompass_sdk::installed_plugins::discover_all_in(
        &[],
        Some(plugins_dir),
    ))
}

fn from_discovered(
    found: Vec<sicompass_sdk::installed_plugins::Discovered>,
) -> Vec<DiscoveredPlugin> {
    found
        .into_iter()
        .filter_map(|(dir, origin, manifest)| match manifest {
            Ok(manifest) => Some(DiscoveredPlugin {
                // Resolve entry relative to the manifest's directory.
                entry_path: dir.join(&manifest.entry),
                manifest,
                dir,
                origin,
            }),
            Err(e) => {
                // `parse_manifest` names a retired plugin type itself.
                eprintln!("sicompass: ignoring {}: {e}", dir.join("plugin.json").display());
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_is_expanded_in_setting_defaults() {
        let home = sicompass_sdk::platform::home_dir().expect("a home in the test sandbox");
        let m = parse_manifest(
            r#"{ "name": "texteditor", "displayName": "text editor", "type": "process", "entry": "plugin",
                 "settings": [ { "type": "text", "label": "l", "key": "textEditorPath", "default": "~" },
                               { "type": "text", "label": "m", "key": "other", "default": "~user" },
                               { "type": "text", "label": "n", "key": "none" } ],
                 "permissions": { "filesystem": ["~/Documents", "/"] } }"#,
        )
        .unwrap();
        let g = grants_for(&m, &approved(&m)).unwrap();
        assert_eq!(
            g.setting_defaults,
            vec![
                ("textEditorPath".to_owned(), home.to_string_lossy().into_owned()),
                // Only `~` and `~/…` mean the home: `~user` is left alone.
                ("other".to_owned(), "~user".to_owned()),
            ]
        );
    }

    /// What the Store records when the user installs `m`.
    fn approved(m: &PluginManifest) -> std::collections::HashMap<String, String> {
        std::collections::HashMap::from([(
            m.name.clone(),
            sicompass_sdk::plugin_abi::approval_fingerprint(m),
        )])
    }

    #[test]
    fn grants_carry_the_declared_setting_keys_and_nothing_else() {
        let m = parse_manifest(
            r#"{ "name": "x", "displayName": "x", "type": "process", "entry": "plugin",
                 "settings": [ { "type": "text", "label": "l", "key": "servers" },
                               { "type": "password", "label": "k", "key": "apiKeys" } ] }"#,
        )
        .unwrap();
        let g = grants_for(&m, &approved(&m)).unwrap();
        assert_eq!(g.settings, vec!["servers".to_owned(), "apiKeys".to_owned()]);
    }

    #[test]
    fn permissions_parse_and_both_allowed_hosts_lists_merge() {
        let m: PluginManifest = serde_json::from_str(
            r#"{
                "name": "notes", "displayName": "notes", "type": "process", "entry": "plugin",
                "allowedHosts": ["cloud.example.org"],
                "permissions": {
                    "allowedHosts": ["Cloud.example.org", "api.example.org"],
                    "storage": true
                },
                "description": "notes-description",
                "service": { "tier": "friendlyflow/cloud", "what": "notes-service" }
            }"#,
        )
        .unwrap();
        assert_eq!(
            m.allowed_hosts(),
            vec!["cloud.example.org".to_owned(), "api.example.org".to_owned()]
        );
        assert!(m.permissions.storage);
        assert_eq!(m.description.as_deref(), Some("notes-description"));
        assert_eq!(
            m.service.as_ref().map(|s| s.tier.as_str()),
            Some("friendlyflow/cloud")
        );
    }

    #[test]
    fn nothing_is_declared_by_default_and_no_folder_is_made() {
        let m: PluginManifest = serde_json::from_str(
            r#"{ "name": "x", "displayName": "x", "type": "process", "entry": "plugin" }"#,
        )
        .unwrap();
        assert_eq!(m.permissions, Permissions::default());
        assert!(m.allowed_hosts().is_empty());
        let g = grants_for(&m, &approved(&m)).unwrap();
        assert!(g.storage_dir.is_none() && g.settings.is_empty() && g.service_tier.is_none());
    }

    #[test]
    fn a_plugin_loads_only_once_this_version_was_approved() {
        let m = parse_manifest(
            r#"{ "name": "x", "displayName": "x", "type": "process", "entry": "plugin",
                 "permissions": { "process": ["git"] } }"#,
        )
        .unwrap();
        let e = grants_for(&m, &Default::default()).unwrap_err();
        assert!(e.contains("approve it in the Store"), "{e}");
        assert!(grants_for(&m, &approved(&m)).is_ok());
        // An update declaring more is a new line, so it asks again.
        let more = parse_manifest(
            r#"{ "name": "x", "displayName": "x", "type": "process", "entry": "plugin",
                 "permissions": { "process": ["git", "ssh"] } }"#,
        )
        .unwrap();
        assert!(grants_for(&more, &approved(&m)).is_err());
    }

    use std::io::Write;

    fn write_manifest(dir: &tempfile::TempDir, json: &str) -> PathBuf {
        let path = dir.path().join("plugin.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(json.as_bytes()).unwrap();
        path
    }

    // --- load_manifest ---

    #[test]
    fn a_full_manifest_parses() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{
                "name": "my-plugin",
                "displayName": "my plugin",
                "type": "process",
                "entry": "plugin",
                "supportsConfigFiles": true
            }"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.name, "my-plugin");
        assert_eq!(m.display_name, "my plugin");
        assert_eq!(m.plugin_type, PluginType::Process);
        assert_eq!(m.entry, "plugin");
        assert!(m.supports_config_files);
        assert!(m.settings.is_empty());
        assert!(m.version.is_none());
    }

    #[test]
    fn load_manifest_with_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{
                "name": "versioned",
                "displayName": "Versioned",
                "type": "process",
                "entry": "plugin",
                "version": "1.2.3"
            }"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn a_retired_plugin_type_is_rejected() {
        // `native`, `script` and `wasm` are gone. A manifest naming one must fail
        // to load rather than be quietly reinterpreted as something else, and
        // `load_manifest` logs which type it recognised.
        for kind in ["native", "script", "wasm"] {
            let dir = tempfile::tempdir().unwrap();
            let path = write_manifest(
                &dir,
                &format!(r#"{{"name":"old","displayName":"Old","type":"{kind}","entry":"p.bin"}}"#),
            );
            assert!(load_manifest(&path).is_none(), "`{kind}` should be refused");
        }
    }

    #[test]
    fn load_manifest_with_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{
                "name": "p",
                "displayName": "P",
                "type": "process",
                "entry": "plugin",
                "settings": [
                    {"type": "text",     "label": "Host",    "key": "host",   "default": "localhost"},
                    {"type": "checkbox", "label": "Enabled", "key": "enabled","defaultChecked": true},
                    {"type": "radio",    "label": "Mode",    "key": "mode",   "options": ["a","b"], "default": "a"}
                ]
            }"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.settings.len(), 3);
        assert_eq!(m.settings[0].kind, SettingKind::Text);
        assert_eq!(m.settings[0].default, "localhost");
        assert_eq!(m.settings[1].kind, SettingKind::Checkbox);
        assert!(m.settings[1].default_checked);
        assert_eq!(m.settings[2].kind, SettingKind::Radio);
        assert_eq!(m.settings[2].options, vec!["a", "b"]);
    }

    #[test]
    fn load_manifest_missing_file_returns_none() {
        assert!(load_manifest(Path::new("/nonexistent/plugin.json")).is_none());
    }

    #[test]
    fn load_manifest_invalid_json_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(&dir, "not json at all");
        assert!(load_manifest(&path).is_none());
    }

    #[test]
    fn load_manifest_wrong_type_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{"name":"p","displayName":"P","type":"unknown","entry":"p.so"}"#,
        );
        assert!(load_manifest(&path).is_none());
    }

    #[test]
    fn a_manifest_without_a_type_is_a_wasm_plugin_and_refused() {
        // Until sicompass 0.3 a missing type meant `wasm`, so such a manifest is
        // a WASM plugin, which needs a new release.
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{"name":"plugin","displayName":"Plugin","entry":"plugin.wasm"}"#,
        );
        assert!(load_manifest(&path).is_none());
    }

    #[test]
    fn allowed_hosts_defaults_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{"name":"w","displayName":"W","type":"process","entry":"plugin"}"#,
        );
        let m = load_manifest(&path).unwrap();
        assert!(m.allowed_hosts().is_empty());
    }

    #[test]
    fn allowed_hosts_are_parsed_from_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{
                "name": "weather",
                "displayName": "weather",
                "type": "process",
                "entry": "plugin",
                "allowedHosts": ["api.weather.example", "tiles.weather.example"]
            }"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(
            m.allowed_hosts(),
            vec![
                "api.weather.example".to_owned(),
                "tiles.weather.example".to_owned()
            ]
        );
    }

    #[test]
    fn allowed_hosts_parses_on_a_factory_manifest_but_grants_nothing() {
        // A factory entry names a provider already compiled in, and nothing
        // consults its allowlist. Parsing it anyway keeps the manifest shape
        // uniform.
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{"name":"web browser","displayName":"web browser","type":"factory",
                "entry":"","allowedHosts":["example.com"]}"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.plugin_type, PluginType::Factory);
        assert_eq!(m.allowed_hosts(), vec!["example.com".to_owned()]);
    }

    #[test]
    fn load_manifest_factory_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{"name":"web browser","displayName":"web browser","type":"factory","entry":""}"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.plugin_type, PluginType::Factory);
    }

    // --- discover_user_plugins ---

    #[test]
    fn discover_finds_valid_plugins() {
        let plugins_root = tempfile::tempdir().unwrap();

        let a = plugins_root.path().join("plugin-a");
        std::fs::create_dir(&a).unwrap();
        std::fs::write(
            a.join("plugin.json"),
            r#"{"name":"a","displayName":"A","type":"process","entry":"plugin"}"#,
        )
        .unwrap();

        let b = plugins_root.path().join("plugin-b");
        std::fs::create_dir(&b).unwrap();
        std::fs::write(
            b.join("plugin.json"),
            r#"{"name":"b","displayName":"B","type":"process","entry":"bin/b"}"#,
        )
        .unwrap();

        // A WASM plugin from before 0.3, which omitted the type: skipped, with a
        // logged reason
        let e = plugins_root.path().join("plugin-wasm");
        std::fs::create_dir(&e).unwrap();
        std::fs::write(
            e.join("plugin.json"),
            r#"{"name":"e","displayName":"E","entry":"plugin.wasm"}"#,
        )
        .unwrap();

        // Subdirectory with no plugin.json — should be skipped
        let c = plugins_root.path().join("not-a-plugin");
        std::fs::create_dir(&c).unwrap();

        // A plugin still declaring a retired type — skipped, with a logged reason
        let d = plugins_root.path().join("plugin-legacy");
        std::fs::create_dir(&d).unwrap();
        std::fs::write(
            d.join("plugin.json"),
            r#"{"name":"legacy","displayName":"Legacy","type":"native","entry":"d.so"}"#,
        )
        .unwrap();

        let mut found = discover_plugins_in(plugins_root.path());
        found.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));

        assert_eq!(
            found.len(),
            2,
            "found {:?}",
            found.iter().map(|p| &p.manifest.name).collect::<Vec<_>>()
        );
        assert_eq!(found[0].manifest.name, "a");
        assert_eq!(found[1].manifest.name, "b");
        assert_eq!(found[0].entry_path, a.join("plugin"));
        assert_eq!(found[1].entry_path, b.join("bin/b"));
        assert_eq!(found[1].manifest.plugin_type, PluginType::Process);
    }

    #[test]
    fn discover_resolves_the_entry_next_to_its_manifest() {
        // The resolved `entry_path` is what the app starts, and its parent is the
        // plugin's directory (its assets, its locales, its working directory), so
        // both must land inside the plugin's own directory.
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("weather");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"name":"weather","displayName":"Weather","type":"process",
                "entry":"plugin","allowedHosts":["api.weather.example"]}"#,
        )
        .unwrap();

        let found = discover_plugins_in(root.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.plugin_type, PluginType::Process);
        assert_eq!(found[0].entry_path, dir.join("plugin"));
        assert_eq!(found[0].entry_path.parent().unwrap(), dir);
        assert_eq!(
            found[0].manifest.allowed_hosts(),
            vec!["api.weather.example".to_owned()]
        );
    }

    #[test]
    fn a_system_plugin_wins_and_its_folder_is_the_manifests_even_for_a_nested_entry() {
        let system = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        for root in [system.path(), user.path()] {
            let dir = root.join("b");
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(
                dir.join("plugin.json"),
                r#"{"name":"b","displayName":"B","type":"process","entry":"bin/b"}"#,
            )
            .unwrap();
        }

        let found = from_discovered(sicompass_sdk::installed_plugins::discover_all_in(
            &[system.path().to_owned()],
            Some(user.path()),
        ));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].origin, PluginOrigin::System);
        // Not `entry_path.parent()`, which is `b/bin`.
        assert_eq!(found[0].dir, system.path().join("b"));
        assert_eq!(found[0].entry_path, system.path().join("b/bin/b"));
    }

    #[test]
    fn discover_empty_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(discover_plugins_in(dir.path()).is_empty());
    }

    #[test]
    fn discover_nonexistent_dir_returns_empty() {
        assert!(discover_plugins_in(Path::new("/no/such/dir")).is_empty());
    }
}
