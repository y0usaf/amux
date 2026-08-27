// Rail panel rendering: pure functions from store state to styled lines.
// Visual language mirrors the harness left sidebar: caps panel headers with a
// crown jewel and braille spinner/notification glyphs.

import { truncateToWidth, visibleWidth } from "@earendil-works/pi-tui"

import { formatDuration } from "./activity.js"
import { createGlyphs } from "./symbols.js"

// Symbol set resolved once at module load from the harness env
// (AGENT_HARNESS_PI_ASCII / AGENT_HARNESS_SYMBOL_OVERRIDES).
const GLYPHS = createGlyphs()
const SPINNER_FRAME_MS = 60
const JEWEL_BLINK_MS = 400

const TOKEN = Object.freeze({
	text: "text",
	muted: "muted",
	heading: "mdHeading",
	accent: "accent",
	accent2: "borderAccent",
	running: "mdLink",
	warning: "warning",
	error: "error",
	success: "success",
	border: "borderMuted",
})

export function createPainter(theme) {
	return (role, text) => theme.fg(TOKEN[role] ?? "text", text)
}

function spinnerGlyph(nowMs) {
	return GLYPHS.spinner[Math.floor(nowMs / SPINNER_FRAME_MS) % GLYPHS.spinner.length]
}

function jewelGlyph(active, nowMs) {
	return active && Math.floor(nowMs / JEWEL_BLINK_MS) % 2 === 1 ? GLYPHS.jewelOpen : GLYPHS.jewel
}

function panelHeader(title, role, paint, width, active, nowMs) {
	const jewel = jewelGlyph(active, nowMs)
	const label = ` ${jewel} ${title} `
	const rule = GLYPHS.rule.repeat(Math.max(0, width - visibleWidth(label)))
	return `${paint(role, label)}${paint("border", rule)}`
}

