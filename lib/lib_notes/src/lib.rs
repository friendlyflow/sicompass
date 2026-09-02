//! The notes provider: a tree of the user's own writing, hashed for sync.
//!
//! # Two invariants, both easy to break by accident
//!
//! 1. **Every row is escaped.** The app parses `<input>`, `<button>`, `<link>`
//!    and friends out of *any* string it renders, so a note titled
//!    `<button>x</button>` would grow a live control that runs on Enter. Note
//!    text is the most user-controlled string in the app; it goes through
//!    [`escape::escape`] before it becomes a label, always.
//!
//! 2. **The `<id>` sits outside the `<input>`, never inside.** The app rewrites
//!    a row as `prefix + <input>buffer</input> + suffix` on every keystroke,
//!    and it strips internal tags out of the edit buffer. An id inside the
//!    wrapper would therefore vanish on the first character typed; one in the
//!    prefix survives, which is what lets a rename find its target.
//!
//! # Where writes come from
//!
//! Almost nowhere in this file. The provider declares
//! `supports_structural_edit()`, so the app owns the mutation: it edits its own
//! FFON tree from the keypress, records the `Structural` entry that makes
//! ctrl-Z work, and hands the result back through
//! [`Provider::sync_ffon_body_children`]. That one method is the write path for
//! inserts, deletes, cuts, pastes, renames, undo and redo alike — it diffs the
//! list it is given against the tree by id, and saves.
//!
//! The consequence worth stating: this provider implements no `undo`/`redo`.
//! There is nothing for it to reverse, because it never applied anything the
//! app did not already record.

mod escape;
pub mod store;
pub mod tree;

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::{
    BuiltinManifest, Provider, localize, register_builtin_manifest, register_provider_factory, tags,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use tree::{Node, NodeId, Tree, Visibility};

// ---------------------------------------------------------------------------
// Test stub: never touch the user's real notes from a test.
//
// The failure this prevents is not a flaky test, it is data loss. `save_tree`
// reconciles a directory against a tree, so a test that builds a two-node tree
// and saves it to the real notes directory does not add two notes — it deletes
// everything else.
//
// Two audiences, hence a compile-time default and a runtime setter:
//
// * This crate's own unit tests get it free from `cfg!(test)`. Setting the
//   per-instance `root_override` is the right way to make a test safe, but
//   forgetting it must fail closed rather than silently reaching the real
//   store.
// * The app's integration tests are a different binary, where this crate is an
//   ordinary dependency compiled *without* `cfg(test)`, and they reach the
//   provider as a `Box<dyn Provider>` with no way to set the override. They
//   call `_set_test_no_persist(true)` once per binary instead.
// ---------------------------------------------------------------------------

static TEST_NO_PERSIST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(cfg!(test));

#[doc(hidden)]
pub fn _set_test_no_persist(enabled: bool) {
    TEST_NO_PERSIST.store(enabled, std::sync::atomic::Ordering::Release);
}

#[inline]
fn test_no_persist() -> bool {
    TEST_NO_PERSIST.load(std::sync::atomic::Ordering::Acquire)
}

/// Register this crate's Fluent bundles. Idempotent.
///
/// Called from `register()` *and* from every trait method that resolves a
/// string: a provider built through the factory can be reached before
/// `register()` has run on some paths, and an unresolved key renders as the key.
pub fn register_translations() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = localize::register_bundle("en-US", include_str!("../locales/en-US.ftl"));
        let _ = localize::register_bundle("nl-BE", include_str!("../locales/nl-BE.ftl"));
        let _ = localize::register_bundle("fr-BE", include_str!("../locales/fr-BE.ftl"));
        let _ = localize::register_bundle("de-BE", include_str!("../locales/de-BE.ftl"));
    });
}

// ---------------------------------------------------------------------------
// Command ids
//
// Stable identifiers, matched by equality in `handle_command`. The app's palette
// renders these raw strings.
//
// `"delete"` is deliberately absent. It is a reserved id: `avail_provider_has_delete`
// would claim both Ctrl+D and Delete and route them to `invoke_provider_delete`,
// which unwinds the cursor to depth 3 before deleting — email-shaped surgery that
// would maul a deep note tree.
// ---------------------------------------------------------------------------

pub const CMD_MOVE_UP: &str = "move up";
pub const CMD_MOVE_DOWN: &str = "move down";
pub const CMD_DUPLICATE: &str = "duplicate";

// ---------------------------------------------------------------------------
// Path model
// ---------------------------------------------------------------------------

/// One step down the tree.
///
/// Keyed on the node's id rather than its position, because a position stops
/// meaning the same node the moment a sibling is inserted above it — and the
/// app persists and restores `current_path()` across a refresh, an undo and a
/// restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Segment {
    Node(NodeId),
    /// The `list meta:` row, which is a rendered header rather than a node.
    Meta,
}

impl Segment {
    fn token(self) -> String {
        match self {
            Segment::Node(id) => format!("n{id}"),
            Segment::Meta => "m".to_owned(),
        }
    }

    fn from_token(tok: &str) -> Option<Segment> {
        if tok == "m" {
            return Some(Segment::Meta);
        }
        tok.strip_prefix('n')
            .and_then(|n| n.parse().ok())
            .map(Segment::Node)
    }
}

