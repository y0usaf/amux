{
  mkShell,
  lib,
  cargo,
  rustc,
  rustfmt,
  clippy,
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
  acpRuntimeLibs = [libcap xz openssl zlib];
in
mkShell {
  packages = [
    cargo
    rustc
    rustfmt
    clippy
    pkg-config
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
    zenity
  ];

  shellHook = ''
    echo "pi-harness dev shell"
    export ACP_RUNTIME_LIBS="${lib.makeLibraryPath acpRuntimeLibs}"
    export LD_LIBRARY_PATH="$ACP_RUNTIME_LIBS''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  '';
}
