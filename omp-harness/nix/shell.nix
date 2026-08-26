{
  mkShell,
  lib,
  cargo,
  rustc,
  rustfmt,
  clippy,
  pkg-config,
  wayland,
  libcap,
  xz,
  openssl,
  zlib,
}: let
  acpRuntimeLibs = [libcap xz openssl zlib];
in
mkShell {
  packages = [
    cargo
    rustc
    rustfmt
    clippy
    pkg-config
    wayland
  ];

  shellHook = ''
    echo "omp-harness dev shell"
    export ACP_RUNTIME_LIBS="${lib.makeLibraryPath acpRuntimeLibs}"
    export LD_LIBRARY_PATH="$ACP_RUNTIME_LIBS''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  '';
}
