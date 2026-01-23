{
  description = "Sicompass Dev Flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    accesskit-c-src = {
      url = "github:AccessKit/accesskit-c";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, accesskit-c-src }:
    let
      supportedSystems = [
        "aarch64-linux"
        "aarch64-darwin"
        "aarch64-windows"
        "i686-linux"
        "riscv64-linux"
        "x86_64-linux"
        "x86_64-darwin"
        "x86_64-windows"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      nixpkgsFor = forAllSystems (system: import nixpkgs { inherit system; });

      # AccessKit C bindings derivation
      # Uses rustPlatform.buildRustPackage to build the Rust library,
      # then installs C headers and shared library
      accesskit-c-for = system:
        let
          pkgs = nixpkgsFor.${system};
        in
        pkgs.rustPlatform.buildRustPackage rec {
          pname = "accesskit-c";
          version = accesskit-c-src.shortRev or "dev";

          src = accesskit-c-src;

          cargoHash = "sha256-KCZ4jpYoARRv7dg44ar228TJKmkz6hVRirkDPpKfsK8=";

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          buildInputs = with pkgs; [
            # Linux accessibility backend (AT-SPI over D-Bus)
          ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            at-spi2-core
            dbus
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            apple-sdk_15
          ];

          # Build as cdylib (C-compatible shared library)
          buildType = "release";

          postInstall = ''
            # Install the C header
            mkdir -p $out/include
            cp include/accesskit.h $out/include/

            # The cdylib is already installed by cargo, but ensure it's findable
            # Create pkg-config file
            mkdir -p $out/lib/pkgconfig
            cat > $out/lib/pkgconfig/accesskit.pc << EOF
            prefix=$out
            libdir=\''${prefix}/lib
            includedir=\''${prefix}/include

            Name: accesskit
            Description: C bindings for AccessKit accessibility infrastructure
            Version: ${version}
            Libs: -L\''${libdir} -laccesskit
            Cflags: -I\''${includedir}
            EOF
          '';

          meta = with pkgs.lib; {
            description = "C bindings for AccessKit accessibility infrastructure";
            homepage = "https://github.com/AccessKit/accesskit-c";
            license = with licenses; [ asl20 mit ];
            maintainers = [];
          };
        };
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgsFor.${system};
          accesskit-c = accesskit-c-for system;
        in
        {
          inherit accesskit-c;

          default = pkgs.stdenv.mkDerivation rec {
            pname = "sicompass";
            version = "0.1";
            src = self;

            nativeBuildInputs = with pkgs; [ meson ninja pkg-config glslang ];
            buildInputs = with pkgs; [
              glfw
              #renderdoc
              spirv-tools
              vulkan-loader
              vulkan-headers
              freetype
              cglm
              stb
              harfbuzz
              uthash
              json_c
              sdl3
              # AccessKit for accessibility
              accesskit-c
              # Testing
              unity-test
            ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              vulkan-volk
              vulkan-tools
              vulkan-validation-layers
              vulkan-extension-layer
              vulkan-tools-lunarg
              xorg.libxcb
              at-spi2-core
            ];

            enableParallelBuilding = true;

            meta = with pkgs.lib; {
              homepage = "https://github.com/friendlyflow/sicompass";
              license = with licenses; [ mit ];
              maintainers = [ "Nico Verrijdt" ];
            };
          };
        });

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgsFor.${system};
          accesskit-c = accesskit-c-for system;
        in
        {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              meson
              ninja
              pkg-config
              glfw
              glslang
              #renderdoc
              spirv-tools
              vulkan-loader
              vulkan-headers
              freetype
              cglm
              stb
              harfbuzz
              uthash
              json_c
              sdl3
              cppcheck
              flawfinder
              # AccessKit for accessibility
              accesskit-c
              # Testing
              unity-test
            ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              gcc
              vulkan-volk
              vulkan-tools
              vulkan-validation-layers
              vulkan-extension-layer
              vulkan-tools-lunarg
              xorg.libxcb
              at-spi2-core
            ];

            # shellHooks = ''
            #   # export PATH="$PWD/node_modules/.bin/:$PATH"
            #   # alias run="npm run"
            # '';

            packages = with pkgs; [
              fish
            ];

            shellHook = with pkgs; ''
              echo "Welcome to my Vulkan Shell"
              echo "vulkan loader: ${vulkan-loader}"
              echo "vulkan headers: ${vulkan-headers}"
              echo "validation layer: ${vulkan-validation-layers}"
              echo "tools: ${vulkan-tools}"
              echo "tools-lunarg: ${vulkan-tools-lunarg}"
              echo "extension-layer: ${vulkan-extension-layer}"

              export LD_LIBRARY_PATH="${stb}/lib:${glfw}/lib:${freetype}/lib:${vulkan-loader}/lib:${vulkan-validation-layers}/lib";
              export VULKAN_SDK="${vulkan-headers}";
              export VK_LAYER_PATH="${vulkan-validation-layers}/share/vulkan/explicit_layer.d";
              
              exec fish
            '';
          };
        });
    };
}

# {
#   description = "A libadwaita wrapper for ExpidusOS with Tokyo Night's styling";
#   outputs = { self, nixpkgs }:
#     let
#       supportedSystems = [
#         "aarch64-linux"
#         "aarch64-darwin"
#         "i686-linux"
#         "riscv64-linux"
#         "x86_64-linux"
#         "x86_64-darwin"
#       ];
#       forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
#       nixpkgsFor = forAllSystems (system: import nixpkgs { inherit system; });
#     in
#     {
#       packages = forAllSystems (system:
#         let
#           pkgs = nixpkgsFor.${system};
#         in
#         {
#           default = pkgs.stdenv.mkDerivation rec {
#             name = "libtokyo";
#             src = self;
#             outputs = [ "out" "dev" ];

#             nativebuildInputs = with pkgs; [ meson ninja pkg-config vala glib sass ];
#             buildInputs = with pkgs; [ libadwaita ];

#             enableParallelBuilding = true;

#             meta = with pkgs.lib; {
#               homepage = "https://github.com/ExpidusOS/libtokyo";
#               license = with licenses; [ gpl3Only ];
#               maintainers = [ "Tristan Ross" ];
#             };
#           };
#         });

#       devShells = forAllSystems (system:
#         let
#           pkgs = nixpkgsFor.${system};
#         in
#         {
#           default = pkgs.mkShell {
#             buildInputs = with pkgs; [
#               meson
#               ninja
#               pkg-config
#               vala
#               nodejs
#               gcc
#               libadwaita.dev
#               libadwaita.devdoc
#             ];

#             shellHooks = ''
#               export PATH="$PWD/node_modules/.bin/:$PATH"
#               alias run="npm run"
#             '';
#           };
#         });
#     };
# }