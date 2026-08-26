// Symbol presets for the rail renderer, mirroring the harness chrome's GlyphSet
// (src/app/glyphs.rs). Unicode is the default; ASCII is the plain-terminal
// opt-in. Per-symbol overrides (addressed by canonical "rail.*" key) layer on
// top and win: config knobs travel here via env from the harness process
// (AGENT_HARNESS_PI_ASCII / AGENT_HARNESS_SYMBOL_OVERRIDES), so the extension
// never touches the sidecar wire format for glyphs.

export const PRESETS = {
	unicode: {
		spinner: ["⠋", "⠙", "⠹", "⠸", "⢸", "⢰", "⣰", "⣠", "⣤", "⣄", "⣆", "⡆", "⡇", "⠇", "⠏"],
		notif: "⣿",
		jewel: "✦",
		jewelOpen: "✧",
		divider: "│",
		rule: "─",
		marker: "▸ ",
		dot: "·",
		fill: "█",
		empty: "░",
		ok: "✓",
		err: "✗",
		clip: "…",
	},
	ascii: {
		spinner: ["-", "\\", "|", "/"],
		notif: "[!]",
		jewel: "*",
		jewelOpen: "*",
		divider: "|",
		rule: "-",
		marker: "> ",
		dot: ".",
		fill: "#",
		empty: "-",
		ok: "ok",
		err: "!!",
		clip: "...",
	},
}

function normalizeOverrideKeys(overrides) {
	const out = {}
	for (const [key, value] of Object.entries(overrides)) {
		out[key.replace(/^rail\./, "")] = value
	}
	return out
}

// Resolve the effective symbol set: pick base preset from the ASCII flag, then
// layer per-key overrides (canonical "rail.*" keys) on top. Invalid override
// JSON is ignored with a console warning rather than crashing the rail.
export function resolveSymbols(env = process.env) {
	const base = env.AGENT_HARNESS_PI_ASCII === "1" ? PRESETS.ascii : PRESETS.unicode
	const raw = env.AGENT_HARNESS_SYMBOL_OVERRIDES
	if (!raw) return base
	let overrides
	try {
		overrides = JSON.parse(raw)
	} catch {
		console.warn(`pi-harness: ignoring invalid AGENT_HARNESS_SYMBOL_OVERRIDES: ${raw}`)
		return base
	}
	if (!overrides || typeof overrides !== "object" || Array.isArray(overrides)) return base
	return { ...base, ...normalizeOverrideKeys(overrides) }
}

// Compact object consumed by the rail renderer. Resolved once at module load.
export function createGlyphs(env = process.env) {
	const s = resolveSymbols(env)
	return {
		spinner: s.spinner,
		notif: s.notif,
		jewel: s.jewel,
		jewelOpen: s.jewelOpen,
		divider: s.divider,
		rule: s.rule,
		marker: s.marker,
		dot: s.dot,
		fill: s.fill,
		empty: s.empty,
		ok: s.ok,
		err: s.err,
		clip: s.clip,
	}
}
