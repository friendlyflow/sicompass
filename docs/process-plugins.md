# Plugin processes

A plugin is a program of its own. sicompass starts it, talks to it over its stdin
and stdout, and lets it go when the tab closes. This document is the host side:
what the app does with a plugin process, and why it does it that way.

If you are writing a plugin, start with the SDK's `sicompass_sdk::plugin` module
(the `Plugin` trait and `main!`) in the [sicompass-plugin-sdk][sdk] repo, and with
any plugin repo as an example (`salesdemo-plugin-sicompass` is the smallest).

[sdk]: https://github.com/friendlyflow/sicompass-plugin-sdk

## Why a process

Until 0.2.x, plugins were sandboxed WebAssembly components (`docs/wasm-plugins.md`
in git history).
The sandbox was real: a component could reach only the host functions linked into
it, so `plugin.json`'s permissions were enforced by construction. It also cost a
great deal. Every capability a plugin wanted (a PTY, a socket, a child process with
a private pipe, a browser sign-in, background work) had to be a host function first,
and the plugins grew a second copy of their logic for the sandbox beside the
native one their tests used.

A plugin process has neither problem. It is ordinary Rust with `std`: threads,
files, sockets, programs and the network work the way they work everywhere. What
it gives up is the sandbox. **It runs with the user's rights**, like any program
they start.

That has consequences, accepted on purpose:

- `plugin.json`'s `permissions` say what the plugin means to do. The Store shows
  them before install, but nothing here enforces them.
- A plugin can read `settings.json`, which holds other providers' secrets and the
  user's licence certificates. The token scoping in `license.token` is the app's
  manners, not a boundary.
- No Mac App Store or iOS build: Apple forbids running downloaded native code.
- One build per platform in every release.

## The shape of it

| Piece | Where |
|---|---|
| The protocol: records, messages, framing | `sicompass_sdk::plugin_ipc` (SDK `src/plugin_ipc/`) |
| The plugin side: `Plugin`, `main!`, the runtime | `sicompass_sdk::plugin` (SDK `src/plugin/`) |
| Starting a plugin, the channel, letting it go | `src/plugin_host/channel.rs` |
| What a plugin asks the app, and the answers | `src/plugin_host/services.rs` |
| The `Provider` it wears | `src/plugin_host/provider.rs` |
| Discovery and instantiation | `src/programs.rs` (`instantiate_user_plugin`) |

`ProcessProvider` implements the SDK's `Provider` trait, so the rest of the app
cannot tell a plugin from a compiled-in built-in. The app calls `tick` for every
provider on every frame, so one `poll` call answers what five trait methods ask,
and the answer is cached. Values that never change come from one `describe`
call.

## The protocol

Each message is a little-endian `u32` byte count, then the message in postcard
(`plugin_ipc::Message`). Either side may call the other at any time, so every call
carries an id and its reply names it.

1. The plugin speaks first: `Hello { protocol, name }`. The app refuses a plugin
   whose protocol major differs from its own (`PROTOCOL_VERSION`, now `1.2`) and
   says "update it from the Store". It keeps the version the plugin named
   (`Channel::speaks`).
