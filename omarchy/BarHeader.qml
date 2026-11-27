pragma ComponentBehavior: Bound

import QtQuick
import qs.Commons

// The sidebar's header on the bar, drawn inside the shaded recess over the
// rail: Omarchy's logo in the rail's icon column, and once the sidebar expands,
// its title in line with the spaces' names and a collapse toggle riding the
// windows' edge. A right-click on the logo opens Omarchy's menu.
Item {
  id: root

  property var sidebar: null
  property int barSize: Style.bar.sizeHorizontal
  property color foreground: Color.bar.text
  property color accent: Color.accent
  property color hover: Color.menu.selectedBackground
  property string screenName: ""
  signal toggleRequested()
  signal menuRequested()

  readonly property int inset: root.sidebar ? root.sidebar.inset : 0
  readonly property int cell: root.sidebar ? root.sidebar.cell : 0
  // How much of the sidebar shows on this screen.
  readonly property real shown: {
    if (!root.sidebar) return 0
    var here = !!root.sidebar.screen && root.sidebar.screen.name === root.screenName
    return here ? root.sidebar.revealed : root.sidebar.railWidth
  }
  readonly property real highlightWidth: Math.max(root.cell, root.shown - root.inset * 2)
  // How much the title and the toggle show, as the list's text
  // (SidebarList.qml): only while the sidebar is expanded on this screen,
  // fading quickly. The logo stays, to expand it again.
  property real textShown: !!root.sidebar && root.sidebar.opened && !!root.sidebar.screen
    && root.sidebar.screen.name === root.screenName ? 1 : 0
  Behavior on textShown { NumberAnimation { duration: Style.duration(120) } }
  // Where the toggle is, and where the title starts, past the logo.
  readonly property real toggleX: root.inset + root.highlightWidth - root.cell
  readonly property real afterLogo: root.inset + root.cell
  readonly property real titleX: root.afterLogo + (root.sidebar ? root.sidebar.textGap : 0)

  height: root.barSize

  // The logo, as the menu draws it, for the sidebar; a right-click opens the
  // menu.
  HeaderButton {
    x: root.inset
    width: root.cell
    text: "\ue900"
    fontFamily: "omarchy"
    fontSize: Style.bar.iconFont
    restingOpacity: 1
    onClicked: function(button) {
      if (button === Qt.RightButton) root.menuRequested()
      else root.toggleRequested()
    }
  }

  Item {
    x: root.titleX
    width: Math.max(0, root.toggleX - root.titleX)
    height: parent.height
    clip: true

    // As wide as its room once expanded, so while the sidebar slides it is
    // uncovered rather than cut short.
    Text {
      width: (root.sidebar ? root.sidebar.panelWidth : 0) - root.inset - root.cell - root.titleX
      height: parent.height
      verticalAlignment: Text.AlignVCenter
      textFormat: Text.PlainText
      text: "Spaces"
      color: root.foreground
      opacity: 0.5 * root.textShown
      font.family: Style.font.family
      font.pixelSize: Style.font.body
      elide: Text.ElideRight
    }
  }

  // Past the logo, so the toggle shows only once it has come out from behind it.
  Item {
    opacity: root.textShown
    x: root.afterLogo
    width: Math.max(0, root.toggleX + root.cell - root.afterLogo)
    height: parent.height
    clip: true

    HeaderButton {
      x: root.toggleX - root.afterLogo
      width: root.cell
      enabled: x >= 0
      // The sidebar, docked on the left (nf-cod-layout_sidebar_left).
      text: "\u{ebf3}"
      onClicked: root.toggleRequested()
    }
  }

  // A button in the sidebar's header on the bar, drawn as the list draws its
  // controls: an icon centred by the ink it draws, lit under the pointer, on a
  // control's box, unless the bar is too thin for one: then as far in from the
  // bar's edges as a tab's.
  component HeaderButton: Item {
    id: button
    property alias text: icon.text
    property string fontFamily: Style.font.family
    property int fontSize: Style.font.icon
    property real restingOpacity: 0.6
    readonly property bool lit: mouse.containsMouse
    signal clicked(int button)

    height: root.barSize

    Rectangle {
      readonly property real size: Math.min(root.sidebar ? root.sidebar.controlSize : 0, root.barSize - Style.spacing.xs * 2)
      anchors.centerIn: parent
      width: size
      height: size
      radius: root.sidebar ? root.sidebar.badgeRadius : 0
      color: button.lit ? Qt.rgba(root.hover.r, root.hover.g, root.hover.b, root.hover.a * 0.6) : "transparent"
    }

    Text {
      id: icon
      x: Math.round((button.width - ink.tightBoundingRect.width) / 2 - ink.tightBoundingRect.x)
      y: Math.round((button.height - ink.tightBoundingRect.height) / 2 - (icon.baselineOffset + ink.tightBoundingRect.y))
      textFormat: Text.PlainText
      color: button.lit ? root.accent : root.foreground
      opacity: button.lit ? 1 : button.restingOpacity
      font.family: button.fontFamily
      font.pixelSize: button.fontSize
    }

    TextMetrics {
      id: ink
      font: icon.font
      text: icon.text
    }

    MouseArea {
      id: mouse
      anchors.fill: parent
      acceptedButtons: Qt.LeftButton | Qt.RightButton
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: function(event) { button.clicked(event.button) }
    }
  }
}
