//! Plugin manifest — parses `plugin.json` and discovers user plugins.
//!
//! User plugins live under `~/.config/sicompass/plugins/<name>/plugin.json`.
//! Each manifest describes the plugin type (native `.so` or script), entry
//! point path, optional `supportsConfigFiles`, and optional extra settings
//! to inject into the settings provider.
//!
//! Equivalent to the `PluginManifest` / `discoverUserPlugins` logic in
//! `src/sicompass/programs.c`.

use serde::Deserialize;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How the plugin is executed.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PluginType {
    /// Sandboxed WebAssembly component. The only kind of third-party plugin that
    /// can ship on Apple's stores, and the only one whose manifest policy is
    /// actually enforceable — see [`crate::wasm_host`].
    Wasm,
    /// Native shared library (`.so` / `.dll` / `.dylib`).  The loader calls
    /// `sicompass_plugin_init` via `libloading`.
    ///
    /// Deprecated: being replaced by [`PluginType::Wasm`]. Apple forbids executing
    /// downloaded native code, and an in-process plugin has full process
    /// privileges, so `allowedHosts` and friends are advisory against it.
    Native,
    /// Script executed through `bun run` — same subcommand protocol as the
    /// built-in TypeScript providers.
    ///
    /// Deprecated alongside [`PluginType::Native`]: shipping and spawning a
    /// general-purpose interpreter is equally incompatible with a store sandbox,
    /// and `bun` is not bundled in release archives at all.
    #[default]
    Script,
    /// Instantiate a built-in factory provider by the manifest's `name` field.
    /// Mirrors C's `PLUGIN_FACTORY` in `src/sicompass/programs.c`.
    Factory,
}

/// Kind of a per-plugin setting entry.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SettingKind {
    Text,
    Checkbox,
    Radio,
}

/// A single setting declared by a plugin manifest.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSetting {
    #[serde(rename = "type")]
    pub kind: SettingKind,
    pub label: String,
    pub key: String,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub default_checked: bool,
    #[serde(default)]
    pub options: Vec<String>,
}

/// Parsed contents of a `plugin.json` manifest file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub name: String,
    pub display_name: String,
    #[serde(rename = "type", default)]
    pub plugin_type: PluginType,
    /// Relative entry path (resolved relative to the manifest directory).
    pub entry: String,
    #[serde(default)]
    pub supports_config_files: bool,
    #[serde(default)]
    pub settings: Vec<PluginSetting>,
    /// Optional plugin version, displayed in the settings tree under the
    /// plugin's section. Authors set this in `plugin.json`.
    #[serde(default)]
    pub version: Option<String>,
    /// HTTPS URL the updater queries for a newer manifest. Absent =>
    /// plugin opts out of auto-update.
    #[serde(default)]
    pub update_url: Option<String>,
    /// Minimum sicompass app version this plugin works against. If the
    /// running app is older, the plugin is skipped at load time.
    #[serde(default)]
    pub min_app_version: Option<String>,
    /// Base64-encoded ed25519 public key. Trust root for verifying
    /// signatures on future updates. First-install is trust-on-first-use.
    #[serde(default)]
    pub pubkey: Option<String>,
    /// Whether the running provider can be torn down + re-instantiated
    /// mid-session after an update lands on disk. Defaults to `true`;
    /// plugins that spawn long-lived threads holding fn-pointers from
    /// their own library must declare `false` and require a restart.
    #[serde(default = "default_hot_reload")]
    pub hot_reload: bool,
    /// Hosts a `wasm` plugin may reach over the network.
    ///
    /// This is a capability declaration, not a hint. Absent or empty means the
    /// network interface is **not linked into the guest at all**, so the plugin has
    /// no reachable network function rather than a blocked one. A component that
    /// uses the network without declaring hosts here is refused before it is
    /// instantiated.
    ///
    /// Matching is exact and case-insensitive: subdomains must be listed
    /// individually, because `evil.example.com` is not `example.com`. Listing them
    /// here is also what shows the user, before they enable the plugin, where it
    /// intends to connect.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}

