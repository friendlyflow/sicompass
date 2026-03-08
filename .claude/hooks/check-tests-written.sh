#!/bin/bash
# Reminds Claude to write/update tests when new source code is added.
# Triggers on new .c, .h, or .ts files (not test files themselves).

INPUT=$(cat)
TOOL=$(echo "$INPUT" | jq -r '.tool_name')
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path')

# Only check Write tool (new file creation)
if [[ "$TOOL" != "Write" ]]; then
  exit 0
fi

# Only check source files
if [[ ! "$FILE_PATH" =~ \.(c|h|ts)$ ]]; then
  exit 0
fi

# Skip if the file itself is a test
if [[ "$FILE_PATH" =~ test_ ]] || [[ "$FILE_PATH" =~ \.test\. ]] || [[ "$FILE_PATH" =~ /tests/ ]]; then
  exit 0
fi

# Skip hook scripts and config files
if [[ "$FILE_PATH" =~ /.claude/ ]]; then
  exit 0
fi

echo "REMINDER: You created a new source file: $FILE_PATH" >&2
echo "Make sure to write or update corresponding tests in tests/." >&2
exit 2
