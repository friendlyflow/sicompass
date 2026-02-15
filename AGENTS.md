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

When implementing a library (except `lib_provider` and `lib_ffon`), it must always use the provider interface to interact with Sicompass. Libraries should not directly depend on or call into Sicompass internals.

## GitHub Workflows

The `.github/workflows/build.yml` and `.github/workflows/release.yml` files share common build logic (dependencies, SDL3, accesskit-c, stb, Unity test framework setup). When making changes to one file, apply the same changes to the other to keep them in sync.
