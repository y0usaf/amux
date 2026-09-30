import { basename } from "node:path"

export default function (pi) {
	let timer
	const title = () => {
		const session = pi.getSessionName()
		const cwd = basename(process.cwd())
		return session ? `π - ${session} - ${cwd}` : `π - ${cwd}`
	}
	const settle = (ctx) => {
		clearInterval(timer)
		timer = undefined
		if (ctx.hasUI) ctx.ui.setTitle(title())
	}

	pi.on("agent_start", async (_event, ctx) => {
		if (!ctx.hasUI) return
		clearInterval(timer)
		const busy = () => ctx.ui.setTitle(`◐ ${title()}`)
		busy()
		timer = setInterval(busy, 1000)
	})

	pi.on("agent_settled", async (_event, ctx) => {
		settle(ctx)
		if (!ctx.hasUI) return
		const body = title().replace(/[\x00-\x1f\x7f-\x9f]/g, " ")
		process.stdout.write(`\x1b]777;notify;π;${body}\x07`)
	})

	pi.on("session_shutdown", async (_event, ctx) => settle(ctx))
}
