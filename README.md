# pi-harness

Minimal Pi terminal harness.

Layout:
- TUI only
- shared cell scene rendered to ANSI
- Neo-tree style left sidebar + unboxed Pi terminal + nvim-style bottom statusline/command row

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

- `:` → command line (`:open <dir>` adds/selects a project, `:archive` opens archive restore viewer, `:usage` opens Pi usage, `:refresh`, `:reload`, `:q` quits)
- click the `+` in the left statusline segment → opens command line prefilled as `:open `
- bottom statusline shows `NORMAL` / `COMMAND`; command mode keeps the nvim-style command row below it
- command line: `Enter` run, `Esc`/`Ctrl+C` cancel, arrows/Home/End edit; `::` sends a literal `:` to the terminal
- `Ctrl+N` → new session
- `Ctrl+R` → refresh selected idle session
- `Ctrl+Shift+R` → reload Pi sessions from disk
- `Ctrl+Delete` → archive selected session
- archive viewer: `↑`/`↓`/`j`/`k` select, `Enter` restores to original project cwd, `r` reloads, `q`/`Esc` closes
- usage overlay: usage totals table/tree by date/model with per-model costs; `r` reloads, `q`/`Esc` closes
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
  "sidebar_width": 36,
  "keybinds": {
    "project_prev": "ctrl+h",
    "project_next": "ctrl+l",
    "session_prev": "ctrl+k",
    "session_next": "ctrl+j"
  }
}
```

- `sidebar_width` sets the left sidebar width in terminal cells (`8..=120`); on narrow terminals it is reduced to preserve the main Pi area
- legacy `sidebar_width_percent` is still accepted as a fallback when `sidebar_width` is unset
- `terminal_width_percent` is accepted for config compatibility but ignored; the Pi terminal fills the remaining main area
- legacy `tui_sidebar_width_percent` is still accepted as a fallback
- missing `keybinds.*` → built-in defaults
- value shape = string or string array
- multi-stroke chords are space-separated, e.g. `"ctrl+p n"`
- key names use tokens like `left/right/up/down`, `delete`, `insert`, `equal`, `plus`, `minus`

## Notes

- Uses Pi session directory precedence: `PI_CODING_AGENT_SESSION_DIR`, then `${PI_CODING_AGENT_DIR:-~/.pi/agent}/sessions`
- Stores harness archives in `ARCHIVE` under resolved Pi session directory
- Launches Pi in a PTY
- Injects the bundled sidecar extension with `-e`
- Compact tool rendering lives in separate `pi-compact`; sidecar remains session/status bridge
- Sidecar snapshots update session name/runtime state in the sidebar + statusline
- Uses the host terminal grid/ANSI renderer, leaves harness chrome on the terminal's default theme, renders an unboxed main terminal with a Neo-tree style sidebar and dual bottom statusline/command row, uses inverse video for sidebar selection, renders sidebar/terminal scrollbars, supports mouse wheel + `Shift+PageUp/PageDown` scrolling, and exits with `Ctrl+Q`
