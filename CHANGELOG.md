# Changelog

cargo-dist parses this file and uses the matching version's section as the
GitHub Release body, so entries here are what users read on the download page.

## 0.1.9

### Downloads for Windows, macOS and Linux

Every platform now has a real installer. Previously only Windows had a package,
and it could not actually start.

- **Windows**: a PowerShell one-liner, or an `.msi` that adds a Start Menu
  entry and an application icon.
- **macOS**: a `.dmg` for Apple Silicon and one for Intel, each bundling
  MoltenVK so there is nothing else to install. Also reachable with
  `curl ... | sh`.
- **Linux**: `.deb`, `.rpm`, AppImage, a `curl ... | sh` installer, and
  `nix run github:friendlyflow/sicompass`.

### The published binary now starts

Releases up to and including 0.1.8 shipped an executable that read its shaders
and fonts from the working directory and shipped neither, so it exited at
startup unless launched from a source checkout.

- Shaders are compiled to SPIR-V and embedded in the binary.
- Fonts are embedded in the binary.
- The `assets/` tree is now found relative to the executable, so it works from
  an archive, an `.app` bundle, a `.deb`, an AppImage, the `.msi` and Nix
  without any wrapper script.

### macOS support

macOS was previously unreleasable. `ash::Entry::load()` looks for
`libvulkan.dylib`, which no Mac has, and the Vulkan instance was created
without the portability flags MoltenVK needs, so it would have found no GPU
even after loading.

- Vulkan is now loaded from the bundled MoltenVK, the LunarG SDK, or Homebrew,
  in that order.
- `VK_KHR_portability_enumeration` and `VK_KHR_portability_subset` are enabled
  where required.
- The app is ad-hoc signed. It is not yet notarized, so clear the quarantine
  flag once after downloading: `xattr -dr com.apple.quarantine
  /Applications/sicompass.app`

### Colour emoji now render

Emoji in chat and email content were silently invisible on every shipped
build. FreeType only decodes Noto Color Emoji's PNG bitmap strikes when it is
compiled with PNG support, and the release builds were falling back to a
vendored FreeType that is not, so glyphs came back zero-sized rather than as
an error. All three platforms now build against a PNG-capable FreeType, and
`sicompass --check` reports whether yours does.

### Changed

- The default monospace face is now DejaVu Sans Mono. The previous face was
  Microsoft Consolas, which cannot be redistributed, and which in practice
  covered only 600 codepoints with no box-drawing glyphs, so most of the
  interface was already being drawn from the DejaVu fallback.
- The application has an icon, on every platform.
- Windows release builds no longer open a console window behind the app.
- `sicompass --check` now reports where resources resolved to, whether colour
  emoji can render, and which Vulkan devices are visible, instead of checking
  for files that no longer exist on disk. It is the first thing to run when
  the app will not start. Set `SICOMPASS_CHECK_FILE` to also write the report
  to a file, which is the only way to read it on Windows, where the app has no
  console.
- `No suitable Vulkan GPU found` now says how many devices were enumerated,
  which distinguishes a driver problem from a capability problem.

### Fixed

- `bundled-sdl3` builds on Linux. The feature enabled `sdl3-sys/no-sdl-libc`
  on every platform, which un-defined `HAVE_STDIO_H` in SDL's build and broke
  the `<errno.h>` include in `SDL_iostream.c` under GCC. It is now applied only
  on Windows, where it is genuinely needed. CI builds the same way the release
  does as a result.
