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
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PluginType {
    /// Native shared library (`.so` / `.dll` / `.dylib`).  The loader calls
    /// `sicompass_plugin_init` via `libloading`.
    Native,
    /// Script executed through `bun run` — same subcommand protocol as the
    /// built-in TypeScript providers.
    Script,
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
    #[serde(rename = "type")]
    pub plugin_type: PluginType,
    /// Relative entry path (resolved relative to the manifest directory).
    pub entry: String,
    #[serde(default)]
    pub supports_config_files: bool,
    #[serde(default)]
    pub settings: Vec<PluginSetting>,
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
    let Some(plugins_dir) = plugins_dir() else {
        return Vec::new();
    };

    let Ok(entries) = std::fs::read_dir(&plugins_dir) else {
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
// Path helper
// ---------------------------------------------------------------------------

/// Returns `~/.config/sicompass/plugins/` or `None` on unsupported platforms.
pub fn plugins_dir() -> Option<PathBuf> {
    sicompass_sdk::platform::main_config_path()
        .map(|p| p.parent().unwrap_or(&p).join("plugins"))
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
