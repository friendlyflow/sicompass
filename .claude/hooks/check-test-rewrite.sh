#!/bin/bash
# Blocks removing or weakening test assertions/test cases without user confirmation.
# Fires on Edit/Write to test files; checks git diff for removed assertion lines.

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path')

# Only check test files
if [[ ! "$FILE_PATH" =~ test_ ]] && \
   [[ ! "$FILE_PATH" =~ \.test\. ]] && \
   [[ ! "$FILE_PATH" =~ /tests/ ]]; then
  exit 0
fi

# Look for removed assertion/test-function lines
REMOVED=$(git -C "$CLAUDE_PROJECT_DIR" diff HEAD -- "$FILE_PATH" 2>/dev/null \
  | grep '^-' | grep -v '^---' \
  | grep -E 'assert[_!]?|#\[test\]|fn test_|expect\(|\.test\(|it\(')

if [ -n "$REMOVED" ]; then
  echo "TEST REWRITE GUARD: assertions or test cases were removed in: $FILE_PATH" >&2
  echo "" >&2
  echo "Removed lines:" >&2
  echo "$REMOVED" >&2
  echo "" >&2
  echo "Rule: never remove or weaken assertions to make a test pass — fix the code instead." >&2
  echo "If the test itself is wrong, ask the user before modifying it." >&2
  exit 2
fi

exit 0
