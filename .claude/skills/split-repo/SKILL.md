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
  `friendlyflow/`. Plugins are `<name>-plugin-sicompass` (for example
  `projectmanagement-plugin-sicompass`). The others keep their crate name.
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
   **Find the crate's older paths first.** Files that moved into
   `<path-in-sicompass>` (a crate carved out of another, a `-rs` rename) carry
   their real history under the old paths, and a plain `--path` drops all of
   it. List them with
   `git log --follow --name-status --format= -- <file> | awk '/^[AR]/{print $2}'`
   for every tracked file, check that none is still tracked by another crate
   today, and pass them through `--paths-from-file`. In that file a
   `old==>new` line is **only a rename**: the old path must also be listed on a
   line of its own, or it is filtered out (sicompass-ui kept 9 commits instead
   of 289 until this was fixed).

   `--no-local` gives filter-repo a fresh clone, which it insists on. It also
   drops the `origin` remote, which is what we want, because it must never
   point at sicompass.
   **Then delete every tag**: `git tag -l | xargs -r git tag -d`. The clone
   carries sicompass's own release tags, rewritten onto the filtered history,
   and the new repo's versions start at its own first release.
   Check the result: `git log --oneline | wc -l` is roughly the number of
   commits `git log --oneline -- <path>` shows in sicompass, and `ls` shows the
   crate at the root.

3. **Apply the template** from `.claude/skills/split-repo/template/`:
   - `LICENSE`: copy sicompass's `LICENSE` byte for byte (the dual commercial +
     GPLv3 text). Do not re-type it.
   - `about.hbs`: copy sicompass's, replacing `sicompass` in its `<title>` with
     the new name.
   - `about.toml`: fill `@TARGETS@` (Linux-only kinds:
     `"x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"`, plugins: the
     five release targets, `"x86_64-unknown-linux-musl",
     "aarch64-unknown-linux-musl", "x86_64-apple-darwin",
     "aarch64-apple-darwin", "x86_64-pc-windows-msvc"`), and delete the
     sections of first-party crates the repo does not use.
   - `README.md` and `CLAUDE.md`: fill every `@...@`. Write the README for
     someone who has never seen sicompass: what it is, how to install it, how to
     build it. Follow the prose rule (no em dashes, no semicolons). Carry over
     the architecture notes from sicompass's `CLAUDE.md` and `docs/` that are
     about this crate, and delete them from sicompass in the same step, leaving
     a one-line pointer.
   - `gitignore` becomes `.gitignore`, `dot-claude/` becomes `.claude/`,
     `dot-github/` becomes `.github/`, `dot-cargo/` becomes `.cargo/`.
   - `.cargo/config.toml` is **not optional** for anything that links
     `sicompass-sdk`. It points every cargo-spawned process at a throwaway XDG
     tree under `target/`, and without it `cargo test` reads and writes the
     developer's real sicompass config, state and trash (see sicompass's own
     `.cargo/config.toml` for how that was found).
   - `.github/workflows/ci.yml` runs clippy with `-D warnings`. Keep that only
     if the crate is already clean. If it carries lints over from sicompass
     (whose CI does not deny warnings), drop the flag rather than fixing
     hundreds of lints as part of the split. They are stored under other names so
     that Claude Code does not pick up the template's skills as skills of
     sicompass itself.
   - `flake.nix`: fill `@...@`. `craneLib.cleanCargoSource` keeps only Cargo
     and `.rs` files. If the crate `include_bytes!`/`include_str!`s anything
     else (`.ftl`, `.spv`, `.ttf`, `.json`, `.webp`), widen the filter with
     `lib.fileset`, or the Nix build fails even though `cargo build` works.
   - `chmod +x .claude/hooks/*.sh`.