/// A row the user typed into an insert placeholder, not yet in the tree.
#[derive(Debug, Clone)]
struct PendingCreate {
    /// Exactly what `commit_edit` was handed, so the row can be recognised.
    typed: String,
    /// The text to store, with a trailing `:` removed.
    text: String,
    is_branch: bool,
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct NotesProvider {
    tree: Tree,
    segments: Vec<Segment>,
    /// `current_path()` hands back a borrow, so the rendered form is a field.
    rendered_path: String,
    /// Label to [`Segment`], per level. `push_path` is handed the text the user
    /// saw, so the code that rendered the labels is what answers for them.
    labels: HashMap<Vec<Segment>, HashMap<String, Segment>>,
    /// Per-instance store location. Wins over everything, including the test
    /// kill switch, so a tempdir-backed test is unaffected by it.
    root_override: Option<PathBuf>,
    /// What the user just typed into an insert placeholder, if anything.
    ///
    /// The app's create flow commits the text and only *then* re-fetches to
    /// find out what shape the new row is, so at sync time the tree still holds
    /// the placeholder. `commit_edit` is where the intent is legible — a
    /// trailing colon means "a branch" — so it is remembered here and applied
    /// when the list comes back. Same idea as `lib_gitclient`'s `Pending`.
    pending_create: Option<PendingCreate>,
    /// Set when the store could not be read. While this is true nothing is
    /// written, because reconciling against a tree we failed to load would
    /// delete the notes we failed to read.
    load_failed: bool,
    loaded: bool,
    error: Option<String>,
}

impl Default for NotesProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl NotesProvider {
    pub fn new() -> Self {
        NotesProvider {
            tree: Tree::new(),
            segments: Vec::new(),
            rendered_path: "/".to_owned(),
            labels: HashMap::new(),
            root_override: None,
            pending_create: None,
            load_failed: false,
            loaded: false,
            error: None,
        }
    }

    /// Where the notes live. `None` means nowhere usable, and the tree stays in
    /// memory for the session rather than being silently discarded.
    ///
    /// Deliberately `data_home()` and not `state_home()`: on macOS the latter is
    /// `~/Library/Logs`, which the OS and every cleanup tool treat as
    /// disposable. Notes are documents.
    fn root(&self) -> Option<PathBuf> {
        if let Some(p) = &self.root_override {
            return Some(p.clone());
        }
        if test_no_persist() {
            return None;
        }
        sicompass_sdk::platform::data_home().map(|d| d.join("sicompass").join("notes"))
    }

    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(root) = self.root() else {
            return;
        };
        match store::load_tree(&root) {
            Some(t) => self.tree = t,
            None => {
                self.load_failed = true;
                self.error = Some(localize::t("notes-error-unreadable"));
            }
        }
    }

    fn save(&mut self) {
        if self.load_failed {
            return;
        }
        let Some(root) = self.root() else {
            return;
        };
        if let Err(e) = store::save_tree(&root, &self.tree) {
            self.error = Some(format!("{}: {e}", localize::t("notes-error-save")));
        }
    }

    /// The node-id chain for the current level, ignoring a trailing `Meta`.
    fn node_path(&self) -> Vec<NodeId> {
        self.segments
            .iter()
            .filter_map(|s| match s {
                Segment::Node(id) => Some(*id),
                Segment::Meta => None,
            })
            .collect()
    }

    fn in_meta(&self) -> bool {
        matches!(self.segments.last(), Some(Segment::Meta))
    }

    /// The top-level note the cursor is inside, which is what owns visibility.
    fn owning_note(&self) -> Option<&Node> {
        let first = self.segments.iter().find_map(|s| match s {
            Segment::Node(id) => Some(*id),
            Segment::Meta => None,
        })?;
        self.tree.notes.iter().find(|n| n.id == first)
    }

    fn sync_rendered_path(&mut self) {
        self.rendered_path = if self.segments.is_empty() {
            "/".to_owned()
        } else {
            format!(
                "/{}",
                self.segments
                    .iter()
                    .map(|s| s.token())
                    .collect::<Vec<_>>()
                    .join("/")
            )
        };
    }

    fn remember(&mut self, level: &[Segment], label: &str, seg: Segment) {
        self.labels
            .entry(level.to_vec())
            .or_default()
            .insert(label.to_owned(), seg);
    }

    fn forget_level(&mut self, level: &[Segment]) {
        self.labels.remove(level);
    }

    /// A row: `<id>N</id><input>text</input>`.
    ///
    /// The id is a *prefix*, outside the `<input>`. See the module docs — inside
    /// it, the app's live edit would eat it on the first keystroke.
    fn row_label(node: &Node) -> String {
        format!(
            "{}{}",
            tags::format_id(&node.id.to_string()),
            tags::format_input(&escape::escape(&node.text))
        )
    }

    fn row(node: &Node) -> FfonElement {
        let label = Self::row_label(node);
        if node.is_branch {
            FfonElement::new_obj(label)
        } else {
            FfonElement::Str(label)
        }
    }

    /// The `list meta:` header for the current level.
    ///
    /// Its key must stay localized, and must therefore never be literally
    /// `"meta"`: the app special-cases an Obj keyed exactly `"meta"` and skips
    /// `pop_path` when leaving it, which would leave this provider's path one
    /// segment deeper than the cursor.
    fn meta_row(&self) -> FfonElement {
        FfonElement::new_obj(localize::t("notes-list-meta"))
    }

    fn meta_children(&self) -> Vec<FfonElement> {
        let mut out = Vec::new();

        // Visibility belongs to the note, so it is offered on the note's own
        // list and nowhere deeper: `[Node(note), Meta]` is length two. The root
        // meta is `[Meta]`, length one, and a sublist's is longer; neither
        // carries the switch. A sublist inherits its note's setting, and the
        // root is not a note at all.
        if self.segments.len() == 2
            && let Some(note) = self.owning_note()
        {
            out.push(visibility_radio(
                note.visibility.unwrap_or(Visibility::Private),
            ));
        }

        let hash = self
            .current_list_owner_hash()
            .unwrap_or_else(|| self.tree.root_hash_hex());
        let mut args = localize::Args::new();
        args.set("hash", hash);
        out.push(FfonElement::new_str(localize::t_args(
            "notes-sha256",
            &args,
        )));
        out
    }

    /// The hash of the node whose children the cursor is looking at.
    fn current_list_owner_hash(&self) -> Option<String> {
        let path = self.node_path();
        let last = path.last()?;
        Node::find(&self.tree.notes, *last).map(|n| n.hash_hex())
    }

