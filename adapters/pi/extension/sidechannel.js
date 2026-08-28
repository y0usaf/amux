// Harness sidechannel bridge.
//
// Upstream (extension → harness): JSON-line session snapshots over the unix
// socket, unchanged wire format from the original harness-sidechannel.js.
// Downstream (harness → extension): `hello` (rail width) and
// `digest` (cross-session summary) lines consumed into the shared store.

import { readFileSync, statSync } from "node:fs"
import net from "node:net"
import { dirname } from "node:path"

const SOCKET_PATH = process.env.AGENT_HARNESS_PI_SIDECAR_SOCKET
const HARNESS_SESSION_ID = process.env.AGENT_HARNESS_PI_SESSION_KEY
const SNAPSHOT_TYPE = "snapshot"
const TITLE_POLL_MS = 250
const SESSION_TITLE_MAX_CHARS = 42
const THEME_FG_TOKENS = ["text", "muted", "mdHeading", "accent", "borderAccent", "borderMuted", null, null, null, "toolTitle", null, "mdLink", "success", "warning", "error"]
const THEME_BG_TOKENS = [null, null, null, null, null, null, "toolPendingBg", "selectedBg", null, null, "toolSuccessBg", null, null, null, null]

export function parseThemeAnsi(value, background = false) {
	const prefix = background ? "48" : "38"
	if (value === `\x1b[${background ? "49" : "39"}m`) return { kind: "default" }
	const rgb = new RegExp(`^\\x1b\\[${prefix};2;(\\d+);(\\d+);(\\d+)m$`).exec(value)
	if (rgb && rgb.slice(1).every((n) => Number(n) >= 0 && Number(n) <= 255)) return { kind: "rgb", r: Number(rgb[1]), g: Number(rgb[2]), b: Number(rgb[3]) }
	const ansi = new RegExp(`^\\x1b\\[${prefix};5;(\\d+)m$`).exec(value)
	if (ansi && Number(ansi[1]) <= 255) return { kind: "ansi", index: Number(ansi[1]) }
	return undefined
}

export function resolveTheme(ctx) {
	const theme = ctx.ui.theme
	return THEME_FG_TOKENS.map((token, i) => {
		const bgToken = THEME_BG_TOKENS[i]
		const parsed = token
			? parseThemeAnsi(theme.getFgAnsi(token), false)
			: bgToken
				? parseThemeAnsi(theme.getBgAnsi(bgToken), true)
				: { kind: "default" }
		return parsed || { kind: "default" }
	})
}
// Subagent sessions live inside the parent session's artifacts dir
// (`<parent>.jsonl` strips its `.jsonl` suffix to `<parent>/`, and a child
// writes `<parent>/<agentId>.jsonl`). Detecting that sibling file identifies
// this runner as a subagent and names its parent row.
export function parentSessionFileFromSessionFile(file) {
	if (!file) return undefined
	const parent = `${dirname(file)}.jsonl`
	try {
		return statSync(parent).isFile() ? parent : undefined
	} catch {
		return undefined
	}
}

function firstNonEmptyLine(text) {
	for (const line of `${text || ""}`.split("\n")) {
		if (line.trim()) return line.trim()
	}
	return ""
}

function truncateTitle(text, max = SESSION_TITLE_MAX_CHARS) {
	const chars = [...`${text || ""}`]
	return chars.length <= max ? chars.join("") : `${chars.slice(0, Math.max(1, max)).join("")}…`
}

function promptSessionName(text, images) {
	const body = `${text || ""}`
	if (!body.trim()) return Array.isArray(images) && images.length > 0 ? "Image" : undefined
	const firstLine = firstNonEmptyLine(body)
	if (!firstLine || firstLine.startsWith("# AGENTS.md instructions")) return undefined
	return truncateTitle(firstLine)
}

