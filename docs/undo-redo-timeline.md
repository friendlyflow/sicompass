# Unified undo/redo (TimelineEntry model)

Undo/redo flows through `sicompass_sdk::timeline::TimelineEntry`, a tagged enum
that subsumes every reversible action in the app.

The enum is defined in the **published `sicompass-sdk` crate**, not in this
repository, so grepping the workspace for `pub enum TimelineEntry` finds
nothing. Read the definition from the vendored registry source for the version
pinned in the root `Cargo.toml`, or from the SDK repo.

## Entry variants

Every variant except `TextChunk` and `Structural` also carries `provider_idx`,
identifying which provider owns the action.

- `Navigate { provider_idx, from_id, to_id, from_path, to_path, kind }` —
  arrow-key cursor motion. **Consecutive presses coalesce**: a run of arrow
  keys within the same provider that does not change the path collapses into a
  single entry, so one ctrl-Z reverts the whole burst rather than one press.
  A move that *does* change the path (Right-pressing into a subdirectory) is
  always its own undo step, because merging it would lose the pre-descent
  `from_path` and undo could not restore it. See `record_entry` in
  `src/sicompass/src/state.rs`.
- `TextChunk { id, before, after, chunk_seq }` — typed text. Repeated text
  edits on the same `id` within `TEXT_CHUNK_IDLE_MS` (default 500 ms) merge
  into the tail entry; typing a long word doesn't fill the timeline.
- `Structural { id, op, payload }` — FFON-tree mutations. `StructuralOp` is
  Append, Insert, Delete, Cut, Paste, and `Replace` (whole-element replacement
  driven by a non-paste UI action, e.g. a radio toggle rewriting a group's
  children slice).
- `FsOp { provider_idx, id, op, before, after, side_effect }` — filesystem
  ops (Create, Rename, Delete, Move, Paste). `FsSideEffect::TrashedFile` /
  `TrashedDir` carry a content snapshot (capped at `TRASH_SNAPSHOT_LIMIT_BYTES =
  4 MiB`) so undo restores even when the OS trash is empty; oversized
  directories fall back to a `RenameOnly` marker and report an error if the
  trash entry is gone.
- `ImapOp { provider_idx, id, op }` — email IMAP ops. Trash/Archive/Move use
  the RFC 5322 Message-ID for lookup (UIDs change after a move);
  SetSeen/SetFlagged use the folder-local UID.
- `ChatOp { provider_idx, id, op }` — Matrix ops (LeaveRoom, AcceptInvite,
  RejectInvite, KickMember, BanMember, PostMessage).
- `ProviderOp { provider_idx, command, payload, label }` — catch-all for
  simple in-process toggles (settings radio/checkbox, etc.). Ops with
  non-trivial side effects use a typed variant instead.

## Ownership and control flow

The `Timeline` lives **per tab** (`AppRenderer::tab_timelines`, kept parallel to
`tabs`); ctrl-Z and ctrl-Shift-Z operate on the active tab's timeline only.
`Timeline::position` is 0 at HEAD, incremented walking back and decremented
walking forward. Recording a new action while `position > 0` truncates the redo
branch.

Provider undo logic lives behind the `Provider` trait methods
`take_timeline_entries(&mut self) -> Vec<TimelineEntry>`,
`undo(&mut self, &TimelineEntry, &mut String)`, and
`redo(&mut self, &TimelineEntry, &mut String)`.

During `walk_back` / `walk_forward` the `in_history_action` flag suppresses
recording, and any entries providers emit as a side effect of the undo are
drained and discarded, so the original entry stays the next redo target.

## Irreversibility caveats

Document these in new features.

- Terminal `commit_edit` (Enter on a typed command line) is irrevocable — the
  shell has already executed the line. Only the unsubmitted input slot is
  undoable.
- Directory deletes larger than 4 MiB rely on the OS trash; if the user
  empties the trash, undo reports an error rather than silently failing.
- IMAP undo can fail when the server-side state diverges (message no longer
  in source folder) — the error path returns "message no longer in {folder}"
  rather than corrupting state.
- Matrix `PostMessage` undo is **redact**, not retraction: recipients see
  "message deleted" rather than the message vanishing.
