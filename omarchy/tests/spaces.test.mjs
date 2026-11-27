import { test } from "node:test"
import assert from "node:assert/strict"

import { cursorIndex, EMPTY, fileUrl, filterIcons, gridMove, isGlyph, isImageIcon, isVersionError, needsUpdate, parseEmojis, parseGlyphs, parseManifestVersion, badgeColor, parseBarEntry, tabsSlot, expandPath, customModuleUrl, headerOnBar, numeralFor, publicSpace, publicTab, closeTabWarning, fairCap, barLook, DEFAULT_PALETTE, monogram, themePalette, ease, filterRows, gapLeft, findTab, focusedMonitor, initialIndex, monitorNamed, nameInput, ordered, orderSettled, parseState, recessAlpha, renameCommand, rowActions, score, stepAction, statusColors, tabActions, windowMotion, startsDrag, dropIndex, dragShift, moveRow, tabDragOffset, tabDropIndex, reorderTabs, sameTabs, tabCells, tabGaps, tabShifts } from "../Spaces.mjs"

// A line of `space watch`, with a tab on each of two monitors.
const line = (spaces, more) => JSON.stringify({ version: 2, space: "0.1.0", focused: "eDP-1", monitors: [
  { name: "eDP-1", space: "s1", tab: "s1:2", refresh: 120, tiled: true, tabs: [
    { id: "s1:1", slot: 1, label: "server", named: true, status: "waiting", windows: 2, active: false },
    { id: "s1:2", slot: 2, label: "claude", named: false, status: null, windows: 1, active: true },
  ] },
  { name: "DP-1", space: "s1", tab: "s1:11", refresh: 60, tiled: false, tabs: [
    { id: "s1:11", slot: 1, label: "", named: false, status: null, windows: 0, active: true },
  ] },
], spaces, ...more })
const rowsOf = (spaces) => parseState(line(spaces)).spaces

const json = JSON.stringify([
  { id: "home", label: "home", named: false, active: false, windows: 4, left_at: 1000 },
  { id: "s1", label: "ren", named: false, active: true, windows: 3, left_at: 3000 },
  { id: "s2", label: "renspace", named: false, active: false, windows: 0, left_at: 2000 },
  { id: "s3", label: "Tax 2026", named: true, active: false, windows: null, left_at: 0 },
])

test("parses a line of space watch", () => {
  const state = parseState(line(JSON.parse(json)))
  assert.equal(state.space, "0.1.0")
  assert.equal(state.focused, "eDP-1")
  assert.deepEqual(state.monitors[0].tabs[1], { id: "s1:2", slot: 2, label: "claude", named: false, status: "", windows: 1, active: true })
  assert.deepEqual(state.monitors.map((m) => [m.name, m.space, m.tab, m.refresh, m.tiled]), [["eDP-1", "s1", "s1:2", 120, true], ["DP-1", "s1", "s1:11", 60, false]])
  const rows = state.spaces
  assert.equal(rows.length, 4)
  assert.deepEqual(rows[0], { id: "home", label: "home", named: false, icon: "", active: false, windows: 4, leftAt: 1000, status: "" })
  assert.equal(rows[3].windows, -1, "no count without Hyprland")
  assert.equal(rows[3].named, true)
  assert.deepEqual(rowsOf([{ label: "x" }]), [], "rows without an id are dropped")
  assert.equal(rowsOf([{ id: "s9" }])[0].label, "s9", "no label: the id")
  assert.deepEqual(rowsOf([{ id: "s1", status: "waiting" }, { id: "s2", status: "asleep" }]).map((r) => r.status), ["waiting", ""], "an unknown status is idle")
})

test("says when space watch isn't one it can read", () => {
  assert.throws(() => parseState("space: error"), (e) => /unexpected/.test(e.message) && !isVersionError(e))
  assert.throws(() => parseState("{}"), (e) => /unexpected/.test(e.message) && !isVersionError(e))
  assert.throws(() => parseState("null"), (e) => /unexpected/.test(e.message) && !isVersionError(e))
  assert.throws(() => parseState(json), (e) => /older than the plugin/.test(e.message) && isVersionError(e))
  assert.throws(() => parseState(line([], { version: 1 })), (e) => /different versions/.test(e.message) && isVersionError(e))
})

test("tells when the plugin and space are on different versions", () => {
  assert.equal(parseManifestVersion('{"version":"0.2.0"}'), "0.2.0")
  assert.equal(parseManifestVersion("nope"), "")
  assert.equal(needsUpdate("0.2.0", "0.1.0"), true)
  assert.equal(needsUpdate("0.2.0", "0.2.0"), false)
  assert.equal(needsUpdate("0.2.0", ""), false)
  assert.equal(needsUpdate("", "0.1.0"), false)
})