export function registerSidechannel(pi, store) {
	if (!SOCKET_PATH) return

	let socket = undefined
	let reconnectTimer = undefined
	let titlePoll = undefined
	let destroyed = false
	let sessionId = undefined
	let sessionFile = undefined
	let parentSessionFile = undefined
	let lastCtx
	let stage = "idle"
	let queued = false
	let toolName = undefined
	let interrupted = false
	let lastSnapshotKey = undefined
	let downstreamBuffer = ""
	const activeTools = new Map()
	let lastThemeKey
	const emitTheme = (ctx) => {
		// Subagent runners do not own the terminal chrome; the main runner's
		// theme line is authoritative.
		if (parentSessionFile) return
		if (!socket || socket.destroyed || !ctx?.ui?.theme) return
		const roles = resolveTheme(ctx), key = JSON.stringify(roles)
		if (key === lastThemeKey) return
		lastThemeKey = key
		socket.write(`${JSON.stringify({ type: "theme", roles })}\n`)
	}

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

	function stopReasonFromEvent(event) {
		for (const candidate of [
			event?.stopReason,
			event?.reason,
			event?.message?.stopReason,
			event?.assistantMessage?.stopReason,
		]) {
			if (typeof candidate === "string" && candidate.length > 0) return candidate
		}
		return undefined
	}

	function lastStopReasonFromSessionFile(path) {
		if (!path) return undefined
		try {
			let lastStopReason = undefined
			const content = readFileSync(path, "utf8")
			for (const line of content.split("\n")) {
				if (!line.includes("stopReason")) continue
				try {
					const value = JSON.parse(line)
					const stopReason = value?.message?.stopReason || value?.stopReason
					if (typeof stopReason === "string" && stopReason.length > 0) {
						lastStopReason = stopReason
					}
				} catch {
					// Ignore partial/corrupt lines while Pi is still writing the log.
				}
			}
			return lastStopReason
		} catch {
			return undefined
		}
	}

	function refreshInterruptedFromSessionFile() {
		const stopReason = lastStopReasonFromSessionFile(sessionFile)
		if (stopReason !== undefined) interrupted = stopReason === "aborted"
	}

	function rememberSessionContext(ctx) {
		if (!ctx) return
		lastCtx = ctx
		sessionId = ctx.sessionManager.getSessionId()
		sessionFile = ctx.sessionManager.getSessionFile()
		parentSessionFile = parentSessionFileFromSessionFile(sessionFile)
		queued = ctx.hasPendingMessages()
	}

	function mirrorToStore() {
		const state = store.state
		if (
			state.stage === stage &&
			state.queued === queued &&
			state.interrupted === interrupted &&
			state.sessionName === currentName()
		) {
			return
		}
		store.update((next) => {
			next.stage = stage
			next.queued = queued
			next.interrupted = interrupted
			next.sessionName = currentName()
		})
	}

	function handleDownstreamLine(line) {
		let message
		try {
			message = JSON.parse(line)
		} catch {
			return
		}
		if (!message || typeof message !== "object") return
		if (message.type === "hello") {
			store.update((state) => {
				state.harness = {
					railWidth: Number.isFinite(message.railWidth) ? message.railWidth : undefined,
				}
			})
		} else if (message.type === "digest" && Array.isArray(message.sessions)) {
			store.update((state) => {
				state.digest = message.sessions
			})
		}
	}

	function handleDownstreamData(chunk) {
		downstreamBuffer += chunk.toString("utf8")
		let index
		while ((index = downstreamBuffer.indexOf("\n")) !== -1) {
			const line = downstreamBuffer.slice(0, index).trim()
			downstreamBuffer = downstreamBuffer.slice(index + 1)
			if (line) handleDownstreamLine(line)
		}
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
				if (lastCtx) emitTheme(lastCtx)
			})
			socket.on("data", handleDownstreamData)
			socket.on("error", () => {
				socket?.destroy()
			})
			socket.on("close", () => {
				socket = undefined
				downstreamBuffer = ""
				scheduleReconnect()
			})
		} catch {
			socket = undefined
			scheduleReconnect()
		}
	}

	function emitSnapshot(ctx, force = false) {
		rememberSessionContext(ctx)
		mirrorToStore()
		if (!sessionId) return

		const snapshot = {
			type: SNAPSHOT_TYPE,
			sessionId,
			harnessSessionId: HARNESS_SESSION_ID,
			sessionFile,
			parentSessionFile,
			sessionName: currentName(),
			stage,
			queued,
			interrupted,
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
			emitTheme(ctx || lastCtx)
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
		rememberSessionContext(ctx)
		refreshInterruptedFromSessionFile()
		stage = ctx.isIdle() ? "idle" : "thinking"
		emitSnapshot(undefined, true)
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
		interrupted = false
		stage = activeTools.size > 0 ? "tool" : "thinking"
		toolName = activeTools.size > 0 ? anyToolName() : undefined
		emitSnapshot(ctx, true)
	})

	pi.on("agent_start", async (_event, ctx) => {
		interrupted = false
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

	pi.on("agent_end", async (event, ctx) => {
		clearRuntimeState()
		rememberSessionContext(ctx)
		const stopReason = stopReasonFromEvent(event)
		if (stopReason !== undefined) {
			interrupted = stopReason === "aborted"
		} else {
			refreshInterruptedFromSessionFile()
		}
		emitSnapshot(undefined, true)

		const refreshTimer = setTimeout(() => {
			if (stage !== "idle") return
			refreshInterruptedFromSessionFile()
			emitSnapshot(undefined, true)
		}, 50)
		refreshTimer.unref?.()
	})
}
