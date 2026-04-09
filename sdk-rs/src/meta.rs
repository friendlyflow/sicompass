/// Centralised meta/shortcut-hint registry.
///
/// Every keyboard shortcut hint shown when the user presses `M` is registered
/// here. Built-in providers are populated at first access via [`populate_builtins`].
/// Script plugins register dynamically at load time.
///
/// Lookup is by provider name string (same value returned by `Provider::name()`).
/// Special sentinels:
/// - [`ROOT`] — hints shown at depth ≤ 1 (root navigation level)
/// - [`EMAIL_DEFAULT`], [`EMAIL_COMPOSE`], [`EMAIL_COMPOSE_BODY`] — email contexts
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single keyboard-shortcut hint entry.
///
/// `label` is the full formatted display string, e.g. `"Ctrl+I  Insert before"`.
#[derive(Debug, Clone, PartialEq)]
pub struct MetaEntry {
    pub label: String,
}

impl MetaEntry {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into() }
    }
}

// ---------------------------------------------------------------------------
// Sentinel keys
// ---------------------------------------------------------------------------

/// Hints shown at root navigation depth (depth ≤ 1).
pub const ROOT: &str = "__root__";

/// Email client — browsing folders / message list.
pub const EMAIL_DEFAULT: &str = "emailclient";
/// Email client — inside a compose/reply/forward view (header fields).
pub const EMAIL_COMPOSE: &str = "emailclient:compose";
/// Email client — inside the body editor of a compose view.
pub const EMAIL_COMPOSE_BODY: &str = "emailclient:compose:body";

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

fn registry() -> &'static RwLock<HashMap<String, Vec<MetaEntry>>> {
    static REG: OnceLock<RwLock<HashMap<String, Vec<MetaEntry>>>> = OnceLock::new();
    REG.get_or_init(|| {
        let mut map = HashMap::new();
        populate_builtins(&mut map);
        RwLock::new(map)
    })
}

/// Register (or replace) entries for a provider name or sentinel.
///
/// Calling this multiple times with the same `name` replaces the previous entries.
/// Script plugins call this each time they supply a fresh `meta` array.
pub fn register(name: impl Into<String>, entries: Vec<MetaEntry>) {
    if let Ok(mut map) = registry().write() {
        map.insert(name.into(), entries);
    }
}

/// Remove entries for a name. Used when a script plugin unloads.
pub fn unregister(name: &str) {
    if let Ok(mut map) = registry().write() {
        map.remove(name);
    }
}

/// Return the registered entries for `name`, or `None` if not found.
pub fn lookup(name: &str) -> Option<Vec<MetaEntry>> {
    registry().read().ok()?.get(name).cloned()
}

/// Return formatted label strings for `name`, or `None` if not found.
///
/// This is the form consumed by the rest of the app (`Vec<String>`).
pub fn lookup_formatted(name: &str) -> Option<Vec<String>> {
    lookup(name).map(|entries| entries.into_iter().map(|e| e.label).collect())
}

/// Like [`lookup_formatted`] but applies path-based context for providers
/// whose hints vary by navigation depth (currently: `emailclient`).
///
/// This is the function the `Provider::meta()` default calls so that context-
/// sensitive providers need no override at all.
pub fn lookup_with_context(name: &str, path: &str) -> Option<Vec<String>> {
    if name == "emailclient" {
        let segs: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let at_compose = segs.first().is_some_and(|s| {
            matches!(*s, "compose" | "reply" | "reply all" | "forward")
        });
        let in_body = segs.iter().any(|s| s.starts_with("Body:"));
        let key = match (at_compose, in_body) {
            (false, _)    => EMAIL_DEFAULT,
            (true, false) => EMAIL_COMPOSE,
            (true, true)  => EMAIL_COMPOSE_BODY,
        };
        return lookup_formatted(key);
    }
    lookup_formatted(name)
}

// ---------------------------------------------------------------------------
// Built-in content
// ---------------------------------------------------------------------------

