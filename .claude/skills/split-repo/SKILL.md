---
name: split-repo
description: Lift a crate out of the sicompass workspace into its own sibling repo in ../, keeping its git history, and give it the standard LICENSE, README, CLAUDE.md, .claude, flake and CI
argument-hint: "<path-in-sicompass> <new-repo-name> [--path <extra>]... [--kind <kind>]"
disable-model-invocation: true
---

Split `<path-in-sicompass>` (e.g. `src/desicompass`, `lib/lib_sales_demo`) into
`../<new-repo-name>`, a repo of its own with that directory's history.

This is the mechanical half of a split. The other half, which is making the
crate build on its own and removing it from sicompass, is different every time
and is described in the plan step that calls for the split. Do both before
calling the step done.

**IMPORTANT: Prefix every command with `cd <absolute path> &&`.** Two repos are
in play, and the shell's working directory persists between calls.

**Environment:** `git filter-repo` comes from the sicompass dev shell. Check
with `command -v git-filter-repo`, and fall back to `nix develop -c`.

## Arguments

- `<path-in-sicompass>`: the directory that becomes the new repo's root.
- `<new-repo-name>`: the directory name in `../` and the GitHub name under
  `friendlyflow/`. Plugins are `<name>_plugin_sicompass` (for example
  `projectmanagement_plugin_sicompass`). The others keep their crate name.
- `--path <extra>`: extra paths whose history comes along (docs, scripts, shared
  assets). Each one also needs a `--path-rename` so it lands where the new
  layout expects it, since everything else is relative to the new root.
- `--kind`: `compositor`, `greeter`, `ui`, `crate` or `plugin`. It goes into
  `repos.json`, and it decides the `about.toml` targets and the flake systems.

## Steps

1. **Preconditions.**
   - The paths being split have no uncommitted changes
     (`git status --short -- <path> <extras>`). The split clones committed
     history only, and filter-repo keeps nothing outside those paths, so work
     elsewhere in the tree does not matter.
   - Commits touching those paths should be pushed
     (`git log --oneline origin/main..main -- <path>` is empty). Otherwise the
     new repo's history holds commits sicompass's remote does not have yet. If
     some are unpushed, say so and push them with `/commit-and-push` before
     step 8.
   - `../<new-repo-name>` does not exist yet.

2. **Clone and filter.**
   ```sh
   cd <parent> && git clone --no-local <sicompass> <new-repo-name>
   cd <parent>/<new-repo-name> && git filter-repo \
     --path <path-in-sicompass>/ [--path <extra> ...] \
     --path-rename <path-in-sicompass>/: [--path-rename <extra>:<dest> ...]
   ```
   `--no-local` gives filter-repo a fresh clone, which it insists on. It also
   drops the `origin` remote, which is what we want, because it must never
   point at sicompass.
   Check the result: `git log --oneline | wc -l` is roughly the number of
   commits `git log --oneline -- <path>` shows in sicompass, and `ls` shows the
   crate at the root.

3. **Apply the template** from `.claude/skills/split-repo/template/`:
   - `LICENSE`: copy sicompass's `LICENSE` byte for byte (the dual commercial +
     GPLv3 text). Do not re-type it.
   - `about.hbs`: copy sicompass's, replacing `sicompass` in its `<title>` with
     the new name.
   - `about.toml`: fill `@TARGETS@` (Linux-only kinds:
     `"x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"`, plugins:
     `"wasm32-wasip2"`).
   - `README.md` and `CLAUDE.md`: fill every `@...@`. Write the README for
     someone who has never seen sicompass: what it is, how to install it, how to
     build it. Follow the prose rule (no em dashes, no semicolons). Carry over
     the architecture notes from sicompass's `CLAUDE.md` and `docs/` that are
     about this crate, and delete them from sicompass in the same step, leaving
     a one-line pointer.
   - `gitignore` becomes `.gitignore`, `dot-claude/` becomes `.claude/`,
     `dot-github/` becomes `.github/`. They are stored under other names so
     that Claude Code does not pick up the template's skills as skills of
     sicompass itself.
   - `flake.nix`: fill `@...@`. `craneLib.cleanCargoSource` keeps only Cargo
     and `.rs` files. If the crate `include_bytes!`/`include_str!`s anything
     else (`.ftl`, `.spv`, `.ttf`, `.json`, `.webp`), widen the filter with
     `lib.fileset`, or the Nix build fails even though `cargo build` works.
   - `chmod +x .claude/hooks/*.sh`.

4. **Make `Cargo.toml` standalone.**
   - Replace every `*.workspace = true` with a real value. The version is
     `0.2.0`, the edition `2024` and the license `GPL-3.0-only`. Dependencies
     take the version sicompass's `[workspace.dependencies]` pins, *with* the
     comment explaining any pin.
   - First-party crates: `sicompass-sdk` from crates.io (the version sicompass
     pins). `sicompass-ui` and `sicompass-payments` come by git with `rev = ...`,
     switching to `tag = "v0.2.0"` at the release. Add the commented-out
     `[patch...]` sections pointing at `../<repo>`, with the same "must stay
     commented on main" note as sicompass's `[patch.crates-io]`.
   - Drop `publish = false`'s "internal crate" wording, but keep
     `publish = false`. Nothing but the SDK goes to crates.io.
   - Give it its own lock by **copying sicompass's `Cargo.lock`** and letting
     cargo prune it (`cargo metadata >/dev/null` rewrites it). That keeps every
     version the crate was last tested with. `cargo generate-lockfile` would
     silently pick the newest of everything instead. Then check that git
     dependencies kept their rev (`grep -A2 'name = "smithay"' Cargo.lock`).

5. **Verify locally, before any remote exists.**
   `cargo build`, `cargo test`, `cargo clippy --all-targets`, and
   `timeout 1200 nix build "git+file://$PWD"`. `git add -A` first, because a
   flake only sees tracked files.
   Then `cargo about generate about.hbs -o THIRD-PARTY-LICENSES.html`.

6. **Commit** the scaffolding as one commit on top of the carried-over history:
   `Split out of sicompass <short-sha> as a standalone repo`.

7. **Register it.** Append `{ "name", "path": "../<new-repo-name>", "kind" }` to
   sicompass's `.claude/repos.json`.

8. **Publish. Ask the user first**, every time, because this creates a public
   repo:
   ```sh
   gh repo create friendlyflow/<new-repo-name> --public \
     --description "<one line>" --source . --remote origin --push
   ```

9. **Report** the commit count carried over, the local verification results,
   the GitHub URL, and what is still left in sicompass for this step (the
   workspace member, flake outputs, CLAUDE.md or docs sections, hooks that name
   the crate).
