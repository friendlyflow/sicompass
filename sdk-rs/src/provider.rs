use crate::ffon::FfonElement;
use std::path::Path;

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// An item in a command's selection list (e.g. applications for "open with").
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    pub label: String,
    pub data: String,
}

/// A result item from deep search (Ctrl+F).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResultItem {
    /// Display label with prefix, e.g. `"- report.pdf"`, `"+ docs"`
    pub label: String,
    /// Relative path context, e.g. `"docs > projects > "`
    pub breadcrumb: String,
    /// Absolute navigation path for teleport, e.g. `"/home/user/docs"`
    pub nav_path: String,
}

// ---------------------------------------------------------------------------
// Provider trait
// ---------------------------------------------------------------------------

/// The core provider interface — equivalent to the C `Provider` vtable.
///
/// Only `name()` and `fetch()` are required. All other methods have sensible
/// defaults that return false/empty/None.
///
/// Providers are `Send + 'static` so they can be boxed and moved across threads.
pub trait Provider: Send + 'static {
    // ---- Identity ----------------------------------------------------------

    fn name(&self) -> &str;

    /// Display name shown in the UI. Defaults to `name()`.
    fn display_name(&self) -> &str {
        self.name()
    }

    // ---- Required: data source ---------------------------------------------

    /// Fetch children at the current path.
    fn fetch(&mut self) -> Vec<FfonElement>;

    // ---- Optional: editing -------------------------------------------------

    /// Commit an inline edit. Returns `true` on success.
    fn commit_edit(&mut self, _old: &str, _new: &str) -> bool {
        false
    }

    // ---- Optional: lifecycle -----------------------------------------------

    fn init(&mut self) {}
    fn cleanup(&mut self) {}

    // ---- Optional: path navigation -----------------------------------------

    fn push_path(&mut self, _segment: &str) {}
    fn pop_path(&mut self) {}
    fn current_path(&self) -> &str { "/" }
    fn set_current_path(&mut self, _path: &str) {}

    // ---- Optional: file operations -----------------------------------------

    fn create_directory(&mut self, _name: &str) -> bool { false }
    fn create_file(&mut self, _name: &str) -> bool { false }
    fn delete_item(&mut self, _name: &str) -> bool { false }
    fn copy_item(
        &mut self,
        _src_dir: &str,
        _src_name: &str,
        _dest_dir: &str,
        _dest_name: &str,
    ) -> bool {
        false
    }

    // ---- Optional: commands ------------------------------------------------

    fn commands(&self) -> Vec<String> { vec![] }

    /// Handle a command — optionally return a UI element for gathering input.
    fn handle_command(
        &mut self,
        _cmd: &str,
        _elem_key: &str,
        _elem_type: i32,
        _error: &mut String,
    ) -> Option<FfonElement> {
        None
    }

    fn command_list_items(&self, _cmd: &str) -> Vec<ListItem> { vec![] }
    fn execute_command(&mut self, _cmd: &str, _selection: &str) -> bool { false }

    // ---- Optional: interactive element callbacks ---------------------------

    fn on_radio_change(&mut self, _group: &str, _value: &str) {}
    fn on_button_press(&mut self, _function_name: &str) {}
    fn on_checkbox_change(&mut self, _label: &str, _checked: bool) {}

    // ---- Optional: settings section management -----------------------------

    /// Register a named section in this provider. Used by programs to give
    /// themselves a section in the settings provider (mirrors C's `settingsAddSection`).
    fn add_settings_section(&mut self, _name: &str) {}

    /// Remove a named section from this provider.
    fn remove_settings_section(&mut self, _name: &str) {}

    /// Register a text entry in a settings section.
    /// Mirrors `settingsAddSectionText` in C. Default: no-op.
    fn add_text_setting(&mut self, _section: &str, _label: &str,
                        _config_key: &str, _default: &str) {}

    /// Register a checkbox entry in a settings section.
    /// Mirrors `settingsAddSectionCheckbox` in C. Default: no-op.
    fn add_checkbox_setting(&mut self, _section: &str, _label: &str,
                            _config_key: &str, _default_checked: bool) {}

    /// Register a radio group in a settings section.
    /// Mirrors `settingsAddSectionRadio` in C. Default: no-op.
    fn add_radio_setting(&mut self, _section: &str, _label: &str,
                         _config_key: &str, _options: &[&str], _default: &str) {}

    /// Called when any setting changes (key/value pair from the settings apply callback).
    ///
    /// Providers implement this to react to settings that affect them
    /// (e.g. `chatHomeserver`, `sortOrder`, `colorScheme`). Default: no-op.
    fn on_setting_change(&mut self, _key: &str, _value: &str) {}

    /// Create a new FFON element for an "Add element:" section.
    fn create_element(&mut self, _key: &str) -> Option<FfonElement> { None }

    // ---- Optional: deep search ---------------------------------------------

    /// Collect all searchable items for Ctrl+F extended search.
    /// Returns `None` to fall back to FFON-tree traversal.
    fn collect_deep_search_items(&self) -> Option<Vec<SearchResultItem>> { None }

    // ---- Optional: meta/help -----------------------------------------------

    /// Keyboard shortcut hints shown when the user presses M.
    /// Returns strings like `"Ctrl+I  Insert before"`.
    fn meta(&self) -> Vec<String> { vec![] }

    // ---- Optional: persistent config ---------------------------------------

    fn load_config(&mut self, _path: &Path) -> bool { false }
    fn save_config(&self, _path: &Path) -> bool { false }

    // ---- Optional: metadata ------------------------------------------------

    /// Path to a dashboard image shown fullscreen via `d` key.
    fn dashboard_image_path(&self) -> Option<&str> { None }

    /// Enable Ctrl+S/O save/load for this provider.
    fn supports_config_files(&self) -> bool { false }

    /// If true, always re-fetch on navigation (no caching).
    fn no_cache(&self) -> bool { false }

    // ---- Optional: cross-thread refresh signal -----------------------------

    fn needs_refresh(&self) -> bool { false }
    fn clear_needs_refresh(&mut self) {}

    // ---- Optional: error reporting -----------------------------------------

    /// Take (consume) any pending error message.
    fn take_error(&mut self) -> Option<String> { None }
}

