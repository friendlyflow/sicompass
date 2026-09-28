---
paths:
  - "flake.nix"
---

# Editing flake.nix

- Do not reintroduce a bare `exec fish` in the flake's `shellHook`. It is guarded
  by `[ -t 0 ]` on purpose: without the guard it replaces the process for
  `nix develop -c <cmd>`, and the command silently never runs (exit 0, no output).
- The dev shell is platform-split. `aarch64-darwin` gets MoltenVK,
  `DYLD_FALLBACK_LIBRARY_PATH` and a `sysctl` job cap; Linux gets Wayland/X11,
  Mesa ICD discovery, `LD_LIBRARY_PATH` and xvfb. Anything added to the shared
  part of the `shellHook` has to hold on both. In particular, never reference a
  Linux-only package (`wayland`, `mesa`, `at-spi2-core`) outside the
  `lib.optionalString stdenv.hostPlatform.isLinux` branch: nixpkgs marks `wayland` bad on
  darwin, so a stray reference breaks `nix develop` at *eval* time on macOS,
  before anything is fetched.
- `x86_64-darwin` builds from a **second** nixpkgs input pinned to
  `nixpkgs-26.05-darwin`, selected by `nixpkgsInputFor`. Unstable (26.11)
  dropped Intel macOS and now *throws* on `import nixpkgs` for it, which would
  take down every eval of the flake on every platform, so it cannot simply be
  listed against the main input. That branch is supported until the end of 2026.
- `nix develop` overwrites `$SHELL` with its own store bash before the
  `shellHook` runs, so `$SHELL` is useless for detecting the user's shell there.
  The hook reads the OS user database instead (`getent`, then `/etc/passwd`,
  then `dscl` on macOS).
