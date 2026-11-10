#!/usr/bin/env bash
# PreToolUse/Bash hook: snapshot ~/.config/sicompass-dev/settings.json before
# any command that can start a debug sicompass and rewrite it (tests, benches,
# dev runs). Release runs (`nix run`, `target/release/sicompass`) use the
# installed app's folder, which this no longer copies.
#
# Runs *before* the command, so the dev settings are always captured
# first. See snapshot-settings.sh for the copy chain.
set -u

INPUT=$(cat)

# Fail safe: if the command cannot be read, snapshot anyway.
if command -v jq >/dev/null 2>&1; then
  CMD=$(printf '%s' "$INPUT" | jq -r '.tool_input.command // ""' 2>/dev/null) || CMD=""
else
  CMD=""
fi

case "$CMD" in
  *"cargo test"* | *"cargo nextest"* | *"cargo bench"* | *"cargo run"* | \
  *"xvfb-run"* | *target/debug/sicompass* | "")
    exec "$(dirname "$0")/snapshot-settings.sh"
    ;;
esac

exit 0
