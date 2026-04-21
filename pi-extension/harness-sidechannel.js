import net from "node:net"
import { homedir } from "node:os"

import {
	createBashToolDefinition,
	createEditToolDefinition,
	createFindToolDefinition,
	createGrepToolDefinition,
	createLsToolDefinition,
	createReadToolDefinition,
	createWriteToolDefinition,
} from "@mariozechner/pi-coding-agent"
import { Text } from "@mariozechner/pi-tui"

const SOCKET_PATH = process.env.AGENT_HARNESS_PI_SIDECAR_SOCKET
const HARNESS_SESSION_ID = process.env.AGENT_HARNESS_PI_SESSION_KEY
const SNAPSHOT_TYPE = "snapshot"
const TITLE_POLL_MS = 250
const SESSION_TITLE_MAX_CHARS = 42
const INLINE_SUMMARY_KEY = "__agentHarnessInlineSummary"
const TOOL_OUTPUT_COLOR = "toolOutput"

const builtInToolDefinitions = new Map()

function createToolDefinitions(cwd) {
	return {
		read: createReadToolDefinition(cwd),
		bash: createBashToolDefinition(cwd),
		edit: createEditToolDefinition(cwd),
		write: createWriteToolDefinition(cwd),
		find: createFindToolDefinition(cwd),
		grep: createGrepToolDefinition(cwd),
		ls: createLsToolDefinition(cwd),
	}
}

function getToolDefinitions(cwd) {
	const key = cwd || process.cwd()
	let defs = builtInToolDefinitions.get(key)
	if (!defs) {
		defs = createToolDefinitions(key)
		builtInToolDefinitions.set(key, defs)
	}
	return defs
}

function shortenPath(path, cwd) {
	if (!path) return ""
	const home = homedir()
	let next = `${path}`
	if (home && next.startsWith(home)) next = `~${next.slice(home.length)}`
	if (cwd && next.startsWith(`${cwd}/`)) next = next.slice(cwd.length + 1)
	return next
}

function truncateInline(text, max = 96) {
	const line = `${text || ""}`.replace(/\s+/g, " ").trim()
	if (line.length <= max) return line
	return `${line.slice(0, Math.max(1, max - 1))}…`
}

function firstTextContent(result) {
	return result?.content?.find((item) => item?.type === "text")?.text || ""
}

function hasImageContent(result) {
	return result?.content?.some((item) => item?.type === "image")
}

function lineCount(text) {
	if (!text) return 0
	return `${text}`.split("\n").length
}

function nonEmptyLineCount(text) {
	if (!text) return 0
	return `${text}`
		.split("\n")
		.map((line) => line.trim())
		.filter(Boolean).length
}

function firstNonEmptyLine(text) {
	for (const line of `${text || ""}`.split("\n")) {
		if (line.trim()) return line.trim()
	}
	return ""
}

function truncateTitle(text, max = SESSION_TITLE_MAX_CHARS) {
	const chars = [...`${text || ""}`]
	if (chars.length <= max) return chars.join("")
	return `${chars.slice(0, Math.max(1, max)).join("")}…`
}

function promptSessionName(text, images) {
	const body = `${text || ""}`
	if (!body.trim()) return Array.isArray(images) && images.length > 0 ? "Image" : undefined
	const firstLine = firstNonEmptyLine(body)
	if (!firstLine || firstLine.startsWith("# AGENTS.md instructions")) return undefined
	return truncateTitle(firstLine)
}

function formatLineRange(offset, limit) {
	if (offset === undefined && limit === undefined) return ""
	const start = Number.isFinite(offset) ? offset : 1
	if (Number.isFinite(limit)) return `:${start}-${start + limit - 1}`
	return `:${start}`
}

function plural(count, noun) {
	return `${count} ${noun}${count === 1 ? "" : "s"}`
}

function setInlineSummary(context, summary) {
	const state = context?.state
	if (!state) return
	if (state[INLINE_SUMMARY_KEY] === summary) return
	state[INLINE_SUMMARY_KEY] = summary
	context.invalidate()
}

function inlineSummary(context) {
	return context?.state?.[INLINE_SUMMARY_KEY]
}

function inlineCallText(label, summary, theme, isError) {
	if (!summary) return label
	return `${label}${theme.fg(isError ? "error" : "muted", ` · ${summary}`)}`
}

function textComponent(text, lastComponent) {
	const node = lastComponent instanceof Text ? lastComponent : new Text("", 0, 0)
	node.setText(text)
	return node
}

