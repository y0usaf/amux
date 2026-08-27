// Run-activity tracking: turns, active tools, recent results, durations.
// Pure state transitions over store.state.run; rendering lives in render.js.

const MAX_RECENT_TOOLS = 3
const MAX_SUMMARY_CHARS = 40

function toText(value) {
	return typeof value === "string" ? value : ""
}

function shortenPath(path, cwd) {
	if (!path) return ""
	if (cwd && path.startsWith(`${cwd}/`)) return path.slice(cwd.length + 1)
	const home = process.env.HOME
	if (home && path.startsWith(`${home}/`)) return `~/${path.slice(home.length + 1)}`
	return path
}

function firstLine(text) {
	const index = text.indexOf("\n")
	return index === -1 ? text : text.slice(0, index)
}

export function summarizeToolArgs(toolName, args, cwd) {
	if (!args || typeof args !== "object") return ""
	let summary = ""
	switch (toolName) {
		case "bash":
			summary = firstLine(toText(args.command))
			break
		case "read":
		case "write":
		case "edit":
			summary = shortenPath(toText(args.path || args.file_path), cwd)
			break
		case "grep":
		case "glob":
			summary = toText(args.pattern)
			break
		default: {
			const value = Object.values(args).find((entry) => typeof entry === "string" && entry.trim())
			summary = value ? firstLine(value) : ""
		}
	}
	summary = summary.trim()
	return summary.length > MAX_SUMMARY_CHARS ? `${summary.slice(0, MAX_SUMMARY_CHARS - 1)}…` : summary
}

export function formatDuration(durationMs) {
	const totalSeconds = Math.floor(Math.max(0, durationMs) / 1000)
	if (totalSeconds < 1) return "<1s"
	if (totalSeconds < 60) return `${totalSeconds}s`
	const minutes = Math.floor(totalSeconds / 60)
	const seconds = totalSeconds % 60
	if (minutes < 60) return `${minutes}m${String(seconds).padStart(2, "0")}s`
	const hours = Math.floor(minutes / 60)
	return `${hours}h${String(minutes % 60).padStart(2, "0")}m`
}

export function registerActivity(pi, store) {
	const now = () => Date.now()

	pi.on("agent_start", async () => {
		store.update((state) => {
			state.run.phase = "running"
			state.run.turn = 0
			state.run.startedAt = now()
			state.run.settledAt = 0
			state.run.activeTools = []
			state.run.recentTools = []
			state.run.doneCount = 0
			state.run.failedCount = 0
		})
	})

	pi.on("turn_start", async (event) => {
		store.update((state) => {
			if (state.run.phase !== "running") {
				state.run.phase = "running"
				state.run.startedAt = now()
			}
			const index = Number.isFinite(event?.turnIndex) ? Math.trunc(event.turnIndex) : state.run.turn
			state.run.turn = Math.max(state.run.turn, index + 1)
		})
	})

	pi.on("tool_execution_start", async (event, ctx) => {
		const id = toText(event.toolCallId)
		if (!id) return
		store.update((state) => {
			state.run.activeTools = state.run.activeTools.filter((tool) => tool.id !== id)
			state.run.activeTools.push({
				id,
				name: toText(event.toolName) || "tool",
				summary: summarizeToolArgs(event.toolName, event.args, ctx?.cwd ?? state.cwd),
				startedAt: now(),
			})
		})
	})

	pi.on("tool_execution_end", async (event) => {
		const id = toText(event.toolCallId)
		store.update((state) => {
			const active = state.run.activeTools.find((tool) => tool.id === id)
			state.run.activeTools = state.run.activeTools.filter((tool) => tool.id !== id)
			const failed = event.isError === true
			state.run.doneCount += failed ? 0 : 1
			state.run.failedCount += failed ? 1 : 0
			state.run.recentTools.unshift({
				name: active?.name ?? toText(event.toolName) ?? "tool",
				summary: active?.summary ?? "",
				durationMs: active ? now() - active.startedAt : 0,
				failed,
			})
			state.run.recentTools = state.run.recentTools.slice(0, MAX_RECENT_TOOLS)
		})
	})

	pi.on("agent_end", async () => {
		store.update((state) => {
			state.run.phase = "settled"
			state.run.settledAt = now()
			state.run.activeTools = []
		})
	})
}
