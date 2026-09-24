---
name: update-cargo
description: Refresh Rust dependencies (Cargo.lock, workspace version requirements, flake.lock) and verify with a build plus the test suite, then smoke-test the sibling SDK repo against freshly resolved deps
argument-hint: "[repo|all] [major] [push] [no-sdk]"
disable-model-invocation: true
model: sonnet
effort: medium
allowed-tools:
  - "Bash(cargo update:*)"
  - "Bash(cargo build:*)"
  - "Bash(cargo test:*)"
  - "Bash(cargo tree:*)"
  - "Bash(cargo search:*)"
  - "Bash(nix flake update:*)"
  - "Bash(git status:*)"
  - "Bash(git diff:*)"
  - "Bash(git add:*)"
  - "Bash(git commit:*)"
  - "Bash(git -C:*)"
  - Read
  - Edit
  - Grep
---

**Target repo.** Before anything else, resolve which repo this runs in, as
described in [.claude/repo-selection.md](../../repo-selection.md). The first
argument may name a sibling repo from `.claude/repos.json`. If it does, and that
repo has its own copy of this skill, follow that copy instead of the steps
below. With no repo argument, the target is sicompass and everything below
applies unchanged.

Update this workspace's dependencies. Mechanical chore — no refactoring, no
unrelated cleanups. One commit, in this repo.

**IMPORTANT: The shell's working directory persists between Bash calls. Prefix
every command with `cd PROJECT_ROOT &&` (the actual absolute project root).**

## The SDK repo is *not* a second lockfile to bump

A natural assumption is that `/update-cargo` should also refresh
`../sicompass-plugin-sdk`. It should not, and the reason is worth stating once
so the question stops coming back:

**That repo gitignores `Cargo.lock`** (`.gitignore` line 2), the normal
convention for a published library crate. Nothing there is committable, so
there is no SDK lockfile that can drift and no SDK dependency commit to make.
Its CI resolves the newest semver-compatible versions on every fresh checkout,
which is the same thing `cargo update` would have done.

So this skill commits in **one** repo: this workspace. What the SDK half is
good for is a *canary* (step 9) — precisely because it has no lockfile to hide
behind, a bad upstream release breaks it immediately, and it is cheap to find
that out here rather than in its CI.

The one SDK artifact this workspace really does own is the
`sicompass-sdk = "..."` pin in `[workspace.dependencies]`, which step 3c
handles and step 11 always reports.

## Scope from `$ARGUMENTS`

- (empty, the default) — semver-compatible updates only: `cargo update`, no
  `Cargo.toml` edits.
- `major` — also raise version requirements in `Cargo.toml` for crates whose
  new release is outside the current requirement. This workspace only; the SDK
  repo is never edited (step 9f).
- `push` — push the commit to `origin/main` at the end. Without it, stop after
  committing and tell the user to run `/commit-and-push` or re-run with `push`.
- `no-sdk` — skip the step 9 canary and say so in the report.

## Environment

Check `command -v cargo` once. Non-empty: run `cargo ...` directly. Empty:
prefix every toolchain command with `nix develop -c` (the `warning: Git tree
... is dirty` line on stderr is noise). Stick with the answer for the session.

The SDK repo has its **own** flake, whose Rust comes from rust-overlay with the
`wasm32-wasip2` target that this workspace's nixpkgs rustc lacks. Run the step 9
canary inside it: `cd ../sicompass-plugin-sdk && nix develop -c ...`. For a
fuller refresh of that repo (its `flake.lock` too), use
`/update-cargo sicompass-plugin-sdk`, which follows that repo's own skill.

## Steps

1. **Verify a clean tree.** `git status --short` must be empty. If not, stop
   and tell the user — a dependency bump commit must contain nothing else.

2. **Compatible updates.** `cargo update`. This touches only `Cargo.lock`.
   This covers `sicompass-sdk` too — see step 3c.

3. **Major/minor bumps** — only when `$ARGUMENTS` contains `major`:

   a. `cargo update --dry-run --verbose 2>&1 | grep -i available` lists every
      crate held back by its requirement, as
      `Unchanged <pkg> v<current> (available: v<new>)`.

   b. Skip the deliberate pins (each has a comment in `Cargo.toml` explaining
      why — never remove those comments, and never bump these without asking
      the user first):
      - `freetype` — held at 0.7 for the bundled-FreeType Windows release build.

      There is no longer an `imap-proto` pin: `lib_emailclient` moved from the
      `imap` 2.x crate to `async-imap`, which re-exports its own `imap-proto`,
      so the version follows `async-imap` and is not declared directly.

   c. **`sicompass-sdk` is in scope**, with one coordination rule. It is the
      only `sicompass-*` crate resolved from crates.io — the rest are path
      members that `cargo update` never touches — so step 2 already picks up
      any compatible SDK release on every run, `major` or not. Since
      `cargo update --dry-run` only reports crates held back by a requirement,
      confirm the newest published version with `cargo search sicompass-sdk`
      and compare it against the `sicompass-sdk = "..."` pin in
      `[workspace.dependencies]`.

      Note this is the *published* version. It can lag the version in
      `../sicompass-plugin-sdk/Cargo.toml`, which is what the working copy will
      publish next — a local `0.9.0` that is not on crates.io yet is not
      something to pin to. Compare against `cargo search`, not the sibling
      checkout.

      When a release sits outside the pin, raise it. The SDK ships ahead of the
      app, so leaving the pin above `[workspace.package] version` is expected
      and correct — do **not** touch the workspace version to match, that is
      `/release`'s job. If the new SDK needs a real API migration, apply the
      step 5 rule (revert the pin, note it as held back) rather than
      refactoring providers here.

      Caveat: if the `[patch.crates-io]` block near the bottom of `Cargo.toml`
      has been uncommented for local SDK development, the SDK resolves from
      `../sicompass-plugin-sdk` and the pin is inert. Leave the patch as you
      found it and say so in the report.

   d. Raise the requirement in `[workspace.dependencies]` in the root
      `Cargo.toml`. A few crates pin their own versions in
      `lib/*/Cargo.toml` or `src/sicompass/Cargo.toml` — grep for the crate
      name and update every occurrence.

   e. `cargo update` again to resolve the new requirements.

