# pi-harness

pi-harness runs pi sessions in their own ekko instance: a sidebar of sessions
grouped by project, beside one full-size pi. It won't run anything but pi,
list past sessions (`pi --resume` does), or keep state of its own.

There is no multiplexer in this repo. `pi-harness` starts
[ekko](https://github.com/y0usaf/ekko) bare, as the instance `pi-harness`, with
the profile in `pi-harness.lisp`. ekko hosts the terminals, keeps them running
after you detach, and draws the sidebar the profile describes.

## Use

```sh
nix run .
```

`pi-harness` starts the instance with pi in the current directory, or attaches
to it if it is running. `pi-harness new [DIR]` opens a session without
attaching; inside the harness, plain `pi-harness` does the same. Other arguments
go to ekko: `pi-harness list`, `pi-harness stop`, `pi-harness config reload`.

| Key | Action |
| --- | --- |
| Alt-n | New session in the focused session's directory |
| Alt-r | `pi --resume` in that directory |
| Alt-j / Alt-k | Next / previous session |
| Alt-1 … Alt-9 | Session by its sidebar number |
| Alt-w | Close the focused session |
| Alt-o | Detach |

Click a session to focus it, or a project name to start a session there. The
wheel over the sidebar moves between sessions. Alt keys need a terminal that
speaks the Kitty keyboard protocol. ekko does, so pi-harness works inside an
ekko window too.

## Sidebar marks

`◐` working, `!` finished and waiting, `●` new output, `×` exited.

`status.js`, loaded into each session with `pi -e`, supplies the first two: a
`◐` title prefix while pi works, and an OSC 777 notification when it settles.

## In finix

```nix
pi-harness = {
  url = "git+ssh://git@github.com/y0usaf/amux.git?ref=main";
  inputs.nixpkgs.follows = "nixpkgs";
  inputs.ekko.follows = "ekko";
  inputs.pi-flake.follows = "pi-flake";
};
```

`nix flake check` loads the profile into the pinned ekko, starts the instance,
and checks that the sidebar is drawn.
