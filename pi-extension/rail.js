// Right rail: persistent non-capturing overlay anchored top-right, with a
// clean-room render wrap so Pi reflows into the remaining columns.
//
// The wrap is the only version-sensitive integration: TUI.render(width) is
// called with the terminal width minus the reserved rail columns. On any
// wrap failure the rail disables itself and Pi renders full width again.

import { renderRail } from "./render.js"

// Fallback only: the harness sends the authoritative width in `hello`, sized
// as the same share of its terminal that the left sidebar takes. These values
// mirror PANEL_WIDTH_* in src/config/mod.rs for the pre-hello frames.
const RAIL_WIDTH_PERCENT = 22
const MIN_RAIL_WIDTH = 24
const MAX_RAIL_WIDTH = 80
const MIN_MAIN_WIDTH = 64
const ANIMATION_MS = 250

export function registerRail(pi, store) {
	let started = false
	let enabled = true
	let wrapBroken = false
	let tuiRef
	let animationTimer

	const fallbackWidth = (terminalWidth) => {
		const share = Math.round(((Number.isFinite(terminalWidth) ? terminalWidth : 0) * RAIL_WIDTH_PERCENT) / 100)
		return Math.min(MAX_RAIL_WIDTH, Math.max(MIN_RAIL_WIDTH, share))
	}

	const railWidth = (terminalWidth) => {
		const width = store.state.harness?.railWidth
		return Number.isFinite(width) ? width : fallbackWidth(terminalWidth)
	}

	const visibleAt = (terminalWidth) => {
		const width = railWidth(terminalWidth)
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
		width: MIN_RAIL_WIDTH,
		maxHeight: "100%",
		margin: 0,
		nonCapturing: true,
		visible: (terminalWidth) => visibleAt(terminalWidth),
	}

	const requestRender = () => tuiRef?.requestRender()

	// Footer takeover: the rail already draws cwd, git, model, usage and context,
	// so while it is visible Pi's footer is replaced with a zero-line component.
	// The factory keeps the footer data provider, which is the only route to
	// `ctx.ui.setStatus` text from other extensions; those land in the EXT panel.
	// When the rail hides (narrow PTY, `/rail off`, broken wrap) the built-in
	// footer comes back, so no state is ever invisible.
	let uiRef
	let footerData
	let footerHidden = false
	let footerWanted = false
	let footerSyncQueued = false

	const hiddenFooter = (_tui, _theme, data) => {
		footerData = data
		return { invalidate() {}, render: () => [] }
	}

	const syncFooter = (hide) => {
		if (hide === footerHidden) return
		try {
			uiRef?.setFooter?.(hide ? hiddenFooter : undefined)
			footerHidden = hide
		} catch {
			// No setFooter (older Pi): keep its footer, the rail still renders.
		}
	}

	// setFooter swaps TUI children, so it must never run inside a render pass.
	const scheduleFooterSync = (hide) => {
		footerWanted = hide
		if (hide === footerHidden || footerSyncQueued) return
		footerSyncQueued = true
		queueMicrotask(() => {
			footerSyncQueued = false
			syncFooter(footerWanted)
		})
	}

	const statusLines = () => {
		if (!footerHidden) return []
		const entries = footerData?.getExtensionStatuses?.()
		if (!entries || entries.size === 0) return []
		return Array.from(entries.entries())
			.sort(([a], [b]) => a.localeCompare(b))
			.map(([, text]) => String(text ?? "").replace(/[\r\n\t]+/g, " ").trim())
			.filter(Boolean)
	}

	function attach(tui) {
		if (tuiRef) return
		tuiRef = tui
		const previousRender = tui.render
		tui.render = function wrappedRender(terminalWidth) {
			const visible = visibleAt(terminalWidth)
			const reserved = visible ? railWidth(terminalWidth) : 0
			scheduleFooterSync(visible)
			overlayOptions.width = reserved > 0 ? reserved : railWidth(terminalWidth) || MIN_RAIL_WIDTH
			try {
				return previousRender.call(tui, terminalWidth - reserved)
			} catch (error) {
				wrapBroken = true
				scheduleFooterSync(false)
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
		uiRef = ctx.ui

		void ctx.ui
			.custom(
				(tui) => {
					attach(tui)
					return {
						render(width) {
							const rows = tuiRef?.terminal?.rows ?? 0
							// Harvested during render, stored without notifying listeners:
							// a notify here would schedule another render pass.
							store.state.statuses = statusLines()
							return renderRail(store.state, Math.max(1, width), Date.now(), rows)
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
		syncFooter(false)
	})
}
