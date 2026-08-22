// Self-contained stand-ins for the two pi-tui text primitives the rail needs.
// omp's legacy-pi compat bundle stopped re-exporting them (`HStack` went
// first), and any static named import from the compat module fails the whole
// extension at ESM link time. Extensions execute inside omp's Bun runtime, so
// Bun.stringWidth provides the same UAX#11 column widths the TUI itself
// renders with.

// ANSI CSI/OSC/escape sequences carry zero columns.
const ANSI_PATTERN = /\x1b(?:\][^\x07]*(?:\x07|\x1b\\)|\[[0-?]*[ -/]*[@-~]|[@-Z\-_])/g
// Escape sequence or single code point.
const TOKEN_PATTERN = /\x1b(?:\][^\x07]*(?:\x07|\x1b\\)|\[[0-?]*[ -/]*[@-~]|[@-Z\-_])|./gsu

const TAB_WIDTH = 4

export function visibleWidth(str) {
	if (!str) return 0
	const plain = str.replace(ANSI_PATTERN, "")
	if (!plain) return 0
	let width = Bun.stringWidth(plain)
	const tabs = plain.length - plain.replace(/\t/g, "").length
	if (tabs > 0) width += tabs * TAB_WIDTH
	return width
}

export function truncateToWidth(str, width) {
	if (!(width > 0)) return ""
	const text = String(str ?? "")
	if (visibleWidth(text) <= width) return text
	let out = ""
	let used = 0
	for (const token of text.match(TOKEN_PATTERN) ?? []) {
		// Escape sequences are preserved verbatim so partial truncation keeps
		// styling well-formed. Code points (not grapheme clusters) are the
		// truncation unit — rail cells are short and ASCII-dominant.
		if (token.startsWith("\x1b")) {
			out += token
			continue
		}
		const tokenWidth = Bun.stringWidth(token)
		if (used + tokenWidth > width) break
		out += token
		used += tokenWidth
	}
	return out
}
