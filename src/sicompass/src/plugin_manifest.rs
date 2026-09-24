//! Plugin manifest — parses `plugin.json` and discovers user plugins.
//!
//! User plugins live under `~/.config/sicompass/plugins/<name>/plugin.json`.
//! Each manifest describes the plugin type (native `.so` or script), entry
//! point path, optional `supportsConfigFiles`, and optional extra settings
//! to inject into the settings provider.
//!
//! Equivalent to the `PluginManifest` / `discoverUserPlugins` logic in
//! `src/sicompass/programs.c`.

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
/// later update asking for more is noticed), and, for an install, the program
/// enabled in "Available programs:".
pub fn record_store_install(m: &PluginManifest, enable: bool) -> Result<(), String> {
    edit_config(|root| {
        let approvals = object_at(root, APPROVALS_KEY);
        approvals.insert(
            m.name.clone(),
            serde_json::Value::String(sicompass_sdk::plugin_abi::approval_fingerprint(m)),
        );
        if enable {
            object_at(root, "Available programs:")
                .insert(format!("enable_{}", m.name), serde_json::Value::Bool(true));
        }
    })
}

/// Forget an uninstalled plugin's approval and enable switch. Its own settings
/// section is kept, like its data folder: reinstalling finds them again.
pub fn forget_store_install(name: &str) -> Result<(), String> {
    edit_config(|root| {
        object_at(root, APPROVALS_KEY).remove(name);
        object_at(root, "Available programs:").remove(&format!("enable_{name}"));
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

/// What a plugin gets, from its manifest and the user's approvals.
///
/// - `allowedHosts`: as declared (shown when the plugin is enabled).
/// - `storage`: its own folder, `app_data_dir()/<name>`. No approval needed, it
///   reaches nothing else. The notes and board plugins find their existing data
///   there, since that is where the built-ins keep it.
/// - `filesystem`: only if the user approved exactly this manifest's access
///   ([`sicompass_sdk::plugin_abi::approval_fingerprint`]); `~` means home.
/// - `process`: the listed programs, under the same approval.
/// - `sockets`: TCP to the listed `host:port` pairs, under the same approval.
///
/// `Err` says why the plugin cannot load: a permission this build cannot grant,
/// or access the user has not approved.
pub fn grants_for(
    m: &PluginManifest,
    approvals: &std::collections::HashMap<String, String>,
) -> Result<crate::wasm_host::Grants, String> {
    if sicompass_sdk::plugin_abi::needs_approval(m)
        && approvals.get(&m.name) != Some(&sicompass_sdk::plugin_abi::approval_fingerprint(m))
    {
        let asked: Vec<String> = m
            .permissions
            .filesystem
            .iter()
            .map(|f| format!("folder {f}"))
            .chain(m.permissions.process.iter().map(|p| format!("program {p}")))
            .chain(
                m.permissions
                    .sockets
                    .iter()
                    .map(|s| format!("connection to {s}")),
            )
            .chain(
                sicompass_sdk::plugin_abi::reaches_any_server(&m.allowed_hosts())
                    .then(|| "any server on the internet".to_owned()),
            )
            .collect();
        return Err(format!(
            "it asks for access you have not approved ({}); approve it in the Store",
            asked.join(", ")
        ));
    }
    let storage_dir = if m.permissions.storage {
        Some(
            sicompass_sdk::platform::app_data_dir()
                .ok_or("no data directory on this system")?
                .join(&m.name),
        )
    } else {
        None
    };
    let home = sicompass_sdk::platform::home_dir();
    let filesystem = m
        .permissions
        .filesystem
        .iter()
        .map(|p| match (p.strip_prefix('~'), &home) {
            (Some(rest), Some(h)) => h.join(rest.trim_start_matches('/')),
            _ => PathBuf::from(p),
        })
        .collect();
    Ok(crate::wasm_host::Grants {
        allowed_hosts: m.allowed_hosts(),
        storage_dir,
        filesystem,
        process: m.permissions.process.clone(),
        sockets: m.permissions.sockets.clone(),
        settings: m.settings.iter().map(|s| s.key.clone()).collect(),
        service_tier: m.service.as_ref().map(|s| s.tier.clone()),
    })
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

/// A discovered plugin: the parsed manifest plus the resolved entry path.
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub manifest: PluginManifest,
    /// Absolute path to the entry point (`.so` or `.ts`/`.js` script).
    pub entry_path: PathBuf,
}

/// Scan `~/.config/sicompass/plugins/` for subdirectories containing a
/// `plugin.json`.  Returns all successfully parsed manifests.
///
/// Mirrors `discoverUserPlugins()` in `src/sicompass/programs.c`.
pub fn discover_user_plugins() -> Vec<DiscoveredPlugin> {
    let Some(dir) = sicompass_sdk::platform::plugins_dir() else {
        return Vec::new();
    };

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut found = Vec::new();
    for entry in entries.flatten() {
        let manifest_path = entry.path().join("plugin.json");
        if let Some(manifest) = load_manifest(&manifest_path) {
            // Resolve entry relative to the manifest's directory.
            let entry_path = entry.path().join(&manifest.entry);
            found.push(DiscoveredPlugin {
                manifest,
                entry_path,
            });
        }
    }
    found
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_carry_the_declared_setting_keys_and_nothing_else() {
        let m = parse_manifest(
            r#"{ "name": "x", "displayName": "x", "entry": "plugin.wasm",
                 "settings": [ { "type": "text", "label": "l", "key": "servers" },
                               { "type": "password", "label": "k", "key": "apiKeys" } ] }"#,
        )
        .unwrap();
        let g = grants_for(&m, &Default::default()).unwrap();
        assert_eq!(g.settings, vec!["servers".to_owned(), "apiKeys".to_owned()]);
    }

    #[test]
    fn permissions_parse_and_both_allowed_hosts_lists_merge() {
        let m: PluginManifest = serde_json::from_str(
            r#"{
                "name": "notes", "displayName": "notes", "entry": "plugin.wasm",
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
    fn nothing_is_granted_by_default() {
        let m: PluginManifest =
            serde_json::from_str(r#"{ "name": "x", "displayName": "x", "entry": "plugin.wasm" }"#)
                .unwrap();
        assert_eq!(m.permissions, Permissions::default());
        assert!(m.allowed_hosts().is_empty());
        // And the host grants it nothing: no hosts, folders, programs, sockets.
        let g = grants_for(&m, &Default::default()).unwrap();
        assert!(g.allowed_hosts.is_empty() && g.storage_dir.is_none());
        assert!(g.filesystem.is_empty() && g.process.is_empty() && g.sockets.is_empty());
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
                "type": "wasm",
                "entry": "plugin.wasm",
                "supportsConfigFiles": true
            }"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.name, "my-plugin");
        assert_eq!(m.display_name, "my plugin");
        assert_eq!(m.plugin_type, PluginType::Wasm);
        assert_eq!(m.entry, "plugin.wasm");
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
                "type": "wasm",
                "entry": "v.wasm",
                "version": "1.2.3"
            }"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn a_retired_plugin_type_is_rejected() {
        // `native` and `script` are gone. A manifest naming one must fail to load
        // rather than be quietly reinterpreted as something else — silently
        // treating a `.so` plugin as WASM would be far more confusing than a
        // refusal, and `load_manifest` logs which type it recognised.
        for kind in ["native", "script"] {
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
                "type": "wasm",
                "entry": "p.wasm",
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
    fn load_manifest_missing_type_defaults_to_wasm() {
        // The safe default: a manifest that says nothing gets the sandbox rather
        // than having to ask for it.
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{"name":"plugin","displayName":"Plugin","entry":"plugin.wasm"}"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.plugin_type, PluginType::Wasm);
    }

    // --- wasm ---

    #[test]
    fn load_wasm_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{
                "name": "weather",
                "displayName": "weather",
                "type": "wasm",
                "entry": "plugin.wasm"
            }"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.plugin_type, PluginType::Wasm);
        assert_eq!(m.entry, "plugin.wasm");
    }

    #[test]
    fn allowed_hosts_defaults_to_empty_which_means_no_network_at_all() {
        // Absent is the safe default and must stay that way: an empty list is what
        // makes the host leave the network interface unlinked, so a plugin that
        // forgets to declare hosts gets no network rather than unrestricted access.
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{"name":"w","displayName":"W","type":"wasm","entry":"p.wasm"}"#,
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
                "type": "wasm",
                "entry": "plugin.wasm",
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
        // A factory entry names a provider already compiled in, so it is not
        // sandboxed and nothing consults its allowlist. Parsing it anyway keeps the
        // manifest shape uniform; only the wasm loader reads the field.
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

        // Plugin A — explicit type
        let a = plugins_root.path().join("plugin-a");
        std::fs::create_dir(&a).unwrap();
        std::fs::write(
            a.join("plugin.json"),
            r#"{"name":"a","displayName":"A","type":"wasm","entry":"a.wasm"}"#,
        )
        .unwrap();

        // Plugin B — omits the type, so it defaults to wasm
        let b = plugins_root.path().join("plugin-b");
        std::fs::create_dir(&b).unwrap();
        std::fs::write(
            b.join("plugin.json"),
            r#"{"name":"b","displayName":"B","entry":"b.wasm"}"#,
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
        assert_eq!(found[0].entry_path, a.join("a.wasm"));
        assert_eq!(found[1].entry_path, b.join("b.wasm"));
        assert_eq!(
            found[1].manifest.plugin_type,
            PluginType::Wasm,
            "absent type defaults to wasm"
        );
    }

    #[test]
    fn discover_resolves_a_wasm_entry_next_to_its_manifest() {
        // The resolved `entry_path` is what the loader opens, and its parent is the
        // confinement root for any path the guest hands back, so both must land
        // inside the plugin's own directory.
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("weather");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"name":"weather","displayName":"Weather","type":"wasm",
                "entry":"plugin.wasm","allowedHosts":["api.weather.example"]}"#,
        )
        .unwrap();

        let found = discover_plugins_in(root.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.plugin_type, PluginType::Wasm);
        assert_eq!(found[0].entry_path, dir.join("plugin.wasm"));
        assert_eq!(found[0].entry_path.parent().unwrap(), dir);
        assert_eq!(
            found[0].manifest.allowed_hosts(),
            vec!["api.weather.example".to_owned()]
        );
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

// Testable variant that accepts an explicit plugins directory.
#[cfg(test)]
pub fn discover_plugins_in(plugins_dir: &Path) -> Vec<DiscoveredPlugin> {
    let Ok(entries) = std::fs::read_dir(plugins_dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let manifest_path = entry.path().join("plugin.json");
        if let Some(manifest) = load_manifest(&manifest_path) {
            let entry_path = entry.path().join(&manifest.entry);
            found.push(DiscoveredPlugin {
                manifest,
                entry_path,
            });
        }
    }
    found
}
