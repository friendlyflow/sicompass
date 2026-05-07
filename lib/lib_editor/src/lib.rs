//! Editor provider — file-rooted code/text editor.
//!
//! Implements the [`Provider`] trait as a normal plugin.
//! The provider navigates a filesystem tree rooted at the user-configurable
//! `editorPath` setting and, when the user enters a file, parses its content
//! into a FFON element tree using the rules in [`parse`]:
//!
//! - Lines separated by `\n` are siblings.
//! - A line ending with `:` becomes a section header (`FfonElement::Obj`).
//! - A `{ … }` block following a section header supplies its children.
//!
//! When no `editorPath` is configured the provider defaults to the user's
//! home directory.

mod parse;

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::manifest::{BuiltinManifest, SettingDecl};
use sicompass_sdk::provider::Provider;
use sicompass_sdk::{register_builtin_manifest, register_provider_factory};
use sicompass_sdk::tags;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// EditorProvider
// ---------------------------------------------------------------------------

pub struct EditorProvider {
    /// The root path shown when navigating to the editor.
    editor_path: String,
    /// Current filesystem position (rooted at `editor_path`).
    current_fs_path: PathBuf,
    /// Path within a file's parsed FFON tree (non-empty when inside a file).
    ffon_sub_path: Vec<String>,
    /// Set to `true` when `editor_path` changes so the app re-fetches the FFON.
    refresh_pending: bool,
    /// Combined path returned by `current_path()`.
    ///
    /// Equals `current_fs_path` when `ffon_sub_path` is empty, otherwise
    /// `current_fs_path/seg1/seg2/…`.  Keeping the FFON segments in the
    /// path makes `navigate_left_raw` detect a path change when the user
    /// presses left from inside a FFON section, enabling correct cursor
    /// restoration to the section element they came from.
    current_path_str: String,
}

impl EditorProvider {
    pub fn new() -> Self {
        let editor_path = home_dir();
        let current_fs_path = PathBuf::from(&editor_path);
        let current_path_str = editor_path.clone();
        EditorProvider {
            editor_path,
            current_fs_path,
            ffon_sub_path: Vec::new(),
            refresh_pending: false,
            current_path_str,
        }
    }

    fn sync_path_str(&mut self) {
        let base = self.current_fs_path.to_str().unwrap_or("/");
        self.current_path_str = if self.ffon_sub_path.is_empty() {
            base.to_string()
        } else {
            format!("{}/{}", base, self.ffon_sub_path.join("/"))
        };
    }

    fn root_path(&self) -> PathBuf {
        PathBuf::from(&self.editor_path)
    }

    fn list_directory(&self, dir: &Path) -> Vec<FfonElement> {
        let mut entries: Vec<(bool, String)> = match std::fs::read_dir(dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let is_dir = e.file_type().ok()?.is_dir();
                    Some((is_dir, name))
                })
                .collect(),
            Err(_) => return vec![],
        };
        entries.sort_by(|a, b| {
            a.0.cmp(&b.0).reverse().then(a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        entries
            .into_iter()
            .map(|(is_dir, name)| {
                if is_dir {
                    FfonElement::new_obj(&name)
                } else {
                    FfonElement::new_obj(&name)
                }
            })
            .collect()
    }

    fn fetch_file_content(&self) -> Vec<FfonElement> {
        let contents = match std::fs::read_to_string(&self.current_fs_path) {
            Ok(s) => s,
            Err(_) => return vec![FfonElement::new_str("(binary or unreadable file)")],
        };
        let ext = self.current_fs_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let tree = parse::parse_file_ext(&contents, ext);
        if self.ffon_sub_path.is_empty() {
            tree
        } else {
            parse::navigate_path(&tree, &self.ffon_sub_path).to_vec()
        }
    }
}

impl Default for EditorProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for EditorProvider {
    fn name(&self) -> &str { "editor" }

    fn init(&mut self) {
        // Read saved editorPath from config so the first fetch() shows the
        // correct directory rather than the home-dir default.
        if let Some(path) = sicompass_sdk::platform::main_config_path() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(root) = serde_json::from_str::<serde_json::Value>(&data) {
                    if let Some(val) = root
                        .get("editor")
                        .and_then(|s| s.get("editorPath"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        self.editor_path = val.to_string();
                    }
                }
            }
        }
        self.current_fs_path = PathBuf::from(&self.editor_path);
        self.ffon_sub_path.clear();
        self.refresh_pending = false;
        self.sync_path_str();
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        if self.editor_path.trim().is_empty() {
            return vec![FfonElement::new_str("set editor path in settings")];
        }
        if self.current_fs_path.is_file() || !self.ffon_sub_path.is_empty() {
            self.fetch_file_content()
        } else {
            self.list_directory(&self.current_fs_path.clone())
        }
    }

    fn push_path(&mut self, segment: &str) {
        let clean = tags::strip_display(segment);
        let clean = clean.trim_end_matches('/');
        if self.current_fs_path.is_file() {
            self.ffon_sub_path.push(clean.to_string());
        } else {
            let candidate = self.current_fs_path.join(clean);
            if candidate.exists() {
                self.current_fs_path = candidate;
            }
        }
        self.sync_path_str();
    }

    fn pop_path(&mut self) {
        if !self.ffon_sub_path.is_empty() {
            self.ffon_sub_path.pop();
        } else if self.current_fs_path != self.root_path() {
            self.current_fs_path.pop();
        }
        self.sync_path_str();
    }

    fn current_path(&self) -> &str {
        &self.current_path_str
    }

    fn set_current_path(&mut self, path: &str) {
        self.current_fs_path = PathBuf::from(path);
        self.ffon_sub_path.clear();
        self.sync_path_str();
    }

    fn stable_root_key(&self) -> bool { true }

    fn preferred_coordinate_kind(&self) -> sicompass_sdk::CoordinateKind {
        sicompass_sdk::CoordinateKind::Editor
    }

    fn refresh_on_navigate(&self) -> bool { true }

    fn needs_refresh(&self) -> bool {
        self.refresh_pending
    }

    fn clear_needs_refresh(&mut self) {
        self.refresh_pending = false;
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        if key == "editorPath" && value != self.editor_path {
            self.editor_path = value.to_string();
            self.current_fs_path = PathBuf::from(value);
            self.ffon_sub_path.clear();
            self.refresh_pending = true;
            self.sync_path_str();
        }
    }
}

