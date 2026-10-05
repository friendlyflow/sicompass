#!/usr/bin/env bash
# Build this plugin, pack it, sign it and verify it the way the sicompass Store
# will. The release workflow runs exactly this; so can you, before tagging:
#
#   nix develop -c ./scripts/release-plugin.sh --dry-run
#
# A plugin is a program, built once per platform. The workflow builds each
# platform on its own runner (`build <target>`), then packs them all here
# (`pack`). Run without a command, it builds this computer's platform and packs
# that alone, which is the local check before a tag.
#
#   release-plugin.sh build <target>   build/<target>/<entry>[.exe]
#   release-plugin.sh pack [--dry-run] every build/<target>/ into dist/
#   release-plugin.sh [--dry-run]      both, for this computer's platform
#
# Leaves dist/ with the files a GitHub release carries, under the fixed names
# the Store downloads (releases/latest/download/<file>):
#   plugin-<target>.tar.gz (one per platform)  release.json  release.json.sig
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
TOOL="${SICOMPASS_PLUGIN:-sicompass-plugin}"

fail() { printf 'FAIL %s\n' "$*" >&2; exit 1; }
say()  { printf '==> %s\n' "$*"; }

command -v jq >/dev/null || fail "jq not found (run inside nix develop)"

NAME="$(jq -r .name plugin.json)"
VERSION="$(jq -r .version plugin.json)"
ENTRY="$(jq -r .entry plugin.json)"
[ "$VERSION" != "null" ] || fail "plugin.json has no version"
[ "$(jq -r .type plugin.json)" = "process" ] || fail "plugin.json must say \"type\": \"process\""
if [ -n "${RELEASE_TAG:-}" ] && [ "$RELEASE_TAG" != "v$VERSION" ]; then
  fail "tag $RELEASE_TAG does not match plugin.json version $VERSION"
fi
BIN="$(sed -n 's/^name *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"

# The platform this computer's sicompass runs plugins for.
host_target() {
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) echo x86_64-unknown-linux-musl ;;
    Linux-aarch64|Linux-arm64) echo aarch64-unknown-linux-musl ;;
    Darwin-arm64) echo aarch64-apple-darwin ;;
    Darwin-x86_64) echo x86_64-apple-darwin ;;
    MINGW*|MSYS*|CYGWIN*) echo x86_64-pc-windows-msvc ;;
    *) fail "no plugin target for $(uname -s) $(uname -m)" ;;
  esac
}

build() {
  local target="$1" exe=""
  case "$target" in *-windows-*) exe=".exe" ;; esac
  say "building $NAME $VERSION for $target"
  cargo build --release --target "$target" --bin "$BIN"
  mkdir -p "build/$target"
  cp "target/$target/release/$BIN$exe" "build/$target/$ENTRY$exe"
}

pack() {
  local dry_run="$1"
  command -v "$TOOL" >/dev/null || fail "$TOOL not found (cargo install --git https://github.com/friendlyflow/sicompass-plugin-sdk sicompass-plugin)"
  local bins=() dir target exe
  for dir in build/*/; do
    target="$(basename "$dir")"
    exe="$ENTRY"
    case "$target" in *-windows-*) exe="$ENTRY.exe" ;; esac
    [ -f "$dir$exe" ] || fail "$dir has no $exe"
    bins+=(--bin "$target=$dir$exe")
  done
  [ "${#bins[@]}" -gt 0 ] || fail "nothing built: run \`$0 build <target>\` first"

  say "packing $(( ${#bins[@]} / 2 )) platform(s)"
  rm -rf dist
  "$TOOL" pack --dir . --out dist "${bins[@]}"

  local scratch key
  scratch="$(mktemp -d)"
  # Expanded now: the variable is gone by the time the shell exits.
  trap "rm -rf '$scratch'" EXIT
  key="$scratch/signing.key"
  (
    umask 077
    if [ "$dry_run" = 1 ]; then
      say "dry run: signing with a throwaway key"
      "$TOOL" keygen --out "$key" >/dev/null 2>&1
    else
      [ -n "${PLUGIN_SIGNING_KEY:-}" ] || fail "PLUGIN_SIGNING_KEY is not set"
      printf '%s\n' "$PLUGIN_SIGNING_KEY" >"$key"
    fi
  )
  "$TOOL" sign --key "$key" --dist dist

  local signed_with
  signed_with="$("$TOOL" pubkey --key "$key")"
  if [ -n "${PLUGIN_PUBLIC_KEY:-}" ] && [ "$dry_run" = 0 ]; then
    [ "$signed_with" = "$PLUGIN_PUBLIC_KEY" ] \
      || fail "signed with $signed_with, but the store lists $PLUGIN_PUBLIC_KEY"
  fi

  say "verifying as the Store will"
  "$TOOL" verify --pubkey "$signed_with" --dist dist
  say "ready: $(cd dist && echo *)"
}

case "${1:-}" in
  build)
    [ -n "${2:-}" ] || fail "usage: $0 build <target>"
    build "$2"
    ;;
  pack)
    pack "$([ "${2:-}" = "--dry-run" ] && echo 1 || echo 0)"
    ;;
  ""|--dry-run)
    rm -rf build
    build "$(host_target)"
    pack "$([ "${1:-}" = "--dry-run" ] && echo 1 || echo 0)"
    ;;
  *)
    fail "unknown command $1 (build <target>, pack [--dry-run], or --dry-run)"
    ;;
esac
