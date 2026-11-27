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

## Customizing the tabs

The tabs sit past the sidebar's edge, and two keys on the `com.t4t5.spaces` entry in `~/.config/omarchy/shell.json` set them: `tabs` for Spaces' own tabs, and `tabsWidget` to show another widget in their place.

### Spaces' own tabs

`tabs` holds their settings. Leave it out for the defaults, or set it to `false` to hide Spaces' own tabs:

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

### Another widget in place of the tabs

`tabsWidget` takes a bar widget entry, written as in the bar's own layout: any bar widget by its id, or one of your own as a custom QML module. `{ "id": "my-tabs", "type": "qml" }` loads `~/.config/omarchy/bar/modules/my-tabs.qml`, and `{ "source": "~/path/to/MyTabs.qml" }` loads that file. The rest of the entry is the widget's settings.

```json
{
  "id": "com.t4t5.spaces",
  "tabsWidget": { "id": "io.github.someone.kanji-tabs", "compact": true }
}
```

- A plugin's widget is only loaded while its id appears in `shell.json`'s layout or `plugins` list, and Spaces' entry doesn't count. Add it to `plugins` as well: `"plugins": [{ "id": "io.github.someone.kanji-tabs" }]`.
- Until the widget loads, or if it never does, the slot shows Spaces' own (unless `tabs` is `false`).
- Widgets get the bar's API that Spaces has. Omarchy's own widgets that rely on the bar's services may not work in the slot.

### Writing a tabs widget

A widget for `tabsWidget` is an ordinary bar widget: Spaces sets `bar`, `moduleName` and `settings` once the widget loads if it has those properties, as the bar does (so bind to them rather than marking them `required` or reading them in `Component.onCompleted`). It also sets:

- `maxWidth`: the room left before the bar's clock (`Infinity` when there's no limit). The widget should fit itself in it.
- `spaces`: the space and its tabs, and what you can do with them.

`spaces` has:

- `version`: the version of this API, now `1`.
- `space`: the space shown on this screen, `{ id, label, named, icon, folder, meta, status, terminals }`. `icon` is a glyph, emoji, image path, or `""`; `folder` is the space's folder path or `""`; `meta` is its metadata object (`space meta`); `status` is `""`, `"working"`, `"done"` or `"waiting"`; `terminals` is its terminal windows, each `{ address, tab, folder, program, label, status }`.
- `spaces`: every space in sidebar order, each `{ id, label, named, icon, folder, meta, status, active, windows, leftAt, terminals }`.
- `tabs`: its tabs, each `{ id, slot, label, named, status, active }`. `slot` is the tab's number; `status` is `""`, `"working"`, `"done"` or `"waiting"`.
- `sidebarOpen`: whether the sidebar is expanded on this screen.
- `renaming`, `renameText` and `renameSelected`: the id of the tab being renamed, the name typed so far, and whether all of it is selected (so the next keypress replaces it). Spaces takes the keys: Enter keeps the name, Escape drops it.
- `goToTab(target)`: by a tab's id or its number.
- `newTab()`.
- `renameTab(id)`: starts renaming a tab, or with `""` the one you're on.
- `reorderTab(id, position)`: moves a tab to a position, counting from 1.
- `openTabMenu(id, x, y)`: opens a tab's menu at a point in the bar's window, from `mapToItem(null, x, y)`.
- `toggleSidebar()`.

Treat the `id`s as opaque: pass them back to `spaces`, but don't read anything into their form. New fields can be added to `spaces` at any time. Spaces is young, so a field may still change or go, but when one does `version` goes up and the change is in the release notes, so a widget can check `spaces.version` before relying on it.

```qml
import QtQuick
import qs.Commons

Item {
  property var bar: null
  property var spaces: null
  property real maxWidth: Infinity

  implicitWidth: row.implicitWidth
  implicitHeight: Style.bar.sizeHorizontal

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

## Customizing the sidebar

Just as `tabs` and `tabsWidget` customize the bar's tabs, `sidebar` and `sidebarWidget` on the `com.t4t5.spaces` entry in `~/.config/omarchy/shell.json` customize the sidebar:

```json
{
  "id": "com.t4t5.spaces",
  "sidebar": { "width": 240 },
  "sidebarWidget": { "id": "my-sidebar", "type": "qml" }
}
```

- `sidebar`: settings for the sidebar frame. Currently `{ "width": 240 }` sets the expanded width (in scaled units, default `200`).
- `sidebarWidget`: a registered bar widget id, `{ "id": "my-sidebar", "type": "qml" }` (loads `~/.config/omarchy/bar/modules/my-sidebar.qml`), or `{ "source": "~/path/MySidebar.qml" }`. Spaces keeps the Wayland exclusive-zone reservation, Hyprland slide animation, glassy/clear background, and top-layer keyboard panel, and mounts your widget inside the sidebar frame in place of its own list. Until your widget loads (or if it fails), Spaces' own sidebar shows.

Spaces sets any of these properties your `sidebarWidget` declares (bind to them rather than marking them `required`): `settings`, `foreground`, `shownWidth` (the width currently uncovered from the left edge as the sidebar slides), `screen`, and `spaces`:

- `version`: `1`.
- `spaces`: all spaces in sidebar order, each `{ id, label, named, icon, folder, meta, status, active, windows, leftAt, terminals }`.
- `filteredSpaces`: `spaces` filtered by `filterText`.
- `space` and `tabs`: the active space and its tabs on this screen.
- `sidebarOpen`, `expansion` (`0` collapsed to `1` expanded), `shownWidth`, `railWidth`, `panelWidth`, `cell`, `inset`, `badgeSize`, `badgeRadius`, `headerOnBar`.
- `searching`, `filterText`, `selectedIndex`, `error`, `renaming`, `renameText`, `renameSelected`, `palette`, `statusColors`.
- `goToSpace(id)`, `newSpace([dir])`, `renameSpace([id])`, `openIconPicker(id, x, y)`, `closeSpace(id)`, `reorderSpace(id, position)`, `openSpaceMenu(id, x, y)`, `toggleSidebar()`, `search()`, `endSearch()`, `select(index)`, `activate(index)`, `goToTab(target)`, `newTab()`, `setMeta(spaceId, key, value)` (`null` removes), `focusWindow(address)`.

See [Extending Spaces](extending.md) for git status, worktree grouping, and agents-list recipes.
