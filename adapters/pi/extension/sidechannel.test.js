import test from "node:test"
import assert from "node:assert/strict"
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { parseThemeAnsi, resolveTheme, parentSessionFileFromSessionFile } from "./sidechannel.js"

test("parseThemeAnsi decodes foreground truecolor", () => {
  assert.deepEqual(parseThemeAnsi("\x1b[38;2;1;23;255m"), { kind: "rgb", r: 1, g: 23, b: 255 })
})
test("parseThemeAnsi decodes foreground indexed color", () => {
  assert.deepEqual(parseThemeAnsi("\x1b[38;5;200m"), { kind: "ansi", index: 200 })
})
test("parseThemeAnsi decodes foreground default", () => {
  assert.deepEqual(parseThemeAnsi("\x1b[39m"), { kind: "default" })
})
test("parseThemeAnsi decodes background truecolor", () => {
  assert.deepEqual(parseThemeAnsi("\x1b[48;2;0;128;255m", true), { kind: "rgb", r: 0, g: 128, b: 255 })
})
test("parseThemeAnsi decodes background indexed color", () => {
  assert.deepEqual(parseThemeAnsi("\x1b[48;5;17m", true), { kind: "ansi", index: 17 })
})
test("parseThemeAnsi decodes background default", () => {
  assert.deepEqual(parseThemeAnsi("\x1b[49m", true), { kind: "default" })
})
test("parseThemeAnsi rejects malformed input", () => {
  for (const value of ["", "38;2;1;2;3m", "\x1b[38;2;256;0;0m", "\x1b[38;2;1;2m", "\x1b[38;5;256m", "\x1b[48;5;-1m", "\x1b[39;1m"]) {
    assert.equal(parseThemeAnsi(value), undefined, value)
  }
  assert.equal(parseThemeAnsi("\x1b[49m"), undefined)
})

test("resolveTheme dispatches foreground and background roles independently", () => {
  const theme = {
    getFgAnsi(token) {
      if (token === "mdHeading") return "not-a-colour"
      return "\x1b[38;5;23m"
    },
    getBgAnsi() {
      return "\x1b[48;5;42m"
    },
  }
  const roles = resolveTheme({ ui: { theme } })
  assert.equal(roles.length, 15)
  assert.deepEqual(roles[6], { kind: "ansi", index: 42 })
  assert.deepEqual(roles[7], { kind: "ansi", index: 42 })
  assert.deepEqual(roles[8], { kind: "default" })
  assert.deepEqual(roles[10], { kind: "ansi", index: 42 })
  assert.deepEqual(roles[2], { kind: "default" })
  assert.deepEqual(roles[1], { kind: "ansi", index: 23 })
  assert.deepEqual(roles[3], { kind: "ansi", index: 23 })
  for (const [i, role] of roles.entries()) {
    if (![2, 6, 7, 8, 10].includes(i)) assert.ok(role.kind === "ansi", `role ${i} should be coloured`)
  }
})

test("parentSessionFileFromSessionFile derives the parent session file from the artifacts dir", () => {
  const dir = mkdtempSync(join(tmpdir(), "sidechannel-test-"))
  try {
    writeFileSync(join(dir, "2026-08-27T17-00-00Z_parent-uuid.jsonl"), "")
    const child = join(dir, "2026-08-27T17-00-00Z_parent-uuid", "Scout.jsonl")
    mkdirSync(join(dir, "2026-08-27T17-00-00Z_parent-uuid"))
    writeFileSync(child, "")
    assert.equal(
      parentSessionFileFromSessionFile(child),
      join(dir, "2026-08-27T17-00-00Z_parent-uuid.jsonl"),
    )
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test("parentSessionFileFromSessionFile is undefined for top-level session files", () => {
  const dir = mkdtempSync(join(tmpdir(), "sidechannel-test-"))
  try {
    const top = join(dir, "2026-08-27T17-00-00Z_root-uuid.jsonl")
    writeFileSync(top, "")
    assert.equal(parentSessionFileFromSessionFile(top), undefined)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test("parentSessionFileFromSessionFile is undefined for missing or empty paths", () => {
  assert.equal(parentSessionFileFromSessionFile(undefined), undefined)
  assert.equal(parentSessionFileFromSessionFile(""), undefined)
  assert.equal(parentSessionFileFromSessionFile(join(tmpdir(), "sidechannel-missing", "root.jsonl")), undefined)
})
