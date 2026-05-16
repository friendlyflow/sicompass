//! Text editor provider — file-rooted code/text editor.
//!
//! Implements the [`Provider`] trait as a normal plugin.
//! The provider navigates a filesystem tree rooted at the user-configurable
//! `textEditorPath` setting and, when the user enters a file, parses its content
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
//! When no `textEditorPath` is configured the provider defaults to the user's
//! home directory.

mod parse;

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::manifest::{BuiltinManifest, SettingDecl};
use sicompass_sdk::provider::Provider;
use sicompass_sdk::tags;
use sicompass_sdk::timeline::{FsOpKind, FsSideEffect, TimelineEntry};
use sicompass_sdk::{register_builtin_manifest, register_provider_factory};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// TextEditorProvider
// ---------------------------------------------------------------------------

pub struct TextEditorProvider {
    /// The root path shown when navigating to the editor.
    text_editor_path: String,
    /// Current filesystem position (rooted at `text_editor_path`).
    current_fs_path: PathBuf,
    /// Path within a file's parsed FFON tree (non-empty when inside a file).
    ffon_sub_path: Vec<String>,
    /// Set to `true` when `text_editor_path` changes so the app re-fetches the FFON.
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
    /// Unified-timeline emission queue, drained by the app after each action.
    /// Populated by `delete_item` (in-file line deletes and file/folder
    /// trashing) so deletions land on the undo timeline.
    pending_timeline_entries: Vec<TimelineEntry>,
}

impl TextEditorProvider {
    pub fn new() -> Self {
        let text_editor_path = home_dir();
        let current_fs_path = PathBuf::from(&text_editor_path);
        let current_path_str = text_editor_path.clone();
        TextEditorProvider {
            text_editor_path,
            current_fs_path,
            ffon_sub_path: Vec::new(),
            refresh_pending: false,
            current_path_str,
            source_lines: Vec::new(),
            cached_ffon: Vec::new(),
            loaded_path: None,
            trailing_newline: false,
            pending_timeline_entries: Vec::new(),
        }
    }

    /// Reload the open file when the cached `source_lines` no longer correspond
    /// to `current_fs_path` — e.g. an undo runs after navigation cleared the
    /// cache. Without this, a delete-line undo would splice into an empty
    /// `source_lines` and flush a truncated file.
    fn ensure_file_loaded(&mut self) {
        if self.loaded_path.as_deref() != Some(self.current_fs_path.as_path()) {
            self.load_file();
        }
    }