test("finds monitors and their tabs", () => {
  const state = parseState(line([]))
  assert.equal(focusedMonitor(state).name, "eDP-1")
  assert.equal(monitorNamed(state, "DP-1").tab, "s1:11")
  assert.deepEqual(monitorNamed(state, "HDMI-1").tabs, [], "unknown yet: no tabs")
  assert.equal(focusedMonitor(EMPTY).space, "")
  assert.equal(findTab(state, "s1:1").tab.label, "server")
  assert.equal(findTab(state, "s1:11").monitor.name, "DP-1", "on whichever monitor it is")
  assert.equal(findTab(state, "s2:1"), null)
})

test("the cursor stays on the space it was moved to, else where it starts", () => {
  const rows = rowsOf(JSON.parse(json))
  assert.equal(cursorIndex(rows, "", ""), 2, "the one left last")
  assert.equal(cursorIndex(rows, "", "s3"), 3)
  assert.equal(cursorIndex(rows.slice().reverse(), "", "s3"), 0, "wherever it goes")
  assert.equal(cursorIndex(rows, "", "gone"), 2)
  assert.equal(cursorIndex(rows, "ren", ""), 0, "searching, on the best")
  assert.equal(cursorIndex([], "", "s1"), 0)
})

test("shows spaces where they were dropped until space watch agrees or can't", () => {
  const rows = rowsOf([{ id: "home" }, { id: "s1" }, { id: "s2" }])
  const ids = (list) => list.map((row) => row.id)
  const order = ["home", "s2", "s1"]
  assert.deepEqual(ids(ordered(rows, order)), order)
  assert.deepEqual(ids(ordered(rows, null)), ["home", "s1", "s2"])
  assert.equal(orderSettled(rows, order), false)
  // What else changed meanwhile shows, as only the order is laid over.
  const busy = rowsOf([{ id: "home" }, { id: "s1", active: true }, { id: "s2" }])
  assert.equal(ordered(busy, order)[2].active, true)
  assert.equal(orderSettled(rowsOf([{ id: "home" }, { id: "s2" }, { id: "s1" }]), order), true)
  // A space came or went: the order is of other spaces.
  const more = rowsOf([{ id: "home" }, { id: "s1" }, { id: "s2" }, { id: "s3" }])
  assert.deepEqual(ids(ordered(more, order)), ["home", "s1", "s2", "s3"])
  assert.equal(orderSettled(more, order), true)
  assert.equal(orderSettled(rowsOf([{ id: "home" }, { id: "s1" }, { id: "s9" }]), order), true)
})

test("scores prefix over word start over substring over letters in order", () => {
  assert.ok(score("ren", "ren") > score("ren", "renspace"))
  assert.ok(score("ren", "renspace") > score("tax", "my-taxes"))
  assert.ok(score("tax", "my-tax") > score("tax", "mytax"))
  assert.ok(score("tax", "mytax") > score("tx", "tax-2026"))
  assert.equal(score("rsp", "renspace") > 0, true)
  assert.equal(score("zz", "renspace"), -1)
  assert.equal(score("REN", "ren") > 0, true, "case-insensitive")
})

test("filters and ranks, keeping the list order with no query", () => {
  const rows = rowsOf(JSON.parse(json))
  const names = (query) => filterRows(rows, query).map((row) => row.label)
  assert.deepEqual(names(""), ["home", "ren", "renspace", "Tax 2026"])
  assert.deepEqual(names("  "), ["home", "ren", "renspace", "Tax 2026"])
  assert.deepEqual(names("ren"), ["ren", "renspace"])
  assert.deepEqual(names("spc"), ["renspace"])
  assert.deepEqual(names("2026"), ["Tax 2026"])
  assert.deepEqual(names("tax"), ["Tax 2026"], "ignoring case")
  assert.deepEqual(names("qq"), [])
})

test("starts on the space left most recently", () => {
  const rows = rowsOf(JSON.parse(json))
  assert.equal(rows[initialIndex(rows)].label, "renspace", "ren is active, so not it")
  const fresh = rows.map((row) => ({ ...row, leftAt: 0 }))
  assert.equal(fresh[initialIndex(fresh)].label, "ren", "nothing left yet: the active one")
  assert.equal(initialIndex([]), 0)
})

