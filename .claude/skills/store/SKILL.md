---
name: store
description: Add, change or remove an entry in the store list (lib/lib_store/store.json), then re-sign it with the maintainer's store key and verify
argument-hint: "pending | onboard <name> [category] | decline <name> <reason> | freeze-check | freeze <name> <tag> | unfreeze <name> | add <name> <owner/repo> <pubkey> [category] | revoke <name> <archive-sha256> | remove <name> | tier <id> <issuer-pubkey> <checkout-url> <fluent-title-id> | sign"
disable-model-invocation: true
allowed-tools:
  - Read
  - Edit
  - "Bash(cargo run:*)"
  - "Bash(cargo test:*)"
  - "Bash(git diff:*)"
  - "Bash(git status:*)"
  - "Bash(curl:*)"
  - "Bash(gh release list:*)"
  - "Bash(gh issue list:*)"
  - "Bash(gh issue view:*)"
  - "Bash(gh issue comment:*)"
  - "Bash(gh issue close:*)"
---

# /store

The store list is `lib/lib_store/store.json`, signed as
`lib/lib_store/store.json.sig`. The app believes it only when that signature is
made by one of the two store keys whose public halves are in
`lib/lib_store/src/source.rs` (`TRUSTED_KEYS`). Design:
`docs/plugin-platform.md` §8.

## Rules

- **Never read, print, copy or commit a secret key.** The working key is
  `~/.config/sicompass/store.key` (mode 600, outside Dropbox). Only the path
  is passed to the tool. The backup key is offline and is not used here.
- The list holds **no versions**, with one exception: a frozen entry's
  `frozen` release tag. A plugin release never needs this skill. Only a new
  plugin, a changed plugin key or repo, a revoked release, a tier, or a freeze
  does.
- **A third party's plugin is listed only while its developer holds Sicompass
  Cloud or Sicompass Commercial** (free and proprietary plugins alike). They
  submit from Store > publish in the app. The license server records the
  submission and opens a GitHub issue labelled `store-submission` in
  `friendlyflow/sicompass`, which is how the maintainer hears of it. The
  server's public listing says whose tier is active:
  `curl -s https://store.sicompass.org/developer/listing` (the `storeUrl`
  setting's server; `plugins.<name>` holds `repo`, `pubkey`, `issue`,
  `status` and, when lapsed, `lapsedAt` in Unix seconds, or `null` when the
  time is unknown). Never add a third party's plugin without an `active`
  submission. First-party `friendlyflow/*` plugins are never in the listing.
- A plugin's `pubkey` is the public key its maintainer printed with
  `sicompass-plugin keygen`. Take it from them, never generate one for them.
- `name` must equal the plugin's `plugin.json` name, and `repo` is where its
  GitHub releases are published (`releases/latest/download/release.json`).
- A `service` must name a tier that is in `tiers`.
- Ask before committing or pushing. The live Store reads the copy on `main`,
  so pushing is what publishes the change.

## Third-party submissions

These end in the signing steps below, except `pending` and `decline`, which
change nothing in the list.

- `pending`: `gh issue list -R friendlyflow/sicompass --label store-submission`,
  each with its listing `status`. Nothing else.
- `onboard <name> [category]`: the listing must have `<name>` with `status:
  active`, otherwise stop and say why. Show its repo, pubkey and issue
  (`gh issue view <issue> -R friendlyflow/sicompass`), and go through the
  issue's checklist with the maintainer:
  - its `plugin.json` names it `<name>` (read it from the repo's latest
    release), and its releases verify against the submitted key;
  - it never gates the user's data;
  - a paid service (`service`) or paid features of its own (`paidFeatures`)
    are disclosed. Ask which applies, and set them on the entry.

  Only after the maintainer says yes, do what `add` does with the submitted
  repo and pubkey, then steps 2-4. After the push, close the issue:
  `gh issue close <issue> -R friendlyflow/sicompass --comment "Listed in the store."`
- `decline <name> <reason>`: comment the reason on the issue and close it
  (`gh issue close <issue> -R friendlyflow/sicompass --comment "<reason>"`).
  The developer sees "not accepted" in Store > publish. Nothing is signed.
- `freeze-check`: for every `store.json` entry that is in the listing:
  - `lapsed` and not frozen: set `frozen` to the newest release tag published
    at or before `lapsedAt`, from
    `gh release list -R <repo> --json tagName,publishedAt,isDraft,isPrerelease`
    (no drafts or pre-releases). With `lapsedAt` `null`, the newest release
    now. Comment on its issue: "Updates are paused at <tag>: the developer's
    tier lapsed."
  - `active` and frozen: remove `frozen`, and comment "Updates resume." on its
    issue.

  Show what changes before signing. No change means nothing to sign.
- `freeze <name> <tag>` and `unfreeze <name>`: the same edits by hand.

## Steps

1. Edit `lib/lib_store/store.json` for the requested change (`$ARGUMENTS`).
   Keep 2-space JSON and the existing order: `version`, `tiers`, `plugins`.
   - `add`: append `{ "name", "repo", "pubkey", "category"? }`.
   - `revoke`: add the archive's SHA-256 (lowercase hex, from that release's
     `release.json` `archiveSha256`) to the plugin's `revoked` list.
   - `freeze` / `unfreeze`: set or delete the entry's `"frozen": "<tag>"`,
     after `revoked`. The Store then installs that release and offers nothing
     newer (`releases/download/<tag>/`). Older sicompass versions ignore it.
   - `remove`: delete the plugin's entry. Installed copies stay installed,
     they just stop getting updates from the Store.
   - `tier`: add or change `tiers.<id>` with `issuer`, `checkout`, `title`.
     The title is a Fluent id, so add it to all four
     `lib/lib_store/locales/*.ftl` bundles.
   - `sign`: no edit, only re-sign (steps 2-4).
2. Sign it, from the SDK repo's tool (its shell has the toolchain):
   ```
   cd ../sicompass-plugin-sdk && nix develop "git+file://$PWD" -c \
     cargo run -q --manifest-path tools/sicompass-plugin/Cargo.toml -- \
     store-sign --key ~/.config/sicompass/store.key ../sicompass/lib/lib_store/store.json
   ```
   It refuses a list that does not parse or check (unknown tier, duplicate
   name, bad key or repo), and prints the public key it signed with, which
   must be the first of `TRUSTED_KEYS`.
3. Verify against both trusted keys, the way the app does:
   ```
   cargo run -q --manifest-path tools/sicompass-plugin/Cargo.toml -- store-verify \
     --pubkey <TRUSTED_KEYS[0]> --pubkey <TRUSTED_KEYS[1]> ../sicompass/lib/lib_store/store.json
   ```
4. Back in sicompass: `cargo test -p sicompass-store`
   (`the_compiled_store_list_is_signed_by_a_trusted_key` must pass), then show
   `git diff lib/lib_store/` and ask before `/commit-and-push`.

## If the working key is lost or leaked

Do not use this skill. The maintainer signs with the offline backup key
instead, then a new working key is generated (`sicompass-plugin keygen`) and
its public half replaces `TRUSTED_KEYS[0]` in the next app release.
