# Demo recording

Generate the README recording from the repository root:

```sh
nix run .#demo
```

The command writes `docs/demo/pi-harness.gif` and `docs/demo/pi-harness.webm`.

The harness runs in a Bubblewrap sandbox with a synthetic home, XDG directories,
Pi agent directory, session directory, and workspace. The parent environment is
cleared before the harness starts, the real home is not mounted, networking is
disabled, and a deterministic Pi stand-in is used. No Pi configuration,
sessions, credentials, or model access are required.

Edit `demo.tape` to change the sequence and `project/` to change the visible
workspace. Generated media is intentionally ignored by Git.
