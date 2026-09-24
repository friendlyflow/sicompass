#!/usr/bin/env bash
#
# Enforces the SDK boundary rule in CLAUDE.md: the `sicompass` app crate may
# not import a `lib_*` provider crate directly. Everything goes through
# `sicompass-sdk` plus the thin registration crate `sicompass-builtins`.
#
# The renderer's half of the rule (sicompass-ui may not link the heavy
# application crates either) moved with it to the sicompass-ui repo, which
# checks it with its own Stop hook, `check-no-heavy-deps.sh`.
#
# Tests may import concrete crates for mock injection, so `#[cfg(test)]` blocks
# and `tests/` trees are out of scope here — this only reads `src/**`.
set -uo pipefail

ROOT="${CLAUDE_PROJECT_DIR:-.}"
status=0

LIB_CRATES='filebrowser|settings|chatclient|emailclient|webbrowser|tutorial|store|text_editor|builtins|updater'

for dir in src/sicompass/src; do
  [ -d "$ROOT/$dir" ] || continue

  # `sicompass-builtins` and `sicompass-updater` are legitimate in the app
  # crate.
  pattern="^use sicompass_($(echo "$LIB_CRATES" | sed 's/|builtins|updater//'))::"
  hits=$(grep -rnE "$pattern" "$ROOT/$dir" 2>/dev/null)
  if [ -n "$hits" ]; then
    echo "SDK boundary violation in $dir: must not import lib crates directly." >&2
    echo "All communication goes through sicompass-sdk and sicompass-builtins." >&2
    echo "$hits" >&2
    status=2
  fi
done

exit $status
