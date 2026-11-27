---
name: omarchy-spaces
description: >
  Customize and control Omarchy Spaces (com.t4t5.spaces): project-based spaces and tabs on Omarchy/Hyprland. Use when customizing the Spaces bar widget (header logo/toggle, tab slot labels such as Japanese numerals, hiding tab labels or the + button, adding custom QML bar widgets), editing ~/.config/omarchy-spaces/config.toml (restore, agent sounds), setting up Spaces keybindings in ~/.config/hypr/bindings.lua, or running the `space` CLI. Triggers: omarchy-spaces, com.t4t5.spaces, spaces bar, spaces sidebar, spaces tabs, space CLI, spaces keybindings, spaces config.
---

# Omarchy Spaces

Omarchy Spaces (`com.t4t5.spaces`) adds project-based spaces (sidebar rail on the left) and per-space tabs on Omarchy's bar. Each tab is a Hyprland workspace (`s1:1`, `s1:2`, …); plain numbered workspaces belong to `home`.

## Safe customization rules

- **Never edit files inside `~/.config/omarchy/plugins/com.t4t5.spaces/`** — plugin updates overwrite them.
- Edit user config instead:
  - `~/.config/omarchy/shell.json` — bar widget composition (`header`, `widgets`, inline options). Hot-reloads on save.
  - `~/.config/omarchy/bar/modules/<name>.qml` — custom QML bar widgets for Spaces.
  - `~/.config/omarchy-spaces/config.toml` — restore and sound settings. Applied live.
  - `~/.config/hypr/bindings.lua` — Spaces keybindings. Validate with `hyprctl reload && hyprctl configerrors`.
- Do **not** clone `omarchy.workspaces` to customize Spaces tabs: `omarchy.workspaces` only reads numeric Hyprland workspaces, whereas Spaces uses named workspaces (`s1:1`, …) and drives tabs from `space watch`. Customize `spaces.tabs` or write a Spaces-aware custom QML widget instead.

## Customizing the bar widget (`~/.config/omarchy/shell.json`)

The `com.t4t5.spaces` entry in `bar.layout.left` accepts `header` (in the shaded recess over the sidebar rail) and `widgets` (sliding beside the sidebar over the space's windows). When omitted, they default to `"spaces.header"` and `["spaces.tabs", "spaces.new-tab"]`.

```json
{
  "id": "com.t4t5.spaces",
  "header": { "id": "spaces.header", "title": "Spaces" },
  "widgets": [
    {
      "id": "spaces.tabs",
      "slotLabels": ["一", "二", "三", "四", "五", "六", "七", "八", "九", "十"],
      "showLabels": false
    },
    "spaces.new-tab"
  ]
}
```

### `header` options
- `"spaces.header"` (default): Omarchy logo over the rail (left-click toggles sidebar, right-click opens `omarchy.menu`), plus title and collapse toggle when expanded. Optional inline settings: `icon` (string glyph/emoji), `iconFont` (font family), `title` (string).
- `false`: draws no header on the bar; the sidebar draws its own toggle at the top of the rail.
- Registered Omarchy bar widget id (e.g. `"omarchy.menu"`): centered over the rail; the sidebar keeps its own toggle below it.
- Custom QML widget (`{ "id": "my-header", "type": "qml" }` or `{ "source": "~/path/to/Header.qml" }`): add `"sidebarHeader": false` if it provides its own sidebar toggle and should hide the sidebar's header, and set `property bool fillHeader: true` in QML if it should span the full sliding recess width (`0 .. spaces.edge`).

### `widgets` entries
Entries can be string ids or objects `{ "id": "...", ... }`:
- `"spaces.tabs"`: active space's tabs with drag-to-reorder, double-click rename, right-click menu, agent status dots, and automatic truncation before the clock. Inline settings:
  - `slotLabels`: 1-indexed array of labels for slots 1–10 (e.g. kanji or Roman numerals).
  - `showLabels`: `false` to show only slot numbers/labels (default `true`).
  - `showGaps`: `"home"` (default), `true`, or `false` for faded empty slot numbers.
- `"spaces.new-tab"`: the `+` button (remove it from `widgets` to hide it). Optional setting: `text` (default `"+"`).
- Simple registered Omarchy bar widgets (e.g. `{ "id": "omarchy.spacer", "size": 12 }`). Note: panel/service widgets like `omarchy.audio` or `omarchy.media` should stay top-level in `bar.layout.*`, not nested inside `com.t4t5.spaces`.
- Custom QML widgets: `{ "id": "my-tabs", "type": "qml" }` loads `~/.config/omarchy/bar/modules/my-tabs.qml`, or pass `"source": "~/.../Widget.qml"`.

### Custom QML widget API (`property var spaces`)

Any custom QML widget loaded by `com.t4t5.spaces` gets `bar`, `settings`, `moduleName`, and `fixedEdge` injected, plus `spaces` if it declares `property var spaces`:

- **State**: `spaces.screenName`, `spaces.model` (`{ name, space, tab, tabs }`), `spaces.space`, `spaces.tabs` (array of `{ id, slot, label, named, status, windows, active }`), `spaces.cells` (tabs interleaved with empty-slot gaps on `home`), `spaces.spaces` (sidebar space rows), `spaces.sidebar`, `spaces.controller`, `spaces.edge`, `spaces.origin`, `spaces.tabsX`.
- **Theme**: `spaces.foreground`, `spaces.accent`, `spaces.hover`, `spaces.fontFamily`, `spaces.fontSize`.
- **Actions**: `spaces.toggleSidebar()`, `spaces.goToTab(idOrSlot)`, `spaces.newTab()`, `spaces.openTabMenu(id, x, y)`, `spaces.run(argvArray)`.

Always set `textFormat: Text.PlainText` on `Text` items displaying dynamic tab or space labels. After creating or editing a custom QML file outside `~/.config/omarchy/plugins/`, restart the shell (`omarchy restart shell`) if it doesn't hot-reload.

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
