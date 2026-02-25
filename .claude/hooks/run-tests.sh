#!/bin/bash
# Runs the full test suite after source file edits.
# C/H files: ninja -C build test (build + all C tests)
# TS files: bun test (all TypeScript provider tests)

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path')

PROJECT_DIR="$CLAUDE_PROJECT_DIR"

if [[ "$FILE_PATH" =~ \.(c|h)$ ]]; then
  # Build and run all C tests
  OUTPUT=$(ninja -C "$PROJECT_DIR/build" test 2>&1)
  EXIT_CODE=$?
  if [ $EXIT_CODE -ne 0 ]; then
    echo "C tests failed after editing: $FILE_PATH" >&2
    echo "$OUTPUT" >&2
    exit 2
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