test("reads the statuses' colours from the theme", () => {
  assert.deepEqual(statusColors('red = "#F7768E"\ngreen = "#9ece6a"\nyellow = "#e0af68"\n'), { waiting: "#f7768e", done: "#9ece6a", working: "#e0af68" })
  assert.deepEqual(statusColors(""), { waiting: "", done: "", working: "" })
})

test("reads Hyprland's window motion", () => {
  const curves = [{ name: "easeOutQuint", X0: 0.23, Y0: 1, X1: 0.32, Y1: 1 }]
  const animations = (overrides) => JSON.stringify([
    [
      { name: "global", overridden: true, bezier: "default", enabled: true, speed: 10 },
      { name: "windows", overridden: true, bezier: "easeOutQuint", enabled: true, speed: 3.79 },
      { name: "windowsMove", overridden: false, bezier: "", enabled: true, speed: 0 },
      ...overrides,
    ],
    curves,
  ])
  assert.deepEqual(windowMotion(animations([])), { duration: 379, bezier: [0.23, 1, 0.32, 1] })

  const moveOverride = animations([{ name: "windowsMove", overridden: true, bezier: "default", enabled: true, speed: 2 }])
  // A later entry wins in the Map, as Hyprland lists each leaf once.
  assert.deepEqual(windowMotion(moveOverride), { duration: 200, bezier: [0, 0.75, 0.15, 1] })

  const off = JSON.stringify([[{ name: "global", overridden: true, bezier: "default", enabled: false, speed: 10 }], []])
  assert.equal(windowMotion(off).duration, 0)
  assert.equal(windowMotion("hyprctl: error"), null)
})

test("reads the left of Hyprland's outer gaps", () => {
  assert.equal(gapLeft('{"option": "general:gaps_out", "css": "10 20 30 40", "set": true}'), 40)
  assert.equal(gapLeft('{"option": "general:gaps_out", "css": "0 0 0 0", "set": true}'), 0)
  assert.equal(gapLeft('{"option": "general:gaps_out", "custom": "5 6 7 8"}'), 8)
  assert.equal(gapLeft('{"css": "10 20"}'), 20, "two values: vertical, then horizontal")
  assert.equal(gapLeft('{"css": "12"}'), 12)
  assert.equal(gapLeft('{"int": 7}'), 7)
  assert.equal(gapLeft('{"css": "a b c d"}'), null)
  assert.equal(gapLeft("hyprctl: no such option"), null)
})

test("marks a space with two letters, as T3 Code marks a project", () => {
  assert.equal(monogram("rencal"), "RL")
  assert.equal(monogram("caldir"), "CR")
  assert.equal(monogram("Tax 2026"), "T2")
  assert.equal(monogram("omarchy-caldir-widget"), "OW")
  assert.equal(monogram("v2ray"), "V2")
  assert.equal(monogram("my notes"), "MN")
  assert.equal(monogram("x"), "X")
  assert.equal(monogram("ångström"), "ÅM")
  assert.equal(monogram("~"), "?")
  assert.equal(monogram(""), "?")
})

test("colours a badge by its space's name", () => {
  const palette = ["#111111", "#222222", "#333333"]
  assert.equal(badgeColor("rencal", palette), badgeColor("rencal", palette), "the same each time")
  assert.equal(badgeColor("Rencal", palette), badgeColor("rencal", palette), "whatever its case")
  assert.ok(palette.includes(badgeColor("caldir", palette)))
  assert.equal(badgeColor("caldir", []), "")
})

test("takes badge colours from the theme", () => {
  const toml = [
    'mode = "dark"',
    'accent = "#faa968"',
    'red = "#F85525"',
    'yellow = "#e97b3c"',
    'orange = "#faa968"',
    'green = "#028391"',
    'blue = "#3f8f8a"',
    'magenta = "#3f8f8a"',
    'bright_red = "#ff0000"',
  ].join("\n")
  assert.deepEqual(themePalette(toml), ["#f85525", "#faa968", "#e97b3c", "#028391", "#3f8f8a"])
  assert.deepEqual(themePalette('red = "#ff0000"'), DEFAULT_PALETTE, "too few of its own")
  assert.deepEqual(themePalette(""), DEFAULT_PALETTE)
})

test("shares the bar's room between tabs, short ones whole", () => {
  assert.equal(fairCap([50, 60], 200), Infinity, "they fit")
  assert.equal(fairCap([40, 100, 100], 200), 80, "the short one keeps its width")
  assert.equal(fairCap([100, 100], 100), 50)
  assert.equal(fairCap([], 10), Infinity)
})