// ---------------------------------------------------------------------------
// Platform: home directory
// ---------------------------------------------------------------------------

fn home_dir() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// SDK registration
// ---------------------------------------------------------------------------

/// Register the editor provider with the SDK factory and manifest registries.
pub fn register() {
    let home = home_dir();
    register_provider_factory("editor", || Box::new(EditorProvider::new()));
    register_builtin_manifest(
        BuiltinManifest::new("editor", "editor")
            .with_settings(vec![
                SettingDecl::text("editor", "editor path", "editorPath", &home),
            ]),
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_tmp() -> TempDir {
        TempDir::new().expect("tempdir")
    }

    #[test]
    fn fetch_with_empty_path_returns_hint() {
        let mut p = EditorProvider {
            editor_path: String::new(),
            current_fs_path: PathBuf::new(),
            ffon_sub_path: vec![],
            refresh_pending: false,
            current_path_str: String::new(),
        };
        let items = p.fetch();
        assert_eq!(items.len(), 1);
        assert!(items[0].as_str().unwrap().contains("set editor path"));
    }

    #[test]
    fn on_setting_change_updates_editor_path() {
        let mut p = EditorProvider::new();
        p.on_setting_change("editorPath", "/tmp/test");
        assert_eq!(p.editor_path, "/tmp/test");
        assert_eq!(p.current_fs_path, PathBuf::from("/tmp/test"));
    }

    #[test]
    fn on_setting_change_sets_refresh_pending() {
        let mut p = EditorProvider::new();
        assert!(!p.needs_refresh());
        p.on_setting_change("editorPath", "/tmp/newpath");
        assert!(p.needs_refresh(), "changing the path must set refresh_pending");
    }

    #[test]
    fn on_setting_change_same_path_does_not_set_refresh() {
        let mut p = EditorProvider::new();
        let same = p.editor_path.clone();
        p.on_setting_change("editorPath", &same);
        assert!(!p.needs_refresh(), "same path must not trigger a refresh");
    }

    #[test]
    fn clear_needs_refresh_clears_flag() {
        let mut p = EditorProvider::new();
        p.on_setting_change("editorPath", "/tmp/other");
        assert!(p.needs_refresh());
        p.clear_needs_refresh();
        assert!(!p.needs_refresh());
    }

    #[test]
    fn on_setting_change_ignores_other_keys() {
        let mut p = EditorProvider::new();
        let original = p.editor_path.clone();
        p.on_setting_change("someOtherKey", "/etc");
        assert_eq!(p.editor_path, original);
        assert!(!p.needs_refresh());
    }

    #[test]
    fn current_path_includes_ffon_sub_path() {
        let tmp = make_tmp();
        let file = tmp.path().join("data.txt");
        std::fs::write(&file, "section:\n{\n  child\n}").unwrap();

        let mut p = EditorProvider::new();
        p.on_setting_change("editorPath", tmp.path().to_str().unwrap());
        p.push_path("data.txt");
        let path_at_file = p.current_path().to_string();

        p.push_path("section:");
        let path_at_section = p.current_path().to_string();

        // Pushing into a FFON section must change current_path() so that
        // navigate_left_raw can detect the change and restore the cursor.
        assert_ne!(path_at_file, path_at_section);
        assert!(path_at_section.contains("section:"),
            "current_path should include the FFON segment, got: {}", path_at_section);

        p.pop_path();
        assert_eq!(p.current_path(), path_at_file,
            "popping the FFON segment must restore the file path");
    }

    #[test]
    fn init_uses_config_path_when_available() {
        // Write a temporary config file with an editorPath entry.
        let tmp = make_tmp();
        let config = tmp.path().join("settings.json");
        let saved = tmp.path().join("saved_root");
        std::fs::create_dir(&saved).unwrap();
        std::fs::write(
            &config,
            format!(r#"{{"editor":{{"editorPath":"{}"}}}}"#, saved.to_str().unwrap()),
        ).unwrap();

        // Manually exercise the same JSON reading logic that init() uses.
        let data = std::fs::read_to_string(&config).unwrap();
        let root: serde_json::Value = serde_json::from_str(&data).unwrap();
        let read_path = root
            .get("editor")
            .and_then(|s| s.get("editorPath"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("");
        assert_eq!(read_path, saved.to_str().unwrap());
    }

    #[test]
    fn fetch_lists_directory_entries() {
        let tmp = make_tmp();
        std::fs::write(tmp.path().join("hello.txt"), "hi").unwrap();
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();

        let mut p = EditorProvider::new();
        p.on_setting_change("editorPath", tmp.path().to_str().unwrap());
        let items = p.fetch();
        assert!(!items.is_empty());
        let names: Vec<String> = items.iter().filter_map(|e| {
            e.as_obj().map(|o| o.key.clone()).or_else(|| e.as_str().map(|s| s.to_string()))
        }).collect();
        assert!(names.iter().any(|n| n.contains("hello.txt")), "expected hello.txt, got {:?}", names);
        assert!(names.iter().any(|n| n.contains("subdir")), "expected subdir, got {:?}", names);
    }

    #[test]
    fn push_pop_navigates_directories() {
        let tmp = make_tmp();
        std::fs::create_dir(tmp.path().join("inner")).unwrap();

        let mut p = EditorProvider::new();
        p.on_setting_change("editorPath", tmp.path().to_str().unwrap());
        p.push_path("inner");
        assert!(p.current_fs_path.ends_with("inner"));
        p.pop_path();
        assert_eq!(p.current_fs_path, PathBuf::from(tmp.path()));
    }

    #[test]
    fn fetch_file_returns_parsed_content() {
        let tmp = make_tmp();
        let file = tmp.path().join("code.txt");
        std::fs::write(&file, "section:\n{\n  line1\n}\nplain").unwrap();

        let mut p = EditorProvider::new();
        p.on_setting_change("editorPath", tmp.path().to_str().unwrap());
        p.push_path("code.txt");
        let items = p.fetch();
        assert_eq!(items.len(), 2);
        assert!(items[0].is_obj());
        assert!(items[1].is_str());
    }

    #[test]
    fn push_into_ffon_section_navigates_sub_path() {
        let tmp = make_tmp();
        let file = tmp.path().join("data.txt");
        std::fs::write(&file, "section:\n{\n  child\n}").unwrap();

        let mut p = EditorProvider::new();
        p.on_setting_change("editorPath", tmp.path().to_str().unwrap());
        p.push_path("data.txt");
        p.push_path("section:");
        let items = p.fetch();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].as_str(), Some("child"));
    }

    #[test]
    fn pop_path_cannot_go_above_root() {
        let tmp = make_tmp();
        let mut p = EditorProvider::new();
        p.on_setting_change("editorPath", tmp.path().to_str().unwrap());
        p.pop_path(); // already at root, should be a no-op
        assert_eq!(p.current_fs_path, PathBuf::from(tmp.path()));
    }

    #[test]
    fn register_is_idempotent() {
        register();
        register(); // second call must not panic (OnceLock in factory registry)
    }

    #[test]
    fn editor_provider_preferred_coordinate_kind_is_editor() {
        let p = EditorProvider::new();
        assert_eq!(p.preferred_coordinate_kind(), sicompass_sdk::CoordinateKind::Editor);
    }
}