function renderExpandedText(text, theme, color = TOOL_OUTPUT_COLOR, lastComponent) {
	if (!text) return textComponent("", lastComponent)
	const body = `${text}`
		.split("\n")
		.map((line) => theme.fg(color, line))
		.join("\n")
	return textComponent(body ? `\n${body}` : "", lastComponent)
}

function renderExpandedDiff(diff, theme, lastComponent) {
	if (!diff) return textComponent("", lastComponent)
	const body = `${diff}`
		.split("\n")
		.map((line) => {
			if (line.startsWith("+++") || line.startsWith("@@")) return theme.fg("muted", line)
			if (line.startsWith("+") && !line.startsWith("+++")) return theme.fg("success", line)
			if (line.startsWith("-") && !line.startsWith("---")) return theme.fg("error", line)
			return theme.fg(TOOL_OUTPUT_COLOR, line)
		})
		.join("\n")
	return textComponent(body ? `\n${body}` : "", lastComponent)
}

function countDiffLines(diff) {
	let additions = 0
	let removals = 0
	for (const line of `${diff || ""}`.split("\n")) {
		if (line.startsWith("+") && !line.startsWith("+++")) additions += 1
		if (line.startsWith("-") && !line.startsWith("---")) removals += 1
	}
	return { additions, removals }
}

const compactToolsRegistered = new Set()

function shouldOverrideBuiltInTool(pi, name) {
	if (typeof pi?.getAllTools !== "function") return false
	const existing = pi.getAllTools().find((tool) => tool?.name === name)
	return existing?.sourceInfo?.source === "builtin"
}

function registerCompactTool(pi, name, handlers) {
	if (compactToolsRegistered.has(name) || !shouldOverrideBuiltInTool(pi, name)) return
	const base = getToolDefinitions(process.cwd())[name]
	pi.registerTool({
		...base,
		async execute(toolCallId, params, signal, onUpdate, ctx) {
			const defs = getToolDefinitions(ctx?.cwd || process.cwd())
			return defs[name].execute(toolCallId, params, signal, onUpdate, ctx)
		},
		renderCall: handlers.renderCall,
		renderResult: handlers.renderResult,
	})
	compactToolsRegistered.add(name)
}

