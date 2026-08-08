# WASM plugins

Third-party plugins are sandboxed WebAssembly components. This document is the
contract: what a plugin can do, what it cannot, and why the boundary sits where it
does.

If you are writing a plugin, start with the SDK's README and
`examples/hello-plugin` in the [sicompass-plugin-sdk][sdk] repo. This file is about
the host side.

[sdk]: https://github.com/friendlyflow/sicompass-plugin-sdk

## Why

Plugins used to load two ways, and both are gone.

`native` opened a `.so`/`.dll`/`.dylib` with `dlopen` and called
`sicompass_plugin_init` for a C vtable. `script` shelled out to `bun` for a
TypeScript file. Apple's stores forbid an app from executing downloaded native code,
and equally forbid bundling a general-purpose interpreter, so neither could ship at
all on macOS or iOS.

The second reason matters more. A native in-process plugin held **full process
privileges**. Every policy in `plugin.json` — `allowedHosts`, rate limits,
robots.txt — was advisory against it: a plugin that did not feel like honouring the
allowlist could open its own socket, and nothing could stop it. The manifest
described intentions, not constraints.

A WASM guest has no syscalls. Its entire ability to affect the outside world is the
set of functions the host links into it. So the capability list is enforced by
construction. That is the whole idea, and everything below follows from it.

## The shape of it

| Piece | Where |
|---|---|
| The contract | `src/sicompass/wit/sicompass-plugin.wit` (vendored; canonical copy lives in the SDK) |
| Engine, linker, capability audit | `src/sicompass/src/wasm_host/mod.rs` |
| `Provider` adapter | `src/sicompass/src/wasm_host/provider.rs` |
| Fuel, deadlines, memory caps | `src/sicompass/src/wasm_host/limits.rs` |
| The mediated `fetch` | `src/sicompass/src/wasm_host/host_fetch.rs` |
| Discovery and manifest | `src/sicompass/src/plugin_manifest.rs` |
| Instantiation | `src/sicompass/src/programs.rs` (`instantiate_user_plugin`) |

`WasmProvider` implements the SDK's `Provider` trait, so the rest of the app cannot
tell a plugin from a compiled-in built-in — both arrive as `Box<dyn Provider>`.

This lives in the app crate rather than a `lib_*` crate deliberately. The SDK
boundary rule is about not reaching around `sicompass-sdk` to talk to *provider*
crates; `wasmtime` is a third-party dependency like `sdl3`. Plugin discovery and
instantiation already live here, so the host belongs with them.

## Capabilities

A plugin can call exactly what `linker_for` links, and nothing else. There is no
`wasi:cli`, no `wasi:filesystem`, no `wasi:sockets`, no environment access — so
`std::fs` and `std::net` inside a guest compile and then fail, rather than reaching
anything.

**`interface host`** — always linked, grants no ambient authority:

| Function | Notes |
|---|---|
| `log` | Written to the host log, prefixed with the plugin name |
| `get-setting` | **Only** this plugin's own settings section |
| `now-millis` | Guests have no `wasi:clocks` |
| `translate` | Goes through the host's Fluent bundles |
| `read-asset` | **Only** files under `assets/` in this plugin's own install directory |

**`interface net`** — linked **only** when `plugin.json` declares a non-empty
`allowedHosts`:

| Function | Notes |
|---|---|
| `fetch` | The only network egress |
| `fetch-url-ffon` | Same, plus the host's HTML→FFON pipeline |

Network is a *separate interface* because wasmtime links host functions an
interface at a time. With `fetch` sitting beside `log`, "link logging" and "link
networking" would have been the same decision, and the conditional capability could
not have been expressed at all.

`get-setting` is scoped to the plugin's own section on purpose. Other providers'
sections hold API keys, IMAP passwords and licence certificates; a sandbox that
hands those over on request is not a sandbox.

`read-asset` is in `host` rather than behind a gate like `net`, even though it is the
one function here that reads a disk. The only reachable bytes are files under
`assets/` in the plugin's own install directory: files that plugin shipped and the
user installed. There is no user-visible authority in that for `plugin.json` to
declare, and gating it would have meant making `host` itself conditional, which is
exactly what makes `net`'s gate legible. The import still appears in a built
component's import list, so the audit below still sees that a plugin uses it.

## Guests target `wasm32-unknown-unknown`

Not `wasm32-wasip2`. wasip2's standard library declares `wasi:*` imports that the
host links none of, so such a guest would only instantiate under
`define_unknown_imports_as_traps()` — and that would reduce the import section from
a capability set to a hint.

Keeping guests WASI-free is what makes this true: **a component's import list is
what it can do.** `tests/wasm_plugin.rs` asserts it on a real artifact.

