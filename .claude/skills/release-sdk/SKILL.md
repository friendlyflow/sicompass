---
name: release-sdk
description: Bump and publish sicompass-sdk (and sicompass-pdk) to crates.io from the sibling plugin-SDK checkout
disable-model-invocation: true
model: sonnet
---

Publish a new `sicompass-sdk` to crates.io.

This skill lives in the sicompass repo but operates on the **sibling checkout**
`../sicompass-plugin-sdk`. That is deliberate: the SDK repo has no skills of its
own, and this is always run from here, immediately before `/release`.

**This publishes to the world.** A crates.io version is permanent — it can be
yanked but never replaced — so everything before step 6 is checking.

**IMPORTANT: The shell's working directory persists between Bash calls. Prefix
every command with `cd <repo root> &&`, using the absolute path of whichever of
the two repos that command belongs to.** Getting this wrong silently runs the
command in the other repo; a `git push` that reports `Everything up-to-date` is
the usual symptom.

## Why this exists separately from `/release`

`/release` cuts the *app*: it bumps `[workspace.package] version`, pushes a
`vX.Y.Z` tag, and lets cargo-dist build the binaries and native packages. It
never touches the SDK repo and never runs `cargo publish`. It only *reads* the
`sicompass-sdk = "..."` pin in `[workspace.dependencies]` and assumes that
version already exists on crates.io.

So the order for any change that crosses the plugin ABI is:

```
/commit-and-push   (both repos)   ->  land the code
/release-sdk       (this skill)   ->  publish the crate
/release                          ->  bump the pin, tag, ship the app
```

Between the middle and last step, `main` in the sicompass repo does not build
from crates.io. That window is expected; keep it short.

## Steps

1. **Verify clean state, both repos.** In each of `.` and
   `../sicompass-plugin-sdk`, `git status --short` must be empty and
   `git rev-list --left-right --count origin/main...main` must print `0  0`.
   If not, stop and tell the user — `/commit-and-push` first.

   Also confirm `[patch.crates-io]` in this repo's root `Cargo.toml` is still
   commented out. It is a local convenience for building the app against an
   unpublished SDK, and it must never be committed: with it live, a green build
   here proves nothing about what a user resolves from crates.io.

2. **Determine the SDK version.** Read `version` in
   `../sicompass-plugin-sdk/Cargo.toml` (it is a plain `[package]`, not a
   workspace) and compare with `git tag --sort=-v:refname | head -1` in that
   repo.

   Semver on the plugin ABI is not cosmetic. A **breaking** bump (`0.3.0` ->
   `0.4.0` in 0.x) is required when any of these changed:

   - the `wit/sicompass-plugin.wit` world — a renamed, added or removed export
     changes the component's import/export names, so every existing guest fails
     to instantiate;
   - a `Provider` trait method signature, which every built-in `lib_*` crate
     implements;
   - the `Plugin` trait or `export_plugin!` macro in `sicompass-pdk`.

   A patch bump covers doc, internal and formatting changes only.

3. **Bump the SDK.** Set `version` in `../sicompass-plugin-sdk/Cargo.toml`.
   There is no CHANGELOG in that repo, so this is the only edit.

4. **Decide whether `sicompass-pdk` ships too.** It is a second crate with its
   own version, its own workspace root, and a pin on the SDK:

   ```toml
   sicompass-sdk = { version = "0.3.0", path = "..", default-features = false }
   ```

   `cargo publish` strips the `path` and leaves the `version`, so **that pin is
   what a plugin author actually resolves**. If the SDK version moved and the
   pdk keeps pointing at the old one, authors build guests against a stale ABI
   and get link errors that look like their own bug.

   Ship the pdk whenever the WIT world, the `Plugin` trait or `export_plugin!`
   changed. When you do, bump both its `version` and its `sicompass-sdk` pin.

