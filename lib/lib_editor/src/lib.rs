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
//! Every FFON element produced from a file carries a `<src=N>` annotation
//! that encodes the 0-based source-line index.  This lets `commit_edit` map
//! edits back to exact lines on disk without a lossy round-trip through the
//! parser.
//!
//! When no `editorPath` is configured the provider defaults to the user's
//! home directory.

mod parse;

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::manifest::{BuiltinManifest, SettingDecl};
use sicompass_sdk::placeholders::I_PLACEHOLDER;
use sicompass_sdk::provider::Provider;
use sicompass_sdk::tags;
use sicompass_sdk::{register_builtin_manifest, register_provider_factory};
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
    current_path_str: String,
    /// Raw source lines of the currently-open file (verbatim, with indentation).
    source_lines: Vec<String>,
    /// Parsed FFON tree for the current file, with `<src=N>` annotations and
    /// `<input>` wrappers so every line is editable.
    cached_ffon: Vec<FfonElement>,
    /// Which file produced the current cache (avoids re-reading on every fetch).
    loaded_path: Option<PathBuf>,
    /// Whether the source file ended with a newline (restored on every flush).
    trailing_newline: bool,
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
            source_lines: Vec::new(),
            cached_ffon: Vec::new(),
            loaded_path: None,
            trailing_newline: false,
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

    fn is_in_file_view(&self) -> bool {
        self.current_fs_path.is_file() || !self.ffon_sub_path.is_empty()
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
                let marker = if is_dir { "<dir>" } else { "<file>" };
                let tagged = format!("{marker}{}", tags::format_input(&name));
                FfonElement::new_obj(&tagged)
            })
            .collect()
    }

    /// Load (or reload) the file at `current_fs_path` into `source_lines` and
    /// build the annotated `cached_ffon`.  Returns `false` if the file cannot
    /// be read.
    fn load_file(&mut self) -> bool {
        let contents = match std::fs::read_to_string(&self.current_fs_path) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let ext = self.current_fs_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        // Keep verbatim lines for write-back (indentation + blank lines preserved).
        self.trailing_newline = contents.ends_with('\n');
        self.source_lines = contents.lines().map(str::to_owned).collect();

        // Parse into an annotated FFON tree, then wrap every element in <input>
        // so the app renders each line as editable.
        let tree = parse::parse_file_ext(&contents, ext);
        self.cached_ffon = wrap_ffon_in_input(tree);
        self.loaded_path = Some(self.current_fs_path.clone());
        true
    }

    fn fetch_file_content(&mut self) -> Vec<FfonElement> {
        // Load or serve from cache.
        if self.loaded_path.as_deref() != Some(self.current_fs_path.as_path()) {
            if !self.load_file() {
                return vec![FfonElement::new_str("(binary or unreadable file)")];
            }
        }
        let tree = &self.cached_ffon;
        if self.ffon_sub_path.is_empty() {
            tree.clone()
        } else {
            parse::navigate_path(tree, &self.ffon_sub_path).to_vec()
        }
    }

    /// Write `source_lines` back to `current_fs_path` and rebuild `cached_ffon`.
    ///
    /// If a single `source_lines` entry contains embedded newlines — which
    /// happens when the user types a multi-line replacement or inserts text
    /// containing `\n` (Ctrl+Enter while in insert mode) — the joined output
    /// still produces the correct file on disk, but the in-memory vector
    /// would be out of sync with the file's line count, breaking any
    /// subsequent edit that addresses lines by index. Rebuild
    /// `source_lines` from the just-written content so it matches the file.
    fn flush_source_lines(&mut self) -> bool {
        let mut content = self.source_lines.join("\n");
        if self.trailing_newline {
            content.push('\n');
        }
        if std::fs::write(&self.current_fs_path, &content).is_err() {
            return false;
        }
        // Re-derive source_lines from the freshly-written content so each
        // vector slot maps to exactly one file line.
        self.source_lines = content.lines().map(str::to_owned).collect();
        // Rebuild cache from the just-written content.
        let ext = self.current_fs_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let tree = parse::parse_file_ext(&content, ext);
        self.cached_ffon = wrap_ffon_in_input(tree);
        true
    }

    fn rename_fs_item(&mut self, old: &str, new: &str) -> bool {
        let old_name = tags::strip_display(old);
        let new_name = tags::strip_display(new);
        if old_name.is_empty() || new_name.is_empty() || old_name == new_name {
            return false;
        }
        let old_path = self.current_fs_path.join(
            old_name.trim_end_matches('/').trim_end_matches('\\'),
        );
        let new_path = self.current_fs_path.join(
            new_name.trim_end_matches('/').trim_end_matches('\\'),
        );
        std::fs::rename(&old_path, &new_path).is_ok()
    }
}

