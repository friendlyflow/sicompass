{
  description = "Sicompass Dev Flake";
  outputs = { self, nixpkgs }:
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
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgsFor.${system};
        in
        {
          default = pkgs.stdenv.mkDerivation rec {
            name = "sicompass";
            src = self;
            outputs = [ "out" "dev" ];

            nativebuildInputs = with pkgs; [ meson ninja pkg-config gcc glibc ];
            buildInputs = with pkgs; [
              glfw
              glslang
              #renderdoc
              spirv-tools
              vulkan-volk
              vulkan-tools
              vulkan-loader
              vulkan-headers
              vulkan-validation-layers
              vulkan-tools-lunarg
              vulkan-extension-layer
              freetype
              cglm
              stb
              harfbuzz
              uthash
              json_c
              xorg.libxcb
              sdl3
            ];

            enableParallelBuilding = true;

            meta = with pkgs.lib; {
              homepage = "https://github.com/friendlyflow/ff";
              license = with licenses; [ MIT ];
              maintainers = [ "Nico Verrijdt" ];
            };
          };
        });

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgsFor.${system};
        in
        {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              meson
              ninja
              pkg-config
              gcc
              glfw
              glslang
              #renderdoc
              spirv-tools
              vulkan-volk
              vulkan-tools
              vulkan-loader
              vulkan-headers
              vulkan-validation-layers
              vulkan-tools-lunarg
              vulkan-extension-layer
              freetype
              cglm
              stb
              harfbuzz
              uthash
              json_c
              xorg.libxcb
              sdl3
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