{
  description = "pi sessions in a focused, bare ekko instance";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    ekko = {
      url = "github:y0usaf/ekko";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    pi-flake = {
      url = "github:y0usaf/pi-flake?ref=main";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    nixpkgs,
    ekko,
    pi-flake,
    ...
  }: let
    system = "x86_64-linux";
    pkgs = nixpkgs.legacyPackages.${system};
    pi = "${pi-flake.packages.${system}.pi-full}/bin/pi";
    env = "${pkgs.coreutils}/bin/env";
    pi-harness = pkgs.runCommand "pi-harness" {meta.mainProgram = "pi-harness";} ''
      mkdir -p $out/bin $out/libexec $out/share/pi-harness
      ln -s ${ekko.packages.${system}.default}/bin/ekko $out/libexec/pi-harness
      cp ${./status.js} $out/share/pi-harness/status.js
      substitute ${./pi-harness.lisp} $out/share/pi-harness/pi-harness.lisp \
        --subst-var-by env ${env} \
        --subst-var-by pi ${pi} \
        --subst-var-by status $out/share/pi-harness/status.js
      substitute ${./pi-harness.sh} $out/bin/pi-harness \
        --subst-var-by shell ${pkgs.runtimeShell} \
        --subst-var-by ekko $out/libexec/pi-harness \
        --subst-var-by profile $out/share/pi-harness/pi-harness.lisp \
        --subst-var-by env ${env} \
        --subst-var-by pi ${pi} \
        --subst-var-by status $out/share/pi-harness/status.js
      chmod +x $out/bin/pi-harness
    '';
  in {
    packages.${system}.default = pi-harness;
    checks.${system}.default = pkgs.runCommand "pi-harness-smoke" {} ''
      export HOME=$TMPDIR XDG_CONFIG_HOME=$TMPDIR/config XDG_STATE_HOME=$TMPDIR/state
      export XDG_RUNTIME_DIR=$TMPDIR/run
      mkdir -p $XDG_CONFIG_HOME && mkdir -m 700 $XDG_RUNTIME_DIR
      ${pi-harness}/bin/pi-harness config check
      ${pi-harness}/bin/pi-harness run --detached sh -c 'exec sleep 600'
      for i in $(seq 50); do
        ${pi-harness}/bin/pi-harness inspect > inspect.json
        grep -q '"owner":"pi-harness"' inspect.json && break
        sleep 0.1
      done
      ${pi-harness}/bin/pi-harness stop
      grep -q '"error":null' inspect.json
      grep -q '"owner":"pi-harness"' inspect.json
      touch $out
    '';
  };
}
