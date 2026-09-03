//! The project-management provider. Its first feature is a kanban board.
//!
//! # Two surfaces, one board
//!
//! The same two-level tree is reachable two ways, and which one you want depends
//! on whether you are reading or arranging:
//!
//! * **The list** (general mode) is the provider root: columns as `Obj` rows,
//!   cards as `Str` rows one level in. Everything structural happens here through
//!   the app's shared structural-edit capability, so Ctrl+I / Ctrl+A / Ctrl+D /
//!   Delete / Ctrl+X / Ctrl+C / Ctrl+V behave exactly as they do in every other
//!   provider that declares it, and the app records the undo.
//! * **The board** (the `d` dashboard) draws the columns side by side. Arrow keys
//!   move between and within them, and the same editing keys work, but here the
//!   provider implements them itself: the app forwards every keystroke into an
//!   interactive dashboard without interpreting it.
//!
//! # Why a card is a `Str`
//!
//! `Obj` versus `Str` is the depth cap, not a stylistic choice. The app descends
//! into an `Obj` and cannot descend into a `Str`, so making cards `Str` means
//! "more layers are not shown" needs no guard anywhere in the navigation code —
//! there is nowhere for a third level to appear.
//!
//! # The `<id>` prefix
//!
//! Every row is `<id>N</id><input>text</input>`, with the id **outside** the
//! `<input>` — inside it, the app's live edit eats it on the first keystroke.
//! Ids are what make `sync_ffon_body_children` a diff rather than a guess:
//! `commit_edit(old, new)` cannot tell two identically titled siblings apart,
//! because `old` is only the previous display text.
//!
//! # Undo, in two halves
//!
//! List edits ride the app's `TimelineEntry::Structural` records, which reverse
//! by mutating the app's FFON tree and calling back into
//! `sync_ffon_body_children`. A board edit never touches that tree, so those arms
//! could not reverse one even if they were recorded. Board edits therefore emit
//! `TimelineEntry::ProviderOp` and are reversed here, in [`Provider::undo`]. Both
//! kinds land on the same per-tab timeline in the order they happened, so there
//! is still one undo history rather than a board-shaped exception to it.

pub mod board;
mod escape;
pub mod render;
pub mod store;

use serde::{Deserialize, Serialize};
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::timeline::TimelineEntry;
use sicompass_sdk::{
    BuiltinManifest, DashboardFrame, DashboardKey, DashboardKeysym, DashboardKind,
    DashboardRequest, NavigationRequest, Provider, localize, register_builtin_manifest,
    register_provider_factory, tags,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use board::{Board, Card, Column, Id};
use render::{Focus, View};

// ---------------------------------------------------------------------------
// Test stub: never touch the user's real board from a test.
//
// The failure this prevents is not a flaky test, it is data loss. `save_board`
// reconciles a directory against a board, so a test that builds a two-column
// board and saves it to the real board directory does not add two columns — it
// deletes everything else.
//
// Two audiences, hence a compile-time default and a runtime setter:
//
// * This crate's own unit tests get it free from `cfg!(test)`. Setting the
//   per-instance `root_override` is the right way to make a test safe, but
//   forgetting it must fail closed rather than silently reaching the real store.
// * The app's integration tests are a different binary, where this crate is an
//   ordinary dependency compiled *without* `cfg(test)`, and they reach the
//   provider as a `Box<dyn Provider>` with no way to set the override. They call
//   `_set_test_no_persist(true)` once per binary instead.
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
/// string: a factory-built provider is reachable before `register()` on some
/// paths, and an unresolved key renders as the key itself.
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
// renders these raw strings, `command_label` localizes them.
//
// `"delete"` is deliberately absent. It is a reserved id: `avail_provider_has_delete`
// would claim both Ctrl+D and Delete and route them to `invoke_provider_delete`,
// which unwinds the cursor to depth 3 before deleting — email-shaped surgery
// that has no meaning on a two-level board.
// ---------------------------------------------------------------------------

pub const CMD_MOVE_UP: &str = "move up";
pub const CMD_MOVE_DOWN: &str = "move down";
pub const CMD_MOVE_LEFT: &str = "move left";
pub const CMD_MOVE_RIGHT: &str = "move right";

// ---------------------------------------------------------------------------
// Board operations, as they cross the timeline
// ---------------------------------------------------------------------------

/// What a card or column looked like, in enough detail to put it back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CardData {
    id: Id,
    text: String,
}

/// One reversible board edit.
///
/// Cards only. Columns are created, renamed, reordered and deleted in the list
/// view, where the app's own structural-edit capability records them as
/// `Structural` entries — the board has no gesture that reaches a column, so it
/// has no column op to record.
///
/// Carried in a `TimelineEntry::ProviderOp` as JSON in the payload rather than as
/// a tagged FFON shape: the app never inspects it, only hands it back, so a
/// self-describing blob keeps the wire format in one place instead of spreading
/// it across the FFON encoder.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op")]
enum BoardOp {
    Add {
        column: Id,
        index: usize,
        card: CardData,
    },
    Delete {
        column: Id,
        index: usize,
        card: CardData,
    },
    Rename {
        id: Id,
        before: String,
        after: String,
    },
    Move {
        card: Id,
        from_column: Id,
        from_index: usize,
        to_column: Id,
        to_index: usize,
    },
}

impl BoardOp {
    /// The stable command id, also the label key suffix (`pm-op-{id}`).
    ///
    /// Spelled out rather than derived from the variant name: these strings
    /// reach the user through the undo-history screen, so renaming a variant
    /// must not silently rename a label key and leave it unresolved.
    fn command(&self) -> &'static str {
        match self {
            BoardOp::Add { .. } => "add-card",
            BoardOp::Delete { .. } => "delete-card",
            BoardOp::Rename { .. } => "rename-card",
            BoardOp::Move { .. } => "move-card",
        }
    }
}

// ---------------------------------------------------------------------------
// The dashboard's own modes and clipboard
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoardMode {
    Board,
    Insert,
}

/// The board's own clipboard. Cards only: a column head is not focusable, so
/// there is no gesture that could put one here.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Clip {
    Card(CardData),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditTarget {
    Card(Id),
}

/// An open insert-mode edit.
///
/// `creating` distinguishes "renaming something that exists" from "typing into
/// something this keypress just made". The difference matters on commit: an empty
/// rename is a rename to nothing, but an empty creation is a cancelled one, and
/// leaving a blank card behind is not what the user asked for.
#[derive(Debug, Clone)]
struct EditState {
    target: EditTarget,
    text: String,
    /// Caret position in **characters**, not bytes.
    caret: usize,
    original: String,
    creating: bool,
}

// ---------------------------------------------------------------------------
// The provider
// ---------------------------------------------------------------------------

pub struct ProjectManagementProvider {
    board: Board,
    /// The column the list cursor has descended into, if any. A board is two
    /// levels deep, so this is the whole path.
    open_column: Option<Id>,
    rendered_path: String,
    /// Displayed label back to the id it names, per level. `push_path` is handed
    /// the label the user was looking at, not an id.
    labels: HashMap<Option<Id>, HashMap<String, Id>>,
    root_override: Option<PathBuf>,
    loaded: bool,
    load_failed: bool,
    error: Option<String>,
    announcement: Option<String>,
    refresh: bool,
    timeline: Vec<TimelineEntry>,
    /// Text taken by `commit_edit` for a row the app has not told us about yet.
    pending_create: Option<String>,

    // ---- Dashboard ------------------------------------------------------
    mode: BoardMode,
    focus: Focus,
    edit: Option<EditState>,
    clip: Option<Clip>,
    /// The letter that opened insert mode, swallowed once. See `dashboard_text`.
    suppress_text: Option<String>,
    /// Where the list cursor should land, queued when the board is left.
    navigation: Option<NavigationRequest>,
    /// Where the list cursor was when the board was entered, handed over by the
    /// app just before `enter_dashboard`.
    entry_path: Vec<usize>,
    /// The app's live palette, refreshed before every frame. Its default is the
    /// app's dark theme, so a provider drawn before the app has handed one over
    /// still draws in real colours.
    palette: sicompass_sdk::DashboardPalette,
    dashboard_request: Option<DashboardRequest>,
}

impl Default for ProjectManagementProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectManagementProvider {
    pub fn new() -> Self {
        ProjectManagementProvider {
            board: Board::new(),
            open_column: None,
            rendered_path: String::new(),
            labels: HashMap::new(),
            root_override: None,
            loaded: false,
            load_failed: false,
            error: None,
            announcement: None,
            refresh: false,
            timeline: Vec::new(),
            pending_create: None,
            mode: BoardMode::Board,
            focus: Focus::default(),
            edit: None,
            clip: None,
            suppress_text: None,
            navigation: None,
            entry_path: Vec::new(),
            palette: sicompass_sdk::DashboardPalette::default(),
            dashboard_request: None,
        }
    }