test("eases along a cubic Bézier as Hyprland does", () => {
  const linear = [0, 0, 1, 1]
  assert.ok(Math.abs(ease(linear, 0.25) - 0.25) < 1e-9)
  assert.equal(ease(linear, -1), 0)
  assert.equal(ease(linear, 2), 1)
  // easeOutQuint, as Omarchy sets it: most of the way there early on.
  const quint = [0.23, 1, 0.32, 1]
  assert.ok(Math.abs(ease(quint, 0.5) - 0.9660) < 1e-3, String(ease(quint, 0.5)))
  // CSS's ease at its midpoint.
  assert.ok(Math.abs(ease([0.25, 0.1, 0.25, 1], 0.5) - 0.8024) < 1e-3, String(ease([0.25, 0.1, 0.25, 1], 0.5)))
  let last = 0
  for (let x = 0; x <= 1; x += 0.01) {
    const y = ease(quint, x)
    assert.ok(y >= last - 1e-9, "never goes back")
    last = y
  }
})

test("sets the background back more on a dark bar than a light one", () => {
  assert.equal(recessAlpha(0.03, 0.08, 0.15), 0.3)
  assert.equal(recessAlpha(0.95, 0.93, 0.9), 0.07)
})

test("reads the bar's background look", () => {
  assert.equal(barLook({ background: "glassy" }), "glassy")
  assert.equal(barLook({ background: "none" }), "none")
  assert.equal(barLook({ transparent: true }), "none", "the older on/off form")
  assert.equal(barLook({ background: "opaque", transparent: true }), "opaque")
  assert.equal(barLook({ background: "neon" }), "opaque")
  assert.equal(barLook(null), "opaque")
})

test("reads whether the sidebar's header is on the bar", () => {
  const id = "com.t4t5.spaces"
  assert.equal(headerOnBar({ layout: { left: [{ id }] } }, id), true)
  assert.equal(headerOnBar({ layout: { left: [id] } }, id), true)
  assert.equal(headerOnBar({ layout: { left: [{ id }, { id: "omarchy.menu" }] } }, id), true)
  assert.equal(headerOnBar({ position: "bottom", layout: { left: [{ id }] } }, id), true)
  assert.equal(headerOnBar({ layout: { left: [{ id: "omarchy.menu" }, { id }] } }, id), false, "the menu has the corner")
  assert.equal(headerOnBar({ position: "left", layout: { left: [{ id }] } }, id), false, "a vertical bar")
  assert.equal(headerOnBar({ position: "right", layout: { left: [{ id }] } }, id), false, "a vertical bar")
  assert.equal(headerOnBar({ layout: { right: [{ id }] } }, id), false, "only the left section lines up")
  assert.equal(headerOnBar({ layout: { left: [null, { id }] } }, id), false)
  assert.equal(headerOnBar({ layout: { left: [] } }, id), false)
  assert.equal(headerOnBar(null, id), false)
})

test("reads a bar widget entry", () => {
  assert.deepEqual(parseBarEntry("io.github.someone.tabs"), { id: "io.github.someone.tabs", settings: {} })
  assert.deepEqual(parseBarEntry({ id: "io.github.someone.tabs", compact: true }), { id: "io.github.someone.tabs", settings: { compact: true } })
  assert.equal(parseBarEntry(""), null)
  assert.equal(parseBarEntry({}), null)
  assert.deepEqual(parseBarEntry({ source: "~/Tabs.qml" }), { id: "", settings: { source: "~/Tabs.qml" } }, "source alone names a custom module")
  assert.deepEqual(parseBarEntry({ type: "qml" }), { id: "", settings: { type: "qml" } }, "type alone names an incomplete module entry")
  assert.equal(parseBarEntry(["io.github.someone.tabs"]), null)
})

test("reads the tabs slot from its two keys", () => {
  const own = { shown: true, settings: {}, widget: null, misplaced: false }
  assert.deepEqual(tabsSlot(null), own)
  assert.deepEqual(tabsSlot({}), own)
  assert.deepEqual(tabsSlot({ tabs: null }), own, "null is unset")
  assert.deepEqual(tabsSlot({ tabs: false }), { ...own, shown: false })
  assert.deepEqual(tabsSlot({ tabs: false, tabsWidget: "io.github.someone.tabs" }),
    { ...own, shown: false, widget: { id: "io.github.someone.tabs", settings: {} } }, "tabsWidget wins over tabs: false")
  assert.deepEqual(tabsSlot({ tabs: { numerals: ["一"], labels: false } }), { ...own, settings: { numerals: ["一"], labels: false } })
  assert.deepEqual(tabsSlot({ tabsWidget: "io.github.someone.tabs" }), { ...own, widget: { id: "io.github.someone.tabs", settings: {} } })
  assert.deepEqual(tabsSlot({ tabsWidget: { source: "~/Tabs.qml", compact: true } }),
    { ...own, widget: { id: "", settings: { source: "~/Tabs.qml", compact: true } } })
  assert.deepEqual(tabsSlot({ tabs: { numerals: false }, tabsWidget: "x" }),
    { ...own, settings: { numerals: false }, widget: { id: "x", settings: {} } }, "each key keeps to its own")

  assert.equal(tabsSlot({ tabs: "io.github.someone.tabs" }).misplaced, true, "a widget id in tabs")
  assert.equal(tabsSlot({ tabs: { id: "io.github.someone.tabs" } }).misplaced, true)
  assert.equal(tabsSlot({ tabs: { source: "~/Tabs.qml" } }).misplaced, true)
  assert.deepEqual(tabsSlot({ tabs: { id: "x", labels: false } }).widget, null, "never read as a widget")
})

