# The sidebar and the bar

The spaces have their own column down the left of the screen. The active space's tabs sit beside it on Omarchy's bar, in place of its workspaces. Both take the bar's look, which double-clicking the bar cycles between opaque, glassy and clear.

## The rail

Collapsed, the column is a rail of badges, one per space: two letters for its name, in a colour from the theme that stays with the name. The space you're in is outlined. A dot on a badge is an [agent's status](agent-status.md).

## The sidebar

Click the logo above the rail, or press a key bound to `spaces.toggle_sidebar()`, to expand it and see each space's name. Your windows and the bar's tabs slide aside to reveal it, and the keyboard stays with your windows. A key bound to `spaces.find_space()` searches it, expanding it first if it's collapsed. See [Keys](keys.md).

## Icons

Right-click a space to give its badge an icon in place of its letters: a Nerd Font glyph, an emoji, or a custom image file (`png`, `svg`, `jpg`, `webp`, `ico`). Tab switches between glyphs and emoji, type to search both at once, or click **Choose image from Files…** (or press `Ctrl+O`) to pick a logo from Files. The badge shows the icon under the cursor until you pick one.

## The logo

With the tabs first in the left section of a bar along the top or bottom of the screen, the sidebar's header is on the bar, over the rail:

- The Omarchy logo expands or collapses the sidebar.
- When the sidebar is expanded, its title shows beside the logo, with a toggle at its right edge.
- A right-click on the logo opens the Omarchy menu.

Anywhere else, the sidebar has its header, with the toggle, at its own top.

## Keeping Omarchy's menu on the bar

Put it before the tabs, and the sidebar draws its own header:

```sh
omarchy plugin enable omarchy.menu --section left --index 0
```

## Customizing the bar

Spaces keeps the behaviour (clicks, drag-to-reorder, inline renaming, menus, width cap, sliding with the sidebar) and lets you customize the bar on its `com.t4t5.spaces` entry in `~/.config/omarchy/shell.json`.

For simple tweaks — such as Japanese numerals, hiding tab labels, or hiding `+` — configure `tabs` directly:

```json
{
  "id": "com.t4t5.spaces",
  "tabs": {
    "numerals": ["一", "二", "三", "四", "五", "六", "七", "八", "九", "十"],
    "labels": false
  }
}
```

- `numerals`: Arabic (`"1"`–`"10"`) by default, an array of custom strings for slots 1–10 (also used for empty gap slots in `home` and inline rename), or `false` to hide numerals.
- `labels`: `true` by default (shows the tab's custom name, foreground process, folder, or app name), or `false` to show only numerals.
- `newTab`: `false` to hide the `+` button, or a `.qml` path to replace it.

### Custom QML views

For full visual control, any node in the tree can point to your own `.qml` file — a file path replaces that node, an object keeps the built-in and overrides its children, `false` hides optional nodes (`newTab`, `header`), and leaving it out keeps the default:

```json
{
  "id": "com.t4t5.spaces",
  "tabs": {
    "tab": "~/.config/omarchy/spaces/MyTab.qml"
  }
}
```

Override `tabs.tab` (the tab cell: background, hover, layout, parts), `tabs.newTab`, or `tabs` (the whole strip inside the sliding frame).

Each custom view declares `readonly property int api: 1`, reports `implicitWidth` and `implicitHeight`, and fills the width Spaces gives it. If a view fails to load or declares a different `api`, Spaces falls back to the built-in view and notifies once. Saving a custom `.qml` file reloads it live.

### View contract (`api: 1`)

- **`tabs.tab`**: receives `tab` (`{ id, slot, label, named, status, windows, active }` or `null` on a gap), `slot`, `gap`, `gapRun`, `current`, `hovered`, `dragged`, `menuOpen`, `editing`, `vertical`, `theme`, and pre-bound `parts` (`parts.status`, `parts.editor`).
- **`tabs.newTab`**: receives `hovered`, `vertical`, `count`, and `theme`.
- **`tabs` (full strip)**: receives `space`, `tabs`, `cells`, `vertical`, `maxWidth`, `theme`, and `actions` (`goToTab(target)`, `newTab()`, `openTabMenu(id, x, y)`).
- **`theme`**: `foreground`, `accent`, `hover`, `fontFamily`, `fontSize`, `barSize`, `padding`, `cornerRadius`, `statusColors`, `palette`.
