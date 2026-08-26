{ lib, crane, rustPlatform, pkg-config, wayland, cordisRs, root, binaryName, pname, cargoPackage }:
let
  src = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [ (root + "/src") (root + "/crates") (root + "/build.rs") (root + "/config.wat") (root + "/Cargo.toml") (root + "/Cargo.lock") (root + "/pi-extension") (root + "/omp-extension") ];
  };
  common = {
    inherit pname src;
    version = (builtins.fromTOML (builtins.readFile (root + "/Cargo.toml")).package.version);
    cargoExtraArgs = "--package ${cargoPackage} --bin ${binaryName}";
    cargoLock = root + "/Cargo.lock";
    preConfigure = ''ln -s ${cordisRs} "$PWD/../cordis-rs"'';
    nativeBuildInputs = [ pkg-config rustPlatform.bindgenHook ];
    buildInputs = [ wayland ];
    cargoVendorDir = crane.vendorCargoDeps { inherit src; cargoLock = root + "/Cargo.lock"; };
    dontPatchELF = true;
  };
in crane.buildPackage common
