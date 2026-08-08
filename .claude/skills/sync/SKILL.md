---
name: sync
description: Fast-forward main from origin, then build and run the test suite to verify the synced tree
argument-hint: "[clippy] [no-jit] [graph]"
disable-model-invocation: true
model: sonnet
effort: medium
allowed-tools:
  - "Bash(git fetch:*)"
  - "Bash(git status:*)"
  - "Bash(git log:*)"
  - "Bash(git diff:*)"
  - "Bash(git merge:*)"
  - "Bash(git pull:*)"
  - "Bash(git rev-list:*)"
  - "Bash(git stash:*)"
  - "Bash(cargo build:*)"
  - "Bash(cargo test:*)"
  - "Bash(cargo clippy:*)"
  - "Bash(nix develop:*)"
  - "Bash(graphify update:*)"
  - Read
  - Grep
---

Pull `origin/main` into the local checkout, then prove the result still builds
and passes its tests. Read-and-verify only — this skill does not commit, push,
tag, or edit source. If the pulled code breaks something, report it; fixing it
is a separate, explicitly requested task.

**IMPORTANT: The shell's working directory persists between Bash calls. Prefix
every command with `cd PROJECT_ROOT &&` (the actual absolute project root).**

**Always work directly on `main`.** Never create a branch, never rebase onto
something else, never force-push. This repo's whole workflow is linear on
`main` (see `/commit-and-push`).

## Scope from `$ARGUMENTS`

- (empty, the default) — sync, `cargo build --workspace`, `cargo test --workspace`.
- `clippy` — also run `cargo clippy --workspace --all-targets`, matching the
  lint leg of `ci.yml`.
- `no-jit` — additionally run the tests over the Pulley backend
  (`--no-default-features --features no-jit-wasm`). CI runs both wasmtime
  backends on Linux because they are genuinely different code paths, not a
  flag. Worth asking for when the pull touched `src/sicompass/src/plugin*`,
  `lib/lib_builtins`, or the `wasmtime` dependency.
- `graph` — finish with `graphify update .` so the knowledge graph in
  `graphify-out/` reflects the code that was just pulled in. AST-only, no API
  cost.

## Environment

Check `command -v cargo` **once**, then stick with the answer for the whole
session:

- Non-empty — the shell is already inside `nix develop`; run `cargo ...`
  directly.
- Empty — prefix every toolchain command with `nix develop -c`. The
  `warning: Git tree ... is dirty` line it prints on stderr first is noise, not
  a failure.

Do **not** pass `--features bundled-sdl3` locally. That feature builds SDL3
from vendored source and only exists for release builds; the dev shell provides
a system SDL3 and linking against it is much faster. CI passes it because its
runners have no SDL3.

## Steps

1. **Check the working tree.** `git status --short`.

   - Clean — continue.
   - Dirty — say so, listing the files, and continue anyway. A fast-forward
     is safe here: step 3 uses `--ff-only`, and git itself refuses to
     overwrite modified files. Never stash, reset, or checkout over the
     user's uncommitted work.

2. **Fetch and measure the gap.**

   ```sh
   git fetch origin
   git rev-list --left-right --count origin/main...main
   ```

   The two numbers are *behind* and *ahead*.

   - `0  0` — already in sync. **Skip step 3** and go straight to the build:
     the point of the skill is still to verify the tree, and a green run on an
     unchanged tree is a useful answer, not a wasted one. Say it was already
     up to date.
   - `N  0` — behind only. The normal case; fast-forward in step 3.
   - `0  N` — ahead only. Nothing to pull. Tell the user they have N unpushed
     commits and that `/commit-and-push` will push them, then verify the tree
     anyway.
   - `N  M` — **diverged**. Stop before touching anything. Show both sides
     (`git log --oneline origin/main ^main` and `git log --oneline main ^origin/main`)
     and ask the user how to reconcile. Do not rebase, merge, or reset on your
     own initiative — this is the one situation where guessing loses work.

3. **Fast-forward.**

   ```sh
   git pull --ff-only origin main
   ```

   `--ff-only` is the safety rail: it fails loudly rather than silently
   creating a merge commit, which would break the linear history the release
   pipeline assumes. If it fails, go back to the diverged case in step 2.

4. **Read what arrived.** `git diff --stat HEAD@{1} HEAD` (skip if nothing was
   pulled). Three things change what you do next:

   - `flake.nix` or `flake.lock` moved — the dev shell definition changed. If
     you are running commands *inside* an already-entered `nix develop`, that
     shell is now stale; tell the user to exit and re-enter it. Prefixed
     `nix develop -c` invocations pick the change up on their own.
   - `Cargo.toml` or `Cargo.lock` moved — expect the build to fetch and
     compile new crates, so a long first build is normal, not a hang.
   - `shaders/*`, `assets/icons/*` or `THIRD-PARTY-LICENSES.html` moved —
     these are committed generated files. You do not regenerate them here;
     `cargo test -p sicompass` fails if they drifted, which is exactly the
     signal this skill exists to surface.

5. **Build.** `cargo build --workspace`.

   A failure here is a report, not a repair job. Give the user the first real
   error (the top-most `error[E...]`, not the trailing `could not compile`
   summary) and the file it points at, and stop. Two failures are worth naming
   explicitly because they are environment, not code:
   - a missing system library or `pkg-config` miss — the dev shell was not
     entered, or `flake.lock` moved (step 4);
   - `sicompass-sdk` failing to resolve — check whether the
     `[patch.crates-io]` block near the bottom of `Cargo.toml` is uncommented
     and pointing at a `../sicompass-plugin-sdk` checkout that is missing or
     itself out of date.

6. **Test.** `cargo test --workspace`.

   Report the pass/fail counts per crate. Never weaken or delete an assertion
   to make a test pass, and never edit a test at all without asking the user
   first. If a test fails, name the test, quote its assertion output, and say
   whether it came in with the pull (`git log --oneline -5 -- <path>`) or was
   already failing locally — that distinction is the first thing the user
   needs.

   Some of this suite reaches the network and wants a display, which is why CI
   only runs it in full on Linux. On a headless machine, run it under
   `xvfb-run` (it is in the dev shell) rather than reporting display failures
   as test failures.

7. **Clippy** — only when `$ARGUMENTS` contains `clippy`:

   ```sh
   cargo clippy --workspace --all-targets
   ```

8. **Pulley leg** — only when `$ARGUMENTS` contains `no-jit`:

   ```sh
   cargo test --workspace --no-default-features --features no-jit-wasm
   ```

9. **Refresh the graph** — only when `$ARGUMENTS` contains `graph`:
   `graphify update .`.

10. **Report.** Four lines, no more:
    - what moved (`old..new`, commit count, one-line summary of the range);
    - build result;
    - test result, with counts;
    - anything the user has to act on — a stale `nix develop`, unpushed
      commits, a failing test, generated-file drift.

## Relationship to the other skills

- `/sync` pulls and verifies. It never writes to the repo or the remote.
- `/commit-and-push` sends local work the other way. Run `/sync` first when
  you are behind, so you push onto current `main` instead of discovering the
  divergence at push time.
- `/release` needs `0  0` from step 2's count and a clean tree before it will
  tag, so `/sync` is a reasonable thing to run just before it.
- `/update-cargo` also builds and tests, but it *changes* the lockfile first.
  Do not run both back to back expecting different information.
