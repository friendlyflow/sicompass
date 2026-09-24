---
name: release
description: Bump this repo's version and push a vX.Y.Z tag, which is the release
argument-hint: "[major|minor|patch]"
disable-model-invocation: true
model: sonnet
---

Cut a release of this repo.

This file is also what `/release <this repo>` follows when it is run from a
sicompass checkout, so `PROJECT_ROOT` is this repo's root.

**IMPORTANT: Prefix every command with `cd PROJECT_ROOT &&`.**

A release here is a **`vX.Y.Z` tag on `main`**. Other repos pin this one by that
tag (`tag = "vX.Y.Z"` in a git dependency, or a flake input `?ref=vX.Y.Z`).
If `.github/workflows/release.yml` exists, the tag push also triggers it, and
that workflow builds and attaches the release artifacts. Nothing in this repo
is published to crates.io.

**A pushed tag is public and other repos start resolving it.** Never move or
delete a pushed tag. If a release is broken, release the next patch.

## Steps

1. **Clean state.** `git status --short` must be empty, and
   `git rev-list --left-right --count origin/main...main` must print `0  0`.
   If not, stop and say to run `/commit-and-push` first.
2. **No live `[patch]`.** Every `[patch...]` section in `Cargo.toml` must be
   commented out. With one live, a green build proves nothing about what the
   tag resolves.
3. **Checks.**
   ```sh
   cargo test
   cargo clippy --all-targets
   timeout 1200 nix build "git+file://$PWD"
   ```
   Also run any extra check `CLAUDE.md` lists under "Releasing".
4. **Version.** Read `[package] version` in `Cargo.toml` and compare it with
   `git tag --sort=-v:refname | head -1`.
   - That version has no tag yet: it was bumped in an earlier commit, so release
     it as it is and skip step 5.
   - It matches the latest tag: bump it. Patch by default, `minor`/`major` if
     `$ARGUMENTS` says so.
5. **Bump.** Set the version in `Cargo.toml`, run
   `cargo update --workspace --offline`, and add a `CHANGELOG.md` section for
   the new version (create the file if it does not exist). Commit
   `Release: bump version to X.Y.Z` and `git push origin HEAD:main`.
6. **Tag.**
   ```sh
   git tag -a vX.Y.Z -m "Release vX.Y.Z"
   git push origin vX.Y.Z
   ```
   A **plugin** is released the same way. Before tagging, run
   `nix develop -c ./scripts/release-plugin.sh --dry-run`: it builds, audits,
   signs with a throwaway key and verifies, so a release that would fail is
   caught before the tag exists. The tag's version must equal `plugin.json`'s.
7. **Follow the workflow**, if there is one:
   `gh run list --workflow=release.yml --limit 1`. Judge success by the release
   page and its assets, not only the run status.
   For a plugin, the release has to carry `plugin.tar.gz`, `release.json` and
   `release.json.sig`: `gh release view vX.Y.Z --json assets`. The Store picks
   it up from `releases/latest/download/` with no change in sicompass.
8. **Report** the tag, and name the repos that pin this one and now need their
   `rev`/`tag` moved. Those are the entries in `../sicompass/.claude/repos.json`
   whose `Cargo.toml` or `flake.nix` mentions this repo.
