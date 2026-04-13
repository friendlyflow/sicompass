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
- Integration tests: `src/sicompass-rs/tests/integration.rs`
- Bun tests (TypeScript providers): `bun test tests/lib_*/*.test.ts` (all), or `bun test tests/<module>/<name>.test.ts` (specific).
- When adding new code, write or update tests.
- If tests fail, fix the code — never leave a task with failing tests.

## GitHub Workflows

The `.github/workflows/build.yml` and `.github/workflows/release.yml` files share common build logic. When making changes to one file, apply the same changes to the other to keep them in sync.