function registerCompactBuiltInRenderers(pi) {
	registerCompactTool(pi, "read", {
		renderCall(args, theme, context) {
			const path = shortenPath(args.path || "", context.cwd)
			const range = formatLineRange(args.offset, args.limit)
			const label = `${theme.fg("toolTitle", theme.bold("read"))} ${theme.fg("accent", path || "…")}${theme.fg("warning", range)}`
			return textComponent(inlineCallText(label, !context.expanded ? inlineSummary(context) : undefined, theme, context.isError), context.lastComponent)
		},
		renderResult(result, options, theme, context) {
			if (options.isPartial) {
				setInlineSummary(context, undefined)
				return textComponent(options.expanded ? theme.fg("muted", "reading…") : "", context.lastComponent)
			}

			if (hasImageContent(result)) {
				setInlineSummary(context, "image")
				return textComponent(options.expanded ? theme.fg("muted", "image") : "", context.lastComponent)
			}

			const text = firstTextContent(result)
			const summary = context.isError
				? truncateInline(firstNonEmptyLine(text) || "read failed", 120)
				: (() => {
					const count = lineCount(text)
					const truncated = result?.details?.truncation?.truncated ? ", trunc" : ""
					return count > 0 ? `${plural(count, "line")}${truncated}` : "ok"
				})()
			setInlineSummary(context, summary)

			if (!options.expanded) return textComponent("", context.lastComponent)
			return renderExpandedText(text, theme, context.isError ? "error" : TOOL_OUTPUT_COLOR, context.lastComponent)
		},
	})

	registerCompactTool(pi, "bash", {
		renderCall(args, theme, context) {
			const command = truncateInline(args.command || "…", 120)
			const timeout = Number.isFinite(args.timeout) ? theme.fg("muted", ` (${args.timeout}s)`) : ""
			const label = `${theme.fg("toolTitle", theme.bold("$"))} ${theme.fg("accent", command)}${timeout}`
			return textComponent(inlineCallText(label, !context.expanded ? inlineSummary(context) : undefined, theme, context.isError), context.lastComponent)
		},
		renderResult(result, options, theme, context) {
			if (options.isPartial) {
				setInlineSummary(context, undefined)
				return textComponent(options.expanded ? theme.fg("muted", "running…") : "", context.lastComponent)
			}

			const text = firstTextContent(result)
			const details = result?.details || {}
			const lines = nonEmptyLineCount(text)
			const exitMatch = text.match(/exited with code\s+(\d+)/i)
			let summary = "ok"
			if (context.isError) {
				summary = exitMatch ? `exit ${exitMatch[1]}` : truncateInline(firstNonEmptyLine(text) || "command failed", 96)
			} else if (lines > 0) {
				summary = plural(lines, "line")
			}
			if (details.truncation?.truncated) summary += ", trunc"
			setInlineSummary(context, summary)

			if (!options.expanded) return textComponent("", context.lastComponent)
			return renderExpandedText(text, theme, context.isError ? "error" : TOOL_OUTPUT_COLOR, context.lastComponent)
		},
	})

	registerCompactTool(pi, "edit", {
		renderCall(args, theme, context) {
			const path = shortenPath(args.path || "", context.cwd)
			const editCount = Array.isArray(args.edits) ? args.edits.length : 0
			const suffix = editCount > 0 ? theme.fg("muted", ` (${plural(editCount, "edit")})`) : ""
			const label = `${theme.fg("toolTitle", theme.bold("edit"))} ${theme.fg("accent", path || "…")}${suffix}`
			return textComponent(inlineCallText(label, !context.expanded ? inlineSummary(context) : undefined, theme, context.isError), context.lastComponent)
		},
		renderResult(result, options, theme, context) {
			if (options.isPartial) {
				setInlineSummary(context, undefined)
				return textComponent(options.expanded ? theme.fg("muted", "editing…") : "", context.lastComponent)
			}

			const diff = result?.details?.diff || ""
			const text = firstTextContent(result)
			const summary = context.isError
				? truncateInline(firstNonEmptyLine(text) || "edit failed", 120)
				: (() => {
					if (!diff) return "applied"
					const stats = countDiffLines(diff)
					return `+${stats.additions}/-${stats.removals}`
				})()
			setInlineSummary(context, summary)

			if (!options.expanded) return textComponent("", context.lastComponent)
			if (context.isError) return renderExpandedText(text, theme, "error", context.lastComponent)
			return renderExpandedDiff(diff || text, theme, context.lastComponent)
		},
	})

	registerCompactTool(pi, "write", {
		renderCall(args, theme, context) {
			const path = shortenPath(args.path || "", context.cwd)
			const lines = lineCount(args.content || "")
			const suffix = lines > 0 ? theme.fg("muted", ` (${plural(lines, "line")})`) : ""
			const label = `${theme.fg("toolTitle", theme.bold("write"))} ${theme.fg("accent", path || "…")}${suffix}`
			return textComponent(inlineCallText(label, !context.expanded ? inlineSummary(context) : undefined, theme, context.isError), context.lastComponent)
		},
		renderResult(result, options, theme, context) {
			if (options.isPartial) {
				setInlineSummary(context, undefined)
				return textComponent(options.expanded ? theme.fg("muted", "writing…") : "", context.lastComponent)
			}

			const text = firstTextContent(result)
			const summary = context.isError
				? truncateInline(firstNonEmptyLine(text) || "write failed", 120)
				: "written"
			setInlineSummary(context, summary)

			if (!options.expanded) return textComponent("", context.lastComponent)
			return renderExpandedText(text, theme, context.isError ? "error" : TOOL_OUTPUT_COLOR, context.lastComponent)
		},
	})

	registerCompactTool(pi, "find", {
		renderCall(args, theme, context) {
			const pattern = truncateInline(args.pattern || "*", 64)
			const path = shortenPath(args.path || ".", context.cwd)
			const label = `${theme.fg("toolTitle", theme.bold("find"))} ${theme.fg("accent", pattern)}${theme.fg("muted", ` in ${path}`)}`
			return textComponent(inlineCallText(label, !context.expanded ? inlineSummary(context) : undefined, theme, context.isError), context.lastComponent)
		},
		renderResult(result, options, theme, context) {
			if (options.isPartial) {
				setInlineSummary(context, undefined)
				return textComponent(options.expanded ? theme.fg("muted", "searching…") : "", context.lastComponent)
			}

			const text = firstTextContent(result)
			const count = nonEmptyLineCount(text)
			let summary = context.isError ? truncateInline(firstNonEmptyLine(text) || "find failed", 120) : plural(count, "file")
			if (!context.isError && (result?.details?.resultLimitReached || result?.details?.truncation?.truncated)) {
				summary += ", limit"
			}
			setInlineSummary(context, summary)

			if (!options.expanded) return textComponent("", context.lastComponent)
			return renderExpandedText(text, theme, context.isError ? "error" : TOOL_OUTPUT_COLOR, context.lastComponent)
		},
	})

	registerCompactTool(pi, "grep", {
		renderCall(args, theme, context) {
			const pattern = truncateInline(args.pattern || "", 64)
			const path = shortenPath(args.path || ".", context.cwd)
			const label = `${theme.fg("toolTitle", theme.bold("grep"))} ${theme.fg("accent", `/${pattern}/`)}${theme.fg("muted", ` in ${path}`)}`
			return textComponent(inlineCallText(label, !context.expanded ? inlineSummary(context) : undefined, theme, context.isError), context.lastComponent)
		},
		renderResult(result, options, theme, context) {
			if (options.isPartial) {
				setInlineSummary(context, undefined)
				return textComponent(options.expanded ? theme.fg("muted", "searching…") : "", context.lastComponent)
			}

			const text = firstTextContent(result)
			const count = nonEmptyLineCount(text)
			let summary = context.isError ? truncateInline(firstNonEmptyLine(text) || "grep failed", 120) : plural(count, "match")
			if (!context.isError && (result?.details?.matchLimitReached || result?.details?.truncation?.truncated || result?.details?.linesTruncated)) {
				summary += ", limit"
			}
			setInlineSummary(context, summary)

			if (!options.expanded) return textComponent("", context.lastComponent)
			return renderExpandedText(text, theme, context.isError ? "error" : TOOL_OUTPUT_COLOR, context.lastComponent)
		},
	})

	registerCompactTool(pi, "ls", {
		renderCall(args, theme, context) {
			const path = shortenPath(args.path || ".", context.cwd)
			const label = `${theme.fg("toolTitle", theme.bold("ls"))} ${theme.fg("accent", path)}`
			return textComponent(inlineCallText(label, !context.expanded ? inlineSummary(context) : undefined, theme, context.isError), context.lastComponent)
		},
		renderResult(result, options, theme, context) {
			if (options.isPartial) {
				setInlineSummary(context, undefined)
				return textComponent(options.expanded ? theme.fg("muted", "listing…") : "", context.lastComponent)
			}

			const text = firstTextContent(result)
			const count = nonEmptyLineCount(text)
			let summary = context.isError ? truncateInline(firstNonEmptyLine(text) || "ls failed", 120) : plural(count, "entry")
			if (!context.isError && (result?.details?.entryLimitReached || result?.details?.truncation?.truncated)) {
				summary += ", limit"
			}
			setInlineSummary(context, summary)

			if (!options.expanded) return textComponent("", context.lastComponent)
			return renderExpandedText(text, theme, context.isError ? "error" : TOOL_OUTPUT_COLOR, context.lastComponent)
		},
	})
}

