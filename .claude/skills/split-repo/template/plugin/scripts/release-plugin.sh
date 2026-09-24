#!/usr/bin/env bash
# Build this plugin, pack it, sign it and verify it the way the sicompass Store
# will. The release workflow runs exactly this; so can you, before tagging:
#
#   nix develop -c ./scripts/release-plugin.sh --dry-run
#
# Leaves dist/ with the three files a GitHub release carries, under the fixed
# names the Store downloads (releases/latest/download/<file>):
#   plugin.tar.gz  release.json  release.json.sig
#
# Environment:
#   PLUGIN_SIGNING_KEY  the secret key (base64, what `sicompass-plugin keygen`
#                       wrote). Required unless --dry-run, which signs with a
#                       throwaway key instead. Never echoed, never on disk
#                       outside a mode-600 temp file that is removed on exit.
#   PLUGIN_PUBLIC_KEY   the public key the store list has for this plugin. When
#                       set, the release must verify against it, which catches
#                       signing with the wrong key before anyone downloads it.
#   SICOMPASS_PLUGIN    the release tool, if not `sicompass-plugin` on PATH.
#   RELEASE_TAG         the tag being released (the workflow sets it); must be
#                       v<version in plugin.json>.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
TARGET=wasm32-wasip2
TOOL="${SICOMPASS_PLUGIN:-sicompass-plugin}"
DRY_RUN=0
[ "${1:-}" = "--dry-run" ] && DRY_RUN=1

fail() { printf 'FAIL %s\n' "$*" >&2; exit 1; }
say()  { printf '==> %s\n' "$*"; }

command -v "$TOOL" >/dev/null || fail "$TOOL not found (cargo install --git https://github.com/friendlyflow/sicompass-plugin-sdk sicompass-plugin)"
command -v jq >/dev/null || fail "jq not found (run inside nix develop)"

NAME="$(jq -r .name plugin.json)"
VERSION="$(jq -r .version plugin.json)"
ENTRY="$(jq -r '.entry // "plugin.wasm"' plugin.json)"
[ "$VERSION" != "null" ] || fail "plugin.json has no version"
if [ -n "${RELEASE_TAG:-}" ] && [ "$RELEASE_TAG" != "v$VERSION" ]; then
  fail "tag $RELEASE_TAG does not match plugin.json version $VERSION"
fi

say "building $NAME $VERSION for $TARGET"
CRATE="$(sed -n 's/^name *= *"\(.*\)"/\1/p' Cargo.toml | head -1 | tr - _)"
cargo build --release --target "$TARGET"
cp "target/$TARGET/release/$CRATE.wasm" "$ENTRY"

say "packing (audits the component's imports against plugin.json)"
rm -rf dist
"$TOOL" pack --dir . --out dist

SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT
KEY="$SCRATCH/signing.key"
(
  umask 077
  if [ "$DRY_RUN" = 1 ]; then
    say "dry run: signing with a throwaway key"
    "$TOOL" keygen --out "$KEY" >/dev/null 2>&1
  else
    [ -n "${PLUGIN_SIGNING_KEY:-}" ] || fail "PLUGIN_SIGNING_KEY is not set"
    printf '%s\n' "$PLUGIN_SIGNING_KEY" >"$KEY"
  fi
)
"$TOOL" sign --key "$KEY" --dist dist

SIGNED_WITH="$("$TOOL" pubkey --key "$KEY")"
if [ -n "${PLUGIN_PUBLIC_KEY:-}" ] && [ "$DRY_RUN" = 0 ]; then
  [ "$SIGNED_WITH" = "$PLUGIN_PUBLIC_KEY" ] \
    || fail "signed with $SIGNED_WITH, but the store lists $PLUGIN_PUBLIC_KEY"
fi

say "verifying as the Store will"
"$TOOL" verify --pubkey "$SIGNED_WITH" --dist dist

say "ready: $(cd dist && echo *)"
