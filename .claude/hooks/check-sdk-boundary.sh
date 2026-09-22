#!/usr/bin/env bash
#
# Enforces the two crate-boundary rules in CLAUDE.md.
#
# 1. Neither `sicompass` nor `sicompass-ui` may import a `lib_*` provider crate
#    directly. Everything goes through `sicompass-sdk` plus the thin
#    registration crate `sicompass-builtins`.
#
# 2. `sicompass-ui` may not import the heavy application dependencies. It is
#    shared with the login greeter, and the whole point of splitting it out was
#    that a login screen does not link wasmtime, a bundled SQLite, a
#    headless-Chromium driver or a TLS stack. Where the renderer needs
#    something only the application can answer, it asks through
#    `registry::HostHooks` or `http::register_body_fetcher`.
#
# Tests may import concrete crates for mock injection, so `#[cfg(test)]` blocks
# and `tests/` trees are out of scope here — this only reads `src/**`.
set -uo pipefail

ROOT="${CLAUDE_PROJECT_DIR:-.}"
status=0

LIB_CRATES='filebrowser|settings|chatclient|emailclient|webbrowser|tutorial|remote|sales_demo|shell|terminal|claude|gitclient|notes|project_management|payments|text_editor|builtins|updater'

for dir in src/sicompass/src src/sicompass-ui/src; do
  [ -d "$ROOT/$dir" ] || continue

  # Rule 1. `sicompass-builtins` and `sicompass-updater` are legitimate in the
  # app crate, so they are only forbidden in the UI crate (rule 2 below).
  pattern="^use sicompass_(${LIB_CRATES})::"
  if [ "$dir" = "src/sicompass/src" ]; then
    pattern="^use sicompass_($(echo "$LIB_CRATES" | sed 's/|builtins|updater//'))::"
  fi
  hits=$(grep -rnE "$pattern" "$ROOT/$dir" 2>/dev/null)
  if [ -n "$hits" ]; then
    echo "SDK boundary violation in $dir: must not import lib crates directly." >&2
    echo "All communication goes through sicompass-sdk and sicompass-builtins." >&2
    echo "$hits" >&2
    status=2
  fi
done

# Rule 2.
if [ -d "$ROOT/src/sicompass-ui/src" ]; then
  hits=$(grep -rnE '^use (wasmtime|reqwest|sicompass_builtins|sicompass_updater)\b|[^a-z_](wasmtime|reqwest|sicompass_builtins|sicompass_updater)::' \
    "$ROOT/src/sicompass-ui/src" 2>/dev/null | grep -v '^\s*//' | grep -vE '//[^"]*\b(wasmtime|reqwest|sicompass_builtins|sicompass_updater)\b')
  if [ -n "$hits" ]; then
    echo "sicompass-ui must not depend on the application's heavy crates." >&2
    echo "It is shared with the login greeter; use registry::HostHooks or" >&2
    echo "http::register_body_fetcher instead. Offending references:" >&2
    echo "$hits" >&2
    status=2
  fi
fi

exit $status
