{
  description = "Minimal Pi desktop harness";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    rust-overlay,
    ...
  }: let
    supportedSystems = ["x86_64-linux" "aarch64-linux"];
    forAllSystems = f:
      nixpkgs.lib.genAttrs supportedSystems (system:
        f (import nixpkgs {
          inherit system;
          overlays = [rust-overlay.overlays.default];
        }));
  in {
    packages = forAllSystems (pkgs: rec {
      default = pi-harness;
      pi-harness = pkgs.callPackage ./nix/build.nix {
        crane = crane.mkLib pkgs;
      };
    });

    devShells = forAllSystems (pkgs: {
      default = pkgs.callPackage ./nix/shell.nix {};
    });

    overlays.default = final: prev: {
      pi-harness = self.packages.${final.system}.default;
    };
  };
}
