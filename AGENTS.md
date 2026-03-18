# Project Instructions

## Build System

This project uses **Meson** as the build system with **Ninja** as the backend.

## Build Commands

Always use ninja for building this project:

```bash
ninja -C build
```

To configure/reconfigure the build:

```bash
rm -rf build
meson setup build
```

Do not use `make` - this project uses Meson + Ninja, not CMake or Makefiles.

## Code Style

### Header Files

Always use `#pragma once` as the include guard in header files. Do not use traditional `#ifndef`/`#define`/`#endif` include guards.

## Plugin SDK (git submodule)

Public headers live in `sdk/include/` as a git submodule (`sicompass-plugin-sdk` repo). When modifying SDK headers (`provider_interface.h`, `provider_tags.h`, `platform.h`, `ffon.h`):

1. Edit files in `sdk/include/`
2. Commit and push inside the submodule: `cd sdk && git add -A && git commit && git push`
3. Update the submodule ref in sicompass: `cd .. && git add sdk && git commit`

## Library Architecture

Libraries (`lib/`) communicate with `src/sicompass` through the `lib_provider` interface.

**Boundary rules (bidirectional):**
- `lib/` code must NEVER `#include` headers from `src/sicompass/`
- `src/sicompass/` must NEVER `#include` library-specific headers (e.g., `filebrowser.h`, `filebrowser_provider.h`, `settings_provider.h`)
- `src/sicompass/` may only include these shared infrastructure headers from `sdk/include/`:
  - `provider_interface.h`, `provider_tags.h`, `platform.h`
  - `ffon.h`
- Libraries may include: their own headers, other `lib/*/include/` public headers, and system/third-party headers

## Testing

- After implementing changes, always run relevant tests before finishing.
- C unit tests: `build/tests/<module>/test_<name>` (specific), or run all via the test binaries in `build/tests/`.
- C integration tests: `build/tests/integration/test_integration` — headless end-to-end tests that link real handlers, providers (filebrowser, webbrowser, settings), and simulate key presses. Add integration tests for cross-provider or full-workflow behavior.
- Bun tests (TypeScript providers): `bun test tests/lib_*/*.test.ts` (all), or `bun test tests/<module>/<name>.test.ts` (specific).
- When adding new code, write or update tests in `tests/`.
- If tests fail, fix the code — never leave a task with failing tests.

## GitHub Workflows

The `.github/workflows/build.yml` and `.github/workflows/release.yml` files share common build logic (dependencies, SDL3, accesskit-c, stb, Unity test framework setup). When making changes to one file, apply the same changes to the other to keep them in sync.
