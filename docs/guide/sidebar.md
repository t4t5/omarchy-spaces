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

## Customizing the bar widget

The `com.t4t5.spaces` entry in `~/.config/omarchy/shell.json` accepts `header` (in the recess over the rail) and `widgets` (sliding past the sidebar's edge). Without them, it defaults to `spaces.header` and `["spaces.tabs", "spaces.new-tab"]`.

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

- **`header`**: `"spaces.header"` (accepts optional `icon`, `iconFont`, `title`), `false` to keep the header inside the sidebar, a registered Omarchy bar widget like `"omarchy.menu"`, or a custom QML file (`{ "id": "my-header", "type": "qml" }` or `{ "source": "~/.config/..." }`).
- **`widgets`**: any mix of `"spaces.tabs"` (accepts `slotLabels`, `showLabels`, `showGaps`), `"spaces.new-tab"` (accepts `text`), simple registered Omarchy widgets like `"omarchy.spacer"`, or custom QML widgets (`type: "qml"` loads `~/.config/omarchy/bar/modules/<id>.qml`, or pass `source`).

Custom QML widgets that declare `property var spaces` receive the live Spaces context (`model`, `space`, `tabs`, `cells`, `spaces`, `sidebar`, `controller`, `foreground`, `accent`, `hover`, `fontFamily`, `fontSize`, `toggleSidebar()`, `goToTab(idOrSlot)`, `newTab()`, `run(args)`).
