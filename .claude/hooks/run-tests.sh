#!/bin/bash
# Runs the full test suite after source file edits.
# C/H files: build with ninja, then run all C test binaries directly
# TS files: bun test (all TypeScript provider tests)
#
# Note: meson test crashes (Python 3.13 / meson 1.10.1 asyncio bug), so
# test binaries are executed directly instead of via `ninja test`.

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path')

PROJECT_DIR="$CLAUDE_PROJECT_DIR"

if [[ "$FILE_PATH" =~ \.(c|h)$ ]]; then
  # Build first
  BUILD_OUTPUT=$(ninja -C "$PROJECT_DIR/build" 2>&1)
  BUILD_EXIT=$?
  if [ $BUILD_EXIT -ne 0 ]; then
    echo "Build failed after editing: $FILE_PATH" >&2
    echo "$BUILD_OUTPUT" >&2
    exit 2
  fi

  # Run all C test binaries directly (meson test crashes on Python 3.13)
  # LSAN_OPTIONS=detect_leaks=0 suppresses false-positive leaks from system libs
  # (libjson-c locale init, glibc, etc.) so only real Unity test failures are caught.
  FAILED=0
  FAILED_TESTS=""
  for f in "$PROJECT_DIR/build/tests/"*/test_*; do
    [ -f "$f" ] || continue
    TEST_OUT=$(LSAN_OPTIONS=detect_leaks=0 "$f" 2>/dev/null)
    TEST_EXIT=$?
    if [ $TEST_EXIT -ne 0 ]; then
      FAILED=1
      FAILED_TESTS="$FAILED_TESTS\n$f (exit $TEST_EXIT):\n$TEST_OUT"
    fi
  done

  if [ $FAILED -ne 0 ]; then
    echo "C tests failed after editing: $FILE_PATH" >&2
    printf "$FAILED_TESTS" >&2
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
