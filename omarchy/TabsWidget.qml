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
// sidebar. Before that line it draws the shaded recess and, when first in the
// left section, the sidebar's header (BarHeader.qml). Past it, from a rounded
// corner, it draws its `tabs` slot, sliding with the windows as the sidebar
// opens.
//
// The `tabs` slot (Spaces.tabsSlot) shows Spaces' own tabs, set by the `tabs`
// key (`false` hides them), or the bar widget or custom QML module in
// `tabsWidget` (which wins if both are set), getting the bar, its settings,
// `maxWidth`, and `spaces` (below), as one on the bar does.
BarWidget {
  id: root
  moduleName: "com.t4t5.spaces"

  readonly property string home: Quickshell.env("HOME") || ""
  readonly property var watchState: root.service && root.service.watch ? root.service.watch.now : Spaces.EMPTY
  readonly property var live: Spaces.monitorNamed(root.watchState, root.screenName)
  readonly property color foreground: root.bar ? root.bar.barForeground : Commons.Color.bar.text
  readonly property color accent: Commons.Color.accent
  readonly property color hover: Commons.Color.menu.selectedBackground
  readonly property string fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
  readonly property int fontSize: Style.font.body
  readonly property real padding: root.vertical ? 0 : Style.spaceReal(4)
  // The space's corner rounds as Hyprland rounds windows.
  readonly property real cornerRadius: Style.cornerRadius

  readonly property var tabsSlot: Spaces.tabsSlot(root.settings)
  // Spaces' own tabs' settings, from `tabs`.
  readonly property var tabsSettings: root.tabsSlot.settings
  function checkTabsSlot() {
    if (root.tabsSlot.misplaced) console.warn("omarchy-spaces: `tabs` holds Spaces' own tabs' settings; put another widget in `tabsWidget`")
  }
  onTabsSlotChanged: root.checkTabsSlot()

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
    root.checkTabsSlot()
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

  // Whether the sidebar's header sits on this bar, in place of its own.
  readonly property bool hasHeader: root.parts && !!root.bar
    && Spaces.headerOnBar({ position: root.bar.position, layout: root.bar.layoutConfig }, root.moduleName)
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

  // What a widget in the `tabs` slot gets as `spaces`, if it has the property.
  // Fields are added without changing `version`; changing or removing one
  // bumps it, so a widget can tell what it's been given.
  QtObject {
    id: spacesApi
    readonly property int version: 1
    // The space shown on this bar's screen:
    // `{ id, label, named, icon, folder, meta, status, terminals }`
    // (Spaces.publicSpace), all spaces (Spaces.publicSpaceRow), and its tabs:
    // `{ id, slot, label, named, status, active }` (Spaces.publicTab).
    readonly property var space: Spaces.publicSpace(root.watchState, root.live.space)
    readonly property var spaces: root.watchState.spaces.map(Spaces.publicSpaceRow)
    readonly property var tabs: root.live.tabs.map(Spaces.publicTab)
    readonly property bool sidebarOpen: !!root.sidebar && root.sidebar.opened
      && !!root.sidebar.screen && root.sidebar.screen.name === root.screenName
    // The tab being renamed, by its id, the name typed so far, and whether all
    // of it is selected (so the next keypress replaces it).
    readonly property string renaming: {
      var id = root.controller ? root.controller.renaming : ""
      return id && root.live.tabs.some(function(tab) { return tab.id === id }) ? id : ""
    }
    readonly property string renameText: spacesApi.renaming && root.controller ? root.controller.nameText : ""
    readonly property bool renameSelected: spacesApi.renaming !== "" && !!root.controller
      && root.controller.nameSelected && spacesApi.renameText !== ""

    // A tab by its id, or its number.
    function goToTab(target) { root.run(["space", "tab", "go", String(target)]) }
    function newTab() { root.run(["space", "tab", "new"]) }
    // Starts renaming a tab, by its id, or ("") the one you are on. Spaces
    // takes the keys: Enter keeps the name, Escape drops it.
    function renameTab(id) { if (root.controller) root.controller.renameTab(String(id || "")) }
    // Moves a tab to a position, from 1.
    function reorderTab(id, position) { root.run(["space", "tab", "reorder", String(id), String(position)]) }
    // Opens a tab's menu at a point on the bar's window: `mapToItem(null, x, y)`.
    function openTabMenu(id, x, y) { root.openTabMenu(String(id), x, y) }
    function toggleSidebar() { root.toggleSidebar() }
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

  // The sidebar's header on the bar, over the rail, when first in the left
  // section.
  Item {
    visible: root.hasHeader
    x: -root.origin.x
    y: -root.origin.y
    width: root.edge
    height: root.barSize
    clip: true

    Loader {
      active: root.hasHeader
      anchors.fill: parent
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
  }

  // The `tabs` slot: Spaces' own tabs, or another widget.
  Item {
    id: tabsHost
    readonly property bool own: root.tabsSlot.shown && (!root.tabsSlot.widget || !tabsWidget.found)
    x: root.vertical ? 0 : root.tabsX
    implicitWidth: tabsHost.own ? ownTabs.implicitWidth : tabsWidget.implicitWidth
    implicitHeight: tabsHost.own ? ownTabs.implicitHeight : tabsWidget.implicitHeight
    width: implicitWidth
    height: implicitHeight

    Loader {
      id: ownTabs
      active: tabsHost.own
      anchors.fill: parent
      sourceComponent: TabsStrip {
        live: root.live
        controller: root.controller
        screenName: root.screenName
        vertical: root.vertical
        barSize: root.barSize
        origin: root.origin
        tabsX: root.tabsX
        maxWidth: root.maxWidth
        foreground: root.foreground
        accent: root.accent
        hover: root.hover
        fontFamily: root.fontFamily
        fontSize: root.fontSize
        cornerRadius: root.cornerRadius
        numerals: root.tabsSettings.numerals === false ? false
          : Array.isArray(root.tabsSettings.numerals) ? root.tabsSettings.numerals : null
        labels: root.tabsSettings.labels !== false
        newTab: root.tabsSettings.newTab !== false
        onGoToTab: function(target) { root.run(["space", "tab", "go", target]) }
        onNewTabRequested: root.run(["space", "tab", "new"])
        onTabMenuRequested: function(id, x, y) { root.openTabMenu(id, x, y) }
        onReorderRequested: function(id, pos) { root.run(["space", "tab", "reorder", id, String(pos)]) }
      }
    }

    HostedWidget {
      id: tabsWidget
      entry: root.tabsSlot.widget
      maxWidth: root.maxWidth
    }
  }

  // Another bar widget in a slot: a registered one by its id, or a custom QML
  // module, loaded and handed its props as the bar does (and `spaces`). Until
  // it's `found`, the slot shows Spaces' own: a plugin's widget is registered
  // only while the shell sees its id in shell.json, which this entry, inside
  // Spaces', isn't, so it has to be in `plugins` too.
  component HostedWidget: Item {
    id: host
    property var entry: null
    property real maxWidth: Infinity
    readonly property string customUrl: Spaces.customModuleUrl(host.entry, root.home)
    readonly property var registered: {
      var registry = root.service ? root.service.barWidgetRegistry : null
      if (!host.entry || !host.entry.id || host.entry.id === root.moduleName || host.customUrl || !registry) return null
      var revision = registry.revision
      var widgets = registry.widgets
      return widgets && widgets[host.entry.id] ? widgets[host.entry.id].component : null
    }
    readonly property var item: registryLoader.item || moduleLoader.item
    readonly property bool found: !!host.item

    // Collapse the size rather than binding `host.visible` to `item.visible`:
    // `item.visible` goes false while any ancestor is hidden, so binding
    // `host.visible` to it would deadlock when the slot shows again.
    implicitWidth: host.item && host.item.visible ? (root.vertical ? root.barSize : host.item.implicitWidth) : 0
    implicitHeight: host.item && host.item.visible ? (root.vertical ? host.item.implicitHeight : root.barSize) : 0
    width: implicitWidth
    height: implicitHeight

    // Two loaders, since setting `source` clears `sourceComponent`.
    Loader {
      id: registryLoader
      active: !!host.registered
      sourceComponent: host.registered
      width: host.width
      height: host.height
      onLoaded: host.inject()
    }

    Loader {
      id: moduleLoader
      active: !!host.entry && host.customUrl !== ""
      source: active ? host.customUrl : ""
      width: host.width
      height: host.height
      onLoaded: host.inject()
      onStatusChanged: if (status === Loader.Error) console.warn("omarchy-spaces: couldn't load", host.customUrl)
    }

    onEntryChanged: host.inject()
    onMaxWidthChanged: if (host.item && "maxWidth" in host.item) host.item.maxWidth = host.maxWidth
    Connections {
      target: root
      function onBarChanged() { host.inject() }
      function onFixedEdgeChanged() { host.inject() }
    }

    function inject() {
      var target = host.item
      if (!target || !host.entry) return
      if ("fixedEdge" in target) target.fixedEdge = root.fixedEdge
      if ("bar" in target) target.bar = root.bar
      if ("moduleName" in target && host.entry.id) target.moduleName = host.entry.id
      if ("settings" in target) target.settings = host.entry.settings
      if ("spaces" in target) target.spaces = spacesApi
      if ("maxWidth" in target) target.maxWidth = host.maxWidth
    }

    // Say so when a plugin's widget doesn't turn up, rather than only showing
    // Spaces' own. (A custom module that fails says so as it loads.)
    Timer {
      running: !!host.entry && !host.customUrl && !host.found
      interval: 5000
      onTriggered: {
        var source = host.entry.settings && host.entry.settings.source
        if (host.entry.id === root.moduleName) console.warn("omarchy-spaces: the tabs slot can't hold " + root.moduleName)
        else if (source) console.warn("omarchy-spaces: invalid source \"" + source + "\" for the slot; use an absolute or ~/ path")
        else if (host.entry.settings && host.entry.settings.type && !host.entry.id) console.warn("omarchy-spaces: custom slot entry with type \"" + host.entry.settings.type + "\" needs an id or source")
        else console.warn("omarchy-spaces: no bar widget \"" + host.entry.id + "\" for the slot; is it in shell.json's plugins?")
      }
    }
  }
}
