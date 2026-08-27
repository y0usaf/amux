{
  writeShellApplication,
  writeShellScriptBin,
  runCommand,
  bash,
  bubblewrap,
  coreutils,
  vhs,
  dejavu_fonts,
  pi-coding-agent,
  piHarness,
}: let
  demoExtensions = runCommand "pi-harness-demo-extensions" {} ''
    mkdir -p $out/demo-provider/node_modules/@earendil-works
    mkdir -p $out/pi-harness/node_modules/@earendil-works
    cp ${./demo-provider.js} $out/demo-provider/index.js
    cp -r ${../pi-extension}/. $out/pi-harness/
    ln -s ${pi-coding-agent}/lib/node_modules/pi-monorepo/node_modules/@mariozechner/pi-ai \
      $out/demo-provider/node_modules/@earendil-works/pi-ai
    ln -s ${pi-coding-agent}/lib/node_modules/pi-monorepo/node_modules/@mariozechner/pi-tui \
      $out/pi-harness/node_modules/@earendil-works/pi-tui
  '';
  demoPi = writeShellScriptBin "pi-demo" ''
    exec ${pi-coding-agent}/bin/pi \
      --offline \
      --no-extensions \
      --provider pi-harness-demo \
      --model demo-1 \
      -e ${demoExtensions}/demo-provider/index.js \
      "$@"
  '';
  sandboxRunner = writeShellApplication {
    name = "pi-harness-demo";
    runtimeInputs = [bubblewrap coreutils];
    text = ''
      repo="$(pwd -P)"
      if [[ ! -f "$repo/docs/demo/demo.tape" ]]; then
        echo "pi-harness-demo must run from the pi-harness repository root" >&2
        exit 2
      fi

      exec bwrap \
        --die-with-parent \
        --unshare-all \
        --ro-bind /nix/store /nix/store \
        --dev /dev \
        --proc /proc \
        --tmpfs /tmp \
        --dir /demo \
        --dir /demo/home \
        --dir /demo/config \
        --dir /demo/state \
        --dir /demo/runtime \
        --dir /demo/pi-agent \
        --dir /demo/pi-agent/sessions \
        --dir /work \
        --ro-bind "$repo" /work \
        --bind "$repo/docs/demo" /work/docs/demo \
        --chdir /work/docs/demo/project \
        --clearenv \
        --setenv HOME /demo/home \
        --setenv XDG_CONFIG_HOME /demo/config \
        --setenv XDG_STATE_HOME /demo/state \
        --setenv XDG_RUNTIME_DIR /demo/runtime \
        --setenv PI_CODING_AGENT_DIR /demo/pi-agent \
        --setenv PI_CODING_AGENT_SESSION_DIR /demo/pi-agent/sessions \
        --setenv PI_BINARY ${demoPi}/bin/pi-demo \
        --setenv AGENT_HARNESS_PI_EXTENSION ${demoExtensions}/pi-harness/index.js \
        --setenv PI_OFFLINE 1 \
        --setenv PI_DEFAULT_PACKAGES "" \
        --setenv PATH ${bash}/bin:${coreutils}/bin:${demoPi}/bin:${pi-coding-agent}/bin \
        --setenv LANG C.UTF-8 \
        --setenv TERM xterm-256color \
        ${piHarness}/bin/pi-harness /work/docs/demo/project
  '';
  };
in
  writeShellApplication {
    name = "pi-harness-record-demo";
    runtimeInputs = [vhs sandboxRunner dejavu_fonts];
    text = ''
      repo="$(pwd -P)"
      if [[ ! -f "$repo/docs/demo/demo.tape" ]]; then
        echo "run this command from the pi-harness repository root" >&2
        exit 2
      fi

      vhs_home="$(mktemp -d)"
      trap 'rm -rf "$vhs_home"' EXIT
      mkdir -p "$vhs_home/home" "$vhs_home/config" "$vhs_home/state" "$vhs_home/cache"

      env -i \
        HOME="$vhs_home/home" \
        XDG_CONFIG_HOME="$vhs_home/config" \
        XDG_STATE_HOME="$vhs_home/state" \
        XDG_CACHE_HOME="$vhs_home/cache" \
        PATH="$PATH" \
        LANG=C.UTF-8 \
        TERM=xterm-256color \
        vhs "$repo/docs/demo/demo.tape"
    '';
  }
