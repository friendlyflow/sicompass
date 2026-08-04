{
  description = "Sicompass Dev Flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [
        "aarch64-linux"
        "aarch64-darwin"
        "x86_64-linux"
        "x86_64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      nixpkgsFor = forAllSystems (system: import nixpkgs { inherit system; });

      # Single source of truth for the version. Reading it here means
      # `nix build` cannot drift from `cargo build` when the workspace version
      # is bumped for a release.
      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
    in
    {
      devShells = forAllSystems (system:
        let
          pkgs = nixpkgsFor.${system};
        in
        {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              # Rust toolchain
              cargo
              rustc
              rust-analyzer
              clippy
              rustfmt

              # WASM plugin guests. No extra Rust target is needed: nixpkgs'
              # rustc already ships std for wasm32-unknown-unknown, which is the
              # target guests use. (Deliberately not a wasip2 target — wasip2's
              # std declares wasi:* imports that the sicompass host links none
              # of, and a guest free of WASI is what makes its import section a
              # true capability set.)
              #
              # lld: nixpkgs strips rustc's bundled rust-lld, so the wasm link
              # step needs wasm-ld from here. Without it, `cargo build --target
              # wasm32-unknown-unknown` fails with "linker `lld` not found"
              # (`cargo check` is unaffected, which is easy to be fooled by).
              lld
              # wasm-tools: `wasm-tools component new` wraps a core module as a
              # component. No WASI adapter is involved, precisely because there
              # are no WASI imports to adapt.
              wasm-tools

              # Native libs required by Rust crates
              pkg-config
              sdl3
              freetype
              libwebp
              curl

              # cmake: several -sys crates drive a CMake build. sdl3-sys needs
              # it for the `bundled-sdl3` feature (which compiles the vendored
              # SDL 3.4.12 from source), and aws-lc-sys and libsqlite3-sys need
              # it unconditionally. Without it `cargo build --features
              # bundled-sdl3` dies in sdl3-sys' build script with
              # "is `cmake` not installed?".
              cmake

              # Vulkan (used via ash crate)
              spirv-tools
              vulkan-loader
              vulkan-headers
              glslang

              # Icon generation (scripts/gen-icons.sh). Not needed to build or
              # run sicompass, only to regenerate assets/icons/* from the two
              # master SVGs, which happens about once a year.
              #   librsvg   -> rsvg-convert, SVG to PNG at each size
              #   imagemagick -> magick, PNG touch-up and previews
              #   icoutils  -> icotool, the multi-resolution Windows .ico
              #   libicns   -> png2icns, the macOS .icns
              librsvg
              imagemagick
              icoutils
              libicns

              # graphify code-graph CLI is a uv-installed Python tool
              # (PyPI package `graphifyy`); uv bootstraps it in the shellHook.
              uv
            ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              # xvfb-run: lets the web browser provider run headed Chrome on an
              # invisible X11 display. Without it, chrome_via_xvfb() falls back
              # to launching Chrome on the real display and a window pops up.
              xvfb-run
              vulkan-volk
              vulkan-tools
              vulkan-validation-layers
              vulkan-extension-layer
              vulkan-tools-lunarg
              wayland
              wayland-scanner
              wayland-protocols
              libxkbcommon

              # SDL3's own build dependencies, needed only by the
              # `bundled-sdl3` feature, which compiles the vendored SDL from
              # source. SDL's CMake aborts configure with "could not find X11
              # or Wayland development libraries" unless it can see at least
              # one windowing backend, and it probes for the audio and DRM
              # backends the same way. The release build uses this feature, so
              # the dev shell has to be able to reproduce it. Mirrors the apt
              # list in `[dist.dependencies.apt]`.
              libx11
              libxext
              libxcursor
              libxi
              libxrandr
              libxscrnsaver
              libxfixes
              libxrender
              libxtst
              # xcb: SDL's bundled vulkan.h includes <xcb/xcb.h> for the
              # VK_USE_PLATFORM_XCB_KHR surface path.
              libxcb
              libdecor
              libGL
              libdrm
              mesa
              alsa-lib
              libpulseaudio

              # Accessibility (accesskit_unix)
              at-spi2-core
              dbus
              accerciser
            ];

            shellHook = with pkgs; ''
              # Rust stdlib source for rust-analyzer
              export RUST_SRC_PATH="${pkgs.rustc}/lib/rustlib/src/rust/library";

              # SDL3 + deps pkg-config / link path (needed by sdl3-rs / cargo build)
              export PKG_CONFIG_PATH="${sdl3}/lib/pkgconfig:${libxkbcommon.dev}/lib/pkgconfig:$PKG_CONFIG_PATH";
              export LIBRARY_PATH="${sdl3}/lib:${libxkbcommon}/lib:${wayland}/lib:$LIBRARY_PATH";

              # Library path for Vulkan and other runtime deps.
              #
              # Store paths only: LD_LIBRARY_PATH outranks the DT_RUNPATH Nix
              # bakes into its binaries, so a system lib dir here is resolved
              # first by *every* binary in the shell. On a distro whose glibc is
              # older than nixpkgs' (Mint 22.3 ships 2.39, nixpkgs-unstable is on
              # 2.42), adding /usr/lib/x86_64-linux-gnu bricks the shell: sh, rm
              # and uname all die with "version `GLIBC_2.42' not found".
              #
              # The system Mesa ICD does not need it. radeon_icd.json names its
              # driver relatively ("libvulkan_radeon.so"), but ldconfig indexes
              # that lib, so the loader's dlopen finds it via ld.so.cache.
              export LD_LIBRARY_PATH="${libwebp}/lib:${freetype}/lib:${vulkan-loader}/lib:${vulkan-validation-layers}/lib:${curl}/lib:${sdl3}/lib:${libxkbcommon}/lib:${wayland}/lib";
              export VULKAN_SDK="${vulkan-headers}";
              export VK_LAYER_PATH="${vulkan-validation-layers}/share/vulkan/explicit_layer.d";

              # Point the Vulkan loader at system drivers on non-NixOS distros.
              # On NixOS the drivers live in /run/opengl-driver and the loader
              # finds them on its own, so leave VK_ICD_FILENAMES unset there:
              # setting it to a missing path makes the loader report zero ICDs
              # and SDL fails with "Vulkan doesn't implement VK_KHR_surface".
              if [ -e /usr/share/vulkan/icd.d/radeon_icd.json ]; then
                export VK_ICD_FILENAMES="/usr/share/vulkan/icd.d/radeon_icd.json";
              fi

              # graphify: uv installs the `graphifyy` package's binaries into
              # ~/.local/bin. Put it on PATH and bootstrap the tool if missing
              # so `graphify` works out of the box in this shell.
              export PATH="$HOME/.local/bin:$PATH";
              if ! command -v graphify >/dev/null 2>&1; then
                uv tool install graphifyy >/dev/null 2>&1 || true;
              fi

              # Do NOT add -fuse-ld=lld for the host target here. It halves
              # linker memory, which is tempting on a small machine, but gcc
              # then invokes ld.lld directly and bypasses Nix's ld wrapper,
              # which is what injects the store paths into RUNPATH. The
              # binaries still link, so a plain `cargo build` looks fine, and
              # then every test binary dies at startup with
              #   error while loading shared libraries: libssl.so.3
              # because its RUNPATH is only the placeholder outputs/out/lib.
              # (lld is still in buildInputs above: the wasm guests invoke
              # wasm-ld directly, where no rpath injection is involved.)

              # Cap parallel cargo jobs by RAM as well as cores. A single rustc
              # on this workspace can hold ~1 GB (chromiumoxide is the worst,
              # and Cargo.toml pins it to opt-level 2 even in dev), so -j nproc
              # overcommits badly on a small machine while a browser and an
              # editor are also resident. One job per 2 GB, never above nproc.
              # A no-op on any machine with enough RAM to cover its cores.
              if [ -r /proc/meminfo ] && [ -z "$CARGO_BUILD_JOBS" ]; then
                _gb=$(awk '/MemTotal/{printf "%d", $2/1048576}' /proc/meminfo);
                _cap=$((_gb / 2));
                [ "$_cap" -lt 1 ] && _cap=1;
                if [ "$_cap" -lt "$(nproc)" ]; then
                  export CARGO_BUILD_JOBS="$_cap";
                fi
                unset _gb _cap;
              fi

              # Drop into fish for interactive shells only. `nix develop -c <cmd>`
              # and tooling (Claude Code's Bash tool, CI) get no tty; exec'ing
              # fish there would replace the process and silently discard the
              # command, which exits 0 with no output.
              if [ -t 0 ]; then
                exec fish
              fi
            '';
          };
        });

      # `nix build`, `nix run github:friendlyflow/sicompass`, and
      # `nix profile install`. This is the fourth Linux package format,
      # alongside the .deb, .rpm and AppImage that native-packages.yml builds.
      packages = forAllSystems (system:
        let
          pkgs = nixpkgsFor.${system};
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "sicompass";
            inherit version;
            src = ./.;

            # Cargo.lock has no git sources, so the lock file alone is enough
            # and there is no cargoHash to keep up to date.
            cargoLock.lockFile = ./Cargo.lock;

            # Only the app crate. The lib_* crates come in transitively.
            cargoBuildFlags = [ "-p" "sicompass" ];

            # The workspace suite wants a network and a display. It is run by
            # ci.yml instead, where both can be arranged.
            doCheck = false;

            nativeBuildInputs = with pkgs; [
              pkg-config
              # aws-lc-sys and libsqlite3-sys both drive a CMake build.
              cmake
              rustPlatform.bindgenHook
              makeWrapper
              copyDesktopItems
            ];

            buildInputs = with pkgs; [
              # System SDL3, not the `bundled-sdl3` feature: inside a Nix
              # build there is no reason to compile a vendored copy when the
              # real dependency can be declared.
              sdl3
              freetype
              libwebp
              curl
            ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              # openssl-sys is in the graph on Linux only (lettre and
              # async-native-tls). macOS uses Security.framework.
              openssl
              libxkbcommon
              wayland
              at-spi2-core
              dbus
            ];

            # No glslang and no fonts here: shaders and fonts are compiled
            # into the binary. `assets/` is the only runtime tree left.
            postInstall = ''
              mkdir -p $out/share/sicompass
              cp -r assets $out/share/sicompass/assets

              install -Dm644 assets/icons/sicompass.svg \
                $out/share/icons/hicolor/scalable/apps/sicompass.svg
              for s in 16 22 24 32 48 64 128 256 512; do
                install -Dm644 "assets/icons/''${s}x''${s}.png" \
                  "$out/share/icons/hicolor/''${s}x''${s}/apps/sicompass.png"
              done

              # The font licenses have to travel with the binary, since the
              # fonts themselves are inside it.
              install -Dm644 fonts/LICENSE-DejaVu.txt \
                $out/share/doc/sicompass/LICENSE-DejaVu.txt
              install -Dm644 fonts/LICENSE-NotoColorEmoji.txt \
                $out/share/doc/sicompass/LICENSE-NotoColorEmoji.txt
              install -Dm644 THIRD-PARTY-LICENSES.html \
                $out/share/doc/sicompass/THIRD-PARTY-LICENSES.html

              # SICOMPASS_RESOURCE_DIR is the first thing
              # `resources::resource_root()` checks, so the derivation states
              # where its assets are rather than relying on a layout guess.
              #
              # vulkan-loader on LD_LIBRARY_PATH is what lets
              # `ash::Entry::load()` dlopen libvulkan.so.1. Deliberately no
              # VK_ICD_FILENAMES: on NixOS the drivers live in
              # /run/opengl-driver and the loader finds them itself, and
              # pinning a path that does not exist makes it report zero ICDs.
              wrapProgram $out/bin/sicompass \
                --set SICOMPASS_RESOURCE_DIR $out/share/sicompass \
                --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath (with pkgs; [
                  vulkan-loader
                  libxkbcommon
                  wayland
                  sdl3
                ])}"
            '';

            desktopItems = [
              (pkgs.makeDesktopItem {
                name = "sicompass";
                desktopName = "Sicompass";
                genericName = "Keyboard Navigator";
                comment = "Use your whole computer from the keyboard, with no mouse needed";
                exec = "sicompass %F";
                icon = "sicompass";
                categories = [ "Utility" "Accessibility" ];
                keywords = [ "accessibility" "screenreader" "keyboard" "navigator" "a11y" ];
                startupWMClass = "sicompass";
                startupNotify = true;
              })
            ];

            meta = with pkgs.lib; {
              description = "Use your whole computer from the keyboard, with no mouse needed";
              homepage = "https://github.com/friendlyflow/sicompass";
              license = licenses.gpl3Only;
              mainProgram = "sicompass";
              platforms = platforms.unix;
            };
          };
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/sicompass";
          meta = self.packages.${system}.default.meta;
        };
      });
    };
}
