// The plugin's logic, kept out of QML so Node can test it. Everything the bar
// and the sidebar show of the spaces comes from one line of `space watch`.

// The version of its lines this reads (src/view.rs).
const VERSION = 2

// Before there is any.
export const EMPTY = { space: "", focused: "", monitors: [], spaces: [] }
const NO_MONITOR = { name: "", space: "", tab: "", refresh: 0, tiled: false, tabs: [] }

// A line of `space watch`, or throws with a readable message:
//   space:    `space`'s version (`Cargo.toml`), "" if omitted
//   focused:  the monitor you are on, by name
//   monitors: [{ name, space, tab, refresh, tiled, tabs }], the space and tab
//             each shows, by their ids (tab "" on a workspace without a
//             number), and the tabs its bar does: [{ id, slot, label, named,
//             status, windows, active }], slot 0 for no number
//   spaces:   the rows, in the sidebar's order, each with the windows it has:
//             clients: [{ address, tab, slot, class, title }], tab "" and slot
//             0 for a workspace without a number
export function parseState(text) {
  let state
  try {
    state = JSON.parse(String(text || ""))
  } catch (e) {
    throw new Error("space watch printed something unexpected")
  }
  if (!state || typeof state !== "object" || !Array.isArray(state.monitors) || !Array.isArray(state.spaces)) {
    // A list is what it printed before it had a version.
    if (Array.isArray(state)) throw versionError("The space command is older than the plugin: install it again")
    throw new Error("space watch printed something unexpected")
  }
  if (state.version !== VERSION) {
    throw versionError("The space command and the plugin are of different versions: run install.sh or update both")
  }
  return {
    space: typeof state.space === "string" ? state.space : "",
    focused: typeof state.focused === "string" ? state.focused : "",
    monitors: state.monitors
      .filter((monitor) => monitor && typeof monitor.name === "string" && typeof monitor.space === "string")
      .map((monitor) => ({
        name: monitor.name,
        space: monitor.space,
        tab: typeof monitor.tab === "string" ? monitor.tab : "",
        refresh: number(monitor.refresh),
        tiled: monitor.tiled === true,
        tabs: (Array.isArray(monitor.tabs) ? monitor.tabs : [])
          .filter((tab) => tab && typeof tab.id === "string" && tab.id !== "")
          .map((tab) => ({
            id: tab.id,
            slot: number(tab.slot),
            label: typeof tab.label === "string" ? tab.label : "",
            named: tab.named === true,
            status: status(tab.status),
            windows: number(tab.windows),
            active: tab.active === true,
          })),
      })),
    spaces: state.spaces
      .filter((row) => row && typeof row.id === "string" && row.id !== "")
      .map((row) => ({
        id: row.id,
        label: typeof row.label === "string" && row.label ? row.label : row.id,
        named: row.named === true,
        icon: typeof row.icon === "string" ? row.icon : "",
        folder: typeof row.folder === "string" ? row.folder : "",
        meta: plainObject(row.meta),
        active: row.active === true,
        windows: typeof row.windows === "number" ? row.windows : -1,
        leftAt: number(row.left_at),
        status: status(row.status),
        terminals: parseTerminals(row.terminals),
        clients: parseClients(row.clients),
      })),
  }
}

function plainObject(value) {
  return value && typeof value === "object" && !Array.isArray(value) ? value : {}
}

function parseTerminals(list) {
  if (!Array.isArray(list)) return []
  return list
    .filter((t) => t && typeof t.address === "string" && t.address !== "")
    .map((t) => ({
      address: t.address,
      tab: typeof t.tab === "string" ? t.tab : "",
      folder: typeof t.folder === "string" ? t.folder : "",
      program: typeof t.program === "string" ? t.program : "",
      label: typeof t.label === "string" ? t.label : "",
      status: status(t.status),
    }))
}

function parseClients(list) {
  if (!Array.isArray(list)) return []
  return list
    .filter((c) => c && typeof c.address === "string" && c.address !== "")
    .map((c) => ({
      address: c.address,
      tab: typeof c.tab === "string" ? c.tab : "",
      slot: number(c.slot),
      class: typeof c.class === "string" ? c.class : "",
      title: typeof c.title === "string" ? c.title : "",
    }))
}

function versionError(message) {
  const err = new Error(message)
  err.versionMismatch = true
  return err
}

// Whether `parseState` failed because `space` and the plugin are on different
// wire versions.
export function isVersionError(error) {
  return !!error && error.versionMismatch === true
}

// The plugin's version from manifest.json, or "" when it can't be read.
export function parseManifestVersion(text) {
  try {
    const data = JSON.parse(String(text || ""))
    return data && typeof data.version === "string" ? data.version : ""
  } catch (e) {
    return ""
  }
}

// Whether `space` should be brought to `pluginVersion`: both are known and
// they differ.
export function needsUpdate(pluginVersion, spaceVersion) {
  return !!pluginVersion && !!spaceVersion && pluginVersion !== spaceVersion
}

function number(value) {
  return typeof value === "number" ? value : 0
}

