{ lib, crane, rustPlatform, pkg-config, wayland, binaryName, pname, cargoPackage }:
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
  nativeBuildInputs = [ pkg-config rustPlatform.bindgenHook ];
  buildInputs = [ wayland ];
  cargoVendorDir = crane.vendorCargoDeps { inherit src; cargoLock = ../Cargo.lock; };
  dontPatchELF = true;
  postInstall =
    lib.optionalString (cargoPackage == "pi-harness-tui") ''
      mkdir -p $out/share/pi-harness
      cp -r ${../adapters/pi/extension} $out/share/pi-harness/pi-extension
      chmod -R u+w $out/share/pi-harness/pi-extension
    ''
    + lib.optionalString (cargoPackage == "omp-harness-tui") ''
      mkdir -p $out/share/omp-harness
      cp -r ${../adapters/omp/extension} $out/share/omp-harness/omp-extension
      chmod -R u+w $out/share/omp-harness/omp-extension
    '';
}
