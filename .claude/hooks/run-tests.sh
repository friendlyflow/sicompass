#!/usr/bin/env bash
# Runs the relevant test suite after source file edits.
# RS files: cargo test (non-blocking, exit 0 so Claude can keep iterating)
# (TypeScript is gone: providers are Rust, third-party plugins are WASM.)
#
# The suite is the one of the repo that *owns the edited file*, not always this
# one. Work on the sibling repos (../desicompass, ../sicompass-ui,
# ../<name>-plugin-sicompass, see .claude/repos.json) is driven from this
# checkout, so an edit there fires this hook too, and running sicompass's suite
# for it would test the wrong code and still take minutes.

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path')

PROJECT_DIR="$CLAUDE_PROJECT_DIR"

if [[ "$FILE_PATH" =~ \.rs$ ]]; then
  REPO_DIR=$(git -C "$(dirname "$FILE_PATH")" rev-parse --show-toplevel 2>/dev/null)
  [ -n "$REPO_DIR" ] || REPO_DIR="$PROJECT_DIR"

  # The tests below can start sicompass and rewrite the production config, so
  # capture it first (same chain as the PreToolUse hook; a no-op if unchanged).
  "$(dirname "$0")/snapshot-settings.sh" >/dev/null 2>&1

  # Run the owning repo's tests (non-blocking: exit 0 so Claude can keep iterating)
  OUTPUT=$(cd "$REPO_DIR" && cargo test 2>&1)
  EXIT_CODE=$?
  if [ $EXIT_CODE -ne 0 ]; then
    echo "Rust tests failed in $REPO_DIR after editing: $FILE_PATH"
    echo "$OUTPUT"
  fi
fi

exit 0
