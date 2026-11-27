pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import Quickshell
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Spaces.mjs" as Spaces

// The Spaces bar frame on Omarchy's bar, in place of its menu and workspaces.
//
// The bar parts where the windows begin, by the sidebar's rail or by the whole
// sidebar. Before that line it draws the shaded recess and the sidebar's header
// (when first in the left section and not disabled). Past it, from a rounded
// corner, it draws the `tabs` strip sliding with the windows as the sidebar
// opens.
//
// Custom views are configured explicitly on the `com.t4t5.spaces` entry in
// `~/.config/omarchy/shell.json`:
//   tabs:   path to replace the whole strip, or `{ tab, newTab }` overrides
//   sidebar: `{ header: false }` (more sidebar nodes follow in later steps)
BarWidget {
  id: root
  moduleName: "com.t4t5.spaces"

  readonly property string home: Quickshell.env("HOME") || ""
  readonly property var live: Spaces.monitorNamed(root.service && root.service.watch ? root.service.watch.now : Spaces.EMPTY, root.screenName)
  readonly property color foreground: root.bar ? root.bar.barForeground : Commons.Color.bar.text
  readonly property color accent: Commons.Color.accent
  readonly property color hover: Commons.Color.menu.selectedBackground
  readonly property string fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
  readonly property int fontSize: Style.font.body
  readonly property real padding: root.vertical ? 0 : Style.spaceReal(4)
  // The space's corner rounds as Hyprland rounds windows.
  readonly property real cornerRadius: Style.cornerRadius

  readonly property var tabsCfg: Spaces.tabsConfig(root.settings, root.home)
  readonly property var sidebarCfg: Spaces.sidebarConfig(root.settings, root.home)

  // Stable `api: 1` theme object handed to swappable views.
  readonly property var viewTheme: Spaces.themeViewModel({
    foreground: root.foreground,
    accent: root.accent,
    hover: root.hover,
    fontFamily: root.fontFamily,
    fontSize: root.fontSize,
    barSize: root.barSize,
    padding: Style.spacing.lg,
    cornerRadius: root.cornerRadius,
    statusColors: root.controller ? root.controller.statusColors : null,
    palette: root.controller ? root.controller.palette : Spaces.DEFAULT_PALETTE,
  })

  implicitWidth: root.vertical ? root.barSize : tabsHost.x + tabsHost.implicitWidth + root.padding
  implicitHeight: root.vertical ? tabsHost.implicitHeight : root.barSize

  function run(args) {
    Quickshell.execDetached(args)
  }

  // The service, which runs the one `space watch` and holds the sidebar.
  readonly property var service: {
    var shell = root.bar ? root.bar.shell : null
    if (shell && shell.services !== undefined) shell.services
    return shell && typeof shell.serviceFor === "function" ? shell.serviceFor("com.t4t5.spaces") : null
  }
  readonly property var sidebar: root.service && root.service.sidebar ? root.service.sidebar : null
  readonly property var controller: root.service && root.service.controller ? root.service.controller : null

  // Where this widget sits on its bar, which spans its screen.
  property point origin: Qt.point(0, 0)
  function place() {
    var content = root.QsWindow.contentItem
    if (content) root.origin = content.mapFromItem(root, 0, 0)
  }
  function placeLater() {
    Qt.callLater(root.place)
  }
  property var followed: []
  function unfollow() {
    for (var item of root.followed) {
      if (!item) continue
      item.xChanged.disconnect(root.placeLater)
      item.yChanged.disconnect(root.placeLater)
      item.parentChanged.disconnect(root.follow)
    }
    root.followed = []
  }
  function follow() {
    root.unfollow()
    var items = []
    for (var item = root; item; item = item.parent) {
      item.xChanged.connect(root.placeLater)
      item.yChanged.connect(root.placeLater)
      item.parentChanged.connect(root.follow)
      items.push(item)
    }
    root.followed = items
    root.placeLater()
  }
  Component.onCompleted: {
    root.follow()
    root.tellHeader()
  }
  Component.onDestruction: {
    root.unfollow()
    if (root.service) root.service.setHeader(root, false)
  }

  readonly property string screenName: {
    var window = root.QsWindow.window
    return window && window.screen ? window.screen.name : ""
  }
  // Whether the sidebar parts this bar: the widget can follow its edge.
  readonly property bool parts: !root.vertical && root.fixedEdge === "start" && !!root.sidebar
  // Where the windows begin on this screen, as they slide.
  readonly property real edge: {
    if (!root.parts) return 0
    var here = !!root.sidebar.screen && root.sidebar.screen.name === root.screenName
    return here ? root.sidebar.edge : root.sidebar.railEdge
  }
  onPartsChanged: root.placeLater()

  // Whether the sidebar's header sits on this bar.
  readonly property bool hasHeader: root.parts && !!root.bar
    && root.sidebarCfg.header.mode !== "hidden"
    && Spaces.firstOnBar({ position: root.bar.position, layout: root.bar.layoutConfig }, root.moduleName)
  function tellHeader() {
    if (root.service) root.service.setHeader(root, root.hasHeader)
  }
  onHasHeaderChanged: root.tellHeader()
  onServiceChanged: root.tellHeader()

  // Where the tabs start: a little past the sidebar's cut.
  readonly property real tabsX: Math.max(root.padding, root.edge - root.origin.x + Style.spacing.sm)

  // Room left before the bar's centre (its clock).
  readonly property real centerStart: {
    var window = root.QsWindow.window
    return window && typeof window.peekEdge === "number" && window.peekEdge > 0 ? window.peekEdge : Infinity
  }
  readonly property real beforeCenter: {
    var layout = root.bar ? root.bar.layoutConfig : null
    var hanging = layout && Array.isArray(layout.centerBegin) && layout.centerBegin.length > 0
    return Style.spacing.lg + (hanging ? Style.bar.iconSlot * 2 : 0)
  }
  readonly property real maxWidth: root.centerStart === Infinity ? Infinity
    : Math.max(0, root.centerStart - root.beforeCenter - root.origin.x - root.tabsX)

  function toggleSidebar() {
    if (!root.sidebar) return
    var window = root.QsWindow.window
    root.sidebar.toggleSidebar(window ? window.screen : null)
  }

  function openTabMenu(id, x, y) {
    var window = root.QsWindow.window
    if (!root.controller || !window || !window.screen) return
    var top = root.bar && root.bar.position === "bottom" ? window.screen.height - window.height : 0
    root.controller.openTabMenu(id, x, top + y, window.screen)
  }

  // Public `api: 1` actions handed to a full custom `tabs` strip replacement.
  QtObject {
    id: stripActions
    function goToTab(target) { root.run(["space", "tab", "go", String(target)]) }
    function newTab() { root.run(["space", "tab", "new"]) }
    function openTabMenu(id, x, y) { root.openTabMenu(id, x, y) }
  }

  // The background, from the screen's edge to where the windows begin, over
  // the bar's whole thickness, and the space's part of the bar's rounded
  // corner past it.
  Item {
    id: recess
    visible: !root.vertical && root.edge > 0.5
    x: -root.origin.x
    y: -root.origin.y
    width: root.edge
    height: root.barSize
    readonly property color shade: {
      var bar = Commons.Color.bar.background
      return Qt.rgba(0, 0, 0, Spaces.recessAlpha(bar.r, bar.g, bar.b))
    }

    Rectangle {
      anchors.fill: parent
      color: recess.shade
    }

    Shape {
      visible: root.cornerRadius > 0
      x: recess.width
      width: root.cornerRadius
      height: root.cornerRadius
      preferredRendererType: Shape.CurveRenderer

      ShapePath {
        strokeWidth: -1
        fillColor: recess.shade
        startX: 0; startY: 0
        PathLine { x: root.cornerRadius; y: 0 }
        PathArc {
          x: 0; y: root.cornerRadius
          radiusX: root.cornerRadius; radiusY: root.cornerRadius
          direction: PathArc.Counterclockwise
        }
        PathLine { x: 0; y: 0 }
      }
    }
  }

  Loader {
    active: root.hasHeader
    x: -root.origin.x
    y: -root.origin.y
    width: root.edge
    height: root.barSize
    sourceComponent: BarHeader {
      sidebar: root.sidebar
      barSize: root.barSize
      foreground: root.foreground
      accent: root.accent
      hover: root.hover
      screenName: root.screenName
      onToggleRequested: root.toggleSidebar()
      onMenuRequested: root.run(["omarchy-shell", "shell", "toggle", "omarchy.menu", "{\"menu\":\"root\"}"])
    }
  }

  Component {
    id: builtinStripComponent
    TabsStrip {
      live: root.live
      service: root.service
      controller: root.controller
      screenName: root.screenName
      vertical: root.vertical
      barSize: root.barSize
      origin: root.origin
      tabsX: root.tabsX
      maxWidth: root.maxWidth
      theme: root.viewTheme
      tabViewNode: root.tabsCfg.tab
      newTabViewNode: root.tabsCfg.newTab
      onGoToTab: function(target) { root.run(["space", "tab", "go", target]) }
      onNewTabRequested: root.run(["space", "tab", "new"])
      onTabMenuRequested: function(id, x, y) { root.openTabMenu(id, x, y) }
      onReorderRequested: function(id, pos) { root.run(["space", "tab", "reorder", id, String(pos)]) }
    }
  }

  // The `tabs` strip: built-in TabsStrip (with swappable `tab` and `newTab`
  // views), or a custom full-strip view when `tabs` is set to a path.
  Item {
    id: tabsHost
    visible: root.tabsCfg.strip.mode !== "hidden"
    x: root.vertical ? 0 : root.tabsX
    implicitWidth: visible ? stripView.implicitWidth : 0
    implicitHeight: visible ? stripView.implicitHeight : 0
    width: implicitWidth
    height: implicitHeight

    ViewLoader {
      id: stripView
      anchors.fill: parent
      node: root.tabsCfg.strip
      fallbackComponent: builtinStripComponent
      service: root.service
      props: root.tabsCfg.strip.mode === "custom" ? ({
        space: root.live.space,
        tabs: root.live.tabs.map(Spaces.tabViewModel),
        cells: Spaces.tabCells(root.live.tabs, root.live.space === "home").map(function(c) {
          return c.tab ? { tab: Spaces.tabViewModel(c.tab), index: c.index } : c
        }),
        vertical: root.vertical,
        maxWidth: root.maxWidth,
        theme: root.viewTheme,
        actions: stripActions,
      }) : null
    }
  }
}
