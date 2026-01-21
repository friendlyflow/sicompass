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
