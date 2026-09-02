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

## Migration state

Legacy `UndoEntry` + `ProviderUndoDescriptor` stacks coexist with the unified
`Timeline` behind `AppRenderer::use_unified_timeline` (default `false`). Both
are dual-written so the unified path can be validated before the gate flips.
After flipping (step 11 in the migration plan), the legacy types and
`Task::{FsCreate,FsRename,FsPaste,FsNavigate,ProviderCommand}` variants are
retired (step 12).
