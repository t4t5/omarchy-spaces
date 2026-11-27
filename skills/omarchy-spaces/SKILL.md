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
  - `~/.config/omarchy/shell.json` — the `tabs`, `tabsWidget`, and `rowWidget` keys on the `com.t4t5.spaces` entry. Applied when saved.
  - `~/.config/omarchy/bar/modules/<id>.qml` (or any file named by `source`) — custom QML widgets for `tabsWidget` and `rowWidget`.
  - `~/.config/omarchy-spaces/config.toml` — restore, sound, custom menu actions (`[[actions.space]]`, `[[actions.tab]]`), and event hooks (`[hooks]`). Applied live.
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

A widget in `tabsWidget` is an ordinary bar widget (`bar`, `moduleName`, `settings`), plus `maxWidth` (room before the clock) and `spaces`: `version` (now `1`; it goes up when a field changes or goes, so check it), `space` (`{ id, label, named, icon, folder, meta, status }`), `tabs` (`{ id, slot, label, named, status, active }`), `sidebarOpen` (on this screen), `renaming`, `renameText`, `renameSelected`, `goToTab(idOrNumber)`, `newTab()`, `renameTab(id)`, `reorderTab(id, position)`, `openTabMenu(id, x, y)` (window coordinates from `mapToItem(null, x, y)`), `toggleSidebar()`, `runAction(id, spaceId, tabId)`. Treat ids as opaque. These properties are assigned once the widget loads, so bind to them rather than marking them `required` or reading them in `Component.onCompleted`. Set `textFormat: Text.PlainText` on any `Text` showing tab or space names. A new or edited `.qml` file may need `omarchy restart shell` before it loads.

To render extra detail under each space's name in the expanded sidebar (such as git branch/dirty state or worktree tags from `space meta`), set `rowWidget` on the `com.t4t5.spaces` entry in `~/.config/omarchy/shell.json` using the same entry forms as `tabsWidget`. Spaces injects `space` (`{ id, label, named, icon, folder, meta, status, active, windows }`), `settings`, `foreground`, `accent`, `fontFamily`, `expanded`, `selected`, and `spaces` (`{ version: 1, goToSpace(id), runAction(id, spaceId) }`). Complete git status and worktree recipes: `~/.config/omarchy/plugins/com.t4t5.spaces/docs/guide/extending.md`.

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

# Optional right-click menu actions (also callable via `space action <id>` or `spaces.action("<id>")`)
[[actions.space]]
id = "new-worktree"
label = "New worktree…"
icon = "󰘬"
when = "folder"   # clauses: folder, home, status, status=<v>, meta.<path>[=v|!=v], !negation
command = "~/.config/omarchy-spaces/scripts/new-worktree.sh"

# Optional event hooks (receive event JSON on stdin + SPACE_EVENT/SPACE_ID/SPACE_DIR/SPACE_META env)
[hooks]
"space.created" = "~/.config/omarchy-spaces/scripts/git-meta.sh"
"space.focused" = "~/.config/omarchy-spaces/scripts/git-meta.sh"
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
- **Spaces**: `toggle_sidebar()`, `find_space()`, `new_space()`, `next_space()`, `prev_space()`, `rename_space()`, `action(id)`.
- **Tabs**: `bind_tab_keys()`, `new_tab()`, `go_to_tab(n)`, `next_tab()`, `prev_tab()`, `rename_tab()`, `send_tab([target])`.
- **Windows**: `terminal()`, `move_window_to_tab(n[, follow])`, `send_window([target[, follow]])`.

## CLI & IPC quick reference

A command given no space or tab acts on the one it's run from (the window it runs in), so an agent changes its own space, not the one the user is looking at. Without a window to go by, as in tmux, `SPACE_ID` gives the space but not the tab: give tab commands the tab's id.

```bash
space ls --json                 # spaces, tabs, data and agent status
space current [--json]          # the space and tab a command run here acts on
space new [dir] --silent --json [-- <cmd...>] # open a space without going there; prints its id; optionally runs a command
space tab new --silent --json [-- <cmd...>]   # the same for a tab; -s <space> for another space
space terminal [-s <space>] [-- <cmd...>]     # open a terminal in a space's folder, optionally running a command
space go <space>                # go to a space
space rename [space] <name>     # "" for its folder's name
space icon [space] <icon>       # a glyph, emoji or image path; "" for its letters
space meta get|set|rm <key>     # data of your own on a space; set takes a value, and --json
space action <id> [-s <space>] [-t <tab>]     # run a custom action from config.toml
space events                    # a line of JSON per change
space tab go <n|id> | next | prev | rename [tab] <name> | send [tab] <space> | close [tab]
space window send <space|next|prev> [--silent]
omarchy-shell shell call com.t4t5.spaces toggleSidebar ''
```

Every command and its JSON: `~/.config/omarchy/plugins/com.t4t5.spaces/docs/guide/cli.md`.
