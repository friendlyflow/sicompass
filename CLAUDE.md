# Project Instructions

## Environment (NixOS)

The whole toolchain (`cargo`, `rustc`, `clippy`, `rustfmt`, `bun`, `graphify`,
`lld` + `wasm-tools` (for WASM plugin guests),
`xvfb-run`, SDL3/Vulkan link paths) comes from the flake dev shell in
[flake.nix](flake.nix). Nothing is installed system-wide.

- **Check once per session**, then stick with the answer: `command -v cargo`.
  - Non-empty: the shell was launched from inside `nix develop`, so run
    `cargo test ...` directly.
  - Empty: prefix every toolchain command with `nix develop -c`, e.g.
    `nix develop -c cargo test -p sicompass-tutorial`.
- `nix develop -c <cmd>` always prints a `warning: Git tree ... is dirty` line on
  stderr first. That warning is noise, not a failure.
- Do not reintroduce a bare `exec fish` in the flake's `shellHook`. It is guarded
  by `[ -t 0 ]` on purpose: without the guard it replaces the process for
  `nix develop -c <cmd>`, and the command silently never runs (exit 0, no output).
- Crate package names differ from directory names: `lib/lib_<x>` is package
  `sicompass-<x>` (exception: `lib/lib_texteditor` is `sicompass-text-editor`).
  Crates under `src/` keep their directory name. `cargo test -p` takes the
  package name.

## Code Style

### Rust

Follow standard Rust idioms. Use `#[allow(...)]` sparingly and only when justified.

### Documentation prose (`README.md` and `lib/lib_tutorial`)

In `README.md` and the tutorial content (`lib/lib_tutorial/src/lib.rs`), do not
use em dashes or semicolons. Use commas instead, or split into separate
sentences (parentheses are fine for true parentheticals).

### Tutorial authoring

When writing or restructuring the in-app tutorial (`lib/lib_tutorial/`), follow
the rules in [docs/tutorial-guidelines.md](docs/tutorial-guidelines.md): teach by
doing, one idea per step, confirm via the screen-reader announcement, keep a short
guided path separate from the reference manual, make keyboard shortcuts lead each
line, and add every new string to all four locale bundles.

## Testing

- After implementing changes, always run relevant tests before finishing.
- Rust tests: `cargo test` (workspace-wide), or `cargo test -p <crate>` (specific crate).
- Integration tests: `src/sicompass/tests/integration.rs`
- When adding new code, write or update tests.
- If tests fail, fix the code — never leave a task with failing tests.

## Test Integrity

- Never remove or weaken test assertions to make a failing test pass. Fix the code instead.
- If a test itself is genuinely wrong and needs changing, **ask the user first** before modifying it.

## Architecture: Unified undo/redo (TimelineEntry model)

All reversible actions flow through `sicompass_sdk::timeline::TimelineEntry`.
When working on undo/redo, or adding any reversible provider action, follow
[docs/undo-redo-timeline.md](docs/undo-redo-timeline.md): the entry variants and
their coalescing rules, per-tab `Timeline` ownership, the `Provider` trait
hooks, the irreversibility caveats, and the legacy-stack migration state.

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

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
