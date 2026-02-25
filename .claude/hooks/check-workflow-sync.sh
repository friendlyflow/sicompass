#!/bin/bash
# Reminds to keep build.yml and release.yml in sync when either is edited.

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path')

PROJECT_DIR="$CLAUDE_PROJECT_DIR"
REL_PATH="${FILE_PATH#"$PROJECT_DIR"/}"

case "$REL_PATH" in
  .github/workflows/build.yml)
    echo "REMINDER: build.yml and release.yml share common build logic." >&2
    echo "Check if this change also needs to be applied to .github/workflows/release.yml" >&2
    exit 2
    ;;
  .github/workflows/release.yml)
    echo "REMINDER: release.yml and build.yml share common build logic." >&2
    echo "Check if this change also needs to be applied to .github/workflows/build.yml" >&2
    exit 2
    ;;
esac

exit 0
