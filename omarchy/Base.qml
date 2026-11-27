pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons

// The spaces sidebar, drawn on the base the workspaces sit on: down the left
// edge of every screen, below the bar, on the layer under the windows. It
// follows the bar's look (opaque, glassy or clear), shaded as the background,
// so it reads as one piece with the bar above it. The surface covers the
// screen but reserves no room and takes clicks only on the sidebar; the
// sidebar's own panels reserve its room (Sidebar.qml), so the windows slide
// aside to uncover it. Laid out at its whole width, it is cut off where the
// windows begin: the rail on every screen but the one it expands on.
Scope {
  id: root

  property var sidebar: null
  property var controller: null
  // The bar's background look: opaque, glassy or none (Spaces.barLook).
  property string look: "opaque"
  // The shell's bar: its edge and its thickness on each screen.
  property var bar: null
  // The widgets under each space's name (`sidebar.space.widgets`), and the
  // bar's widget registry, where a plugin's widget is found.
  property var spaceWidgets: []
  property var barWidgetRegistry: null

  BarSurface {
    id: surface
    look: root.look
    edge: "left"
    size: root.sidebar ? root.sidebar.panelWidth : 0
  }

  Variants {
    model: Quickshell.screens

    PanelWindow {
      id: base
      required property var modelData
      screen: modelData

      // The bar's thickness on this screen, where it runs along `edge`.
      function barOn(edge) {
        if (!root.bar || root.bar.barHidden || root.bar.position !== edge) return 0
        return root.bar.barSizeFor(base.modelData.name)
      }

      anchors { top: true; bottom: true; left: true; right: true }
      exclusionMode: ExclusionMode.Ignore
      color: "transparent"
      mask: Region { item: sidebarPart.visible ? sidebarPart : null }

      readonly property bool expanding: !!root.sidebar && !!root.sidebar.screen
        && root.sidebar.screen.name === base.modelData.name
      WlrLayershell.namespace: "omarchy-spaces-base"
      WlrLayershell.layer: WlrLayer.Bottom
      WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

      Rectangle {
        id: sidebarPart
        visible: !!root.sidebar
        x: base.barOn("left")
        y: base.barOn("top")
        width: !root.sidebar ? 0 : base.expanding ? root.sidebar.revealed : root.sidebar.railWidth
        height: base.height - y - base.barOn("bottom")
        color: surface.background
        clip: true

        // Set back as the background, beside the space's windows; the bar
        // above it is shaded the same (TabsWidget.qml).
        Rectangle {
          anchors.fill: parent
          color: surface.recess
        }

        SidebarList {
          width: root.sidebar ? root.sidebar.panelWidth : 0
          height: parent.height
          shownWidth: sidebarPart.width
          screen: base.modelData
          sidebar: root.sidebar
          controller: root.controller
          foreground: surface.foreground
          widgets: root.spaceWidgets
          widgetRegistry: root.barWidgetRegistry
        }
      }
    }
  }
}
