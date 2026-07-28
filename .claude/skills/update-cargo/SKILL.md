---
name: update-cargo
description: Refresh Rust dependencies (Cargo.lock, workspace version requirements, flake.lock) and verify with a build plus the test suite
argument-hint: "[major] [push]"
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
  - Read
  - Edit
  - Grep
---

Update this workspace's dependencies. Mechanical chore — no refactoring, no
unrelated cleanups.

**IMPORTANT: The shell's working directory persists between Bash calls. Prefix
every command with `cd PROJECT_ROOT &&` (the actual absolute project root).**

## Scope from `$ARGUMENTS`

- (empty, the default) — semver-compatible updates only: `cargo update`, no
  `Cargo.toml` edits.
- `major` — also raise version requirements in `Cargo.toml` for crates whose
  new release is outside the current requirement.
- `push` — push the commit to `origin/main` at the end. Without it, stop after
  committing and tell the user to run `/commit-and-push` or re-run with `push`.

## Environment

Check `command -v cargo` once. Non-empty: run `cargo ...` directly. Empty:
prefix every toolchain command with `nix develop -c` (the `warning: Git tree
... is dirty` line on stderr is noise). Stick with the answer for the session.

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
      - `imap-proto` — held at 0.10 to match the `imap` 2.x re-exports.

   c. **`sicompass-sdk` is in scope**, with one coordination rule. It is the
      only `sicompass-*` crate resolved from crates.io — the rest are path
      members that `cargo update` never touches — so step 2 already picks up
      any compatible `0.1.x` SDK release on every run, `major` or not. Since
      `cargo update --dry-run` only reports crates held back by a requirement,
      confirm the newest published version with `cargo search sicompass-sdk`
      and compare it against the `sicompass-sdk = "..."` pin in
      `[workspace.dependencies]`.

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

9. **Push** only if `$ARGUMENTS` contains `push`: `git push origin HEAD:main`.
   Never force-push, never move the work onto a branch.

10. **Report** which crates moved, which were held back, and the test result.
    Always state the `sicompass-sdk` version explicitly and whether it moved,
    even when it did not — it is the one dependency whose staleness is easy to
    miss, and `/release` reads that pin.