3b. **A plugin (`--kind plugin`) takes `template/plugin/` instead** of the
   top-level `flake.nix`, `gitignore` and `dot-github/workflows/ci.yml`. A plugin
   is a **plugin process**: a program sicompass starts and talks to over its
   stdin and stdout (sicompass's `docs/process-plugins.md`). The result has the
   shape of `../salesdemo-plugin-sicompass`, the reference plugin repo, so
   compare with it whenever in doubt.
   - `flake.nix`: fill `@NAME@` and `@DESCRIPTION@`. rust-overlay's toolchain
     with this computer's plugin target (static musl on Linux, which nixpkgs'
     rustc has no `std` for) and `jq`. There is no `packages.default`: a plugin
     ships as signed archives, not a Nix package.
   - `gitignore` becomes `.gitignore`. It adds `dist/` and `build/`, which the
     release script writes.
   - `scripts/release-plugin.sh` (keep it executable), as is:
     `build <target>` builds the program for one platform into
     `build/<target>/`, `pack` packs every build with `sicompass-plugin pack`,
     signs and verifies the way the Store will, and with no command it does
     both for this computer's platform. `--dry-run` signs with a throwaway key.
     The workflows call it, and so can a person before tagging.
   - `dot-github/workflows/release.yml` (tag `vX.Y.Z`: builds on five runners,
     Linux x86_64 and arm64 as static musl, macOS arm64 and x86_64, Windows
     x86_64, then packs, signs and verifies in one job and attaches one
     `plugin-<target>.tar.gz` per platform, `release.json` and
     `release.json.sig` to the GitHub release) and `dot-github/workflows/ci.yml`
     (tests, a dry-run release of this computer's platform, clippy, format).
     Both install the release tool from the SDK release in `SDK_TAG`. Keep it
     equal to the SDK version the plugin depends on.
   - `Cargo.toml`: a library with the plugin's logic, which the unit tests use,
     and a `[[bin]]` for the program, with the release profile salesdemo has:
     ```toml
     [[bin]]
     name = "<crate-name>"
     path = "src/main.rs"

     [dependencies]
     sicompass-sdk = { version = "<sdk>", default-features = false, features = ["plugin"] }

     [profile.release]
     opt-level = "s"
     lto = true
     strip = true
     codegen-units = 1
     ```
     The `[[bin]]` name is the crate name, which the release script builds and
     copies to `plugin.json`'s `entry`.
   - `src/main.rs` only names the plugin type, which the library defines and
     which implements `sicompass_sdk::plugin::Plugin`:
     ```rust
     //! The program sicompass starts: <what it is>, served over stdin and stdout.

     sicompass_sdk::plugin::main!(<crate_name>::<PluginType>);
     ```
   - `plugin.json` says `"type": "process"` and `"entry": "plugin"` (no
     extension, sicompass adds `.exe` on Windows), with `version` (equal to
     `[package] version`, and to the release tag) and the `permissions` it
     declares (docs/plugin-platform.md §4). Nothing enforces them: a plugin runs
     with the user's rights, and the Store shows them before install. Its
     strings are `locales/<lang>.ftl` with ids prefixed `<name>-`, in all four
     languages.
   - `README.md` and `CLAUDE.md` follow salesdemo's: the README's install
     section is the Store (and the plugins folder for a build of one's own),
     and its building section ends with `./scripts/release-plugin.sh
     --dry-run` instead of `nix build`. CLAUDE.md says it is a plugin process,
     what it declares, that the version lives in both `plugin.json` and
     `Cargo.toml`, that testing includes the dry run, and how releases are
     signed (below).
   - **Its signing key**, once per plugin. Ask the user before creating it, and
     never print, copy or commit the secret half:
     ```sh
     sicompass-plugin keygen --out ~/.config/sicompass/plugin-keys/<name>.key
     gh secret set PLUGIN_SIGNING_KEY -R friendlyflow/<new-repo-name> \
       < ~/.config/sicompass/plugin-keys/<name>.key
     gh variable set PLUGIN_PUBLIC_KEY -R friendlyflow/<new-repo-name> \
       --body "$(sicompass-plugin pubkey --key ~/.config/sicompass/plugin-keys/<name>.key)"
     ```
     (the `gh` steps after step 8, when the repo exists). The public key goes
     into the store list with `/store add <name> friendlyflow/<new-repo-name>
     <pubkey>`. Losing the key is survivable: the store list names a new one.
   - Check with `nix develop -c ./scripts/release-plugin.sh --dry-run` in step 5
     instead of `nix build`. It needs the release tool:
     `cargo install --git https://github.com/friendlyflow/sicompass-plugin-sdk sicompass-plugin`.
   - For sicompass's integration tests, pin the new repo by git `rev` as a
     dev-dependency in `src/Cargo.toml`, add
     `src/examples/plugin_<name>.rs` to build it into a program, and
     copy that commit's `plugin.json` and `locales/` into
     `src/tests/fixtures/plugins/<name>/`. The rev and the fixture
     folder move together.

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
   A dependant pinned by `git = "https://github.com/..."` cannot resolve until
   the repo is published, and a `[patch]` does not help, because cargo still
   fetches the original source. To test the dependant first, point its
   dependency at `git = "file:///<absolute path>", rev = "..."` for the moment,
   and switch it to the https URL (then `cargo metadata`, which rewrites only
   the `source` line in `Cargo.lock`) once the repo is pushed.

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
