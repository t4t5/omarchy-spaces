---
name: omarchy-spaces
description: >
  Customize and control Omarchy Spaces (com.t4t5.spaces): project-based spaces and tabs on Omarchy/Hyprland. Use when customizing the Spaces bar (tab numerals such as kanji, hiding labels or the + button, or putting another bar widget or a custom QML widget in place of its tabs), editing ~/.config/omarchy-spaces/config.toml (restore, agent sounds), setting up Spaces keybindings in ~/.config/hypr/bindings.lua, or running the `space` CLI. Triggers: omarchy-spaces, com.t4t5.spaces, spaces bar, spaces sidebar, spaces tabs, space CLI, spaces keybindings, spaces config, custom tabs widget.
---

# Omarchy Spaces

Omarchy Spaces (`com.t4t5.spaces`) adds project-based spaces (sidebar rail on the left) and per-space tabs on Omarchy's bar. Each tab is a Hyprland workspace (`s1:1`, `s1:2`, …); plain numbered workspaces belong to `home`.

## Safe customization rules

- **Never edit files inside `~/.config/omarchy/plugins/com.t4t5.spaces/`** — plugin updates overwrite them.
- Edit user config instead:
  - `~/.config/omarchy/shell.json` — the `tabs` and `tabsWidget` keys on the `com.t4t5.spaces` entry. Applied when saved.
  - `~/.config/omarchy/bar/modules/<id>.qml` (or any file named by `source`) — custom QML widgets for `tabsWidget`.
  - `~/.config/omarchy-spaces/config.toml` — restore and sound settings. Applied live.
  - `~/.config/hypr/bindings.lua` — Spaces keybindings. Validate with `hyprctl reload && hyprctl configerrors`.
- Do **not** clone `omarchy.workspaces` to customize Spaces tabs: `omarchy.workspaces` only reads numeric Hyprland workspaces, whereas Spaces uses named workspaces (`s1:1`, …) and drives tabs from `space watch`. Put a widget in Spaces' `tabsWidget` instead.
- To place other Omarchy bar widgets beside Spaces, add them before or after `com.t4t5.spaces` in `bar.layout.left` (widgets after it already slide with it; `omarchy.menu` before it takes the corner while the sidebar keeps its header).

## Customizing the bar (`~/.config/omarchy/shell.json`)

Two keys on the `com.t4t5.spaces` entry set the tabs (past the sidebar's edge): `tabs` holds only Spaces' own tabs' settings, and `tabsWidget` holds a bar widget entry to show instead. Never put a widget entry in `tabs`. Full reference, including what a widget gets: `~/.config/omarchy/plugins/com.t4t5.spaces/docs/guide/sidebar.md`.

Spaces' own tabs:

```json
{
  "id": "com.t4t5.spaces",
  "tabs": {
    "numerals": ["一", "二", "三", "四", "五", "六", "七", "八", "九", "十"],
    "labels": false,
    "newTab": false
  }
}
```

`numerals`: strings for tabs 1–10, or `false` for none. `labels: false` hides tab names. `newTab: false` hides `+`.

To replace them, set `tabsWidget` to a widget entry, written as in the bar's own layout:
- A registered bar widget by id: `"tabsWidget": "io.github.someone.kanji-tabs"`. Also add `{ "id": "io.github.someone.kanji-tabs" }` to the top-level `plugins` list, or the shell won't load it.
- A custom QML widget: `"tabsWidget": { "id": "my-tabs", "type": "qml" }` (loads `~/.config/omarchy/bar/modules/my-tabs.qml`) or `{ "source": "~/path/MyTabs.qml" }`. Other keys become its `settings`.
- `"tabs": false` hides Spaces' own tabs (if `tabsWidget` is also set, `tabsWidget` still shows, with no fallback to Spaces' own tabs).
- Until a widget loads, the slot shows Spaces' own unless `"tabs": false`.

