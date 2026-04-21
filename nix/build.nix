{
  lib,
  crane,
  rustPlatform,
  makeWrapper,
  pkg-config,
  fontconfig,
  freetype,
  libGL,
  libxkbcommon,
  vulkan-loader,
  wayland,
  libxcb,
  libx11,
  libxcursor,
  libxi,
  libxrandr,
  libcap,
  xz,
  openssl,
  zlib,
  zenity,
}: let
  pname = "pi-harness";
  acpRuntimeLibs = [libcap xz openssl zlib];
  craneLib = crane;
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../src
      ../pi-extension/harness-sidechannel.js
      ../Cargo.toml
      ../Cargo.lock
    ];
  };

  commonArgs = {
    inherit pname src;
    version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).package.version;
    cargoLock = ../Cargo.lock;

    nativeBuildInputs = [pkg-config rustPlatform.bindgenHook makeWrapper];
    buildInputs = [
      fontconfig
      freetype
      libGL
      libxkbcommon
      vulkan-loader
      wayland
      libxcb
      libx11
      libxcursor
      libxi
      libxrandr
    ];

    env = {
      NIX_LDFLAGS = "-rpath ${lib.makeLibraryPath [vulkan-loader wayland libGL libxkbcommon]}";
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

    postFixup = ''
      wrapProgram $out/bin/pi-harness \
        --prefix PATH : ${lib.makeBinPath [zenity fontconfig]}
    '';

    meta.mainProgram = pname;
  })
