{
  # @NAME@: @DESCRIPTION@. Skeleton for a sicompass plugin, from sicompass's
  # /split-repo. A plugin is a program, released for every platform sicompass
  # runs plugins on. On Linux that is a static musl build, which nixpkgs' rustc
  # has no std for, so the toolchain comes from rust-overlay. flake.lock pins it.
  description = "@NAME@: @DESCRIPTION@";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    nixpkgs-x86-darwin.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, nixpkgs-x86-darwin, rust-overlay }:
    let
      supportedSystems = [ "aarch64-linux" "aarch64-darwin" "x86_64-linux" "x86_64-darwin" ];
      nixpkgsInputFor = system:
        if system == "x86_64-darwin" then nixpkgs-x86-darwin else nixpkgs;
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      nixpkgsFor = forAllSystems (system:
        import (nixpkgsInputFor system) {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        });
    in
    {
      devShells = forAllSystems (system:
        let
          pkgs = nixpkgsFor.${system};
          # This computer's plugin target, which `release-plugin.sh --dry-run`
          # builds. The other platforms are built on their own CI runners.
          pluginTarget = {
            "x86_64-linux" = "x86_64-unknown-linux-musl";
            "aarch64-linux" = "aarch64-unknown-linux-musl";
            "aarch64-darwin" = "aarch64-apple-darwin";
            "x86_64-darwin" = "x86_64-apple-darwin";
          }.${system};
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
            targets = [ pluginTarget ];
          };
        in
        {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              rustToolchain
              # scripts/release-plugin.sh reads plugin.json with it.
              jq
            ];
            shellHook = ''
              export RUST_SRC_PATH="${rustToolchain}/lib/rustlib/src/rust/library";
            '';
          };
        });
    };
}
