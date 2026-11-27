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

The `com.t4t5.spaces` entry in `~/.config/omarchy/shell.json` has two slots: `header`, in the corner over the sidebar, and `tabs`, past the sidebar's edge. Each holds a bar widget entry, written the same way as an entry in the bar's own layout.

### Spaces' own tabs

Leave `tabs` out and you get Spaces' own tabs. An object without an `id` changes their settings:

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

- `numerals`: a list of strings to number tabs 1–10 with, or `false` for no numbers.
- `labels`: `false` shows only the numbers, without the tabs' names.
- `newTab`: `false` hides the `+`.

### Another widget in a slot

Put any bar widget in a slot by its id, or one of your own as a custom QML module, as the bar takes them: `{ "id": "my-tabs", "type": "qml" }` loads `~/.config/omarchy/bar/modules/my-tabs.qml`, and `{ "source": "~/path/to/MyTabs.qml" }` loads that file. The rest of the entry is the widget's settings.

```json
{
  "id": "com.t4t5.spaces",
  "header": "io.github.someone.logo",
  "tabs": { "id": "io.github.someone.kanji-tabs", "compact": true }
}
```

- A plugin's widget is only loaded while its id appears in `shell.json`'s layout or `plugins` list, and Spaces' entry doesn't count. Add it to `plugins` as well: `"plugins": [{ "id": "io.github.someone.kanji-tabs" }]`.
- Until the widget loads, or if it never does, the slot shows Spaces' own.
- `"header": false` leaves the corner to the sidebar, which then draws its own header. Any widget in `header` replaces the sidebar's header, so it should give a way to open the sidebar (`spaces.toggleSidebar()`).
- Widgets get the bar's API that Spaces has. Omarchy's own widgets that rely on the bar's services may not work in a slot.

### Writing a widget for a slot

A widget for a slot is an ordinary bar widget: Spaces gives it `bar`, `moduleName` and `settings` if it has those properties, as the bar does. It also gets:

- `maxWidth`: in the `tabs` slot, the room left before the bar's clock (`Infinity` when there's no limit). The widget should fit itself in it.
- `spaces`: the space and its tabs, and what you can do with them.

`spaces` has:

- `space`: the id of the space shown on this screen.
- `tabs`: its tabs, each `{ id, slot, label, named, status, active }`. `status` is `""`, `"working"`, `"done"` or `"waiting"`.
- `sidebarOpen`: whether the sidebar is expanded.
- `renaming` and `renameText`: the id of the tab being renamed, and the name typed so far. Spaces takes the keys: Enter keeps the name, Escape drops it.
- `goToTab(target)`: by a tab's id or its number.
- `newTab()`.
- `renameTab(id)`: starts renaming a tab, or with `""` the one you're on.
- `reorderTab(id, position)`: moves a tab to a position, counting from 1.
- `openTabMenu(id, x, y)`: opens a tab's menu at a point in the bar's window, from `mapToItem(null, x, y)`.
- `toggleSidebar()`.

Fields are only ever added to `spaces`, so a widget written for it keeps working.

```qml
import QtQuick
import qs.Commons

Item {
  property var bar: null
  property var spaces: null
  property real maxWidth: Infinity

  implicitWidth: row.implicitWidth
  implicitHeight: bar ? bar.barSize : 0

  Row {
    id: row
    anchors.verticalCenter: parent.verticalCenter
    spacing: Style.spacing.md

    Repeater {
      model: spaces ? spaces.tabs : []

      Text {
        required property var modelData
        textFormat: Text.PlainText
        text: modelData.slot
        color: modelData.active ? Color.accent : Color.bar.text
        font.family: Style.font.family
        font.pixelSize: Style.font.body

        MouseArea {
          anchors.fill: parent
          onClicked: spaces.goToTab(modelData.id)
        }
      }
    }
  }
}
```
