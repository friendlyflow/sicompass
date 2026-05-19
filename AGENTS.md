# Project Instructions

## Build System

This project uses **Cargo** as the build system for the Rust implementation.

## Build Commands

Build the Rust workspace:

```bash
cargo build
```

Run tests:

```bash
cargo test
```

## Code Style

### Rust

Follow standard Rust idioms. Use `#[allow(...)]` sparingly and only when justified.

### Documentation prose (`README.md` and `lib/lib_tutorial`)

In `README.md` and the tutorial content (`lib/lib_tutorial/src/lib.rs`), do not
use em dashes or semicolons. Use commas instead, or split into separate
sentences (parentheses are fine for true parentheticals).

## Testing

- After implementing changes, always run relevant tests before finishing.
- Rust tests: `cargo test` (workspace-wide), or `cargo test -p <crate>` (specific crate).
- Integration tests: `src/sicompass/tests/integration.rs`
- Bun tests (TypeScript providers): `bun test tests/lib_*/*.test.ts` (all), or `bun test tests/<module>/<name>.test.ts` (specific).
- When adding new code, write or update tests.
- If tests fail, fix the code — never leave a task with failing tests.

## Test Integrity

- Never remove or weaken test assertions to make a failing test pass. Fix the code instead.
- If a test itself is genuinely wrong and needs changing, **ask the user first** before modifying it.

## Architecture: Unified undo/redo (TimelineEntry model)

Undo/redo flows through `sicompass_sdk::timeline::TimelineEntry`, a tagged enum
that subsumes every reversible action in the app:

- `Navigate { from_id, to_id, from_path, to_path, kind }` — arrow-key cursor
  motion. Each press is recorded as its own entry; ctrl-Z walks the cursor
  back one move at a time.
- `TextChunk { id, before, after, chunk_seq }` — typed text. Repeated text
  edits on the same `id` within `TEXT_CHUNK_IDLE_MS` (default 500 ms) merge
  into the tail entry; typing a long word doesn't fill the timeline.
- `Structural { id, op, payload }` — FFON-tree mutations: Append, Insert,
  Delete, Cut, Paste.
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
  simple in-process toggles (settings radio/checkbox, etc.).

The `Timeline` lives **per tab** (`AppRenderer::tab_timelines`); ctrl-Z and
ctrl-Shift-Z operate on the active tab's timeline only. Provider undo logic
lives behind the `Provider` trait methods `take_timeline_entries(&mut self)
-> Vec<TimelineEntry>`, `undo(&mut self, &TimelineEntry, &mut String)`, and
`redo(&mut self, &TimelineEntry, &mut String)`.

**Irreversibility caveats** (document these in new features):
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

Migration state: legacy `UndoEntry` + `ProviderUndoDescriptor` stacks coexist
with the unified `Timeline` behind `AppRenderer::use_unified_timeline` (default
`false`). Both are dual-written so the unified path can be validated before
the gate flips. After flipping (step 11 in the migration plan), the legacy
types and `Task::{FsCreate,FsRename,FsPaste,FsNavigate,ProviderCommand}`
variants are retired (step 12).

## Architecture: SDK boundary (hard rule)

The `sicompass` app crate (`src/sicompass/src/**`) must not import any `lib_*`
crate directly. All communication flows through `sicompass-sdk` (the `Provider`
trait, the factory registry, setting-injection hooks) plus the thin registration
crate `sicompass-builtins`. No exceptions — this includes `sicompass-settings`,
which is reached via `sdk::create_provider_by_name("settings")` and configured
through the `Provider` trait, and `sicompass-remote`, which is reached via
`sicompass_builtins::create_remote(name, url, key)`.

Tests (`src/sicompass/tests/**` and `#[cfg(test)]` blocks) may import concrete
lib crates for mock injection — these deps live in `[dev-dependencies]`.

A Stop hook enforces this rule automatically at the end of each Claude turn.

## GitHub Workflows

- `release.yml` (Windows + macOS releases) is **generated by cargo-dist** — do
  not hand-edit it. Change `dist-workspace.toml` (or the relevant `Cargo.toml`
  metadata) and regenerate with `dist generate`.
- `release-linux.yml` builds the Linux `.deb`. It is currently parked: it still
  uses the removed meson build and must be ported to `cargo build`.
- `licenses.yml` verifies third-party licensing (see the `cargo about` config
  in `about.toml`).
