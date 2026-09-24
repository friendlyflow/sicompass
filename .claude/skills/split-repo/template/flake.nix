{
  # Skeleton from sicompass's /split-repo. Fill in every @...@ marker, and
  # delete the nixpkgs-x86-darwin input if the repo is Linux-only. The comments
  # in ../sicompass/flake.nix explain the why behind each piece in full.
  description = "@DESCRIPTION@";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    # Intel macOS only: nixpkgs 26.11 throws on `import` for x86_64-darwin, so
    # it is built from the last branch that carries it (supported to the end of
    # 2026). Remove along with the system below when that runs out.
    nixpkgs-x86-darwin.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";

    # Dependency build keyed on Cargo.lock alone, crate on top. Vendors git
    # dependencies by the rev in Cargo.lock, so there is no hash to maintain.
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, nixpkgs-x86-darwin, crane }:
    let
      supportedSystems = [ @SYSTEMS@ ];

      nixpkgsInputFor = system:
        if system == "x86_64-darwin" then nixpkgs-x86-darwin else nixpkgs;

      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      nixpkgsFor = forAllSystems (system:
        import (nixpkgsInputFor system) { inherit system; });

      # Single source of truth for the version.
      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
    in
    {
      devShells = forAllSystems (system:
        let pkgs = nixpkgsFor.${system}; in
        {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              cargo
              rustc
              rust-analyzer
              clippy
              rustfmt
              pkg-config
              @DEV_PACKAGES@
            ];

            shellHook = ''
              export RUST_SRC_PATH="${pkgs.rustc}/lib/rustlib/src/rust/library";
              @SHELL_HOOK@
            '';
          };
        });

      packages = forAllSystems (system:
        let
          pkgs = nixpkgsFor.${system};
          craneLib = crane.mkLib pkgs;
          commonArgs = {
            inherit version;
            pname = "@PACKAGE@";
            src = craneLib.cleanCargoSource ./.;
            strictDeps = true;
            # The suite is run by ci.yml and locally, where a display can be
            # arranged.
            doCheck = false;
            @BUILD_ARGS@
          };
        in
        {
          default = craneLib.buildPackage (commonArgs // {
            cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          });
        });
    };
}
