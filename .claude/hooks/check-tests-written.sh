#!/bin/bash
# Reminds Claude to write/update tests when source code is added or modified.
# - Write (new file): remind to create tests
# - Edit (handlers/provider): remind to add integration tests

INPUT=$(cat)
TOOL=$(echo "$INPUT" | jq -r '.tool_name')
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path')

# Only check source files
if [[ ! "$FILE_PATH" =~ \.(ts|rs)$ ]]; then
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

# New file creation: remind to write tests
if [[ "$TOOL" == "Write" ]]; then
  echo "REMINDER: You created a new source file: $FILE_PATH" >&2
  if [[ "$FILE_PATH" =~ \.rs$ ]]; then
    echo "Make sure to write or update corresponding tests (inline #[cfg(test)] module or integration tests)." >&2
  else
    echo "Make sure to write or update corresponding tests in tests/." >&2
  fi
  exit 2
fi

# Edits to handler/provider code: remind about integration tests
if [[ "$TOOL" == "Edit" ]]; then
  if [[ "$FILE_PATH" =~ src/sicompass-rs/src/handlers\.rs ]] || \
     [[ "$FILE_PATH" =~ src/sicompass-rs/src/provider\.rs ]] || \
     [[ "$FILE_PATH" =~ src/sicompass-rs/src/events\.rs ]] || \
     [[ "$FILE_PATH" =~ lib/lib_.*-rs/src/.*\.rs ]]; then
    echo "REMINDER: You edited $FILE_PATH" >&2
    echo "Consider adding/updating integration tests in src/sicompass-rs/tests/integration.rs for cross-provider or full-workflow behavior." >&2
    exit 2
  fi
fi

exit 0
