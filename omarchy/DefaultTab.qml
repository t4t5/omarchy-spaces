pragma ComponentBehavior: Bound

import QtQuick
import qs.Commons

// The built-in `tabs.tab` view, drawn through the same `api: 1` contract as a
// user's custom tab view. Draws a tab (number + label + status dot), an empty
// gap number on `home`, an inline rename state (`parts.editor`), and compact
// vertical mode.
Item {
  id: root

  readonly property int api: 1
  property var tab: null
  property int slot: 0
  property bool gap: false
  property bool gapRun: false
  property bool current: false
  property bool hovered: false
  property bool dragged: false
  property bool menuOpen: false
  property bool editing: false
  property bool vertical: false
  property var theme: null
  property var parts: null

  readonly property color foreground: root.theme ? root.theme.foreground : Color.bar.text
  readonly property color accent: root.theme ? root.theme.accent : Color.accent
  readonly property color hoverColor: root.theme ? root.theme.hover : Color.menu.selectedBackground
  readonly property string fontFamily: root.theme ? root.theme.fontFamily : Style.font.family
  readonly property int fontSize: root.theme && root.theme.fontSize > 0 ? root.theme.fontSize : Style.font.body
  readonly property int barSize: root.theme && root.theme.barSize > 0 ? root.theme.barSize : Style.bar.sizeHorizontal
  readonly property real padding: root.theme ? root.theme.padding : Style.spacing.lg
  readonly property real cornerRadius: root.theme ? root.theme.cornerRadius : Style.cornerRadius

  readonly property string numberText: root.slot > 0 ? String(root.slot) : ""
  readonly property string displayText: {
    if (root.gap) return root.gapRun ? "…" : String(root.slot)
    if (root.vertical) return root.numberText || "·"
    var labelText = root.tab ? root.tab.label : ""
    return root.numberText + (root.numberText && labelText ? " " : "") + (labelText || (root.numberText ? "" : "·"))
  }

  readonly property bool hasStatus: !!root.tab && root.tab.status !== ""
  readonly property real dotRoom: root.hasStatus && !root.vertical ? dotLoader.implicitWidth + Style.spacing.sm : 0

  implicitWidth: root.vertical ? root.barSize
    : (root.editing ? editorRow.implicitWidth : label.implicitWidth) + root.dotRoom + root.padding * 2
  implicitHeight: root.vertical ? Style.space(26) : root.barSize

  Rectangle {
    anchors.fill: parent
    anchors.margins: Style.spacing.xs
    radius: root.cornerRadius
    color: root.editing ? Qt.rgba(root.accent.r, root.accent.g, root.accent.b, 0.08)
      : root.menuOpen || root.dragged || (root.hovered && !root.current) ? root.hoverColor : "transparent"
    border.width: root.editing ? Math.max(1, Style.space(1)) : 0
    border.color: Qt.rgba(root.accent.r, root.accent.g, root.accent.b, 0.6)
  }

  Text {
    id: label
    visible: !root.editing
    anchors.fill: parent
    anchors.leftMargin: root.vertical ? 0 : root.padding
    anchors.rightMargin: root.vertical ? 0 : root.padding + root.dotRoom
    verticalAlignment: Text.AlignVCenter
    horizontalAlignment: Text.AlignHCenter
    textFormat: Text.PlainText
    text: root.displayText
    elide: Text.ElideRight
    color: root.current ? root.accent : root.foreground
    opacity: root.current ? 1 : root.gap ? 0.3 : 0.6
    font.family: root.fontFamily
    font.pixelSize: root.fontSize
    font.bold: root.current
  }

  Row {
    id: editorRow
    visible: root.editing && !root.vertical
    x: root.padding
    anchors.verticalCenter: parent.verticalCenter

    Text {
      visible: root.numberText !== ""
      textFormat: Text.PlainText
      text: root.numberText + " "
      color: label.color
      font: label.font
    }

    Loader {
      active: root.editing && !root.vertical && !!root.parts
      sourceComponent: root.parts ? root.parts.editor : null
      anchors.verticalCenter: parent.verticalCenter
    }
  }

  Loader {
    id: dotLoader
    active: root.hasStatus && !!root.parts
    sourceComponent: root.parts ? root.parts.status : null
    x: root.vertical ? parent.width * 0.7
      : root.editing ? editorRow.x + editorRow.implicitWidth + Style.spacing.sm
      : label.x + Math.min(label.contentWidth, label.width) + Style.spacing.sm
    y: root.vertical ? Style.spacing.xs : (parent.height - height) / 2
  }
}