function registerSidechannel(pi) {
	if (!SOCKET_PATH) return

	let socket = undefined
	let reconnectTimer = undefined
	let titlePoll = undefined
	let destroyed = false
	let sessionId = undefined
	let sessionFile = undefined
	let stage = "idle"
	let queued = false
	let toolName = undefined
	let lastSnapshotKey = undefined
	const activeTools = new Map()

	function currentName() {
		const name = pi.getSessionName?.()
		return typeof name === "string" && name.trim().length > 0 ? name.trim() : undefined
	}

	function maybeSetSessionName(prompt, images) {
		if (typeof pi.setSessionName !== "function" || currentName()) return false
		const name = promptSessionName(prompt, images)
		if (!name) return false
		pi.setSessionName(name)
		return true
	}

	function anyToolName() {
		return activeTools.values().next().value
	}

	function stageFromAssistantEvent(eventType) {
		if (eventType.startsWith("thinking")) return "thinking"
		if (eventType.startsWith("text") || eventType.startsWith("toolcall")) return "outputting"
		return stage
	}

	function scheduleReconnect() {
		if (destroyed || reconnectTimer) return
		reconnectTimer = setTimeout(() => {
			reconnectTimer = undefined
			ensureSocket()
		}, 2000)
		reconnectTimer.unref?.()
	}

	function ensureSocket() {
		if (destroyed || socket) return
		try {
			socket = net.createConnection(SOCKET_PATH)
			socket.setNoDelay(true)
			socket.on("connect", () => {
				emitSnapshot(undefined, true)
			})
			socket.on("error", () => {
				socket?.destroy()
			})
			socket.on("close", () => {
				socket = undefined
				scheduleReconnect()
			})
		} catch {
			socket = undefined
			scheduleReconnect()
		}
	}

	function emitSnapshot(ctx, force = false) {
		if (ctx) {
			sessionId = ctx.sessionManager.getSessionId()
			sessionFile = ctx.sessionManager.getSessionFile()
			queued = ctx.hasPendingMessages()
		}
		if (!sessionId) return

		const snapshot = {
			type: SNAPSHOT_TYPE,
			sessionId,
			harnessSessionId: HARNESS_SESSION_ID,
			sessionFile,
			sessionName: currentName(),
			stage,
			queued,
			toolName,
		}
		const snapshotKey = JSON.stringify(snapshot)
		if (!force && snapshotKey === lastSnapshotKey) return
		lastSnapshotKey = snapshotKey

		const payload = JSON.stringify({
			...snapshot,
			tsMs: Date.now(),
		})
		ensureSocket()
		if (socket && !socket.destroyed && socket.writable) {
			socket.write(`${payload}\n`)
		}
	}

	function clearRuntimeState() {
		activeTools.clear()
		toolName = undefined
		queued = false
		stage = "idle"
	}

	pi.on("session_start", async (_event, ctx) => {
		clearRuntimeState()
		stage = ctx.isIdle() ? "idle" : "thinking"
		emitSnapshot(ctx, true)
		if (!titlePoll) {
			titlePoll = setInterval(() => emitSnapshot(), TITLE_POLL_MS)
			titlePoll.unref?.()
		}
	})

	pi.on("session_shutdown", async (_event, ctx) => {
		destroyed = true
		clearRuntimeState()
		emitSnapshot(ctx, true)
		if (titlePoll) {
			clearInterval(titlePoll)
			titlePoll = undefined
		}
		if (reconnectTimer) {
			clearTimeout(reconnectTimer)
			reconnectTimer = undefined
		}
		if (socket && !socket.destroyed) {
			socket.end()
		}
	})

	pi.on("input", async (_event, ctx) => {
		if (!ctx.isIdle()) {
			queued = true
			emitSnapshot(ctx, true)
		}
		return { action: "continue" }
	})

	pi.on("before_agent_start", async (event, ctx) => {
		maybeSetSessionName(event.prompt, event.images)
		stage = activeTools.size > 0 ? "tool" : "thinking"
		toolName = activeTools.size > 0 ? anyToolName() : undefined
		emitSnapshot(ctx, true)
	})

	pi.on("agent_start", async (_event, ctx) => {
		stage = activeTools.size > 0 ? "tool" : "thinking"
		toolName = activeTools.size > 0 ? anyToolName() : undefined
		emitSnapshot(ctx, true)
	})

	pi.on("turn_start", async (_event, ctx) => {
		if (activeTools.size === 0) {
			stage = "thinking"
			toolName = undefined
			emitSnapshot(ctx, false)
		}
	})

	pi.on("message_update", async (event, ctx) => {
		if (activeTools.size > 0) return
		const nextStage = stageFromAssistantEvent(event.assistantMessageEvent.type)
		if (nextStage !== stage) {
			stage = nextStage
			emitSnapshot(ctx, false)
		}
	})

	pi.on("tool_execution_start", async (event, ctx) => {
		activeTools.set(event.toolCallId, event.toolName)
		stage = "tool"
		toolName = event.toolName
		emitSnapshot(ctx, true)
	})

	pi.on("tool_execution_update", async (event, ctx) => {
		if (!activeTools.has(event.toolCallId)) {
			activeTools.set(event.toolCallId, event.toolName)
		}
		if (stage !== "tool" || toolName !== event.toolName) {
			stage = "tool"
			toolName = event.toolName
			emitSnapshot(ctx, false)
		}
	})

	pi.on("tool_execution_end", async (event, ctx) => {
		activeTools.delete(event.toolCallId)
		if (activeTools.size > 0) {
			stage = "tool"
			toolName = anyToolName()
		} else {
			toolName = undefined
			stage = ctx.isIdle() ? "idle" : "thinking"
		}
		emitSnapshot(ctx, true)
	})

	pi.on("agent_end", async (_event, ctx) => {
		clearRuntimeState()
		emitSnapshot(ctx, true)
	})
}

export default function (pi) {
	let compactRenderersInitialized = false
	pi.on("session_start", async () => {
		if (compactRenderersInitialized) return
		registerCompactBuiltInRenderers(pi)
		compactRenderersInitialized = true
	})
	registerSidechannel(pi)
}
