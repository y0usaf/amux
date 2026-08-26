import test from "node:test"
import assert from "node:assert/strict"
import { createGlyphs, PRESETS, resolveSymbols } from "./symbols.js"

test("unicode default yields braille spinner, │ divider, ✓ ok", () => {
  const g = createGlyphs({})
  assert.deepEqual(g.spinner, PRESETS.unicode.spinner)
  assert.equal(g.divider, "│")
  assert.equal(g.ok, "✓")
  assert.equal(resolveSymbols({}).notif, "⣿")
  assert.equal(resolveSymbols({}).marker, "▸ ")
})

test("AGENT_HARNESS_PI_ASCII=1 yields ascii spinner, | divider, ok", () => {
  const g = createGlyphs({ AGENT_HARNESS_PI_ASCII: "1" })
  assert.deepEqual(g.spinner, ["-", "\\", "|", "/"])
  assert.equal(g.divider, "|")
  assert.equal(g.ok, "ok")
  assert.equal(g.notif, "[!]")
  assert.equal(g.marker, "> ")
})

test("symbol override rail.ok maps onto resolved preset", () => {
  const g = createGlyphs({ AGENT_HARNESS_SYMBOL_OVERRIDES: JSON.stringify({ "rail.ok": "OK" }) })
  assert.equal(g.ok, "OK")
})

test("override wins over ascii preset", () => {
  const g = createGlyphs({
    AGENT_HARNESS_PI_ASCII: "1",
    AGENT_HARNESS_SYMBOL_OVERRIDES: JSON.stringify({ "rail.err": "XX" }),
  })
  assert.equal(g.err, "XX")
  assert.equal(g.divider, "|")
})

test("invalid override JSON warns and falls back to preset", () => {
  const warnings = []
  const original = console.warn
  console.warn = (msg) => warnings.push(msg)
  try {
    const g = createGlyphs({ AGENT_HARNESS_SYMBOL_OVERRIDES: "not-json" })
    assert.equal(g.ok, "✓")
  } finally {
    console.warn = original
  }
  assert.ok(warnings.length > 0, "expected a console warning on bad override JSON")
})