    /// Where the board lives. `None` means nowhere usable, and the board stays in
    /// memory for the session rather than being silently discarded.
    ///
    /// Deliberately `data_home()` and not `state_home()`: on macOS the latter is
    /// `~/Library/Logs`, which the OS and every cleanup tool treat as disposable.
    /// A board is a document.
    fn root(&self) -> Option<PathBuf> {
        if let Some(p) = &self.root_override {
            return Some(p.clone());
        }
        if test_no_persist() {
            return None;
        }
        sicompass_sdk::platform::data_home().map(|d| d.join("sicompass").join("projectmanagement"))
    }

    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(root) = self.root() else {
            return;
        };
        match store::load_board(&root) {
            Some(mut b) => {
                // Normalise on the way in, not only on the way out. Stripping a
                // trailing colon when a title is *written* leaves every title
                // written before that fix still carrying one, and the board would
                // go on showing it until the user happened to rename the column.
                // Doing it here heals what is already on disk, and the next save
                // persists the clean form.
                for c in b.columns.iter_mut() {
                    c.title = column_title(&c.title);
                }
                self.board = b;
            }
            None => {
                // Not an empty board. Refusing to write is the whole point: a
                // save would reconcile the directory against a board that failed
                // to load and delete what could not be read.
                self.load_failed = true;
                self.error = Some(localize::t("pm-error-unreadable"));
            }
        }
    }

    fn persist(&mut self) {
        if self.load_failed {
            return;
        }
        let Some(root) = self.root() else {
            return;
        };
        if store::save_board(&root, &self.board).is_err() {
            self.error = Some(localize::t("pm-error-save"));
        }
    }

    // ---- Row rendering --------------------------------------------------

    /// A row: `<id>N</id><input>text</input>`.
    fn row_label(id: Id, text: &str) -> String {
        format!(
            "{}{}",
            tags::format_id(&id.to_string()),
            tags::format_input(&escape::escape(text))
        )
    }

    fn remember(&mut self, level: Option<Id>, label: &str, id: Id) {
        let entry = self.labels.entry(level).or_default();
        entry.insert(label.to_owned(), id);
        entry.insert(tags::strip_display(label), id);
    }

    /// The rows for the level the cursor is on.
    fn level_children(&mut self) -> Vec<FfonElement> {
        let level = self.open_column;
        self.labels.remove(&level);

        let rows: Vec<(Id, String, bool)> = match level {
            None => self
                .board
                .columns
                .iter()
                .map(|c| (c.id, c.title.clone(), true))
                .collect(),
            Some(col) => match self.board.column(col) {
                Some(c) => c
                    .cards
                    .iter()
                    .map(|k| (k.id, k.text.clone(), false))
                    .collect(),
                // The column was deleted in another tab. An empty level is
                // right; the app's Left will take the cursor back out.
                None => Vec::new(),
            },
        };

        let mut out = Vec::with_capacity(rows.len().max(1));
        for (id, text, is_column) in rows {
            let label = Self::row_label(id, &text);
            self.remember(level, &label, id);
            out.push(if is_column {
                FfonElement::new_obj(label)
            } else {
                FfonElement::Str(label)
            });
        }

        // Never empty. The app seeds its own insert placeholder into an empty
        // level, which would then look like a row this provider had rendered.
        if out.is_empty() {
            out.push(FfonElement::new_str(localize::t(if level.is_none() {
                "pm-empty-columns"
            } else {
                "pm-empty-cards"
            })));
        }
        out
    }

    fn sync_rendered_path(&mut self) {
        self.rendered_path = match self.open_column {
            None => String::new(),
            Some(id) => format!("/c{id}"),
        };
    }

    // ---- The write path -------------------------------------------------

    /// Reconcile one list against what the app says it now holds.
    ///
    /// Rows carry their `<id>`, so this is a diff rather than a guess: a row with
    /// a known id keeps its node and takes the new text, a row with no id is new,
    /// and a node whose row is gone was deleted. Order is the order the app gave.
    fn reconcile(&mut self, children: &[FfonElement]) {
        if self.load_failed {
            self.error = Some(localize::t("pm-error-unreadable"));
            return;
        }

        // Rows this provider renders but does not store. The app hands back
        // whatever it was displaying, so without this the "no columns yet" line
        // would become the user's first column the moment they made a second.
        let rendered_only = [
            localize::t("pm-empty-columns"),
            localize::t("pm-empty-cards"),
        ];

        // Which list is this? Not necessarily the one the cursor is on: undo and
        // redo hand back a list from wherever the reversed edit happened, and
        // they do not move the provider's path to match. The app cannot supply it
        // either — a provider's path is not in step with FFON depth.
        //
        // The rows answer it themselves. Any row with an id names something, and
        // that thing's owner is the list. Only a list of nothing but new rows
        // falls back to the cursor, which is right: a list of nothing but new
        // rows can only be the one the user is typing into.
        let ids: Vec<Id> = children.iter().filter_map(|e| row_id(raw_of(e))).collect();
        let target = ids
            .iter()
            .find_map(|id| {
                if self.board.column_index(*id).is_some() {
                    Some(None)
                } else {
                    self.board
                        .locate_card(*id)
                        .map(|(ci, _)| Some(self.board.columns[ci].id))
                }
            })
            .unwrap_or(self.open_column);

        match target {
            None => self.reconcile_columns(children, &rendered_only),
            Some(col) => self.reconcile_cards(col, children, &rendered_only),
        }
        self.persist();
    }

    fn reconcile_columns(&mut self, children: &[FfonElement], rendered_only: &[String]) {
        let mut rebuilt: Vec<Column> = Vec::new();
        for elem in children {
            let raw = raw_of(elem);
            if rendered_only.iter().any(|r| r == raw) {
                continue;
            }
            let text = column_title(&row_text(raw));
            match row_id(raw) {
                Some(id) => {
                    let existing = self.board.column(id).cloned();
                    match existing {
                        Some(mut c) => {
                            c.title = text;
                            rebuilt.push(c);
                        }
                        None => rebuilt.push(Column::new(id, text)),
                    }
                }
                None => {
                    let text = column_title(&self.resolve_new_text(text));
                    // A row the app is still showing as its blank placeholder is
                    // not a column. Only a committed one has text.
                    if text.is_empty() {
                        continue;
                    }
                    let id = self.board.mint_id();
                    rebuilt.push(Column::new(id, text));
                }
            }
        }
        self.board.columns = rebuilt;
        self.board.reseat_counter();
    }

    fn reconcile_cards(&mut self, col: Id, children: &[FfonElement], rendered_only: &[String]) {
        let Some(ci) = self.board.column_index(col) else {
            return;
        };
        let mut rebuilt: Vec<Card> = Vec::new();
        for elem in children {
            let raw = raw_of(elem);
            if rendered_only.iter().any(|r| r == raw) {
                continue;
            }
            let text = row_text(raw);
            match row_id(raw) {
                Some(id) => {
                    // The card may be moving in from another column, so look it
                    // up board-wide rather than only in this one.
                    match self.board.card(id).cloned() {
                        Some(mut k) => {
                            k.text = text;
                            rebuilt.push(k);
                        }
                        None => rebuilt.push(Card::new(id, text)),
                    }
                }
                None => {
                    let text = self.resolve_new_text(text);
                    if text.is_empty() {
                        continue;
                    }
                    let id = self.board.mint_id();
                    rebuilt.push(Card::new(id, text));
                }
            }
        }
        // A card that moved into this column has to leave the one it came from,
        // or the same id would exist twice and `locate_card` would find the stale
        // copy first.
        let moved: Vec<Id> = rebuilt.iter().map(|k| k.id).collect();
        for (i, c) in self.board.columns.iter_mut().enumerate() {
            if i != ci {
                c.cards.retain(|k| !moved.contains(&k.id));
            }
        }
        self.board.columns[ci].cards = rebuilt;
        self.board.reseat_counter();
    }

    /// The text for a brand-new row: what `commit_edit` took, when the tree the
    /// app hands back still holds its blank placeholder and cannot say.
    fn resolve_new_text(&mut self, from_row: String) -> String {
        if !from_row.is_empty() {
            self.pending_create = None;
            return from_row;
        }
        self.pending_create.take().unwrap_or_default()
    }

    // ---- Board mutation, with a timeline entry --------------------------

    fn record(&mut self, op: BoardOp) {
        let label = localize::t(&format!("pm-op-{}", op.command()));
        let payload = serde_json::to_string(&op).unwrap_or_default();
        self.timeline.push(TimelineEntry::ProviderOp {
            // Patched by the app: `drain_provider_entries` rewrites it to the
            // originating provider's real index, which this crate cannot know.
            provider_idx: 0,
            command: op.command().to_owned(),
            payload: FfonElement::Str(payload),
            label,
        });
    }

    /// Apply an op, or its inverse.
    ///
    /// One function for both directions so the two can never drift: a redo that
    /// is not exactly the undo's inverse is the classic way an undo history stops
    /// matching what is on screen.
    fn apply(&mut self, op: &BoardOp, forward: bool) {
        match op {
            BoardOp::Add {
                column,
                index,
                card,
            } => {
                if forward {
                    self.insert_card(*column, *index, card.clone());
                } else {
                    self.remove_card(card.id);
                }
            }
            BoardOp::Delete {
                column,
                index,
                card,
            } => {
                if forward {
                    self.remove_card(card.id);
                } else {
                    self.insert_card(*column, *index, card.clone());
                }
            }
            BoardOp::Rename { id, before, after } => {
                let want = if forward { after } else { before };
                if let Some((ci, ki)) = self.board.locate_card(*id) {
                    self.board.columns[ci].cards[ki].text = want.clone();
                }
            }
            BoardOp::Move {
                card,
                from_column,
                from_index,
                to_column,
                to_index,
            } => {
                // Where it came from is not named: the card is lifted from
                // wherever it actually is, so a stale `from_*` can never make the
                // two disagree. Only the destination differs by direction.
                let (dst, di) = if forward {
                    (*to_column, *to_index)
                } else {
                    (*from_column, *from_index)
                };
                if let Some((ci, ki)) = self.board.locate_card(*card) {
                    let taken = self.board.columns[ci].cards.remove(ki);
                    if let Some(target) = self.board.column_index(dst) {
                        let at = di.min(self.board.columns[target].cards.len());
                        self.board.columns[target].cards.insert(at, taken);
                    } else {
                        // The destination column is gone. Put it back rather than
                        // dropping the card on the floor.
                        self.board.columns[ci].cards.insert(ki, taken);
                    }
                }
            }
        }
        self.board.reseat_counter();
        self.refresh = true;
        self.persist();
    }

    fn insert_card(&mut self, column: Id, index: usize, card: CardData) {
        if let Some(ci) = self.board.column_index(column) {
            let at = index.min(self.board.columns[ci].cards.len());
            self.board.columns[ci]
                .cards
                .insert(at, Card::new(card.id, card.text));
        }
    }

    fn remove_card(&mut self, id: Id) {
        if let Some((ci, ki)) = self.board.locate_card(id) {
            self.board.columns[ci].cards.remove(ki);
        }
    }

    // ---- Announcements --------------------------------------------------

    fn say(&mut self, text: String) {
        self.announcement = Some(text);
    }

    fn say_key(&mut self, key: &str, args: &[(&str, String)]) {
        let mut a = localize::Args::new();
        for (k, v) in args {
            a.set(*k, v.clone());
        }
        self.announcement = Some(localize::t_args(key, &a));
    }

    /// Describe whatever the board cursor is now on.
    ///
    /// The column is named on every card, not only when the cursor crosses into
    /// a new one. Left and Right change columns without changing the card's
    /// position in its own list, so "card 2 of 4" alone would leave a listener
    /// unable to tell a sideways move from a vertical one.
    fn say_focus(&mut self) {
        let total = self.board.columns.len();
        let Some(col) = self.board.columns.get(self.focus.col) else {
            self.say(localize::t("pm-empty-columns"));
            return;
        };
        let title = col.title.clone();
        let cards = col.cards.len();
        if cards == 0 {
            self.say_key(
                "pm-say-column-empty",
                &[
                    ("index", (self.focus.col + 1).to_string()),
                    ("total", total.to_string()),
                    ("title", title),
                ],
            );
            return;
        }
        let i = self.focus.row.min(cards - 1);
        let text = col.cards[i].text.clone();
        self.say_key(
            "pm-say-card",
            &[
                ("column", title),
                ("index", (i + 1).to_string()),
                ("total", cards.to_string()),
                ("text", text),
            ],
        );
    }

    fn say_edit(&mut self) {
        let text = self
            .edit
            .as_ref()
            .map(|e| e.text.clone())
            .unwrap_or_default();
        if text.is_empty() {
            self.say(localize::t("pm-say-insert-empty"));
        } else {
            self.say_key("pm-say-insert", &[("text", text)]);
        }
    }

    // ---- Board navigation -----------------------------------------------

    /// True when the cursor is on an empty column's placeholder rather than a
    /// real card. Insert acts, delete and copy do not: there is nothing there.
    fn on_placeholder(&self) -> bool {
        render::is_placeholder(&self.board, self.focus.col)
    }

    /// Keep the cursor on something that exists after the board changed under it.
    fn clamp_focus(&mut self) {
        if self.board.columns.is_empty() {
            self.focus = Focus::default();
            return;
        }
        self.focus.col = self.focus.col.min(self.board.columns.len() - 1);
        // `slots` is one for an empty column, because its placeholder is a real
        // focus target: it is the only thing to stand on while adding the first
        // card.
        let n = render::slots(&self.board, self.focus.col);
        self.focus.row = self.focus.row.min(n.saturating_sub(1));
    }

    fn move_column(&mut self, delta: isize) -> bool {
        if self.board.columns.is_empty() {
            return false;
        }
        let next = self.focus.col as isize + delta;
        if next < 0 || next as usize >= self.board.columns.len() {
            self.say(localize::t("pm-say-edge"));
            return true;
        }
        self.focus.col = next as usize;
        // The row is kept where it can be: walking sideways along a rank of cards
        // is the gesture, and snapping to the top every time would undo it.
        self.clamp_focus();
        self.say_focus();
        true
    }

    fn move_row(&mut self, down: bool) -> bool {
        let n = render::slots(&self.board, self.focus.col);
        let cur = self.focus.row;
        let next = if down {
            (cur + 1).min(n.saturating_sub(1))
        } else {
            cur.saturating_sub(1)
        };
        if next == cur {
            self.say(localize::t("pm-say-edge"));
            return true;
        }
        self.focus.row = next;
        self.say_focus();
        true
    }

    // ---- Board editing --------------------------------------------------

    fn focused_column_id(&self) -> Option<Id> {
        self.board.columns.get(self.focus.col).map(|c| c.id)
    }

    /// Open insert mode on the focused card.
    ///
    /// `at_end` is the only difference between `i` and `a`, exactly as it is in
    /// the app's own `handle_i` / `handle_a`: both edit the item the cursor is
    /// on, one with the caret at the start and one at the end.
    ///
    /// On an empty column's placeholder there is nothing to edit, so both start a
    /// new card instead. That mirrors the list, where an empty level is an
    /// `<input>` slot and typing into it is how the first row appears.
    fn begin_rename(&mut self, at_end: bool) {
        if self.on_placeholder() {
            self.begin_new_card(0);
            return;
        }
        let Some(card) = self
            .board
            .columns
            .get(self.focus.col)
            .and_then(|c| c.cards.get(self.focus.row))
        else {
            return;
        };
        let original = card.text.clone();
        let target = EditTarget::Card(card.id);
        let caret = if at_end { original.chars().count() } else { 0 };
        self.edit = Some(EditState {
            target,
            text: original.clone(),
            caret,
            original,
            creating: false,
        });
        self.mode = BoardMode::Insert;
        self.say_edit();
    }

    /// Index for "a new card above this one".
    ///
    /// Zero on an empty column's placeholder: there is no card to be above, and
    /// the new one is simply the first.
    fn above(&self) -> usize {
        if self.on_placeholder() {
            0
        } else {
            self.focus.row
        }
    }

    /// Index for "a new card below this one".
    fn below(&self) -> usize {
        if self.on_placeholder() {
            0
        } else {
            self.focus.row + 1
        }
    }

    /// Make an empty card at `index` in the focused column and type into it.
    fn begin_new_card(&mut self, index: usize) {
        let Some(ci) = self
            .board
            .columns
            .get(self.focus.col)
            .map(|_| self.focus.col)
        else {
            self.error = Some(localize::t("pm-error-no-column"));
            return;
        };
        let id = self.board.mint_id();
        let at = index.min(self.board.columns[ci].cards.len());
        self.board.columns[ci].cards.insert(at, Card::new(id, ""));
        self.focus.row = at;
        self.edit = Some(EditState {
            target: EditTarget::Card(id),
            text: String::new(),
            caret: 0,
            original: String::new(),
            creating: true,
        });
        self.mode = BoardMode::Insert;
        self.say_edit();
    }

    /// Close insert mode, keeping what was typed.
    ///
    /// A creation typed into and then left empty is a cancelled creation, not a
    /// blank card: it is removed and no timeline entry is recorded, so Ctrl+Z
    /// does not step through edits that left no trace.
    fn commit_edit_state(&mut self) {
        let Some(edit) = self.edit.take() else {
            self.mode = BoardMode::Board;
            return;
        };
        self.mode = BoardMode::Board;
        let text = edit.text.trim().to_owned();

        if edit.creating && text.is_empty() {
            match edit.target {
                EditTarget::Card(id) => self.remove_card(id),
            }
            self.clamp_focus();
            self.say(localize::t("pm-say-board"));
            self.persist();
            return;
        }

        match edit.target {
            EditTarget::Card(id) => {
                if let Some((ci, ki)) = self.board.locate_card(id) {
                    self.board.columns[ci].cards[ki].text = text.clone();
                    if edit.creating {
                        let column = self.board.columns[ci].id;
                        self.record(BoardOp::Add {
                            column,
                            index: ki,
                            card: CardData { id, text },
                        });
                    } else if text != edit.original {
                        self.record(BoardOp::Rename {
                            id,
                            before: edit.original,
                            after: text,
                        });
                    }
                }
            }
        }
        self.refresh = true;
        self.persist();
        self.say(localize::t("pm-say-board"));
    }

    /// Delete the focused card.
    ///
    /// A no-op on an empty column's placeholder: it looks like a slot so the
    /// cursor has somewhere to stand, but there is nothing behind it to remove.
    fn delete_focused(&mut self) {
        if self.on_placeholder() {
            return;
        }
        let Some(col) = self.board.columns.get(self.focus.col) else {
            return;
        };
        let Some(card) = col.cards.get(self.focus.row) else {
            return;
        };
        let text = card.text.clone();
        let op = BoardOp::Delete {
            column: col.id,
            index: self.focus.row,
            card: CardData {
                id: card.id,
                text: card.text.clone(),
            },
        };
        self.apply(&op, true);
        self.record(op);
        self.clamp_focus();
        self.say_key("pm-say-deleted", &[("text", text)]);
    }

    fn copy_focused(&mut self, cut: bool) {
        if self.on_placeholder() {
            return;
        }
        let Some(card) = self
            .board
            .columns
            .get(self.focus.col)
            .and_then(|c| c.cards.get(self.focus.row))
        else {
            return;
        };
        let text = card.text.clone();
        self.clip = Some(Clip::Card(CardData {
            id: card.id,
            text: card.text.clone(),
        }));
        if cut {
            self.delete_focused();
            self.say_key("pm-say-cut", &[("text", text)]);
        } else {
            self.say_key("pm-say-copied", &[("text", text)]);
        }
    }

    /// Paste the board clipboard after the focus.
    ///
    /// Always with a **fresh id**: ids are minted once and never reused, so a
    /// pasted copy is a new card, not a second row claiming to be the original.
    /// Without this, `locate_card` would find whichever copy came first and every
    /// later edit would land on the wrong one.
    fn paste(&mut self) {
        let Some(Clip::Card(card)) = self.clip.clone() else {
            self.error = Some(localize::t("pm-error-nothing-to-paste"));
            return;
        };
        let Some(column) = self.focused_column_id() else {
            self.error = Some(localize::t("pm-error-no-column"));
            return;
        };
        // Onto an empty column's placeholder the card becomes the first one;
        // otherwise it lands below the card the cursor is on.
        let index = if self.on_placeholder() {
            0
        } else {
            self.focus.row + 1
        };
        let op = BoardOp::Add {
            column,
            index,
            card: CardData {
                id: self.board.mint_id(),
                text: card.text.clone(),
            },
        };
        self.apply(&op, true);
        self.record(op);
        self.focus.row = index;
        self.clamp_focus();
        self.say_key("pm-say-pasted", &[("text", card.text)]);
    }

    /// Turn pasted system-clipboard text into one card per non-blank line.
    fn paste_text(&mut self, text: &str) {
        let Some(column) = self.focused_column_id() else {
            self.error = Some(localize::t("pm-error-no-column"));
            return;
        };
        let mut index = if self.on_placeholder() {
            0
        } else {
            self.focus.row + 1
        };
        let mut last = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let op = BoardOp::Add {
                column,
                index,
                card: CardData {
                    id: self.board.mint_id(),
                    text: line.to_owned(),
                },
            };
            self.apply(&op, true);
            self.record(op);
            last = line.to_owned();
            index += 1;
        }
        if !last.is_empty() {
            self.focus.row = index - 1;
            self.clamp_focus();
            self.say_key("pm-say-pasted", &[("text", last)]);
        }
    }

    // ---- Insert-mode text editing ---------------------------------------

    fn insert_text(&mut self, s: &str) {
        let Some(edit) = self.edit.as_mut() else {
            return;
        };
        let at = byte_offset(&edit.text, edit.caret);
        edit.text.insert_str(at, s);
        edit.caret += s.chars().count();
    }

    fn backspace(&mut self) {
        let Some(edit) = self.edit.as_mut() else {
            return;
        };
        if edit.caret == 0 {
            return;
        }
        let from = byte_offset(&edit.text, edit.caret - 1);
        let to = byte_offset(&edit.text, edit.caret);
        edit.text.replace_range(from..to, "");
        edit.caret -= 1;
    }

    fn delete_forward(&mut self) {
        let Some(edit) = self.edit.as_mut() else {
            return;
        };
        let len = edit.text.chars().count();
        if edit.caret >= len {
            return;
        }
        let from = byte_offset(&edit.text, edit.caret);
        let to = byte_offset(&edit.text, edit.caret + 1);
        edit.text.replace_range(from..to, "");
    }

    // ---- Command handling (colon commands, list side) --------------------

    fn move_row_in_list(&mut self, down: bool, key: &str) -> bool {
        let Some(id) = self
            .labels
            .get(&self.open_column)
            .and_then(|m| m.get(key))
            .copied()
        else {
            return false;
        };
        match self.open_column {
            None => {
                let Some(i) = self.board.column_index(id) else {
                    return false;
                };
                let j = if down { i + 1 } else { i.wrapping_sub(1) };
                if down && j >= self.board.columns.len() || !down && i == 0 {
                    return false;
                }
                self.board.columns.swap(i, j);
            }
            Some(col) => {
                let Some(ci) = self.board.column_index(col) else {
                    return false;
                };
                let Some(i) = self.board.columns[ci].cards.iter().position(|k| k.id == id) else {
                    return false;
                };
                let j = if down { i + 1 } else { i.wrapping_sub(1) };
                if down && j >= self.board.columns[ci].cards.len() || !down && i == 0 {
                    return false;
                }
                self.board.columns[ci].cards.swap(i, j);
            }
        }
        self.persist();
        self.refresh = true;
        true
    }

    /// Move the card named by `key` to the adjacent column.
    ///
    /// The one kanban verb the generic keymap has no key for: every other edit is
    /// covered by insert, delete and cut/paste, but "this is done now" is the
    /// motion the board exists for.
    fn move_card_sideways(&mut self, right: bool, key: &str) -> bool {
        let Some(id) = self
            .labels
            .get(&self.open_column)
            .and_then(|m| m.get(key))
            .copied()
        else {
            return false;
        };
        let Some((ci, ki)) = self.board.locate_card(id) else {
            return false;
        };
        let target = if right { ci + 1 } else { ci.wrapping_sub(1) };
        if right && target >= self.board.columns.len() || !right && ci == 0 {
            return false;
        }
        let op = BoardOp::Move {
            card: id,
            from_column: self.board.columns[ci].id,
            from_index: ki,
            to_column: self.board.columns[target].id,
            to_index: self.board.columns[target].cards.len(),
        };
        self.apply(&op, true);
        self.record(op);
        true
    }
}

