# Project Instructions

## Environment (Nix: NixOS, other Linux, and macOS)

The whole toolchain (`cargo`, `rustc`, `clippy`, `rustfmt`, `graphify`,
`lld` + `wasm-tools` (to inspect WASM plugin guests; they are built in the
SDK repo's shell, which has the `wasm32-wasip2` target),
`cmake` (SDL3 is compiled from source by the `bundled-sdl3` feature),
`librsvg`/`imagemagick`/`icoutils`/`libicns` (icons),
`xvfb-run`, SDL3/Vulkan link paths) comes from the flake dev shell in
[flake.nix](flake.nix). Nothing is installed system-wide.

`flake.nix` also exposes `packages.default`, so `nix build` and `nix run`
produce the installable Linux package. It reads the version from
`[workspace.package]` in `Cargo.toml`, so there is only one version to bump.

- **Check once per session**, then stick with the answer: `command -v cargo`.
  - Non-empty: the shell was launched from inside `nix develop`, so run
    `cargo test ...` directly.
  - Empty: prefix every toolchain command with `nix develop -c`, e.g.
    `nix develop -c cargo test -p sicompass-tutorial`.
- `nix develop -c <cmd>` always prints a `warning: Git tree ... is dirty` line on
  stderr first. That warning is noise, not a failure.
- Do not reintroduce a bare `exec fish` in the flake's `shellHook`. It is guarded
  by `[ -t 0 ]` on purpose: without the guard it replaces the process for
  `nix develop -c <cmd>`, and the command silently never runs (exit 0, no output).
- Crate package names differ from directory names: `lib/lib_<x>` is package
  `sicompass-<x>` (exception: `lib/lib_texteditor` is `sicompass-text-editor`).
  Crates under `src/` keep their directory name. `cargo test -p` takes the
  package name.
- The dev shell is platform-split. `aarch64-darwin` gets MoltenVK,
  `DYLD_FALLBACK_LIBRARY_PATH` and a `sysctl` job cap; Linux gets Wayland/X11,
  Mesa ICD discovery, `LD_LIBRARY_PATH` and xvfb. Anything added to the shared
  part of the `shellHook` has to hold on both. In particular, never reference a
  Linux-only package (`wayland`, `mesa`, `at-spi2-core`) outside the
  `lib.optionalString stdenv.hostPlatform.isLinux` branch: nixpkgs marks `wayland` bad on
  darwin, so a stray reference breaks `nix develop` at *eval* time on macOS,
  before anything is fetched.
- `x86_64-darwin` builds from a **second** nixpkgs input pinned to
  `nixpkgs-26.05-darwin`, selected by `nixpkgsInputFor`. Unstable (26.11)
  dropped Intel macOS and now *throws* on `import nixpkgs` for it, which would
  take down every eval of the flake on every platform, so it cannot simply be
  listed against the main input. That branch is supported until the end of 2026.
- `nix develop` overwrites `$SHELL` with its own store bash before the
  `shellHook` runs, so `$SHELL` is useless for detecting the user's shell there.
  The hook reads the OS user database instead (`getent`, then `/etc/passwd`,
  then `dscl` on macOS).

## Sibling repos

sicompass is being split into sibling repos under the same parent directory,
all driven from this checkout. [.claude/repos.json](.claude/repos.json) lists
them. So far:

- `../desicompass`, the Wayland compositor, together with the NixOS module
  (`services.desicompass.*`) that wires it, the app and the greeter into a
  session.
- `../loginsicompass`, the greetd login screen.
- `../sicompass-ui`, the renderer shared with the greeter, with the shaders
  and the embedded fonts. A git dependency, see "the sicompass-ui split" below.
- `../sicompass-plugin-sdk`, the SDK and the WASM plugin kit (on crates.io).

The Store (`lib/lib_store`, package `sicompass-store`) installs plugins from
the signed store list `lib/lib_store/store.json`. Edit it only through
`/store`, which re-signs it with `~/.config/sicompass/store.key`. Never read or
print that key. See [docs/plugin-platform.md](docs/plugin-platform.md) §8-9.

`/commit-and-push`, `/release`, `/sync` and `/update-cargo` take a repo name as
their first argument and then follow that repo's own copy of the skill (see
[.claude/repo-selection.md](.claude/repo-selection.md)). `/split-repo` moves a
crate out, keeping its history. Edits under `../<repo>` fire this repo's hooks,
and `run-tests.sh` runs the suite of the repo that owns the edited file.

## Code Style

### Rust

Follow standard Rust idioms. Use `#[allow(...)]` sparingly and only when justified.

### Documentation prose (`README.md` and `lib/lib_tutorial`)

In `README.md` and the tutorial content (`lib/lib_tutorial/src/lib.rs`), do not
use em dashes or semicolons. Use commas instead, or split into separate
sentences (parentheses are fine for true parentheticals).

