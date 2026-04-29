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

The `.github/workflows/build.yml` and `.github/workflows/release.yml` files share common build logic. When making changes to one file, apply the same changes to the other to keep them in sync.
