#!/bin/bash
# Runs the relevant test suite after source file edits.
# RS files: cargo test (non-blocking, exit 0 so Claude can keep iterating)
# TS files: bun test (blocking)

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path')

PROJECT_DIR="$CLAUDE_PROJECT_DIR"

if [[ "$FILE_PATH" =~ \.rs$ ]]; then
  # Run Rust workspace tests (non-blocking: exit 0 so Claude can keep iterating)
  OUTPUT=$(cd "$PROJECT_DIR" && cargo test 2>&1)
  EXIT_CODE=$?
  if [ $EXIT_CODE -ne 0 ]; then
    echo "Rust tests failed after editing: $FILE_PATH"
    echo "$OUTPUT"
  fi

elif [[ "$FILE_PATH" =~ \.ts$ ]]; then
  # Run all bun tests
  OUTPUT=$(cd "$PROJECT_DIR" && bun test tests/lib_*/*.test.ts 2>&1)
  EXIT_CODE=$?
  if [ $EXIT_CODE -ne 0 ]; then
    echo "Bun tests failed after editing: $FILE_PATH" >&2
    echo "$OUTPUT" >&2
    exit 2
  fi
fi

exit 0
