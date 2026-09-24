#!/usr/bin/env bash
# Reminds Claude to write or update tests when a Rust source file is created.

INPUT=$(cat)
TOOL=$(echo "$INPUT" | jq -r '.tool_name')
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path')

[[ "$FILE_PATH" =~ \.rs$ ]] || exit 0
[[ "$FILE_PATH" =~ /tests/ ]] && exit 0
[[ "$FILE_PATH" =~ /.claude/ ]] && exit 0

if [[ "$TOOL" == "Write" ]]; then
  echo "REMINDER: You created a new source file: $FILE_PATH" >&2
  echo "Make sure to write or update corresponding tests (inline #[cfg(test)] module or tests/)." >&2
  exit 2
fi

exit 0
