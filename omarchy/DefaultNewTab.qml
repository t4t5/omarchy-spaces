pragma ComponentBehavior: Bound

import QtQuick
import qs.Commons

// The built-in `tabs.newTab` view (`+`), drawn through the `api: 1` contract.
// Click and hover state are owned by the host (`TabsStrip.qml`).
Item {
  id: root

  readonly property int api: 1
  property bool hovered: false
  property bool vertical: false
  property int count: 0
  property var theme: null

  readonly property color foreground: root.theme ? root.theme.foreground : Color.bar.text
  readonly property color hoverColor: root.theme ? root.theme.hover : Color.menu.selectedBackground
  readonly property string fontFamily: root.theme ? root.theme.fontFamily : Style.font.family
  readonly property int fontSize: root.theme && root.theme.fontSize > 0 ? root.theme.fontSize : Style.font.body
  readonly property int barSize: root.theme && root.theme.barSize > 0 ? root.theme.barSize : Style.bar.sizeHorizontal
  readonly property real padding: root.theme ? root.theme.padding : Style.spacing.lg
  readonly property real cornerRadius: root.theme ? root.theme.cornerRadius : Style.cornerRadius

  implicitWidth: label.implicitWidth + root.padding * 2
  implicitHeight: root.barSize

  Rectangle {
    anchors.fill: parent
    anchors.margins: Style.spacing.xs
    radius: root.cornerRadius
    color: root.hovered ? root.hoverColor : "transparent"
  }

  Text {
    id: label
    anchors.fill: parent
    anchors.leftMargin: root.padding
    anchors.rightMargin: root.padding
    verticalAlignment: Text.AlignVCenter
    horizontalAlignment: Text.AlignHCenter
    textFormat: Text.PlainText
    text: "+"
    color: root.foreground
    opacity: 0.6
    font.family: root.fontFamily
    font.pixelSize: root.fontSize
  }
}
