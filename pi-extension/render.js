// Rail panel rendering: pure functions from store state to styled lines.
// Visual language mirrors the harness left sidebar: caps panel headers with a
// crown jewel, braille spinner/notification glyphs, harness palette colours.

import { truncateToWidth, visibleWidth } from "@earendil-works/pi-tui"

import { formatDuration } from "./activity.js"

// Glyph set shared with src/app/sidebar.rs.
export const SPINNER_FRAMES = ["⠋", "⠙", "⠹", "⠸", "⢸", "⢰", "⣰", "⣠", "⣤", "⣄", "⣆", "⡆", "⡇", "⠇", "⠏"]
const SPINNER_FRAME_MS = 60
const NOTIFICATION_GLYPH = "⣿"
const JEWEL = "✦"
const JEWEL_OPEN = "✧"
const JEWEL_BLINK_MS = 400

const FALLBACK_PALETTE = {
	text: "#d0d0d0",
	muted: "#808080",
	heading: "#c0c0c0",
	accent: "#8888ff",
	accent2: "#88ccff",
	running: "#c8e64c",
	warning: "#e6c84c",
	error: "#e6604c",
	success: "#4ce688",
	border: "#404040",
}

function sgr(hex) {
	const value = /^#([0-9a-f]{6})$/i.exec(hex ?? "")?.[1]
	if (!value) return ""
	const r = Number.parseInt(value.slice(0, 2), 16)
	const g = Number.parseInt(value.slice(2, 4), 16)
	const b = Number.parseInt(value.slice(4, 6), 16)
	return `\u001b[38;2;${r};${g};${b}m`
}

const RESET = "\u001b[0m"

export function createPainter(palette) {
	const roles = { ...FALLBACK_PALETTE, ...(palette ?? {}) }
	const noColor = Boolean(process.env.NO_COLOR)
	return (role, text) => {
		if (noColor || !text) return text
		const open = sgr(roles[role] ?? roles.text)
		return open ? `${open}${text}${RESET}` : text
	}
}

function spinnerGlyph(nowMs) {
	return SPINNER_FRAMES[Math.floor(nowMs / SPINNER_FRAME_MS) % SPINNER_FRAMES.length]
}

function jewelGlyph(active, nowMs) {
	return active && Math.floor(nowMs / JEWEL_BLINK_MS) % 2 === 1 ? JEWEL_OPEN : JEWEL
}

function panelHeader(title, role, paint, width, active, nowMs) {
	const jewel = jewelGlyph(active, nowMs)
	const label = ` ${jewel} ${title} `
	const rule = "─".repeat(Math.max(0, width - visibleWidth(label)))
	return `${paint(role, label)}${paint("border", rule)}`
}

function clip(line, width) {
	return visibleWidth(line) > width ? truncateToWidth(line, Math.max(0, width - 1)) + "…" : line
}

function pad(text, columns) {
	const gap = columns - visibleWidth(text)
	return gap > 0 ? text + " ".repeat(gap) : text
}

function formatTokens(count) {
	const safe = Math.max(0, Number.isFinite(count) ? count : 0)
	if (safe < 1000) return String(safe)
	if (safe < 10_000) return `${(safe / 1000).toFixed(1)}k`
	if (safe < 1_000_000) return `${Math.round(safe / 1000)}k`
	return `${(safe / 1_000_000).toFixed(1)}M`
}

function stageWord(state) {
	if (state.run.phase === "running") {
		if (state.run.activeTools.length > 0) return { word: "TOOL", role: "running" }
		if (state.stage === "outputting") return { word: "WRITING", role: "running" }
		return { word: "THINKING", role: "running" }
	}
	if (state.queued) return { word: "QUEUED", role: "warning" }
	if (state.interrupted) return { word: "INTERRUPTED", role: "error" }
	return { word: "READY", role: "success" }
}

function agentLines(state, width, paint, nowMs) {
	const lines = []
	const stage = stageWord(state)
	const busy = state.run.phase === "running"
	const glyph = busy ? spinnerGlyph(nowMs) : NOTIFICATION_GLYPH
	lines.push(clip(` ${paint(stage.role, glyph)} ${paint(stage.role, stage.word)}`, width))
	if (state.model) {
		const think = state.thinkingLevel ? `  ${paint("muted", `think:${state.thinkingLevel}`)}` : ""
		lines.push(clip(` ${paint("text", state.model.id)}${think}`, width))
		const access = state.subscription ? "sub" : "api"
		lines.push(clip(` ${paint("muted", `${state.model.provider} · ${access}`)}`, width))
	}
	return lines
}

function activityLines(state, width, paint, nowMs) {
	const run = state.run
	if (run.phase !== "running" && run.recentTools.length === 0 && run.doneCount + run.failedCount === 0) {
		return []
	}
	const lines = []
	const elapsedBase = run.phase === "running" ? nowMs : run.settledAt || nowMs
	const elapsed = formatDuration(elapsedBase - (run.startedAt || elapsedBase))
	const turn = run.turn > 0 ? `turn ${run.turn}` : "turn –"
	lines.push(clip(` ${paint("text", turn)}  ${paint("muted", elapsed)}`, width))
	for (const tool of run.activeTools.slice(0, 4)) {
		const dur = formatDuration(nowMs - tool.startedAt)
		const summary = tool.summary ? ` ${paint("muted", tool.summary)}` : ""
		lines.push(clip(` ${paint("running", spinnerGlyph(nowMs))} ${paint("text", tool.name)}${summary} ${paint("muted", dur)}`, width))
	}
	for (const tool of run.recentTools) {
		const mark = tool.failed ? paint("error", "✗") : paint("success", "✓")
		const summary = tool.summary ? ` ${paint("muted", tool.summary)}` : ""
		lines.push(clip(` ${mark} ${paint("text", tool.name)}${summary} ${paint("muted", formatDuration(tool.durationMs))}`, width))
	}
	const failed = run.failedCount > 0 ? `  ${paint("error", `${run.failedCount} failed`)}` : ""
	lines.push(clip(` ${paint("muted", `${run.doneCount} done`)}${failed}`, width))
	return lines
}

