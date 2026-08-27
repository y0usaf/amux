{
  description = "Pi and omp terminal harnesses with shared amux workspace";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = { url = "github:oxalica/rust-overlay"; inputs.nixpkgs.follows = "nixpkgs"; };
    cordis-rs = { url = "github:y0usaf/cordis-rs/426a34e72d25ffb3dc72201523eed48416934f65"; flake = false; };
    oh-my-pi.url = "github:can1357/oh-my-pi";
  };
  outputs = { self, nixpkgs, crane, rust-overlay, cordis-rs, oh-my-pi, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; }));
    in {
      packages = forAllSystems (pkgs: {
        pi-harness = pkgs.callPackage ./nix/build.nix { crane = crane.mkLib pkgs; cordisRs = cordis-rs; pname = "pi-harness"; cargoPackage = "pi-harness-tui"; binaryName = "pi-harness"; };
        omp-harness = pkgs.callPackage ./nix/build.nix { crane = crane.mkLib pkgs; cordisRs = cordis-rs; pname = "omp-harness"; cargoPackage = "omp-harness-tui"; binaryName = "omp-harness"; };
        default = self.packages.${pkgs.system}.pi-harness;
      });
      apps = forAllSystems (pkgs: {
        pi-harness = { type = "app"; program = "${self.packages.${pkgs.system}.pi-harness}/bin/pi-harness"; };
        omp-harness = { type = "app"; program = "${self.packages.${pkgs.system}.omp-harness}/bin/omp-harness"; };
        default = self.apps.${pkgs.system}.pi-harness;
      });
      devShells = forAllSystems (pkgs: { default = pkgs.mkShell { packages = [ pkgs.rustc pkgs.cargo ]; }; });
    };
}