// A monitor by its name, one without tabs when there's none.
export function monitorNamed(state, name) {
  return state.monitors.find((monitor) => monitor.name === name) || NO_MONITOR
}

// The monitor you are on.
export function focusedMonitor(state) {
  return monitorNamed(state, state.focused)
}

// The monitor whose bar has a tab, by its id, and the tab: `{ monitor, tab }`,
// or null when none does.
export function findTab(state, id) {
  for (const monitor of state.monitors) {
    const tab = monitor.tabs.find((t) => t.id === id)
    if (tab) return { monitor, tab }
  }
  return null
}

// What an agent is doing, least to most in need of you; "" when idle.
const STATUSES = ["", "working", "done", "waiting"]

function status(value) {
  return STATUSES.includes(value) ? value : ""
}

// The colours of the statuses from the theme's colors.toml: waiting in its
// red, done in its green, working in its yellow, as Herdr shows it.
export function statusColors(text) {
  const colors = themeColors(text)
  return { waiting: colors.red || "", done: colors.green || "", working: colors.yellow || "" }
}

// How well `query` matches `name`: higher is better, -1 for no match. A
// prefix beats a match at a word start (after `-`), which beats a substring,
// which beats the letters in order with gaps.
export function score(query, name) {
  const q = query.toLowerCase()
  const n = name.toLowerCase()
  if (!q) return 0
  if (n.startsWith(q)) return 400 - n.length
  const at = n.indexOf(q)
  if (at > 0 && n[at - 1] === "-") return 300 - n.length
  if (at > 0) return 200 - at
  let from = 0
  let gaps = 0
  for (const c of q) {
    const found = n.indexOf(c, from)
    if (found < 0) return -1
    gaps += found - from
    from = found + 1
  }
  return 100 - gaps
}

// The rows matching `query` by what they're called, best first. With no
// query, every row in the order `space watch` gives (home, then the order they
// were made in), so each space keeps its place.
export function filterRows(rows, query) {
  const q = String(query || "").trim()
  if (!q) return rows.slice()
  return rows
    .map((row, index) => ({ row, index, score: score(q, row.label) }))
    .filter((entry) => entry.score >= 0)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((entry) => entry.row)
}

// What the sidebar lists while you search, in order: the spaces that match
// `query` by what they're called or by a window they have, each followed by
// its matching windows. A window matches by its title or its app, scored as a
// name is. A space comes where its best match puts it, so a window can bring
// its space up the list; its windows keep the order they score in, then the
// order they came in. A window is an item of its own, `{ id, window: true,
// space, address, tab, slot, title, app, label }`, whose id is made of its
// address alone: a title that changes (browsers retitle all the time) is the
// same item, so the cursor stays on it. With no query, just the rows, as
// `filterRows` has them, and no windows.
export function searchRows(rows, query) {
  const q = String(query || "").trim()
  if (!q) return rows.slice()
  const found = []
  rows.forEach((row, index) => {
    const windows = (Array.isArray(row.clients) ? row.clients : [])
      .map((client, at) => ({ client, at, score: Math.max(score(q, client.title), score(q, client.class)) }))
      .filter((entry) => entry.score >= 0)
      .sort((a, b) => b.score - a.score || a.at - b.at)
    const named = score(q, row.label)
    if (named < 0 && windows.length === 0) return
    const best = Math.max(named, windows.length ? windows[0].score : -1)
    found.push({ row, index, best, windows })
  })
  found.sort((a, b) => b.best - a.best || a.index - b.index)
  const out = []
  for (const entry of found) {
    out.push(entry.row)
    for (const { client } of entry.windows) {
      out.push({
        id: "window:" + client.address,
        window: true,
        space: entry.row.id,
        address: client.address,
        tab: client.tab,
        slot: client.slot,
        title: client.title,
        app: client.class,
        label: client.title || client.class,
        icon: "",
        active: false,
        status: "",
      })
    }
  }
  return out
}

// Whether two lists of what the sidebar shows would look the same. A row's
// windows are left out: they matter only once a search lists them, and then
// they are items of their own. So a window retitled while you aren't
// searching leaves the list as it is.
export function sameRows(a, b) {
  // Only the rows' own `clients`: a key of that name in their metadata is theirs.
  const plain = (items) => JSON.stringify(items.map((item) => {
    const copy = Object.assign({}, item)
    delete copy.clients
    return copy
  }))
  return plain(a) === plain(b)
}

// A row's right-click menu: which actions apply, and why not when one
// doesn't. `home` has none. Icons are Nerd Font glyphs: a pencil, a smiley,
// a broom, a cross.
export function rowActions(row) {
  if (!row || row.id === "home") return []
  const windows = row.windows > 0 ? row.windows : 0
  const actions = [
    { id: "rename", icon: "\u{f0cb6}", label: "Rename", enabled: true, hint: "", separatorBefore: false },
    { id: "icon", icon: "\u{f01f5}", label: row.icon ? "Change icon" : "Set icon", enabled: true, hint: "", separatorBefore: false },
  ]
  if (row.icon) {
    actions.push({ id: "clear-icon", icon: "\u{f00e2}", label: "Remove icon", enabled: true, hint: "", separatorBefore: false })
  }
  actions.push({
    id: "close",
    icon: "\u{f0156}",
    label: "Close space",
    enabled: windows > 0,
    hint: windows === 1 ? "1 window" : windows > 1 ? windows + " windows" : "No windows",
    separatorBefore: true,
  })
  return actions
}

