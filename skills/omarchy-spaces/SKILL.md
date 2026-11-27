---
name: omarchy-spaces
description: >
  Customize and control Omarchy Spaces (com.t4t5.spaces): project-based spaces and tabs on Omarchy/Hyprland. Use when customizing the Spaces bar (tab numerals such as kanji, hiding labels or the + button, or putting another bar widget or a custom QML widget in its header or tabs slot), editing ~/.config/omarchy-spaces/config.toml (restore, agent sounds), setting up Spaces keybindings in ~/.config/hypr/bindings.lua, or running the `space` CLI. Triggers: omarchy-spaces, com.t4t5.spaces, spaces bar, spaces sidebar, spaces tabs, space CLI, spaces keybindings, spaces config, custom tabs widget.
---

# Omarchy Spaces

Omarchy Spaces (`com.t4t5.spaces`) adds project-based spaces (sidebar rail on the left) and per-space tabs on Omarchy's bar. Each tab is a Hyprland workspace (`s1:1`, `s1:2`, …); plain numbered workspaces belong to `home`.

## Safe customization rules

- **Never edit files inside `~/.config/omarchy/plugins/com.t4t5.spaces/`** — plugin updates overwrite them.
- Edit user config instead:
  - `~/.config/omarchy/shell.json` — the `header` and `tabs` slots on the `com.t4t5.spaces` entry. Applied when saved.
  - `~/.config/omarchy/bar/modules/<id>.qml` (or any file named by `source`) — custom QML widgets for a slot.
  - `~/.config/omarchy-spaces/config.toml` — restore and sound settings. Applied live.
  - `~/.config/hypr/bindings.lua` — Spaces keybindings. Validate with `hyprctl reload && hyprctl configerrors`.
- Do **not** clone `omarchy.workspaces` to customize Spaces tabs: `omarchy.workspaces` only reads numeric Hyprland workspaces, whereas Spaces uses named workspaces (`s1:1`, …) and drives tabs from `space watch`. Put a widget in Spaces' `tabs` slot instead.
- To place other Omarchy bar widgets beside Spaces, add them before or after `com.t4t5.spaces` in `bar.layout.left` (widgets after it already slide with it; `omarchy.menu` before it takes the corner while the sidebar keeps its header).

## Customizing the bar (`~/.config/omarchy/shell.json`)

The `com.t4t5.spaces` entry has two slots: `header` (the corner over the sidebar) and `tabs` (past the sidebar's edge). Each takes a bar widget entry, written as in the bar's own layout. Full reference, including what a widget gets: `~/.config/omarchy/plugins/com.t4t5.spaces/docs/guide/sidebar.md`.

Spaces' own tabs take settings from an object with neither an `id` nor a `source` (a `source` makes it a custom module):

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

To replace a slot, give it a widget entry:
- A registered bar widget by id: `"tabs": "io.github.someone.kanji-tabs"`. Also add `{ "id": "io.github.someone.kanji-tabs" }` to the top-level `plugins` list, or the shell won't load it.
- A custom QML widget: `"tabs": { "id": "my-tabs", "type": "qml" }` (loads `~/.config/omarchy/bar/modules/my-tabs.qml`) or `{ "source": "~/path/MyTabs.qml" }`. Other keys become its `settings`.
- `"header": false` gives the corner back to the sidebar, which draws its own header. A widget in `header` replaces that header, so it should call `spaces.toggleSidebar()`.
- Until a widget loads, the slot shows Spaces' own.

A slot widget is an ordinary bar widget (`bar`, `moduleName`, `settings`), plus `maxWidth` (in `tabs`: room before the clock) and `spaces`: `space`, `tabs` (`{ id, slot, label, named, status, active }`), `sidebarOpen`, `renaming`, `renameText`, `goToTab(idOrNumber)`, `newTab()`, `renameTab(id)`, `reorderTab(id, position)`, `openTabMenu(id, x, y)` (window coordinates from `mapToItem(null, x, y)`), `toggleSidebar()`. Set `textFormat: Text.PlainText` on any `Text` showing tab names. A new `.qml` file in a folder the shell has already loaded from may need `omarchy restart shell` before it loads.

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

```bash
space ls --json                     # inspect spaces, tabs, slots, and agent status
space new [dir]                     # open a new space
space go <space>                    # switch space
space rename [space] <name>         # rename space ("" resets to folder name)
space icon [space] <icon-or-path>   # glyph, emoji, or image path ("" resets)
space tab new | go <n|id> | next | prev | rename [tab] <name> | send [tab] <space> | close [tab]
space window send <space|next|prev> [--silent]
omarchy-shell shell call com.t4t5.spaces toggleSidebar ''
```
