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

## Migration state

Legacy `UndoEntry` + `ProviderUndoDescriptor` stacks coexist with the unified
`Timeline` behind `AppRenderer::use_unified_timeline` (default `false`). Both
are dual-written so the unified path can be validated before the gate flips.
After flipping (step 11 in the migration plan), the legacy types and
`Task::{FsCreate,FsRename,FsPaste,FsNavigate,ProviderCommand}` variants are
retired (step 12).