// A tab's right-click menu on the bar, of its monitor (`model`): name it, as
// double-clicking it does, move it to another space, of which there may be
// none (`elsewhere`), or close it. An empty tab is only there while
// you're on it, so closing it takes another to go to, or a space to end:
// `home` doesn't.
export function tabActions(tab, model, elsewhere) {
  if (!tab) return []
  const leavable = model.space !== "home" || model.tabs.some((t) => t.id !== tab.id && t.windows > 0)
  return [
    { id: "rename-tab", icon: "\u{f0cb6}", label: "Rename", enabled: true, hint: "", separatorBefore: false },
    {
      id: "move-tab",
      icon: "\u{f0054}",
      label: "Move to space…",
      enabled: !!elsewhere && tab.windows > 0,
      hint: "",
      separatorBefore: false,
    },
    {
      id: "close-tab",
      icon: "\u{f0156}",
      label: "Close tab",
      enabled: tab.windows > 0 || (tab.active && leavable),
      hint: "",
      separatorBefore: true,
    },
  ]
}

// What to ask before closing a tab, for the windows that go with it; "" when
// it has none, and there is nothing to ask.
export function closeTabWarning(tab) {
  if (!tab || !(tab.windows > 0)) return ""
  const number = tab.slot ? `tab ${tab.slot}` : "this tab"
  const what = tab.label ? `${number} (${tab.label})` : number
  return `Close ${what}? ${tab.windows === 1 ? "Its window closes" : `Its ${tab.windows} windows close`} too.`
}

// The next enabled action going `by` (1 or -1) from `from`, wrapping; from
// nothing (-1), the first or the last. -1 when none is enabled.
export function stepAction(actions, from, by) {
  const count = actions.length
  let index = from < 0 ? (by > 0 ? -1 : count) : from
  for (let i = 0; i < count; i++) {
    index = (index + by + count) % count
    if (actions[index].enabled) return index
  }
  return -1
}

// The command that gives a space or a tab, by its id, a name. Only tabs' ids
// have a colon.
export function renameCommand(target, name) {
  return target.includes(":") ? ["space", "tab", "rename", target, name] : ["space", "rename", target, name]
}

// A name being typed, after typing `text`: anything printable, up to 64
// characters, as `space rename` and `space tab rename` take.
export function nameInput(name, text) {
  return Array.from(String(name || "") + String(text || "")).slice(0, 64).join("")
}

// ---------------------------------------------------------------- icons
//
// A space's badge can show an icon you pick in place of its letters: a Nerd
// Font glyph, tinted in the badge's colour, an emoji, in its own, or a local
// image file (png, svg, etc.).

// Whether an icon is a local image file path rather than a glyph or emoji.
export function isImageIcon(icon) {
  return String(icon || "").startsWith("/")
}

// Turns an absolute local path into a file:// URL safe for QML Image.source.
export function fileUrl(path) {
  const raw = String(path || "")
  if (!raw.startsWith("/")) return ""
  return "file://" + raw.split("/").map(encodeURIComponent).join("/")
}

// Whether an icon is a Nerd Font glyph: every character in Unicode's private
// use areas, where Nerd Fonts puts them. Anything else is an emoji or image.
export function isGlyph(icon) {
  const chars = Array.from(String(icon || ""))
  return chars.length > 0 && chars.every((c) => {
    const code = c.codePointAt(0)
    return (code >= 0xe000 && code <= 0xf8ff) || code >= 0xf0000
  })
}

// The glyphs the picker offers, from glyphs.json (scripts/glyphs.py):
// `[{ icon, keywords, glyph: true }]`, or none when it can't be read.
export function parseGlyphs(text) {
  try {
    const data = JSON.parse(String(text || ""))
    return Array.isArray(data)
      ? data.filter((g) => Array.isArray(g) && typeof g[0] === "string" && g[0])
        .map((g) => ({ icon: g[0], keywords: String(g[1] || "").toLowerCase(), glyph: true }))
      : []
  } catch (e) {
    return []
  }
}

// The emojis, from Omarchy's own picker's emojis.json (`[{ e, k }]`), as
// `[{ icon, keywords, glyph: false }]`.
export function parseEmojis(text) {
  try {
    const data = JSON.parse(String(text || ""))
    return Array.isArray(data)
      ? data.filter((e) => e && typeof e.e === "string" && e.e)
        .map((e) => ({ icon: e.e, keywords: String(e.k || "").toLowerCase(), glyph: false }))
      : []
  } catch (e) {
    return []
  }
}