    fn level_children(&mut self) -> Vec<FfonElement> {
        let level = self.segments.clone();
        self.forget_level(&level);

        if self.in_meta() {
            return self.meta_children();
        }

        let path = self.node_path();
        let Some(nodes) = self.tree.list_at(&path).cloned() else {
            return vec![FfonElement::new_str(localize::t("notes-gone"))];
        };

        // Every list opens with its meta, the root included. At the root the
        // meta carries the tree's root hash and nothing else, so a glance at
        // the first row says whether anything anywhere below has changed —
        // which is the whole point of chaining the hashes.
        let mut out = Vec::with_capacity(nodes.len() + 1);
        let meta = self.meta_row();
        let key = meta.as_obj().map(|o| o.key.clone()).unwrap_or_default();
        self.remember(&level, &key, Segment::Meta);
        out.push(meta);
        for node in &nodes {
            let elem = Self::row(node);
            let key = match &elem {
                FfonElement::Str(s) => s.clone(),
                FfonElement::Obj(o) => o.key.clone(),
            };
            // `push_path` is handed the *stripped* label, so remember both
            // forms: the raw key for anything that echoes it back verbatim, and
            // the display text for the normal navigation path.
            self.remember(&level, &key, Segment::Node(node.id));
            self.remember(&level, &tags::strip_display(&key), Segment::Node(node.id));
            out.push(elem);
        }

        // Never empty. The app seeds its own insert placeholder into an empty
        // level, which would then look like a row the provider had rendered.
        if out.is_empty() {
            out.push(FfonElement::new_str(localize::t("notes-empty")));
        }
        out
    }

    // ---- The write path ---------------------------------------------------

    /// Reconcile one list against what the app says it now holds.
    ///
    /// Rows carry their `<id>`, so this is a diff rather than a guess: a row
    /// with a known id keeps its node (and its children) and takes the new
    /// text, a row with no id is new, and a node whose row is gone was deleted.
    /// Order is the order the app gave.
    ///
    /// That is what makes a rename unambiguous where `commit_edit(old, new)` is
    /// not — `old` is only the previous display text, so two identically titled
    /// siblings cannot be told apart through it.
    fn reconcile(&mut self, children: &[FfonElement]) {
        if self.load_failed {
            return;
        }
        // A meta row is rendered, not stored, and the app may hand it straight
        // back. Filtering it here is also what makes it un-reorderable: it is
        // re-inserted at index 0 on the next fetch no matter where it was.
        // Rows this provider renders but does not store. The app hands back
        // whatever it was displaying, so without this the "no notes yet" line
        // would become the user's first note the moment they wrote their
        // second one.
        let rendered_only = [
            localize::t("notes-list-meta"),
            localize::t("notes-empty"),
            localize::t("notes-gone"),
        ];
        // Which list is this? Not necessarily the one the cursor is on: undo
        // and redo hand back a list from wherever the reversed edit happened,
        // and they do not move the provider's path to match. The app cannot
        // supply it either — a provider's path is not in step with FFON depth.
        //
        // The rows answer it themselves. Any row that already has an id names a
        // node, and that node's parent chain is the list. Only a list whose
        // rows are all brand new falls back to the cursor, which is right,
        // because a list of nothing but new rows can only be the one the user
        // is typing into.
        let path = children
            .iter()
            .filter_map(|e| {
                let raw = match e {
                    FfonElement::Str(s) => s.as_str(),
                    FfonElement::Obj(o) => o.key.as_str(),
                };
                row_id(raw)
            })
            .find_map(|id| Node::path_to_parent_of(&self.tree.notes, id))
            .unwrap_or_else(|| self.node_path());

        let mut rebuilt: Vec<Node> = Vec::new();
        let mut fresh: Vec<usize> = Vec::new();
        for elem in children {
            let (raw, is_branch) = match elem {
                FfonElement::Str(s) => (s.clone(), false),
                FfonElement::Obj(o) => (o.key.clone(), true),
            };
            if rendered_only.contains(&raw) {
                continue;
            }
            let text = row_text(&raw);
            // The app's own "type here" placeholder, and the empty row it
            // leaves behind after deleting the last child. Neither is a note.
            if text.is_empty() && !is_branch {
                continue;
            }
            match row_id(&raw) {
                Some(id) => {
                    let existing = self
                        .tree
                        .list_at(&path)
                        .and_then(|l| l.iter().find(|n| n.id == id))
                        .cloned();
                    match existing {
                        Some(mut n) => {
                            n.text = text;
                            // A row only becomes a branch, never stops being
                            // one: `is_branch` on the FFON side is whether the
                            // app happens to hold children in memory, and a
                            // level the user has not visited holds none.
                            n.is_branch = n.is_branch || is_branch;
                            rebuilt.push(n);
                        }
                        None => {
                            rebuilt.push(Node {
                                id,
                                text,
                                is_branch,
                                children: Vec::new(),
                                visibility: None,
                            });
                        }
                    }
                }
                None => {
                    // A row with no id is new. If it is the one `commit_edit`
                    // just took, its shape is known from what was typed; the
                    // tree the app is handing back still holds the placeholder
                    // and cannot say.
                    let pending = self
                        .pending_create
                        .as_ref()
                        .filter(|p| p.typed == text || p.text == text)
                        .cloned();
                    let (text, is_branch) = match pending {
                        Some(p) => {
                            self.pending_create = None;
                            (p.text, p.is_branch || is_branch)
                        }
                        None => (text, is_branch),
                    };
                    fresh.push(rebuilt.len());
                    rebuilt.push(Node {
                        id: 0,
                        text,
                        is_branch,
                        children: Vec::new(),
                        visibility: None,
                    });
                }
            }
        }
        for i in fresh {
            rebuilt[i].id = self.tree.mint_id();
        }
        // A new top-level note is a new unit of sharing, and starts private.
        if path.is_empty() {
            for n in rebuilt.iter_mut() {
                if n.visibility.is_none() {
                    n.visibility = Some(Visibility::Private);
                }
            }
        }

        if let Some(list) = self.tree.list_at_mut(&path) {
            *list = rebuilt;
        }
        self.save();
    }
}

