## Locked decisions

- **2026-07-30 — Background token ownership:** Background roles come from Pi's background tokens only (toolSuccessBg / userMessageBg / tool*Bg), never from foreground tokens, because Pi's theme has no statusbar background token and using `accent` produces a bright bar.
- **2026-07-30 — Terminal colour ownership:** The harness does not post-process colours Pi already chose; terminal attributes travel as attributes, and the terminal's default foreground stays the terminal's. Dim remains the accepted approximation because the renderer has no dim attribute; revisit when the renderer supports one.
- **2026-07-30 — Palette direction:** Pi → harness; Pi's active theme is the sole colour source for the in-Pi rail and harness chrome. `pi-extension/` reads public `ctx.ui.theme`, parses values from `getFgAnsi`/`getBgAnsi`, and sends a `theme` JSON-line over the existing socket; Rust applies it and repaints. Rejected: a smaller-looking Rust `settings.json`/theme-JSON reader; duplicating Pi's resolver (variable references, 256-colour conversion, terminal-default `""`, `auto:dark,light`, and discovery order) is about 200–250 lines that drift with Pi's schema.
- **2026-07-30 — SGR decoding:** Accept only truecolour `38;2;R;G;Bm`/`48;2;R;G;Bm`, 256-index `38;5;N`/`48;5;N`, and default `39m`/`49m`. The latter carries no value and maps to transparent/default, never a guessed colour. Rejected: accepting arbitrary SGR or guessing defaults, which makes terminal-default transparency non-deterministic.
- **2026-07-30 — Theme wire roles:** `text←text`, `muted←muted`, `heading←mdHeading`, `accent←accent`, `accent2←borderAccent`, `border←borderMuted`, `surface←toolPendingBg` (background), `surface_raised←selectedBg` (background), `sidebar_bg←default`, `status_fg←toolTitle`, `status_bg←toolSuccessBg` (background), `running←mdLink`, `success←success`, `warning←warning`, `error←error`.
- **2026-07-30 — Role vocabulary:** Collapse `DerivedTheme` to the 15 painted roles: `text`, `muted`, `heading`, `accent`, `accent_2`, `border`, `surface`, `surface_raised`, `sidebar_bg`, `statusbar_fg`, `statusbar_bg`, `running`, `success`, `warning`, `error`. Map to Pi tokens by name, except `heading→mdHeading`, `running→thinkingText`, `surface→toolPendingBg`, `surface_raised→selectedBg`, `statusbar_bg→accent`, and `border→border`; derive `accent_2` as `brighten(accent, 40)` and keep `sidebar_bg`/`term_bg` transparent. Rejected: retaining the 35-role Crush vocabulary, whose unused aliases increase schema and maintenance surface. Delete `src/app/theme/charmtone.rs` and `pantera.rs`.
- **2026-07-30 — Cell role identity:** Replace colour-sentinel role tags with an explicit role enum carried by cells. Pi themes can collide (Pi dark resolves `text`, `userMessageText`, `customMessageText`, and `toolTitle` to `#d4d4d4`), so `palette_color()` comparisons in `src/app/scene.rs` cannot safely dispatch. Rejected: preserving sentinel `Color` tags, which silently mis-dispatch on collisions.
- **2026-07-30 — Terminal palette:** Use `Color::ansi_index(0..15)` for the embedded terminal's 16 ANSI entries; Pi defines no ANSI tokens, so the host palette remains authoritative. Keep `term_bg` transparent to preserve host transparency. Rejected: importing Pi role colours into ANSI slots, which would take ownership from the host terminal.
- **2026-07-30 — Wire ownership:** Add extension → harness `theme` lines; retain `hello` with `railWidth` but remove its `palette`; leave unknown line types ignored on both ends. The rail calls `ctx.ui.theme.fg(...)` directly, deleting `FALLBACK_PALETTE` and the hand-rolled `sgr()` painter in `pi-extension/render.js`. Rejected: a second palette payload, which creates two colour authorities.
- **2026-07-30 — Rail tokens:** Map roles to Pi tokens: `text→text`, `muted→muted`, `heading→mdHeading`, `accent→accent`, `accent2→borderAccent`, `running→mdLink`, `warning→warning`, `error→error`, `success→success`, `border→borderMuted`. In the light theme, `mdHeading` equals `warning` and `borderAccent` equals `accent`; these collisions are tolerable because the roles already serve equivalent visual emphasis. Drop `NO_COLOR` handling to follow Pi exactly. Keep the divider unpainted so it uses the terminal's own contrast.
- **2026-07-30 — Theme updates:** Fingerprint the Pi token set on every rail render and resend only when it changes; Pi exposes no extension theme-change hook (`onThemeChange` is internal interactive-mode machinery). Rejected: a timer, which adds wakeups and can still miss the exact change boundary.
- **Undated — TUI surface:** TUI only, with no GUI and no TUI framework; one shared cell scene renders to ANSI. This matches the minimal host-grid design documented by `README.md`; a GUI/framework would add an unnecessary rendering owner.
- **Undated — Bar sizing:** Size both bars from one `panel_width_percent`; explicit per-bar overrides remain the documented compatibility exceptions. Rejected: independently calculated defaults, which let the left sidebar and in-Pi rail drift.
- **Undated — Footer handoff:** The visible rail takes over Pi's footer and returns it when hidden. Rejected: permanently suppressing Pi's footer, which loses Pi and extension status text when the rail is unavailable.
- **Undated — Compact tools:** Keep compact tool rendering in the separate `pi-compact` project, not this harness. Rejected: duplicating that renderer here, which couples session bridging to tool presentation.
- **Canon applicability:** `[[canon:no-privileged-path]]` is n/a: the harness has no plugin story; only Pi's extension surface exists. Reverses when a second in-Pi companion ships. `[[canon:daemon-thin-client]]` is n/a: one viewer owns one lifetime and no state outlives it; Pi's session directory and `~/.config/pi-harness` persist what matters. Reverses if sessions must stay warm without a viewer.

