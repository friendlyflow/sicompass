---
name: update-cargo
description: Refresh this repo's Cargo.lock and flake.lock, verify with a build and the tests, and commit
argument-hint: "[major] [push]"
disable-model-invocation: true
model: sonnet
effort: medium
---

Update this repo's dependencies. It is a mechanical chore, so no refactoring and
no unrelated cleanups. The result is one commit.

This file is also what `/update-cargo <this repo>` follows when it is run from
a sicompass checkout. **Prefix every command with `cd PROJECT_ROOT &&`.**

1. `git status --short` must be empty. A dependency commit contains nothing
   else.
2. `cargo update`.
3. With `major` in `$ARGUMENTS`: list held-back crates with
   `cargo update --dry-run --verbose 2>&1 | grep -i available`, and raise their
   requirements in `Cargo.toml`. Skip any requirement that has a comment
   explaining a pin, and ask before touching it. Then `cargo update` again.
   First-party git dependencies (`sicompass-ui`, `sicompass-payments`) are
   pinned by `rev`/`tag` and are moved by a release, not here.
4. `nix flake update`.
5. `cargo build`, then `cargo test`. If a bumped crate needs a large migration,
   revert that one requirement, note it as held back, and continue. Never weaken
   a test to get it to pass.
6. `git diff --stat` should show only `Cargo.lock`, `flake.lock`, and with
   `major` also `Cargo.toml`.
7. Commit on `main`: `chore: update Cargo.lock and flake.lock dependencies`, or
   `Update cargo dependencies (crate X, crate Y)` with a body naming anything
   held back. No co-author trailer.
8. With `push` in `$ARGUMENTS`: `git push origin HEAD:main`.
9. Report which crates moved, which were held back, and the test result.
