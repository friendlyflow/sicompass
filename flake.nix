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

              # Native libs required by Rust crates
              pkg-config
              sdl3
              freetype
              libwebp
              curl

              # Vulkan (used via ash crate)
              spirv-tools
              vulkan-loader
              vulkan-headers
              glslang

              # Script providers (TypeScript)
              bun

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

              # Library path for Vulkan and other runtime deps
              export LD_LIBRARY_PATH="${libwebp}/lib:${freetype}/lib:${vulkan-loader}/lib:${vulkan-validation-layers}/lib:${curl}/lib:${sdl3}/lib:${libxkbcommon}/lib:${wayland}/lib:/usr/lib/x86_64-linux-gnu";
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

              exec fish
            '';
          };
        });
    };
}
