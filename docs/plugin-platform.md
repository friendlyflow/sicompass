# The plugin platform (design, Step 4 of the 0.2.0 split)

Status: **design for review**. Nothing here is implemented yet. Once a part is
built, its section moves into [wasm-plugins.md](wasm-plugins.md) and this file
shrinks to what is still open.

Every program except the tutorial and Settings is leaving sicompass to become a
WASM plugin in its own repo, installed on demand from a Store. Today's plugin
host can run a sandboxed provider with no filesystem, no processes, no sockets,
no background work and no translations of its own, and can only be installed by
copying a folder by hand. This document is everything needed to close that gap:
the guest ABI, the permissions model, packaging and signing, the Store, and the
paid tiers.

## Contents

1. [Business model](#1-business-model)
2. [Guest toolchain](#2-guest-toolchain)
3. [The WIT world 0.2.0](#3-the-wit-world-020)
4. [Permissions](#4-permissions)
5. [Capabilities, one by one](#5-capabilities-one-by-one)
6. [Translations shipped by a plugin](#6-translations-shipped-by-a-plugin)
7. [Packaging and signing](#7-packaging-and-signing)
8. [The store](#8-the-store)
9. [lib_store](#9-lib_store)
10. [Tiers and certificates](#10-tiers-and-certificates)
11. [Host changes in sicompass](#11-host-changes-in-sicompass)
12. [SDK and pdk changes](#12-sdk-and-pdk-changes)
13. [Build order and tests](#13-build-order-and-tests)
14. [Decisions on the former open questions](#14-decisions-on-the-former-open-questions-2026-09-24)

---

## 1. Business model

Decided with the maintainer on 2026-09-24.

- **Plugins are free and GPL.** No first-party plugin checks a licence. What is
  paid is a **service on our server**: cloud backup today, anything else we host
  later. Someone who runs their own server points the plugin at it and needs
  nothing from us.
- **Never gate the user's data.** Whatever a paid service or a lapsed licence
  switches off, what the user already made stays viewable and exportable. This
  generalises the existing cloud-backup rule ("the paywall is on the service,
  never on the data").
- **Installing and updating free plugins through the Store is free.** The Store
  must never be the reason a new user, in particular a screen-reader user, cannot
  get the file browser, the text editor or the web browser.
- **Tiers, per user:**

  | Tier | Contents | Billing |
  |---|---|---|
  | Sicompass Cloud | cloud backup, and paid extras bought through the Store | monthly or yearly |
  | Sicompass Commercial | everything in Sicompass Cloud, plus the commercial licence: the right to adapt the code and share private adaptations within a closed circle without publishing them under the GPL | monthly or yearly |
  | Sponsor | a contribution, no service (kept from today) | the monthly sponsor tiers, or a donation of your choice above a minimum |
  | Support | paid help (kept from today) | yearly |

  Prices are set on the server. A discount programme for users with a disability,
  through partner organisations, is intended; the server issues those
  certificates like any other.

  The commercial licence is a legal right. The certificate proves it, and no code
  enforces it, exactly like today's `scope: "commercial"` certificates.
- **The Store says what is paid.** Every store entry states whether the plugin
  uses a paid service and which tier, before the user installs it.
- **Third parties choose, and disclose.** A third-party plugin may be proprietary
  and gate its own features. The store shows "paid features", and
  never-gate-the-data applies, checked when its store PR is reviewed. From day
  one a third party runs its own checkout and signs its own certificates. Our
  server acting as their issuer (a marketplace with Stripe Connect) is a later,
  server-only change.

## 2. Guest toolchain

Guests move from `wasm32-unknown-unknown` to **`wasm32-wasip2`**. rustc's wasip2
target links through `wasm-component-ld` and emits a component directly, so the
`wasm-tools component new` step disappears.

nixpkgs' rustc ships `std` only for `wasm32-unknown-unknown` (checked: neither
wasip1 nor wasip2 has a `std`). Guest builds therefore take their toolchain from
**rust-overlay**, pinned per repo:

```nix
rust-overlay.url = "github:oxalica/rust-overlay";
# ...
guestToolchain = pkgs.rust-bin.stable.latest.default.override {
  targets = [ "wasm32-wasip2" ];
};
```

This applies to the SDK repo (which gets its first flake, from the `/split-repo`
template), the plugin template and every plugin repo. sicompass's host build keeps
nixpkgs' rustc: the host never compiles a guest, and the committed test fixtures
are built in the SDK repo.

## 3. The WIT world 0.2.0

`package sicompass:plugin@0.2.0`. WASI is 0.2.9, the version the current
wasip2 `std` imports (measured: hello-plugin built for `wasm32-wasip2` imports
exactly the baseline below, `wasi:cli` stdio, environment, exit and `terminal-*`,
`wasi:io` and the monotonic clock, and nothing gated). The version finally moves (it has said `0.1.0`
through several breaking changes). An 0.1 guest does not instantiate on an 0.2
host, and the host says so in plain words rather than a linker error.

```wit
world plugin {
  // WASI p2, baseline: always linked, and inert without a permission.
  import wasi:cli/environment@0.2.x;   // no args, no env vars
  import wasi:cli/exit@0.2.x;
  import wasi:cli/stdin@0.2.x;         // always empty
  import wasi:cli/stdout@0.2.x;        // into the host log
  import wasi:cli/stderr@0.2.x;        // into the host log
  import wasi:clocks/wall-clock@0.2.x;
  import wasi:clocks/monotonic-clock@0.2.x;
  import wasi:random/random@0.2.x;
  import wasi:io/streams@0.2.x;        // and io/error, io/poll
  import wasi:cli/terminal-input@0.2.x;   // and terminal-output, terminal-stdin,
                                          // terminal-stdout, terminal-stderr:
                                          // always "not a terminal"
  import wasi:filesystem/types@0.2.x;  // no preopens without a permission
  import wasi:filesystem/preopens@0.2.x;

  // WASI p2, gated: linked only when plugin.json grants `sockets`.
  import wasi:sockets/network@0.2.x;
  import wasi:sockets/instance-network@0.2.x;
  import wasi:sockets/tcp@0.2.x;
  import wasi:sockets/tcp-create-socket@0.2.x;
  import wasi:sockets/ip-name-lookup@0.2.x;

  import host;      // always: log, get-setting, translate, translate-args, read-asset, now-millis
  import desktop;   // always: open-url, open-path, trash (paths confined to preopens)
  import license;   // always: status(tier)
  import tasks;     // always: spawn, cancel, emit
  import net;       // gated: allowedHosts
  import process;   // gated: process

  export provider;
}
```

The rule the current design is built on survives: **the import section is the
capability set.** What changes is that "baseline" now includes WASI interfaces
that grant nothing on their own. A filesystem with no preopens reaches nothing, an
empty environment leaks nothing, and stdout goes to the log. Everything that does
grant authority is still linked only when `plugin.json` asks for it, and the
install-time audit still rejects a component whose imports exceed its manifest.
`define_unknown_imports_as_traps` stays unused.

`wasi:http` is deliberately not offered. The existing `net` interface already
enforces the allowlist, robots.txt and quotas, and a second HTTP path would have
to re-implement all of it.

### Parity with the host `Provider` trait

The host trait has moved ahead of the WIT, and first-party plugins use those
parts. 0.2.0 adds:

- `navigation-request::select-path(list<u32>)`
- `set-dashboard-entry(list<u32>)`, `set-dashboard-palette(palette)`,
  `dashboard-uses-app-undo` in the descriptor
- `frame` gains `selection`, `half-gap-rows` and cursor style

Before the WIT is frozen, every `lib_*` crate's `impl Provider` is diffed against
it, so the 13 ports in Steps 5-10 do not each discover a missing method.

## 4. Permissions

`plugin.json` 0.2:

```json
{
  "name": "notes",
  "displayName": "notes",
  "version": "0.2.0",
  "entry": "plugin.wasm",
  "minAppVersion": "0.2.0",
  "description": "notes-description",
  "permissions": {
    "allowedHosts": ["cloud.sicompass.org"],
    "storage": true,
    "filesystem": [],
    "process": [],
    "sockets": []
  },
  "service": { "tier": "friendlyflow/cloud", "what": "notes-service-cloud-backup" },
  "settings": []
}
```

| Permission | Grants | Shown to the user as |
|---|---|---|
| `allowedHosts` | `net` linked, requests to these hosts only (as today) | "connects to cloud.sicompass.org" |
| `storage` | a preopen of `app_data_dir()/<name>`, the plugin's own folder, at `/storage` inside the guest | nothing (see below) |
| `filesystem` | preopens of these paths, at the same path inside the guest | "reads and writes your files in ~/" |
| `process` | `process` linked, may start only these programs | "runs git" |
| `sockets` | `wasi:sockets` linked, connections to these `host:port` pairs only | "connects to imap.gmail.com:993" |

- **`storage` is not a prompt.** Like `read-asset`, a folder that belongs to the
  plugin alone grants no authority over anything else. Because the folder is
  `app_data_dir()/<name>`, the notes and board plugins keep reading exactly the
  directories the built-ins use today (`…/notes`, `…/projectmanagement`), so
  existing data carries over with no migration.
- `filesystem` paths may use `~`. A preopen is mounted at the same absolute path
  inside the guest, so a file browser shows real paths, and `path-is-filesystem`
  keeps its meaning.
- `process` names programs, not paths. The host resolves them on `PATH` at spawn
  time. `"$SHELL"` means the user's login shell.
- `sockets` is enforced by wasmtime-wasi's socket-address check, and
  `ip-name-lookup` resolves only the names listed.

**Approval.** The Store shows the requested permissions before installing, in
plain words, spoken as one list. The user's grant is recorded in `settings.json`
under `pluginApprovals` as `{ "<name>": "<fingerprint>" }`, where the fingerprint
is `sicompass_sdk::plugin_abi::approval_fingerprint`: one canonical line of
hosts, folders, programs and sockets (storage is left out, it needs no approval).
Implemented in 4.4: a plugin whose `filesystem` access is not approved with the
current fingerprint does not load, and says so. **If an update asks for more, the plugin
stays on the old version, and the Store shows "this update asks for more
access" until the user approves.** A plugin copied into `plugins/` by hand asks
on first enable instead.

## 5. Capabilities, one by one

### desktop (always)

```wit
interface desktop {
  open-url: func(url: string) -> result<_, string>;   // http(s) and mailto only
  open-path: func(path: string) -> result<_, string>; // inside a preopen only
  trash: func(path: string) -> result<_, string>;     // inside a preopen only
  restore: func(path: string) -> result<_, string>;   // undo of a trash, by original path
}
```

Paths arrive as the guest sees them: `/storage/...` maps back to the plugin's
host folder, and a user-granted folder has the same path on both sides.

The host re-checks every path against the plugin's preopens after resolving
symlinks, the same way `read-asset` is confined today.

### tasks (always): background work

Every guest call runs on the UI thread under a 10 s deadline. Mail sync, the
Matrix long-poll, a cloud-backup upload and `git status` on a large repository
cannot. A task is **a second instance of the same component on a worker thread**:

```wit
interface tasks {
  spawn: func(name: string, input: list<u8>) -> u64;  // from the UI instance
  cancel: func(id: u64);
  emit: func(event: list<u8>);                        // from inside a task
}
// exported by the provider:
run-task: func(name: string, input: list<u8>) -> result<list<u8>, string>;
on-task-event: func(id: u64, event: task-event);      // progress / done, UI instance
```

- The worker instance gets the same linker and permissions, **its own store and
  memory**, no epoch deadline (a sync loop may run for hours) but a cancellation
  check at every epoch tick, and the same 64 MiB cap.
- State is not shared. A task receives its input and reports through `emit` and
  its result, which the host delivers to the UI instance through `on-task-event`,
  just before the next `poll`. That is the price of a sandbox with no shared
  memory, and it is the shape every existing worker thread in the libs already has
  (a channel into the provider).
- At most 4 tasks per plugin run at once. Further `spawn`s queue.
- Closing the provider cancels its tasks, including dropping it without
  `cleanup` (a tab closed, a hot reload).

Built in 4.5 (`src/wasm_host/tasks.rs`). As built: `spawn` returns
`result<u64, string>` and is refused inside a task; the interface also has
`cancelled()`, so a loop can stop cleanly. Cancellation takes effect the next
time the task runs WebAssembly: a task blocked inside a host call (a network
request, a sleep) stops when that call returns. Worker threads are named
`task:<plugin>`. The `task-plugin` example is the fixture.

### process (gated)

```wit
interface process {
  resource child {
    spawn: static func(program: string, args: list<string>, cwd: option<string>,
                       env: list<tuple<string, string>>, pty: option<pty-size>)
           -> result<child, string>;
    read: func(max: u32) -> list<u8>;       // non-blocking, stdout (and the pty)
    read-stderr: func(max: u32) -> list<u8>;
    write: func(bytes: list<u8>) -> result<_, string>;
    resize: func(size: pty-size);
    try-wait: func() -> option<s32>;
    kill: func();
  }
}
```

Reads never block, so a terminal reads its PTY from `poll`, which it already does
through `tick` today. The host implements it with `portable-pty`, which the
workspace already uses in `lib_shell`. `cwd` must be inside a preopen.

Built in 4.6 (`src/wasm_host/process.rs`). As built: `cwd` defaults to the
user's home; a program is a bare name from the approved list, resolved on
`PATH`, and `$SHELL` is the login shell from the user database; the child gets
the host's environment plus the plugin's `env` (the guest's own stays empty);
unread output is capped at 8 MiB per stream (the program is slowed, not the
host's memory grown); dropping the resource or the instance kills the program.
The audit and the linker work from the approved grants, so an unapproved
`process` import is refused before instantiation. The `process-plugin` example
is the fixture.

### sockets (gated)

This is plain `wasi:sockets` with the address check above. It exists for IMAP and
SMTP. TLS is the guest's job (rustls with a pure-Rust crypto provider), so the
host never sees a password or a plaintext mailbox.

Built in 4.7 (`src/wasm_host/sockets.rs`). As built: `wasi:sockets/ip-name-lookup`
is **never** linked (it would resolve any name, and a lookup of a made-up name is
enough to leak data). A plugin resolves through `sicompass:plugin/sockets.resolve`,
which answers only for approved `host:port` pairs, and connects by address;
`sicompass_pdk::sockets::connect(host, port)` does both. The host's socket check
lets a connection through only to an address an approved name resolves to, on its
port, plus the implicit bind `connect` makes; no listening, accepting or UDP. A
grant links what `std::net::TcpStream` imports on `wasm32-wasip2` (measured:
network, instance-network, tcp, tcp-create-socket, and the UDP pair, switched off).
Listed endpoints may be local (a mail bridge on `localhost`). `try_clone` is not
supported on wasip2. The `socket-plugin` example is the fixture.

### license (always)

```wit
interface license {
  enum tier-status { active, grace, expired, missing }
  status: func(tier: string) -> tier-status;
}
```

The host verifies the user's certificate for `tier` against the issuer key the
store lists for that tier. A plugin never handles keys or certificates.
First-party plugins do not call it (see §1). It exists for third parties.
(The enum is `tier-status` because WIT does not allow a type and a function
of one name in an interface.) Built in 4.9: the Store registers the check
(`sicompass_sdk::license::register_checker`), and a third party's certificate
is read from `providers/license-<anything>.json`.

## 6. Translations shipped by a plugin

A plugin ships `locales/<lang>.ftl`. At load, the host registers each file into
the global Fluent bundles, **only if every message id starts with `<name>-`**.
Otherwise the file is refused and the refusal logged. Without that rule, a plugin
could silently lose a key to a built-in (first registration wins) or override
another plugin's. `translate(key)` then works unchanged, and
`translate-args(key, list<tuple<string, string>>)` covers what built-ins do with
`t_args`.

## 7. Packaging and signing

A plugin release is **one archive plus two small files**, attached to a GitHub
Release under fixed names so `releases/latest/download/<file>` always works:

| File | Contents |
|---|---|
| `plugin.tar.gz` | `plugin.json`, `plugin.wasm`, `assets/`, `locales/`, `LICENSE`, `THIRD-PARTY-LICENSES.html` |
| `release.json` | `{ name, version, minAppVersion, permissions, service, archiveSha256 }` |
| `release.json.sig` | Ed25519 signature over `release.json`, by the plugin's key |

The signature covers `release.json`, which pins the archive by hash. So the Store
can show version, permissions and tier from a 1 KB download, and verify the
archive after fetching it. This replaces today's updater format, which signs only
`plugin.wasm` and therefore **drops a plugin's `assets/` on every update** (a bug
this fixes).

A new CLI in the SDK repo, `sicompass-plugin` (published with the SDK, so third
parties `cargo install` the same tool CI uses), does the whole cycle:

```
sicompass-plugin keygen                # prints the public key for the store
sicompass-plugin pack                  # builds, audits imports against plugin.json, archives
sicompass-plugin sign --key <file>     # writes release.json(.sig)
sicompass-plugin verify <dir|url>      # what the Store does, runnable by hand
```

The plugin template's `release.yml`: tag, then build, then `pack`, then
`sign` (key from the `PLUGIN_SIGNING_KEY` repo secret), then attach to the
release.

**Built in 4.10:** `/split-repo --kind plugin` uses `template/plugin/`: a
rust-overlay flake with `wasm32-wasip2`, `scripts/release-plugin.sh` (build,
pack and audit, sign, verify as the Store will, with `--dry-run` signing with a
throwaway key), and `release.yml`/`ci.yml` that run it. The release job also
checks the tag against `plugin.json`'s version and the signing key against the
`PLUGIN_PUBLIC_KEY` variable (the key the store list names), so neither mistake
reaches a user. Dry-run on a copy of `hello-plugin` outside the SDK tree: it
packs, signs and verifies, and a wrong key or tag is refused. Plugin keys are
kept in `~/.config/sicompass/plugin-keys/` and set as repo secrets with `gh`.
docs/wasm-plugins.md has "Publishing a plugin", the README and the tutorial
mention the Store.

## 8. The store

`lib/lib_store/store.json` and `store.json.sig`, in the sicompass repo.

```json
{
  "version": 1,
  "tiers": {
    "friendlyflow/cloud":      { "issuer": "<ed25519 pub>", "checkout": "https://…", "title": "store-tier-cloud" },
    "friendlyflow/commercial": { "issuer": "<ed25519 pub>", "checkout": "https://…", "title": "store-tier-commercial" }
  },
  "plugins": [
    { "name": "notes", "repo": "friendlyflow/notes_plugin_sicompass",
      "pubkey": "<ed25519 pub>", "category": "productivity",
      "service": "friendlyflow/cloud", "paidFeatures": false }
  ]
}
```

- `title` is a Fluent id in `lib_store`'s bundles, so "Sicompass Cloud" and
  "Sicompass Commercial" are translated, and renaming is a string change.
- The store holds **no versions**. The Store reads `release.json` from each
  repo's latest release, so releasing a plugin never needs a sicompass commit. It
  changes only when a plugin, a key or a tier is added.
- It is signed with a **friendlyflow store key**, whose public half is compiled
  into `lib_store`. A `/store` skill adds an entry and re-signs. A pull request
  can therefore propose an entry but cannot make the app trust it.
- **Key custody is the maintainer's**, and deliberately not CI's: the key file
  lives in `~/.config/sicompass/store.key` (mode 600), the app's own config
  directory, **outside Dropbox**, which syncs to the cloud. `lib_store` trusts **two** store keys:
  that working key and a cold backup key, generated at the same time and kept
  offline. If the working key is lost or leaked, the backup signs a store, and
  the next app release carries a new working key. A hardware key (FIDO2,
  `ed25519-sk`) can replace the working key later without changing the scheme.
- **Plugin keys rotate through the store.** The Store trusts whatever key the
  current signed store lists for a plugin, so a lost or leaked plugin key is
  replaced by a store change alone. The store also pins each plugin's
  **repo**, so a leaked key without write access to that repo ships nothing. Two
  more fields handle a leak: `revoked` (SHA-256 hashes of bad releases, which the
  Store refuses and offers to replace), and the Store never downgrades. A plugin
  installed by hand with only an `updateUrl` keeps trust-on-first-use, so its key
  rotates only by reinstalling, and the Store says so.
- **Live, with a fallback:** the Store fetches the signed copy from
  `raw.githubusercontent.com/friendlyflow/sicompass/main/lib/lib_store/` and falls
  back to the copy compiled into the binary when offline or when the signature
  does not verify.

## 9. lib_store

A new built-in provider, **Store**, always present like Settings and the
tutorial. It is not a plugin, because it is what installs plugins.

```
Store
+ Programs
  + notes                       installed, 0.2.0
    - Keeps notes in a tree
    - Uses: cloud backup (Cloud & Store)
    - Access: its own folder, connects to cloud.sicompass.org
    - <button>Update to 0.2.1</button>
    - <button>Uninstall</button>
  + email client                not installed
    - Access: connects to your mail servers, keeps a local copy
    - <button>Install</button>
+ Tiers
  + Cloud & Store               active until 2027-09-24
  + Commercial
  + Sponsor
  + Support
+ Licences
  - License redeem token: <input></input>
```

- **Install, update and uninstall happen in the running app.** The Store works in
  the background: download, verify, stage, audit the imports against the
  approved permissions, then swap into `plugins/<name>/` atomically. It then tells
  the app through the same queued-callback pattern Settings uses (the SDK
  boundary forbids a direct call). The app rescans, loads, **injects the plugin's
  settings** (today a hot enable skips them, which is a bug), and enables it. No
  restart, unlike today.
- **Updates** move here from `lib_updater`, which keeps only the app's own
  update. Store plugins update through their `release.json`. A plugin copied in
  by hand with an `updateUrl` uses the same format.
- **The tier pages move here from Settings**, with the licence redeem and the
  store URL. They became `lib_store`'s `payments` module in Step 7, and
  `lib_settings` stopped depending on them.
- **Uninstall** removes the program. It asks separately, defaulting to no,
  whether to delete the plugin's storage folder. That folder is user data.
- First-run: the tutorial points at the Store for the programs it describes, per
  [tutorial-guidelines.md](tutorial-guidelines.md), in all four languages.

**Built in 4.8** (`lib/lib_store`, package `sicompass-store`):

- Store > programs lists the signed store list. Each entry's title says its
  state (`notes, installed, version 0.2.0`), and inside are the latest
  version, the access in plain words, a paid service, and the buttons.
  Nothing touches the network until programs is opened.
- Install and Update run on a worker thread. They install only the exact
  release that was shown (a release published in between must be looked at
  again), refuse a revoked or older one and one that needs a newer sicompass,
  and swap `plugins/<name>/` in with a rename from `plugins/.store/`.
- The app receives `pluginInstalled`, `pluginUpdated` or `pluginRemoved`
  through the settings queue (`programs::wire_store`). It records the approval
  and the enable switch in `settings.json`, adds the program's line and its
  settings section, and loads or unloads it in every tab.
- `/store` edits and re-signs the list with `~/.config/sicompass/store.key`.
- Tests: `lib/lib_store/src/tests.rs` (wiremock, every refusal) and
  `src/sicompass/tests/store.rs` (a real component, install to uninstall).

- Before the swap, the component is audited against the permissions the user
  is approving, with the same wasmtime check a load runs
  (`wasm_host::audit_plugin_bytes`, registered through
  `sicompass_sdk::package::register_component_auditor`). With no auditor
  registered, nothing installs.
- After an uninstall the entry offers, as a separate button, to move the data
  folder to the trash. The app does it (`pluginDataTrash`) with its guarded
  trash, and refuses while the plugin is installed or when a built-in program
  shares the folder.
- Plugins installed by hand are listed too. With an `updateUrl` (the folder
  holding the three release files) and a `pubkey` in `plugin.json`, they
  update here, and an update naming another key is refused. `lib_updater`
  now updates only the app.

Tiers and licences come in 4.9.

## 10. Tiers and certificates

A certificate names its tier in `scope`: `friendlyflow/cloud`,
`friendlyflow/commercial`, `friendlyflow/sponsor`, `friendlyflow/support`, or a
third party's `acme/pro`. The `Payload` is unchanged.

(This first said "the payload gets a `tiers` list". It cannot: a client verifies
by deserializing into its own `Payload` and serializing it again, and a client
that does not know a field drops it, so every certificate issued with a new
field would fail on every 0.1.x install. One purchase is one tier, so `scope`
is enough.)

- A certificate is verified against the issuer key of the tier it claims, which
  the store lists. For `friendlyflow/*` that is today's licence key in
  `cert.rs`. A third party's key is only ever trusted for that party's own tiers.
- **Old certificates keep working:** `scope: "commercial"` (what "cloud and
  store" sold) maps to `friendlyflow/commercial`, which includes
  `friendlyflow/cloud`, so nobody who paid loses anything. `support` and
  `sponsor` map to theirs and stay valid until they expire.
- **Expiry:** 14 days of `grace` with a visible notice, then `expired`. For our
  tiers that stops the service, for example uploads, never the data. Nothing
  phones home at startup.
- **Devices:** a certificate names a person and works on all their devices, with
  no activation and no device list (honour system, as today). Limits live on the
  **server**, on the paid resource itself: a **monthly network-activity cap** and
  a **storage cap** per user for Sicompass Cloud. The server reports usage with
  each backup response, and the Store shows it ("2.1 GB of 10 GB, 340 MB of
  5 GB transferred this month"). Going over a cap pauses uploads with a notice,
  and never touches what is already stored or on disk.
- **`../server` changes** (private repo): the tier in `scope`, the Cloud and
  Commercial products (`cloud-monthly`, `cloud-yearly`, `commercial-monthly`,
  `commercial-yearly`, with a `/commercial` page), the mapping of existing
  licences, the 14-day grace on the backup routes, and the two caps (defaults
  10 GB stored and 5 GB a month, set in `.env`). A restore is never refused for
  traffic. The minimum donation is checked at checkout.

**Built in 4.9:** the above on both sides; Store > tiers, which replaces the
tier links in Settings and serves the server's tier pages from the Store's own
tree (a `<link>` graft has no path, so the refresh after redeeming a token put
the tiers list where the page was); the usage lines under the tiers; the
`license` import. `lib_settings` no longer depends on `sicompass-payments`.
Checked against a local server by hand (see each file's header): the
certificates per tier, Commercial including Cloud and grace on the client in
`lib/lib_store/tests/live_server.rs`, and grace on the server, usage and a
support licence refused for backup in `sicompass-plugin-sdk`'s
`sicompass-payments/tests/live_server.rs`.

## 11. Host changes in sicompass

- `wasmtime-wasi` 48 joins `wasmtime` in the app crate only. sicompass-ui, and
  so the greeter, stays free of it. That boundary is enforced by its Stop hook.
- `HostState` gains a `WasiCtx` built from the approved permissions: preopens,
  the socket check, stdout and stderr to the log, and an empty environment.
- `linker_for` links the baseline WASI and the always-on interfaces, and each
  gated interface only when granted. `audit_component_imports` checks against
  that same table, so the two cannot drift.
- Task workers are a small thread pool per plugin, with a channel of events
  drained before `poll`.
- `plugin_manifest.rs`: `permissions`, `service`, `description`, and
  `allowedHosts` at the top level still read, as an alias.
- The plugin cache becomes rescannable (`USER_PLUGIN_CACHE` is filled only at
  startup today).

## 12. SDK and pdk changes

- `sicompass-sdk` 0.9.0: the 0.2 WIT, and trait parity (§3).
- `sicompass-pdk` 0.6.0:
  - wasip2
  - `Plugin` gains `run_task`/`on_task_event` with typed helpers
  - `desktop`, `process`, `license` and `translate_args` wrappers
  - `std::fs` works inside the granted preopens, so the guidance "std::fs fails at
    runtime" goes away
- `sicompass-plugin` CLI (§7).
- `tests/wit_contract.rs`: `no_wasi_imports` becomes "WASI imports are exactly
  baseline plus gated", and the capability-set tests grow with the new
  interfaces.
- `verify-guest.sh`, or its replacement `sicompass-plugin pack`, which audits the
  same way: checks for wasip2 and the new allowed import table, and covers every
  example, not only hello-plugin.
- An example per capability: `hello` (baseline), `net`, `storage`, `fs`, `task`,
  `process`, `socket`. Each doubles as a host test fixture.
- The SDK repo gets the repo kit it lacks: a flake, `.claude/`, LICENSE and a
  CLAUDE.md.

## 13. Build order and tests

Each part ends in something runnable, like Steps 1-3.

| # | Part | Verified by |
|---|---|---|
| 4.1 | SDK repo kit and rust-overlay guest toolchain | hello-plugin builds for wasip2 under `nix develop` |
| 4.2 | WIT 0.2 baseline: WASI inert, permissions, audit, trait parity, plugin translations | `wit_contract`. Host tests: an fs call without a preopen fails, a plugin's `.ftl` with a foreign key is refused, 0.1 guests are refused with a clear message |
| 4.3 | `sicompass-plugin` CLI: keygen, pack, sign, verify | round-trip test. A tampered archive and a wrong key both fail |
| 4.4 | Capabilities: storage, filesystem, desktop | fixtures: read/write inside a preopen, `..` and symlink escapes refused, `trash` confined |
| 4.5 | Tasks | fixture: a task outlives the 10 s call deadline, is cancelled on close, and 4 run at once |
| 4.6 | Process and PTY | fixture: `echo` round-trip, a PTY reads a prompt, a program not listed is refused |
| 4.7 | Sockets | fixture: a listed `host:port` connects, an unlisted one is refused |
| 4.8 | Store and lib_store (programs): install, update, uninstall live, permission approval | integration test with wiremock serving a store and a signed release. Bad signature, bigger permissions and a tampered archive are each refused |
| 4.9 | Tiers: certificate `tiers`, `../server`, pages moved from Settings, `license` import | against a local `../server`: redeem, grace, expiry. Old certificates map correctly |
| 4.10 | Plugin release workflow in the template, docs, tutorial | a dry-run release of hello-plugin from a fork |

Step 5 (the salesdemo pilot) then exercises 4.2, 4.3 and 4.8 end to end, and
Steps 6-10 exercise the rest.

**Step 5 done (2026-09-24):** github.com/friendlyflow/salesdemo_plugin_sicompass
(32 commits of history), released as v0.2.0 by its release workflow with its own
key, listed in the store list, installed from GitHub through the Store into a
dev build, and `lib/lib_sales_demo` removed. The port needed two per-level
answers a cached descriptor cannot give (structural editing only where "Add
element:" is, the diagram only at the root), so `poll-result` gained
`structural-edit-here` and `dashboard-here`, and the host polls again after
every navigation.

**Step 6 done (2026-09-24), after a detour:** remote services were first
removed, because a plugin could reach only the hosts its `plugin.json` names,
and a remote server is whatever the user types in. They came back as
github.com/friendlyflow/remote_plugin_sicompass once plugins could ask for
**any public server**: `"allowedHosts": ["*"]`, which needs the user's approval
(shown in the Store as "connects to any server on the internet, never to your
own computer or local network") and is in the approval fingerprint. The host
still refuses internal addresses and honours robots.txt. One plugin serves every
server: `servers` (one `name URL` per line) and `apiKeys` (a new password
setting kind) in its settings section, and `migrate_remotes_to_plugin` moves
the old per-server sections there. It fetches each level itself with the key
as a bearer token, instead of leaving `<link>`s for the app to follow. Link
rows are an app feature regardless; `tests/links.rs` follows one end to end.

**Step 7 (2026-09-24):** plugins do their own cloud backup, the way a third
party's would. The `sicompass-payments` guest library
(`sicompass-payments/` in the SDK repo) holds the snapshot format, the backup protocol
over a plugin's `net`, the debounce, and `cloud::Cloud`, the service as a
plugin runs it (switch, row, uploads and restores as background tasks). The
host adds `license.standing` and `license.token`, the token only for the tier a
manifest names as its `service`. Notes and project management became
`notes_plugin_sicompass` and `projectmanagement_plugin_sicompass`, with
`storage` mapped to the folders the built-ins used, so existing notes and
boards open unchanged, and settings sections that keep their names. A user
who had them learns where they went from the Store: a listed program that is
not installed but has data here is named at the top of the Store (from the
compiled list, so without the network), comes first in programs with "your
data for it is on this computer", and says that installing opens it again. The backup
row no longer links to a tier page: it says where the user stands and points to
store, tiers. What was left of `lib_payments` became `lib_store`'s `payments`
module, and the Store asks `GET /usage` itself. The integration tests that
drove the built-in notes and board now load the released plugins from
`tests/fixtures/plugins`, which found two host bugs: a guest writing a file
during undo panicked (a nested tokio runtime), and a navigation request made in
`leave-dashboard` was only seen a frame later.

## 14. Decisions on the former open questions (2026-09-24)

1. **Store key custody:** a key file on the maintainer's machine, outside
   Dropbox, plus a cold offline backup key that the app also trusts (§8). Custody
   is the maintainer's responsibility. Claude generates keys and signs through the
   `/store` skill, but never holds them.
2. **Plugin key loss or leak:** the signed store is the sole authority, with a
   pinned repo, `revoked` releases and no downgrades (§8).
3. **Names and billing:** Sicompass Cloud and Sicompass Commercial, monthly or
   yearly; Sponsor pay-what-you-want; Support yearly; a disability discount
   programme (§1).
4. **Devices:** unlimited per person in the app; server-side caps on monthly
   network activity and on storage for the cloud service (§10).