// The icons whose keywords hold every word of `query`, best first: a word
// that is a keyword beats one that starts one, which beats one inside one,
// and among equals, the one with fewer keywords, whose name is closer to
// what you typed (`fire md` before `fire alert md`), then the one earlier in
// `icons`. With no query, all of them in their own order.
export function filterIcons(icons, query) {
  const words = String(query || "").toLowerCase().trim().split(/\s+/).filter(Boolean)
  if (words.length === 0) return (icons || []).slice()
  const out = []
  for (let index = 0; index < (icons || []).length; index++) {
    const icon = icons[index]
    const keywords = " " + icon.keywords + " "
    let score = 0
    for (const word of words) {
      if (keywords.includes(" " + word + " ")) score += 3
      else if (keywords.includes(" " + word)) score += 2
      else if (keywords.includes(word)) score += 1
      else {
        score = -1
        break
      }
    }
    if (score >= 0) out.push({ icon, index, score, length: icon.keywords.split(" ").length })
  }
  return out
    .sort((a, b) => b.score - a.score || a.length - b.length || a.index - b.index)
    .map((entry) => entry.icon)
}

// Where the cursor goes in a grid of `count` cells, `columns` wide, from
// `from`, `dx` across and `dy` down: across wraps from row to row, down stops
// at the last cell. From nothing (-1), the first cell.
export function gridMove(from, count, columns, dx, dy) {
  if (count <= 0) return -1
  if (from < 0) return 0
  const next = from + dx + dy * Math.max(1, columns)
  if (dx !== 0) return (next + count) % count
  return Math.max(0, Math.min(count - 1, next))
}

// Where the cursor is: on the row with the id you moved it to (`picked`),
// while it's there, and otherwise where it starts, or, searching, on the best.
export function cursorIndex(rows, query, picked) {
  const index = picked ? rows.findIndex((row) => row.id === picked) : -1
  if (index >= 0) return index
  return String(query || "").trim() ? 0 : initialIndex(rows)
}

// Where the cursor starts: on the space you left most recently, so opening
// the sidebar and pressing Enter goes back. Failing that, the active one.
export function initialIndex(rows) {
  let best = -1
  for (let i = 0; i < rows.length; i++) {
    if (rows[i].active || rows[i].leftAt <= 0) continue
    if (best < 0 || rows[i].leftAt > rows[best].leftAt) best = i
  }
  if (best >= 0) return best
  const active = rows.findIndex((row) => row.active)
  return active >= 0 ? active : 0
}

// ---------------------------------------------------------------- motion
//
// The sidebar moves with the windows it pushes aside, on Hyprland's own
// curve and duration, so no gap opens between them.

// Hyprland's curve when an animation names `default`.
const DEFAULT_BEZIER = [0, 0.75, 0.15, 1]

// How Hyprland moves windows, from `hyprctl animations -j`:
// `{ duration (ms), bezier: [x0, y0, x1, y1] }`, with duration 0 when the
// animation is off, or null when the output can't be read. `windowsMove`
// inherits from `windows`, which inherits from `global`, unless overridden.
export function windowMotion(text) {
  let data
  try {
    data = JSON.parse(String(text || ""))
  } catch (e) {
    return null
  }
  if (!Array.isArray(data) || !Array.isArray(data[0])) return null
  const animations = new Map(data[0].map((a) => [a.name, a]))
  const curves = Array.isArray(data[1]) ? data[1] : []

  let leaf = null
  for (const name of ["windowsMove", "windows", "global"]) {
    const animation = animations.get(name)
    if (animation && (animation.overridden || name === "global")) {
      leaf = animation
      break
    }
  }
  if (!leaf) return null
  if (!leaf.enabled) return { duration: 0, bezier: DEFAULT_BEZIER }

  const curve = curves.find((c) => c.name === leaf.bezier)
  return {
    duration: Math.round(Number(leaf.speed) * 100),
    bezier: curve ? [curve.X0, curve.Y0, curve.X1, curve.Y1] : DEFAULT_BEZIER,
  }
}

// The left of Hyprland's outer gaps, from `hyprctl getoption general:gaps_out
// -j`: the windows' distance from what reserves the screen's edge. Hyprland
// prints the four sides as CSS does (`css`, or `custom` before it), or one
// number for all (`int`). Null when the output can't be read.
export function gapLeft(text) {
  let data
  try {
    data = JSON.parse(String(text || ""))
  } catch (e) {
    return null
  }
  if (!data || typeof data !== "object") return null
  if (typeof data.int === "number") return Math.max(0, data.int)
  const sides = String(data.css ?? data.custom ?? "").trim().split(/\s+/).map(Number)
  if (sides.length === 0 || sides.some((n) => !Number.isFinite(n))) return null
  // CSS order: top, right, bottom, left; fewer values repeat as CSS repeats them.
  const left = sides.length === 4 ? sides[3] : sides.length >= 2 ? sides[1] : sides[0]
  return Math.max(0, left)
}

