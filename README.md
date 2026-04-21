# pi-harness

Minimal native Pi desktop spike.

Layout:
- top bar
- left sidebar
- centered nested Pi terminal
- same monospace software renderer for chrome + terminal

No GPUI. No TUI framework. Minimal config surface.

## Code layout

```text
src/
  app/       # app orchestration + input/layout/sidebar/theme
  pi/        # Pi discovery/session scan/files/types
  render/    # color/frame/text/font backend
  sidecar/   # unix socket stream ingestion
  state/     # persisted state + project/session models + merge/sort
  terminal/  # controller + input/process/selection
  util/      # paths/text/time helpers
```

Notes:
- `src/lib.rs` is the canonical module root.
- `src/main.rs` is bootstrap only.

## Controls

- `Ctrl+N` → new session
- `Ctrl+=` / `Ctrl++` / `Cmd+=` / `Cmd++` → zoom in
- `Ctrl+-` / `Cmd+-` → zoom out
- `Ctrl+0` / `Cmd+0` → reset zoom
- `Ctrl/Cmd` + mouse wheel → zoom
- `Ctrl+O` → open project picker
- `Ctrl+Delete` → archive selected session
- `Ctrl+R` → reload Pi sessions from disk
- `Ctrl+Shift+Delete` / `Ctrl+Shift+D` → remove selected project
- `Ctrl+H` / `Ctrl+L` → prev/next project
- `Ctrl+K` / `Ctrl+J` → prev/next session
- drag in terminal → highlight text + auto-copy selection (`Clipboard` + Linux `Primary`)
- `Ctrl+Shift+C` / `Cmd+C` → copy current terminal selection
- `Ctrl+V` / `Cmd+V` / `Shift+Insert` → paste clipboard into terminal
- `Shift+PageUp` / `Shift+PageDown` → local terminal scrollback
- `Shift+Home` / `Shift+End` → top/bottom of local terminal scrollback

## Run

```bash
nix develop . --command cargo run -- /path/to/project
# or
nix run . -- /path/to/project
```

If no project path is passed, the app falls back to persisted projects, then current working directory.

## Config

`~/.config/pi-harness/config.json`

```json
{
  "ui_scale": 1.0,
  "font_family": null
}
```

- `font_family=null`/missing → system default monospace via fontconfig
- if set → prefer that family; missing symbols/glyphs still use fontconfig fallbacks

## Notes

- Uses Pi session discovery from `~/.pi/agent/sessions/...`
- Launches Pi in a PTY
- Injects the bundled sidecar extension with `-e`
- Sidecar snapshots update session name/runtime state in the sidebar + top bar
- UI/text scale is persisted in `$XDG_CONFIG_HOME/pi-harness/config.json` (`~/.config/pi-harness/config.json` fallback)
- `Ctrl+O` uses `zenity` for directory picking
- Current scope is intentionally narrow: project/session management + centered terminal + top bar only
