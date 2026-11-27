pragma ComponentBehavior: Bound

import QtQuick
import qs.Commons

// The `+` button after a space's tabs (`spaces.new-tab`): opens a new tab in
// the active space. Stays under the bar's drag handler so dragging it moves
// the whole Spaces widget on the bar.
Item {
  id: root

  property var bar: null
  property var spaces: null
  property var settings: ({})
  property string moduleName: "spaces.new-tab"

  readonly property bool vertical: root.bar ? root.bar.vertical : false
  readonly property int barSize: root.bar ? root.bar.barSize : Style.bar.sizeHorizontal
  readonly property color foreground: root.spaces ? root.spaces.foreground : Color.bar.text
  readonly property color hover: root.spaces ? root.spaces.hover : Color.menu.selectedBackground
  readonly property string fontFamily: root.spaces ? root.spaces.fontFamily : Style.font.family
  readonly property int fontSize: root.spaces ? root.spaces.fontSize : Style.font.body
  readonly property real tabPadding: Style.spacing.lg
  readonly property string labelText: root.settings && typeof root.settings.text === "string" && root.settings.text !== ""
    ? root.settings.text : "+"

  visible: !root.vertical && !!root.spaces && root.spaces.tabs.length < 10
  implicitWidth: label.implicitWidth + root.tabPadding * 2
  implicitHeight: root.barSize

  Rectangle {
    anchors.fill: parent
    anchors.margins: Style.spacing.xs
    radius: Style.cornerRadius
    color: mouse.containsMouse ? root.hover : "transparent"
  }

  Text {
    id: label
    anchors.fill: parent
    anchors.leftMargin: root.tabPadding
    anchors.rightMargin: root.tabPadding
    verticalAlignment: Text.AlignVCenter
    horizontalAlignment: Text.AlignHCenter
    textFormat: Text.PlainText
    text: root.labelText
    color: root.foreground
    opacity: 0.6
    font.family: root.fontFamily
    font.pixelSize: root.fontSize
  }

  MouseArea {
    id: mouse
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    onClicked: if (root.spaces) root.spaces.newTab()
  }
}
