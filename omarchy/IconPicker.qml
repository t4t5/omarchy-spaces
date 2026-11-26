pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Spaces.mjs" as Spaces

// The icon picker for a space's badge, opened from its menu (SpaceMenu.qml)
// where the menu was. Two tabs: Nerd Font glyphs (glyphs.json, from
// scripts/glyphs.py), tinted in the badge's colour, and emojis, from
// Omarchy's own emoji picker. Tab switches between them. Typing searches
// both at once, in one list, best match first, without the tabs. The arrows
// move the cursor and Enter picks; the space's badge shows the icon under
// the cursor meanwhile. Like
// the menu, it has a surface of its own above the windows, covering the
// screen: a click outside the card closes it, as does Escape.
PanelWindow {
  id: root

  property var controller: null
  property string omarchyPath: ""

  readonly property var row: controller ? controller.iconRow : null
  // Glyphs or emojis, while not searching.
  property string tab: "glyphs"
  property string query: ""
  property int current: -1
  // Read the first time it opens.
  property bool wanted: false
  property var glyphs: []
  property var emojis: []
  readonly property bool searching: query.trim() !== ""
  // Both, for the search: emojis first, so on a tie an emoji comes first.
  readonly property var found: searching ? Spaces.filterIcons(emojis.concat(glyphs), query) : []
  readonly property var shown: searching ? found : tab === "glyphs" ? glyphs : emojis

  readonly property color foreground: Commons.Color.popups.text
  readonly property color accent: Commons.Color.accent
  readonly property color selectedBackground: Commons.Color.menu.selectedBackground
  readonly property var borderSpec: Border.localOrSurfaceSpec("popups", "border", Commons.Color.popups.border, Commons.Color.popups.border, Style.normalBorderWidth)
  // The colour the space's badge takes, which its glyphs are shown in.
  readonly property color hue: row && controller ? (Spaces.badgeColor(row.label, controller.palette) || foreground) : foreground
  readonly property int edge: Style.spacing.md
  readonly property int cellSize: Style.space(40)
  readonly property int fieldHeight: Math.max(Style.spacing.popupRowHeight, Style.font.body + Style.spacing.controlPaddingY * 2)
  readonly property int columns: Math.max(1, Math.floor(grid.width / cellSize))

  visible: !!row && !!controller && !!controller.menuScreen
  screen: controller ? controller.menuScreen : null
  anchors { top: true; bottom: true; left: true; right: true }
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "omarchy-spaces-icons"
  WlrLayershell.layer: WlrLayer.Overlay
  WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive

  // Opens on the tab of the icon the space has, the cursor on it when it's
  // in the list.
  onVisibleChanged: {
    if (!visible) return
    root.wanted = true
    root.query = ""
    var icon = root.row ? root.row.icon : ""
    root.tab = icon && !Spaces.isGlyph(icon) && !Spaces.isImageIcon(icon) ? "emojis" : "glyphs"
    root.current = Math.max(0, root.shown.findIndex(function(entry) { return entry.icon === icon }))
    Qt.callLater(function() {
      keys.forceActiveFocus()
      grid.positionViewAtIndex(root.current, GridView.Center)
    })
  }

  onShownChanged: if (root.current >= root.shown.length) root.current = root.shown.length - 1
  onCurrentChanged: {
    if (root.current >= 0) grid.positionViewAtIndex(root.current, GridView.Contain)
    root.preview()
  }

  // The badge shows the icon under the cursor.
  function preview() {
    if (!root.controller || !root.visible) return
    var entry = root.shown[root.current]
    root.controller.iconPreview = entry ? entry.icon : ""
  }

  function setQuery(text) {
    root.query = text
    root.current = root.shown.length > 0 ? 0 : -1
    grid.positionViewAtBeginning()
    root.preview()
  }

  function switchTab(to) {
    if (root.tab === to) return
    root.tab = to
    root.current = root.shown.length > 0 ? 0 : -1
    grid.positionViewAtBeginning()
    root.preview()
  }

  function pick(index) {
    var entry = root.shown[index]
    if (entry) root.controller.pickIcon(entry.icon)
  }

  FileView {
    path: root.wanted ? String(Qt.resolvedUrl("glyphs.json")).replace(/^file:\/\//, "") : ""
    printErrors: false
    onLoaded: root.glyphs = Spaces.parseGlyphs(text())
  }

  FileView {
    path: root.wanted ? (root.omarchyPath || Quickshell.env("OMARCHY_PATH") || "/usr/share/omarchy") + "/shell/plugins/emojis/emojis.json" : ""
    printErrors: false
    onLoaded: root.emojis = Spaces.parseEmojis(text())
  }

  // Anywhere outside the card closes it.
  MouseArea {
    anchors.fill: parent
    acceptedButtons: Qt.AllButtons
    onPressed: root.controller.closeIconPicker()
  }

  Item {
    id: keys
    anchors.fill: parent
    focus: true
    Keys.priority: Keys.BeforeItem
    Keys.onPressed: function(event) {
      var ctrl = event.modifiers & Qt.ControlModifier
      var printable = event.text && event.text.length === 1 && !ctrl
          && event.text.charCodeAt(0) >= 32 && event.text.charCodeAt(0) !== 127
      if (event.key === Qt.Key_Escape) {
        if (root.query) root.setQuery("")
        else root.controller.closeIconPicker()
      } else if (event.key === Qt.Key_Tab || event.key === Qt.Key_Backtab) {
        if (!root.searching) root.switchTab(root.tab === "glyphs" ? "emojis" : "glyphs")
      } else if (event.key === Qt.Key_Left) {
        root.current = Spaces.gridMove(root.current, root.shown.length, root.columns, -1, 0)
      } else if (event.key === Qt.Key_Right) {
        root.current = Spaces.gridMove(root.current, root.shown.length, root.columns, 1, 0)
      } else if (event.key === Qt.Key_Up || (ctrl && event.key === Qt.Key_P)) {
        root.current = Spaces.gridMove(root.current, root.shown.length, root.columns, 0, -1)
      } else if (event.key === Qt.Key_Down || (ctrl && event.key === Qt.Key_N)) {
        root.current = Spaces.gridMove(root.current, root.shown.length, root.columns, 0, 1)
      } else if (event.key === Qt.Key_PageUp || event.key === Qt.Key_PageDown) {
        var rows = Math.max(1, Math.floor(grid.height / root.cellSize))
        root.current = Spaces.gridMove(root.current, root.shown.length, root.columns, 0, event.key === Qt.Key_PageUp ? -rows : rows)
      } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
        root.pick(root.current)
      } else if (ctrl && event.key === Qt.Key_O) {
        root.controller.pickIconFromFile()
      } else if (Util.editsFilter(event, root.query)) {
        root.setQuery(Util.editedFilter(event, root.query))
      } else if (printable) {
        root.setQuery(root.query + event.text)
      } else {
        return
      }
      event.accepted = true
    }
  }

  BorderSurface {
    id: card
    // Where the menu was, kept on the screen: above the pointer when there
    // isn't room below it.
    x: Math.max(root.edge, Math.min(root.controller ? root.controller.menuX : 0, root.width - width - root.edge))
    y: {
      var at = root.controller ? root.controller.menuY : 0
      return at + height + root.edge <= root.height ? at : Math.max(root.edge, at - height)
    }
    width: Math.min(Style.space(372), root.width - root.edge * 2)
    height: Math.min(Style.space(420), root.height - root.edge * 2)
    color: Commons.Color.popups.background
    borderSpec: root.borderSpec
    radius: Style.cornerRadius
    padding: Style.spacing.sm

    // Clicks inside stay in the card.
    MouseArea {
      anchors.fill: parent
      acceptedButtons: Qt.AllButtons
    }

    Column {
      id: content
      x: card.contentLeftInset
      y: card.contentTopInset
      width: card.width - card.contentLeftInset - card.contentRightInset
      height: card.height - card.contentTopInset - card.contentBottomInset
      spacing: Style.spacing.sm

      // The search: a magnifier, what you've typed, and a caret.
      Rectangle {
        width: parent.width
        height: root.fieldHeight
        radius: Style.cornerRadius
        color: root.selectedBackground

        Text {
          id: magnifier
          x: Style.spacing.controlPaddingX
          anchors.verticalCenter: parent.verticalCenter
          textFormat: Text.PlainText
          // A magnifying glass (nf-md-magnify).
          text: "\u{f0349}"
          color: root.foreground
          opacity: 0.6
          font.family: Style.font.family
          font.pixelSize: Style.font.icon
        }

        Row {
          anchors.left: magnifier.right
          anchors.leftMargin: Style.spacing.controlGap
          anchors.right: parent.right
          anchors.rightMargin: Style.spacing.controlPaddingX
          anchors.verticalCenter: parent.verticalCenter
          clip: true

          Text {
            visible: root.query !== ""
            width: Math.min(implicitWidth, parent.width - caret.width)
            textFormat: Text.PlainText
            text: root.query
            color: root.foreground
            font.family: Style.font.family
            font.pixelSize: Style.font.body
            elide: Text.ElideLeft
          }

          Rectangle {
            id: caret
            width: Math.max(1, Style.space(2))
            height: Style.font.body + Style.spacing.xxs
            anchors.verticalCenter: parent.verticalCenter
            color: root.accent

            SequentialAnimation on opacity {
              running: root.visible
              loops: Animation.Infinite
              alwaysRunToEnd: false
              NumberAnimation { to: 1; duration: 0 }
              PauseAnimation { duration: 530 }
              NumberAnimation { to: 0; duration: 0 }
              PauseAnimation { duration: 530 }
            }
          }

          Text {
            visible: root.query === ""
            leftPadding: Style.spacing.xxs
            textFormat: Text.PlainText
            text: "Search icons"
            color: root.foreground
            opacity: 0.4
            font.family: Style.font.family
            font.pixelSize: Style.font.body
          }
        }
      }

      // The tabs, hidden while searching, which searches both.
      Row {
        id: tabs
        visible: !root.searching
        width: parent.width
        spacing: Style.spacing.xxs

        Repeater {
          model: [
            { id: "glyphs", label: "Glyphs" },
            { id: "emojis", label: "Emoji" },
          ]

          delegate: Rectangle {
            id: tabItem
            required property var modelData
            readonly property bool chosen: root.tab === modelData.id

            width: (tabs.width - tabs.spacing) / 2
            height: root.fieldHeight
            radius: Style.cornerRadius
            color: chosen ? root.selectedBackground
              : tabMouse.containsMouse ? Qt.rgba(root.selectedBackground.r, root.selectedBackground.g, root.selectedBackground.b, root.selectedBackground.a * 0.5)
              : "transparent"

            Text {
              anchors.centerIn: parent
              textFormat: Text.PlainText
              text: tabItem.modelData.label
              color: tabItem.chosen ? root.accent : root.foreground
              opacity: tabItem.chosen ? 1 : 0.7
              font.family: Style.font.family
              font.pixelSize: Style.font.body
              font.bold: tabItem.chosen
            }

            MouseArea {
              id: tabMouse
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: root.switchTab(tabItem.modelData.id)
            }
          }
        }
      }

      Item {
        width: parent.width
        height: parent.height - (root.fieldHeight + parent.spacing) * (tabs.visible ? 3 : 2)

        GridView {
          id: grid
          // Centred, a whole number of cells wide.
          width: Math.floor(parent.width / root.cellSize) * root.cellSize
          height: parent.height
          anchors.horizontalCenter: parent.horizontalCenter
          model: root.shown
          clip: true
          cellWidth: root.cellSize
          cellHeight: root.cellSize
          boundsBehavior: Flickable.StopAtBounds
          // The search changes the model; the cursor places the view.
          currentIndex: -1

          delegate: Rectangle {
            id: cell
            required property var modelData
            required property int index
            readonly property bool hot: index === root.current
            readonly property bool glyph: modelData.glyph === true
            readonly property bool chosen: !!root.row && root.row.icon === modelData.icon

            width: root.cellSize
            height: root.cellSize
            radius: Style.cornerRadius
            color: hot ? root.selectedBackground : "transparent"
            border.width: chosen ? 1 : 0
            border.color: root.accent

            Text {
              anchors.centerIn: parent
              textFormat: Text.PlainText
              text: cell.modelData.icon
              color: cell.hot ? root.hue : root.foreground
              font.family: Style.font.family
              font.pixelSize: cell.glyph ? Math.round(root.cellSize * 0.5) : Math.round(root.cellSize * 0.55)
            }

            MouseArea {
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onPositionChanged: root.current = cell.index
              onClicked: root.pick(cell.index)
            }
          }
        }

        Text {
          anchors.centerIn: parent
          width: parent.width
          visible: root.shown.length === 0
          horizontalAlignment: Text.AlignHCenter
          wrapMode: Text.Wrap
          textFormat: Text.PlainText
          text: root.searching ? "No icon matches “" + root.query.trim() + "”" : "Loading…"
          color: root.foreground
          opacity: 0.7
          font.family: Style.font.family
          font.pixelSize: Style.font.body
        }
      }

      // Pick a custom PNG/SVG logo from the native Files dialog.
      Rectangle {
        width: parent.width
        height: root.fieldHeight
        radius: Style.cornerRadius
        color: fileMouse.containsMouse ? root.selectedBackground
          : Qt.rgba(root.selectedBackground.r, root.selectedBackground.g, root.selectedBackground.b, root.selectedBackground.a * 0.45)

        Row {
          anchors.centerIn: parent
          spacing: Style.spacing.controlGap

          Text {
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            // An image outline (nf-md-image_outline).
            text: "\u{f0976}"
            color: fileMouse.containsMouse ? root.accent : root.foreground
            opacity: fileMouse.containsMouse ? 1 : 0.75
            font.family: Style.font.family
            font.pixelSize: Style.font.icon
          }

          Text {
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            text: "Choose image from Files…"
            color: fileMouse.containsMouse ? root.accent : root.foreground
            opacity: fileMouse.containsMouse ? 1 : 0.85
            font.family: Style.font.family
            font.pixelSize: Style.font.body
          }
        }

        MouseArea {
          id: fileMouse
          anchors.fill: parent
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: root.controller.pickIconFromFile()
        }
      }
    }
  }
}
