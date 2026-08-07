// Right rail: non-capturing overlay anchored top-right, rendered through Pi's
// supported `ctx.ui.custom` overlay seam. In regular mode Pi exposes `tui` as
// a stable Proxy, so we find the unproxied `render` on the prototype chain
// once and install one replacement that calls it with the rail width
// subtracted — the rail docks (reserves a right column) instead of overlaying
// content. In fullscreen alt-screen mode we wrap the current layout root in an
// `HStack` that reserves a right column the same width the overlay draws at,
// making Pi content reflow instead of bleed underneath the rail's transparent
// padding. Column width and footer handoff are driven from the overlay
// `visible`/`render` callbacks. Mirrors pi-atelier's dual-path adapter design.
import { HStack } from "@earendil-works/pi-tui"
import { renderRail } from "./render.js"

// Fallback only: the harness sends the authoritative width in `hello`, sized
// as the same share of its terminal that the left sidebar takes. These values
// mirror PANEL_WIDTH_* in src/config/mod.rs for the pre-hello frames.
const RAIL_WIDTH_PERCENT = 22
const MIN_RAIL_WIDTH = 24
const MAX_RAIL_WIDTH = 80
const MIN_MAIN_WIDTH = 64
const ANIMATION_MS = 250

// Docked-layout bookkeeping lives on the tui itself under a symbol key, so
// other extensions do not observe it and our state disappears with the tui.
// `owner` guards against two adapters (e.g. pi-atelier) fighting over the
// layout root: whoever set it owns it until they restore it.
const ADAPTER_OWNER = Symbol("pi-harness.fullscreen-layout-owner")
const FULLSCREEN_LAYOUT_ADAPTER = Symbol("pi-harness.fullscreen-layout-adapter")
const REGULAR_RENDER_ADAPTER = Symbol("pi-harness.regular-render-adapter")

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

	// The column the dock reserves (and the overlay draws at) is the resolved
	// rail width, clamped into the same MIN/MAX band the overlay uses so the
	// HStack basis never exceeds the documented rail sizing.
	const resolveSidebarWidth = (terminalWidth) =>
		Math.min(MAX_RAIL_WIDTH, Math.max(MIN_RAIL_WIDTH, railWidth(terminalWidth)))

	// Dock controller state: the sidebar width the current split root reserves.
	// Refreshed whenever the terminal width / harness railWidth changes; the
	// split root is recreated on mismatch in syncFullscreenLayoutAdapter.
	let sidebarWidth = MIN_RAIL_WIDTH

	const overlayOptions = {
		anchor: "top-right",
		width: MIN_RAIL_WIDTH,
		maxHeight: "100%",
		margin: 0,
		nonCapturing: true,
		visible: (terminalWidth) => {
			overlayOptions.width = railWidth(terminalWidth)
			sidebarWidth = resolveSidebarWidth(terminalWidth)
			// Re-sync on every width callback so a resize updates the reserved
			// column. Cheap and idempotent (sync returns early on no change).
			syncFullscreenLayoutAdapter()
			syncRegularRenderAdapter()
			const visible = visibleAt(terminalWidth)
			scheduleFooterSync(visible)
			return visible
		},
	}

	const requestRender = () => tuiRef?.requestRender()

	// --- Docked-layout adapter (fullscreen mode only) ---
	// Wraps the running layout root in an HStack that reserves a right column
	// for the rail; the overlay still draws the content. Ported faithfully from
	// pi-atelier's fullscreen adapter. Guarded to `mode === "fullscreen"` and
	// to the `TuiAltScreen` class, so regular mode is a no-op. Wrap the real
	// root, leaving the placeholder HStack child empty — it only reserves space.

	const createFullscreenSplitRoot = (originalRoot) =>
		new HStack([
			{ component: originalRoot, basis: 0, grow: 1, shrink: 1, minSize: MIN_MAIN_WIDTH },
			{
				component: { render: () => [], invalidate() {} },
				basis: sidebarWidth,
				grow: 0,
				shrink: 1,
				minSize: MIN_RAIL_WIDTH,
				maxSize: MAX_RAIL_WIDTH,
				visible: ({ width }) => visibleAt(width),
			},
		])

	const syncFullscreenLayoutAdapter = () => {
		const tui = tuiRef
		if (!tui || tui.mode !== "fullscreen") return
		const adaptedTui = tui
		const prototype = Object.getPrototypeOf(tui)
		if (prototype?.constructor?.name !== "TuiAltScreen") return
		const currentState = adaptedTui[FULLSCREEN_LAYOUT_ADAPTER]
		if (currentState && currentState.owner !== ADAPTER_OWNER) return
		const currentRoot = adaptedTui.layoutRoot
		if (currentState?.owner === ADAPTER_OWNER && currentRoot === currentState.splitRoot) {
			if (currentState.sidebarWidth === sidebarWidth) return
			const splitRoot = createFullscreenSplitRoot(currentState.originalRoot)
			adaptedTui.setLayoutRoot(splitRoot)
			currentState.splitRoot = splitRoot
			currentState.sidebarWidth = sidebarWidth
			return
		}
		if (!currentRoot) return
		const splitRoot = createFullscreenSplitRoot(currentRoot)
		adaptedTui.setLayoutRoot(splitRoot)
		adaptedTui[FULLSCREEN_LAYOUT_ADAPTER] = { owner: ADAPTER_OWNER, originalRoot: currentRoot, splitRoot, sidebarWidth }
	}

	const restoreFullscreenLayoutAdapter = () => {
		if (!tuiRef) return
		const adaptedTui = tuiRef
		const currentState = adaptedTui[FULLSCREEN_LAYOUT_ADAPTER]
		if (currentState?.owner !== ADAPTER_OWNER) return
		if (adaptedTui.layoutRoot === currentState.splitRoot) adaptedTui.setLayoutRoot(currentState.originalRoot)
		adaptedTui[FULLSCREEN_LAYOUT_ADAPTER] = undefined
	}

	// --- Docked-layout adapter (regular mode) ---
	// Pi regular mode (TuiMainScreen) exposes `tui.render` through a stable Proxy,
	// so a naive capture-and-replace of the proxied render recurses. Instead we
	// find the unproxied `render` on the prototype chain once (findPrototypeRender)
	// and install one replacement that calls it with the rail width subtracted.
	// Guarded to `mode === "regular"` and the `TuiMainScreen` class, so fullscreen
	// mode is a no-op. Ported faithfully from pi-atelier's regular adapter.

	const findPrototypeRender = (nextTui) => {
		let prototype = Object.getPrototypeOf(nextTui)
		if (prototype?.constructor?.name !== "TuiMainScreen") return undefined
		while (prototype) {
			const descriptor = Object.getOwnPropertyDescriptor(prototype, "render")
			if (typeof descriptor?.value === "function") return descriptor.value
			prototype = Object.getPrototypeOf(prototype)
		}
		return undefined
	}

	const syncRegularRenderAdapter = () => {
		const tui = tuiRef
		if (!tui || tui.mode !== "regular") return
		const adaptedTui = tui
		const currentState = adaptedTui[REGULAR_RENDER_ADAPTER]
		if (currentState?.owner === ADAPTER_OWNER) return
		if (currentState) return // another extension owns this renderer
		const baseRender = findPrototypeRender(tui)
		if (!baseRender) return
		adaptedTui[REGULAR_RENDER_ADAPTER] = { owner: ADAPTER_OWNER, baseRender }
		adaptedTui.render = (width) => {
			const sidebar = visibleAt(width) ? resolveSidebarWidth(width) : 0
			return Reflect.apply(baseRender, tui, [sidebar > 0 ? width - sidebar : width])
		}
	}

	const restoreRegularRenderAdapter = () => {
		if (!tuiRef) return
		const adaptedTui = tuiRef
		const currentState = adaptedTui[REGULAR_RENDER_ADAPTER]
		if (currentState?.owner !== ADAPTER_OWNER) return
		adaptedTui.render = currentState.baseRender
		adaptedTui[REGULAR_RENDER_ADAPTER] = undefined
	}

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
					tuiRef = tui
					syncFullscreenLayoutAdapter()
					syncRegularRenderAdapter()
					return {
						render(width) {
							const rows = tuiRef?.terminal?.rows ?? 0
							// Harvested during render, stored without notifying listeners:
							// a notify here would schedule another render pass.
							store.state.statuses = statusLines()
							return renderRail(store.state, Math.max(1, width), Date.now(), rows, uiRef.theme)
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
			// Roster, not history: getAllTools() is every configured tool,
			// getActiveTools() the subset the LLM may call right now. Both are
			// optional on older Pi, which leaves the TOOLS panel absent.
			const activeTools = new Set(pi.getActiveTools?.() ?? [])
			const tools = (pi.getAllTools?.() ?? [])
				.map((tool) => ({ name: String(tool?.name ?? ""), active: activeTools.has(tool?.name) }))
				.filter((tool) => tool.name)
				.sort((a, b) => a.name.localeCompare(b.name))
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
				state.tools = tools
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
			if (enabled) {
				syncFullscreenLayoutAdapter()
				syncRegularRenderAdapter()
			} else {
				restoreFullscreenLayoutAdapter()
				restoreRegularRenderAdapter()
			}
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
		restoreFullscreenLayoutAdapter()
		restoreRegularRenderAdapter()
	})
}
