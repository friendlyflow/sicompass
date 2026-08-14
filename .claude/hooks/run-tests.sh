#!/usr/bin/env bash
# Runs the relevant test suite after source file edits.
# RS files: cargo test (non-blocking, exit 0 so Claude can keep iterating)
# (TypeScript is gone: providers are Rust, third-party plugins are WASM.)

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path')

PROJECT_DIR="$CLAUDE_PROJECT_DIR"

if [[ "$FILE_PATH" =~ \.rs$ ]]; then
  # The tests below can start sicompass and rewrite the production config, so
  # capture it first (same chain as the PreToolUse hook; a no-op if unchanged).
  "$(dirname "$0")/snapshot-settings.sh" >/dev/null 2>&1

  # Run Rust workspace tests (non-blocking: exit 0 so Claude can keep iterating)
  OUTPUT=$(cd "$PROJECT_DIR" && cargo test 2>&1)
  EXIT_CODE=$?
  if [ $EXIT_CODE -ne 0 ]; then
    echo "Rust tests failed after editing: $FILE_PATH"
    echo "$OUTPUT"
  fi
fi

exit 0
