{
  description = "Sicompass Dev Flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    # Intel macOS only. nixpkgs 26.11 dropped x86_64-darwin outright, and it
    # does not merely stop building: `import nixpkgs { system =
    # "x86_64-darwin"; }` throws, so a single-input flake that lists the system
    # fails to evaluate on *every* platform, not just that one. 26.05 is the
    # last branch carrying it, and it gets security fixes until the end of
    # 2026. Retire this input, and the system below, when that runs out.
    nixpkgs-x86-darwin.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";
  };

  outputs = { self, nixpkgs, nixpkgs-x86-darwin }:
    let
      supportedSystems = [
        "aarch64-linux"
        "aarch64-darwin"
        "x86_64-linux"
        "x86_64-darwin"
      ];

      # Which nixpkgs a given system is built from. Everything tracks unstable
      # except Intel macOS, which unstable no longer has, per the input note.
      nixpkgsInputFor = system:
        if system == "x86_64-darwin" then nixpkgs-x86-darwin else nixpkgs;

      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      nixpkgsFor = forAllSystems (system:
        import (nixpkgsInputFor system) { inherit system; });

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

              # git: the sicompass-gitclient provider shells out to it rather
              # than linking libgit2, so it is a runtime dependency of that
              # provider and a test dependency of its crate. Pinned here so
              # `nix develop -c cargo test` does not silently depend on
              # whatever git happens to be on the contributor's PATH.
              git

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
            ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
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
            ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              # MoltenVK is the only Vulkan driver on macOS: it implements
              # Vulkan on top of Metal, and vulkan-loader enumerates zero ICDs
              # without it. Nothing on the Linux list has a macOS counterpart
              # to add here, because SDL and accesskit both go through Cocoa:
              # no Wayland, no X11, no xkb, no D-Bus, no Mesa.
              moltenvk
            ];

            # The hook is split three ways because macOS shares almost none of
            # the Linux graphics stack. `wayland` does not merely fail to
            # build on darwin, it fails to *evaluate* (nixpkgs marks
            # aarch64-darwin in its meta.badPlatforms), so an unconditional
            # reference to it here took down `nix develop` on macOS at eval
            # time, before any package was fetched.
            shellHook = with pkgs; ''
              # Rust stdlib source for rust-analyzer
              export RUST_SRC_PATH="${pkgs.rustc}/lib/rustlib/src/rust/library";

              # SDL3 pkg-config / link path (needed by sdl3-rs / cargo build)
              export PKG_CONFIG_PATH="${sdl3}/lib/pkgconfig:$PKG_CONFIG_PATH";
              export VULKAN_SDK="${vulkan-headers}";
            ''
            + lib.optionalString stdenv.hostPlatform.isLinux ''
              export PKG_CONFIG_PATH="${libxkbcommon.dev}/lib/pkgconfig:$PKG_CONFIG_PATH";
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
              # The system Mesa ICDs cannot make up for it either, see the
              # VK_ICD_FILENAMES block below.
              # curl.out, not curl: curl's *default* output is `bin`, which
              # holds no lib directory at all, so a bare ${curl}/lib here was
              # a path that has never existed.
              export LD_LIBRARY_PATH="${libwebp}/lib:${freetype}/lib:${vulkan-loader}/lib:${vulkan-validation-layers}/lib:${curl.out}/lib:${sdl3}/lib:${libxkbcommon}/lib:${wayland}/lib";
              export VK_LAYER_PATH="${vulkan-validation-layers}/share/vulkan/explicit_layer.d";

              # Vulkan ICD discovery on non-NixOS distros.
              #
              # /usr/share/vulkan/icd.d is no use to us. Every manifest there
              # names its driver relatively ("libvulkan_intel.so",
              # "libGLX_nvidia.so.0"), and nixpkgs' glibc ships no
              # ld.so.cache, so the loader's dlopen has nothing but
              # LD_LIBRARY_PATH to search and resolves none of them. The app
              # then dies with SDL's "Vulkan doesn't implement the
              # VK_KHR_surface extension", which is what zero usable ICDs
              # looks like from the outside. Symlinking the drivers onto the
              # path does not rescue it: they fail on their own dependencies
              # (libdrm.so.2, libLLVM.so.20.1) for the same reason. Adding
              # /usr/lib/x86_64-linux-gnu would resolve every one of them and
              # brick the shell, per the note above.
              #
              # nixpkgs' Mesa carries absolute store paths in its manifests,
              # so Intel, AMD (radv) and the llvmpipe software fallback all
              # load with no system library involved. Prefer it.
              #
              # Mesa also ships nouveau, but that does not amount to NVIDIA
              # support: nouveau is the open reimplementation, and on a
              # machine running the proprietary driver it is blacklisted and
              # never binds the card. NVIDIA is handled separately below.
              #
              # On NixOS the drivers live in /run/opengl-driver and the loader
              # finds them unaided, so leave VK_ICD_FILENAMES unset there:
              # pointing it at a missing path makes the loader report zero
              # ICDs and produces exactly the same SDL failure.
              if [ ! -d /run/opengl-driver ]; then
                _icd=$(ls ${mesa}/share/vulkan/icd.d/*.json 2>/dev/null | tr '\n' ':' | sed 's/:$//');

                # Proprietary NVIDIA is the one driver nixpkgs cannot stand in
                # for: it has to match the running kernel module, so it can
                # only come from the host. Unlike Mesa it has no dependency
                # fan-out beyond libc, so a symlink farm of just libGLX_nvidia
                # and libnvidia-* is enough for dlopen to resolve it without
                # putting the system glibc on the path. Listed ahead of Mesa
                # so it wins on a machine that has both.
                if [ -e /usr/share/vulkan/icd.d/nvidia_icd.json ]; then
                  _farm="$HOME/.cache/sicompass/vk-nvidia";
                  mkdir -p "$_farm";
                  for _lib in /usr/lib/x86_64-linux-gnu/libGLX_nvidia.so.* \
                              /usr/lib/x86_64-linux-gnu/libnvidia-*.so.*; do
                    [ -e "$_lib" ] && ln -sfn "$_lib" "$_farm/$(basename "$_lib")";
                  done
                  export LD_LIBRARY_PATH="$_farm:$LD_LIBRARY_PATH";
                  _icd="/usr/share/vulkan/icd.d/nvidia_icd.json:$_icd";
                  unset _farm _lib;
                fi

                [ -n "$_icd" ] && export VK_ICD_FILENAMES="$_icd";
                unset _icd;
              fi
            ''
            + lib.optionalString stdenv.hostPlatform.isDarwin ''
              export LIBRARY_PATH="${sdl3}/lib:$LIBRARY_PATH";

              # dyld, not ld.so. Nix's darwin linker bakes an absolute store
              # path into each dylib's install name, so anything cargo *links*
              # resolves with no search path at all. This is here for what the
              # app dlopens instead: `ash::Entry::load()` asks for
              # libvulkan.1.dylib by bare name, and MoltenVK is loaded in turn
              # by the loader, so neither is reachable without it.
              #
              # FALLBACK rather than DYLD_LIBRARY_PATH: the fallback list is
              # consulted last, so it cannot shadow a system framework the way
              # the Linux LD_LIBRARY_PATH note warns about.
              #
              # This variable cannot be relied on, and SDL_VULKAN_LIBRARY below
              # is what actually carries the day. System Integrity Protection
              # strips every DYLD_* variable across an exec of a protected
              # binary, and /bin is protected, so the `exec "$_sh"` at the end
              # of this hook destroys it for anyone whose login shell is
              # /bin/zsh, which is the macOS default. It survives only for
              # `nix develop -c <cmd>`, which does not exec a login shell.
              # Kept for exactly that case, and for libraries other than the
              # Vulkan pair.
              export DYLD_FALLBACK_LIBRARY_PATH="${vulkan-loader}/lib:${moltenvk}/lib:${sdl3}/lib:${freetype}/lib:${libwebp}/lib:${curl.out}/lib:$HOME/lib:/usr/local/lib:/usr/lib";

              # The SIP-proof half of the above: an ordinary variable name, so
              # it survives the exec into the user's shell. SDL3 reads it for
              # its own loader (SDL_HINT_VULKAN_LIBRARY) and sicompass reads
              # the same name in `load_vulkan_entry`, so one value steers both.
              # Point it at MoltenVK directly rather than the loader, since
              # MoltenVK exports the Vulkan entry points itself.
              export SDL_VULKAN_LIBRARY="${moltenvk}/lib/libMoltenVK.dylib";

              # One ICD, always present, and its manifest carries an absolute
              # store path. So unlike the Linux branch there is nothing to
              # probe for and no symlink farm to build: point at it and stop.
              export VK_ICD_FILENAMES="${moltenvk}/share/vulkan/icd.d/MoltenVK_icd.json";
            ''
            + ''
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
              #
              # The probe is the platform-specific part: macOS has neither
              # /proc/meminfo nor nproc, so it answers the same two questions
              # through sysctl. The cap itself is shared.
              if [ -z "$CARGO_BUILD_JOBS" ]; then
                if [ -r /proc/meminfo ]; then
                  _gb=$(awk '/MemTotal/{printf "%d", $2/1048576}' /proc/meminfo);
                  _cores=$(nproc);
                elif command -v sysctl >/dev/null 2>&1; then
                  _gb=$(( $(sysctl -n hw.memsize) / 1073741824 ));
                  _cores=$(sysctl -n hw.ncpu);
                fi
                if [ -n "$_gb" ]; then
                  _cap=$((_gb / 2));
                  [ "$_cap" -lt 1 ] && _cap=1;
                  if [ "$_cap" -lt "$_cores" ]; then
                    export CARGO_BUILD_JOBS="$_cap";
                  fi
                  unset _cap;
                fi
                unset _gb _cores;
              fi

              # Hand interactive sessions to whatever shell the user actually
              # uses. `nix develop` always starts bash, which is correct for
              # `nix develop -c <cmd>` but not what a person wants to be typing
              # into.
              #
              # The user's login shell rather than a hardcoded name: this used
              # to be a bare `exec fish`, which drops any machine without fish
              # installed straight into "fish: command not found" the moment
              # `nix develop` finishes, with no shell left to type into. fish
              # is not in buildInputs and deliberately still is not, because
              # the point is to honour the user's choice, not to ship a second
              # one. Set SICOMPASS_DEV_SHELL to override; set it to `bash` to
              # stay in the bash that nix develop provides.
              #
              # Do NOT reach for $SHELL here, however obvious it looks: `nix
              # develop` overwrites it with its own store bash before this hook
              # runs, so it reports bash on every machine and this block
              # silently never fires. The login shell has to come from the OS
              # user database, and that is the platform-specific part, so it is
              # asked three ways and the first hit wins.
              #
              # The [ -t 0 ] guard is load-bearing and must stay. `nix develop
              # -c <cmd>` and tooling (Claude Code's Bash tool, CI) get no tty;
              # exec'ing a shell there replaces the process and silently
              # discards the command, which exits 0 with no output.
              #
              # Note the absence of a login flag. `exec "$_sh"` starts a
              # non-login shell on purpose: on macOS a login shell sources
              # /etc/zprofile, which runs path_helper, which reorders PATH to
              # put /usr/bin ahead of everything and would bury this shell's
              # toolchain behind the system one.
              if [ -t 0 ]; then
                _sh="$SICOMPASS_DEV_SHELL";
                _me=$(id -un);

                # getent is glibc's nsswitch front end, so it is the only one
                # of the three that also answers for LDAP/SSSD accounts with
                # no local passwd line. Linux-only, and not reliably on PATH
                # inside a nix shell (it lives in glibc's `bin` output, which
                # is not a build input here), hence the plain-file read next.
                if [ -z "$_sh" ] && command -v getent >/dev/null 2>&1; then
                  _sh=$(getent passwd "$_me" 2>/dev/null | cut -d: -f7);
                fi

                # /etc/passwd needs no binary at all, which is what makes it
                # the dependable path on Linux, NixOS included: NixOS
                # generates a real passwd file for local users. Harmlessly
                # empty on macOS, where this file lists only system accounts.
                if [ -z "$_sh" ] && [ -r /etc/passwd ]; then
                  _sh=$(awk -F: -v u="$_me" '$1 == u { print $7 }' /etc/passwd);
                fi

                # macOS keeps local accounts in Directory Service instead.
                if [ -z "$_sh" ] && command -v dscl >/dev/null 2>&1; then
                  _sh=$(dscl . -read "/Users/$_me" UserShell 2>/dev/null \
                        | sed 's/^UserShell: *//');
                fi
                unset _me;
                case "''${_sh##*/}" in
                  # Already in bash, and nix's bash is set up for this shell.
                  bash | "") ;;
                  *) command -v "$_sh" >/dev/null 2>&1 && exec "$_sh" ;;
                esac
                unset _sh;
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
            ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
              # .desktop files are a freedesktop concept. macOS discovers apps
              # through an .app bundle's Info.plist instead, which is built by
              # the packaging pipeline in docs/releasing.md, not here.
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
            ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
              # openssl-sys is in the graph on Linux only (lettre and
              # async-native-tls). macOS uses Security.framework.
              openssl
              libxkbcommon
              wayland
              at-spi2-core
              dbus
            ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              # Vulkan-on-Metal. Same reason as the dev shell: without an ICD
              # the loader enumerates no devices and SDL reports the missing
              # VK_KHR_surface extension.
              moltenvk
            ];

            # No glslang, no fonts and no assets here: shaders, fonts and every
            # provider asset are compiled into the binary, so there is no runtime
            # tree to install. Only the icons and the .desktop item below, which
            # the desktop environment reads rather than the app.
            postInstall = ''
              # The font licenses have to travel with the binary, since the
              # fonts themselves are inside it.
              install -Dm644 fonts/LICENSE-DejaVu.txt \
                $out/share/doc/sicompass/LICENSE-DejaVu.txt
              install -Dm644 fonts/LICENSE-NotoColorEmoji.txt \
                $out/share/doc/sicompass/LICENSE-NotoColorEmoji.txt
              install -Dm644 THIRD-PARTY-LICENSES.html \
                $out/share/doc/sicompass/THIRD-PARTY-LICENSES.html
            ''
            + pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
              # The hicolor theme is where freedesktop desktops look for an
              # app icon, and it pairs with the .desktop item below. macOS
              # reads the icon out of the .app bundle instead, so this tree
              # would be dead weight there.
              install -Dm644 assets/icons/sicompass.svg \
                $out/share/icons/hicolor/scalable/apps/sicompass.svg
              for s in 16 22 24 32 48 64 128 256 512; do
                install -Dm644 "assets/icons/''${s}x''${s}.png" \
                  "$out/share/icons/hicolor/''${s}x''${s}/apps/sicompass.png"
              done

              # vulkan-loader on LD_LIBRARY_PATH is what lets
              # `ash::Entry::load()` dlopen libvulkan.so.1. Deliberately no
              # VK_ICD_FILENAMES: on NixOS the drivers live in
              # /run/opengl-driver and the loader finds them itself, and
              # pinning a path that does not exist makes it report zero ICDs.
              # xvfb-run on PATH is what lets the web browser provider run
              # Chrome headed on an invisible X11 display. Without it the
              # provider falls back to Chrome's own headless mode, which some
              # sites block. Same reason the dev shell carries it, and the same
              # reason the .deb and .rpm depend on xvfb.
              wrapProgram $out/bin/sicompass \
                --prefix PATH : "${pkgs.lib.makeBinPath (with pkgs; [ xvfb-run ])}" \
                --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath (with pkgs; [
                  vulkan-loader
                  libxkbcommon
                  wayland
                  sdl3
                ])}"
            ''
            + pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isDarwin ''
              # Same three jobs as the Linux wrapper, minus the one that has no
              # macOS meaning. dyld reads DYLD_FALLBACK_LIBRARY_PATH, not
              # LD_LIBRARY_PATH, and again this is only for what the app
              # dlopens: libvulkan.1.dylib by bare name.
              #
              # VK_ICD_FILENAMES *is* set here, unlike on Linux, because the
              # objection there does not apply: MoltenVK is a store path this
              # derivation depends on, so it cannot be missing at runtime, and
              # macOS has no /run/opengl-driver for the loader to find on its
              # own.
              #
              # No xvfb-run: X11 is not how anything on macOS displays, and
              # the web browser provider's headed-Chrome path does not go
              # through a virtual X server there.
              wrapProgram $out/bin/sicompass \
                --set VK_ICD_FILENAMES "${pkgs.moltenvk}/share/vulkan/icd.d/MoltenVK_icd.json" \
                --prefix DYLD_FALLBACK_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath (with pkgs; [
                  vulkan-loader
                  moltenvk
                  sdl3
                ])}"
            '';

            desktopItems = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
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
