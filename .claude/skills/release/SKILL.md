---
name: release
description: Bump the sicompass workspace version and cut a GitHub release via the tag-push pipeline
disable-model-invocation: true
model: sonnet
---

Cut a new sicompass release.

Pushing a `vX.Y.Z` tag is the only trigger: `release-on-tag.yml` bridges the tag
push to the cargo-dist `release.yml`, which builds all four targets and calls
`native-packages.yml` for the `.deb`, `.rpm`, AppImage, `.app` and `.dmg`.
There is no manual GitHub-UI step.

**This publishes to the world.** The tag push is the point of no return, so
everything before it is checking, and step 1 is not optional.

Full background is in [docs/releasing.md](../../../docs/releasing.md). Read it
before running this the first time.

**IMPORTANT: The shell's working directory persists between Bash calls. Prefix
every command with `cd PROJECT_ROOT &&` (the actual absolute project root).**

## Steps

1. **Run the checks by hand.** Nothing in this repository runs on commit, so
   nothing has been verified unless someone asked for it.

   ```sh
   gh workflow run ci.yml && gh workflow run licenses.yml
   gh run list --workflow=ci.yml --limit 1
   ```

   Wait for CI to go green before continuing. It covers Linux, macOS and
   Windows, and includes `dist generate --check`. If you skip this, the first
   time anyone finds out that a platform does not build is during the release.

2. **Verify clean state.** `git status --short` must be empty, and
   `git rev-list --left-right --count origin/main...main` must print `0  0`.
   If not, stop and tell the user — use `/commit-and-push` first.

3. **Determine the version.** Read `version` under `[workspace.package]` in
   `Cargo.toml` (line ~22) and compare it with `git tag --sort=-v:refname | head -1`.

   - **If that version has no tag yet**, it was already bumped in a previous
     commit. That is the version being released. **Skip steps 4 to 6**, but
     still confirm step 6's `grep` returns 0: the README is bumped by hand and
     is the piece most often left behind.
   - **If it matches the latest tag**, bump it. Default to a patch bump
     (`0.1.3` -> `0.1.4`), matching release history, unless the user asks for
     minor or major.

   Either way, check the `sicompass-sdk = "..."` pin in
   `[workspace.dependencies]`. The SDK is released ahead of the app, so the pin
   usually already points at the target version. If it lags, bump it to match
   in the same edit.

   If the version it needs is not on crates.io yet, stop: run `/release-sdk`
   first. That skill publishes the crate from the sibling
   `../sicompass-plugin-sdk` checkout, which is a step this one does not do and
   cannot do for you.

4. **Bump `Cargo.toml`.** Set `[workspace.package] version`, and the
   `sicompass-sdk` pin if it lagged. `flake.nix` reads the version from there,
   so there is nothing else to bump.

5. **Add the `CHANGELOG.md` section** for the new version. cargo-dist parses
   this file and uses the matching section as the GitHub Release body, so it is
   what users read on the download page. Without an entry the notes are generic
   boilerplate.

6. **Bump the versioned filenames in `README.md`.** Do this as soon as the
   version is known, in the same commit as the `Cargo.toml` bump, rather than
   after the release lands.

   The `.deb`, `.rpm` and AppImage are the only assets whose *file name*
   carries the version, and the README names all three literally: once in the
   download table, and again in the `apt` / `dnf` / `chmod +x` snippets below
   it. Six or seven occurrences in total. Their
   `releases/latest/download/...` URLs 404 the moment the new release exists,
   so a stale README silently breaks every Linux download link, which is the
   one platform where the links are load-bearing.

   ```sh
   sed -i 's/OLD/NEW/g' README.md   # e.g. 0.1.10 -> 0.1.11
   grep -c 'OLD' README.md          # must print 0
   ```

   The `.msi`, both `.dmg`s, the `.zip` and the `.tar.xz`s are not versioned in
   their file names, so those links never need touching. After the release is
   published, `curl -sIL -o /dev/null -w '%{http_code}'` on the three bumped
   URLs should return 200.

7. **Sync `Cargo.lock`.** `cargo update --workspace --offline` rewrites the
   version of the ~15 workspace member crates without touching external deps.
   Confirm only `Cargo.toml` and `Cargo.lock` changed.

8. **Commit and push `main`** (skip if steps 4 to 7 were all skipped).

   ```sh
   git add Cargo.toml Cargo.lock CHANGELOG.md README.md
   git commit -m "Release: bump version to X.Y.Z"
   git push origin HEAD:main
   ```

   Plain message, no co-author trailer.

9. **Tag and push the tag.**

   ```sh
   git tag -a vX.Y.Z -m "Release vX.Y.Z"
   git push origin vX.Y.Z
   ```

10. **Report.** Give the user the Actions URL:
    https://github.com/friendlyflow/sicompass/actions

11. **Smoke test after the artifacts appear.** The pipeline has published
    unrunnable binaries before. At minimum, download one package and run
    `sicompass --check`: it reports where resources resolved to and which
    Vulkan devices it sees, which is the whole class of failure that used to
    only surface on a user's machine. The per-platform list is in
    `docs/releasing.md`.

## Notes

- **cargo-dist's own `host` job publishes the release.** An earlier version of
  this file said it always 403s and that `release-publish.yml` picks up the
  slack. That was wrong, and v0.1.9 proved it: `host` succeeded. The repo's
  `default_workflow_permissions` is `read`, but that is only the default and
  the generated `release.yml` asks for `contents: write`.
- `release-publish.yml` has never run and will not: its `workflow_run` trigger
  does not fire for a `Release` dispatched on a tag ref. Treat it as a dormant
  fallback, not part of the path.
- Judge the outcome by the **Releases page and the asset count**, not by the
  run status.
- `release.yml` is generated by cargo-dist — never hand-edit it. Change
  `dist-workspace.toml` and run `dist generate`. The exception is
  `src/sicompass/wix/main.wxs`, which is hand-edited and protected by
  `allow-dirty = ["msi"]`.
- A release should carry roughly 25 assets across the four targets. If it has
  only the Windows ones, `custom-native-packages` failed inside the Release run
  and its logs are in the same run.
