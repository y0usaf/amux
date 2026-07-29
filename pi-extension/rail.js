// Right rail: persistent non-capturing overlay anchored top-right, with a
// clean-room render wrap so Pi reflows into the remaining columns.
//
// The wrap is the only version-sensitive integration: TUI.render(width) is
// called with the terminal width minus the reserved rail columns. On any
// wrap failure the rail disables itself and Pi renders full width again.

import { renderRail } from "./render.js"

const DEFAULT_RAIL_WIDTH = 44
const MIN_MAIN_WIDTH = 64
const ANIMATION_MS = 250

export function registerRail(pi, store) {
	let started = false
	let enabled = true
	let wrapBroken = false
	let tuiRef
	let animationTimer

	const railWidth = () => {
		const width = store.state.harness?.railWidth
		return Number.isFinite(width) ? width : DEFAULT_RAIL_WIDTH
	}

	const visibleAt = (terminalWidth) => {
		const width = railWidth()
		return (
			enabled &&
			!wrapBroken &&
			width > 0 &&
			Number.isFinite(terminalWidth) &&
			terminalWidth >= width + MIN_MAIN_WIDTH
		)
	}

	const overlayOptions = {
		anchor: "top-right",
		width: DEFAULT_RAIL_WIDTH,
		maxHeight: "100%",
		margin: 0,
		nonCapturing: true,
		visible: (terminalWidth) => visibleAt(terminalWidth),
	}

	const requestRender = () => tuiRef?.requestRender()

	function attach(tui) {
		if (tuiRef) return
		tuiRef = tui
		const previousRender = tui.render
		tui.render = function wrappedRender(terminalWidth) {
			const reserved = visibleAt(terminalWidth) ? railWidth() : 0
			overlayOptions.width = reserved > 0 ? reserved : railWidth() || DEFAULT_RAIL_WIDTH
			try {
				return previousRender.call(tui, terminalWidth - reserved)
			} catch (error) {
				wrapBroken = true
				return previousRender.call(tui, terminalWidth)
			}
		}
	}

	function animationNeeded() {
		if (store.state.run.phase === "running") return true
		const digest = store.state.digest
		return Array.isArray(digest) && digest.some((entry) => (entry.stage && entry.stage !== "idle") || entry.queued)
	}

	function launch(ctx) {
		if (started || ctx.mode !== "tui" || !ctx.hasUI) return
		started = true

		void ctx.ui
			.custom(
				(tui) => {
					attach(tui)
					return {
						render(width) {
							return renderRail(store.state, Math.max(1, width), Date.now())
						},
					}
				},
				{ overlay: true, overlayOptions },
			)
			.catch(() => {
				wrapBroken = true
			})

		store.subscribe(requestRender)
		animationTimer = setInterval(() => {
			if (animationNeeded()) requestRender()
		}, ANIMATION_MS)
		animationTimer.unref?.()
	}

	function refreshFromCtx(ctx) {
		try {
			const model = ctx.model
			const context = ctx.getContextUsage?.()
			let usage
			const messages = ctx.sessionManager?.getEntries?.() ?? []
			let input = 0
			let output = 0
			let cacheRead = 0
			let cacheWrite = 0
			let cost = 0
			let usageSeen = false
			let costAvailable = false
			for (const entry of messages) {
				if (entry.type !== "message" || entry.message?.role !== "assistant") continue
				const u = entry.message.usage
				if (!u || typeof u !== "object") continue
				usageSeen = true
				input += Number.isFinite(u.input) ? u.input : 0
				output += Number.isFinite(u.output) ? u.output : 0
				cacheRead += Number.isFinite(u.cacheRead) ? u.cacheRead : 0
				cacheWrite += Number.isFinite(u.cacheWrite) ? u.cacheWrite : 0
				if (Number.isFinite(u.cost?.total)) {
					cost += u.cost.total
					costAvailable = true
				}
			}
			if (usageSeen) usage = { input, output, cacheRead, cacheWrite, cost, costAvailable }

			store.update((state) => {
				state.model = model ? { id: model.id, provider: model.provider } : undefined
				state.thinkingLevel = pi.getThinkingLevel?.()
				state.subscription = Boolean(model && ctx.modelRegistry?.isUsingOAuth?.(model))
				state.usage = usage
				state.context = context ?? undefined
				state.cwd = ctx.cwd ?? state.cwd
			})
		} catch {
			// Refresh is best-effort; the rail keeps rendering the last state.
		}
	}

	async function refreshGit(ctx) {
		try {
			const result = await pi.exec("git", ["status", "--short", "--branch", "--untracked-files=no"], {
				timeout: 2000,
			})
			if (result.code !== 0) return
			const lines = String(result.stdout ?? "").split(/\r?\n/).filter(Boolean)
			const header = lines[0]?.startsWith("## ") ? lines[0].slice(3).trim() : ""
			const rawBranch = header.split("...")[0]?.trim() ?? ""
			const unborn = rawBranch.match(/^No commits yet on (.+)$/)?.[1]?.trim()
			const branch = rawBranch === "HEAD (no branch)" ? "detached" : (unborn ?? rawBranch)
			store.update((state) => {
				state.git = {
					branch: branch || undefined,
					dirty: lines.some((line) => !line.startsWith("## ")),
				}
			})
		} catch {
			// Missing git or non-repo cwd leaves the panel row absent.
		}
	}

	pi.registerCommand("rail", {
		description: "Toggle the harness right rail (on|off)",
		handler: async (args, ctx) => {
			const argument = String(args ?? "").trim()
			if (argument === "on") enabled = true
			else if (argument === "off") enabled = false
			else enabled = !enabled
			ctx.ui?.notify?.(`rail ${enabled ? "on" : "off"}`, "info")
			requestRender()
		},
	})

	pi.on("session_start", async (_event, ctx) => {
		launch(ctx)
		refreshFromCtx(ctx)
		void refreshGit(ctx)
	})

	pi.on("turn_start", async (_event, ctx) => refreshFromCtx(ctx))
	pi.on("turn_end", async (_event, ctx) => refreshFromCtx(ctx))
	pi.on("tool_execution_end", async (_event, ctx) => refreshFromCtx(ctx))
	pi.on("agent_end", async (_event, ctx) => {
		refreshFromCtx(ctx)
		void refreshGit(ctx)
	})

	pi.on("session_shutdown", async () => {
		if (animationTimer) clearInterval(animationTimer)
	})
}
