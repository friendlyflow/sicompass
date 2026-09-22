# The login greeter

`src/loginsicompass` is the greetd greeter. It draws the login screen with the
application's own renderer, so a screen-reader user meets the same list, the
same prefixes and the same masked field at the login prompt as inside the app.

It is a separate binary, not a mode of `sicompass`. It links `sicompass-ui` and
deliberately not `sicompass` — see [The crate split](#the-crate-split).

```
greetd
  └─ desicompass --backend tty --startup-cmd loginsicompass-start
       └─ loginsicompass                      supervisor
            └─ loginsicompass --render-backend gpu   sicompass-ui, Vulkan
                 or, if that dies,
               loginsicompass --render-backend shm   tiny-skia, last resort
```

## The page

One provider, `LoginProvider`, whose `fetch()` is the whole screen:

```
+R User [nico]                    ← radio group, names its own selection
+R Session [Desicompass]
-i Password:                      ← cursor lands here at startup
-  <greetd's prompt>              ← only when greetd sent one
-  <last failure>                 ← sticky, so it can be re-read
-b Suspend
-b Restart
-b Shut down
-  Tuesday 22 September, 15:04
```

**There is no login button.** Enter in Insert mode on the password field
submits, which is how every other `<input>` in the app commits. A button would
be a second way to do one thing, and would sit between the field and the error
it produces.

The radio groups are served **path-scoped**: `fetch()` returns the whole page at
`/` and only that group's options at `/User`. Do not add `"login"` to the
`whole_tree` list in `sicompass-ui/src/provider.rs` — returning the whole tree
from inside a group grafts a copy of the page under one of its own descendants.

## The crate split

`sicompass-ui` holds the renderer; `sicompass` holds everything only an
*application* has. Linking the application into a login screen cost 465 crates —
wasmtime, a bundled SQLite, a headless-Chromium driver, an IMAP and an SMTP
client, none of them ever called. The greeter links 302.

The rule, enforced by `.claude/hooks/check-sdk-boundary.sh`: **`sicompass-ui`
must not depend on `sicompass-builtins`, `sicompass-updater`, `wasmtime` or
`reqwest`.** Where the renderer needs an answer only the embedder has, it asks:

| | |
|---|---|
| `registry::HostHooks` | settings, the updater, per-tab provider sets, and when to stop. Seven methods, all defaulting to a no-op — which is correct for the greeter. |
| `http::register_body_fetcher` | an HTTP client for `<link>` and URL `<image>` values. Unregistered, a link reports that it cannot be followed and the node still renders. |
| `app_state::AppConfig` | everything the window used to hardcode. `Default` reproduces the application exactly, and a test asserts it. |

`AppState` lives in `sicompass-ui`, so Rust's orphan rule stops the app adding
an inherent `AppState::new()`. Application startup is `boot::app_state()`.

## Talking to greetd

`greetd.rs` speaks the wire format from `greetd-ipc(7)`: a 32-bit length in
**native byte order**, then JSON. Native, not big-endian — an earlier revision
got this wrong and the greeter had never once completed a real authentication,
because greetd read `00 00 00 2c` as a 738 MB frame and waited. There is a test
pinned to the man page's published hexdump; do not loosen it.

`start_session` takes an **argv and an environment**, not a command line. A
session's `Exec=` is ten words on a NixOS host, and greetd would otherwise
`execve` a file whose name is the whole line. The environment is where
`XDG_SESSION_TYPE`, `XDG_SESSION_DESKTOP` and `XDG_CURRENT_DESKTOP` come from;
without them the session comes up subtly wrong.

`auth.rs` runs the conversation on its own thread, because PAM can take seconds
and the UI thread must keep drawing and keep talking to the screen reader.
`info` and `error` auth messages are acknowledged **inside the worker**: that is
a protocol obligation with no decision in it, and greetd waits forever without
it.

## Enumeration

No privileged helper. greetd owns PAM, so nothing here reads `/etc/shadow`, and
both sources are world-readable.

- **Users** — `/etc/passwd`, bounded by `UID_MIN`/`UID_MAX` from
  `/etc/login.defs` and filtered by login shell. Both defences matter and are
  tested separately: a NixOS host has 32 `nixbld` accounts above UID 1000, and
  `UID_MAX 29999` is what keeps them off the login screen.
- **Sessions** — `.desktop` files under `$XDG_DATA_DIRS/{wayland-sessions,
  xsessions}`, plus `/run/current-system/sw/share` unconditionally, plus
  `--sessions-dir`. The desktop-file **id** is the stable key; `Name=` is
  locale-dependent and is display only.

The remembered user and session are committed **only after `start_session`
succeeds**. Persisting on selection would make a mistyped username the
remembered default.

## The password

Masking is the renderer's, not the greeter's: `<password>` already masks the
drawn row, the per-keystroke spoken echo, the spoken context and in-field
search, and `accesskit_sdl.rs` has tests forbidding the value ever being spoken.

The buffer is zeroized — bytes wiped, not just the length reset — on every path
that ends a password edit. What that does *not* buy, stated so nobody assumes
more: the live FFON element holds the typed value while the field is being
edited (that is how the provider is handed it), the OS may have paged it out,
and PAM keeps its own copy. It closes the "same process, freed allocation"
window.

## When the GPU path fails

A login screen that does not appear leaves no graphical way into the machine,
and the Vulkan path can fail in ways that are not a `Result`: `.expect`s on
swapchain recreation, a driver taking `SIGSEGV`, SDL calling `abort()`.
`catch_unwind` covers the panics and nothing else.

So the process greetd starts is a supervisor. It re-execs itself as the GPU
greeter and, once, as the software fallback if that dies without having started
a session. It watches a **pipe, not the exit status**: greetd tears the greeter
down the instant `start_session` succeeds, so a successful child is often
`SIGKILL`ed a millisecond later and is otherwise indistinguishable from a crash.

`supervisor::decide` is pure and has the policy in one place. The fallback
(`--render-backend shm`) has no text, no accessibility and no pickers — it is a
way to get in and fix things, not a greeter anyone should meet twice.

Because it draws no text it cannot *show* a picker, so it has to be told who to
log in and what to start. It resolves that through the same enumeration the
graphical greeter uses — the remembered user and session, else the first of
each. It used to read `--user` and `--command`, whose defaults were `nobody` and
`false`; falling back should change how the login screen looks, not who it logs
in.

## Running it without touching the boot path

Nothing below needs `services.desicompass.greeter.enable`. That switch is the
last thing turned on.

```sh
# A greetd that always asks for a password and accepts the one you name.
# The socket path must be short: sockaddr_un caps it at ~108 bytes.
cargo run -p loginsicompass --example fake-greetd -- /tmp/greetd.sock hunter2

# The greeter, nested inside desicompass, inside your current session.
GREETD_SOCK=/tmp/greetd.sock cargo run -p desicompass -- --backend auto \
  --startup-cmd "$PWD/target/debug/loginsicompass --state-dir /tmp/lsc-state"
```

To prove the fallback rather than assume it, add
`SICOMPASS_FORCE_VULKAN_FAILURE=1` (debug builds only) and watch the log go
`starting the gpu greeter` → `falling back to the software renderer` →
`starting the shm greeter`.

Then, in order, and only then:

1. `services.desicompass.enable = true` — adds the session, leaves greetd alone.
   A failure costs one logout.
2. `services.desicompass.greeter.enable = true` — replaces the greeter.
   `nixos-rebuild build` before `switch`, a root shell already open on another
   VT, and `journalctl -t loginsicompass -b` afterwards to see which renderer it
   took.

`greeter.enable` asserts `enable`, because everything else is gated on the
latter and "I turned it on and nothing happened" is a bad way to learn that
about a login screen.

## What the NixOS module has to get right

- **`dbus-run-session`.** `accesskit_unix` speaks AT-SPI2 over the *session*
  bus. Without one the greeter stalls 400ms waiting for a registration that
  never arrives and is then mute to Orca — for an accessibility-first shell that
  is a failure, not a degradation. (COSMIC's own greeter does not do this, and
  its screen-reader toggle is consequently cosmetic on NixOS.)
- **`systemd-cat`.** greetd captures neither stdout nor stderr of what it
  starts, so a greeter that fell back — or failed twice — would say so to
  nobody.
- **A writable state directory.** The greeter user's home is `/var/empty`, so
  `XDG_CONFIG_HOME` and friends point into a tmpfiles-created
  `/var/lib/loginsicompass/xdg`. `sicompass_sdk::platform` honours those ahead
  of `$HOME`.
- **No `--user` or `--command`.** Those flags are what made the old greeter
  authenticate `nobody` and then launch `false`.
