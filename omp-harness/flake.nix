{
  description = "Minimal omp terminal harness";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # The shared WASM config kernel (crate `cordis`). The root Cargo.toml path
    # dep `cordis = { path = "../cordis-rs/crates/cordis" }` (a `cordis-rs`
    # sibling of the source root) is not covered by this repo's source filter,
    # so cordis-rs is a flake input and its source is repointed into the build
    # (see `cordisSymlink` in nix/build.nix). Pinned git input (not `path:`):
    # only committed files ship, so the target/ dir never enters the store.
    # `flake = false` because cordis-rs's flake.nix is not committed at this rev;
    # we only need the crate source tree (its workspace Cargo.toml is committed).
    cordis-rs = {
      url = "github:y0usaf/cordis-rs/426a34e72d25ffb3dc72201523eed48416934f65";
      flake = false;
    };
    # The agent this harness drives. Depending on the upstream flake puts the
    # real `omp` binary in the harness closure: `nix run` works with no
    # system-wide install, and the harness always launches an omp it built
    # against the same nixpkgs.
    oh-my-pi.url = "github:can1357/oh-my-pi";
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    rust-overlay,
    cordis-rs,
    oh-my-pi,
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
      default = omp-harness;

      # Unwrapped TUI binary crate (both bin names).
      omp-harness-tui = pkgs.callPackage ./nix/build.nix {
        crane = crane.mkLib pkgs;
        pname = "omp-harness-tui";
        cordisRs = cordis-rs;
        cargoPackage = "omp-harness-tui";
        binaryName = "omp-harness-tui";
      };

      # Default entrypoint: the TUI wrapped so the upstream flake's `omp` is
      # on PATH inside the program environment — discovery resolves it via
      # which() with zero configuration.
      omp-harness = pkgs.callPackage ./nix/wrapper.nix {
        harness = pkgs.callPackage ./nix/build.nix {
          crane = crane.mkLib pkgs;
          pname = "omp-harness";
          cordisRs = cordis-rs;
          cargoPackage = "omp-harness-tui";
          binaryName = "omp-harness";
        };
        omp = oh-my-pi.packages.${pkgs.system}.omp;
      };

      # Passthrough so `nix run .#omp` works from this repo too.
      inherit (oh-my-pi.packages.${pkgs.system}) omp;
    });

    apps = forAllSystems (pkgs: {
      default = {
        type = "app";
        program = "${self.packages.${pkgs.system}.omp-harness}/bin/omp-harness";
      };
    });

    devShells = forAllSystems (pkgs: {
      default = pkgs.callPackage ./nix/shell.nix {};
    });

    checks = forAllSystems (pkgs: {
      # Proves config-as-WASM loads at startup and reverts on unmount (the
      # config::cordis tests in src/config/cordis.rs) under Nix.
      config-wasm-tests = pkgs.callPackage ./nix/build.nix {
        crane = crane.mkLib pkgs;
        pname = "omp-harness-config-wasm-tests";
        cordisRs = cordis-rs;
        cargoPackage = "omp-harness-tui";
        binaryName = "omp-harness";
        doCheck = true;
        cargoTestFlags = ["-p" "omp-harness" "--lib" "config::cordis"];
      };
      omp-extension-tests = pkgs.runCommand "omp-extension-tests" {
        nativeBuildInputs = [pkgs.nodejs];
        src = builtins.path {path = ./omp-extension; name = "omp-extension-tests-src";};
      } ''
        tests=("$src"/*.test.js)
        if [ ! -e "''${tests[0]}" ]; then
          echo "omp-extension-tests: no test files matched $src/*.test.js" >&2
          exit 1
        fi
        node --test "''${tests[@]}"
        touch $out
      '';
    });

    overlays.default = final: prev: {
      omp-harness = self.packages.${final.system}.default;
    };
  };
}