    /// Queue an `insert_lines` timeline entry for `lines` spliced in at
    /// source-line index `clamp`. Shared by the in-file insert (`<srcins=N>`)
    /// and the empty-file first-write paths of `commit_edit`. The `<src=N>`
    /// payload prefix records the position; the rest is the inserted lines
    /// joined by `\n`, so undo can remove exactly them and redo splice back.
    fn push_insert_lines_entry(&mut self, clamp: usize, lines: &[String]) {
        let payload_text =
            format!("{}{}", tags::format_src(clamp), lines.join("\n"));
        self.pending_timeline_entries.push(TimelineEntry::ProviderOp {
            provider_idx: 0, // patched by app
            command: "texteditor.insert_lines".to_owned(),
            payload: FfonElement::new_str(payload_text),
            label: if lines.len() == 1 {
                format!("insert line {}", clamp + 1)
            } else {
                format!("insert {} lines at {}", lines.len(), clamp + 1)
            },
        });
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
        PathBuf::from(&self.text_editor_path)
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

impl Default for TextEditorProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for TextEditorProvider {
    fn name(&self) -> &str { "texteditor" }

    fn display_name(&self) -> &str { "text editor" }

    fn init(&mut self) {
        // Read saved textEditorPath from config so the first fetch() shows the
        // correct directory rather than the home-dir default.
        if let Some(path) = sicompass_sdk::platform::main_config_path() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(root) = serde_json::from_str::<serde_json::Value>(&data) {
                    if let Some(val) = root
                        .get("text editor")
                        .and_then(|s| s.get("textEditorPath"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        self.text_editor_path = val.to_string();
                    }
                }
            }
        }
        self.current_fs_path = PathBuf::from(&self.text_editor_path);
        self.ffon_sub_path.clear();
        self.refresh_pending = false;
        self.loaded_path = None;
        self.source_lines.clear();
        self.cached_ffon.clear();
        self.pending_timeline_entries.clear();
        self.sync_path_str();
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        if self.text_editor_path.trim().is_empty() {
            return vec![FfonElement::new_str("set text editor path in settings")];
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
            self.source_lines.splice(clamp..clamp, new_lines.iter().cloned());
            if !self.flush_source_lines() {
                return false;
            }
            self.push_insert_lines_entry(clamp, &new_lines);
            return true;
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
            self.source_lines.splice(pos..pos, new_lines.iter().cloned());
            if !self.flush_source_lines() {
                return false;
            }
            // The first write into an empty file is a line insert, not a file
            // creation — record it as such so undo/redo restore the content.
            self.push_insert_lines_entry(pos, &new_lines);
            return true;
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
            if line_idx >= self.source_lines.len() {
                return false;
            }
            // Capture the verbatim line (indentation included) so undo can
            // restore it exactly. The recorded `<src=N>` prefix encodes the
            // index; the rest is the raw line text.
            let verbatim = self.source_lines[line_idx].clone();
            self.source_lines.remove(line_idx);
            if !self.flush_source_lines() {
                return false;
            }
            let payload = FfonElement::new_str(format!(
                "{}{}",
                tags::format_src(line_idx),
                verbatim,
            ));
            self.pending_timeline_entries.push(TimelineEntry::ProviderOp {
                provider_idx: 0, // patched by app
                command: "texteditor.delete_line".to_owned(),
                payload,
                label: format!("delete line {}", line_idx + 1),
            });
            return true;
        }

        // Directory view: move file/folder to OS trash.
        let clean = tags::strip_display(name);
        let clean = clean.trim_end_matches('/').trim_end_matches('\\');
        if clean.is_empty() { return false; }
        let full = self.current_fs_path.join(clean);
        // Snapshot before trashing so an undo can restore even if the OS trash
        // is later emptied (see `sicompass_sdk::fs_trash`).
        let side_effect = sicompass_sdk::fs_trash::snapshot_for_delete(&full);
        if trash::delete(&full).is_err() {
            return false;
        }
        // The text editor lists *both* files and directories as navigable
        // `Obj`s (see `list_directory`), so an undo must reinsert the entry as
        // an `Obj`. A bare `Str` would be unnavigable, and a later descent
        // into it would resolve to nothing — a blank list.
        let before_elem = FfonElement::new_obj(clean);
        self.pending_timeline_entries.push(TimelineEntry::FsOp {
            provider_idx: 0, // patched by app
            id: sicompass_sdk::ffon::IdArray::new(),
            op: FsOpKind::Delete,
            before: Some(before_elem),
            after: None,
            side_effect,
        });
        true
    }

    fn take_timeline_entries(&mut self) -> Vec<TimelineEntry> {
        std::mem::take(&mut self.pending_timeline_entries)
    }

    fn undo(&mut self, entry: &TimelineEntry, error: &mut String) {
        match entry {
            TimelineEntry::FsOp { op: FsOpKind::Delete, side_effect, .. } => {
                sicompass_sdk::fs_trash::restore_side_effect(side_effect, error);
            }
            TimelineEntry::ProviderOp { command, payload, .. }
                if command == "texteditor.delete_line" =>
            {
                if let Some((idx, verbatim)) = decode_line_payload(payload) {
                    self.ensure_file_loaded();
                    let clamp = idx.min(self.source_lines.len());
                    self.source_lines.splice(clamp..clamp, [verbatim]);
                    if !self.flush_source_lines() {
                        *error = "undo delete line: failed to write file".to_owned();
                    }
                }
            }
            TimelineEntry::ProviderOp { command, payload, .. }
                if command == "texteditor.insert_lines" =>
            {
                // Undo a line insert: remove the inserted lines back off disk.
                if let Some((idx, joined)) = decode_line_payload(payload) {
                    let count = joined.split('\n').count();
                    self.ensure_file_loaded();
                    let start = idx.min(self.source_lines.len());
                    let end = (start + count).min(self.source_lines.len());
                    self.source_lines.drain(start..end);
                    if !self.flush_source_lines() {
                        *error = "undo insert line: failed to write file".to_owned();
                    }
                }
            }
            _ => {}
        }
    }

    fn redo(&mut self, entry: &TimelineEntry, error: &mut String) {
        match entry {
            TimelineEntry::FsOp { op: FsOpKind::Delete, side_effect, .. } => {
                // Re-trash at the absolute original path recorded in the side
                // effect; the cursor may have moved since the delete.
                let path: Option<&Path> = match side_effect {
                    FsSideEffect::TrashedFile { original_path, .. }
                    | FsSideEffect::TrashedDir { original_path, .. } => Some(original_path),
                    FsSideEffect::RenameOnly { from, .. } => Some(from),
                    FsSideEffect::None => None,
                };
                if let Some(path) = path {
                    if let Err(e) = trash::delete(path) {
                        *error = format!("redo delete: trash failed: {e}");
                    }
                }
            }
            TimelineEntry::ProviderOp { command, payload, .. }
                if command == "texteditor.delete_line" =>
            {
                if let Some((idx, _)) = decode_line_payload(payload) {
                    self.ensure_file_loaded();
                    if idx < self.source_lines.len() {
                        self.source_lines.remove(idx);
                        if !self.flush_source_lines() {
                            *error = "redo delete line: failed to write file".to_owned();
                        }
                    }
                }
            }
            TimelineEntry::ProviderOp { command, payload, .. }
                if command == "texteditor.insert_lines" =>
            {
                // Redo a line insert: splice the recorded lines back in.
                if let Some((idx, joined)) = decode_line_payload(payload) {
                    self.ensure_file_loaded();
                    let lines: Vec<String> =
                        joined.split('\n').map(str::to_owned).collect();
                    let clamp = idx.min(self.source_lines.len());
                    self.source_lines.splice(clamp..clamp, lines);
                    if !self.flush_source_lines() {
                        *error = "redo insert line: failed to write file".to_owned();
                    }
                }
            }
            _ => {}
        }
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
        if key == "textEditorPath" && value != self.text_editor_path {
            self.text_editor_path = value.to_string();
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

/// Decode a `texteditor.delete_line` / `texteditor.insert_lines` timeline
/// payload into its `(line_index, text)` pair. The payload key is `<src=N>text`,
/// optionally wrapped in `<input>…</input>`. For an insert the `text` is the
/// inserted lines joined by `\n`; for a delete it is the single removed line.
fn decode_line_payload(payload: &FfonElement) -> Option<(usize, String)> {
    let key = match payload {
        FfonElement::Str(s) => s.as_str(),
        FfonElement::Obj(o) => o.key.as_str(),
    };
    let inner = tags::extract_input(key).unwrap_or_else(|| key.to_owned());
    tags::extract_src(&inner).map(|(idx, text)| (idx, text.to_owned()))
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

/// Register the text editor provider with the SDK factory and manifest registries.
pub fn register() {
    let home = home_dir();
    register_provider_factory("texteditor", || Box::new(TextEditorProvider::new()));
    register_builtin_manifest(
        BuiltinManifest::new("texteditor", "text editor")
            .with_settings(vec![
                SettingDecl::text("text editor", "text editor path", "textEditorPath", &home),
            ]),
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sicompass_sdk::placeholders::I_PLACEHOLDER;
    use tempfile::TempDir;

    fn make_tmp() -> TempDir {
        TempDir::new().expect("tempdir")
    }

    fn make_text_editor(tmp: &TempDir) -> TextEditorProvider {
        let mut p = TextEditorProvider::new();
        p.on_setting_change("textEditorPath", tmp.path().to_str().unwrap());
        p
    }

    // ---- basic fetch / navigation ------------------------------------------

    #[test]
    fn fetch_with_empty_path_returns_hint() {
        let mut p = TextEditorProvider {
            text_editor_path: String::new(),
            current_fs_path: PathBuf::new(),
            ffon_sub_path: vec![],
            refresh_pending: false,
            current_path_str: String::new(),
            source_lines: vec![],
            cached_ffon: vec![],
            loaded_path: None,
            trailing_newline: false,
            pending_timeline_entries: vec![],
        };
        let items = p.fetch();
        assert_eq!(items.len(), 1);
        assert!(items[0].as_str().unwrap().contains("set text editor path"));
    }

    #[test]
    fn on_setting_change_updates_text_editor_path() {
        let mut p = TextEditorProvider::new();
        p.on_setting_change("textEditorPath", "/tmp/test");
        assert_eq!(p.text_editor_path, "/tmp/test");
        assert_eq!(p.current_fs_path, PathBuf::from("/tmp/test"));
    }

    #[test]
    fn on_setting_change_sets_refresh_pending() {
        let mut p = TextEditorProvider::new();
        assert!(!p.needs_refresh());
        p.on_setting_change("textEditorPath", "/tmp/newpath");
        assert!(p.needs_refresh(), "changing the path must set refresh_pending");
    }

    #[test]
    fn on_setting_change_same_path_does_not_set_refresh() {
        let mut p = TextEditorProvider::new();
        let same = p.text_editor_path.clone();
        p.on_setting_change("textEditorPath", &same);
        assert!(!p.needs_refresh(), "same path must not trigger a refresh");
    }

    #[test]
    fn clear_needs_refresh_clears_flag() {
        let mut p = TextEditorProvider::new();
        p.on_setting_change("textEditorPath", "/tmp/other");
        assert!(p.needs_refresh());
        p.clear_needs_refresh();
        assert!(!p.needs_refresh());
    }

    #[test]
    fn on_setting_change_ignores_other_keys() {
        let mut p = TextEditorProvider::new();
        let original = p.text_editor_path.clone();
        p.on_setting_change("someOtherKey", "/etc");
        assert_eq!(p.text_editor_path, original);
        assert!(!p.needs_refresh());
    }

    #[test]
    fn current_path_includes_ffon_sub_path() {
        let tmp = make_tmp();
        let file = tmp.path().join("data.txt");
        std::fs::write(&file, "section:\n{\n  child\n}").unwrap();

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
        p.push_path("inner");
        assert!(p.current_fs_path.ends_with("inner"));
        p.pop_path();
        assert_eq!(p.current_fs_path, PathBuf::from(tmp.path()));
    }

    #[test]
    fn pop_path_cannot_go_above_root() {
        let tmp = make_tmp();
        let mut p = make_text_editor(&tmp);
        p.pop_path();
        assert_eq!(p.current_fs_path, PathBuf::from(tmp.path()));
    }

    #[test]
    fn fetch_lists_directory_entries_with_input_wrappers() {
        let tmp = make_tmp();
        std::fs::write(tmp.path().join("hello.txt"), "hi").unwrap();
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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
    fn text_editor_provider_has_editor_semantics() {
        let p = TextEditorProvider::new();
        assert!(p.has_editor_semantics());
    }

    // ---- directory create / delete / rename --------------------------------

    #[test]
    fn create_file_creates_on_disk() {
        let tmp = make_tmp();
        let mut p = make_text_editor(&tmp);
        assert!(p.create_file("new.txt"));
        assert!(tmp.path().join("new.txt").exists());
    }

    #[test]
    fn create_directory_creates_on_disk() {
        let tmp = make_tmp();
        let mut p = make_text_editor(&tmp);
        assert!(p.create_directory("mydir"));
        assert!(tmp.path().join("mydir").is_dir());
    }

    #[test]
    fn create_file_rejects_empty_name() {
        let tmp = make_tmp();
        let mut p = make_text_editor(&tmp);
        assert!(!p.create_file(""));
    }

    #[test]
    fn create_directory_rejects_empty_name() {
        let tmp = make_tmp();
        let mut p = make_text_editor(&tmp);
        assert!(!p.create_directory(""));
    }

    #[test]
    fn delete_item_moves_file_to_trash() {
        let tmp = make_tmp();
        let file = tmp.path().join("bye.txt");
        std::fs::write(&file, "").unwrap();
        let mut p = make_text_editor(&tmp);
        assert!(p.delete_item("<input>bye.txt</input>"));
        assert!(!file.exists());
    }

    #[test]
    fn commit_edit_renames_file() {
        let tmp = make_tmp();
        let file = tmp.path().join("old.txt");
        std::fs::write(&file, "").unwrap();
        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
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

        let mut p = make_text_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        let old = format!("<input>{}beta</input>", tags::format_src(1));
        assert!(p.commit_edit(&old, ""));

        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "alpha\n\ngamma\n");
    }

    // ---- delete timeline emission + undo/redo -------------------------------

    #[test]
    fn delete_item_emits_fsop_with_file_snapshot() {
        let tmp = make_tmp();
        std::fs::write(tmp.path().join("doomed.txt"), b"important content").unwrap();
        let mut p = make_text_editor(&tmp);

        assert!(p.delete_item("<input>doomed.txt</input>"));
        assert!(!tmp.path().join("doomed.txt").exists());

        let entries = p.take_timeline_entries();
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            TimelineEntry::FsOp { op, side_effect, before, .. } => {
                assert_eq!(*op, FsOpKind::Delete);
                // The text editor lists files as navigable `Obj`s, so the
                // restore element must be an `Obj` (not a bare `Str`).
                assert!(matches!(before, Some(FfonElement::Obj(_))));
                match side_effect {
                    FsSideEffect::TrashedFile { content_snapshot, .. } => {
                        assert_eq!(content_snapshot, b"important content");
                    }
                    other => panic!("expected TrashedFile, got {other:?}"),
                }
            }
            other => panic!("expected FsOp, got {other:?}"),
        }
    }

    #[test]
    fn delete_item_emits_fsop_with_dir_snapshot() {
        let tmp = make_tmp();
        let dir = tmp.path().join("doomed_dir");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("inner.txt"), b"nested").unwrap();
        let mut p = make_text_editor(&tmp);

        assert!(p.delete_item("<input>doomed_dir</input>"));
        assert!(!dir.exists());

        let entries = p.take_timeline_entries();
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            TimelineEntry::FsOp { op, side_effect, before, .. } => {
                assert_eq!(*op, FsOpKind::Delete);
                assert!(matches!(before, Some(FfonElement::Obj(_))));
                assert!(matches!(side_effect, FsSideEffect::TrashedDir { .. }));
            }
            other => panic!("expected FsOp, got {other:?}"),
        }
    }

    #[test]
    fn undo_fsop_delete_restores_file() {
        let tmp = make_tmp();
        let target = tmp.path().join("doomed.txt");
        std::fs::write(&target, b"restore me").unwrap();
        let mut p = make_text_editor(&tmp);

        assert!(p.delete_item("<input>doomed.txt</input>"));
        assert!(!target.exists());

        let entries = p.take_timeline_entries();
        let mut err = String::new();
        p.undo(&entries[0], &mut err);
        assert!(err.is_empty(), "undo error: {err}");
        assert_eq!(std::fs::read(&target).unwrap(), b"restore me");
    }

    #[test]
    fn undo_fsop_delete_restores_directory_tree() {
        let tmp = make_tmp();
        let dir = tmp.path().join("a");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("inner.txt"), b"nested").unwrap();
        let sub = dir.join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("deep.txt"), b"deeper").unwrap();
        let mut p = make_text_editor(&tmp);

        assert!(p.delete_item("<input>a</input>"));
        assert!(!dir.exists());

        let entries = p.take_timeline_entries();
        let mut err = String::new();
        p.undo(&entries[0], &mut err);
        assert!(err.is_empty(), "undo error: {err}");
        assert!(dir.is_dir());
        assert_eq!(std::fs::read(dir.join("inner.txt")).unwrap(), b"nested");
        assert_eq!(std::fs::read(sub.join("deep.txt")).unwrap(), b"deeper");
    }

    #[test]
    fn redo_fsop_delete_removes_file_again() {
        let tmp = make_tmp();
        let target = tmp.path().join("doomed.txt");
        std::fs::write(&target, b"x").unwrap();
        let mut p = make_text_editor(&tmp);

        assert!(p.delete_item("<input>doomed.txt</input>"));
        let entries = p.take_timeline_entries();
        let mut err = String::new();
        p.undo(&entries[0], &mut err);
        assert!(target.exists());
        p.redo(&entries[0], &mut err);
        assert!(err.is_empty(), "redo error: {err}");
        assert!(!target.exists(), "redo deletes the file again");
    }

    #[test]
    fn delete_item_line_emits_provider_op() {
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\nbeta\ngamma").unwrap();
        let mut p = make_text_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch(); // prime source_lines

        let name = format!("<input>{}beta</input>", tags::format_src(1));
        assert!(p.delete_item(&name));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\ngamma");

        let entries = p.take_timeline_entries();
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            TimelineEntry::ProviderOp { command, .. } => {
                assert_eq!(command, "texteditor.delete_line");
            }
            other => panic!("expected ProviderOp, got {other:?}"),
        }
    }