5. **Run the checks by hand.** Nothing runs on commit in that repo either.
   These are the two gates the release workflow applies, plus the full suite:

   ```sh
   cd ../sicompass-plugin-sdk
   cargo test --all                                                  # ~400 tests
   cargo test --test wit_contract                                    # the ABI contract
   cargo check --no-default-features --target wasm32-unknown-unknown # the guest half
   cargo fmt --all -- --check
   ```

   `wit_contract.rs` is the important one: it asserts the exact export list, the
   exact host import set, and that `net` stays a separate interface. It is what
   catches an ABI change that was only half-applied.

   Then prove the app still builds against the new SDK, without committing the
   patch — pass it on the command line instead:

   ```sh
   cd <sicompass root>
   cargo test --workspace \
     --config 'patch.crates-io.sicompass-sdk.path="../sicompass-plugin-sdk"'
   ```

6. **Commit, push and tag the SDK.**

   ```sh
   cd ../sicompass-plugin-sdk
   git add -A && git commit -m "Release: bump version to X.Y.Z"
   git push origin HEAD:main
   git tag -a vX.Y.Z -m "Release vX.Y.Z" && git push origin vX.Y.Z
   ```

   The tag fires `release.yml`, which reruns the two gates and then tries to
   publish. **Expect the publish step to fail with `403 Forbidden`** — see the
   notes. The tag is still worth pushing: it is the version marker, and the
   gates run on a clean checkout.

7. **Publish `sicompass-sdk` by hand**, from the SDK root:

   ```sh
   cd ../sicompass-plugin-sdk
   cargo publish --dry-run
   cargo publish
   ```

8. **Publish `sicompass-pdk`**, if step 4 said so. It must come second: its
   path dependency is stripped on publish, so crates.io has to already hold the
   SDK version it pins. Give the index a moment to catch up, then:

   ```sh
   cd ../sicompass-plugin-sdk/sicompass-pdk
   cargo publish --dry-run
   cargo publish
   ```

   Two traps here. The crate is its own workspace root, so it is *not* covered
   by `cargo test` at the SDK root and needs its own commands. And it only
   really builds for `wasm32-unknown-unknown`: `cargo check` passes on the host
   because check never links, but publish verification runs a real build of the
   `cdylib`. If that fails, verify against the true target instead:

   ```sh
   cargo publish --target wasm32-unknown-unknown
   ```

   Reach for `--no-verify` only as a last resort, and if you do, verify the way
   commit `c2c02ce` did: extract the packaged `.crate` and build it for
   `wasm32-unknown-unknown` against the published SDK. That also confirms the
   `wit/` symlink materialized as a real file in the tarball.

9. **Rebuild the committed WASM fixtures**, if the WIT world changed. This repo
   commits two guest components, and a stale one fails to instantiate against
   the new host:

   ```sh
   cd ../sicompass-plugin-sdk/examples/hello-plugin
   cargo build --release --target wasm32-unknown-unknown
   wasm-tools component new \
     target/wasm32-unknown-unknown/release/hello_plugin.wasm -o plugin.wasm
   cp plugin.wasm <sicompass root>/src/sicompass/tests/fixtures/wasm/hello.wasm
   ```

   Same for `examples/net-plugin` -> `tests/fixtures/wasm/net.wasm`. The header
   of `src/sicompass/tests/wasm_plugin.rs` documents both and explains why they
   are committed rather than built. Also check this repo's vendored
   `src/sicompass/wit/sicompass-plugin.wit` matches the SDK's copy —
   `wit_vendor_matches_host_tables` catches the drift, but only if you run it.

10. **Report and hand off.** Confirm the version is live
    (`cargo search sicompass-sdk`, or the crates.io page), then tell the user
    the app side is ready: `/release` will bump the `sicompass-sdk` pin in
    `[workspace.dependencies]` as part of its own version bump.

## Notes

- **The workflow's crates.io token is invalid.** `CARGO_REGISTRY_TOKEN` in the
  SDK repo has failed with `403 Forbidden: authentication failed` on v0.1.6,
  v0.2.0 and v0.3.0; all three went out via a local `cargo publish`. That is why
  steps 7 and 8 exist. Fixing the repository secret would collapse them into the
  tag push, and steps 6 to 8 would become one step.
- **`release.yml` publishes only `sicompass-sdk`.** Even with a working token,
  `sicompass-pdk` has no automated path and is always published by hand.
- A published version cannot be replaced. If a broken one goes out, `cargo yank`
  it and publish the next patch — do not try to reuse the number.
