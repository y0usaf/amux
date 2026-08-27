import { createAssistantMessageEventStream } from "@earendil-works/pi-ai"

const responses = [
	{ type: "toolCall", id: "demo-read-readme", name: "read", arguments: { path: "README.md" } },
	{ type: "toolCall", id: "demo-read-main", name: "read", arguments: { path: "src/main.rs" } },
	{ type: "text", text: "## Focused next step\n\nAdd a small command parser and a focused test for JSON output." },
]

function streamDemo(model, context) {
	const stream = createAssistantMessageEventStream()
	const block = responses[Math.min(context.messages.filter((message) => message.role === "assistant").length, responses.length - 1)]
	const message = {
		role: "assistant",
		content: [block],
		api: model.api,
		provider: model.provider,
		model: model.id,
		usage: {
			input: 0,
			output: 0,
			cacheRead: 0,
			cacheWrite: 0,
			totalTokens: 0,
			cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
		},
		stopReason: block.type === "toolCall" ? "toolUse" : "stop",
		timestamp: Date.now(),
	}

	queueMicrotask(() => {
		stream.push({ type: "start", partial: message })
		if (block.type === "toolCall") {
			stream.push({ type: "toolcall_start", contentIndex: 0, partial: message })
			stream.push({ type: "toolcall_end", contentIndex: 0, toolCall: block, partial: message })
		} else {
			stream.push({ type: "text_start", contentIndex: 0, partial: message })
			stream.push({ type: "text_delta", contentIndex: 0, delta: block.text, partial: message })
			stream.push({ type: "text_end", contentIndex: 0, content: block.text, partial: message })
		}
		stream.push({ type: "done", reason: message.stopReason, message })
		stream.end()
	})

	return stream
}

export default function (pi) {
	pi.registerProvider("pi-harness-demo", {
		baseUrl: "http://localhost.invalid",
		apiKey: "demo",
		api: "pi-harness-demo-api",
		models: [
			{
				id: "demo-1",
				name: "Pi Harness Demo",
				reasoning: false,
				input: ["text"],
				cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
				contextWindow: 16_000,
				maxTokens: 2_000,
			},
		],
		streamSimple: streamDemo,
	})
}
