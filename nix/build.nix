{ lib, crane, rustPlatform, pkg-config, wayland, cordisRs, binaryName, pname, cargoPackage }:
let
  workspaceRoot = ../.;
  src = lib.fileset.toSource {
    root = workspaceRoot;
    fileset = workspaceRoot;
  };
in crane.buildPackage {
  inherit pname src;
  version = "0.1.0";
  cargoExtraArgs = "--package ${cargoPackage} --bin ${binaryName}";
  cargoLock = ../Cargo.lock;
  preConfigure = ''ln -s ${cordisRs} "$PWD/cordis-rs"'';
  nativeBuildInputs = [ pkg-config rustPlatform.bindgenHook ];
  buildInputs = [ wayland ];
  cargoVendorDir = crane.vendorCargoDeps { inherit src; cargoLock = ../Cargo.lock; };
  dontPatchELF = true;
  postInstall = lib.optionalString (cargoPackage == "omp-harness-tui") ''
    mkdir -p $out/share/omp-harness
    cp -r ${../adapters/omp/extension} $out/share/omp-harness/omp-extension
    chmod -R u+w $out/share/omp-harness/omp-extension
  '';
}