### Tutorial authoring

When writing or restructuring the in-app tutorial (`lib/lib_tutorial/`), follow
the rules in [docs/tutorial-guidelines.md](docs/tutorial-guidelines.md): teach by
doing, one idea per step, confirm via the screen-reader announcement, keep a short
guided path separate from the reference manual, make keyboard shortcuts lead each
line, and add every new string to all four locale bundles.

## Generated files that are committed

Two things are generated by a script and committed, rather than built:

- `assets/icons/*` and `src/sicompass/wix/Product.ico` —
  [scripts/gen-icons.sh](scripts/gen-icons.sh). Rerun after editing either
  master SVG. The 256 px PNG is also the window icon, which the renderer
  embeds from its own copy: copy it into `../sicompass-ui/assets/icon-256x256.png`
  too, tag that repo, and move the pin. `tests/packaging.rs` fails until you do.
- `THIRD-PARTY-LICENSES.html` — `cargo about generate about.hbs`. `licenses.yml`
  fails if it drifts.

(The compiled shaders moved with the renderer to `../sicompass-ui`, whose
`scripts/gen-shaders.sh` regenerates them.)

Each script's header explains why it is not a build step. The short version:
they change about once a year, and the alternative is putting a non-Rust
toolchain on the critical path of every build on four CI runners, in the Nix
derivation, and on every contributor's machine.

## Architecture: standalone binary

Shaders, fonts and every provider asset are compiled into the executable
(`shaders.rs` and `fonts.rs` in the sicompass-ui repo, and `include_bytes!` in
each provider crate). `fonts/` here holds only the font license texts, which
every package must ship because the fonts are inside the binary.
`tests/packaging.rs` checks them against `sicompass_ui::fonts::LICENSES`. There is no runtime resource tree and nothing is located
relative to the executable, which is what makes one binary work from an archive,
a `.deb`, an `.rpm`, an AppImage, a macOS `.app`, the Windows MSI and Nix
without a wrapper script. The top-level `assets/` holds packaging inputs only
(icons, the `.desktop` entry), and `src/sicompass/tests/packaging.rs` enforces
that.

**A file the app reads at runtime belongs in the crate that owns it**, not in
`assets/`: put it in `lib/lib_<x>/assets/`, `include_bytes!` it, and publish it
in `register()` with `sicompass_sdk::assets::register_bytes("<provider>", "<file>", BYTES)`.
Refer to it as `asset:<provider>/<file>` wherever a path used to go — an
`<image>`/`<link>` tag, `dashboard_image_path()` — and the host resolves it. A
WASM plugin does the same, with its files in `<plugin_dir>/assets/`; see
[docs/wasm-plugins.md](docs/wasm-plugins.md).

Shipping a loose file instead means editing **four** hand-maintained lists that
nothing verifies (`include` in `dist-workspace.toml` reaches the archives only,
plus the cargo-packager `resources`, the `generate-rpm` assets and
`wix/main.wxs`). That is what made every release up to 0.1.8 unable to start.
See [docs/releasing.md](docs/releasing.md).

## Architecture: paid cloud backup