// ---------------------------------------------------------------------------
// GenericProvider — wraps a fetch closure, handles path management
// ---------------------------------------------------------------------------

/// A convenient concrete `Provider` implementation for simple providers
/// that only need to implement `fetch` as a function of the current path.
///
/// Equivalent to `providerCreate(ops)` in the C code.
pub struct GenericProvider {
    name: String,
    display_name: String,
    current_path: String,
    fetch_fn: Box<dyn Fn(&str) -> Vec<FfonElement> + Send + 'static>,
    error: Option<String>,
}

impl GenericProvider {
    pub fn new(
        name: impl Into<String>,
        display_name: impl Into<String>,
        fetch_fn: impl Fn(&str) -> Vec<FfonElement> + Send + 'static,
    ) -> Self {
        GenericProvider {
            name: name.into(),
            display_name: display_name.into(),
            current_path: "/".to_owned(),
            fetch_fn: Box::new(fetch_fn),
            error: None,
        }
    }
}

impl Provider for GenericProvider {
    fn name(&self) -> &str { &self.name }
    fn display_name(&self) -> &str { &self.display_name }

    fn fetch(&mut self) -> Vec<FfonElement> {
        (self.fetch_fn)(&self.current_path)
    }

    fn push_path(&mut self, segment: &str) {
        if self.current_path == "/" {
            self.current_path = format!("/{segment}");
        } else {
            self.current_path.push('/');
            self.current_path.push_str(segment);
        }
    }

    fn pop_path(&mut self) {
        if let Some(slash) = self.current_path.rfind('/') {
            if slash == 0 {
                self.current_path = "/".to_owned();
            } else {
                self.current_path.truncate(slash);
            }
        }
    }

    fn current_path(&self) -> &str { &self.current_path }

    fn set_current_path(&mut self, path: &str) {
        self.current_path = path.to_owned();
    }

    fn take_error(&mut self) -> Option<String> { self.error.take() }
}

// ---------------------------------------------------------------------------
// Provider factory registry
// ---------------------------------------------------------------------------

