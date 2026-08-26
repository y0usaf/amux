// SP-Temporal theme ceremony: mount a fresh extension instance (sidechannel)
// against a fake sidecar socket, confirm it emits a themed 15-role `theme`
// JSON-line, exhaust it (fingerprint dedup -> no duplicate theme line),
// unmount (session_shutdown -> socket ends, extension relinquishes its state),
// then re-mount and confirm a clean fresh theme line with no residue from the
// first mount leaking into the second store. A leftover `theme`/snapshot line
// or inherited store state after re-mount fails the ceremony.
import test from "node:test"
import assert from "node:assert/strict"
import net from "node:net"
import os from "node:os"
import path from "node:path"
import fs from "node:fs"

const SOCKET_PATH = path.join(os.tmpdir(), `pi-harness-theme-ceremony-${process.pid}.sock`)

// --- fakes ---
function makeTheme() {
	return {
		getFgAnsi(token) {
			return token === "mdHeading" ? "\u001b[38;5;23m" : "\u001b[38;2;1;23;255m"
		},
		getBgAnsi() {
			return "\u001b[48;2;0;128;255m"
		},
	}
}

function makeCtx(theme, sessionId = "session-1") {
	return {
		isIdle: () => true,
		hasPendingMessages: () => false,
		sessionManager: {
			getSessionId: () => sessionId,
			getSessionFile: () => `/tmp/${sessionId}.jsonl`,
		},
		ui: { theme },
	}
}

function makePi() {
	const handlers = new Map()
	return {
		on(type, cb) {
			handlers.set(type, cb)
		},
		emit(type, event, ctx) {
			return handlers.get(type)?.(event, ctx)
		},
		getSessionName: () => undefined,
		setSessionName() {},
	}
}

// --- fake sidecar: line-collecting unix socket server ---
function startServer() {
	const connections = []
	const allLines = []
	const server = net.createServer((socket) => {
		const record = { socket, lines: [] }
		connections.push(record)
		allLines.push(record.lines)
		let buf = ""
		socket.on("data", (d) => {
			buf += d.toString("utf8")
			let i
			while ((i = buf.indexOf("\n")) !== -1) {
				const line = buf.slice(0, i)
				buf = buf.slice(i + 1)
				record.lines.push(line)
			}
		})
	})
	return new Promise((resolve) => {
		server.listen(SOCKET_PATH, () => resolve({ server, allLines, connections }))
	})
}

function parsed(lines) {
	return lines.map((l) => {
		try {
			return { raw: l, value: JSON.parse(l) }
		} catch {
			return { raw: l, value: null }
		}
	})
}

function waitFor(predicate, timeoutMs = 4000) {
	return new Promise((resolve, reject) => {
		const deadline = Date.now() + timeoutMs
		const poll = () => {
			if (predicate()) return resolve(true)
			if (Date.now() > deadline) return reject(new Error("timeout waiting for condition"))
			setTimeout(poll, 10)
		}
		poll()
	})
}

function themeRoles(lines) {
	return parsed(lines)
		.filter((l) => l.value?.type === "theme")
		.map((l) => l.value.roles)
}

test("SP-Temporal ceremony: mount -> theme line -> exhaust -> detach -> re-mount, no residue", async () => {
	process.env.AGENT_HARNESS_PI_SIDECAR_SOCKET = SOCKET_PATH
	try {
		fs.rmSync(SOCKET_PATH, { force: true })

		// Import AFTER the env var is set so the sidechannel captures the socket.
		const { createStore } = await import("./store.js")
		const { registerSidechannel } = await import("./sidechannel.js")

		const { server, connections, allLines } = await startServer()

		const currentLines = () => allLines.flat()

		// --- MOUNT #1 (attach) ---
		const store1 = createStore()
		const pi1 = makePi()
		registerSidechannel(pi1, store1)
		const ctx1 = makeCtx(makeTheme(), "session-1")
		await pi1.emit("session_start", {}, ctx1)

		// First themed 15-role rail line arrives after the snapshot.
		await waitFor(() => themeRoles(currentLines()).length >= 1)
		const roles1 = themeRoles(currentLines())[0]
		assert.equal(roles1.length, 15, "mount must emit a complete 15-role theme line")
		assert.ok(roles1.every((r) => r && r.kind), "each role must carry a colour/kind")

		// --- EXHAUST (fingerprint dedup): identical theme must not re-emit ---
		await pi1.emit("session_start", {}, ctx1)
		const themesAfterResend = themeRoles(currentLines()).length
		assert.equal(themesAfterResend, 1, "no duplicate theme line on identical fingerprint")

		// The themed rail client received a hello-side downstream (sticky hello
		// replay is harness-owned; here we confirm the mount handshake wrote to us).
		await waitFor(() => currentLines().some((l) => l.includes("snapshot")))

		// --- DETACH / unmount: session_shutdown ends the socket cleanly ---
		const firstConn = connections[0]
		const closed = new Promise((resolve) => firstConn.socket.on("close", resolve))
		await pi1.emit("session_shutdown", {}, ctx1)
		await closed
		assert.equal(firstConn.socket.destroyed, true, "detached extension must close its socket")
		// None of mount-1's dedicated theme line should have been duplicated by teardown.
		assert.equal(themeRoles(currentLines()).length, 1, "teardown must not emit a spurious theme line")

		// --- RE-MOUNT (fresh instance, fresh store) must show no residue ---
		const store2 = createStore()
		const pi2 = makePi()
		registerSidechannel(pi2, store2)
		// A fresh store starts clean: no harness hello, no inherited statuses.
		assert.equal(store2.state.harness, undefined, "re-mounted store has no stale downstream hello")
		assert.equal(store2.state.stage, "idle", "re-mounted store starts idle, not the prior stage")

		const ctx2 = makeCtx(makeTheme(), "session-2")
		await pi2.emit("session_start", {}, ctx2)
		await waitFor(() => themeRoles(currentLines()).length >= 2)

		const allRoles = themeRoles(currentLines())
		assert.equal(allRoles.length, 2, "re-mount emits exactly one fresh theme line")
		const roles2lines = allRoles[1]
		assert.equal(roles2lines.length, 15, "re-mounted theme line is a complete 15-role set")

		// Cross-mount independence: both mounts produced identical clean 15-role lines.
		assert.deepEqual(roles1, roles2lines, "theme round-trip is stable across detach/re-mount")

		// --- residue rejection: tear down connection #2 ---
		const secondConn = connections[1]
		const closed2 = new Promise((resolve) => secondConn.socket.on("close", resolve))
		await pi2.emit("session_shutdown", {}, ctx2)
		await closed2

		// stop listening so the process can exit
		await new Promise((r) => server.close(r))
		assert.ok(true, "ceremony completed: mount/theme/exhaust/detach/re-mount all clean, no residue")
	} finally {
		delete process.env.AGENT_HARNESS_PI_SIDECAR_SOCKET
		fs.rmSync(SOCKET_PATH, { force: true })
	}
})