The app's half of the commercial client is `lib_store`'s `payments` module
(certificates, checkout, the tier pages' controls, redeem tokens, usage), shown
in Store > tiers. The server is the **separate, private** repo `../server` (the
Ed25519 signing key must never sit in GPL client code).

The backups themselves are the plugins'. Notes and project management are
plugins now (`../notes_plugin_sicompass`, `../projectmanagement_plugin_sicompass`)
and back up the way a third party's plugin would, with the `sicompass-payments`
guest library (`../payments_plugin_sicompass`). The host gives a plugin two
things through the `license` interface: where the user stands with a tier
(`license.standing`), and the redeem token, only for the tier its `plugin.json`
names as `service` (`license.token`, gated by `Grants::service_tier`).

Three things are easy to get wrong here:

- **The paywall is on the service, never on the data.** `payments/cert.rs`
  says verification is display-only, and that stays true: a plugin shows and
  saves the user's data whatever `license.standing` says. Only the copy on our
  server is gated.
- **A token is for one plugin's own service.** `license.token` answers only
  for `service.tier`. Widening it would hand every plugin the user's
  credential for a server that holds their data.
- **Tier pages are served from the Store's own tree**, not grafted into
  another provider, so a refresh after redeeming keeps the page (and the typed
  token) where it was.

Restoring never runs over a store that already has files in it. A backup is not
a sync, and the machine in front of the user wins.

## Architecture: text fields

Every editable value is an `<input>`, edited by the app's shared Insert mode
(wrapping, wrap-aware Up/Down, selection, one background block, Ctrl+Enter
newline). A surface that edits text itself (a dashboard) builds on
`sicompass_sdk::input` rather than its own string and caret. Follow
[docs/multiline-input.md](docs/multiline-input.md).

## Releasing

See [docs/releasing.md](docs/releasing.md) for the tag-to-artifact pipeline,
the pre-release checklist, and the post-release smoke test.

## Testing

- After implementing changes, always run relevant tests before finishing.
- Rust tests: `cargo test` (workspace-wide), or `cargo test -p <crate>` (specific crate).
- Integration tests: `src/sicompass/tests/integration.rs`
- When adding new code, write or update tests.
- If tests fail, fix the code — never leave a task with failing tests.

## Test Integrity

- Never remove or weaken test assertions to make a failing test pass. Fix the code instead.
- If a test itself is genuinely wrong and needs changing, **ask the user first** before modifying it.

## Architecture: Unified undo/redo (TimelineEntry model)

All reversible actions flow through `sicompass_sdk::timeline::TimelineEntry`.
When working on undo/redo, or adding any reversible provider action, follow
[docs/undo-redo-timeline.md](docs/undo-redo-timeline.md): the entry variants and
their coalescing rules, per-tab `Timeline` ownership, the `Provider` trait
hooks, the irreversibility caveats, and the legacy-stack migration state.

## Architecture: SDK boundary (hard rule)

Neither the `sicompass` app crate (`src/sicompass/src/**`) nor the renderer
crate `sicompass-ui` (the sicompass-ui repo, which enforces it with its own
Stop hook) may import any `lib_*` crate directly. All communication flows through `sicompass-sdk` (the `Provider`
trait, the factory registry, setting-injection hooks) plus the thin registration
crate `sicompass-builtins`. No exceptions — this includes `sicompass-settings`,
which is reached via `sdk::create_provider_by_name("settings")` and configured
through the `Provider` trait.

Tests (`src/sicompass/tests/**` and `#[cfg(test)]` blocks) may import concrete
lib crates for mock injection — these deps live in `[dev-dependencies]`.

A Stop hook (`.claude/hooks/check-sdk-boundary.sh`) enforces this automatically
at the end of each Claude turn.

## Architecture: the greeter

The greetd login screen is its own repo, `../loginsicompass`
(github:friendlyflow/loginsicompass), a separate binary that draws with the
app's renderer (`sicompass-ui`) and must never link this app crate. Its
`docs/greeter.md` covers the page it shows, the greetd conversation, user and
session enumeration, the password's lifetime, the GPU-failure supervisor, and
how to run it nested without touching the boot path.

## Architecture: the sicompass-ui split (hard rule)

The renderer lives in its own repo, `../sicompass-ui`
(github:friendlyflow/sicompass-ui), and this workspace depends on it by git
(`rev` between releases, `tag` at a release). Its `CLAUDE.md` is the full
reference. Work on the renderer and the app together by uncommenting the
`[patch."https://github.com/friendlyflow/sicompass-ui"]` section at the bottom
of `Cargo.toml`, and comment it out again before committing. It is shared by
two binaries: the `sicompass` application and the `loginsicompass` greetd
greeter. It holds the SDL3 window, the Vulkan device, font rasterisation, the
list layout, the key handlers and the AccessKit bridge. `src/sicompass` keeps
what only an *application* has: the provider catalogue and the `settings.json`
that selects from it (`programs`), the WASM plugin host, the self-updater and
the Windows Start Menu entry.

**`sicompass-ui` must not depend on `sicompass-builtins`, `sicompass-updater`,
`wasmtime` or `reqwest`.** That is the rule the split exists to enforce: linking
the application into a login screen cost 465 crates, including a bundled SQLite,
a headless-Chromium driver, an IMAP client and an SMTP client, none of which a
login screen ever calls. The sicompass-ui repo's Stop hook checks this.

Where the renderer needs something only the embedder can answer, it asks:

- `registry::HostHooks` — six methods, every one defaulting to a no-op, stored
  on `AppRenderer`. The app installs `boot::ProgramsHooks`; the greeter takes
  the defaults, which are all correct for something with no settings file, no
  updater and no tabs. Integration tests must install the app's hooks too (see
  `app_renderer()` in `tests/integration.rs`), or opening a tab silently builds
  an empty provider set.
- `http::register_body_fetcher` — an HTTP client for following `<link>` and for
  `<image>` values that are URLs. Same shape as
  `sicompass_sdk::register_url_fetcher`. Unregistered, an HTTP link reports that
  it cannot be followed and the node still renders.
- `app_state::AppConfig` — everything the window used to hardcode (title,
  `app_id`, size, custom titlebar, maximized, fullscreen, icon, font scale).
  `Default` reproduces the application exactly, and a test asserts it, so a
  field added here must default to whatever the line it replaced did.

Because `AppState` now belongs to another crate, Rust's orphan rule stops the
app adding an inherent `AppState::new()`. Application startup is the free
function `boot::app_state()` instead.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