/// The `<radio>` group the user toggles to publish a note.
///
/// The option *labels* are localized; the values written to disk are not
/// (`Visibility::as_str`). A server reads those files, so `private` has to stay
/// `private` whatever language the app is in.
fn visibility_radio(current: Visibility) -> FfonElement {
    let mut radio = FfonElement::new_obj(format!("<radio>{}", localize::t("notes-visibility")));
    if let Some(o) = radio.as_obj_mut() {
        for v in [Visibility::Private, Visibility::Public] {
            let label = localize::t(match v {
                Visibility::Private => "notes-visibility-private",
                Visibility::Public => "notes-visibility-public",
            });
            o.push(FfonElement::Str(if v == current {
                tags::format_checked(&label)
            } else {
                label
            }));
        }
    }
    radio
}

/// The row's node id, read **only from the prefix**.
///
/// `tags::extract_id` scans the whole string, so calling it on a row directly
/// would let a note whose *text* is `<id>1</id>...` claim another note's
/// identity — and on the way back in, a freshly typed row is not escaped yet,
/// so the forged tag is live. The id this provider writes is always in front of
/// the `<input>`, so that is the only place worth looking.
fn row_id(raw: &str) -> Option<NodeId> {
    let prefix = match raw.find("<input>") {
        Some(at) => &raw[..at],
        None => raw,
    };
    tags::extract_id(prefix).and_then(|s| s.parse().ok())
}

/// The text of a row, with the `<id>` prefix and the `<input>` wrapper removed
/// and any escaping undone.
///
/// `extract_input` hands back the raw slice, escapes and all, so the unescape
/// belongs here. `strip_display` already unescapes, hence the two arms.
fn row_text(raw: &str) -> String {
    match tags::extract_input(raw) {
        Some(inner) => escape::unescape(&inner),
        None => tags::strip_display(raw),
    }
}

#[async_trait::async_trait]
impl Provider for NotesProvider {
    fn name(&self) -> &str {
        "notes"
    }

    fn display_name(&self) -> String {
        register_translations();
        localize::t("notes-display-name")
    }