test("finds a custom QML module as the bar does", () => {
  assert.equal(expandPath("~/bar/Tabs.qml", "/home/u"), "/home/u/bar/Tabs.qml")
  assert.equal(expandPath("$HOME/bar/Tabs.qml", "/home/u/"), "/home/u/bar/Tabs.qml")
  assert.equal(expandPath("/abs/Tabs.qml", "/home/u"), "/abs/Tabs.qml")
  const url = (entry) => customModuleUrl(parseBarEntry(entry), "/home/u")
  assert.equal(url({ id: "kanji", source: "~/bar/Kanji.qml" }), "file:///home/u/bar/Kanji.qml")
  assert.equal(url({ source: "~/bar/Kanji.qml" }), "file:///home/u/bar/Kanji.qml")
  assert.equal(url({ id: "kanji", type: "qml" }), "file:///home/u/.config/omarchy/bar/modules/kanji.qml")
  assert.equal(url({ type: "qml" }), "", "qml without id or source")
  assert.equal(url({ id: "../evil", type: "qml" }), "")
  assert.equal(url({ id: "kanji", source: "relative.qml" }), "", "a path from nowhere")
  assert.equal(url({ id: "kanji", type: "command", source: "~/x.qml" }), "")
  assert.equal(url("io.github.someone.tabs"), "", "a registered widget")
})

test("numbers tabs with custom numerals", () => {
  const kanji = ["一", "二", "三", "四", "五"]
  assert.equal(numeralFor(2), "2")
  assert.equal(numeralFor(2, kanji), "二")
  assert.equal(numeralFor(6, kanji), "6", "past the list")
  assert.equal(numeralFor(1, false), "", "no numerals")
  assert.equal(numeralFor(0, kanji), "")
})

test("hands widgets in a slot a space and tabs of their own", () => {
  const state = parseState(line([
    { id: "s1", label: "Tax 2026", named: true, icon: "🚀", status: "waiting", windows: 3, active: true, left_at: 1000 },
    { id: "s2", status: "bogus" },
  ]))
  assert.deepEqual(publicSpace(state, "s1"), { id: "s1", label: "Tax 2026", named: true, icon: "🚀", status: "waiting" })
  assert.deepEqual(publicSpace(state, "s2"), { id: "s2", label: "s2", named: false, icon: "", status: "" })
  assert.deepEqual(publicSpace(EMPTY, "s9"), { id: "s9", label: "s9", named: false, icon: "", status: "" }, "before it's in spaces")
  assert.deepEqual(publicSpace(EMPTY, ""), { id: "", label: "", named: false, icon: "", status: "" }, "before space watch")

  const tab = publicTab({ id: "s1:2", slot: 2, label: "claude", named: false, status: "working", windows: 1, active: true, pid: 99 })
  assert.deepEqual(tab, { id: "s1:2", slot: 2, label: "claude", named: false, status: "working", active: true })
  assert.deepEqual(publicTab({ id: "s1:1", status: "bogus" }), { id: "s1:1", slot: 0, label: "", named: false, status: "", active: false })
})

test("offers rename, an icon and close, saying why close doesn't apply", () => {
  const rows = rowsOf(JSON.parse(json))
  const actions = (row) => rowActions(row).map((a) => [a.id, a.enabled, a.hint])
  const close = (row) => actions(row).find((a) => a[0] === "close")
  assert.deepEqual(rowActions(rows[0]), [], "home has none")
  assert.deepEqual(actions(rows[1]), [["rename", true, ""], ["icon", true, ""], ["close", true, "3 windows"]])
  assert.deepEqual(close(rows[2]), ["close", false, "No windows"])
  assert.equal(close({ id: "s4", windows: 1 })[2], "1 window")
  assert.equal(close(rows[3])[1], false, "no count without Hyprland")
  assert.deepEqual(rowActions(rows[1]).map((a) => a.separatorBefore), [false, false, true])
})

