// pi-harness companion extension.
//
// One extension, two halves sharing one store and one socket:
// - sidechannel: session snapshots up to the harness, hello/digest down
// - rail: atelier-style right rail rendered from the shared store
//
// Outside the harness (no AGENT_HARNESS_PI_SIDECAR_SOCKET) the sidechannel
// stays dormant and the rail still renders per-session panels with the
// fallback state; the SESSIONS digest panel simply never appears.

import { registerActivity } from "./activity.js"
import { registerRail } from "./rail.js"
import { registerSidechannel } from "./sidechannel.js"
import { createStore } from "./store.js"

export default function (pi) {
	const store = createStore()
	registerSidechannel(pi, store)
	registerActivity(pi, store)
	registerRail(pi, store)
}
