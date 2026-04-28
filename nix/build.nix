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
  binaryName ? "pi-harness",
  pname ? binaryName,
  cargoPackage ? pname,
}: let
  acpRuntimeLibs = [libcap xz openssl zlib];
  craneLib = crane;
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../src
      ../crates
      ../pi-extension/harness-sidechannel.js
      ../Cargo.toml
      ../Cargo.lock
    ];
  };

  commonArgs = {
    inherit pname src;
    version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).package.version;
    cargoExtraArgs = "--package ${cargoPackage} --bin ${binaryName}";
    cargoLock = ../Cargo.lock;

    nativeBuildInputs = [pkg-config rustPlatform.bindgenHook];
    buildInputs = [wayland];

    env = {
      NIX_LDFLAGS = "-rpath ${lib.makeLibraryPath [wayland]}";
      ACP_RUNTIME_LIBS = lib.makeLibraryPath acpRuntimeLibs;
    };

    dontPatchELF = true;
    doCheck = false;

    cargoVendorDir = craneLib.vendorCargoDeps {
      inherit src;
      cargoLock = ../Cargo.lock;
    };
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
  craneLib.buildPackage (lib.recursiveUpdate commonArgs {
    inherit cargoArtifacts;

    postInstall = ''
      install -Dm644 ${../pi-extension/harness-sidechannel.js} \
        $out/share/pi-harness/pi-extension/harness-sidechannel.js
    '';

    meta.mainProgram = binaryName;
  })