test("renames a space or a tab with its own command", () => {
  assert.deepEqual(renameCommand("s1", "work: 2"), ["space", "rename", "s1", "work: 2"])
  assert.deepEqual(renameCommand("s1:2", ""), ["space", "tab", "rename", "s1:2", ""])
})

test("offers to rename, move or close a tab", () => {
  const tab = (slot, windows, active) => ({ id: "s1:" + slot, slot, label: "", windows, active })
  const closes = (space, tabs) => tabActions(tabs[0], { space, tabs }).map((a) => [a.id, a.enabled]).at(-1)
  const moves = (tab, elsewhere) => tabActions(tab, { space: "s1", tabs: [] }, elsewhere).find((a) => a.id === "move-tab").enabled
  assert.deepEqual(tabActions(tab(2, 1, true), { space: "s1", tabs: [] }).map((a) => a.id), ["rename-tab", "move-tab", "close-tab"])
  assert.equal(moves(tab(1, 1, true), true), true)
  assert.equal(moves(tab(1, 0, true), true), false, "nothing to move")
  assert.equal(moves(tab(1, 1, true), false), false, "nowhere to move it")
  assert.deepEqual(closes("home", [tab(1, 2, true)]), ["close-tab", true], "its windows")
  assert.deepEqual(closes("home", [tab(1, 0, true), tab(2, 1, false)]), ["close-tab", true], "another to go to")
  assert.deepEqual(closes("home", [tab(1, 0, true)]), ["close-tab", false], "home doesn't end")
  assert.deepEqual(closes("s1", [tab(1, 0, true)]), ["close-tab", true], "a space does")
  assert.deepEqual(closes("s1", [tab(1, 0, false)]), ["close-tab", false], "empty, shown on another monitor")
  assert.deepEqual(tabActions(null, { space: "s1", tabs: [] }), [], "the tab has gone")
})

test("warns before closing a tab with windows", () => {
  assert.equal(closeTabWarning({ slot: 2, label: "server", windows: 3 }), "Close tab 2 (server)? Its 3 windows close too.")
  assert.equal(closeTabWarning({ slot: 1, label: "", windows: 1 }), "Close tab 1? Its window closes too.")
  assert.equal(closeTabWarning({ slot: 1, label: "", windows: 0 }), "")
  assert.equal(closeTabWarning(null), "")
})

test("steps through the enabled actions", () => {
  const actions = [{ enabled: true }, { enabled: true }, { enabled: false }]
  assert.equal(stepAction(actions, -1, 1), 0, "down from nothing: the first")
  assert.equal(stepAction(actions, -1, -1), 1, "up from nothing: the last enabled")
  assert.equal(stepAction(actions, 1, 1), 0, "skips the disabled one and wraps")
  assert.equal(stepAction(actions, 0, -1), 1)
  assert.equal(stepAction([{ enabled: false }], -1, 1), -1)
  assert.equal(stepAction([], -1, 1), -1)
})

test("takes any name up to 64 characters", () => {
  assert.equal(nameInput("Japan", "ese"), "Japanese")
  assert.equal(nameInput("", "Tax 2026"), "Tax 2026")
  assert.equal(nameInput("日本", "語"), "日本語")
  assert.equal(nameInput("x".repeat(64), "y"), "x".repeat(64))
})

test("tells glyphs from emojis and image files", () => {
  assert.equal(isGlyph("\u{f0415}"), true)
  assert.equal(isGlyph("\ue7a8"), true)
  assert.equal(isGlyph("🚀"), false)
  assert.equal(isGlyph("👩‍💻"), false)
  assert.equal(isGlyph("/home/u/logo.png"), false)
  assert.equal(isGlyph(""), false)

  assert.equal(isImageIcon("/home/u/logo.png"), true)
  assert.equal(isImageIcon("🚀"), false)
  assert.equal(isImageIcon(""), false)

  assert.equal(fileUrl("/home/u/My Logo #1.png"), "file:///home/u/My%20Logo%20%231.png")
  assert.equal(fileUrl("relative.png"), "")
  assert.equal(fileUrl(""), "")
})

test("reads glyphs and emojis", () => {
  assert.deepEqual(parseGlyphs('[["\u{f0415}","Plus md"],["",""],"x"]'), [{ icon: "\u{f0415}", keywords: "plus md", glyph: true }])
  assert.deepEqual(parseEmojis('[{"e":"🚀","k":"rocket space"},{"k":"none"}]'), [{ icon: "🚀", keywords: "rocket space", glyph: false }])
  assert.deepEqual(parseGlyphs("nope"), [])
  assert.deepEqual(parseEmojis(""), [])
})