## Architecture

- `src/app/theme/` — **decision-making**: Pi-token role mapping, SGR/theme resolution, derived roles, transparent/default policy.
- `src/app/scene.rs` — **decision-making**: explicit cell roles and chrome/terminal colour selection.
- `src/app/rail_bridge.rs` — **decision-making / extension boundary**: harness → extension `hello`, `theme`, and `digest` payload construction.
- `src/app/` (`backend`, `layout`, `sidebar`, `status`, `workspace`, `tui`, reducers/sync, terminal manager) — **decision-making**: harness interaction, layout, rail policy, and state presentation.
- `src/app/cell_surface.rs`, `src/app/terminal_view.rs` — **machinery**: cell storage and terminal-screen traversal/rendering.
- `src/app/tui/` — **machinery**: event loop and overlays; consumes resolved decisions.
- `src/config/` — **decision-making**: config validation, sizing, and key bindings.
- `src/notify.rs` — **machinery**: cross-thread wakeup notifications.
- `src/pi/` — **machinery**: Pi discovery, session files, scans, types, and usage reads.
- `src/render/color.rs` — **machinery**: colour representation and ANSI emission.
- `src/sidecar.rs` — **machinery / extension boundary**: Unix socket lifecycle, JSON-lines ingestion, sticky downstream hello, and broadcasts.
- `src/sidecar/stream.rs` — **machinery**: inbound stream parsing.
- `src/state/` — **decision-making**: persisted project/session models, merge, sorting, and archive state.
- `src/terminal/` — **machinery**: PTY process, input, selection, and controller.
- `src/util/` — **machinery**: paths, text, and time helpers.
- `crates/pi-harness-tui/` — **machinery**: binary entrypoints and run bootstrap.
- `pi-extension/` — **decision-making**: Pi theme reads, rail/footer policy, event-derived store, and socket protocol; **machinery**: rail layout/rendering, activity collection, and JSON-line transport.

## Deferred

- Rust theme-JSON reader: omitted to avoid duplicating Pi's resolver; until the first post-PTY-attach `theme` line, harness chrome uses terminal defaults.
- Per-session divergent themes: project-level `.pi/themes` can disagree; last applied theme wins for now rather than adding session-scoped chrome state.
- Pi background tokens beyond `selectedBg` and `tool*Bg`: omitted because current chrome has no justified mapping.
- Light-theme-specific derived-role tuning: omitted until a concrete light-theme contrast case exists.

## Roadmap

1. **Rail inherits:** `test "$(rg 'FALLBACK_PALETTE|sgr\(' pi-extension | wc -l)" -eq 0 && test "$(rg 'palette' src/app/rail_bridge.rs | wc -l)" -eq 0`.
2. **Theme uplink:** `nix build .` exits zero; a connected extension sends a complete 15-role `theme` line and a changed active theme is applied without restarting the harness.
3. **Role collapse:** `test "$(rg 'charmtone|pantera' src | wc -l)" -eq 0 && nix flake check` exits zero.
4. **Fallback:** with no `theme` line, chrome renders on terminal defaults and remains readable on both light and dark terminal palettes.
