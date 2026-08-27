{
  lib,
  crane,
  rustPlatform,
  pkg-config,
  wayland,
  libcap,
  xz,
  openssl,
  zlib,
  cordisRs,
  binaryName ? "pi-harness",
  pname ? binaryName,
  cargoPackage ? pname,
  cargoExtraArgs ? null,
  doCheck ? false,
  cargoTestFlags ? null,
}: let
  acpRuntimeLibs = [libcap xz openssl zlib];
  craneLib = crane;
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../src
      ../crates
      ../pi-extension
      ../build.rs
      ../config.wat
      ../Cargo.toml
      ../Cargo.lock
    ];
  };
  # The root Cargo.toml has `cordis = { path = "../cordis-rs/crates/cordis" }`,
  # i.e. a `cordis-rs` sibling of the source root. Cargo resolves it one level
  # up from $PWD (crane's source root), and `../` is not part of this repo's
  # source filter, so repoint that sibling at the cordis-rs flake input inside
  # the writable build dir (cwd = source root; `..` = the build dir). Mirrors
  # the ekko/tomoe cordis wiring.
  cordisSymlink = ''
    ln -s ${cordisRs} "$PWD/../cordis-rs"
  '';

  common = {
    inherit pname src doCheck;
    version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).package.version;
    cargoExtraArgs =
      if cargoExtraArgs != null
      then cargoExtraArgs
      else "--package ${cargoPackage} --bin ${binaryName}";
    cargoLock = ../Cargo.lock;
    preConfigure = cordisSymlink;

    nativeBuildInputs = [pkg-config rustPlatform.bindgenHook];
    buildInputs = [wayland];

    env = {
      NIX_LDFLAGS = "-rpath ${lib.makeLibraryPath [wayland]}";
      ACP_RUNTIME_LIBS = lib.makeLibraryPath acpRuntimeLibs;
    };

    dontPatchELF = true;

    cargoVendorDir = craneLib.vendorCargoDeps {
      inherit src;
      cargoLock = ../Cargo.lock;
    };
  };

  commonTest =
    if cargoTestFlags != null
    then common // { cargoTestFlags = cargoTestFlags; }
    else common;

  cargoArtifacts = craneLib.buildDepsOnly commonTest;
in
  craneLib.buildPackage (lib.recursiveUpdate commonTest {
    inherit cargoArtifacts;

    postInstall = ''
      mkdir -p $out/share/pi-harness
      cp -r ${../pi-extension} $out/share/pi-harness/pi-extension
      chmod -R u+w $out/share/pi-harness/pi-extension
    '';

    meta.mainProgram = binaryName;
  })