// ---------------------------------------------------------------------------
// Row helpers
// ---------------------------------------------------------------------------

fn raw_of(elem: &FfonElement) -> &str {
    match elem {
        FfonElement::Str(s) => s.as_str(),
        FfonElement::Obj(o) => o.key.as_str(),
    }
}

/// The id of a row, read from the prefix before `<input>`.
///
/// Scoped to the prefix on purpose: a user who types `<id>9</id>` into a card
/// has it escaped on the way in, but reading the whole string would still be one
/// unescaping bug away from letting a card claim another card's identity.
fn row_id(raw: &str) -> Option<Id> {
    let prefix = match raw.find("<input>") {
        Some(at) => &raw[..at],
        None => raw,
    };
    tags::extract_id(prefix).and_then(|s| s.parse().ok())
}

/// A column title as it should be stored.
///
/// A trailing colon is the list's syntax for "this row is an object", not part of
/// the name: the app strips it the same way when a typed line creates an `Obj`
/// (`state::strip_trailing_colon`). Keeping it would store the punctuation and
/// then show it on the board, where there is no object convention to explain it.
fn column_title(text: &str) -> String {
    text.trim_end().trim_end_matches(':').trim_end().to_owned()
}

/// The text of a row, with the `<id>` prefix and `<input>` wrapper removed and
/// any escaping undone.
fn row_text(raw: &str) -> String {
    match tags::extract_input(raw) {
        Some(inner) => escape::unescape(&inner),
        None => tags::strip_display(raw),
    }
}