test("filters icons by every word, whole words first", () => {
  const icons = [
    { icon: "a", keywords: "homeassistant md" },
    { icon: "b", keywords: "home md" },
    { icon: "c", keywords: "smart home outline md" },
    { icon: "d", keywords: "folder md" },
  ]
  assert.deepEqual(filterIcons(icons, "").map((i) => i.icon), ["a", "b", "c", "d"])
  assert.deepEqual(filterIcons(icons, "home").map((i) => i.icon), ["b", "c", "a"])
  assert.deepEqual(filterIcons(icons, "md").map((i) => i.icon), ["a", "b", "d", "c"], "fewer keywords first")
  assert.deepEqual(filterIcons(icons, "Home  OUT").map((i) => i.icon), ["c"])
  assert.deepEqual(filterIcons(icons, "older").map((i) => i.icon), ["d"])
  assert.deepEqual(filterIcons(icons, "nothing"), [])
})

test("searches emojis and glyphs together, the closest name first", () => {
  const emojis = [{ icon: "🔥", keywords: "fire burn" }, { icon: "🚒", keywords: "fire_engine fire engine" }]
  const glyphs = [{ icon: "g1", keywords: "fire alert md" }, { icon: "g2", keywords: "fire md" }]
  assert.deepEqual(filterIcons(emojis.concat(glyphs), "fire").map((i) => i.icon), ["🔥", "g2", "🚒", "g1"])
})

test("moves through a grid", () => {
  assert.equal(gridMove(-1, 10, 4, 1, 0), 0)
  assert.equal(gridMove(3, 10, 4, 1, 0), 4, "across wraps to the next row")
  assert.equal(gridMove(9, 10, 4, 1, 0), 0)
  assert.equal(gridMove(0, 10, 4, -1, 0), 9)
  assert.equal(gridMove(1, 10, 4, 0, 1), 5)
  assert.equal(gridMove(7, 10, 4, 0, 1), 9, "down stops at the last")
  assert.equal(gridMove(2, 10, 4, 0, -1), 0)
  assert.equal(gridMove(0, 0, 4, 1, 0), -1)
})

test("offers an icon, and its removal once there is one", () => {
  const ids = (row) => rowActions(row).map((a) => a.id)
  assert.deepEqual(ids({ id: "s1", icon: "", windows: 1 }), ["rename", "icon", "close"])
  assert.deepEqual(ids({ id: "s1", icon: "🚀", windows: 1 }), ["rename", "icon", "clear-icon", "close"])
  assert.equal(rowActions({ id: "s1", icon: "🚀" })[1].label, "Change icon")
})

test("drags tabs, which aren't equally wide", () => {
  const widths = [40, 100, 60]
  assert.equal(tabDropIndex(widths, 0, 0, 4), 0)
  assert.equal(tabDropIndex(widths, 0, 54, 4), 0, "up to the next one's middle")
  assert.equal(tabDropIndex(widths, 0, 55, 4), 1)
  assert.equal(tabDropIndex(widths, 0, 139, 4), 2)
  assert.equal(tabDropIndex(widths, 2, -55, 4), 1)
  assert.equal(tabDropIndex(widths, 2, -129, 4), 0)
  assert.equal(tabDropIndex(widths, 1, -25, 4), 0)
  assert.equal(tabDragOffset(widths, 1, -500, 4), -44, "no further than the first")
  assert.equal(tabDragOffset(widths, 1, 500, 4), 64, "nor than the last")
  assert.equal(tabDragOffset(widths, 1, 12, 4), 12)
})

test("shows the empty numbers among home's tabs, faded", () => {
  const tab = (slot) => ({ id: "s1:" + slot, slot })
  const shown = (tabs, withGaps = true) => tabCells(tabs, withGaps).map((cell) => cell.tab ? cell.tab.slot + "@" + cell.index : "(" + cell.gap + (cell.run ? "…" : "") + ")@" + cell.before)
  assert.deepEqual(shown([tab(1), tab(3), tab(4)]), ["1@0", "(2)@1", "3@1", "4@2"])
  assert.deepEqual(shown([tab(3), tab(5)]), ["(1…)@0", "3@0", "(4)@1", "5@1"], "from 1, two or more as one")
  assert.deepEqual(shown([tab(1), tab(2), tab(6)]), ["1@0", "2@1", "(3…)@2", "6@2"])
  assert.deepEqual(shown([tab(1), tab(2)]), ["1@0", "2@1"], "none after the last")
  assert.deepEqual(shown([tab(2), tab(0)]), ["(1)@0", "2@0", "0@1"], "none for a tab without a number")
  assert.deepEqual(shown([]), [])
  assert.deepEqual(shown([tab(1), tab(3)], false), ["1@0", "3@1"], "a space's, numbered again in a moment")
})

