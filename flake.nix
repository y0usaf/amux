{
  description = "Minimal Pi terminal harness";

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
        pname = "pi-harness";
        cargoPackage = "pi-harness-tui";
        binaryName = "pi-harness";
      };
      pi-harness-tui = pkgs.callPackage ./nix/build.nix {
        crane = crane.mkLib pkgs;
        pname = "pi-harness-tui";
        cargoPackage = "pi-harness-tui";
        binaryName = "pi-harness-tui";
      };
    });

    devShells = forAllSystems (pkgs: {
      default = pkgs.callPackage ./nix/shell.nix {};
    });

    checks = forAllSystems (pkgs: {
      pi-extension-tests = pkgs.runCommand "pi-extension-tests" {
        nativeBuildInputs = [pkgs.nodejs];
        src = builtins.path {path = ./pi-extension; name = "pi-extension-tests-src";};
      } ''
        tests=("$src"/*.test.js)
        if [ ! -e "''${tests[0]}" ]; then
          echo "pi-extension-tests: no test files matched $src/*.test.js" >&2
          exit 1
        fi
        node --test "''${tests[@]}"
        touch $out
      '';
    });

    overlays.default = final: prev: {
      pi-harness = self.packages.${final.system}.default;
    };
  };
}