/// Byte index of character `n`, clamped to the end.
fn byte_offset(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len())
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl Provider for ProjectManagementProvider {
    fn name(&self) -> &str {
        "projectmanagement"
    }

    fn display_name(&self) -> String {
        register_translations();
        localize::t("projectmanagement-display-name")
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

    fn sync_ffon_body_children(&mut self, children: &[FfonElement]) {
        register_translations();
        self.ensure_loaded();
        self.reconcile(children);
    }

    fn commit_edit(&mut self, _old: &str, new: &str) -> bool {
        register_translations();
        if self.load_failed {
            self.error = Some(localize::t("pm-error-unreadable"));
            return false;
        }
        // Remembered rather than applied: the app has not yet handed back the
        // list this row belongs to, and the row itself still carries its blank
        // placeholder. `reconcile` picks this up when it does.
        self.pending_create = Some(row_text(new));
        true
    }

    fn delete_item(&mut self, _name: &str) -> bool {
        // The veto on the FFON delete path. The trait default is `false`, which
        // reads as "always refuse", so a capability provider has to answer.
        register_translations();
        if self.load_failed {
            self.error = Some(localize::t("pm-error-unreadable"));
            return false;
        }
        true
    }

    fn push_path(&mut self, segment: &str) {
        // Cards are leaves, so only the root level descends. Without this guard a
        // stale label from the card level could push a second segment and leave
        // the provider a level deeper than the cursor.
        if self.open_column.is_some() {
            return;
        }
        let resolved = self
            .labels
            .get(&None)
            .and_then(|m| m.get(segment))
            .copied()
            .or_else(|| segment.strip_prefix("c").and_then(|s| s.parse().ok()));
        if let Some(id) = resolved {
            self.open_column = Some(id);
            self.sync_rendered_path();
        }
    }

    fn pop_path(&mut self) {
        self.open_column = None;
        self.sync_rendered_path();
    }

    fn current_path(&self) -> &str {
        &self.rendered_path
    }

    fn set_current_path(&mut self, path: &str) {
        self.open_column = path
            .trim_start_matches('/')
            .strip_prefix('c')
            .and_then(|s| s.parse().ok());
        self.sync_rendered_path();
    }

    fn at_root(&self) -> bool {
        self.open_column.is_none()
    }

    fn set_config_path(&mut self, path: PathBuf) {
        // The trait's own escape hatch for tests, and the only way the app's
        // integration tests — a separate binary, where this crate is compiled
        // without `cfg(test)` and reached as a `Box<dyn Provider>` — can keep
        // away from the real board directory.
        self.root_override = Some(path);
        self.loaded = false;
        self.load_failed = false;
    }

    /// The children of the level the cursor is on, so a commit refreshes just
    /// that list. Without this the app falls back to rebuilding the provider
    /// root, which misroutes a descended path and leaves the level empty.
    fn fetch_subtree_children(&mut self) -> Option<Vec<FfonElement>> {
        self.open_column?;
        register_translations();
        self.ensure_loaded();
        Some(self.level_children())
    }

    fn needs_refresh(&self) -> bool {
        self.refresh
    }

    fn clear_needs_refresh(&mut self) {
        self.refresh = false;
    }

    fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    fn take_announcement(&mut self) -> Option<String> {
        self.announcement.take()
    }

    fn take_timeline_entries(&mut self) -> Vec<TimelineEntry> {
        std::mem::take(&mut self.timeline)
    }

    async fn undo(&mut self, entry: &TimelineEntry, _error: &mut String) {
        register_translations();
        if let Some((op, label)) = decode_op(entry) {
            self.apply(&op, false);
            self.clamp_focus();
            self.say_key("pm-say-undone", &[("what", label)]);
        }
    }

    async fn redo(&mut self, entry: &TimelineEntry, _error: &mut String) {
        register_translations();
        if let Some((op, label)) = decode_op(entry) {
            self.apply(&op, true);
            self.clamp_focus();
            self.say_key("pm-say-redone", &[("what", label)]);
        }
    }

    fn commands(&self) -> Vec<String> {
        vec![
            CMD_MOVE_UP.to_owned(),
            CMD_MOVE_DOWN.to_owned(),
            CMD_MOVE_LEFT.to_owned(),
            CMD_MOVE_RIGHT.to_owned(),
        ]
    }

    fn command_label(&self, cmd: &str) -> String {
        register_translations();
        match cmd {
            CMD_MOVE_UP => localize::t("pm-cmd-move-up"),
            CMD_MOVE_DOWN => localize::t("pm-cmd-move-down"),
            CMD_MOVE_LEFT => localize::t("pm-cmd-move-left"),
            CMD_MOVE_RIGHT => localize::t("pm-cmd-move-right"),
            other => other.to_owned(),
        }
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        elem_key: &str,
        _elem_type: i32,
        _error: &mut String,
    ) -> Option<FfonElement> {
        register_translations();
        self.ensure_loaded();
        let ok = match cmd {
            CMD_MOVE_UP => self.move_row_in_list(false, elem_key),
            CMD_MOVE_DOWN => self.move_row_in_list(true, elem_key),
            CMD_MOVE_LEFT => self.move_card_sideways(false, elem_key),
            CMD_MOVE_RIGHT => self.move_card_sideways(true, elem_key),
            _ => false,
        };
        if ok {
            Some(FfonElement::new_str(""))
        } else {
            None
        }
    }

    // ---- The board ------------------------------------------------------

    fn dashboard_kind(&self) -> DashboardKind {
        DashboardKind::Interactive
    }

    fn set_dashboard_palette(&mut self, palette: sicompass_sdk::DashboardPalette) {
        self.palette = palette;
    }

    fn dashboard_uses_app_undo(&self) -> bool {
        // The board's edits go onto the app's timeline as `ProviderOp` entries,
        // so Ctrl+Z belongs to the app here rather than being forwarded as a
        // keystroke this provider would have to reimplement against a second,
        // divergent undo stack.
        true
    }

    fn set_dashboard_entry(&mut self, path: &[usize]) {
        self.entry_path = path.to_vec();
    }

    fn enter_dashboard(&mut self) {
        register_translations();
        self.ensure_loaded();
        self.mode = BoardMode::Board;
        self.edit = None;
        self.suppress_text = None;
        // Open on exactly what the list cursor was standing on, so `d` continues
        // the user's train of thought. The mirror of what `leave_dashboard`
        // queues, and it has to come from the app: `current_path()` names the
        // column the user descended into, but nothing tells this provider which
        // row of it the cursor is on, because moving within a level never calls
        // `push_path`.
        //
        // A title has no card index, so the board opens on that column's first
        // card. Anything else is left where it was.
        match std::mem::take(&mut self.entry_path).as_slice() {
            [col] => self.focus = Focus { col: *col, row: 0 },
            [col, card, ..] => {
                self.focus = Focus {
                    col: *col,
                    row: *card,
                }
            }
            _ => {}
        }
        self.clamp_focus();
        self.say_focus();
    }

    fn leave_dashboard(&mut self) {
        // An open edit is kept rather than discarded: leaving is not a cancel,
        // and the text is one Ctrl+Z from being reverted anyway.
        if self.edit.is_some() {
            self.commit_edit_state();
        }
        self.mode = BoardMode::Board;
        self.suppress_text = None;
        self.refresh = true;
        // Put the list cursor on the card the board was showing. Entering already
        // follows the list — `enter_dashboard` opens on the column the cursor was
        // in — and without this the round trip is one-way: Escape drops the user
        // back wherever they pressed `d`, which after a few minutes of arranging
        // cards is nowhere near what they were last looking at.
        //
        // Skipped for a placeholder: there is no card to land on, so the column
        // row itself is as close as the list can get.
        if !self.on_placeholder() && self.board.columns.get(self.focus.col).is_some() {
            self.navigation = Some(NavigationRequest::SelectPath(vec![
                self.focus.col,
                self.focus.row,
            ]));
        }
    }

    fn take_dashboard_request(&mut self) -> Option<DashboardRequest> {
        self.dashboard_request.take()
    }

    fn take_navigation_request(&mut self) -> Option<NavigationRequest> {
        self.navigation.take()
    }

    fn dashboard_render(&mut self, cols: u16, rows: u16) -> DashboardFrame {
        register_translations();
        self.ensure_loaded();
        self.clamp_focus();
        let editing = self.edit.as_ref().map(|e| (e.text.as_str(), e.caret));
        // Resolved here rather than inside the renderer: `render` is pure drawing
        // and has no business reaching the localizer.
        let empty_label = localize::t("pm-board-empty-slot");
        let no_columns_label = localize::t("pm-board-no-columns");
        let view = View {
            focus: self.focus,
            editing,
            empty_label: &empty_label,
            no_columns_label: &no_columns_label,
            palette: self.palette,
        };
        render::render(&self.board, &view, cols, rows)
    }

    fn dashboard_key(&mut self, key: DashboardKey) -> bool {
        register_translations();
        self.ensure_loaded();
        match self.mode {
            BoardMode::Board => self.board_key(key),
            BoardMode::Insert => self.insert_key(key),
        }
    }

    fn dashboard_text(&mut self, text: &str) {
        // The duplicate-keystroke guard. SDL fires KEYDOWN before TEXTINPUT, so
        // the `a` or `i` that opened insert mode arrives here immediately after
        // as text. `lib_terminal` sidesteps this by never encoding an unmodified
        // `Char`; this provider cannot, because those letters are its commands.
        if let Some(pending) = self.suppress_text.take()
            && pending == text
        {
            return;
        }
        if self.mode != BoardMode::Insert {
            return;
        }
        self.insert_text(text);
    }

    fn dashboard_paste(&mut self, text: &str) {
        register_translations();
        self.ensure_loaded();
        match self.mode {
            // Inside a card, a paste is text: newlines would make one card into
            // several mid-word, so they become spaces.
            BoardMode::Insert => {
                let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
                self.insert_text(&flat);
            }
            BoardMode::Board => self.paste_text(text),
        }
    }
}

impl ProjectManagementProvider {
    /// Keys in board mode.
    ///
    /// The bare keys act on what is focused; the Ctrl keys act on the list the
    /// focus is in. On a card those two coincide and Ctrl+A is an alias for `a`;
    /// on a column head they differ, which is what makes columns creatable from
    /// the board at all.
    fn board_key(&mut self, key: DashboardKey) -> bool {
        use DashboardKeysym as K;
        let ctrl = key.ctrl;
        match key.keysym {
            K::Left => self.move_column(-1),
            K::Right => self.move_column(1),
            K::Up => self.move_row(false),
            K::Down => self.move_row(true),
            K::Escape => {
                self.dashboard_request = Some(DashboardRequest::Leave);
                true
            }
            K::Delete => {
                self.delete_focused();
                true
            }
            K::Char('h') if !ctrl => self.move_column(-1),
            K::Char('l') if !ctrl => self.move_column(1),
            K::Char('k') if !ctrl => self.move_row(false),
            K::Char('j') if !ctrl => self.move_row(true),
            // `i` and `a` edit the focused card, caret at the start or the end.
            // The same meaning they carry everywhere else in the app. On an empty
            // column's placeholder there is nothing to edit, so they start the
            // first card instead — the list behaves the same way, because an
            // empty level there *is* an `<input>` slot.
            K::Char('i') if !ctrl => {
                self.suppress_text = Some("i".to_owned());
                self.begin_rename(false);
                true
            }
            K::Char('a') if !ctrl => {
                self.suppress_text = Some("a".to_owned());
                self.begin_rename(true);
                true
            }
            // `o` and `O` open a new card below or above, the way they open a
            // new line in a vim-shaped editor. Ctrl+I and Ctrl+A are the app's
            // own insert-before and append-after, and on a board of cards those
            // mean the same two things — kept as aliases so the keys that work
            // in the list keep working here.
            K::Char('o') if !ctrl && !key.shift => {
                self.suppress_text = Some("o".to_owned());
                self.begin_new_card(self.below());
                true
            }
            K::Char('o') if !ctrl && key.shift => {
                self.suppress_text = Some("O".to_owned());
                self.begin_new_card(self.above());
                true
            }
            K::Char('i') if ctrl => {
                self.begin_new_card(self.above());
                true
            }
            K::Char('a') if ctrl => {
                self.begin_new_card(self.below());
                true
            }
            K::Char('d') if ctrl => {
                self.delete_focused();
                true
            }
            K::Char('x') if ctrl => {
                self.copy_focused(true);
                true
            }
            K::Char('c') if ctrl => {
                self.copy_focused(false);
                true
            }
            K::Char('v') if ctrl && !key.shift => {
                self.paste();
                true
            }
            _ => false,
        }
    }

    /// Keys in insert mode. Printable characters arrive through `dashboard_text`.
    fn insert_key(&mut self, key: DashboardKey) -> bool {
        use DashboardKeysym as K;
        match key.keysym {
            // Escape keeps what was typed rather than discarding it, matching
            // the app's own Insert to General transition. Anyone who wanted the
            // old text back is one Ctrl+Z away, and that is undoable in turn.
            K::Enter | K::Escape => {
                self.commit_edit_state();
                true
            }
            K::Backspace => {
                self.backspace();
                true
            }
            K::Delete => {
                self.delete_forward();
                true
            }
            K::Left => {
                if let Some(e) = self.edit.as_mut() {
                    e.caret = e.caret.saturating_sub(1);
                }
                true
            }
            K::Right => {
                if let Some(e) = self.edit.as_mut() {
                    e.caret = (e.caret + 1).min(e.text.chars().count());
                }
                true
            }
            K::Home => {
                if let Some(e) = self.edit.as_mut() {
                    e.caret = 0;
                }
                true
            }
            K::End => {
                if let Some(e) = self.edit.as_mut() {
                    e.caret = e.text.chars().count();
                }
                true
            }
            _ => false,
        }
    }
}

/// Read a board op back out of a timeline entry, with its localized label.
///
/// Anything that is not one of ours is ignored rather than guessed at: the tab's
/// timeline also carries the app's own `Structural` entries for the list surface.
fn decode_op(entry: &TimelineEntry) -> Option<(BoardOp, String)> {
    let TimelineEntry::ProviderOp { payload, label, .. } = entry else {
        return None;
    };
    let json = payload.as_str()?;
    let op: BoardOp = serde_json::from_str(json).ok()?;
    Some((op, label.clone()))
}

pub fn register() {
    register_translations();
    register_provider_factory("projectmanagement", || {
        Box::new(ProjectManagementProvider::new())
    });
    register_builtin_manifest(
        BuiltinManifest::new("projectmanagement", "project management").enable_by_default(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A provider backed by a real, disposable directory.
    ///
    /// The `cfg!(test)` default already stops a forgotten override reaching the
    /// user's board, but a test that asserts anything about the store needs
    /// somewhere real to look.
    fn provider(dir: &TempDir) -> ProjectManagementProvider {
        register_translations();
        let mut p = ProjectManagementProvider::new();
        p.root_override = Some(dir.path().join("board"));
        p
    }

    /// Two columns and three cards, and no disk at all.
    fn seeded() -> ProjectManagementProvider {
        register_translations();
        let mut p = ProjectManagementProvider::new();
        p.loaded = true;
        let mut todo = Column::new(1, "To do");
        todo.cards.push(Card::new(2, "fix login"));
        todo.cards.push(Card::new(3, "write docs"));
        let mut doing = Column::new(4, "Doing");
        doing.cards.push(Card::new(5, "kanban ui"));
        p.board.columns.push(todo);
        p.board.columns.push(doing);
        p.board.reseat_counter();
        p
    }

    /// What a screen reader would read: every row's display text, tags gone.
    fn labels(elems: &[FfonElement]) -> Vec<String> {
        elems.iter().map(|e| row_text(raw_of(e))).collect()
    }

    fn cards(p: &ProjectManagementProvider, col: usize) -> Vec<String> {
        p.board.columns[col]
            .cards
            .iter()
            .map(|c| c.text.clone())
            .collect()
    }

    fn key(k: DashboardKeysym) -> DashboardKey {
        DashboardKey {
            keysym: k,
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    fn shift(k: DashboardKeysym) -> DashboardKey {
        DashboardKey {
            keysym: k,
            ctrl: false,
            shift: true,
            alt: false,
        }
    }

    fn ctrl(k: DashboardKeysym) -> DashboardKey {
        DashboardKey {
            keysym: k,
            ctrl: true,
            shift: false,
            alt: false,
        }
    }

    /// Descend into a column the way the app does: render the level, then push
    /// the label the user was looking at. `push_path` resolves a *displayed
    /// label*, so a level that was never rendered has no labels to resolve
    /// against.
    fn descend(p: &mut ProjectManagementProvider, title: &str) {
        let _ = p.fetch();
        p.push_path(title);
    }

    /// Open the board and type a card at the cursor.
    fn add_card(p: &mut ProjectManagementProvider, text: &str) {
        p.dashboard_key(key(DashboardKeysym::Char('o')));
        p.dashboard_text(text);
        p.dashboard_key(key(DashboardKeysym::Enter));
    }

    // ---- The list surface ----------------------------------------------

    #[test]
    fn the_root_lists_the_columns_and_a_column_lists_its_cards() {
        let mut p = seeded();
        assert_eq!(labels(&p.fetch()), vec!["To do", "Doing"]);
        p.push_path("To do");
        assert_eq!(labels(&p.fetch()), vec!["fix login", "write docs"]);
        p.pop_path();
        assert_eq!(labels(&p.fetch()), vec!["To do", "Doing"]);
    }

    #[test]
    fn a_column_is_a_branch_and_a_card_is_a_leaf() {
        // Obj versus Str *is* the depth cap: the app descends into an Obj and
        // cannot descend into a Str, so a card has nowhere to grow children.
        let mut p = seeded();
        assert!(p.fetch().iter().all(|e| e.is_obj()), "columns must be Obj");
        p.push_path("To do");
        assert!(p.fetch().iter().all(|e| e.is_str()), "cards must be Str");
    }

    #[test]
    fn a_card_cannot_be_descended_into() {
        let mut p = seeded();
        descend(&mut p, "To do");
        let before = p.current_path().to_owned();
        p.push_path("fix login");
        assert_eq!(p.current_path(), before, "a card is a leaf");
    }

    #[test]
    fn an_empty_level_renders_a_placeholder_rather_than_nothing() {
        let mut p = ProjectManagementProvider::new();
        p.loaded = true;
        register_translations();
        assert_eq!(p.fetch().len(), 1);
        assert!(labels(&p.fetch())[0].contains("no columns"));
    }

    #[test]
    fn the_placeholder_never_becomes_a_real_column() {
        let mut p = ProjectManagementProvider::new();
        p.loaded = true;
        register_translations();
        let placeholder = p.fetch();
        p.sync_ffon_body_children(&placeholder);
        assert!(p.board.columns.is_empty());
    }

    #[test]
    fn a_row_with_no_id_becomes_a_new_column() {
        let mut p = seeded();
        let mut rows = p.fetch();
        rows.push(FfonElement::new_obj(tags::format_input("Done")));
        p.sync_ffon_body_children(&rows);
        assert_eq!(
            p.board
                .columns
                .iter()
                .map(|c| c.title.as_str())
                .collect::<Vec<_>>(),
            vec!["To do", "Doing", "Done"]
        );
    }

    #[test]
    fn a_removed_row_deletes_its_card() {
        let mut p = seeded();
        descend(&mut p, "To do");
        let mut rows = p.fetch();
        rows.remove(0);
        p.sync_ffon_body_children(&rows);
        assert_eq!(cards(&p, 0), vec!["write docs"]);
    }

    #[test]
    fn a_trailing_colon_is_object_syntax_and_is_not_part_of_the_name() {
        // Typing a trailing colon in the list is how a row becomes an `Obj`, and
        // the app strips it before storing the key. Keeping it would store the
        // punctuation and then show it on the board, where nothing explains it.
        let mut p = seeded();
        let mut rows = p.fetch();
        rows.push(FfonElement::new_obj(tags::format_input("Done:")));
        p.sync_ffon_body_children(&rows);
        assert_eq!(p.board.columns[2].title, "Done");
    }

    #[test]
    fn a_colon_already_on_disk_is_cleaned_up_on_load() {
        // Stripping only on write leaves every title written before that fix
        // still carrying one, and the board goes on showing it until the user
        // happens to rename the column.
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        let mut stale = crate::board::Board::new();
        stale.columns.push(Column::new(1, "To do:"));
        crate::store::save_board(&root, &stale).unwrap();

        let mut p = provider(&dir);
        p.ensure_loaded();
        assert_eq!(p.board.columns[0].title, "To do");
        assert_eq!(labels(&p.fetch()), vec!["To do"]);
    }

    #[test]
    fn a_colon_inside_a_title_is_left_alone() {
        let mut p = seeded();
        let mut rows = p.fetch();
        rows.push(FfonElement::new_obj(tags::format_input("Q4: roadmap")));
        p.sync_ffon_body_children(&rows);
        assert_eq!(p.board.columns[2].title, "Q4: roadmap");
    }

    #[test]
    fn a_card_keeps_its_colon() {
        // Only a column is an object, so only a column title carries the syntax.
        let mut p = seeded();
        descend(&mut p, "To do");
        let mut rows = p.fetch();
        rows.push(FfonElement::Str(tags::format_input("note: check this")));
        p.sync_ffon_body_children(&rows);
        assert_eq!(cards(&p, 0)[2], "note: check this");
    }

    #[test]
    fn a_rename_keeps_the_id_so_two_identical_titles_stay_apart() {
        // The reason rows carry an `<id>` at all: `commit_edit(old, new)` cannot
        // tell two identically titled siblings apart, because `old` is only the
        // previous display text.
        let mut p = seeded();
        let rows = vec![
            FfonElement::new_obj(ProjectManagementProvider::row_label(1, "Backlog")),
            FfonElement::new_obj(ProjectManagementProvider::row_label(4, "Backlog")),
        ];
        p.sync_ffon_body_children(&rows);
        assert_eq!(p.board.columns[0].id, 1);
        assert_eq!(p.board.columns[1].id, 4);
        assert_eq!(p.board.columns[1].cards[0].text, "kanban ui");
    }

    #[test]
    fn a_list_is_identified_from_its_row_ids_not_from_the_cursor() {
        // Undo hands back a list from wherever the reversed edit happened, and
        // does not move the provider's path to match.
        let mut p = seeded();
        assert!(p.at_root(), "cursor is at the root");
        let rows = vec![FfonElement::Str(ProjectManagementProvider::row_label(
            5,
            "kanban ui, renamed",
        ))];
        p.sync_ffon_body_children(&rows);
        assert_eq!(p.board.columns[1].cards[0].text, "kanban ui, renamed");
        assert_eq!(cards(&p, 0).len(), 2, "the other column is untouched");
    }

    #[test]
    fn a_card_moved_between_columns_does_not_exist_twice() {
        let mut p = seeded();
        descend(&mut p, "Doing");
        let rows = vec![
            FfonElement::Str(ProjectManagementProvider::row_label(5, "kanban ui")),
            FfonElement::Str(ProjectManagementProvider::row_label(2, "fix login")),
        ];
        p.sync_ffon_body_children(&rows);
        assert_eq!(cards(&p, 1).len(), 2);
        assert_eq!(cards(&p, 0).len(), 1);
        assert_eq!(p.board.locate_card(2), Some((1, 1)));
    }

    #[test]
    fn card_text_that_looks_like_a_tag_stays_text() {
        let mut p = seeded();
        p.board.columns[0].cards[0].text = "<button>submit</button>Send".to_owned();
        descend(&mut p, "To do");
        let rows = p.fetch();
        assert!(
            !tags::has_button(raw_of(&rows[0])),
            "a card must not forge a button"
        );
        assert_eq!(labels(&rows)[0], "<button>submit</button>Send");
    }

    #[test]
    fn a_delete_is_refused_while_the_board_could_not_be_read() {
        let mut p = seeded();
        p.load_failed = true;
        assert!(!p.delete_item("anything"));
        assert!(p.take_error().is_some());
    }

    #[test]
    fn nothing_is_written_while_the_board_could_not_be_read() {
        let dir = TempDir::new().unwrap();
        let mut p = provider(&dir);
        p.load_failed = true;
        p.loaded = true;
        p.board.columns.push(Column::new(1, "ghost"));
        p.persist();
        assert!(
            !dir.path().join("board").exists(),
            "a failed load must never lead to a write"
        );
    }

    // ---- Persistence ----------------------------------------------------

    #[test]
    fn a_board_written_by_one_provider_is_read_by_the_next() {
        let dir = TempDir::new().unwrap();
        {
            let mut p = provider(&dir);
            p.ensure_loaded();
            let mut rows = p.fetch();
            rows.push(FfonElement::new_obj(tags::format_input("To do")));
            p.sync_ffon_body_children(&rows);
        }
        let mut p = provider(&dir);
        assert_eq!(labels(&p.fetch()), vec!["To do"]);
    }

    #[test]
    fn the_path_survives_a_restart() {
        let mut p = seeded();
        descend(&mut p, "Doing");
        let saved = p.current_path().to_owned();
        let mut fresh = seeded();
        fresh.set_current_path(&saved);
        assert_eq!(labels(&fresh.fetch()), vec!["kanban ui"]);
    }

    // ---- Only cards are focusable ---------------------------------------

    #[test]
    fn the_cursor_starts_on_a_card_not_on_a_head() {
        let mut p = seeded();
        p.enter_dashboard();
        assert_eq!(p.focus, Focus { col: 0, row: 0 });
        assert!(!p.on_placeholder());
    }

    #[test]
    fn up_from_the_first_card_stays_on_it() {
        // There is no head to land on any more, so the top card is the top.
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(key(DashboardKeysym::Up));
        assert_eq!(p.focus, Focus { col: 0, row: 0 });
    }

    #[test]
    fn arrows_move_between_columns_and_within_one() {
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(key(DashboardKeysym::Down));
        assert_eq!(p.focus, Focus { col: 0, row: 1 });
        p.dashboard_key(key(DashboardKeysym::Down));
        assert_eq!(
            p.focus,
            Focus { col: 0, row: 1 },
            "clamped at the last card"
        );
        p.dashboard_key(key(DashboardKeysym::Right));
        assert_eq!(p.focus, Focus { col: 1, row: 0 }, "Doing holds one card");
    }

    #[test]
    fn hjkl_move_the_same_way_the_arrows_do() {
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(key(DashboardKeysym::Char('j')));
        assert_eq!(p.focus.row, 1);
        p.dashboard_key(key(DashboardKeysym::Char('l')));
        assert_eq!(p.focus.col, 1);
        p.dashboard_key(key(DashboardKeysym::Char('h')));
        assert_eq!(p.focus.col, 0);
        p.dashboard_key(key(DashboardKeysym::Char('k')));
        assert_eq!(p.focus.row, 0);
    }

    #[test]
    fn an_empty_column_still_holds_the_cursor_on_its_placeholder() {
        let mut p = seeded();
        p.board.columns.push(Column::new(9, "Done"));
        p.enter_dashboard();
        p.dashboard_key(key(DashboardKeysym::Right));
        p.dashboard_key(key(DashboardKeysym::Right));
        assert_eq!(p.focus, Focus { col: 2, row: 0 });
        assert!(p.on_placeholder());
    }

    #[test]
    fn the_board_opens_on_the_card_the_list_cursor_was_on() {
        let mut p = seeded();
        descend(&mut p, "To do");
        // The cursor is on the second card of the first column.
        p.set_dashboard_entry(&[0, 1]);
        p.enter_dashboard();
        assert_eq!(p.focus, Focus { col: 0, row: 1 });
    }

    #[test]
    fn from_a_column_title_the_board_opens_on_that_columns_first_card() {
        let mut p = seeded();
        // The cursor is on the second column's title, at the provider root.
        p.set_dashboard_entry(&[1]);
        p.enter_dashboard();
        assert_eq!(p.focus, Focus { col: 1, row: 0 });
    }

    #[test]
    fn entering_and_leaving_are_mirror_images() {
        // Whatever the board was showing is where the list lands, and whatever
        // the list was on is where the board opens.
        let mut p = seeded();
        p.set_dashboard_entry(&[0, 1]);
        p.enter_dashboard();
        assert_eq!(p.focus, Focus { col: 0, row: 1 });
        p.leave_dashboard();
        assert_eq!(
            p.take_navigation_request(),
            Some(NavigationRequest::SelectPath(vec![0, 1]))
        );
    }

    #[test]
    fn an_entry_path_past_the_end_of_the_board_is_clamped() {
        // The list and the board can disagree after an edit in another tab.
        let mut p = seeded();
        p.set_dashboard_entry(&[9, 9]);
        p.enter_dashboard();
        assert_eq!(
            p.focus,
            Focus { col: 1, row: 0 },
            "clamped to something real"
        );
    }

    #[test]
    fn entering_from_the_provider_root_with_no_path_keeps_the_cursor() {
        let mut p = seeded();
        p.focus = Focus { col: 1, row: 0 };
        p.set_dashboard_entry(&[]);
        p.enter_dashboard();
        assert_eq!(p.focus, Focus { col: 1, row: 0 });
    }

    #[test]
    fn every_focus_move_says_where_it_landed() {
        let mut p = seeded();
        p.enter_dashboard();
        let _ = p.take_announcement();
        p.dashboard_key(key(DashboardKeysym::Down));
        let said = p.take_announcement().expect("a move must be announced");
        assert!(said.contains("write docs"), "got {said:?}");
        assert!(said.contains("To do"), "got {said:?}");
    }

    #[test]
    fn an_empty_columns_placeholder_announces_the_column_as_empty() {
        let mut p = seeded();
        p.board.columns.push(Column::new(9, "Done"));
        p.enter_dashboard();
        p.focus = Focus { col: 2, row: 0 };
        p.say_focus();
        let said = p.take_announcement().unwrap();
        assert!(said.contains("Done"), "got {said:?}");
        // Compared against the resolved key rather than the English word: the
        // Fluent localizer is process-global, so a test elsewhere switching the
        // locale would otherwise make this one fail for the wrong reason.
        let mut a = localize::Args::new();
        a.set("index", "3");
        a.set("total", "3");
        a.set("title", "Done");
        assert_eq!(
            said,
            localize::t_args("pm-say-column-empty", &a),
            "got {said:?}"
        );
    }

    // ---- Insert mode ----------------------------------------------------

    #[test]
    fn i_puts_the_caret_at_the_start_and_a_puts_it_at_the_end() {
        for (k, want) in [('i', 0usize), ('a', 9usize)] {
            let mut p = seeded();
            p.enter_dashboard();
            p.dashboard_key(key(DashboardKeysym::Char(k)));
            let edit = p.edit.as_ref().expect("insert mode should be open");
            assert_eq!(edit.text, "fix login");
            assert_eq!(edit.caret, want, "`{k}` caret");
            assert!(!edit.creating, "`{k}` must not create a card");
        }
    }

    #[test]
    fn neither_i_nor_a_adds_a_card_to_a_column_that_has_some() {
        for k in ['i', 'a'] {
            let mut p = seeded();
            p.enter_dashboard();
            let before = p.board.card_count();
            p.dashboard_key(key(DashboardKeysym::Char(k)));
            p.dashboard_key(key(DashboardKeysym::Escape));
            assert_eq!(p.board.card_count(), before, "`{k}` must not create");
        }
    }

    #[test]
    fn o_opens_insert_mode_and_does_not_type_its_own_letter() {
        // SDL fires KEYDOWN before TEXTINPUT, so the letter that opened insert
        // mode arrives again as text. Without the guard the card reads "ox".
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(key(DashboardKeysym::Char('o')));
        p.dashboard_text("o");
        p.dashboard_text("x");
        assert_eq!(p.edit.as_ref().unwrap().text, "x");
    }

    #[test]
    fn the_guard_never_eats_a_real_letter_later_on() {
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(key(DashboardKeysym::Char('o')));
        // The duplicate never arrives (a different keyboard path, say), and the
        // user types the same letter on purpose two keystrokes later.
        p.dashboard_text("b");
        p.dashboard_text("o");
        assert_eq!(p.edit.as_ref().unwrap().text, "bo");
    }

    #[test]
    fn o_opens_a_card_below_and_shift_o_above() {
        let mut p = seeded();
        p.enter_dashboard();
        add_card(&mut p, "below");
        p.focus = Focus { col: 0, row: 0 };
        p.dashboard_key(shift(DashboardKeysym::Char('o')));
        p.dashboard_text("above");
        p.dashboard_key(key(DashboardKeysym::Enter));
        assert_eq!(
            cards(&p, 0),
            vec!["above", "fix login", "below", "write docs"]
        );
    }

    #[test]
    fn ctrl_i_and_ctrl_a_insert_before_and_after_like_they_do_in_the_list() {
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(ctrl(DashboardKeysym::Char('a')));
        p.dashboard_text("after");
        p.dashboard_key(key(DashboardKeysym::Enter));
        p.focus = Focus { col: 0, row: 0 };
        p.dashboard_key(ctrl(DashboardKeysym::Char('i')));
        p.dashboard_text("before");
        p.dashboard_key(key(DashboardKeysym::Enter));
        assert_eq!(
            cards(&p, 0),
            vec!["before", "fix login", "after", "write docs"]
        );
    }

    // ---- The empty-column placeholder -----------------------------------

    #[test]
    fn ctrl_a_on_the_placeholder_puts_the_first_card_in_the_column() {
        let mut p = seeded();
        p.board.columns.push(Column::new(9, "Done"));
        p.enter_dashboard();
        p.focus = Focus { col: 2, row: 0 };
        p.dashboard_key(ctrl(DashboardKeysym::Char('a')));
        p.dashboard_text("shipped");
        p.dashboard_key(key(DashboardKeysym::Enter));
        assert_eq!(cards(&p, 2), vec!["shipped"]);
    }

    #[test]
    fn every_insert_key_starts_the_first_card_on_a_placeholder() {
        // `i` and `a` have nothing to edit there, so they create rather than
        // being dead keys — the list behaves the same way, because an empty level
        // there *is* an `<input>` slot.
        for k in [
            key(DashboardKeysym::Char('i')),
            key(DashboardKeysym::Char('a')),
            key(DashboardKeysym::Char('o')),
            shift(DashboardKeysym::Char('o')),
            ctrl(DashboardKeysym::Char('i')),
            ctrl(DashboardKeysym::Char('a')),
        ] {
            let mut p = seeded();
            p.board.columns.push(Column::new(9, "Done"));
            p.enter_dashboard();
            p.focus = Focus { col: 2, row: 0 };
            p.dashboard_key(k);
            p.dashboard_text("first");
            p.dashboard_key(key(DashboardKeysym::Enter));
            assert_eq!(cards(&p, 2), vec!["first"], "for {k:?}");
        }
    }

    #[test]
    fn delete_on_a_placeholder_does_nothing() {
        let mut p = seeded();
        p.board.columns.push(Column::new(9, "Done"));
        p.enter_dashboard();
        p.focus = Focus { col: 2, row: 0 };
        p.dashboard_key(ctrl(DashboardKeysym::Char('d')));
        assert_eq!(p.board.columns.len(), 3, "the column must survive");
        assert!(p.take_timeline_entries().is_empty());
    }

    #[test]
    fn pasting_onto_a_placeholder_puts_the_card_in_the_empty_column() {
        let mut p = seeded();
        p.board.columns.push(Column::new(9, "Done"));
        p.enter_dashboard();
        p.dashboard_key(ctrl(DashboardKeysym::Char('c')));
        p.focus = Focus { col: 2, row: 0 };
        p.dashboard_key(ctrl(DashboardKeysym::Char('v')));
        assert_eq!(cards(&p, 2), vec!["fix login"]);
    }

    // ---- Columns are the list's job -------------------------------------

    #[test]
    fn the_board_never_touches_a_column() {
        // Only cards are focusable, so no board gesture can reach a column. This
        // is the guard on that: whatever the keys do, the column list is the same
        // list afterwards.
        let mut p = seeded();
        p.enter_dashboard();
        let before: Vec<(Id, String)> = p
            .board
            .columns
            .iter()
            .map(|c| (c.id, c.title.clone()))
            .collect();
        for k in [
            key(DashboardKeysym::Char('i')),
            key(DashboardKeysym::Char('a')),
            key(DashboardKeysym::Char('o')),
            shift(DashboardKeysym::Char('o')),
            ctrl(DashboardKeysym::Char('i')),
            ctrl(DashboardKeysym::Char('a')),
            ctrl(DashboardKeysym::Char('d')),
            ctrl(DashboardKeysym::Char('x')),
            ctrl(DashboardKeysym::Char('c')),
            ctrl(DashboardKeysym::Char('v')),
            key(DashboardKeysym::Delete),
        ] {
            p.dashboard_key(k);
            p.dashboard_text("x");
            p.dashboard_key(key(DashboardKeysym::Enter));
        }
        let after: Vec<(Id, String)> = p
            .board
            .columns
            .iter()
            .map(|c| (c.id, c.title.clone()))
            .collect();
        assert_eq!(
            before, after,
            "the board must not create, rename or delete a column"
        );
    }

    // ---- Editing --------------------------------------------------------

    #[test]
    fn a_creation_typed_into_and_left_empty_is_cancelled_not_kept() {
        let mut p = seeded();
        p.enter_dashboard();
        let before = p.board.card_count();
        p.dashboard_key(key(DashboardKeysym::Char('o')));
        p.dashboard_key(key(DashboardKeysym::Escape));
        assert_eq!(p.board.card_count(), before);
        assert!(
            p.take_timeline_entries().is_empty(),
            "an edit that left no trace must leave no undo step"
        );
    }

    #[test]
    fn escape_in_insert_returns_to_the_board_and_does_not_leave_it() {
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(key(DashboardKeysym::Char('i')));
        assert_eq!(p.mode, BoardMode::Insert);
        p.dashboard_key(key(DashboardKeysym::Escape));
        assert_eq!(p.mode, BoardMode::Board);
        assert_eq!(
            p.take_dashboard_request(),
            None,
            "escape must not also leave"
        );
    }

    #[test]
    fn escape_on_the_board_asks_the_app_to_leave() {
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(key(DashboardKeysym::Escape));
        assert_eq!(p.take_dashboard_request(), Some(DashboardRequest::Leave));
        assert_eq!(p.take_dashboard_request(), None, "two-call semantics");
    }

    #[test]
    fn the_caret_moves_and_edits_land_where_it_is() {
        let mut p = seeded();
        p.enter_dashboard();
        p.focus = Focus { col: 1, row: 0 };
        p.dashboard_key(key(DashboardKeysym::Char('i')));
        p.dashboard_text("new ");
        p.dashboard_key(key(DashboardKeysym::Enter));
        assert_eq!(cards(&p, 1), vec!["new kanban ui"]);
    }

    #[test]
    fn editing_multibyte_text_does_not_split_a_character() {
        let mut p = seeded();
        p.board.columns[1].cards[0].text = "héllo wörld".to_owned();
        p.enter_dashboard();
        p.focus = Focus { col: 1, row: 0 };
        p.dashboard_key(key(DashboardKeysym::Char('a')));
        p.dashboard_key(key(DashboardKeysym::Backspace));
        p.dashboard_key(key(DashboardKeysym::Home));
        p.dashboard_key(key(DashboardKeysym::Delete));
        p.dashboard_key(key(DashboardKeysym::Enter));
        assert_eq!(cards(&p, 1), vec!["éllo wörl"]);
    }

    #[test]
    fn ctrl_d_and_delete_both_remove_the_focused_card() {
        for k in [
            ctrl(DashboardKeysym::Char('d')),
            key(DashboardKeysym::Delete),
        ] {
            let mut p = seeded();
            p.enter_dashboard();
            p.dashboard_key(k);
            assert_eq!(cards(&p, 0), vec!["write docs"]);
        }
    }

    #[test]
    fn cut_then_paste_moves_a_card_to_another_column() {
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(ctrl(DashboardKeysym::Char('x')));
        p.focus = Focus { col: 1, row: 0 };
        p.dashboard_key(ctrl(DashboardKeysym::Char('v')));
        assert_eq!(cards(&p, 0), vec!["write docs"]);
        assert_eq!(cards(&p, 1), vec!["kanban ui", "fix login"]);
    }

    #[test]
    fn copy_then_paste_duplicates_with_a_different_id() {
        // Ids are minted once and never reused. A pasted copy sharing the
        // original's id would make `locate_card` find whichever came first, and
        // every later edit would land on the wrong card.
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(ctrl(DashboardKeysym::Char('c')));
        p.dashboard_key(ctrl(DashboardKeysym::Char('v')));
        let c = &p.board.columns[0].cards;
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].text, c[1].text);
        assert_ne!(c[0].id, c[1].id);
    }

    #[test]
    fn pasting_with_an_empty_clipboard_says_so_instead_of_doing_nothing() {
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(ctrl(DashboardKeysym::Char('v')));
        assert!(p.take_error().is_some());
        assert_eq!(p.board.card_count(), 3);
    }

    #[test]
    fn clipboard_text_becomes_one_card_per_line() {
        let mut p = seeded();
        p.enter_dashboard();
        p.focus = Focus { col: 1, row: 0 };
        p.dashboard_paste("alpha\n\n  beta  \ngamma\n");
        assert_eq!(cards(&p, 1), vec!["kanban ui", "alpha", "beta", "gamma"]);
    }

    #[test]
    fn a_paste_inside_a_card_stays_one_card() {
        let mut p = seeded();
        p.enter_dashboard();
        p.focus = Focus { col: 1, row: 0 };
        p.dashboard_key(key(DashboardKeysym::Char('i')));
        p.dashboard_paste("one\ntwo ");
        p.dashboard_key(key(DashboardKeysym::Enter));
        assert_eq!(cards(&p, 1), vec!["one twokanban ui"]);
    }

    // ---- Undo -----------------------------------------------------------

    /// Every board edit is one entry, and feeding it back reverses it exactly.
    ///
    /// Columns only, deliberately: the id counter is *not* part of what an undo
    /// restores. It only ever moves forward, so an id handed out once is never
    /// handed out again even after the card holding it is undone away. See
    /// `Board::reseat_counter`.
    fn round_trips(mut p: ProjectManagementProvider, act: impl Fn(&mut ProjectManagementProvider)) {
        p.enter_dashboard();
        let before = p.board.columns.clone();
        act(&mut p);
        let after = p.board.columns.clone();
        assert_ne!(before, after, "the action must actually change the board");

        let entries = p.take_timeline_entries();
        assert_eq!(entries.len(), 1, "one edit, one undo step");

        let mut err = String::new();
        sicompass_sdk::block_on(p.undo(&entries[0], &mut err));
        assert_eq!(
            p.board.columns, before,
            "undo must restore the board exactly"
        );

        sicompass_sdk::block_on(p.redo(&entries[0], &mut err));
        assert_eq!(
            p.board.columns, after,
            "redo must be the undo's exact inverse"
        );
    }

    #[test]
    fn adding_a_card_round_trips() {
        round_trips(seeded(), |p| add_card(p, "ship it"));
    }

    #[test]
    fn adding_the_first_card_to_an_empty_column_round_trips() {
        let mut p = seeded();
        p.board.columns.push(Column::new(9, "Done"));
        p.board.reseat_counter();
        round_trips(p, |p| {
            p.focus = Focus { col: 2, row: 0 };
            add_card(p, "shipped");
        });
    }

    #[test]
    fn deleting_a_card_round_trips() {
        round_trips(seeded(), |p| {
            p.focus = Focus { col: 0, row: 1 };
            p.dashboard_key(ctrl(DashboardKeysym::Char('d')));
        });
    }

    #[test]
    fn renaming_a_card_round_trips() {
        round_trips(seeded(), |p| {
            p.dashboard_key(key(DashboardKeysym::Char('a')));
            p.dashboard_text(" now");
            p.dashboard_key(key(DashboardKeysym::Enter));
        });
    }

    #[test]
    fn pasting_a_card_round_trips() {
        let mut p = seeded();
        p.clip = Some(Clip::Card(CardData {
            id: 99,
            text: "from the clipboard".to_owned(),
        }));
        round_trips(p, |p| {
            p.focus = Focus { col: 1, row: 0 };
            p.dashboard_key(ctrl(DashboardKeysym::Char('v')));
        });
    }

    #[test]
    fn a_rename_to_the_same_text_records_nothing() {
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(key(DashboardKeysym::Char('a')));
        p.dashboard_key(key(DashboardKeysym::Enter));
        assert!(p.take_timeline_entries().is_empty());
    }

    #[test]
    fn moving_a_card_sideways_round_trips() {
        let mut p = seeded();
        descend(&mut p, "To do");
        let rows = p.fetch();
        let key_label = raw_of(&rows[0]).to_owned();
        let before = p.board.clone();
        let mut err = String::new();
        assert!(p.move_card_sideways(true, &key_label));
        let after = p.board.clone();
        assert_eq!(cards(&p, 1).len(), 2);

        let entries = p.take_timeline_entries();
        assert_eq!(entries.len(), 1);
        sicompass_sdk::block_on(p.undo(&entries[0], &mut err));
        assert_eq!(p.board.columns, before.columns);
        sicompass_sdk::block_on(p.redo(&entries[0], &mut err));
        assert_eq!(p.board.columns, after.columns);
    }

    #[test]
    fn an_undone_card_does_not_hand_its_id_to_the_next_one() {
        // Undo shrinks the board; a redo still holds the card it removed. If the
        // counter followed the board down, the next new card would take that id
        // and the redo would insert a duplicate identity.
        let mut p = seeded();
        p.enter_dashboard();
        add_card(&mut p, "first");
        let first_id = p.board.columns[0].cards[1].id;
        let entries = p.take_timeline_entries();
        let mut err = String::new();
        sicompass_sdk::block_on(p.undo(&entries[0], &mut err));

        add_card(&mut p, "second");
        let second_id = p.board.columns[0].cards[1].id;
        assert_ne!(first_id, second_id, "an id must never be handed out twice");
    }

    #[test]
    fn an_entry_from_another_provider_is_ignored_rather_than_guessed_at() {
        // The tab's timeline also carries the app's own `Structural` entries for
        // the list surface, and undo hands this provider whatever it recorded.
        let mut p = seeded();
        let before = p.board.clone();
        let mut err = String::new();
        let foreign = TimelineEntry::ProviderOp {
            provider_idx: 0,
            command: "something-else".to_owned(),
            payload: FfonElement::Str("not our json".to_owned()),
            label: "x".to_owned(),
        };
        sicompass_sdk::block_on(p.undo(&foreign, &mut err));
        assert_eq!(p.board.columns, before.columns);
    }

    #[test]
    fn an_undo_says_what_it_undid() {
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(ctrl(DashboardKeysym::Char('d')));
        let entries = p.take_timeline_entries();
        let _ = p.take_announcement();
        let mut err = String::new();
        sicompass_sdk::block_on(p.undo(&entries[0], &mut err));
        let said = p.take_announcement().expect("undo must be announced");
        assert!(
            said.contains(&localize::t("pm-op-delete-card")),
            "got {said:?}"
        );
    }

    #[test]
    fn every_board_op_resolves_to_a_real_label() {
        // `record` builds the key as `pm-op-{command}`, so a command whose label
        // is missing renders the key itself into the undo history.
        register_translations();
        localize::set_locale("en-US");
        for command in ["add-card", "delete-card", "rename-card", "move-card"] {
            let key = format!("pm-op-{command}");
            assert_ne!(localize::t(&key), key, "{key} has no label");
        }
    }

    #[test]
    fn a_board_edit_asks_the_list_to_refresh() {
        // Without this the list view still shows the pre-edit board when the
        // user leaves the dashboard.
        let mut p = seeded();
        p.enter_dashboard();
        p.clear_needs_refresh();
        p.dashboard_key(ctrl(DashboardKeysym::Char('d')));
        assert!(p.needs_refresh());
    }

    #[test]
    fn a_board_edit_reaches_the_disk() {
        let dir = TempDir::new().unwrap();
        let mut p = provider(&dir);
        p.ensure_loaded();
        p.board.columns.push(Column::new(1, "To do"));
        p.enter_dashboard();
        add_card(&mut p, "ship it");

        let mut fresh = provider(&dir);
        descend(&mut fresh, "To do");
        assert_eq!(labels(&fresh.fetch()), vec!["ship it"]);
    }

    #[test]
    fn leaving_the_board_asks_for_the_cursor_to_follow_the_focused_card() {
        let mut p = seeded();
        p.enter_dashboard();
        p.focus = Focus { col: 1, row: 0 };
        p.leave_dashboard();
        assert_eq!(
            p.take_navigation_request(),
            Some(NavigationRequest::SelectPath(vec![1, 0]))
        );
        assert_eq!(p.take_navigation_request(), None, "two-call semantics");
    }

    #[test]
    fn leaving_an_empty_columns_slot_asks_for_nothing() {
        // There is no card to land on, so the column row the list already shows
        // is as close as it gets.
        let mut p = seeded();
        p.board.columns.push(Column::new(9, "Done"));
        p.enter_dashboard();
        p.focus = Focus { col: 2, row: 0 };
        p.leave_dashboard();
        assert_eq!(p.take_navigation_request(), None);
    }

    #[test]
    fn leaving_mid_edit_keeps_the_text_and_still_follows_the_card() {
        let mut p = seeded();
        p.enter_dashboard();
        p.dashboard_key(key(DashboardKeysym::Char('o')));
        p.dashboard_text("half typed");
        p.leave_dashboard();
        assert_eq!(cards(&p, 0)[1], "half typed", "leaving is not a cancel");
        assert_eq!(
            p.take_navigation_request(),
            Some(NavigationRequest::SelectPath(vec![0, 1]))
        );
    }

    #[test]
    fn the_board_declares_it_wants_the_apps_undo() {
        assert!(ProjectManagementProvider::new().dashboard_uses_app_undo());
        assert_eq!(
            ProjectManagementProvider::new().dashboard_kind(),
            DashboardKind::Interactive
        );
    }

    // ---- Registration and locales ---------------------------------------

    #[test]
    fn the_display_name_matches_the_factory_key_once_spaces_are_stripped() {
        // The settings panel matches a section to a provider that way; a mismatch
        // silently drops the section rather than failing anywhere visible.
        register_translations();
        localize::set_locale("en-US");
        let p = ProjectManagementProvider::new();
        assert_eq!(p.display_name().replace(' ', ""), p.name());
    }

    #[test]
    fn all_four_bundles_carry_the_same_keys() {
        fn keys(src: &str) -> Vec<String> {
            src.lines()
                .filter_map(|l| l.split_once('='))
                .map(|(k, _)| k.trim().to_owned())
                .filter(|k| !k.is_empty() && !k.starts_with('#'))
                .collect()
        }
        let en = keys(include_str!("../locales/en-US.ftl"));
        for (name, src) in [
            ("nl-BE", include_str!("../locales/nl-BE.ftl")),
            ("fr-BE", include_str!("../locales/fr-BE.ftl")),
            ("de-BE", include_str!("../locales/de-BE.ftl")),
        ] {
            assert_eq!(keys(src), en, "{name} has drifted from en-US");
        }
    }
}