// A space's badge: two letters for its name, as T3 Code marks its projects.
// The first letter, then the first digit after it in the first word, or the
// last word's first letter, or the first word's last letter: `rencal` is RL,
// `Tax 2026` is T2, `my notes` is MN. A one-letter name keeps its one letter.
//
// The shell's JavaScript has no Unicode classes (`\p{L}`), so a word is a run
// of anything but spaces and ASCII punctuation, and a digit an ASCII one.
export function monogram(name) {
  const text = String(name || "")
  const words = (text.normalize ? text.normalize("NFKC") : text).trim().match(/[^\s!-/:-@[-`{-~]+/g) || []
  if (words.length === 0) return "?"
  const glyphs = Array.from(words[0])
  const first = glyphs[0]
  const digit = glyphs.slice(1).find((glyph) => /[0-9]/.test(glyph))
  let second = digit
  if (!second && words.length > 1) second = Array.from(words[words.length - 1])[0]
  if (!second && glyphs.length > 1) second = glyphs[glyphs.length - 1]
  return Array.from((first + (second || "")).toUpperCase()).slice(0, 2).join("")
}

// The colour of a space's badge, from `palette`, by its name: the same name
// always takes the same colour.
export function badgeColor(name, palette) {
  if (!palette || palette.length === 0) return ""
  const text = String(name || "")
  const seed = (text.normalize ? text.normalize("NFKC") : text).trim().toLowerCase() || "space"
  let index = 0
  for (const glyph of seed) index = (index * 31 + glyph.codePointAt(0)) % palette.length
  return palette[index]
}

// What badges are coloured with when the theme doesn't say.
export const DEFAULT_PALETTE = ["#f87171", "#fb923c", "#facc15", "#4ade80", "#2dd4bf", "#60a5fa", "#a78bfa", "#f472b6"]

// The theme's named colours from its colors.toml: { red: "#f7768e", ... }.
export function themeColors(text) {
  const colors = {}
  for (const line of String(text || "").split("\n")) {
    const match = line.match(/^\s*([a-z_]+)\s*=\s*"(#[0-9a-fA-F]{6})"/)
    if (match) colors[match[1]] = match[2].toLowerCase()
  }
  return colors
}

// The colours for badges from an Omarchy theme's colors.toml: its hues, each
// once, in a fixed order, or the default palette when it names too few.
export function themePalette(text) {
  const colors = themeColors(text)
  const palette = []
  for (const hue of ["red", "orange", "yellow", "green", "cyan", "blue", "magenta"]) {
    if (colors[hue] && !palette.includes(colors[hue])) palette.push(colors[hue])
  }
  return palette.length >= 3 ? palette : DEFAULT_PALETTE
}

// The widest any tab may be for `widths` (each tab's own) to fit in `room`:
// tabs narrower than an equal share keep their width, and the rest split
// what is left. Infinity when they all fit.
export function fairCap(widths, room) {
  const sorted = widths.slice().sort((a, b) => a - b)
  let left = room
  for (let i = 0; i < sorted.length; i++) {
    const share = left / (sorted.length - i)
    if (sorted[i] > share) return share
    left -= sorted[i]
  }
  return Infinity
}

// How far along a cubic Bézier easing is at `x` (0 to 1) of its time, as
// Hyprland and CSS ease: `bezier` is [x1, y1, x2, y2], the curve running from
// (0, 0) to (1, 1). It solves the curve's x for its parameter, by Newton's
// method and bisection where that stalls, then reads its y there.
export function ease(bezier, x) {
  if (x <= 0) return 0
  if (x >= 1) return 1
  const [x1, y1, x2, y2] = bezier
  const at = (a, b, t) => 3 * a * t * (1 - t) * (1 - t) + 3 * b * t * t * (1 - t) + t * t * t
  const slope = (a, b, t) => 3 * a * (1 - t) * (1 - t) + 6 * (b - a) * t * (1 - t) + 3 * (1 - b) * t * t
  let t = x
  for (let i = 0; i < 8; i++) {
    const d = slope(x1, x2, t)
    if (Math.abs(d) < 1e-6) break
    const next = t - (at(x1, x2, t) - x) / d
    if (next < 0 || next > 1) break
    t = next
  }
  if (Math.abs(at(x1, x2, t) - x) > 1e-5) {
    let lo = 0
    let hi = 1
    for (let i = 0; i < 40; i++) {
      t = (lo + hi) / 2
      if (at(x1, x2, t) < x) lo = t
      else hi = t
    }
  }
  return at(y1, y2, t)
}

// How dark a shade lies over the bar's colour where the background shows,
// beside the space's windows and tabs: over the sidebar, and over the bar
// above it. Given the bar's colour (0 to 1 a channel), a light one takes a
// lighter shade, which is enough to set it back.
export function recessAlpha(r, g, b) {
  const luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b
  return luminance > 0.5 ? 0.07 : 0.3
}

// ---------------------------------------------------------------- slots
//
// The `com.t4t5.spaces` entry on the bar has a `tabs` slot (past the sidebar's
// edge), set by two keys kept apart, so neither is guessed from the other's
// shape: `tabs`, the settings of Spaces' own tabs (or `false` for none), and
// `tabsWidget`, a bar widget entry to show in their place, as the bar's own
// layout takes one: a registered bar widget by its id, or a custom QML module
// (`{ id, type: "qml" }` or `{ source }`), with the rest of the entry as its
// settings. The sidebar's settings are in `sidebar`, its widgets among them,
// in entries of the same form.

// Whether the sidebar's header, its toggle and title, sits on the bar: when
// the widget `id` comes first in the bar's left section, on a bar along the
// top or bottom, so the corner over the sidebar is its own. With something
// before it, as Omarchy's menu, the sidebar keeps its header.
export function headerOnBar(config, id) {
  if (!config || config.position === "left" || config.position === "right") return false
  const left = config.layout && config.layout.left
  return Array.isArray(left) && !!left[0] && (left[0] === id || left[0].id === id)
}

// A bar widget entry, a string id or an object, as `{ id, settings }`, the
// settings being the rest of it; or null when it names nothing.
export function parseBarEntry(entry) {
  if (typeof entry === "string") {
    const id = entry.trim()
    return id ? { id, settings: {} } : null
  }
  if (!entry || typeof entry !== "object" || Array.isArray(entry)) return null
  const id = typeof entry.id === "string" ? entry.id.trim() : ""
  const source = typeof entry.source === "string" ? entry.source.trim() : ""
  const type = typeof entry.type === "string" ? entry.type.trim() : ""
  if (!id && !source && !type) return null
  const settings = {}
  for (const key of Object.keys(entry)) if (key !== "id") settings[key] = entry[key]
  return { id, settings }
}

// The settings on the bar's `id` entry from `barConfig.layout`, or `{}`.
export function pluginSettings(config, id) {
  const layout = config && config.layout
  if (!layout || typeof layout !== "object") return {}
  for (const section of ["left", "centerBegin", "center", "centerEnd", "right"]) {
    const items = layout[section]
    if (!Array.isArray(items)) continue
    for (const item of items) {
      if (item && typeof item === "object" && item.id === id) {
        const out = {}
        for (const key of Object.keys(item)) if (key !== "id") out[key] = item[key]
        return out
      }
    }
  }
  return {}
}

// The sidebar's settings, from `sidebar` on Spaces' entry: `{ width, widgets,
// misplaced }`. `width` is the expanded width (0 for the default), and
// `widgets` the bar widget entries in `sidebar.space.widgets`, shown under
// each space's name, in order. Spaces' own id is left out, as it can't hold
// itself. `misplaced` says `sidebar` looks like a widget entry, which goes in
// `sidebar.space.widgets`.
export function sidebarSlot(settings, ownId) {
  const sidebar = settings ? settings.sidebar : undefined
  const plain = isPlainObject(sidebar)
  const width = plain && typeof sidebar.width === "number" && Number.isFinite(sidebar.width) && sidebar.width > 0
    ? Math.round(sidebar.width)
    : 0
  const space = plain && isPlainObject(sidebar.space) ? sidebar.space : {}
  const widgets = (Array.isArray(space.widgets) ? space.widgets : [])
    .map(parseBarEntry)
    .filter((entry) => entry && entry.id !== ownId)
  return {
    width,
    widgets,
    misplaced: typeof sidebar === "string" || (plain && ("id" in sidebar || "source" in sidebar || "type" in sidebar)),
  }
}

function isPlainObject(value) {
  return !!value && typeof value === "object" && !Array.isArray(value)
}

// How wide each of a space's widgets may be, from how wide each would be and
// the room on the line, `gap` apart: each as wide as it would be, or, when
// they don't all fit, no wider than a fair share (`fairCap`).
export function widgetWidths(widths, room, gap) {
  const shown = widths.filter((w) => w > 0).length
  const cap = fairCap(widths, Math.max(0, room - gap * Math.max(0, shown - 1)))
  return widths.map((w) => Math.min(w, cap))
}

// The `tabs` slot as Spaces' `settings` set it: `{ shown, settings, widget,
// misplaced }`. `shown` is whether Spaces' own tabs may show (`tabs !== false`),
// `settings` are Spaces' own tabs' (`tabs`, when it's an object), and `widget`
// is the entry in `tabsWidget`, or null. `tabsWidget` wins over `tabs: false`.
// `misplaced` says `tabs` looks like a widget entry, which goes in `tabsWidget`.
export function tabsSlot(settings) {
  const tabs = settings ? settings.tabs : undefined
  const plain = !!tabs && typeof tabs === "object" && !Array.isArray(tabs)
  return {
    shown: tabs !== false,
    settings: plain ? tabs : {},
    widget: settings ? parseBarEntry(settings.tabsWidget) : null,
    misplaced: typeof tabs === "string" || (plain && ("id" in tabs || "source" in tabs || "type" in tabs)),
  }
}

// Expands a leading `~/` or `$HOME/` against `home`.
export function expandPath(path, home) {
  const value = String(path || "").trim()
  if (!value) return ""
  const base = String(home || "").replace(/\/$/, "")
  if (value.startsWith("~/")) return base + value.slice(1)
  if (value.startsWith("$HOME/")) return base + value.slice(5)
  return value
}

// The file:// URL of a custom QML module entry, found as the bar finds one:
// its `source`, or with `type: "qml"`, `~/.config/omarchy/bar/modules/<id>.qml`.
// "" for any other entry.
export function customModuleUrl(entry, home) {
  if (!entry) return ""
  const settings = entry.settings || {}
  if (settings.type && settings.type !== "qml") return ""
  if (typeof settings.source === "string" && settings.source.trim()) return fileUrl(expandPath(settings.source, home))
  if (settings.type !== "qml" || !entry.id || entry.id.includes("..") || entry.id.startsWith("/")) return ""
  return fileUrl(String(home || "").replace(/\/$/, "") + "/.config/omarchy/bar/modules/" + entry.id + ".qml")
}

// The numeral shown for a tab slot `1..10`: its own (`"1"`–`"10"`), the one at
// its place in a custom `numerals` list (as kanji), or "" with `numerals`
// false or without a slot.
export function numeralFor(slot, numerals) {
  if (!(slot > 0) || numerals === false) return ""
  if (Array.isArray(numerals)) {
    const custom = numerals[slot - 1]
    if (custom !== undefined && custom !== null && String(custom) !== "") return String(custom)
  }
  return String(slot)
}

// A terminal window as a widget in a slot sees it.
export function publicTerminal(t) {
  return {
    address: String(t && t.address || ""),
    tab: String(t && t.tab || ""),
    folder: String(t && t.folder || ""),
    program: String(t && t.program || ""),
    label: String(t && t.label || ""),
    status: status(t && t.status),
  }
}

// The space shown on a monitor as a widget in a slot sees it, apart from
// `space watch`'s own form of it.
export function publicSpace(state, id) {
  const spaceId = String(id || "")
  const row = spaceId && state && Array.isArray(state.spaces)
    ? state.spaces.find((s) => s && s.id === spaceId)
    : null
  return {
    id: spaceId,
    label: row && row.label ? String(row.label) : spaceId,
    named: !!row && row.named === true,
    icon: row && typeof row.icon === "string" ? row.icon : "",
    folder: row && typeof row.folder === "string" ? row.folder : "",
    meta: plainObject(row && row.meta),
    status: row ? status(row.status) : "",
    terminals: row && Array.isArray(row.terminals) ? row.terminals.map(publicTerminal) : [],
  }
}

// A space as a widget sees it: in `spaces` for a tabs widget, and as `space`
// for a widget under its name in the sidebar.
export function publicSpaceRow(row) {
  if (!row) {
    return { id: "", label: "", named: false, icon: "", folder: "", meta: {}, status: "", active: false, windows: -1, leftAt: 0, terminals: [] }
  }
  return {
    id: String(row.id || ""),
    label: row.label ? String(row.label) : String(row.id || ""),
    named: row.named === true,
    icon: typeof row.icon === "string" ? row.icon : "",
    folder: typeof row.folder === "string" ? row.folder : "",
    meta: plainObject(row.meta),
    status: status(row.status),
    active: row.active === true,
    windows: typeof row.windows === "number" ? row.windows : -1,
    leftAt: number(row.leftAt),
    terminals: Array.isArray(row.terminals) ? row.terminals.map(publicTerminal) : [],
  }
}

// A tab as a widget in the `tabs` slot sees it, apart from `space watch`'s
// own form of it.
export function publicTab(tab) {
  return {
    id: String(tab.id || ""),
    slot: number(tab.slot),
    label: String(tab.label || ""),
    named: tab.named === true,
    status: status(tab.status),
    active: tab.active === true,
  }
}

// The bar's background, as Omarchy's shell config holds it: opaque, glassy
// (see-through, blurred behind) or none (clear). Double-clicking the bar steps
// through them. A config from before `background` says `transparent` instead.
export function barLook(config) {
  const look = config && config.background
  if (look === "opaque" || look === "glassy" || look === "none") return look
  return config && config.transparent === true ? "none" : "opaque"
}

// Dragging a space up or down the sidebar. A press becomes a drag only once
// it has moved `threshold` up or down, so a click that wobbles stays a click.
// `home` stays first: nothing drags it, and nothing drops above it.
export function startsDrag(row, dy, threshold) {
  return !!row && row.id !== "home" && Math.abs(dy) >= threshold
}

// Where a row dragged `dy` from `from` would drop, rows being `pitch` apart.
export function dropIndex(from, dy, pitch, count) {
  const to = from + Math.round(dy / Math.max(1, pitch))
  return Math.max(1, Math.min(count - 1, to))
}

// How far the row at `index` moves aside while another is dragged from
// `from` to `to`, to open the gap it would drop into.
export function dragShift(index, from, to, pitch) {
  if (from < 0 || index === from) return 0
  if (from < to && index > from && index <= to) return -pitch
  if (to < from && index >= to && index < from) return pitch
  return 0
}

// The tabs as the bar shows them: each numbered tab after the numbers before
// it that no tab has, as Omarchy shows its empty workspaces, faded. One empty
// number shows as itself; two or more in a row as one `…`, which goes to the
// first. Each is { tab, index } for the tab at `index`, or { gap, run,
// before } for the empty number `gap` (the first of a run, if `run`) before
// the tab at `before`. No gaps after the last tab, nor among those without a
// number. Only `home`'s have gaps to show (`withGaps`): a space's tabs are
// numbered again as one ends (`space renumber`), so a gap there is gone in a
// moment.
export function tabCells(tabs, withGaps) {
  const cells = []
  let last = 0
  tabs.forEach((tab, index) => {
    if (withGaps && tab.slot > 0) {
      if (tab.slot > last + 1) cells.push({ gap: last + 1, run: tab.slot > last + 2, before: index })
      last = Math.max(last, tab.slot)
    }
    cells.push({ tab, index })
  })
  return cells
}

// The room the empty numbers take before each tab, from the cells' widths and
// the row's spacing: what a tab dragged over them passes as well.
export function tabGaps(cells, widths, spacing) {
  const gaps = []
  let room = 0
  cells.forEach((cell, i) => {
    if (cell.tab) {
      gaps.push(room)
      room = 0
    } else {
      room += (widths[i] || 0) + spacing
    }
  })
  return gaps
}

// How far a tab dragged `dx` from `from` goes, kept among the tabs, which are
// `widths` wide and `spacing` apart, with `gaps` of empty numbers before
// each.
export function tabDragOffset(widths, from, dx, spacing, gaps) {
  const gap = (i) => (gaps && gaps[i]) || 0
  let left = 0
  for (let i = 0; i < from; i++) left += widths[i] + spacing + gap(i + 1)
  let right = 0
  for (let i = from + 1; i < widths.length; i++) right += gap(i) + widths[i] + spacing
  return Math.max(-left, Math.min(right, dx))
}

// Where it would drop: past each tab whose middle its leading edge has
// crossed, as tabs aren't equally wide.
export function tabDropIndex(widths, from, dx, spacing, gaps) {
  const gap = (i) => (gaps && gaps[i]) || 0
  let to = from
  let reach = 0
  if (dx > 0) {
    for (let i = from + 1; i < widths.length; i++) {
      if (dx <= reach + gap(i) + spacing + widths[i] / 2) break
      reach += gap(i) + spacing + widths[i]
      to = i
    }
  } else {
    for (let i = from - 1; i >= 0; i--) {
      if (-dx <= reach + gap(i + 1) + spacing + widths[i] / 2) break
      reach += gap(i + 1) + spacing + widths[i]
      to = i
    }
  }
  return to
}

// How far each tab moves aside while the one at `from` is dragged to `to`,
// and each tab's empty numbers before it: to where they will be once it
// drops. The numbers stay with their places, as the tabs' numbers do, so
// they move only as much as the tabs before them change width.
export function tabShifts(widths, from, to, spacing, gaps) {
  const count = widths.length
  const none = { tabs: widths.map(() => 0), gaps: widths.map(() => 0) }
  if (from < 0 || to < 0 || from === to) return none
  const gap = (i) => (gaps && gaps[i]) || 0
  const order = moveRow(widths.map((_, i) => i), from, to)
  // Where each place starts, with the tabs in an order.
  const starts = (list) => {
    const out = []
    let x = 0
    for (let p = 0; p < count; p++) {
      x += gap(p)
      out.push(x)
      x += widths[list[p]] + spacing
    }
    return out
  }
  const before = starts(widths.map((_, i) => i))
  const after = starts(order)
  const tabs = widths.map(() => 0)
  order.forEach((tab, p) => { tabs[tab] = tab === from ? 0 : after[p] - before[tab] })
  return { tabs, gaps: before.map((x, p) => gap(p) ? after[p] - x : 0) }
}

// The tabs once `space tab reorder` has moved the one at `from` to
// `to`: the same numbers, with what's on them moved along.
export function reorderTabs(model, from, to) {
  return {
    space: model.space,
    tabs: moveRow(model.tabs, from, to).map((tab, i) => Object.assign({}, tab, { id: model.tabs[i].id, slot: model.tabs[i].slot })),
  }
}

// Whether two lists of tabs show the same, whatever their agents are doing.
export function sameTabs(a, b) {
  return a.space === b.space && a.tabs.length === b.tabs.length && a.tabs.every((tab, i) => {
    const other = b.tabs[i]
    return tab.slot === other.slot && tab.label === other.label && tab.windows === other.windows
      && tab.active === other.active
  })
}

// The rows in an order they were dragged into, by their ids, that `space
// watch` hasn't printed yet. An order of other spaces than these says nothing.
export function ordered(rows, order) {
  if (!order || order.length !== rows.length) return rows
  const out = order.map((id) => rows.find((row) => row.id === id))
  return out.every(Boolean) ? out : rows
}

// Whether the rows are in that order, or no longer can be.
export function orderSettled(rows, order) {
  return ordered(rows, order).every((row, i) => row === rows[i])
}

// The rows with the one at `from` moved to `to`.
export function moveRow(rows, from, to) {
  const out = rows.slice()
  const [row] = out.splice(from, 1)
  out.splice(to, 0, row)
  return out
}