fn default_hot_reload() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Manifest loading
// ---------------------------------------------------------------------------

/// Parse a `plugin.json` from disk.  Returns `None` on I/O or parse error.
pub fn load_manifest(path: &Path) -> Option<PluginManifest> {
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
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
            found.push(DiscoveredPlugin { manifest, entry_path });
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
    use std::io::Write;

    fn write_manifest(dir: &tempfile::TempDir, json: &str) -> PathBuf {
        let path = dir.path().join("plugin.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(json.as_bytes()).unwrap();
        path
    }

    // --- load_manifest ---

    #[test]
    fn load_native_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{
                "name": "my-c-plugin",
                "displayName": "my C plugin",
                "type": "native",
                "entry": "plugin.so"
            }"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.name, "my-c-plugin");
        assert_eq!(m.display_name, "my C plugin");
        assert_eq!(m.plugin_type, PluginType::Native);
        assert_eq!(m.entry, "plugin.so");
        assert!(!m.supports_config_files);
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
                "type": "script",
                "entry": "v.ts",
                "version": "1.2.3"
            }"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn load_script_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{
                "name": "my-ts-plugin",
                "displayName": "my TS plugin",
                "type": "script",
                "entry": "plugin.ts",
                "supportsConfigFiles": true
            }"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.plugin_type, PluginType::Script);
        assert!(m.supports_config_files);
    }

    #[test]
    fn load_manifest_with_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{
                "name": "p",
                "displayName": "P",
                "type": "native",
                "entry": "p.so",
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
    fn load_manifest_missing_type_defaults_to_script() {
        // Matches the C behavior: absent "type" field → PLUGIN_SCRIPT.
        // Also ensures sdk/examples/typescript/plugin.json (no type) loads correctly.
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{"name":"ts-plugin","displayName":"TS Plugin","entry":"plugin.ts"}"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.plugin_type, PluginType::Script);
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
        assert!(m.allowed_hosts.is_empty());
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
            m.allowed_hosts,
            vec!["api.weather.example".to_owned(), "tiles.weather.example".to_owned()]
        );
    }

    #[test]
    fn allowed_hosts_is_ignored_for_non_wasm_types() {
        // It parses on any manifest, but only the wasm loader consults it. A native
        // plugin could never have been constrained by it anyway — it can open its
        // own socket — which is precisely why native is going away.
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(
            &dir,
            r#"{"name":"n","displayName":"N","type":"native","entry":"n.so",
                "allowedHosts":["example.com"]}"#,
        );
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.plugin_type, PluginType::Native);
        assert_eq!(m.allowed_hosts, vec!["example.com".to_owned()]);
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

        // Plugin A
        let a = plugins_root.path().join("plugin-a");
        std::fs::create_dir(&a).unwrap();
        std::fs::write(
            a.join("plugin.json"),
            r#"{"name":"a","displayName":"A","type":"script","entry":"a.ts"}"#,
        )
        .unwrap();

        // Plugin B
        let b = plugins_root.path().join("plugin-b");
        std::fs::create_dir(&b).unwrap();
        std::fs::write(
            b.join("plugin.json"),
            r#"{"name":"b","displayName":"B","type":"native","entry":"b.so"}"#,
        )
        .unwrap();

        // Subdirectory with no plugin.json — should be skipped
        let c = plugins_root.path().join("not-a-plugin");
        std::fs::create_dir(&c).unwrap();

        let mut found = discover_plugins_in(plugins_root.path());
        found.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].manifest.name, "a");
        assert_eq!(found[1].manifest.name, "b");
        assert_eq!(found[0].entry_path, a.join("a.ts"));
        assert_eq!(found[1].entry_path, b.join("b.so"));
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
        assert_eq!(found[0].manifest.allowed_hosts, vec!["api.weather.example".to_owned()]);
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
            found.push(DiscoveredPlugin { manifest, entry_path });
        }
    }
    found
}
