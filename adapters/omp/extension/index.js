// omp-harness companion extension.
//
// The omp adapter only uses the sidechannel and activity state. The rail is
// Pi-specific for now and must not alter omp's TUI rendering/input path.

import { registerActivity } from "./activity.js"
import { registerSidechannel } from "./sidechannel.js"
import { createStore } from "./store.js"

export default function (pi) {
	const store = createStore()
	registerSidechannel(pi, store)
	registerActivity(pi, store)
}
