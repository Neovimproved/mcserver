{
  description = "A very basic flake";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
    }:
    let
      inherit (nixpkgs) lib;

      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
    in
    {
      packages = lib.genAttrs supportedSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};

          craneLib = crane.mkLib pkgs;
          src = craneLib.cleanCargoSource ./.;

          commonArgs = {
            inherit src;

            strictDeps = true;

            buildInputs = [

            ];
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          crateArgs = commonArgs // {
            inherit cargoArtifacts;
            inherit (craneLib.crateNameFromCargoToml { inherit src; }) version;
          };

          fileSetForCrate =
            crate:
            lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                (craneLib.fileset.commonCargoSources ./core)
                (craneLib.fileset.commonCargoSources ./daemon)
                (craneLib.fileset.commonCargoSources crate)
              ];
            };
        in
        rec {

          cli = craneLib.buildPackage crateArgs // {
            pname = "mcserver";
            # cargoExtraArgs = "-p mcserver";
            src = fileSetForCrate ./cli;
          };

          daemon = craneLib.buildPackage crateArgs // {
            pname = "mcserver-daemon";
            # cargoExtraArgs = "-p mcserver-daemon";
            src = fileSetForCrate ./daemon;
          };

          default = cli;
        }
      );

      devShells = lib.genAttrs supportedSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [
              self.packages.${system}.cli
              self.packages.${system}.daemon
            ];

            nativeBuildInputs = with pkgs; [
              rust-analyzer
            ];
          };
        }
      );
    };
}
