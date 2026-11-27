---
name: omarchy-spaces
description: >
  Customize and control Omarchy Spaces (com.t4t5.spaces): project-based spaces and tabs on Omarchy/Hyprland. Use when customizing Spaces views (custom QML tab views such as Japanese numerals, hiding or replacing the + button or bar header), editing ~/.config/omarchy-spaces/config.toml (restore, agent sounds), setting up Spaces keybindings in ~/.config/hypr/bindings.lua, or running the `space` CLI. Triggers: omarchy-spaces, com.t4t5.spaces, spaces bar, spaces sidebar, spaces tabs, space CLI, spaces keybindings, spaces config, custom tab view.
---

# Omarchy Spaces

Omarchy Spaces (`com.t4t5.spaces`) adds project-based spaces (sidebar rail on the left) and per-space tabs on Omarchy's bar. Each tab is a Hyprland workspace (`s1:1`, `s1:2`, …); plain numbered workspaces belong to `home`.

## Safe customization rules

- **Never edit files inside `~/.config/omarchy/plugins/com.t4t5.spaces/`** — plugin updates overwrite them.
- Edit user config instead:
  - `~/.config/omarchy/shell.json` — explicit view tree on the `com.t4t5.spaces` entry. Hot-reloads on save.
  - `~/.config/omarchy/spaces/*.qml` (or any path you reference in `shell.json`) — custom QML views. Reloaded live when saved.
  - `~/.config/omarchy-spaces/config.toml` — restore and sound settings. Applied live.
  - `~/.config/hypr/bindings.lua` — Spaces keybindings. Validate with `hyprctl reload && hyprctl configerrors`.
- Do **not** clone `omarchy.workspaces` to customize Spaces tabs: `omarchy.workspaces` only reads numeric Hyprland workspaces, whereas Spaces uses named workspaces (`s1:1`, …) and drives tabs from `space watch`. Swap the `tabs.tab` view instead.
- To place other Omarchy bar widgets beside Spaces, add them before or after `com.t4t5.spaces` in `bar.layout.left` (widgets after it already slide with it; `omarchy.menu` before it takes the corner while the sidebar keeps its header).

## Swappable views (`~/.config/omarchy/shell.json`)

Spaces keeps all behaviour and framing (clicks, drag-to-reorder, inline rename, context menus, width cap before the clock, sliding edge). Views only draw. Every node on the `com.t4t5.spaces` entry follows one rule:

- omitted: built-in default
- `"~/path/to/View.qml"`: replaces that node with your QML view
- `{ ... }`: keeps the built-in host and overrides child nodes
- `false`: hides optional nodes (`tabs.newTab`, `sidebar.header`)

```json
{
  "id": "com.t4t5.spaces",
  "tabs": {
    "tab": "~/.config/omarchy/spaces/KanjiTab.qml",
    "newTab": false
  },
  "sidebar": {
    "header": false
  }
}
```

### View contract (`api: 1`)

Every custom view must declare `readonly property int api: 1`, report `implicitWidth` and `implicitHeight` (never derive `implicitWidth` from `width`), and set `textFormat: Text.PlainText` on any `Text` displaying dynamic tab/space labels. On a load error or `api` mismatch, Spaces falls back to the built-in view and notifies once.

- **`tabs.tab`**: receives `tab` (`{ id, slot, label, named, status, windows, active }` or `null` on an empty gap in `home`), `slot` (int), `gap` (bool), `gapRun` (bool, two+ consecutive gaps), `current`, `hovered`, `dragged`, `menuOpen`, `editing`, `vertical`, `theme`, and pre-bound `parts`:
  - `parts.status`: pre-bound `StatusDot` `Component` — mount with `Loader { active: !!tab && tab.status !== ""; sourceComponent: parts ? parts.status : null }`.
  - `parts.editor`: pre-bound inline name editor (text + selection + caret, without the slot number) — mount with `Loader { active: editing && !vertical; sourceComponent: parts ? parts.editor : null }`.
- **`tabs.newTab`**: receives `hovered`, `vertical`, `count`, `theme`.
- **`tabs` (full strip path)**: replaces the entire strip inside the sliding frame; receives `space`, `tabs`, `cells`, `vertical`, `maxWidth`, `theme`, and `actions` (`goToTab(target)`, `newTab()`, `openTabMenu(id, x, y)`).
- **`theme`**: `{ foreground, accent, hover, fontFamily, fontSize, barSize, padding, cornerRadius, statusColors, palette }`.

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
