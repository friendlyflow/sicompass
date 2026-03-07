#!/bin/bash
# Enforces library architecture boundaries:
# - lib/ must not include src/sicompass/ headers
# - src/sicompass/ must only include whitelisted shared infrastructure headers from lib/

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path')

# Only check .c and .h files
if [[ ! "$FILE_PATH" =~ \.(c|h)$ ]]; then
  exit 0
fi

# Resolve to relative path from project root
PROJECT_DIR="$CLAUDE_PROJECT_DIR"
REL_PATH="${FILE_PATH#"$PROJECT_DIR"/}"

# Whitelisted shared infrastructure headers that src/sicompass/ may include from lib/
ALLOWED_LIB_HEADERS="provider_interface\.h|provider_tags\.h|platform\.h|ffon\.h|settings_provider\.h"

# Known src/sicompass/ internal headers
SICOMPASS_HEADERS="main\.h|view\.h|provider\.h|text\.h|image\.h|rectangle\.h|caret\.h|checkmark\.h|accesskit_sdl\.h|programs\.h|unicode_search\.h|handlers\.h|render\.h|list\.h"

if [[ "$REL_PATH" == lib/* ]]; then
  # Library file: must not include any src/sicompass/ header
  VIOLATIONS=$(grep -nE '#include\s*[<"]('$SICOMPASS_HEADERS')[>"]' "$FILE_PATH" 2>/dev/null || true)
  if [[ -n "$VIOLATIONS" ]]; then
    echo "BOUNDARY VIOLATION: lib/ file includes src/sicompass/ header" >&2
    echo "File: $REL_PATH" >&2
    echo "$VIOLATIONS" >&2
    echo "" >&2
    echo "Libraries must not depend on src/sicompass/ internals." >&2
    echo "Use the lib_provider interface instead." >&2
    exit 2
  fi

elif [[ "$REL_PATH" == src/sicompass/* ]]; then
  # Sicompass file: check all #include lines for lib headers that are NOT whitelisted
  # Extract all included header filenames
  INCLUDES=$(grep -nE '#include\s*[<"]' "$FILE_PATH" 2>/dev/null || true)

  while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    # Extract the header filename
    HEADER=$(echo "$line" | sed -nE 's/.*#include\s*[<"]([^>"]+)[>"].*/\1/p')
    [[ -z "$HEADER" ]] && continue

    # Skip system/third-party headers (contain /)
    if [[ "$HEADER" == */* ]]; then
      continue
    fi

    # Skip src/sicompass/ own headers
    if echo "$HEADER" | grep -qE "^($SICOMPASS_HEADERS)$"; then
      continue
    fi

    # Skip whitelisted shared infrastructure headers
    if echo "$HEADER" | grep -qE "^($ALLOWED_LIB_HEADERS)$"; then
      continue
    fi

    # Skip standard library headers
    if [[ "$HEADER" =~ ^(std|SDL|stb_|vulkan|json-c|ctype|errno|assert|signal|unistd|dirent|sys/|fcntl|spawn|limits|math|float|time|locale) ]]; then
      continue
    fi

    # Any remaining header from lib/ is a violation
    # Check if this header exists under lib/
    if find "$PROJECT_DIR/lib" -name "$HEADER" -print -quit 2>/dev/null | grep -q .; then
      echo "BOUNDARY VIOLATION: src/sicompass/ includes non-whitelisted library header" >&2
      echo "File: $REL_PATH" >&2
      echo "$line" >&2
      echo "" >&2
      echo "src/sicompass/ may only include these lib headers:" >&2
      echo "  provider_interface.h, provider_tags.h, platform.h (lib_provider)" >&2
      echo "  ffon.h (lib_ffon)" >&2
      exit 2
    fi
  done <<< "$INCLUDES"
fi

exit 0