It also means componentizing needs no adapter, and no extra Rust target: nixpkgs'
rustc already ships `wasm32-unknown-unknown` std.

## The install-time audit

Because LTO drops imports a guest never calls, the import list of a *built*
component is the set of capabilities it actually uses. `audit_component_imports`
runs before instantiation and refuses a component that reaches beyond its manifest —
notably one importing `net` with no `allowedHosts` declared.

Instantiation would refuse it anyway, since the interface would not be linked. The
value is a clear, early diagnostic naming the mismatch instead of an opaque link
failure.

## Containment

A guest can loop forever, exhaust memory, or panic. None of that may take the host
down, so every call re-arms three independent limits:

- **Fuel** caps work per call, so an infinite loop runs out instead of hanging the
  render thread.
- **Epoch deadlines** cap wall-clock time. Fuel alone cannot catch a guest blocked
  *inside* a host function, because host time burns no fuel.
- **`StoreLimits`** caps memory (64 MiB), tables and instances.

Which one fires depends on the backend, and both are needed. Measured with the
`spin` command in `examples/hello-plugin`: under Cranelift the guest burns the fuel
budget well inside the deadline and traps with `OutOfFuel`; under Pulley the
interpreter is slow enough that the 10s deadline arrives first.

A trap **poisons** the instance. After one, the guest's linear memory is in an
arbitrary state, so continuing to call it would produce garbage rather than errors.
A poisoned provider answers like an inert one and surfaces its error **once** — not
per call, because `tick` runs every frame and re-queueing would bury whatever the
user was reading under ~60 error rows a second.

## JIT and no-JIT

| Feature | Backend | Use |
|---|---|---|
| `jit-wasm` (default) | Cranelift → native code | Everywhere except Apple's stores |
| `no-jit-wasm` | Cranelift → Pulley bytecode, interpreted | App Store, iOS |

```sh
cargo build --no-default-features --features no-jit-wasm,bundled-sdl3
```

Both keep Cranelift, and that is not an oversight. Pulley is an *interpreter for a
bytecode Cranelift lowers wasm into*, not a replacement for it — with `pulley`
alone, `Component::new` does not exist, because nothing can turn a `.wasm` into
anything runnable. What `no-jit-wasm` changes is the Cranelift *target*, so the
output is interpreted rather than executable: no W^X pages, no `allow-jit`
entitlement, and viable on iOS where runtime codegen is forbidden outright.

Cargo features are additive, so a feature cannot *remove* Cranelift — hence the
default-on/opt-out shape rather than an opt-in flag.

### Known limitation: interactive dashboards under Pulley

`dashboard_render` at 80x24 costs **0.49ms under Cranelift and 26.16ms under
Pulley** — 157% of a 60fps frame, before Vulkan, text shaping or AccessKit do
anything. A full-screen interactive dashboard cannot run at 60fps in the no-JIT
build. Smaller grids are usable (40x12 ≈ 49%).

Cost is linear in cell count (~13µs/cell) with negligible per-call overhead
(`poll()` is 43µs), so it is not the crossing — it is the guest lowering N
six-field records through the Canonical ABI under an interpreter. The structural fix
would be a flatter wire format (parallel `list<u32>` lowers close to a memcpy); that
is a WIT change and has not been made.

Reproduce with:

```sh
cargo test -p sicompass --test wasm_plugin -- --ignored --nocapture profile
```

## Startup cost

Compiling a component dominates plugin startup: **~1.2s for a 65 KiB plugin**,
against 0.4ms to instantiate one. Two caches handle it.

`load_component` keeps compiled components in-process, keyed on path, modification
time and length. The app builds a fresh provider set per tab, so without this the
same bytes are compiled once per tab. Mtime and length are in the key because the
updater swaps a plugin's `.wasm` in place, and a path-only key would keep running
the old code.

Wasmtime's on-disk cache carries the compile across runs, so only the first launch
after installing or updating a plugin pays at all. It is keyed on wasmtime's version
and configuration, so an upgrade or a backend change invalidates it.

## The manifest

`~/.config/sicompass/plugins/<name>/plugin.json`, beside the `.wasm`.

```json
{
  "name": "weather",
  "displayName": "weather",
  "type": "wasm",
  "entry": "plugin.wasm",
  "version": "1.0.0",
  "allowedHosts": ["api.weather.example"],
  "updateUrl": "https://example.com/weather/manifest.json",
  "pubkey": "<base64 ed25519>"
}
```

`type` defaults to `wasm`, so omitting it gets the sandbox rather than having to ask
for it. `native` and `script` are refused with an explanatory message rather than
skipped in silence.

