{
  description = "A very basic flake";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
  };

  outputs = { self, nixpkgs, }: let
    lib = nixpkgs.lib;
    supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
    ];
  in {
    packages = lib.genAttrs supportedSystems (system: let
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      default = pkgs.rustPlatform.buildRustPackage {
        pname = "mst";
        version = "0.0.1";
        cargoLock.lockFile = ./Cargo.lock;
        src = pkgs.lib.cleanSource ./.;
      };
    });

    shells = lib.genAttrs supportedSystems (system: let
      pkgs = nixpkgs.legacyPackages.${system};
    in {
        default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.default ];

            nativeBuildInputs = with pkgs; [
                rust-analyzer
            ];
        };
    });
  };
}