4. **Flake input.** `nix flake update` to refresh `flake.lock` (nixpkgs). Skip
   this if the user asked for cargo only.

5. **Build.** `cargo build --workspace`. Fix any breakage caused by a bumped
   crate — adapt the call sites to the new API. If a major bump needs a real
   migration (large API rewrite), revert that one crate to its previous
   requirement, note it in the commit message as held back, and continue.

6. **Test.** `cargo test --workspace`. Never weaken an assertion to make a test
   pass; fix the code, and ask the user before changing a test itself.

7. **Review the diff.** `git diff --stat` — expect `Cargo.lock`, `flake.lock`,
   and (with `major`) `Cargo.toml`. Anything else is a mistake.

8. **Commit** on `main`, no co-author trailer. Message style follows the
   existing history:
   - lockfile only: `chore: update Cargo.lock and flake.lock dependencies`
   - with requirement bumps: `Update cargo dependencies (crate X, crate Y)`
     plus a short body naming anything held back and why.

   Commit this repo **before** starting step 9, so a failure over there leaves
   a finished, self-contained commit here rather than a half-done tree.

9. **SDK canary** (`../sicompass-plugin-sdk`) — skip when `$ARGUMENTS`
   contains `no-sdk`. This produces **no commit**: that repo gitignores its
   lockfiles. It is a build/test check against freshly resolved dependencies,
   and its only output is a line in the report.

   a. Skip it, with a report line, if the directory is missing or
      `git -C ../sicompass-plugin-sdk status --short` shows tracked changes.
      Untracked `Cargo.lock` churn there is expected and is not a reason to
      skip.

   b. It has **two lockfiles, neither reachable from the other** — both
      untracked. `sicompass-pdk/` declares its own bare `[workspace]`, on
      purpose: it only builds for `wasm32-unknown-unknown`, and wit-bindgen's
      generated `wasm_import_module` extern blocks do not link on a host
      target, so the SDK root's `cargo` never descends into it. Refresh both,
      to resolve what a clean CI checkout would get:
      - `cargo update` in `../sicompass-plugin-sdk`
      - `cargo update` in `../sicompass-plugin-sdk/sicompass-pdk`

   c. No `nix flake update` here: that repo's `flake.lock` is refreshed by its
      own `/update-cargo sicompass-plugin-sdk`, and this canary makes no commit.

   d. **Check the SDK root in both feature configurations.** `host` is a
      default-on *additive* feature; WASM guests depend on the SDK with
      `default-features = false`. A bumped crate can break the guest
      configuration while the default one still builds, so run both:
      - `cargo test` (the default `host` build, plus its test suite)
      - `cargo build --no-default-features` (the guest surface)

   e. **Check the PDK** with `cargo build --target wasm32-unknown-unknown` in
      `sicompass-pdk/`. Do not run `cargo test` there — there is no host target
      to run it on, which is the whole reason for the split workspace.

   f. Never edit anything in that repo — not `Cargo.toml` requirements even
      under `major`, and above all not its `version`. That belongs to the SDK's
      own release flow, and a bump here would strand this app's pin against a
      crates.io release that does not exist.

   g. If the canary **fails**, do not try to fix the SDK here. Report the
      failing crate and configuration; the workspace commit from step 8 still
      stands on its own.

10. **Push** only if `$ARGUMENTS` contains `push`: `git push origin HEAD:main`.
    Never force-push, never move the work onto a branch. There is nothing to
    push for the SDK — step 9 makes no commit.

11. **Report** which crates moved, which were held back, and the test result.
    Always state:
    - the `sicompass-sdk` version this workspace pins and whether it moved,
      even when it did not — it is the one dependency whose staleness is easy
      to miss, and `/release` reads that pin;
    - the SDK canary's result: passed, failed (with the crate), or skipped
      (with the reason). Say that it made no commit, so a green canary is not
      mistaken for a pending SDK release.