    fn version(&self) -> Option<&str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        register_translations();
        self.ensure_loaded();
        self.level_children()
    }

    fn supports_structural_edit(&self) -> bool {
        true
    }

    /// Where every edit lands. See the module docs.
    fn sync_ffon_body_children(&mut self, children: &[FfonElement]) {
        register_translations();
        self.ensure_loaded();
        // The meta level holds rendered controls, not notes; a radio toggle
        // there arrives through `on_radio_change` instead.
        if self.in_meta() {
            return;
        }
        self.reconcile(children);
    }

    /// Accept the typed text, and veto an edit aimed at the meta row.
    ///
    /// The rename itself is done by `sync_ffon_body_children`, which the app
    /// calls right after this with the whole list and its ids.
    fn commit_edit(&mut self, old: &str, _new: &str) -> bool {
        register_translations();
        if self.load_failed {
            self.error = Some(localize::t("notes-error-unreadable"));
            return false;
        }
        if old == localize::t("notes-list-meta") {
            self.error = Some(localize::t("notes-error-meta-readonly"));
            return false;
        }
        if old.is_empty() {
            // A create. The app's convention, shared with the file browser: a
            // trailing colon (from typing `name:` or `+name`) asks for
            // something with children.
            let typed = _new.to_owned();
            let (text, is_branch) = match typed.strip_suffix(':') {
                Some(t) => (t.trim_end().to_owned(), true),
                None => (typed.clone(), false),
            };
            self.pending_create = Some(PendingCreate {
                typed,
                text,
                is_branch,
            });
        }
        true
    }

    /// The veto the app asks for before removing a row.
    ///
    /// `name` is the raw element text, tags intact, so the meta row is
    /// recognisable by its key.
    fn delete_item(&mut self, name: &str) -> bool {
        register_translations();
        if self.load_failed {
            self.error = Some(localize::t("notes-error-unreadable"));
            return false;
        }
        if name == localize::t("notes-list-meta") {
            self.error = Some(localize::t("notes-error-meta-undeletable"));
            return false;
        }
        // The removal itself is the app's; it reports the result back through
        // `sync_ffon_body_children`.
        true
    }

    fn on_radio_change(&mut self, _group: &str, value: &str) {
        register_translations();
        let private = localize::t("notes-visibility-private");
        let v = if value == private {
            Visibility::Private
        } else {
            Visibility::Public
        };
        let Some(note_id) = self.owning_note().map(|n| n.id) else {
            return;
        };
        if let Some(n) = Node::find_mut(&mut self.tree.notes, note_id) {
            n.visibility = Some(v);
        }
        self.save();
    }

    fn push_path(&mut self, segment: &str) {
        let level = self.segments.clone();
        let resolved = self
            .labels
            .get(&level)
            .and_then(|m| m.get(segment))
            .copied()
            .or_else(|| Segment::from_token(segment));
        if let Some(s) = resolved {
            self.segments.push(s);
            self.sync_rendered_path();
        }
    }

    fn pop_path(&mut self) {
        self.segments.pop();
        self.sync_rendered_path();
    }

    fn current_path(&self) -> &str {
        &self.rendered_path
    }

    fn set_current_path(&mut self, path: &str) {
        self.segments = path
            .split('/')
            .filter(|s| !s.is_empty())
            .filter_map(Segment::from_token)
            .collect();
        self.sync_rendered_path();
    }

    fn at_root(&self) -> bool {
        self.segments.is_empty()
    }

    /// Point the store somewhere else. The trait's own escape hatch for tests,
    /// and the only way the app's integration tests — a separate binary, where
    /// this crate is compiled without `cfg(test)` and reached as a
    /// `Box<dyn Provider>` — can keep away from the real notes directory.
    fn set_config_path(&mut self, path: PathBuf) {
        self.root_override = Some(path);
        self.loaded = false;
        self.load_failed = false;
    }

    /// The children of the level the cursor is on, so a commit refreshes just
    /// that list. Without this the app falls back to rebuilding the provider
    /// root, which misroutes a deep path and leaves the level empty.
    fn fetch_subtree_children(&mut self) -> Option<Vec<FfonElement>> {
        if self.segments.is_empty() {
            return None;
        }
        register_translations();
        self.ensure_loaded();
        Some(self.level_children())
    }

    fn commands(&self) -> Vec<String> {
        vec![
            CMD_MOVE_UP.to_owned(),
            CMD_MOVE_DOWN.to_owned(),
            CMD_DUPLICATE.to_owned(),
        ]
    }

    fn command_label(&self, cmd: &str) -> String {
        register_translations();
        match cmd {
            CMD_MOVE_UP => localize::t("notes-cmd-move-up"),
            CMD_MOVE_DOWN => localize::t("notes-cmd-move-down"),
            CMD_DUPLICATE => localize::t("notes-cmd-duplicate"),
            other => other.to_owned(),
        }
    }

    fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    fn supports_config_files(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register() {
    register_translations();
    register_provider_factory("notes", || Box::new(NotesProvider::new()));
    register_builtin_manifest(BuiltinManifest::new("notes", "notes").enable_by_default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A provider backed by a real, disposable directory.
    ///
    /// Every test uses one. The `cfg!(test)` default already stops a forgotten
    /// override reaching the user's notes, but a test that asserts anything
    /// about the store needs somewhere real to look.
    fn provider(dir: &TempDir) -> NotesProvider {
        register_translations();
        let mut p = NotesProvider::new();
        p.root_override = Some(dir.path().join("notes"));
        p
    }

    /// What a screen reader would read: every row's display text, tags gone.
    fn labels(elems: &[FfonElement]) -> Vec<String> {
        elems
            .iter()
            .map(|e| match e {
                FfonElement::Str(s) => tags::strip_display(s),
                FfonElement::Obj(o) => tags::strip_display(&o.key),
            })
            .collect()
    }

    /// Walk into the row with this label and return the level below it.
    ///
    /// The `fetch` first is not ceremony: `push_path` is handed the text the
    /// user saw, and the label-to-node map that resolves it is built while
    /// rendering. The app always renders a level before the user can move
    /// inside it.
    fn enter(p: &mut NotesProvider, label: &str) -> Vec<FfonElement> {
        p.fetch();
        p.push_path(label);
        p.fetch()
    }

    /// Hand the provider a list the way the app does after an edit.
    fn sync(p: &mut NotesProvider, rows: Vec<FfonElement>) {
        p.sync_ffon_body_children(&rows);
    }

    /// A row as the app would hand one back: a brand-new one, with no id yet.
    fn new_row(text: &str) -> FfonElement {
        FfonElement::Str(tags::format_input(text))
    }

    fn new_branch_row(text: &str) -> FfonElement {
        FfonElement::new_obj(tags::format_input(text))
    }

    /// The rows the provider currently renders for this level, verbatim — what
    /// the app would hand back unchanged if the user changed nothing. Includes
    /// the `list meta:` header, because the app hands that back too.
    fn rows(p: &mut NotesProvider) -> Vec<FfonElement> {
        p.fetch()
    }

    /// The display text of the note rows only, with the meta header dropped.
    fn note_labels(elems: &[FfonElement]) -> Vec<String> {
        labels(elems)
            .into_iter()
            .filter(|l| *l != localize::t("notes-list-meta"))
            .collect()
    }

    // ---- Shape -----------------------------------------------------------

    /// The root's meta is what makes the chain useful: one row, read first,
    /// that changes if anything anywhere in the tree changed.
    #[test]
    fn the_top_level_list_opens_with_its_list_meta_too() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_row("Groceries")]);
        assert_eq!(
            labels(&p.fetch()),
            vec![localize::t("notes-list-meta"), "Groceries".to_owned()]
        );
    }

    #[test]
    fn the_root_meta_shows_the_root_hash_and_offers_no_visibility() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_row("Groceries")]);
        let meta = enter(&mut p, &localize::t("notes-list-meta"));

        assert!(
            !meta
                .iter()
                .any(|e| matches!(e, FfonElement::Obj(o) if tags::has_radio(&o.key))),
            "the root is not a note, so there is nothing to publish: {:?}",
            labels(&meta)
        );
        assert!(
            labels(&meta)
                .iter()
                .any(|l| l.contains(&p.tree.root_hash_hex())),
            "the root hash is on show: {:?}",
            labels(&meta)
        );
    }

    /// The property the root hash exists for: it moves when anything below it
    /// moves, however deep.
    #[test]
    fn the_root_hash_changes_on_any_edit_anywhere() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        let after_first = p.tree.root_hash_hex();

        // A line added two levels down.
        enter(&mut p, "Groceries");
        let mut kept = rows(&mut p);
        kept.push(new_row("milk"));
        sync(&mut p, kept);
        let after_child = p.tree.root_hash_hex();
        assert_ne!(after_child, after_first, "adding a line moved the root");

        // Renaming that line.
        let mut kept = rows(&mut p);
        let last = kept.len() - 1;
        let id = tags::extract_id(match &kept[last] {
            FfonElement::Str(s) => s,
            FfonElement::Obj(o) => &o.key,
        })
        .unwrap();
        kept[last] = FfonElement::Str(format!(
            "{}{}",
            tags::format_id(&id),
            tags::format_input("oat milk")
        ));
        sync(&mut p, kept);
        let after_rename = p.tree.root_hash_hex();
        assert_ne!(after_rename, after_child, "renaming it moved the root");

        // Deleting it again returns the tree to what it was, so the root hash
        // must return with it — the hash is of the content, not of the history.
        let kept: Vec<FfonElement> = rows(&mut p)
            .into_iter()
            .filter(|e| !labels(std::slice::from_ref(e))[0].contains("oat milk"))
            .collect();
        sync(&mut p, kept);
        assert_eq!(
            p.tree.root_hash_hex(),
            after_first,
            "same content, same hash"
        );
    }

    /// Reordering two siblings is a change, and the hashes have to say so —
    /// otherwise a peer could not tell two different trees apart.
    #[test]
    fn reordering_siblings_changes_the_root_hash() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_row("a"), new_row("b")]);
        let before = p.tree.root_hash_hex();

        let mut kept = rows(&mut p);
        let meta = kept.remove(0);
        kept.swap(0, 1);
        kept.insert(0, meta);
        sync(&mut p, kept);

        assert_eq!(
            p.tree
                .notes
                .iter()
                .map(|n| n.text.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a"]
        );
        assert_ne!(p.tree.root_hash_hex(), before);
    }

    #[test]
    fn every_list_below_the_top_level_opens_with_its_list_meta() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        let inside = enter(&mut p, "Groceries");
        assert_eq!(labels(&inside)[0], localize::t("notes-list-meta"));
    }

    #[test]
    fn a_deeper_list_also_opens_with_its_list_meta() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        enter(&mut p, "Groceries");
        let mut kept = rows(&mut p);
        kept.push(new_branch_row("Weekend"));
        sync(&mut p, kept);
        let inside = enter(&mut p, "Weekend");
        assert_eq!(labels(&inside)[0], localize::t("notes-list-meta"));
    }

    #[test]
    fn no_level_is_ever_empty() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        assert!(!p.fetch().is_empty(), "the empty top level still says so");
        sync(&mut p, vec![new_branch_row("Groceries")]);
        assert!(!enter(&mut p, "Groceries").is_empty());
    }

    // ---- Visibility ------------------------------------------------------

    #[test]
    fn a_new_note_is_private() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        assert_eq!(p.tree.notes[0].visibility, Some(Visibility::Private));
    }

    #[test]
    fn visibility_is_offered_on_a_notes_own_list_and_nowhere_deeper() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        enter(&mut p, "Groceries");
        let note_meta = enter(&mut p, &localize::t("notes-list-meta"));
        assert!(
            note_meta
                .iter()
                .any(|e| matches!(e, FfonElement::Obj(o) if tags::has_radio(&o.key))),
            "a note's own list offers visibility: {:?}",
            labels(&note_meta)
        );

        // One level deeper: the hash, and no second visibility switch.
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        enter(&mut p, "Groceries");
        let mut kept = rows(&mut p);
        kept.push(new_branch_row("Weekend"));
        sync(&mut p, kept);
        enter(&mut p, "Weekend");
        let sub_meta = enter(&mut p, &localize::t("notes-list-meta"));
        assert!(
            !sub_meta
                .iter()
                .any(|e| matches!(e, FfonElement::Obj(o) if tags::has_radio(&o.key))),
            "a sublist inherits its note's visibility: {:?}",
            labels(&sub_meta)
        );
    }

    #[test]
    fn toggling_visibility_leaves_every_hash_alone() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        let before = p.tree.root_hash_hex();

        enter(&mut p, "Groceries");
        enter(&mut p, &localize::t("notes-list-meta"));
        p.on_radio_change("visibility", &localize::t("notes-visibility-public"));

        assert_eq!(p.tree.notes[0].visibility, Some(Visibility::Public));
        assert_eq!(
            p.tree.root_hash_hex(),
            before,
            "publishing a note changes its audience, not its content"
        );
    }

    // ---- Hashing ---------------------------------------------------------

    #[test]
    fn editing_a_leaf_rehashes_every_ancestor_up_to_the_root() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        enter(&mut p, "Groceries");
        let mut kept = rows(&mut p);
        kept.push(new_row("milk"));
        sync(&mut p, kept);

        let note_before = p.tree.notes[0].hash_hex();
        let root_before = p.tree.root_hash_hex();

        // Rename the leaf, keeping its id, exactly as the app hands it back.
        let mut kept = rows(&mut p);
        let last = kept.len() - 1;
        let id = tags::extract_id(match &kept[last] {
            FfonElement::Str(s) => s,
            FfonElement::Obj(o) => &o.key,
        })
        .unwrap();
        kept[last] = FfonElement::Str(format!(
            "{}{}",
            tags::format_id(&id),
            tags::format_input("oat milk")
        ));
        sync(&mut p, kept);

        assert_ne!(p.tree.notes[0].hash_hex(), note_before, "the note rehashed");
        assert_ne!(p.tree.root_hash_hex(), root_before, "the root rehashed");
    }

    #[test]
    fn a_leaf_and_a_branch_with_the_same_text_hash_differently() {
        let leaf = Node::leaf(1, "x");
        let branch = Node::branch(1, "x");
        assert_ne!(
            leaf.hash(),
            branch.hash(),
            "without domain separation a tree could be restructured invisibly"
        );
    }

    // ---- Identity --------------------------------------------------------

    #[test]
    fn two_sibling_notes_with_the_same_title_stay_distinct() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_row("todo"), new_row("todo")]);
        let ids: Vec<NodeId> = p.tree.notes.iter().map(|n| n.id).collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1], "identical text, different notes");

        // Rename only the second. `commit_edit`'s `old` could not tell them
        // apart; the id in the row can.
        let mut kept = rows(&mut p);
        let second = kept
            .iter()
            .position(|e| {
                row_id(match e {
                    FfonElement::Str(s) => s,
                    FfonElement::Obj(o) => &o.key,
                }) == Some(ids[1])
            })
            .expect("the second note is on screen");
        kept[second] = FfonElement::Str(format!(
            "{}{}",
            tags::format_id(&ids[1].to_string()),
            tags::format_input("todo later")
        ));
        sync(&mut p, kept);
        assert_eq!(
            p.tree
                .notes
                .iter()
                .map(|n| n.text.as_str())
                .collect::<Vec<_>>(),
            vec!["todo", "todo later"]
        );
    }

    #[test]
    fn a_notes_id_survives_an_insert_above_it() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_row("second")]);
        let id = p.tree.notes[0].id;

        let mut kept = rows(&mut p);
        kept.insert(0, new_row("first"));
        sync(&mut p, kept);

        assert_eq!(p.tree.notes[1].text, "second");
        assert_eq!(p.tree.notes[1].id, id, "position moved, identity did not");
    }

    #[test]
    fn every_segment_round_trips_through_its_token() {
        for seg in [Segment::Node(1), Segment::Node(9_999), Segment::Meta] {
            assert_eq!(Segment::from_token(&seg.token()), Some(seg));
        }
        assert_eq!(Segment::from_token("nonsense"), None);
    }

    #[test]
    fn the_path_round_trips_through_set_current_path() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        enter(&mut p, "Groceries");
        let path = p.current_path().to_owned();

        let mut q = provider(&d);
        q.set_current_path(&path);
        assert_eq!(q.current_path(), path);
    }

    // ---- The protected meta row -----------------------------------------

    #[test]
    fn the_list_meta_row_refuses_to_be_deleted() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        enter(&mut p, "Groceries");
        assert!(
            !p.delete_item(&localize::t("notes-list-meta")),
            "the meta row belongs to the list, not to the user"
        );
        assert!(p.take_error().is_some(), "and says why");
    }

    #[test]
    fn the_list_meta_row_refuses_to_be_edited() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        enter(&mut p, "Groceries");
        assert!(!p.commit_edit(&localize::t("notes-list-meta"), "something else"));
    }

    #[test]
    fn a_meta_row_handed_back_is_not_stored_as_a_note() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        enter(&mut p, "Groceries");
        // The app hands back exactly what it was rendering, meta row included.
        let kept = rows(&mut p);
        sync(&mut p, kept);
        assert!(
            p.tree.notes[0].children.is_empty(),
            "the meta row is rendered, not stored: {:?}",
            p.tree.notes[0].children
        );
    }

    #[test]
    fn the_meta_row_is_pinned_back_to_the_top_after_a_reorder() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        enter(&mut p, "Groceries");
        let mut kept = rows(&mut p);
        kept.push(new_row("milk"));
        sync(&mut p, kept);

        // The user drags the meta row to the bottom, or the app hands it back
        // in a different order after some edit.
        let mut shuffled = rows(&mut p);
        let meta = shuffled.remove(0);
        shuffled.push(meta);
        sync(&mut p, shuffled);

        assert_eq!(labels(&p.fetch())[0], localize::t("notes-list-meta"));
    }

    // ---- Escaping --------------------------------------------------------

    /// The security property: a note the user *typed* must never become a
    /// control the app will run.
    ///
    /// Only the raw row is asserted here, because that is what the tag parser
    /// reads. The rendered display text is a separate, weaker matter — see
    /// `a_tag_shaped_note_reads_back_without_its_tag`.
    #[test]
    fn a_note_titled_with_a_button_tag_stays_inert() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_row("<button>wipe</button>Click me")]);
        let rendered = p.fetch();
        let raw = match &rendered[0] {
            FfonElement::Str(s) => s.clone(),
            FfonElement::Obj(o) => o.key.clone(),
        };
        assert!(!tags::has_button(&raw), "still parses as a button: {raw}");
        assert!(!tags::has_input(&tags::strip_display(&raw)));
        // The text itself is stored intact, whatever the renderer makes of it.
        assert_eq!(p.tree.notes[0].text, "<button>wipe</button>Click me");
    }

    /// A known cosmetic limitation of the SDK, pinned here so it is a decision
    /// rather than a surprise.
    ///
    /// `tags::strip_display` unescapes partway through and then re-scans what it
    /// just unescaped (the `<id>` arm ends in
    /// `strip_display(&unescape(&result))`), so an escaped tag survives one pass
    /// and is stripped on the next. The note is inert either way — nothing
    /// becomes a live control — but the user reads "Click me" rather than the
    /// angle brackets they typed. `lib_gitclient` has the same behaviour for
    /// diff lines. Fixing it means deferring the unescape to the final return
    /// in the SDK, which changes rendering for every provider and belongs in its
    /// own change.
    #[test]
    fn a_tag_shaped_note_reads_back_without_its_tag() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_row("<button>wipe</button>Click me")]);
        assert_eq!(note_labels(&p.fetch()), vec!["Click me"]);
    }

    #[test]
    fn a_note_cannot_forge_an_id() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_row("real")]);
        let real_id = p.tree.notes[0].id;
        // A note whose *text* looks like an id tag must not be mistaken for one.
        let mut kept = rows(&mut p);
        kept.push(new_row("<id>1</id>impostor"));
        sync(&mut p, kept);
        assert_eq!(p.tree.notes[0].id, real_id);
        assert_eq!(p.tree.notes[1].text, "<id>1</id>impostor");
        assert_ne!(p.tree.notes[1].id, real_id);
    }

    // ---- The store -------------------------------------------------------

    #[test]
    fn a_branch_writes_a_file_and_a_dot_d_folder() {
        let d = TempDir::new().unwrap();
        let root = d.path().join("notes");
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        enter(&mut p, "Groceries");
        let mut kept = rows(&mut p);
        kept.push(new_row("milk"));
        sync(&mut p, kept);

        assert_eq!(
            std::fs::read_to_string(root.join("0001")).unwrap(),
            "Groceries"
        );
        assert!(root.join("0001.d").is_dir());
        assert_eq!(
            std::fs::read_to_string(root.join("0001.d").join("0001")).unwrap(),
            "milk"
        );
        assert!(root.join("0001.d").join(store::LISTMETA).is_file());
    }

    #[test]
    fn a_leaf_writes_no_folder() {
        let d = TempDir::new().unwrap();
        let root = d.path().join("notes");
        let mut p = provider(&d);
        sync(&mut p, vec![new_row("Ideas")]);
        assert!(root.join("0001").is_file());
        assert!(!root.join("0001.d").exists());
    }

    #[test]
    fn reloading_the_store_reproduces_the_same_root_hash() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries"), new_row("Ideas")]);
        enter(&mut p, "Groceries");
        let mut kept = rows(&mut p);
        kept.push(new_row("milk"));
        sync(&mut p, kept);
        let hash = p.tree.root_hash_hex();
        let ids: Vec<NodeId> = p.tree.notes.iter().map(|n| n.id).collect();

        let mut q = provider(&d);
        q.fetch();
        assert_eq!(q.tree.root_hash_hex(), hash);
        assert_eq!(
            q.tree.notes.iter().map(|n| n.id).collect::<Vec<_>>(),
            ids,
            "ids survive a restart, or every stored path would break"
        );
    }

    #[test]
    fn visibility_survives_a_reload() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries")]);
        enter(&mut p, "Groceries");
        enter(&mut p, &localize::t("notes-list-meta"));
        p.on_radio_change("visibility", &localize::t("notes-visibility-public"));

        let mut q = provider(&d);
        q.fetch();
        assert_eq!(q.tree.notes[0].visibility, Some(Visibility::Public));
    }

    #[test]
    fn deleting_a_note_removes_its_folder() {
        let d = TempDir::new().unwrap();
        let root = d.path().join("notes");
        let mut p = provider(&d);
        sync(&mut p, vec![new_branch_row("Groceries"), new_row("Ideas")]);
        assert!(root.join("0001.d").is_dir());

        // The app removed the "Groceries" row and handed back what is left.
        let kept: Vec<FfonElement> = rows(&mut p)
            .into_iter()
            .filter(|e| !labels(std::slice::from_ref(e))[0].contains("Groceries"))
            .collect();
        sync(&mut p, kept);

        assert_eq!(std::fs::read_to_string(root.join("0001")).unwrap(), "Ideas");
        assert!(
            !root.join("0001.d").exists(),
            "the folder went with the note"
        );
        assert!(!root.join("0002").exists(), "the list renumbered densely");
    }

    #[test]
    fn a_file_the_user_dropped_in_is_left_alone() {
        let d = TempDir::new().unwrap();
        let root = d.path().join("notes");
        let mut p = provider(&d);
        sync(&mut p, vec![new_row("Ideas")]);
        std::fs::write(root.join("README"), "mine").unwrap();

        sync(&mut p, vec![new_row("Ideas"), new_row("More")]);
        assert_eq!(
            std::fs::read_to_string(root.join("README")).unwrap(),
            "mine"
        );
    }

    #[test]
    fn an_unreadable_store_leaves_the_directory_untouched() {
        let d = TempDir::new().unwrap();
        let root = d.path().join("notes");
        std::fs::create_dir_all(&root).unwrap();
        // A note whose contents are not valid UTF-8: readable as bytes, not as
        // text, so the load fails rather than silently dropping it.
        std::fs::write(root.join("0001"), [0xff, 0xfe, 0xfd]).unwrap();

        let mut p = provider(&d);
        p.fetch();
        assert!(
            p.load_failed,
            "a store that will not parse is not an empty one"
        );

        // Anything that would normally write must now decline.
        sync(&mut p, vec![new_row("clobber")]);
        assert_eq!(
            std::fs::read(root.join("0001")).unwrap(),
            vec![0xff, 0xfe, 0xfd],
            "the unreadable note is still there"
        );
    }

    #[test]
    fn a_missing_directory_is_an_empty_tree_not_a_failure() {
        let d = TempDir::new().unwrap();
        let mut p = provider(&d);
        p.fetch();
        assert!(!p.load_failed);
    }

    #[test]
    fn nothing_is_written_when_there_is_nowhere_to_write() {
        // No override and the test guard on: `root()` is None, and the tree
        // lives in memory for the session rather than being discarded.
        let mut p = NotesProvider::new();
        register_translations();
        sync(&mut p, vec![new_row("in memory only")]);
        assert_eq!(p.tree.notes.len(), 1);
    }

    // ---- Locale parity ---------------------------------------------------

    #[test]
    fn all_four_bundles_carry_the_same_keys() {
        fn keys(ftl: &str) -> Vec<String> {
            ftl.lines()
                .filter(|l| !l.trim_start().starts_with('#') && l.contains('='))
                .filter_map(|l| l.split('=').next())
                .map(|k| k.trim().to_owned())
                .filter(|k| !k.is_empty() && !k.contains(' '))
                .collect()
        }
        let en = keys(include_str!("../locales/en-US.ftl"));
        for (name, ftl) in [
            ("nl-BE", include_str!("../locales/nl-BE.ftl")),
            ("fr-BE", include_str!("../locales/fr-BE.ftl")),
            ("de-BE", include_str!("../locales/de-BE.ftl")),
        ] {
            assert_eq!(keys(ftl), en, "{name} has drifted from en-US");
        }
    }

    /// The app skips `pop_path` when leaving an `Obj` keyed exactly `"meta"`,
    /// which would leave this provider's path a level deeper than the cursor.
    #[test]
    fn the_meta_rows_key_is_never_the_bare_word_meta() {
        register_translations();
        assert_ne!(localize::t("notes-list-meta"), "meta");
    }
}