- Git client: `stage`, `unstage`, `stash`, `create branch` and `commit` are on
  the timeline. `commit` undoes as `git reset --soft` back to the recorded
  parent, so the work returns to the index exactly as it was; undoing the
  *first* commit of a repository is `git update-ref -d HEAD`, because there is
  no parent to reset to.
- Git client, not on the timeline at all, and deliberately: `push`, `pull` and
  `fetch` (undoing a push means force-pushing over someone else's work),
  `discard changes` (the content is gone, which is why it asks first),
  `commit --amend` (the commit it replaced is unreachable by name afterwards),
  and `revert`, `cherry-pick`, `merge` and `rebase` (each produces its own
  commits, and git's own `revert` or `reflog` is the honest way back).

## The structural-edit capability

A provider that returns `true` from `supports_structural_edit()` opts into the
generic editing keymap (Ctrl+I / Ctrl+A insert, Ctrl+D / Delete remove,
Ctrl+X / Ctrl+C / Ctrl+V cut, copy and paste). Undo comes free, and the provider
implements no `undo`/`redo` at all: the app mutates its own FFON tree, records
the `Structural` entry, and reports the result back through
`sync_ffon_body_children`, which is the provider's single write path — for the
keypress, and for the ctrl-Z that reverses it.

Two things about that contract are easy to get wrong:

- **The provider works out which list it was handed, not the app.** A provider's
  path is not in step with FFON depth (the mail client sits at `/compose/Body:`
  while its compose fields hang off the provider root), so the app cannot derive
  it. The notes provider tags each row with an `<id>` and looks up the parent.
- **A provider whose `sync_ffon_body_children` means one specific thing must not
  declare the capability.** The mail client's implementation means "this is the
  draft body"; handed a list from two levels deeper it would overwrite the draft
  with a fragment of itself. It keeps its own path check in `shortcuts.rs`.

`delete_item` is asked before a row is removed, and `false` cancels it — the
only veto on the FFON delete path. A provider that declares the capability must
implement it, because the trait default returns `false`, which reads as "always
refuse".

## Notes

Every list opens with a `list meta:` row carrying that list's SHA-256, the root
included — so the first row of the notes provider is the tree's root hash, and a
glance at it says whether anything anywhere below has changed. The hashes are
recomputed from the tree on demand, never cached, so any text change, insert,
delete or reorder moves the affected node's hash and every ancestor's up to the
root. Visibility is excluded, deliberately: publishing a note changes its
audience, not its content.

Propagating the hash through the *tree* is not enough on its own, and the gap is
easy to miss because every unit test passes without it: the app keeps parent
levels in its own FFON tree and `Left` walks back through them without
re-fetching, so an ancestor's `sha256:` row goes on showing a digest that stopped
being true several edits ago. `sync_provider_children` therefore calls
`refresh_visible_path` for a capability provider — every rendered level of a tree
that just changed is stale, not only the one that changed.

Every edit is reversible: writing a note, deleting one, cutting, pasting,
renaming, and flipping a note between private and public. All of it rides the
`Structural` entries the app records, so there is one undo model rather than a
provider-specific one.

Not reversible, and worth stating:

- A note deleted **outside** the app. Undo restores the app's tree and writes it
  back, so the file returns only if the tree still held it.
- A `.listmeta` rewritten by another process, or by a future sync. The provider
  reconciles the directory against its tree on every save; it does not merge.
- Reading the store can fail (a note that is not valid UTF-8, a permission
  error). That is **not** treated as an empty tree: nothing is written at all
  until the store can be read, because reconciling against a tree that failed to
  load would delete the notes that failed to read.

## Project management (the kanban board)

The one provider whose edits arrive through **two** paths, which is why it is the
only one that declares `supports_structural_edit()` *and* implements
`undo`/`redo`.

- **The list surface** is an ordinary capability provider. Ctrl+I / Ctrl+A /
  Ctrl+D / Delete / Ctrl+X / Ctrl+C / Ctrl+V are the app's, the app records the
  `Structural` entry, and the result comes back through
  `sync_ffon_body_children`. No `undo` of the provider's own is involved.
- **The board surface** (the interactive dashboard) cannot use any of that. The
  dashboard forwards every keystroke to the provider without interpreting it, so
  the app never sees the edit, and the `Structural` undo arms work by mutating
  the app's FFON tree, which a board edit never touches. Board edits therefore
  emit `TimelineEntry::ProviderOp` from `take_timeline_entries()` and are
  reversed by the provider's own `undo`/`redo`. Four ops, all about cards:
  `add-card`, `delete-card`, `rename-card`, `move-card`.

Only **cards** are focusable on the board, which is why there is no column op.
Columns are created, renamed, reordered and deleted in the list, where the app's
own capability records them, so no board gesture can reach one. An empty column
still draws a single focusable slot: it is the only place to stand while adding
its first card.

Both kinds land on the same per-tab timeline in the order they happened, so
Ctrl+Z walks back through a board edit and then a list edit without the user
having to know which surface made which.

Three app-side pieces exist for that second path, each guarding a specific way it
breaks:

- `events::drain_dashboard_timeline_entries` runs every frame while a dashboard
  is open. Nothing else drains a provider's entries there, because no handler is
  on the path, so without it Ctrl+Z would have nothing to undo.
- `state::spawn_provider_op` applies the op **synchronously** when the provider
  is the active one, a dashboard is open, and it declares
  `dashboard_uses_app_undo()`. The async path checks the provider out and leaves
  a `PlaceholderProvider` in its slot for a frame or more, and the placeholder
  does not override `dashboard_kind()` — reporting `None` for even one frame
  flips key routing back to the SHORTCUTS table and drops the render into the
  image branch with no image, ejecting the user from the board mid-edit.
- `state::settle_before_history` skips the `handle_escape` that `walk_back` and
  `walk_forward` otherwise run first. Escaping a transient mode before a history
  step is right for an insert session or a search; a dashboard is not transient,
  it is the surface the undo was asked for from, and escaping it throws the user
  out of the board on their first Ctrl+Z.

`dashboard_uses_app_undo()` is host-side only and absent from the WIT
descriptor: it decides which keys a provider intercepts, and a sandboxed guest
does not get to make that choice. See `docs/wasm-plugins.md`.

Not reversible, and worth stating:

- A board changed **outside** the app. Undo restores the provider's board and
  writes it back, so a column returns only if the board still held it.
- Reading the store can fail (a card that is not valid UTF-8, a permission
  error). That is **not** treated as an empty board: nothing is written at all
  until the store can be read, because reconciling against a board that failed to
  load would delete the columns that failed to read.

The board draws in the **app's own palette**, handed over by
`Provider::set_dashboard_palette` before every frame (`view.rs`, the interactive
dashboard branch). It invents no colours and no conventions: a column title is
plain `text` on nothing, a resting card is `text` on nothing exactly like an
unselected row, and the focused card is the only filled thing on the board. Per
frame rather than on entry, so a light/dark switch reaches the board on the next
frame without the provider watching for one — which a hardcoded constant could
never do.

A column title is the name as the user typed it, not restyled: not uppercased,
and with any trailing colon stripped when it is stored (`column_title`). That
colon is the list's syntax for "this row is an object" and the app strips it the
same way for a typed `Obj` key (`state::strip_trailing_colon`); it is not part of
the name, and on the board there is no object convention to explain it. It is
stripped on **load** as well as on write, so a title stored before this existed
is cleaned up rather than showing its colon until someone happens to rename it.

Half a line separates a title from its first card, and below that cards sit on
consecutive rows with no separator, again like list rows. Half, because a blank
row is a whole line and reads as too much under a one-line heading — and a grid of
whole cells cannot express half of one, so `DashboardFrame::half_gap_rows` names
the row to open a gap after and the app does it in pixels. The board draws no
status line of its own: the app's header above the grid already names the mode
and the position. A wrapped card's continuation lines carry
a hanging indent (`render::CONT_INDENT`).
With no blank row between cards that indent is the only thing distinguishing "more
of the card above" from "a new card", which is the same job the list's content
column does for its own wrapped rows.

Text starts flush at each column's left edge, so the space before it is exactly
`MARGIN` — the same as the space after the last column and, near enough on a cell
grid, the one blank row above the titles. An extra cell of padding inside each
column made the left inset three cells against one row at the top, which is
visible. Only the hanging indent is reserved out of a column's width.

The caret in a card is a **bar**, not a filled cell: `DashboardFrame::cursor_style`
is `DashboardCursor::Bar`, and the app draws it as the same thin blinking
rectangle it draws in its own insert mode, gated on the same `caret.visible`. A
filled cell is what a *terminal* cursor is, which is why that stays the default
and both the terminal and `WasmProvider` keep it.

The dashboard key and text paths **reset the blink**, like every insert-mode
handler in the app. Without it the bar free-runs, so a keystroke can land in its
dark half and the caret shows up late or not at all — which reads as the whole
board lagging behind the keyboard rather than as a blink out of phase.

The column titles sit on row 0. The grid already starts below the app's header
line and its separator, and the list puts its own first row straight after that,
so a blank row here made the board sit a whole line lower than every other view.

`render::layout` keeps the two outer margins equal (`render::MARGIN`, the same
width as the gutter, so one rule covers the space before the first column,
between any two, and after the last). The division rarely comes out even, and
dropping the remainder used to dump it past the last column: a wide ragged gap on
the right against a flush left edge. The leftover cells go one each to the
leading columns instead.

Entering and leaving the board are mirror images: `d` opens the board on the card
the list cursor is on, and Escape puts the list cursor back on the card the board
was showing. Without both halves the round trip loses the user's place — Escape
would drop them wherever they pressed `d`, and `d` would reset them to the top of
a column, which after a few minutes of arranging cards is nowhere near what they
were looking at.

Each direction needs the app, and for the same reason: **a provider is never told
which row of a level the cursor is on.** `current_path()` names the level the user
descended into, and moving within that level never calls `push_path`. So the app
hands the cursor's indices over with `Provider::set_dashboard_entry` just before
`enter_dashboard`, and takes them back as
`NavigationRequest::SelectPath([column, card])` on the way out. Entering from a
column title gives a one-element path, and the board opens on that column's first
card.

`SelectPath` is new alongside `EnterChildren`, which cannot express it — that one
descends onto the *first* child with no way to say which. The app walks the path
through the same `handle_left` / `handle_right` the arrow keys use, so the
provider's own path is pushed and popped in step with the cursor and each descent
records its `Navigate` entry; a hand-built `IdArray` would leave both behind.
Indices are clamped at each level, so a request built against a tree that has
since shrunk lands on something real. An empty column's placeholder queues
nothing: there is no card to land on.

`DashboardFrame::selection` names the focused card so the app paints it the way it
paints a selected list row: **one** rounded rectangle at radius 5.0, inset
vertically. Both halves of that need the region named rather than inferred per
cell. Corner rounding is per rectangle, so a multi-row card drawn row by row comes
out as a stack of separate blobs; and a cell is a whole row tall, so painting the
highlight slightly shorter than the rows it covers is the only way to get
breathing space around it. `WasmProvider` always reports `None` — a guest's fills
are its own colours, not a licence to borrow the app's selection furniture — and
so does the terminal, whose fills are SGR backgrounds a program asked for.

One more thing the board owes the timeline: `Board::reseat_counter` is
**monotonic**. Undo shrinks the board, and a counter that followed it down would
hand the removed card's id straight back to the next new card, colliding with
whatever a redo is still holding. `locate_card` would then find whichever copy
came first and every later edit would land on the wrong card. The id counter is
therefore not part of what an undo restores.

Known limitation: the board's cut/copy/paste clipboard is the provider's own, not
the app's FFON clipboard, so a card copied on the board cannot be pasted into the
list and vice versa. A provider has no windowing access, and Ctrl+V inside a
dashboard arrives as a plain keystroke rather than as clipboard text. Ctrl+Shift+V
is the exception the app already carves out, and it pastes real system-clipboard
text as one card per line.

## Migration state

Legacy `UndoEntry` + `ProviderUndoDescriptor` stacks coexist with the unified
`Timeline` behind `AppRenderer::use_unified_timeline` (default `false`). Both
are dual-written so the unified path can be validated before the gate flips.
After flipping (step 11 in the migration plan), the legacy types and
`Task::{FsCreate,FsRename,FsPaste,FsNavigate,ProviderCommand}` variants are
retired (step 12).
