---
name: sync
description: Fast-forward main from origin, then build and run this repo's tests to verify the synced tree
argument-hint: "[clippy]"
disable-model-invocation: true
model: sonnet
effort: medium
---

Pull `origin/main`, then prove the result builds and passes its tests. This is
read-and-verify only. It never commits, pushes or edits source.

This file is also what `/sync <this repo>` follows when it is run from a
sicompass checkout. **Prefix every command with `cd PROJECT_ROOT &&`.**

1. `git status --short`. If the tree is dirty, list the files and continue. Never
   stash, reset or check out over uncommitted work.
2. `git fetch origin`, then `git rev-list --left-right --count origin/main...main`
   (behind, ahead).
   - `0 0`: already in sync. Skip step 3 but still build and test.
   - `N 0`: fast-forward in step 3.
   - `0 N`: N unpushed commits. Say so and verify anyway.
   - `N M`: diverged. Stop, show both sides, and ask. Never rebase or reset on
     your own initiative.
3. `git pull --ff-only origin main`.
4. `cargo build`, then `cargo test`. On failure, report the top-most real error
   and stop. Fixing it is a separate task.
5. With `clippy` in `$ARGUMENTS`: `cargo clippy --all-targets`.
6. Report in four lines: what moved, build, tests (with counts), and anything
   the user has to act on.
