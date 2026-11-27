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

## Customizing views

Spaces keeps the behaviour (clicks, drag-to-reorder, inline renaming, menus, width cap, sliding with the sidebar) and lets you swap how its parts draw by pointing at your own QML files in `~/.config/omarchy/shell.json`.

Every node follows one rule: a file path replaces that node with your view, an object keeps the built-in and overrides its children, `false` hides optional nodes (`newTab`, `header`), and leaving it out keeps the default.

```json
{
  "id": "com.t4t5.spaces",
  "tabs": {
    "tab": "~/.config/omarchy/spaces/KanjiTab.qml",
    "newTab": false
  }
}
```

You can also set `"tabs": "~/.config/omarchy/spaces/MyTabs.qml"` to replace the whole tab strip while keeping the recess, sliding edge, and room before the clock.

Each view declares `readonly property int api: 1`, reports its `implicitWidth` and `implicitHeight`, and fills the width Spaces gives it (e.g. when the tab cap narrows tabs before the clock). If a view fails to load or declares a different `api`, Spaces falls back to the built-in view and shows a notification. Saving a custom view file reloads it live.

### Example: Japanese tabs (`KanjiTab.qml`)

```qml
import QtQuick

Item {
  readonly property int api: 1
  property var tab          // null on an empty gap number in home
  property int slot
  property bool gap
  property bool gapRun
  property bool current
  property bool hovered
  property bool dragged
  property bool menuOpen
  property bool editing
  property bool vertical
  property var theme
  property var parts

  readonly property var kanji: ["一", "二", "三", "四", "五", "六", "七", "八", "九", "十"]
  readonly property string mark: gapRun ? "…" : (kanji[slot - 1] || (slot > 0 ? String(slot) : "·"))

  implicitWidth: vertical ? theme.barSize : row.implicitWidth + theme.padding * 2
  implicitHeight: vertical ? 26 : theme.barSize

  Rectangle {
    anchors.fill: parent
    anchors.margins: 4
    radius: theme.cornerRadius
    color: menuOpen || dragged || (hovered && !current) ? theme.hover : "transparent"
  }

  Row {
    id: row
    anchors.centerIn: parent
    spacing: 6

    Text {
      textFormat: Text.PlainText
      text: mark
      color: current ? theme.accent : theme.foreground
      opacity: current ? 1 : gap ? 0.3 : 0.6
      font.family: theme.fontFamily
      font.pixelSize: theme.fontSize
      font.bold: current
    }

    Loader { active: editing && !vertical; sourceComponent: parts ? parts.editor : null }
    Loader { active: !!tab && tab.status !== ""; sourceComponent: parts ? parts.status : null }
  }
}
```

### View contract (`api: 1`)

- **`tabs.tab`**: receives `tab` (`{ id, slot, label, named, status, windows, active }` or `null` on a gap), `slot`, `gap`, `gapRun`, `current`, `hovered`, `dragged`, `menuOpen`, `editing`, `vertical`, `theme`, and pre-bound `parts` (`parts.status`, `parts.editor`).
- **`tabs.newTab`**: receives `hovered`, `vertical`, `count`, and `theme`.
- **`tabs` (full strip)**: receives `space`, `tabs`, `cells`, `vertical`, `maxWidth`, `theme`, and `actions` (`goToTab(target)`, `newTab()`, `openTabMenu(id, x, y)`).
- **`theme`**: `foreground`, `accent`, `hover`, `fontFamily`, `fontSize`, `barSize`, `padding`, `cornerRadius`, `statusColors`, `palette`.