test("drags tabs past the empty numbers, which stay in their places", () => {
  // 1 (2) 3 4: tabs 40, 100 and 60 wide, the 2 20 wide.
  const cells = tabCells([1, 3, 4].map((slot) => ({ id: "s1:" + slot, slot })), true)
  const gaps = tabGaps(cells, [40, 20, 100, 60], 4)
  assert.deepEqual(gaps, [0, 24, 0])
  assert.equal(tabDropIndex([40, 100, 60], 0, 78, 4, gaps), 0, "up to the next one's middle, past the 2")
  assert.equal(tabDropIndex([40, 100, 60], 0, 79, 4, gaps), 1)
  assert.equal(tabDropIndex([40, 100, 60], 2, -153, 4, gaps), 0)
  assert.equal(tabDragOffset([40, 100, 60], 2, -500, 4, gaps), -172, "no further than the first")
  assert.equal(tabDragOffset([40, 100, 60], 0, 500, 4, gaps), 192, "nor than the last")
  // The last to the front, 60 wide: 1 0 → 64 and the 2 after it, 44 → 64;
  // the first tab moves past the 2 to the second place, 0 → 88.
  const front = tabShifts([40, 100, 60], 2, 0, 4, gaps)
  assert.deepEqual(front, { tabs: [88, 64, 0], gaps: [0, 20, 0] })
  // The first past the 3: the 3 takes the first place, 68 → 0, before the 2,
  // which moves along by how much wider the 3 is.
  const past = tabShifts([40, 100, 60], 0, 1, 4, gaps)
  assert.deepEqual(past, { tabs: [0, -68, 0], gaps: [0, 60, 0] })
  assert.deepEqual(tabShifts([40, 100, 60], -1, -1, 4, gaps), { tabs: [0, 0, 0], gaps: [0, 0, 0] }, "no drag")
  assert.deepEqual(tabShifts([40, 100, 60], 0, 2, 4), { tabs: [0, -44, -44], gaps: [0, 0, 0] }, "as dragShift with no gaps")
})

test("a moved tab takes its place's number", () => {
  const tab = (slot, label, active) => ({ id: "s1:" + slot, slot, label, named: false, status: "", windows: 1, active })
  const model = { space: "s1", tabs: [tab(1, "a", true), tab(2, "b", false), tab(4, "c", false)] }
  const moved = reorderTabs(model, 2, 0)
  assert.deepEqual(moved, { space: "s1", tabs: [tab(1, "c", false), tab(2, "a", true), tab(4, "b", false)] })
  assert.equal(sameTabs(model, moved), false)
  assert.equal(sameTabs(moved, { space: "s1", tabs: moved.tabs.map((t) => Object.assign({}, t, { status: "done" })) }), true)
  assert.equal(sameTabs(model, { space: "s2", tabs: model.tabs }), false)
})

test("drags spaces, never home, only past the threshold", () => {
  assert.equal(startsDrag({ id: "s1" }, 3, 12), false, "a wobble is a click")
  assert.equal(startsDrag({ id: "s1" }, -12, 12), true)
  assert.equal(startsDrag({ id: "home" }, 40, 12), false)
  assert.equal(dropIndex(1, 0, 30, 4), 1)
  assert.equal(dropIndex(1, 44, 30, 4), 2, "past half a row")
  assert.equal(dropIndex(1, 500, 30, 4), 3, "no further than last")
  assert.equal(dropIndex(3, -500, 30, 4), 1, "never above home")
  assert.deepEqual([0, 1, 2, 3].map((i) => dragShift(i, 1, 3, 30)), [0, 0, -30, -30])
  assert.deepEqual([0, 1, 2, 3].map((i) => dragShift(i, 3, 1, 30)), [0, 30, 30, 0])
  assert.deepEqual([0, 1, 2].map((i) => dragShift(i, -1, 1, 30)), [0, 0, 0], "no drag")
  assert.deepEqual(moveRow(["h", "a", "b", "c"], 1, 3), ["h", "b", "c", "a"])
  assert.deepEqual(moveRow(["h", "a", "b", "c"], 3, 1), ["h", "c", "a", "b"])
})
