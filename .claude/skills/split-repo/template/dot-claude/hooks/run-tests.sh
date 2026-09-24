#!/usr/bin/env bash
# Runs this repo's test suite after a Rust source edit. Non-blocking: it always
# exits 0, so Claude can keep iterating, and reports the output on failure.

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path')

if [[ "$FILE_PATH" =~ \.rs$ ]]; then
  OUTPUT=$(cd "$CLAUDE_PROJECT_DIR" && cargo test 2>&1)
  EXIT_CODE=$?
  if [ $EXIT_CODE -ne 0 ]; then
    echo "Rust tests failed after editing: $FILE_PATH"
    echo "$OUTPUT"
  fi
fi

exit 0
