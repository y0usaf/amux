# pi-harness

Minimal Pi terminal harness.

Layout:
- TUI only
- shared cell scene rendered to ANSI
- centered Pi terminal + sidebar + top bar

No GUI. No TUI framework. Minimal config surface.

## Code layout

```text
src/
  app/       # TUI runtime + app state + workspace/sidebar/sidecar components
  pi/        # Pi discovery/session scan/files/types
  render/    # color type
  sidecar/   # unix socket stream ingestion
  state/     # persisted state + project/session models + merge/sort
  terminal/  # controller + input/process/selection
  util/      # paths/text/time helpers
crates/
  pi-harness-tui/  # TUI binary crate (`pi-harness`, plus `pi-harness-tui` alias)
```

Notes:
- `src/lib.rs` is the shared library module root.
- TUI entrypoints live in `crates/pi-harness-tui/src/main*.rs`.

## Controls

- `:` → command line (`:open <dir>` adds/selects a project, `:q` quits)
- click `+ NEW PROJECT` → opens command line prefilled as `:open `
- command line: `Enter` run, `Esc`/`Ctrl+C` cancel, arrows/Home/End edit; `::` sends a literal `:` to the terminal
- `Ctrl+N` → new session
- `Ctrl+Delete` → archive selected session
- `Ctrl+R` → reload Pi sessions from disk
- `Ctrl+Shift+Delete` / `Ctrl+Shift+D` → remove selected project
- `Ctrl+Left` / `Ctrl+Right` → prev/next project
- `Ctrl+Up` / `Ctrl+Down` → prev/next session
- drag in terminal → highlight text + auto-copy selection (`Clipboard` + Linux `Primary`)
- `Ctrl+Shift+C` → copy current terminal selection
- `Ctrl+V` / `Shift+Insert` → paste clipboard text into terminal; image clipboard → saved to temp file + path pasted
- `Shift+PageUp` / `Shift+PageDown` → local terminal scrollback
- `Shift+Home` / `Shift+End` → top/bottom of local terminal scrollback

## Run

```bash
nix develop . --command cargo run --package pi-harness-tui --bin pi-harness -- /path/to/project
# or
nix run . -- /path/to/project
# explicit alias binary:
nix run .#pi-harness-tui -- /path/to/project
```

If no project path is passed, the app falls back to persisted projects, then current working directory.

## Config

`~/.config/pi-harness/config.json`

```json
{
  "terminal_width_percent": 50,
  "sidebar_width_percent": 13,
  "body_height_percent": 100,
  "keybinds": {
    "project_prev": "ctrl+h",
    "project_next": "ctrl+l",
    "session_prev": "ctrl+k",
    "session_next": "ctrl+j"
  }
}
```

- `terminal_width_percent` + `sidebar_width_percent` set shared cell UI widths as percentages of available columns; valid when `terminal > sidebar` and `terminal + sidebar*2 < 100`; unused space stays as margins, not flex fill
- `body_height_percent` sets sidebar + terminal height as a percentage of rows below the fixed top bar (`1..=100`); unused space stays below the body
- legacy `tui_*_percent` keys are still accepted as fallbacks
- missing `keybinds.*` → built-in defaults
- value shape = string or string array
- multi-stroke chords are space-separated, e.g. `"ctrl+p n"`
- key names use tokens like `left/right/up/down`, `delete`, `insert`, `equal`, `plus`, `minus`

## Notes

- Uses Pi session discovery from `~/.pi/agent/sessions/...`
- Launches Pi in a PTY
- Injects the bundled sidecar extension with `-e`
- Sidecar snapshots update session name/runtime state in the sidebar + top bar
- Uses the host terminal grid/ANSI renderer, leaves harness chrome on the terminal's default theme, centers the terminal panel, uses inverse video for sidebar selection, renders sidebar/terminal scrollbars, supports mouse wheel + `Shift+PageUp/PageDown` scrolling, and exits with `Ctrl+Q`