A widget in `tabsWidget` is an ordinary bar widget (`bar`, `moduleName`, `settings`), plus `maxWidth` (room before the clock) and `spaces`: `version` (now `1`; it goes up when a field changes or goes, so check it), `space` (`{ id, label, named, icon, status }`), `tabs` (`{ id, slot, label, named, status, active }`), `sidebarOpen` (on this screen), `renaming`, `renameText`, `renameSelected`, `goToTab(idOrNumber)`, `newTab()`, `renameTab(id)`, `reorderTab(id, position)`, `openTabMenu(id, x, y)` (window coordinates from `mapToItem(null, x, y)`), `toggleSidebar()`. Treat ids as opaque. These properties are assigned once the widget loads, so bind to them rather than marking them `required` or reading them in `Component.onCompleted`. Set `textFormat: Text.PlainText` on any `Text` showing tab or space names. A new or edited `.qml` file may need `omarchy restart shell` before it loads.

## Configuration (`~/.config/omarchy-spaces/config.toml`)

Create this file with only the keys you want to override (changes apply immediately):

```toml
[restore]
auto = true       # reopen spaces after logout/reboot
agents = true     # resume agent conversations in terminals
browsers = true   # reopen browser windows

[sounds]
enabled = false   # chime when an agent finishes or waits for input
# done = "~/ding.oga"
# waiting = "~/dong.oga"
```

## Keybindings (`~/.config/hypr/bindings.lua`)

Wrap Spaces bindings in a `package.searchpath` guard so uninstalling Spaces cleanly restores Omarchy's defaults, and use the Lua helpers instead of raw `space` CLI calls:

```lua
if package.searchpath("hypr.omarchy-spaces", package.path) then
  local spaces = require("hypr.omarchy-spaces")

  spaces.bind_tab_keys()
  o.rebind("SUPER + B", "Show or hide spaces", spaces.toggle_sidebar())
  o.rebind("SUPER + P", "Find space", spaces.find_space())
  o.rebind("SUPER + N", "New space", spaces.new_space())
  o.rebind("SUPER + SHIFT + J", "Next space", spaces.next_space())
  o.rebind("SUPER + SHIFT + K", "Previous space", spaces.prev_space())
end
```

Available helpers on `require("hypr.omarchy-spaces")`:
- **Spaces**: `toggle_sidebar()`, `find_space()`, `new_space()`, `next_space()`, `prev_space()`, `rename_space()`.
- **Tabs**: `bind_tab_keys()`, `new_tab()`, `go_to_tab(n)`, `next_tab()`, `prev_tab()`, `rename_tab()`, `send_tab([target])`.
- **Windows**: `terminal()`, `move_window_to_tab(n[, follow])`, `send_window([target[, follow]])`.

## CLI & IPC quick reference

Commands without an explicit space or tab default to the caller's live Hyprland window (found via `/proc`), else the focused workspace when run outside a window. Terminals launched by Spaces also set `SPACE_ID` as a fallback when there is no window (e.g. inside tmux); that identifies the space but not the tab, so tab commands require an explicit tab id there.

```bash
space current [--json]                                                 # inspect caller space/tab/window and focused place
space ls --json                                                        # inspect spaces, tabs, slots, note, meta, and agent status
space new [dir] [--name <name>] [--note <text>] [--silent] [--json]    # open a space (use --silent --json in background)
space go <space>                                                       # switch space
space rename [space] <name>                                            # rename space ("" resets to folder name)
space icon [space] <icon-or-path>                                      # glyph, emoji, or image path ("" resets)
space note [space] <text>                                              # set or clear ("") a one-line checkpoint note
space meta get [-s <space>] [key]                                      # read opaque JSON metadata
space meta set [-s <space>] <key> <value> [--json]                     # write metadata (plain string unless --json)
space meta rm  [-s <space>] <key>                                      # remove metadata key
space events [--type <types>] [--no-snapshot]                          # NDJSON event stream served by `space watch` (snapshot + typed diffs)
space tab new [-s <space>] [--dir <dir>] [--name <name>] [--silent] [--json]
space tab go <n|id> | next | prev | rename [tab] <name> | send [tab] <space> | close [tab]
space window send <space|next|prev> [--silent]
omarchy-shell shell call com.t4t5.spaces toggleSidebar ''
```
