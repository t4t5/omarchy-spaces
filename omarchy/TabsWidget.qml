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
// sidebar. Before that line it draws the shaded recess and the `header` widget
// (`BarHeader.qml` by default) when first in the left section. Past it, from a
// rounded corner, it draws the configured `widgets` (`spaces.tabs` and
// `spaces.new-tab` by default) sliding with the windows as the sidebar opens.
//
// Each child entry can be a Spaces widget (`spaces.header`, `spaces.tabs`,
// `spaces.new-tab`), a registered Omarchy bar widget (`omarchy.menu`,
// `omarchy.spacer`, …), or a custom QML file (`{ id, type: "qml" }` or
// `{ id, source: "~/..." }`). Children that declare `property var spaces`
// receive the live Spaces context.
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

  readonly property var headerEntry: Spaces.barHeader(root.settings)
  readonly property var widgetEntries: Spaces.barWidgets(root.settings)

  implicitWidth: root.vertical ? root.barSize : row.x + row.implicitWidth + root.padding
  implicitHeight: root.vertical ? column.implicitHeight : root.barSize

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

  // Whether this widget owns the corner over the sidebar, whether a header
  // widget is drawn there, and whether that header replaces the sidebar's own.
  readonly property bool isFirstLeft: root.parts && !!root.bar
    && Spaces.firstOnBar({ position: root.bar.position, layout: root.bar.layoutConfig }, root.moduleName)
  readonly property bool hasBarHeader: root.isFirstLeft && root.headerEntry !== null
  readonly property bool hasHeader: root.hasBarHeader && Spaces.replacesSidebarHeader(root.headerEntry)
  function tellHeader() {
    if (root.service) root.service.setHeader(root, root.hasHeader)
  }
  onHasHeaderChanged: root.tellHeader()
  onServiceChanged: root.tellHeader()

  // Where the space's widgets start: a little past the sidebar's cut.
  readonly property real tabsX: Math.max(root.padding, root.edge - root.origin.x + Style.spacing.sm)

  // Where the bar's centre starts (its clock), and how much room the space's
  // widgets share before it.
  readonly property real centerStart: {
    var window = root.QsWindow.window
    return window && typeof window.peekEdge === "number" && window.peekEdge > 0 ? window.peekEdge : Infinity
  }
  readonly property real beforeCenter: {
    var layout = root.bar ? root.bar.layoutConfig : null
    var hanging = layout && Array.isArray(layout.centerBegin) && layout.centerBegin.length > 0
    return Style.spacing.lg + (hanging ? Style.bar.iconSlot * 2 : 0)
  }
  readonly property real rowBudget: root.centerStart === Infinity ? Infinity
    : root.centerStart - root.beforeCenter - root.origin.x - root.tabsX

  function availableWidthFor(hostIndex) {
    if (root.rowBudget === Infinity) return Infinity
    var used = 0
    for (var i = 0; i < rowRepeater.count; i++) {
      if (i === hostIndex) continue
      var item = rowRepeater.itemAt(i)
      if (!item || item.width <= 0 || item.flexing) continue
      used += item.width + row.spacing
    }
    return Math.max(0, root.rowBudget - used)
  }

  function toggleSidebar() {
    if (!root.sidebar) return
    var window = root.QsWindow.window
    root.sidebar.toggleSidebar(window ? window.screen : null)
  }

  // Opens a tab's menu at a point on the bar's window, which need not start at
  // its screen's top when the bar is at the bottom.
  function openTabMenu(id, x, y) {
    var window = root.QsWindow.window
    if (!root.controller || !window || !window.screen) return
    var top = root.bar && root.bar.position === "bottom" ? window.screen.height - window.height : 0
    root.controller.openTabMenu(id, x, top + y, window.screen)
  }

  // Live Spaces context injected into any child widget declaring `property var spaces`.
  QtObject {
    id: spacesApi
    readonly property string screenName: root.screenName
    readonly property var model: root.live
    readonly property string space: root.live.space
    readonly property var tabs: root.live.tabs
    readonly property var cells: Spaces.tabCells(root.live.tabs, root.live.space === "home")
    readonly property var spaces: root.service && root.service.watch ? root.service.watch.now.spaces : []
    readonly property var sidebar: root.sidebar
    readonly property var controller: root.controller
    readonly property real edge: root.edge
    readonly property point origin: root.origin
    readonly property real tabsX: root.tabsX
    readonly property color foreground: root.foreground
    readonly property color accent: root.accent
    readonly property color hover: root.hover
    readonly property string fontFamily: root.fontFamily
    readonly property int fontSize: root.fontSize

    function toggleSidebar() { root.toggleSidebar() }
    function goToTab(target) { root.run(["space", "tab", "go", String(target)]) }
    function newTab() { root.run(["space", "tab", "new"]) }
    function openTabMenu(id, x, y) { root.openTabMenu(id, x, y) }
    function run(args) { root.run(args) }
  }

  Component { id: builtinHeader; BarHeader {} }
  Component { id: builtinTabs; TabsStrip {} }
  Component { id: builtinNewTab; NewTabButton {} }

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

  // The configured `header` widget in the recess over the sidebar.
  Item {
    id: headerSlot
    visible: root.hasBarHeader
    x: -root.origin.x
    y: -root.origin.y
    width: root.edge
    height: root.barSize
    clip: true

    HostedWidget {
      id: headerHost
      entry: root.hasBarHeader ? root.headerEntry : null
      readonly property bool fullRecess: (!!entry && entry.id === "spaces.header")
        || (!!item && item.fillHeader === true)
      x: fullRecess ? 0 : Math.round((root.sidebar ? root.sidebar.axis : 0) - width / 2)
      y: fullRecess ? 0 : Math.round((parent.height - height) / 2)
      width: fullRecess ? parent.width : implicitWidth
      height: fullRecess ? parent.height : implicitHeight
    }
  }

  Row {
    id: row
    visible: !root.vertical
    x: root.tabsX
    height: root.barSize
    spacing: Style.space(1)

    Repeater {
      id: rowRepeater
      model: root.vertical ? [] : root.widgetEntries

      HostedWidget {
        required property var modelData
        required property int index
        entry: modelData
        maxWidth: root.availableWidthFor(index)
      }
    }
  }

  Column {
    id: column
    visible: root.vertical

    Repeater {
      model: root.vertical ? root.widgetEntries : []

      HostedWidget {
        required property var modelData
        entry: modelData
      }
    }
  }

  // Loads a single built-in Spaces widget, custom QML module, or registered
  // Omarchy bar widget, injecting the standard bar props plus `spaces`.
  component HostedWidget: Item {
    id: host
    property var entry: null
    property real maxWidth: Infinity
    readonly property bool flexing: !!entry && entry.id === "spaces.tabs"
    readonly property string entryId: entry ? entry.id : ""
    readonly property var entrySettings: entry ? entry.settings : ({})
    readonly property string customUrl: Spaces.customWidgetUrl(entry, root.home)
    readonly property var builtinComponent: {
      if (entryId === "spaces.header") return builtinHeader
      if (entryId === "spaces.tabs") return builtinTabs
      if (entryId === "spaces.new-tab") return builtinNewTab
      return null
    }
    readonly property var registryComponent: {
      if (!entry || builtinComponent || customUrl) return null
      var reg = root.service ? root.service.barWidgetRegistry : null
      if (!reg) return null
      var rev = reg.revision
      var w = reg.widgets
      return w && w[entryId] ? w[entryId].component : null
    }
    readonly property var resolvedComponent: builtinComponent || registryComponent
    readonly property var item: componentLoader.item || sourceLoader.item

    // Do not bind `visible` to `item.visible`: QQuickItem::isVisible() is false
    // whenever any ancestor is invisible, which would deadlock `visible` at false.
    implicitWidth: item && item.visible ? (root.vertical ? root.barSize : item.implicitWidth) : 0
    implicitHeight: item && item.visible ? item.implicitHeight : 0
    width: implicitWidth
    height: implicitHeight

    // Separate loaders: setting `source: ""` on a Loader clears `sourceComponent`.
    Loader {
      id: componentLoader
      active: !!host.entry && !!host.resolvedComponent
      sourceComponent: host.resolvedComponent
      width: host.width
      height: host.height
      onLoaded: {
        host.inject()
        Qt.callLater(host.inject)
      }
    }

    Loader {
      id: sourceLoader
      active: !!host.entry && !host.resolvedComponent && host.customUrl !== ""
      source: active ? host.customUrl : ""
      width: host.width
      height: host.height
      onLoaded: {
        host.inject()
        Qt.callLater(host.inject)
      }
    }

    onEntrySettingsChanged: host.inject()
    onMaxWidthChanged: if (item && "maxWidth" in item) item.maxWidth = host.maxWidth
    onXChanged: if (item && "xInRow" in item) item.xInRow = host.x

    Connections {
      target: root
      function onBarChanged() { host.inject() }
      function onFixedEdgeChanged() { host.inject() }
    }

    function inject() {
      var target = host.item
      if (!target) return
      if ("fixedEdge" in target) target.fixedEdge = root.fixedEdge
      if ("bar" in target) target.bar = root.bar
      if ("moduleName" in target) target.moduleName = host.entryId
      if ("settings" in target) target.settings = host.entrySettings
      if ("spaces" in target) target.spaces = spacesApi
      if ("maxWidth" in target) target.maxWidth = host.maxWidth
      if ("xInRow" in target) target.xInRow = host.x
    }
  }
}
