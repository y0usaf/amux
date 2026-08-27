# omp-harness

Minimal omp (oh-my-pi) terminal harness — the pi-harness TUI retargeted to
drive [omp](https://github.com/can1357/oh-my-pi) instead of Pi.

Layout:
- TUI only
- shared cell scene rendered to ANSI
- Neo-tree style left sidebar + unboxed omp terminal + nvim-style bottom statusline/command row

No GUI. No TUI framework. Minimal config surface.

## Code layout

```text
src/
  app/       # TUI runtime + app state + workspace/sidebar/sidecar components
  omp/       # omp discovery/session scan/files/types/usage
  render/    # color type
  sidecar/   # unix socket stream ingestion
  state/     # persisted state + project/session models + merge/sort
  terminal/  # controller + input/process/selection
  util/      # paths/text/time helpers
crates/
  omp-harness-tui/  # TUI binary crate (`omp-harness`, plus `omp-harness-tui` alias)
omp-extension/     # companion extension injected into every omp PTY
```

## The omp dependency

The flake depends on upstream `github:can1357/oh-my-pi`, so `nix run` pulls the
real `omp` binary into the closure — no system install, no version drift:

```bash
nix run . -- /path/to/project        # harness + omp, both from the flake
nix run .#omp -- /path/to/project    # bare omp passthrough
```

The default package wraps `bin/omp-harness` with the flake's `omp` on PATH;
discovery resolves it via `which()`. Overrides in discovery order:

1. config agent path (`~/.config/omp-harness/config.json`)
2. `$OMP_BINARY`
3. `which("omp")` — what the wrapper provides
4. well-known install locations (`.bun/bin/omp`, npm globals, mise shims, Nix profile)
5. `bunx`/`npx @oh-my-pi/pi-coding-agent`

## Controls

Identical to pi-harness:

- `:` → command line (`:open <dir>`, `:archive`, `:usage`, `:refresh`, `:reload`, `:q`)
- `Ctrl+N` new session · `Ctrl+R` refresh idle session · `Ctrl+Shift+R` reload sessions
- `Ctrl+Delete` archive session · archive viewer: `↑↓/jk` select, `Enter` restore, `r` reload, `q` close
- `:usage` → usage totals by date/model · `r` reload
- `Ctrl+Left/Right` prev/next project · `Ctrl+Up/Down` prev/next session
- drag → select + auto-copy · `Ctrl+Shift+C` copy · `Ctrl+V` / `Shift+Insert` paste (image paste saves temp file)
- `Shift+PageUp/PageDown`, `Shift+Home/End` local scrollback · `Ctrl+Q` quit

## Launch contract

Harness spawns omp in a PTY with:

- `-e <extension>` — the bundled companion extension (`omp-extension/index.js`)
- `--session <file>` — resume when reopening a session

`--tui-mode` was dropped: omp removed that flag upstream. The `tui_mode`
config key no longer exists.

Env forwarded into the PTY:

| Variable | Purpose |
| --- | --- |
| `AGENT_HARNESS_OMP_SIDECAR_SOCKET` | unix socket path for the sidecar bridge |
| `AGENT_HARNESS_OMP_SESSION_KEY` | harness-side session id |
| `AGENT_HARNESS_OMP_ASCII` | render rail/chrome with ASCII glyphs |
| `AGENT_HARNESS_SYMBOL_OVERRIDES` | JSON map of `rail.*` glyph overrides |

## Session directory resolution

Mirrors omp's own resolver so the sidebar sees live sessions:

1. `$PI_CODING_AGENT_SESSION_DIR` (honored natively by omp)
2. `$PI_CODING_AGENT_DIR/sessions`
3. `$XDG_DATA_HOME/omp/sessions` once `$XDG_DATA_HOME/omp` exists
4. `~/.omp/agent/sessions`

Harness archives live in `ARCHIVE/` under the resolved sessions root;
restores move files back to the encoded project directory.

The companion extension provides the sidechannel and activity bridge. Omp does
not load the Pi-only right rail extension, so the harness terminal remains the
sole owner of omp's layout and input path.

- **sidechannel** — session snapshots up to the harness (name, run state,
  context, usage), hello/digest lines down

The Pi-only right rail is intentionally not loaded by omp.

Verified against omp's extension API: `pi.getAllTools/getActiveTools`,
`pi.registerCommand`, `ctx.ui.custom/setStatus/theme`,
`session_start`/`before_agent_start`/`agent_*`/`turn_*`/`tool_execution_*`/
`message_update`/`session_shutdown` events, `getContextUsage`,
`modelRegistry.isUsingOAuth`.

Text primitives (`visibleWidth`, `truncateToWidth`) are self-contained in
`pi-tui-shim.js` (backed by `Bun.stringWidth`). omp's legacy-pi compat bundle
dropped the upstream exports (`HStack` first), and any static named import
from it fails the whole extension at ESM link time. The pi-atelier-style
fullscreen docking adapter was removed with them: omp dropped fullscreen mode.

Outside the harness (no `AGENT_HARNESS_OMP_SIDECAR_SOCKET`) the sidechannel
stays dormant and the rail renders fallback panels.

## Config

`~/.config/omp-harness/config.json`

```json
{
  "panel_width_percent": 22,
  "right_rail_width": 32,
  "ascii": false,
  "keybinds": {
    "project_prev": "ctrl+h",
    "project_next": "ctrl+l"
  }
}
```

- `panel_width_percent` — both bars share one percentage (5..=40, default 22)
- `sidebar_width` / `right_rail_width` — fixed cell-count overrides (rail 0 disables)
- `ascii` + `symbols.overrides` (`"rail.ok": "OK"`) — glyph control
- keybind names/strokes as in pi-harness; multi-stroke chords are space-separated

## Development

```bash
nix develop . --command cargo check --workspace --all-targets
nix develop . --command cargo test --workspace
nix develop . --command cargo run -p omp-harness-tui --bin omp-harness -- /path/to/project
```

Extension tests: `nix build .#checks.x86_64-linux.omp-extension-tests`
(node --test over `omp-extension/*.test.js`).

## Run

```bash
nix run . -- /path/to/project
# or explicit package:
nix run .#omp-harness -- /path/to/project
```

If no project path is passed, the app falls back to persisted projects, then
the current working directory.
