//! Builtin provider manifest types and registry.
//!
//! Libs call [`register_builtin_manifest`] from their `register()` function to
//! declare their display name, default-enabled state, and setting declarations.
//! The app calls [`builtin_manifests`] at startup to drive the "Available programs:"
//! section and setting injection without having direct knowledge of any lib crate.

use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Setting declarations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum SettingKind {
    Text,
    Checkbox,
    Radio,
}

/// Declares one setting entry that a lib wants injected into the settings panel.
#[derive(Debug, Clone)]
pub struct SettingDecl {
    pub kind: SettingKind,
    pub section: String,
    pub label: String,
    pub key: String,
    /// Default value string (used for Text and Radio; empty for Checkbox).
    pub default: String,
    pub default_checked: bool,
    /// Option list (Radio only).
    pub options: Vec<String>,
}

impl SettingDecl {
    pub fn text(section: &str, label: &str, key: &str, default: &str) -> Self {
        SettingDecl {
            kind: SettingKind::Text,
            section: section.to_owned(),
            label: label.to_owned(),
            key: key.to_owned(),
            default: default.to_owned(),
            default_checked: false,
            options: vec![],
        }
    }

    pub fn checkbox(section: &str, label: &str, key: &str, default_checked: bool) -> Self {
        SettingDecl {
            kind: SettingKind::Checkbox,
            section: section.to_owned(),
            label: label.to_owned(),
            key: key.to_owned(),
            default: String::new(),
            default_checked,
            options: vec![],
        }
    }

    pub fn radio(section: &str, label: &str, key: &str, options: &[&str], default: &str) -> Self {
        SettingDecl {
            kind: SettingKind::Radio,
            section: section.to_owned(),
            label: label.to_owned(),
            key: key.to_owned(),
            default: default.to_owned(),
            default_checked: false,
            options: options.iter().map(|s| s.to_string()).collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// BuiltinManifest
// ---------------------------------------------------------------------------

/// Describes a built-in provider.
///
/// Libs construct and register one of these per provider they ship.
/// The app iterates [`builtin_manifests`] at startup to wire up the
/// "Available programs:" section and provider-specific settings — without
/// needing a direct dependency on any lib crate.
#[derive(Debug, Clone)]
pub struct BuiltinManifest {
    /// Internal name (matches `Provider::name()` and the factory key).
    pub name: String,
    /// Display name shown in the UI and settings section header.
    pub display_name: String,
    /// Whether the provider is on by default in `Available programs:`.
    pub enable_default: bool,
    /// If true the provider is registered unconditionally and not listed in
    /// "Available programs:" (e.g. the file browser).
    pub always_enabled: bool,
    pub settings: Vec<SettingDecl>,
}

impl BuiltinManifest {
    pub fn new(name: &str, display_name: &str) -> Self {
        BuiltinManifest {
            name: name.to_owned(),
            display_name: display_name.to_owned(),
            enable_default: false,
            always_enabled: false,
            settings: vec![],
        }
    }

    pub fn enable_by_default(mut self) -> Self {
        self.enable_default = true;
        self
    }

    pub fn always_enabled(mut self) -> Self {
        self.always_enabled = true;
        self
    }

    pub fn with_settings(mut self, settings: Vec<SettingDecl>) -> Self {
        self.settings = settings;
        self
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

static BUILTIN_MANIFESTS: OnceLock<Mutex<Vec<BuiltinManifest>>> = OnceLock::new();

fn manifest_registry() -> &'static Mutex<Vec<BuiltinManifest>> {
    BUILTIN_MANIFESTS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Register a builtin manifest. Called once per provider from each lib's `register()`.
pub fn register_builtin_manifest(manifest: BuiltinManifest) {
    manifest_registry().lock().unwrap().push(manifest);
}

/// Return a snapshot of all registered builtin manifests.
pub fn builtin_manifests() -> Vec<BuiltinManifest> {
    manifest_registry().lock().unwrap().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setting_decl_text_constructor() {
        let d = SettingDecl::text("section", "label", "key", "default");
        assert_eq!(d.kind, SettingKind::Text);
        assert_eq!(d.section, "section");
        assert_eq!(d.default, "default");
        assert!(d.options.is_empty());
    }

    #[test]
    fn setting_decl_checkbox_constructor() {
        let d = SettingDecl::checkbox("s", "l", "k", true);
        assert_eq!(d.kind, SettingKind::Checkbox);
        assert!(d.default_checked);
        assert!(d.default.is_empty());
    }

    #[test]
    fn setting_decl_radio_constructor() {
        let d = SettingDecl::radio("s", "l", "k", &["a", "b"], "a");
        assert_eq!(d.kind, SettingKind::Radio);
        assert_eq!(d.options, vec!["a", "b"]);
    }

    #[test]
    fn builtin_manifest_defaults() {
        let m = BuiltinManifest::new("test", "Test");
        assert!(!m.enable_default);
        assert!(!m.always_enabled);
        assert!(m.settings.is_empty());
    }

    #[test]
    fn builtin_manifest_builder() {
        let m = BuiltinManifest::new("fb", "File Browser")
            .always_enabled()
            .enable_by_default();
        assert!(m.always_enabled);
        assert!(m.enable_default);
    }
}