**Why a manifest at all, when the component can describe itself?** Because a
capability declaration has to live *outside* the thing it constrains. A component
declaring its own `allowedHosts` would be self-attestation — worthless as a control,
and it could only be read after instantiating the very code it is meant to
constrain. The manifest is also what the user reads *before* enabling a plugin, what
the settings tree is built from while the plugin has never run, and what the updater
reads on a background thread without standing up a wasmtime instance. The audit
above only works because manifest and component are independent sources.

`describe()` does overlap it for `name`, `displayName` and `version`. The host
prefers `plugin.json` for the version and the component for the rest.

Plugins are discovered **at startup**, so a newly installed one needs a restart.

## Runtime assets

Every provider owns an `assets/` directory, and names what is in it the same way:

    asset:<provider>/<file>

**A plugin** ships its files in `assets/` beside its `plugin.json`. Two ways to reach
them, both confined to that directory:

- `host::read_asset("equipment.json")` — the guest gets the bytes.
- `asset:<plugin-name>/<file>` in an `<image>` or `<link>` tag, or returned from
  `dashboard-image-path` — the *host* resolves and reads it. Preferred for anything
  the host renders: no copy through guest memory, no decoding in the guest.

`<plugin-name>` is the **manifest** `name`, not the guest's self-reported
`describe().name`: the namespace is the boundary, so the untrusted side does not
choose it. `register_plugin_assets` installs the resolver at instantiation time, and
`read_confined_asset` enforces the boundary for both routes — refusing absolute
paths, `..`, anything that resolves outside `assets/` after symlinks (including via a
symlinked intermediate directory), anything that is not a regular file, and anything
over 16 MiB. A refusal and a missing file are indistinguishable to the guest, so this
cannot be turned into a probe for what exists on the host.

This also closed a gap. A guest's `<image>` value used to go straight to the image
decoder with no confinement and no rebasing, so a plugin could have the host open any
file the user could read and learn its dimensions through `texture_size`.

**A built-in provider** keeps its files in its own crate — `lib/lib_tutorial/assets/`,
`lib/lib_sales_demo/assets/` — `include_bytes!`s them, and publishes them in
`register()`:

```rust
const TEXTURE: &[u8] = include_bytes!("../assets/texture.jpg");
sicompass_sdk::assets::register_bytes("tutorial", "texture.jpg", TEXTURE);
```

They are compiled into the binary, like the shaders and fonts, so no packaging list
has to know about them. `sicompass --check` resolves every registered URI and reports
the byte count, which is how a provider naming an asset nobody registered gets
noticed. Plugin assets come from a resolver closure and so cannot be enumerated
there.

Known limitation: a guest could put another *installed* plugin's URI in its own
`<image>` tag, because the tag is a bare string with no provider context at the point
the host renders it. What that reaches is a file the user installed and can open in a
file manager, not a secret. `read-asset` has no such hole — it is scoped to the
calling plugin — and `dashboard-image-path` is checked against the manifest name,
because there the host knows who is asking.

## Testing

```sh
cargo test -p sicompass wasm_host          # unit
cargo test -p sicompass --test wasm_plugin # against a real component
cargo test -p sicompass --no-default-features --features no-jit-wasm
./scripts/verify-guest.sh                  # in the SDK repo: builds and audits a guest
```

`tests/fixtures/wasm/hello.wasm` and `net.wasm` are built from the SDK's
`examples/`. They are committed rather than built here so `cargo test` does not need
the wasm toolchain; the regeneration commands are in `tests/wasm_plugin.rs`.
`tests/fixtures/wasm/` doubles as a plugin directory, so
`tests/fixtures/wasm/assets/hello-asset.txt` is the hello fixture's own asset — the
guest reads it through `read-asset` and reports the byte count, which is what the
end-to-end asset tests check.

The security tests are the point: fuel exhaustion, memory caps, guest panics,
instance isolation, allowlist enforcement, robots.txt, redirect re-checking, asset
confinement against a real directory, and a component refused for reaching past its
manifest.

## Known limits

- **The allowlist is by name, so this is not full SSRF protection.** A hostname
  resolving to an internal address defeats the literal-address check, and DNS
  rebinding is unaddressed. Closing that needs resolve-then-connect-to-a-pinned-IP,
  which `reqwest` does not expose. The literal check stops the naive case.
- Interactive dashboards are impractical under `no-jit-wasm` (above).
- Plugins cannot claim a dashboard exit key. The host forwards every key to an
  interactive dashboard so a terminal emulator can receive Escape, so
  `dashboard-key`'s return value means *"redraw"*, not *"consumed"*, and leaving is
  the host's double-Ctrl+C.