/// A factory function that creates a `Provider` by name.
pub type ProviderFactory = Box<dyn Fn() -> Box<dyn Provider> + Send + Sync>;

/// Global provider factory registry.
///
/// Call `register_provider_factory` at startup to make a provider
/// instantiable by name. Use `create_provider` to instantiate one.
static REGISTRY: std::sync::OnceLock<std::sync::Mutex<Vec<(String, ProviderFactory)>>> =
    std::sync::OnceLock::new();

fn registry() -> &'static std::sync::Mutex<Vec<(String, ProviderFactory)>> {
    REGISTRY.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

pub fn register_provider_factory(
    name: &str,
    factory: impl Fn() -> Box<dyn Provider> + Send + Sync + 'static,
) {
    registry().lock().unwrap().push((name.to_owned(), Box::new(factory)));
}

pub fn create_provider_by_name(name: &str) -> Option<Box<dyn Provider>> {
    let guard = registry().lock().unwrap();
    guard.iter().find(|(n, _)| n == name).map(|(_, f)| f())
}

// ---------------------------------------------------------------------------
// Tests — port of tests/lib_provider/test_provider_interface.c (30 tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct SimpleProvider {
        name: String,
        path: String,
        error: Option<String>,
    }

    impl SimpleProvider {
        fn new(name: &str) -> Self {
            SimpleProvider { name: name.to_owned(), path: "/".to_owned(), error: None }
        }
    }

    impl Provider for SimpleProvider {
        fn name(&self) -> &str { &self.name }
        fn fetch(&mut self) -> Vec<FfonElement> {
            vec![FfonElement::new_str("item")]
        }
        fn push_path(&mut self, seg: &str) {
            if self.path == "/" { self.path = format!("/{seg}"); }
            else { self.path.push('/'); self.path.push_str(seg); }
        }
        fn pop_path(&mut self) {
            if let Some(slash) = self.path.rfind('/') {
                if slash == 0 { self.path = "/".to_owned(); }
                else { self.path.truncate(slash); }
            }
        }
        fn current_path(&self) -> &str { &self.path }
        fn set_current_path(&mut self, p: &str) { self.path = p.to_owned(); }
        fn take_error(&mut self) -> Option<String> { self.error.take() }
    }

    #[test]
    fn test_provider_name() {
        let p = SimpleProvider::new("test");
        assert_eq!(p.name(), "test");
    }

    #[test]
    fn test_provider_fetch_returns_elements() {
        let mut p = SimpleProvider::new("test");
        let elems = p.fetch();
        assert!(!elems.is_empty());
    }

    #[test]
    fn test_provider_push_path_from_root() {
        let mut p = SimpleProvider::new("t");
        p.push_path("dir");
        assert_eq!(p.current_path(), "/dir");
    }

    #[test]
    fn test_provider_push_path_nested() {
        let mut p = SimpleProvider::new("t");
        p.push_path("a");
        p.push_path("b");
        assert_eq!(p.current_path(), "/a/b");
    }

    #[test]
    fn test_provider_pop_path() {
        let mut p = SimpleProvider::new("t");
        p.push_path("a");
        p.push_path("b");
        p.pop_path();
        assert_eq!(p.current_path(), "/a");
    }

    #[test]
    fn test_provider_pop_path_to_root() {
        let mut p = SimpleProvider::new("t");
        p.push_path("a");
        p.pop_path();
        assert_eq!(p.current_path(), "/");
    }

    #[test]
    fn test_provider_pop_path_at_root_stays_root() {
        let mut p = SimpleProvider::new("t");
        p.pop_path();
        assert_eq!(p.current_path(), "/");
    }

    #[test]
    fn test_provider_set_current_path() {
        let mut p = SimpleProvider::new("t");
        p.set_current_path("/some/path");
        assert_eq!(p.current_path(), "/some/path");
    }

    #[test]
    fn test_provider_commit_edit_default_false() {
        let mut p = SimpleProvider::new("t");
        assert!(!p.commit_edit("old", "new"));
    }

    #[test]
    fn test_provider_commands_default_empty() {
        let p = SimpleProvider::new("t");
        assert!(p.commands().is_empty());
    }

    #[test]
    fn test_provider_create_directory_default_false() {
        let mut p = SimpleProvider::new("t");
        assert!(!p.create_directory("dir"));
    }

    #[test]
    fn test_provider_create_file_default_false() {
        let mut p = SimpleProvider::new("t");
        assert!(!p.create_file("file.txt"));
    }

    #[test]
    fn test_provider_delete_item_default_false() {
        let mut p = SimpleProvider::new("t");
        assert!(!p.delete_item("file.txt"));
    }

    #[test]
    fn test_provider_copy_item_default_false() {
        let mut p = SimpleProvider::new("t");
        assert!(!p.copy_item("/src", "file.txt", "/dst", "file.txt"));
    }

    #[test]
    fn test_provider_execute_command_default_false() {
        let mut p = SimpleProvider::new("t");
        assert!(!p.execute_command("create file", "notes.txt"));
    }

    #[test]
    fn test_provider_dashboard_image_path_default_none() {
        let p = SimpleProvider::new("t");
        assert!(p.dashboard_image_path().is_none());
    }

    #[test]
    fn test_provider_supports_config_files_default_false() {
        let p = SimpleProvider::new("t");
        assert!(!p.supports_config_files());
    }

    #[test]
    fn test_provider_no_cache_default_false() {
        let p = SimpleProvider::new("t");
        assert!(!p.no_cache());
    }

    #[test]
    fn test_provider_needs_refresh_default_false() {
        let p = SimpleProvider::new("t");
        assert!(!p.needs_refresh());
    }

    #[test]
    fn test_provider_take_error_default_none() {
        let mut p = SimpleProvider::new("t");
        assert!(p.take_error().is_none());
    }

    #[test]
    fn test_provider_collect_deep_search_default_none() {
        let p = SimpleProvider::new("t");
        assert!(p.collect_deep_search_items().is_none());
    }

    #[test]
    fn test_provider_init_and_cleanup_no_panic() {
        let mut p = SimpleProvider::new("t");
        p.init();
        p.cleanup();
    }

    // --- GenericProvider ---

    #[test]
    fn test_generic_provider_fetch() {
        let mut p = GenericProvider::new("test", "Test", |_path| {
            vec![FfonElement::new_str("hello")]
        });
        let elems = p.fetch();
        assert_eq!(elems.len(), 1);
    }

    #[test]
    fn test_generic_provider_path_management() {
        let mut p = GenericProvider::new("test", "Test", |path| {
            vec![FfonElement::new_str(path)]
        });
        p.push_path("dir");
        assert_eq!(p.current_path(), "/dir");
        let elems = p.fetch();
        assert_eq!(elems[0].as_str(), Some("/dir"));
        p.pop_path();
        assert_eq!(p.current_path(), "/");
    }

    #[test]
    fn test_generic_provider_set_path() {
        let mut p = GenericProvider::new("t", "T", |_| vec![]);
        p.set_current_path("/a/b/c");
        assert_eq!(p.current_path(), "/a/b/c");
    }

    // --- Factory registry ---

    #[test]
    fn test_factory_register_and_create() {
        register_provider_factory("test_factory_provider", || {
            Box::new(GenericProvider::new("test_factory_provider", "T", |_| {
                vec![FfonElement::new_str("from factory")]
            }))
        });
        let mut p = create_provider_by_name("test_factory_provider").unwrap();
        let elems = p.fetch();
        assert_eq!(elems[0].as_str(), Some("from factory"));
    }

    #[test]
    fn test_factory_create_unknown_returns_none() {
        assert!(create_provider_by_name("__nonexistent__").is_none());
    }

    #[test]
    fn test_provider_init_path_is_root() {
        let p = GenericProvider::new("p", "P", |_| vec![]);
        assert_eq!(p.current_path(), "/");
    }

    #[test]
    fn test_provider_display_name_defaults_to_name() {
        let p = GenericProvider::new("myname", "My Name", |_| vec![]);
        assert_eq!(p.display_name(), "My Name");
        assert_eq!(p.name(), "myname");
    }
}
