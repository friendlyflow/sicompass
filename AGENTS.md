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

## Library Architecture

Libraries (`lib/`) communicate with `src/sicompass` through the `lib_provider` interface.

**Boundary rules (bidirectional):**
- `lib/` code must NEVER `#include` headers from `src/sicompass/`
- `src/sicompass/` must NEVER `#include` library-specific headers (e.g., `filebrowser.h`, `filebrowser_provider.h`, `settings_provider.h`)
- `src/sicompass/` may only include these shared infrastructure headers from `lib/`:
  - `provider_interface.h`, `provider_tags.h`, `platform.h` (from lib_provider)
  - `ffon.h` (from lib_ffon)
- Libraries may include: their own headers, other `lib/*/include/` public headers, and system/third-party headers

## Testing

- After implementing changes, always run relevant tests before finishing.
- C tests: `ninja -C build test` (all), or `build/tests/test_<module>` (specific).
- Bun tests (TypeScript providers): `bun test tests/lib_*/*.test.ts` (all), or `bun test tests/<module>/<name>.test.ts` (specific).
- When adding new code, write or update tests in `tests/`.
- If tests fail, fix the code — never leave a task with failing tests.

## GitHub Workflows

The `.github/workflows/build.yml` and `.github/workflows/release.yml` files share common build logic (dependencies, SDL3, accesskit-c, stb, Unity test framework setup). When making changes to one file, apply the same changes to the other to keep them in sync.