/// Populate `map` with all hard-coded built-in meta entries.
///
/// This is the **single source of truth** for every built-in shortcut
/// description in the Rust port. To change a key label, edit this function.
fn populate_builtins(map: &mut HashMap<String, Vec<MetaEntry>>) {
    // Root navigation (depth ≤ 1)
    map.insert(ROOT.to_owned(), vec![
        MetaEntry::new("Tab     Search providers"),
        MetaEntry::new("Ctrl+F  Extended search"),
        MetaEntry::new("D       Dashboard"),
        MetaEntry::new("Space   Collapse/expand"),
    ]);

    // File browser
    map.insert("filebrowser".to_owned(), vec![
        MetaEntry::new("Ctrl+I  Insert before"),
        MetaEntry::new("Ctrl+A  Append after"),
        MetaEntry::new("Del     Delete"),
        MetaEntry::new("Ctrl+X  Cut"),
        MetaEntry::new("Ctrl+C  Copy"),
        MetaEntry::new("Ctrl+V  Paste"),
        MetaEntry::new("I       Rename"),
        MetaEntry::new(":       Commands"),
        MetaEntry::new("/       Search"),
        MetaEntry::new("Ctrl+F  Extended search"),
        MetaEntry::new("F5      Refresh"),
    ]);

    // Settings
    map.insert("settings".to_owned(), vec![
        MetaEntry::new("/   Search"),
        MetaEntry::new("Ctrl+F  Extended search"),
        MetaEntry::new("F5  Refresh"),
    ]);

    // Web browser
    map.insert("webbrowser".to_owned(), vec![
        MetaEntry::new("I   Edit URL"),
        MetaEntry::new("/   Search"),
        MetaEntry::new("Ctrl+F  Extended search"),
        MetaEntry::new("F5  Refresh"),
        MetaEntry::new(":   Commands"),
    ]);

    // Tutorial (read-only browser)
    map.insert("tutorial".to_owned(), vec![
        MetaEntry::new("/       Search"),
        MetaEntry::new("Ctrl+F  Extended search"),
    ]);

    // Sales demo (script provider)
    map.insert("sales demo".to_owned(), vec![
        MetaEntry::new("/   Search"),
        MetaEntry::new("F5  Refresh"),
        MetaEntry::new(":   Commands"),
    ]);

    // Chat client
    map.insert("chatclient".to_owned(), vec![
        MetaEntry::new("/       Search"),
        MetaEntry::new("Ctrl+F  Extended search"),
        MetaEntry::new("F5      Refresh"),
        MetaEntry::new(":       Commands"),
    ]);

    // Email client — browsing
    map.insert(EMAIL_DEFAULT.to_owned(), vec![
        MetaEntry::new("/       Search"),
        MetaEntry::new("F5      Refresh"),
        MetaEntry::new(":       Commands"),
    ]);

    // Email client — compose/reply header fields
    map.insert(EMAIL_COMPOSE.to_owned(), vec![
        MetaEntry::new("Tab     Next field"),
    ]);

    // Email client — body editor
    map.insert(EMAIL_COMPOSE_BODY.to_owned(), vec![
        MetaEntry::new("Tab     Next field"),
        MetaEntry::new("Ctrl+I  Insert before"),
        MetaEntry::new("Ctrl+A  Append after"),
        MetaEntry::new("D       Delete"),
    ]);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_root_contains_tab_and_ctrl_f() {
        let entries = lookup_formatted(ROOT).expect("ROOT hints should be registered");
        assert!(entries.iter().any(|s| s.contains("Tab")));
        assert!(entries.iter().any(|s| s.contains("Ctrl+F")));
    }

    #[test]
    fn builtins_filebrowser_has_11_entries() {
        let entries = lookup(ROOT).unwrap();
        assert!(!entries.is_empty());
        let fb = lookup("filebrowser").expect("filebrowser should be registered");
        assert_eq!(fb.len(), 11);
        assert!(fb[0].label.contains("Ctrl+I"));
    }

    #[test]
    fn builtins_email_contexts_registered() {
        assert!(lookup(EMAIL_DEFAULT).is_some());
        assert!(lookup(EMAIL_COMPOSE).is_some());
        assert!(lookup(EMAIL_COMPOSE_BODY).is_some());
    }

    #[test]
    fn register_and_lookup_roundtrip() {
        let name = "__test_provider_abc__";
        register(name, vec![MetaEntry::new("X   Do thing")]);
        let result = lookup_formatted(name).expect("should find registered entry");
        assert_eq!(result, vec!["X   Do thing"]);
        unregister(name);
        assert!(lookup(name).is_none());
    }

    #[test]
    fn register_replaces_existing() {
        let name = "__test_provider_xyz__";
        register(name, vec![MetaEntry::new("A   First")]);
        register(name, vec![MetaEntry::new("B   Second")]);
        let result = lookup_formatted(name).unwrap();
        assert_eq!(result, vec!["B   Second"]);
        unregister(name);
    }

    #[test]
    fn unregister_removes_entry() {
        let name = "__test_provider_unregister__";
        register(name, vec![MetaEntry::new("X   thing")]);
        unregister(name);
        assert!(lookup(name).is_none());
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(lookup("__no_such_provider__").is_none());
        assert!(lookup_formatted("__no_such_provider__").is_none());
    }

    // lookup_with_context — email path branching

    #[test]
    fn email_default_path_shows_browse_hints() {
        let hints = lookup_with_context("emailclient", "/").unwrap();
        assert!(hints.iter().any(|s| s.contains("Search")));
        assert!(hints.iter().any(|s| s.contains("Refresh")));
        // Should NOT show compose-specific hints
        assert!(!hints.iter().any(|s| s.contains("Tab")));
    }

    #[test]
    fn email_compose_path_shows_tab_only() {
        let hints = lookup_with_context("emailclient", "/compose").unwrap();
        assert_eq!(hints.len(), 1);
        assert!(hints[0].contains("Tab"));
    }

    #[test]
    fn email_reply_path_shows_tab_only() {
        let hints = lookup_with_context("emailclient", "/reply").unwrap();
        assert!(hints.iter().any(|s| s.contains("Tab")));
        assert!(!hints.iter().any(|s| s.contains("Ctrl+I")));
    }

    #[test]
    fn email_compose_body_shows_editing_hints() {
        let hints = lookup_with_context("emailclient", "/compose/Body:").unwrap();
        assert!(hints.iter().any(|s| s.contains("Tab")));
        assert!(hints.iter().any(|s| s.contains("Ctrl+I")));
        assert!(hints.iter().any(|s| s.contains("Ctrl+A")));
    }

    #[test]
    fn email_forward_body_shows_editing_hints() {
        let hints = lookup_with_context("emailclient", "/forward/Body:plain/sub").unwrap();
        assert!(hints.iter().any(|s| s.contains("Ctrl+I")));
    }

    #[test]
    fn non_email_unaffected_by_path() {
        let with_path = lookup_with_context("filebrowser", "/some/deep/path");
        let without = lookup_formatted("filebrowser");
        assert_eq!(with_path, without);
    }
}