function clip(line, width) {
	return visibleWidth(line) > width ? truncateToWidth(line, Math.max(0, width - 1)) + GLYPHS.clip : line
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
	const glyph = busy ? spinnerGlyph(nowMs) : GLYPHS.notif
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
		const mark = tool.failed ? paint("error", GLYPHS.err) : paint("success", GLYPHS.ok)
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
	const bar = paint(role, GLYPHS.fill.repeat(filled)) + paint("border", GLYPHS.empty.repeat(barWidth - filled))
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

// Tool roster packed into ls-style columns: names read down each column, so a
// 30-tool roster costs a handful of rows instead of 30. Cell width follows the
// longest name; when that would run past MAX_TOOL_ROWS rows, cells shrink
// (names truncate) down to MIN_TOOL_CELL, and anything still over the row
// budget is dropped into a `+n more` row like SESSIONS does.
const TOOL_GUTTER = 1
const MAX_TOOL_ROWS = 8
const MIN_TOOL_CELL = 10

function toolsLines(state, width, paint) {
	const tools = state.tools
	if (!Array.isArray(tools) || tools.length === 0) return []
	const natural = Math.max(...tools.map((tool) => visibleWidth(tool.name))) + 2 // "✓ " prefix
	const wanted = Math.max(1, Math.ceil(tools.length / MAX_TOOL_ROWS))
	const budget = Math.floor((width - 1 + TOOL_GUTTER) / wanted) - TOOL_GUTTER
	const cellWidth = Math.max(1, Math.min(natural, width - 1, Math.max(MIN_TOOL_CELL, budget)))
	const columns = Math.max(1, Math.floor((width - 1 + TOOL_GUTTER) / (cellWidth + TOOL_GUTTER)))

	// Trim the tail of the list before packing, never the tail of each column:
	// column-major order stays alphabetically readable top-to-bottom.
	const capacity = MAX_TOOL_ROWS * columns
	const hidden = Math.max(0, tools.length - capacity)
	const shown = hidden > 0 ? tools.slice(0, capacity) : tools
	const rows = Math.ceil(shown.length / columns)

	const cells = shown.map((tool) => {
		const marker = tool.active ? paint("success", GLYPHS.ok) : paint("border", GLYPHS.dot)
		return `${marker} ${paint(tool.active ? "text" : "muted", clip(tool.name, Math.max(1, cellWidth - 2)))}`
	})

	const lines = []
	for (let row = 0; row < rows; row++) {
		let line = ""
		for (let column = 0; column < columns; column++) {
			const cell = cells[column * rows + row]
			if (cell === undefined) continue
			line += pad(cell, cellWidth) + " ".repeat(TOOL_GUTTER)
		}
		lines.push(clip(` ${line.trimEnd()}`, width))
	}
	if (hidden > 0) lines.push(clip(` ${paint("muted", `+${hidden} more`)}`, width))
	return lines
}

// Extension statuses (ctx.ui.setStatus) rendered verbatim: the owning
// extension already styled them, so the rail only clips them to width.
function statusLines(state, width, paint) {
	const statuses = state.statuses
	if (!Array.isArray(statuses) || statuses.length === 0) return []
	return statuses.slice(0, 4).map((status) => clip(` ${paint("text", status)}`, width))
}

function digestGlyph(entry, paint, nowMs) {
	if (entry.stage && entry.stage !== "idle") return paint("running", spinnerGlyph(nowMs))
	if (entry.queued) return paint("warning", spinnerGlyph(nowMs))
	if (entry.interrupted) return paint("error", GLYPHS.notif)
	if (entry.unread) return paint("success", GLYPHS.notif)
	return paint("border", GLYPHS.dot)
}

function sessionsLines(state, width, paint, nowMs) {
	const digest = state.digest
	if (!Array.isArray(digest) || digest.length === 0) return []
	const selfKey = process.env.AGENT_HARNESS_PI_SESSION_KEY
	const lines = []
	for (const entry of digest.slice(0, 8)) {
		const self = selfKey && entry.key === selfKey
		const marker = self ? paint("accent2", GLYPHS.marker) : "  "
		const role = self ? "text" : entry.unread ? "heading" : "muted"
		const name = entry.name || "(unnamed)"
		lines.push(clip(`${marker}${digestGlyph(entry, paint, nowMs)} ${paint(role, name)}`, width))
	}
	if (digest.length > 8) {
		lines.push(clip(`    ${paint("muted", `+${digest.length - 8} more`)}`, width))
	}
	return lines
}

// Left-edge divider: a rail glyph in the terminal's default foreground, the
// same treatment as the harness sidebar rail. Default fg is the terminal's
// own high-contrast complement to its background, so the thin line adapts
// to any theme without knowing its colors.

export function renderRail(state, width, nowMs, rows = 0, theme) {
	const paint = createPainter(theme)
	const inner = Math.max(1, width - 1)
	const running = state.run.phase === "running"
	const panels = [
		{ title: "AGENT", role: stageWord(state).role, active: running, lines: agentLines(state, inner, paint, nowMs) },
		{ title: "ACTIVITY", role: "accent", active: running, lines: activityLines(state, inner, paint, nowMs) },
		{ title: "USAGE", role: "accent", active: false, lines: usageLines(state, inner, paint) },
		{ title: "CONTEXT", role: "accent2", active: false, lines: contextLines(state, inner, paint) },
		{ title: "WORKSPACE", role: "accent", active: false, lines: workspaceLines(state, inner, paint) },
		{ title: "TOOLS", role: "accent", active: false, lines: toolsLines(state, inner, paint) },
		{ title: "EXT", role: "accent", active: false, lines: statusLines(state, inner, paint) },
		{ title: "SESSIONS", role: "accent2", active: false, lines: sessionsLines(state, inner, paint, nowMs) },
	]

	const lines = []
	for (const panel of panels) {
		if (panel.lines.length === 0) continue
		if (lines.length > 0) lines.push("")
		lines.push(panelHeader(panel.title, panel.role, paint, inner, panel.active, nowMs))
		lines.push(...panel.lines)
	}
	// Run the divider the full terminal height, not just the content height.
	while (lines.length < rows) lines.push("")
	return lines.map((line) => GLYPHS.divider + pad(line, inner))
}