function usageLines(state, width, paint) {
	const usage = state.usage
	if (!usage) return []
	const left = ` ${paint("muted", "in")} ${paint("text", formatTokens(usage.input))}  ${paint("muted", "out")} ${paint("text", formatTokens(usage.output))}`
	const cache = ` ${paint("muted", "cache")} ${paint("text", `${formatTokens(usage.cacheRead)}r/${formatTokens(usage.cacheWrite)}w`)}`
	const lines = [clip(left, width), clip(cache, width)]
	if (usage.costAvailable) {
		lines.push(clip(` ${paint("muted", "$")}${paint("text", usage.cost.toFixed(3))}${state.subscription ? paint("muted", " (sub)") : ""}`, width))
	}
	return lines
}

function contextLines(state, width, paint) {
	const context = state.context
	if (!context || !Number.isFinite(context.percent ?? Number.NaN)) return []
	const percent = Math.max(0, Math.min(100, context.percent))
	const barWidth = Math.max(4, width - 10)
	const filled = Math.round((percent / 100) * barWidth)
	const role = percent >= 90 ? "error" : percent >= 70 ? "warning" : "accent2"
	const bar = paint(role, "█".repeat(filled)) + paint("border", "░".repeat(barWidth - filled))
	const lines = [clip(` ${bar} ${paint("text", `${Math.round(percent)}%`)}`, width)]
	if (Number.isFinite(context.tokens ?? Number.NaN) && context.contextWindow > 0) {
		lines.push(clip(` ${paint("muted", `${formatTokens(context.tokens)} / ${formatTokens(context.contextWindow)}`)}`, width))
	}
	return lines
}

function workspaceLines(state, width, paint) {
	const lines = []
	const cwd = state.cwd?.split("/").filter(Boolean).pop() ?? ""
	if (cwd) lines.push(clip(` ${paint("text", cwd)}`, width))
	if (state.git?.branch) {
		const dirty = state.git.dirty ? paint("warning", " *") : ""
		lines.push(clip(` ${paint("muted", state.git.branch)}${dirty}`, width))
	}
	return lines
}

function digestGlyph(entry, paint, nowMs) {
	if (entry.stage && entry.stage !== "idle") return paint("running", spinnerGlyph(nowMs))
	if (entry.queued) return paint("warning", spinnerGlyph(nowMs))
	if (entry.interrupted) return paint("error", NOTIFICATION_GLYPH)
	if (entry.unread) return paint("success", NOTIFICATION_GLYPH)
	return paint("border", "·")
}

function sessionsLines(state, width, paint, nowMs) {
	const digest = state.digest
	if (!Array.isArray(digest) || digest.length === 0) return []
	const selfKey = process.env.AGENT_HARNESS_PI_SESSION_KEY
	const lines = []
	for (const entry of digest.slice(0, 8)) {
		const self = selfKey && entry.key === selfKey
		const marker = self ? paint("accent2", "▸ ") : "  "
		const role = self ? "text" : entry.unread ? "heading" : "muted"
		const name = entry.name || "(unnamed)"
		lines.push(clip(`${marker}${digestGlyph(entry, paint, nowMs)} ${paint(role, name)}`, width))
	}
	if (digest.length > 8) {
		lines.push(clip(`    ${paint("muted", `+${digest.length - 8} more`)}`, width))
	}
	return lines
}

// Left-edge divider: a reverse-video space renders as a solid bar in the
// inverse of whatever background sits underneath, so it adapts to any
// terminal theme without knowing its colors.
const DIVIDER = "\u001b[7m \u001b[27m"

export function renderRail(state, width, nowMs) {
	const paint = createPainter(state.harness?.palette)
	const inner = Math.max(1, width - 1)
	const running = state.run.phase === "running"
	const panels = [
		{ title: "AGENT", role: stageWord(state).role, active: running, lines: agentLines(state, inner, paint, nowMs) },
		{ title: "ACTIVITY", role: "accent", active: running, lines: activityLines(state, inner, paint, nowMs) },
		{ title: "USAGE", role: "accent", active: false, lines: usageLines(state, inner, paint) },
		{ title: "CONTEXT", role: "accent2", active: false, lines: contextLines(state, inner, paint) },
		{ title: "WORKSPACE", role: "accent", active: false, lines: workspaceLines(state, inner, paint) },
		{ title: "SESSIONS", role: "accent2", active: false, lines: sessionsLines(state, inner, paint, nowMs) },
	]

	const lines = []
	for (const panel of panels) {
		if (panel.lines.length === 0) continue
		if (lines.length > 0) lines.push("")
		lines.push(panelHeader(panel.title, panel.role, paint, inner, panel.active, nowMs))
		lines.push(...panel.lines)
	}
	return lines.map((line) => DIVIDER + pad(line, inner))
}