// ---------------------------------------------------------------------------
// FFON wrapping helper
// ---------------------------------------------------------------------------

/// Recursively wrap every Str and Obj key in `<input>...</input>` so the app
/// renders each element as editable.  The `<src=N>` annotation is preserved
/// inside the `<input>` wrapper so commit_edit can decode the source-line index.
fn wrap_ffon_in_input(elements: Vec<FfonElement>) -> Vec<FfonElement> {
    elements
        .into_iter()
        .map(|elem| match elem {
            FfonElement::Str(s) => FfonElement::new_str(tags::format_input(&s)),
            FfonElement::Obj(mut obj) => {
                obj.key = tags::format_input(&obj.key);
                obj.children = wrap_ffon_in_input(obj.children);
                FfonElement::Obj(obj)
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Provider trait implementation
// ---------------------------------------------------------------------------

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
        self.loaded_path = None;
        self.source_lines.clear();
        self.cached_ffon.clear();
        self.sync_path_str();
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        if self.editor_path.trim().is_empty() {
            return vec![FfonElement::new_str("set editor path in settings")];
        }
        if self.is_in_file_view() {
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
                // Entering a new path invalidates the file cache.
                self.loaded_path = None;
                self.source_lines.clear();
                self.cached_ffon.clear();
                self.current_fs_path = candidate;
            }
        }
        self.sync_path_str();
    }

    fn pop_path(&mut self) {
        if !self.ffon_sub_path.is_empty() {
            self.ffon_sub_path.pop();
        } else if self.current_fs_path != self.root_path() {
            // Leaving a file — clear the cache.
            self.loaded_path = None;
            self.source_lines.clear();
            self.cached_ffon.clear();
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
        self.loaded_path = None;
        self.source_lines.clear();
        self.cached_ffon.clear();
        self.sync_path_str();
    }

    // ---- Editing -----------------------------------------------------------

    fn commit_edit(&mut self, old: &str, new: &str) -> bool {
        // Unwrap <input>...</input> wrappers that may be present (e.g. from
        // undo/redo paths where FsRename passes element keys verbatim).
        let old_inner = tags::extract_input(old)
            .unwrap_or_else(|| old.to_owned());
        let new_inner = tags::extract_input(new)
            .unwrap_or_else(|| new.to_owned());

        // ── Case 1: in-file line REPLACE ────────────────────────────────────
        // old_inner starts with <src=N>: edit the Nth source line.
        if let Some((line_idx, _old_text)) = tags::extract_src(&old_inner) {
            if line_idx >= self.source_lines.len() {
                return false;
            }
            // new_inner may be:
            //  a) "<srcins=N>"  → undo of a prior line insert: delete line N
            //  b) plain text   → replace the line with new content
            if let Some(del_idx) = tags::extract_src_insert(&new_inner) {
                // Undo of a line insert: delete line del_idx.
                if del_idx < self.source_lines.len() {
                    self.source_lines.remove(del_idx);
                    return self.flush_source_lines();
                }
                return false;
            }

            // Normal line replace: preserve original indentation. Multi-line
            // replacements (input buffer contains `\n`, e.g. typed via
            // Ctrl+Enter) expand into multiple source lines, each carrying the
            // same indent — empty inner lines are kept empty so the file ends
            // up with real blank lines.
            let new_text = tags::strip_display(&new_inner);
            let indent = leading_whitespace(&self.source_lines[line_idx]).to_owned();
            let new_lines = build_indented_lines(&new_text, &indent);
            if new_lines.len() == 1 && self.source_lines[line_idx] == new_lines[0] {
                return true; // nothing actually changed — still signal success
            }
            self.source_lines.splice(line_idx..line_idx + 1, new_lines);
            return self.flush_source_lines();
        }

        // ── Case 2: in-file line INSERT placeholder ─────────────────────────
        // old_inner is "<srcins=N>": insert a new line at position N. Like
        // Case 1, multi-line input expands into multiple lines each carrying
        // the surrounding indent; empty input becomes a single blank line so
        // pressing Ctrl+I and then Enter inserts a true empty line.
        if let Some(insert_idx) = tags::extract_src_insert(&old_inner) {
            let new_text = tags::strip_display(&new_inner);
            // Determine the indent from the line that will be displaced (if any).
            let indent = if insert_idx < self.source_lines.len() {
                leading_whitespace(&self.source_lines[insert_idx]).to_owned()
            } else if !self.source_lines.is_empty() {
                leading_whitespace(self.source_lines.last().unwrap()).to_owned()
            } else {
                String::new()
            };
            let new_lines = build_indented_lines(&new_text, &indent);
            let clamp = insert_idx.min(self.source_lines.len());
            self.source_lines.splice(clamp..clamp, new_lines);
            return self.flush_source_lines();
        }

        // ── Case 2b: in-file I_PLACEHOLDER ──────────────────────────────────
        // An empty file (or empty FFON sub-section) gets an `i <input></input>`
        // placeholder seeded by `navigate_right_raw` so the user has something
        // to type into. The Enter handler then calls commit_edit with an empty
        // `old` string. Insert the typed text as new file lines.
        if old_inner.is_empty() && !new_inner.is_empty() && self.is_in_file_view() {
            let new_text = tags::strip_display(&new_inner);
            if new_text.is_empty() {
                return false;
            }
            let new_lines = build_indented_lines(&new_text, "");
            let pos = self.source_lines.len();
            self.source_lines.splice(pos..pos, new_lines);
            return self.flush_source_lines();
        }

        // ── Case 3: directory create (old is empty = new item from placeholder) ─
        if old_inner.is_empty() && !new_inner.is_empty() && !self.is_in_file_view() {
            return if new_inner.ends_with(':') {
                self.create_directory(new_inner.trim_end_matches(':'))
            } else {
                self.create_file(&new_inner)
            };
        }

        // ── Case 4: directory rename ─────────────────────────────────────────
        if !self.is_in_file_view() {
            return self.rename_fs_item(old, new);
        }

        false
    }

    // ---- File operations ---------------------------------------------------

    fn create_file(&mut self, name: &str) -> bool {
        if name.is_empty() || self.is_in_file_view() { return false; }
        std::fs::File::create(self.current_fs_path.join(name)).is_ok()
    }

    fn create_directory(&mut self, name: &str) -> bool {
        if name.is_empty() || self.is_in_file_view() { return false; }
        std::fs::create_dir(self.current_fs_path.join(name)).is_ok()
    }

    fn delete_item(&mut self, name: &str) -> bool {
        // In-file: delete the indicated source line.
        let inner = tags::extract_input(name).unwrap_or_else(|| name.to_owned());
        if let Some((line_idx, _)) = tags::extract_src(&inner) {
            if line_idx < self.source_lines.len() {
                self.source_lines.remove(line_idx);
                return self.flush_source_lines();
            }
            return false;
        }

        // Directory view: move file/folder to OS trash.
        let clean = tags::strip_display(name);
        let clean = clean.trim_end_matches('/').trim_end_matches('\\');
        if clean.is_empty() { return false; }
        let full = self.current_fs_path.join(clean);
        trash::delete(&full).is_ok()
    }

    // ---- Commands ----------------------------------------------------------

    fn commands(&self) -> Vec<String> {
        if self.is_in_file_view() {
            vec![]
        } else {
            vec!["create directory".to_owned(), "create file".to_owned()]
        }
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        _elem_key: &str,
        _elem_type: i32,
        _error: &mut String,
    ) -> Option<FfonElement> {
        match cmd {
            "create directory" => Some(FfonElement::new_obj("<input></input>")),
            "create file" => Some(FfonElement::new_str("<input></input>".to_owned())),
            _ => None,
        }
    }

    // ---- Settings ----------------------------------------------------------

    fn stable_root_key(&self) -> bool { true }

    fn at_root(&self) -> bool {
        self.current_fs_path == self.root_path() && self.ffon_sub_path.is_empty()
    }

    fn has_editor_semantics(&self) -> bool { true }

    fn path_is_filesystem(&self) -> bool { true }

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
            self.loaded_path = None;
            self.source_lines.clear();
            self.cached_ffon.clear();
            self.refresh_pending = true;
            self.sync_path_str();
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn leading_whitespace(s: &str) -> &str {
    let end = s.find(|c: char| !c.is_whitespace()).unwrap_or(0);
    &s[..end]
}

/// Split `text` on `\n` into one source line per piece, prepending `indent`
/// to non-empty pieces. Empty input → a single empty line; trailing/leading
/// `\n` produce blank lines so the on-disk file actually contains them.
fn build_indented_lines(text: &str, indent: &str) -> Vec<String> {
    text.split('\n')
        .map(|piece| {
            if piece.is_empty() {
                String::new()
            } else {
                format!("{indent}{piece}")
            }
        })
        .collect()
}

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

    fn make_editor(tmp: &TempDir) -> EditorProvider {
        let mut p = EditorProvider::new();
        p.on_setting_change("editorPath", tmp.path().to_str().unwrap());
        p
    }

    // ---- basic fetch / navigation ------------------------------------------

    #[test]
    fn fetch_with_empty_path_returns_hint() {
        let mut p = EditorProvider {
            editor_path: String::new(),
            current_fs_path: PathBuf::new(),
            ffon_sub_path: vec![],
            refresh_pending: false,
            current_path_str: String::new(),
            source_lines: vec![],
            cached_ffon: vec![],
            loaded_path: None,
            trailing_newline: false,
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

        let mut p = make_editor(&tmp);
        p.push_path("data.txt");
        let path_at_file = p.current_path().to_string();

        p.push_path("section:");
        let path_at_section = p.current_path().to_string();

        assert_ne!(path_at_file, path_at_section);
        assert!(path_at_section.contains("section:"),
            "current_path should include the FFON segment, got: {}", path_at_section);

        p.pop_path();
        assert_eq!(p.current_path(), path_at_file,
            "popping the FFON segment must restore the file path");
    }

    #[test]
    fn push_pop_navigates_directories() {
        let tmp = make_tmp();
        std::fs::create_dir(tmp.path().join("inner")).unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("inner");
        assert!(p.current_fs_path.ends_with("inner"));
        p.pop_path();
        assert_eq!(p.current_fs_path, PathBuf::from(tmp.path()));
    }

    #[test]
    fn pop_path_cannot_go_above_root() {
        let tmp = make_tmp();
        let mut p = make_editor(&tmp);
        p.pop_path();
        assert_eq!(p.current_fs_path, PathBuf::from(tmp.path()));
    }

    #[test]
    fn fetch_lists_directory_entries_with_input_wrappers() {
        let tmp = make_tmp();
        std::fs::write(tmp.path().join("hello.txt"), "hi").unwrap();
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();

        let mut p = make_editor(&tmp);
        let items = p.fetch();
        assert!(!items.is_empty());
        // All entries must be wrapped in <input>
        for item in &items {
            let key = match item {
                FfonElement::Obj(o) => o.key.as_str(),
                FfonElement::Str(s) => s.as_str(),
            };
            if key == I_PLACEHOLDER { continue; }
            assert!(tags::has_input(key),
                "directory entry '{}' must be wrapped in <input>", key);
        }
        // File and directory names must appear somewhere
        let all_text: Vec<String> = items.iter().map(|e| match e {
            FfonElement::Obj(o) => o.key.clone(),
            FfonElement::Str(s) => s.clone(),
        }).collect();
        assert!(all_text.iter().any(|n| n.contains("hello.txt")));
        assert!(all_text.iter().any(|n| n.contains("subdir")));
    }

    #[test]
    fn list_directory_tags_dirs_and_files() {
        let tmp = make_tmp();
        std::fs::write(tmp.path().join("hello.txt"), "hi").unwrap();
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();

        let mut p = make_editor(&tmp);
        let items = p.fetch();

        let mut saw_dir = false;
        let mut saw_file = false;
        for item in &items {
            let key = match item {
                FfonElement::Obj(o) => o.key.as_str(),
                FfonElement::Str(s) => s.as_str(),
            };
            if key == I_PLACEHOLDER { continue; }
            if key.contains("subdir") {
                assert!(tags::has_dir(key), "subdir entry must carry <dir>: {key}");
                assert!(!tags::has_file(key), "subdir entry must not carry <file>: {key}");
                saw_dir = true;
            }
            if key.contains("hello.txt") {
                assert!(tags::has_file(key), "hello.txt entry must carry <file>: {key}");
                assert!(!tags::has_dir(key), "hello.txt entry must not carry <dir>: {key}");
                saw_file = true;
            }
        }
        assert!(saw_dir && saw_file, "expected both dir and file entries");
    }

    #[test]
    fn fetch_file_returns_annotated_content() {
        let tmp = make_tmp();
        let file = tmp.path().join("code.txt");
        std::fs::write(&file, "section:\n{\n  line1\n}\nplain").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("code.txt");
        let items = p.fetch();
        assert_eq!(items.len(), 2);
        assert!(items[0].is_obj());
        assert!(items[1].is_str());
        // All items must be <input>-wrapped and carry <src=N>
        for item in &items {
            let key = match item {
                FfonElement::Obj(o) => o.key.as_str(),
                FfonElement::Str(s) => s.as_str(),
            };
            assert!(tags::has_input(key), "element must have <input> wrapper");
            let inner = tags::extract_input(key).unwrap();
            assert!(tags::extract_src(&inner).is_some(),
                "inner content must have <src=N>, got: {}", inner);
        }
    }

    #[test]
    fn push_into_ffon_section_navigates_sub_path() {
        let tmp = make_tmp();
        let file = tmp.path().join("data.txt");
        std::fs::write(&file, "section:\n{\n  child\n}").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("data.txt");
        p.push_path("section:");
        let items = p.fetch();
        assert_eq!(items.len(), 1);
        // Child element must carry <src=N>
        let inner = tags::extract_input(items[0].as_str().unwrap()).unwrap();
        assert!(tags::extract_src(&inner).is_some());
    }

    #[test]
    fn register_is_idempotent() {
        register();
        register();
    }

    #[test]
    fn editor_provider_has_editor_semantics() {
        let p = EditorProvider::new();
        assert!(p.has_editor_semantics());
    }

    // ---- directory create / delete / rename --------------------------------

    #[test]
    fn create_file_creates_on_disk() {
        let tmp = make_tmp();
        let mut p = make_editor(&tmp);
        assert!(p.create_file("new.txt"));
        assert!(tmp.path().join("new.txt").exists());
    }

    #[test]
    fn create_directory_creates_on_disk() {
        let tmp = make_tmp();
        let mut p = make_editor(&tmp);
        assert!(p.create_directory("mydir"));
        assert!(tmp.path().join("mydir").is_dir());
    }

    #[test]
    fn create_file_rejects_empty_name() {
        let tmp = make_tmp();
        let mut p = make_editor(&tmp);
        assert!(!p.create_file(""));
    }

    #[test]
    fn create_directory_rejects_empty_name() {
        let tmp = make_tmp();
        let mut p = make_editor(&tmp);
        assert!(!p.create_directory(""));
    }

    #[test]
    fn delete_item_moves_file_to_trash() {
        let tmp = make_tmp();
        let file = tmp.path().join("bye.txt");
        std::fs::write(&file, "").unwrap();
        let mut p = make_editor(&tmp);
        assert!(p.delete_item("<input>bye.txt</input>"));
        assert!(!file.exists());
    }

    #[test]
    fn commit_edit_renames_file() {
        let tmp = make_tmp();
        let file = tmp.path().join("old.txt");
        std::fs::write(&file, "").unwrap();
        let mut p = make_editor(&tmp);
        assert!(p.commit_edit("<input>old.txt</input>", "<input>new.txt</input>"));
        assert!(!file.exists());
        assert!(tmp.path().join("new.txt").exists());
    }

    // ---- file content editing -----------------------------------------------

    #[test]
    fn commit_edit_replaces_line_on_disk() {
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\nbeta\ngamma").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch(); // prime cache

        // Simulate editing line 1 ("beta") via <input><src=1>beta</input>
        let old = format!("<input>{}beta</input>", tags::format_src(1));
        let ok = p.commit_edit(&old, "BETA");
        assert!(ok);

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "alpha\nBETA\ngamma");
    }

    #[test]
    fn commit_edit_preserves_indentation() {
        let tmp = make_tmp();
        let file = tmp.path().join("py.py");
        std::fs::write(&file, "def foo():\n    pass\n").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("py.py");
        p.fetch();

        // Line 1 is "    pass" (4 spaces). Editing with stripped content "pass".
        let old = format!("<input>{}pass</input>", tags::format_src(1));
        let ok = p.commit_edit(&old, "return 1");
        assert!(ok);

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "def foo():\n    return 1\n");
    }

    #[test]
    fn commit_edit_insert_new_line() {
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\ngamma").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        // Insert before line 1 ("gamma")
        let old = format!("<input>{}</input>", tags::format_src_insert(1));
        let ok = p.commit_edit(&old, "beta");
        assert!(ok);

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "alpha\nbeta\ngamma");
    }

    #[test]
    fn commit_edit_undo_of_insert_deletes_line() {
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\nbeta\ngamma").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        // Undo of an insert at line 1: old = current line 1, new = srcins placeholder
        let old = format!("<input>{}beta</input>", tags::format_src(1));
        let new = format!("<input>{}</input>", tags::format_src_insert(1));
        let ok = p.commit_edit(&old, &new);
        assert!(ok);

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "alpha\ngamma");
    }

    #[test]
    fn delete_item_removes_source_line() {
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\nbeta\ngamma").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        // Delete line 1 ("beta")
        let name = format!("<input>{}beta</input>", tags::format_src(1));
        let ok = p.delete_item(&name);
        assert!(ok);

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "alpha\ngamma");
    }

    #[test]
    fn commit_edit_multiline_replace_writes_each_line_with_indent() {
        // Multi-line input from Ctrl+Enter expands into multiple file lines,
        // each preserving the original indentation of the line being replaced.
        let tmp = make_tmp();
        let file = tmp.path().join("py.py");
        std::fs::write(&file, "def foo():\n    pass\n").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("py.py");
        p.fetch();

        // Replace line 1 ("    pass") with two lines.
        let old = format!("<input>{}pass</input>", tags::format_src(1));
        let ok = p.commit_edit(&old, "x = 1\ny = 2");
        assert!(ok);

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "def foo():\n    x = 1\n    y = 2\n");
    }

    #[test]
    fn commit_edit_after_multiline_replace_keeps_subsequent_edits_aligned() {
        // Regression: previously source_lines wasn't rebuilt after a
        // multi-line replace, so a single source slot ended up containing
        // an embedded `\n`. Subsequent edits used stale indices and either
        // bailed out or overwrote the wrong line.
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\nbeta\ngamma\n").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        // Replace line 1 ("beta") with two lines.
        let old = format!("<input>{}beta</input>", tags::format_src(1));
        assert!(p.commit_edit(&old, "b1\nb2"));

        // After re-parsing, "gamma" is now at src=3. Editing it must succeed.
        let old_gamma = format!("<input>{}gamma</input>", tags::format_src(3));
        assert!(p.commit_edit(&old_gamma, "GAMMA"));

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "alpha\nb1\nb2\nGAMMA\n");
    }

    #[test]
    fn commit_edit_insert_empty_line() {
        // Ctrl+I followed by Enter (no text) inserts a blank line at the
        // placeholder position.
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\ngamma\n").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        let old = format!("<input>{}</input>", tags::format_src_insert(1));
        assert!(p.commit_edit(&old, ""));

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "alpha\n\ngamma\n");
    }

    #[test]
    fn commit_edit_insert_multiline() {
        // Ctrl+I, type "first\nsecond\nthird", Enter — three lines inserted
        // before the displaced line, each at the displaced line's indent.
        let tmp = make_tmp();
        let file = tmp.path().join("py.py");
        std::fs::write(&file, "def foo():\n    pass\n").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("py.py");
        p.fetch();

        let old = format!("<input>{}</input>", tags::format_src_insert(1));
        assert!(p.commit_edit(&old, "a = 1\nb = 2\nc = 3"));

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            written,
            "def foo():\n    a = 1\n    b = 2\n    c = 3\n    pass\n"
        );
    }

    #[test]
    fn commit_edit_writes_first_line_to_empty_file() {
        // Repro: create file → right (open) → i (insert mode on the seeded
        // placeholder) → type → Enter. Previously commit_edit fell through all
        // four cases and returned false, leaving the file empty on disk.
        let tmp = make_tmp();
        let file = tmp.path().join("new.txt");
        std::fs::write(&file, "").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("new.txt");
        p.fetch();

        // Empty `old` mirrors what handle_enter_insert sends when the
        // active element is the I_PLACEHOLDER (`<input></input>` is empty).
        assert!(p.commit_edit("", "hello"));

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "hello");
    }

    #[test]
    fn commit_edit_writes_multiline_to_empty_file() {
        // Multi-line input via Ctrl+Enter → each piece becomes its own file
        // line, including trailing/leading blank lines.
        let tmp = make_tmp();
        let file = tmp.path().join("new.txt");
        std::fs::write(&file, "").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("new.txt");
        p.fetch();

        assert!(p.commit_edit("", "first\n\nthird"));

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "first\n\nthird");
    }

    #[test]
    fn commit_edit_after_first_write_can_edit_following_lines() {
        // Regression for the empty-file flow: after the placeholder commit,
        // source_lines must mirror the file so a subsequent edit lands on
        // the right line.
        let tmp = make_tmp();
        let file = tmp.path().join("new.txt");
        std::fs::write(&file, "").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("new.txt");
        p.fetch();

        assert!(p.commit_edit("", "first\nsecond"));

        // After the placeholder commit the file has two lines (src=0,1).
        let old = format!("<input>{}second</input>", tags::format_src(1));
        assert!(p.commit_edit(&old, "SECOND"));

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "first\nSECOND");
    }

    #[test]
    fn commit_edit_replace_with_empty_string_blanks_line() {
        // Replacing a line with empty input keeps the slot but blanks it out.
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\nbeta\ngamma\n").unwrap();

        let mut p = make_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        let old = format!("<input>{}beta</input>", tags::format_src(1));
        assert!(p.commit_edit(&old, ""));

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "alpha\n\ngamma\n");
    }
}
