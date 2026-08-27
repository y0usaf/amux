# Wraps the TUI binary so the `omp` agent from the upstream oh-my-pi flake is
# on PATH inside the program environment. Discovery (src/omp/discovery.rs)
# resolves `omp` through which(), so the harness needs no other configuration.
# An explicit override still wins at runtime: OMP_BINARY or the config's
# agent path.
{
  lib,
  symlinkJoin,
  makeWrapper,
  harness,
  omp,
}:
symlinkJoin {
  name = "${harness.pname}-wrapped";
  paths = [harness];
  nativeBuildInputs = [makeWrapper];

  postBuild = ''
    wrapProgram "$out/bin/omp-harness" \
      --prefix PATH : ${lib.makeBinPath [omp]}
  '';

  passthru = {
    inherit harness omp;
    mainProgram = "omp-harness";
  };

  meta = harness.meta or {};
}