2. The app calls with `Call { id, request }`. There is one `Request` per thing a
   provider does, the same set the WIT `provider` interface had, plus
   `LocaleChanged`, (1.1) `CannotAddHere` and (1.2) `AllowsScrollPrefetch`. The plugin answers with `Reply { id, response, moved_to }`.
   `moved_to` is where the plugin is now, when the call moved it (a command, an
   edit, a shell's `cd`), so `current_path` never needs a call.
3. The plugin asks with `HostCall { id, request }` for what only the app knows, and
   gets `HostReply { id, response }`. It may do so in the middle of answering a
   call, and from any of its threads.

FFON still travels as bytes in the SDK's binary codec, because it is a tree.

A minor bump adds requests at the end. A peer that does not know one cannot
read it: postcard fails on the unknown variant, and a plugin's runtime takes
the channel for broken and exits. So the app sends a request only to a plugin
whose hello names the minor that added it, and treats an older plugin like the
trait's default (`CannotAddHere` is `None` for a 1.0 plugin). The exception
is a default only safe for a plugin that could have said no:
`AllowsScrollPrefetch` is `true` in the trait and `false` for a plugin older
than 1.2, because scroll mode (`S`) then calls `fetch` for levels the user has
not opened, and a plugin whose fetch marks mail read must be able to refuse.

### What a plugin asks the app

| Request | Why it is the app's |
|---|---|
| `GetSetting` | its own `plugin.json` settings, from the user's `settings.json` section for it. They also arrive with `Init` and through `OnSettingChange` |
| `Translate` | the app's Fluent bundles, which hold the plugin's own `locales/` in the user's language. The runtime caches them until `LocaleChanged` |
| `LicenseStatus`, `LicenseStanding`, `LicenseToken` | the Store's certificates. The token only for the tier `plugin.json` names as its `service` |
| `OpenUrl`, `OpenPath`, `Applications`, `OpenWith` | the user's desktop, the same code as the built-ins |
| `Trash`, `Restore` | the OS trash, through the app, so the undo tests' stub (`TEST_NO_TRASH`) covers plugins too |
| `OauthRedirect` | a browser sign-in on a loopback port the app opens once, ended when the plugin goes |
| `Rendered` | a page the plugin rendered for a link (`"rendersPages"`), taken only for URLs it was asked about |

Everything else is the plugin's own business: files, programs, sockets, HTTP.

## Starting and stopping

`Channel::spawn` starts `<plugin dir>/<entry><EXE_SUFFIX>` in its own directory,
with stdin, stdout and stderr piped (and `CREATE_NO_WINDOW` on Windows). `entry` has
no extension in `plugin.json`, so one manifest serves every platform.

- **stdout is the channel.** The plugin runtime moves the real stdin and stdout to
  private descriptors before any plugin code runs, points fd 0 at `/dev/null` and
  fd 1 at stderr. So a `println!` lands in the log, and a program the plugin starts
  with inherited stdio (git, the claude CLI) cannot read the app's messages
  (`sicompass_sdk::plugin` `stdio.rs`).
- **stderr is the log**, line by line into tracing (`target: "plugin"`), with the
  last lines kept to say why a plugin stopped.
- **ETXTBSY.** A just-written executable (the Store installing one, a test copying
  one) is briefly held open for writing by any process another thread forks in the
  meantime, and executing it fails. The spawn is retried for a few milliseconds.
- **One process per tab.** The app builds a provider set per tab, and each gets its
  own plugin process. A process starts in milliseconds, so there is no shared
  plugin process and no state shared between tabs. Only the active tab's providers
  are ticked.
- **Letting go.** Dropping the provider closes the plugin's stdin. The runtime sees
  the end, calls `cleanup` if the app did not, and exits, so a plugin stops what it
  started (Chrome, a shell) itself. One still running three seconds later is
  killed. Waiting for it happens on a thread of its own, so closing a tab never
  waits on a plugin. If the app crashes, the pipes close the same way.

## A slow plugin

The app calls a plugin on its UI thread and waits for the answer, as it always did
for a plugin that was a library or a script. There is no deadline: while a call
runs, the app draws nothing and takes no keys, so a plugin keeps anything slower
than a moment on a thread of its own and reports it through `poll`. The startup
handshake is the one exception, 10 seconds, so a broken executable cannot hang
loading. (`ProcessProvider::set_call_deadline` sets a limit per call, which the
tests use.)

## A broken plugin

A panic arrives as `Response::Failed` with its message (the runtime catches it,
answers, and exits, because the plugin's state is whatever it was mid-call). An
exit arrives as the end of the channel. An answer of the wrong shape is a broken
plugin too.

All of them **poison** the provider: the error is queued once for `take_error`, the
process is killed, and every later call answers like an inert provider. So does a
missed deadline, when one is set. `tick` runs
every frame, so the error is shown once, never 60 times a second. A dashboard frame
whose cell count disagrees with its grid is the one exception: it is reported and
drawn blank, without stopping the plugin, because it may be an off-by-one during a
resize.

## Installing

A release of a plugin process has one archive per platform,
`plugin-<target>.tar.gz`, named in `release.json` under `targets` by SHA-256, and
still one signature. `abi` is `process/1.0`, which an app from before 0.3 (which
ran WASM components) refuses, keeping the plugin version it has. This app refuses a
WASM release the same way, with a reason.

The Store reads `release.json`, picks this platform's archive
(`plugin_abi::plugin_target`), downloads only that one, checks it against the
signed hash, unpacks it into staging, checks the manifest against `release.json`,
makes the executable runnable, and swaps the folder in. What the user approves is
that it runs at all.

The targets: `x86_64` and `aarch64-unknown-linux-musl` (static, so one build runs on
every distribution, NixOS included), `x86_64` and `aarch64-apple-darwin`, and
`x86_64-pc-windows-msvc`.

**Approval.** A plugin process always needs the user's approval
(`plugin_abi::needs_approval`). Its fingerprint starts with `process;`, so an
approval given to a WASM plugin in 0.2 does not carry over. A plugin copied into
the plugins folder by hand has no approval either: it does not start, and its
Store entry says so, lists what it declares and offers "approve", which records
the approval and starts it, as an install would. A WASM plugin left on
disk is refused when the app starts ("update it from the Store"), and the Store
offers its program as an install. The Store's page leads with "runs as a program
on this computer, with your rights", then lists what the plugin declares, and an
update that declares more asks again.

## Plugins the computer's configuration provides

`SICOMPASS_PLUGIN_PATH` lists more plugin folders, separated like `PATH`, each
laid out like `~/.config/sicompass/plugins/` (one subfolder per plugin, as the
Store unpacks it). The desicompass NixOS module sets it for its dev session,
pointing at plugins built from local checkouts (`services.desicompass.dev.plugins`),
so `nixos-rebuild switch` puts a plugin edit into that session the way it does an
app edit.

A debug build of the app (`target/debug/sicompass`) does the same for itself
when the variable is unset (`src/dev_plugins.rs`): it links every sibling
checkout `../<x>-plugin-sicompass` that has a `target/debug/<x>-plugin` into
`target/dev-plugins/` and points the variable there. A debug `cargo build` of
the app runs `cargo build` in each of those checkouts too (`src/build.rs`,
only when a checkout's sources changed), so a plugin edit needs only
`cargo build` in sicompass, and a checkout that does not build fails it. A
checkout without a debug build keeps the Store's copy, and setting the
variable, even to nothing, turns this off, at build time as at run time.

- **Precedence.** These folders are read first and one plugin is kept per name
  (`installed_plugins::discover_all` in the SDK), so a plugin here replaces the
  user's copy of the same name. The app, the Store and the tutorial all read
  through that one function.
- **No approval.** It starts without asking (`plugin_manifest::approved_grants`)
  and is on unless the user switched it off. Whoever sets the session's
  environment can already replace sicompass itself, so asking would protect
  nothing. It is not a sandbox and not a boundary: such a plugin runs with the
  user's rights like every other.
- **The Store leaves it alone.** Its entry says the configuration provides it and
  offers no install, update, approve or uninstall. Pressing one anyway is
  refused, so a hidden second copy is never written under it.

## Testing

```sh
cargo test -p sicompass --test process_plugin   # against real plugin processes
cargo test -p sicompass plugin_host             # unit
cargo test -p sicompass-store                   # installing process releases
```

The fixtures are examples of the app crate (`src/examples/process_fixture*.rs`),
built by `cargo test`. They start a real process for every behaviour: settings,
translations, the licence token's scope, storage, config, the timeline, the
dashboard, rendered pages, the tab switcher's child pid, a panic, a hang with a
deadline set, a protocol from the future, and nothing left running after a tab
closes.

`tests/integration.rs` runs the real plugins. `src/Cargo.toml` pins each
plugin repo by git rev as a dev-dependency, `examples/plugin_<name>.rs` makes it a
program, and `tests/fixtures/plugins/<name>` holds its `plugin.json` and
`locales/` from the same rev. A plugin process sees the test's environment, so the
Claude plugin gets one with no `claude` to find and the web browser one whose
`PATH` is only a fake Chrome (`tests/fake_chrome`).

A real plugin, from an unpacked release or a checkout with its executable copied in:

```sh
SICOMPASS_TEST_PLUGIN=<plugin dir> cargo test -p sicompass --test process_plugin \
  -- --ignored a_real_plugin --nocapture
```
