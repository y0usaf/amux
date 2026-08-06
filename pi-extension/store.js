// Shared state between the sidechannel bridge and the rail renderer.
// Single mutable store + change listeners; everything the rail draws lives here.

export function createStore() {
	const listeners = new Set()

	const state = {
		// per-session runtime (written by sidechannel event handlers)
		stage: "idle", // idle | thinking | outputting | tool
		queued: false,
		interrupted: false,
		sessionName: undefined,

		// run activity (written by activity tracker)
		run: {
			phase: "idle", // idle | running | settled
			turn: 0,
			startedAt: 0,
			settledAt: 0,
			activeTools: [], // { id, name, summary, startedAt }
			recentTools: [], // { name, summary, durationMs, failed }
			doneCount: 0,
			failedCount: 0,
		},

		// model / usage / context (refreshed from ctx on lifecycle events)
		model: undefined, // { id, provider }
		thinkingLevel: undefined,
		subscription: false,
		usage: undefined, // { input, output, cacheRead, cacheWrite, cost, costAvailable }
		context: undefined, // { tokens, contextWindow, percent }
		tools: [], // [{ name, active }] registered roster, sorted by name

		// workspace
		cwd: process.cwd(),
		git: undefined, // { branch, dirty }

		// other extensions' ctx.ui.setStatus text, harvested from Pi's footer data
		// provider while the rail owns the footer (written by the rail renderer)
		statuses: [], // string[]

		// harness downstream (written by sidechannel socket reader)
		harness: undefined, // { railWidth }
		digest: undefined, // [{ key, name, project, stage, queued, interrupted, unread, selected }]
	}

	function notifyListeners() {
		for (const listener of listeners) {
			try {
				listener()
			} catch {
				// Rail render failures must never break the bridge.
			}
		}
	}

	return {
		state,
		update(mutate) {
			mutate(state)
			notifyListeners()
		},
		subscribe(listener) {
			listeners.add(listener)
			return () => listeners.delete(listener)
		},
	}
}