    #[test]
    fn undo_line_delete_restores_line_verbatim() {
        // The deleted line carries indentation — undo must restore it exactly,
        // without routing through commit_edit (which re-applies indent).
        let tmp = make_tmp();
        let file = tmp.path().join("py.py");
        std::fs::write(&file, "def foo():\n    pass\n    return 1\n").unwrap();
        let mut p = make_text_editor(&tmp);
        p.push_path("py.py");
        p.fetch();

        // Delete line 1 ("    pass").
        let name = format!("<input>{}pass</input>", tags::format_src(1));
        assert!(p.delete_item(&name));
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "def foo():\n    return 1\n"
        );

        let entries = p.take_timeline_entries();
        let mut err = String::new();
        p.undo(&entries[0], &mut err);
        assert!(err.is_empty(), "undo error: {err}");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "def foo():\n    pass\n    return 1\n"
        );
    }

    #[test]
    fn redo_line_delete_removes_line_again() {
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\nbeta\ngamma").unwrap();
        let mut p = make_text_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        let name = format!("<input>{}beta</input>", tags::format_src(1));
        assert!(p.delete_item(&name));
        let entries = p.take_timeline_entries();
        let mut err = String::new();
        p.undo(&entries[0], &mut err);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\nbeta\ngamma");
        p.redo(&entries[0], &mut err);
        assert!(err.is_empty(), "redo error: {err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\ngamma");
    }

    #[test]
    fn line_delete_undo_redo_round_trip() {
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "a\nb\nc").unwrap();
        let mut p = make_text_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        let name = format!("<input>{}b</input>", tags::format_src(1));
        assert!(p.delete_item(&name));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a\nc");

        let entries = p.take_timeline_entries();
        let mut err = String::new();
        p.undo(&entries[0], &mut err);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a\nb\nc");
        p.redo(&entries[0], &mut err);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a\nc");
        p.undo(&entries[0], &mut err);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a\nb\nc");
        assert!(err.is_empty(), "undo/redo error: {err}");
    }

    // ---- line-insert timeline emission + undo/redo --------------------------

    #[test]
    fn commit_edit_insert_emits_provider_op() {
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\ngamma").unwrap();
        let mut p = make_text_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        let old = format!("<input>{}</input>", tags::format_src_insert(1));
        assert!(p.commit_edit(&old, "beta"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\nbeta\ngamma");

        let entries = p.take_timeline_entries();
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            TimelineEntry::ProviderOp { command, .. } => {
                assert_eq!(command, "texteditor.insert_lines");
            }
            other => panic!("expected ProviderOp, got {other:?}"),
        }
    }

    #[test]
    fn undo_line_insert_removes_line() {
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\ngamma").unwrap();
        let mut p = make_text_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        let old = format!("<input>{}</input>", tags::format_src_insert(1));
        assert!(p.commit_edit(&old, "beta"));
        let entries = p.take_timeline_entries();
        let mut err = String::new();
        p.undo(&entries[0], &mut err);
        assert!(err.is_empty(), "undo error: {err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\ngamma");
    }

    #[test]
    fn redo_line_insert_re_adds_line() {
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\ngamma").unwrap();
        let mut p = make_text_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        let old = format!("<input>{}</input>", tags::format_src_insert(1));
        assert!(p.commit_edit(&old, "beta"));
        let entries = p.take_timeline_entries();
        let mut err = String::new();
        p.undo(&entries[0], &mut err);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\ngamma");
        p.redo(&entries[0], &mut err);
        assert!(err.is_empty(), "redo error: {err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\nbeta\ngamma");
    }

    #[test]
    fn line_insert_undo_redo_multiline() {
        // A multi-line insert (Ctrl+Enter input with embedded `\n`) must undo
        // and redo as one atomic step covering every inserted line.
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\ngamma").unwrap();
        let mut p = make_text_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        let old = format!("<input>{}</input>", tags::format_src_insert(1));
        assert!(p.commit_edit(&old, "b1\nb2"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\nb1\nb2\ngamma");

        let entries = p.take_timeline_entries();
        let mut err = String::new();
        p.undo(&entries[0], &mut err);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\ngamma");
        p.redo(&entries[0], &mut err);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\nb1\nb2\ngamma");
        assert!(err.is_empty(), "undo/redo error: {err}");
    }

    #[test]
    fn insert_blank_line_undo_redo() {
        // Ctrl+I then Enter with no text inserts one blank line; undo/redo must
        // treat the empty payload as exactly one line.
        let tmp = make_tmp();
        let file = tmp.path().join("sample.txt");
        std::fs::write(&file, "alpha\ngamma\n").unwrap();
        let mut p = make_text_editor(&tmp);
        p.push_path("sample.txt");
        p.fetch();

        let old = format!("<input>{}</input>", tags::format_src_insert(1));
        assert!(p.commit_edit(&old, ""));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\n\ngamma\n");

        let entries = p.take_timeline_entries();
        let mut err = String::new();
        p.undo(&entries[0], &mut err);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\ngamma\n");
        p.redo(&entries[0], &mut err);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\n\ngamma\n");
        assert!(err.is_empty(), "undo/redo error: {err}");
    }

    #[test]
    fn commit_edit_first_line_of_empty_file_emits_provider_op() {
        // The first write into an empty file is a line insert, not a file
        // creation — it must emit an `insert_lines` entry.
        let tmp = make_tmp();
        let file = tmp.path().join("new.txt");
        std::fs::write(&file, "").unwrap();
        let mut p = make_text_editor(&tmp);
        p.push_path("new.txt");
        p.fetch();

        assert!(p.commit_edit("", "hello"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");

        let entries = p.take_timeline_entries();
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            TimelineEntry::ProviderOp { command, .. } => {
                assert_eq!(command, "texteditor.insert_lines");
            }
            other => panic!("expected ProviderOp, got {other:?}"),
        }
    }

    #[test]
    fn first_line_write_undo_redo() {
        let tmp = make_tmp();
        let file = tmp.path().join("new.txt");
        std::fs::write(&file, "").unwrap();
        let mut p = make_text_editor(&tmp);
        p.push_path("new.txt");
        p.fetch();

        assert!(p.commit_edit("", "hello"));
        let entries = p.take_timeline_entries();
        let mut err = String::new();
        p.undo(&entries[0], &mut err);
        assert!(err.is_empty(), "undo error: {err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "");
        p.redo(&entries[0], &mut err);
        assert!(err.is_empty(), "redo error: {err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");
    }
}
