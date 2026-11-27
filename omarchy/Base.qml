pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons
import "Spaces.mjs" as Spaces

// The spaces sidebar, drawn on the base the workspaces sit on: down the left
// edge of every screen, below the bar, on the layer under the windows. It
// follows the bar's look (opaque, glassy or clear), shaded as the background,
// so it reads as one piece with the bar above it. The surface covers the
// screen but reserves no room and takes clicks only on the sidebar; the
// sidebar's own panels reserve its room (Sidebar.qml), so the windows slide
// aside to uncover it. Laid out at its whole width, it is cut off where the
// windows begin: the rail on every screen but the one it expands on.
//
// Shows Spaces' own SidebarList, or the widget in `sidebarWidget` (from
// shell.json), handed `spaces`, `settings`, `foreground`, `shownWidth` and
// `screen`. Until a custom widget loads, Spaces' own shows in its place.
Scope {
  id: root

  property var sidebar: null
  property var controller: null
  // The bar's background look: opaque, glassy or none (Spaces.barLook).
  property string look: "opaque"
  // The shell's bar: its edge and its thickness on each screen.
  property var bar: null
  // Optional `sidebarWidget` entry from `shell.json`, and the bar's widget registry.
  property var sidebarWidget: null
  property var barWidgetRegistry: null
  readonly property string home: Quickshell.env("HOME") || ""

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

      // What a widget in `sidebarWidget` gets as `spaces`, if it declares the
      // property. Fields are added without changing `version`; changing or
      // removing one bumps it.
      QtObject {
        id: spacesApi
        readonly property int version: 1
        readonly property var watchState: root.sidebar ? root.sidebar.now : Spaces.EMPTY
        readonly property var live: Spaces.monitorNamed(spacesApi.watchState, base.modelData.name)

        // Model: all spaces in sidebar order, filtered by `filterText`, the
        // active space on this screen, and its tabs.
        readonly property var spaces: root.sidebar ? root.sidebar.rows.map(Spaces.publicSpaceRow) : []
        readonly property var filteredSpaces: root.sidebar ? root.sidebar.shownRows.map(Spaces.publicSpaceRow) : []
        readonly property var space: Spaces.publicSpace(spacesApi.watchState, spacesApi.live.space)
        readonly property var tabs: spacesApi.live.tabs.map(Spaces.publicTab)

        // Sidebar geometry & live state on this screen.
        readonly property bool sidebarOpen: base.expanding && !!root.sidebar && root.sidebar.opened
        readonly property real shownWidth: sidebarPart.width
        readonly property int railWidth: root.sidebar ? root.sidebar.railWidth : 0
        readonly property int panelWidth: root.sidebar ? root.sidebar.panelWidth : 0
        readonly property real expansion: Math.max(0, Math.min(1,
          (spacesApi.shownWidth - spacesApi.railWidth) / Math.max(1, spacesApi.panelWidth - spacesApi.railWidth)))
        readonly property int cell: root.sidebar ? root.sidebar.cell : 0
        readonly property int inset: root.sidebar ? root.sidebar.inset : 0
        readonly property int badgeSize: root.sidebar ? root.sidebar.badgeSize : 0
        readonly property int badgeRadius: root.sidebar ? root.sidebar.badgeRadius : 0
        readonly property bool headerOnBar: !!root.sidebar && root.sidebar.headerOnBar

        // Search, inline rename, theme palette, and errors.
        readonly property bool searching: !!root.sidebar && root.sidebar.searching
        readonly property string filterText: root.sidebar ? root.sidebar.filterText : ""
        readonly property int selectedIndex: root.sidebar ? root.sidebar.selectedIndex : 0
        readonly property string error: root.sidebar ? root.sidebar.error : ""
        readonly property string renaming: root.controller ? root.controller.renaming : ""
        readonly property string renameText: spacesApi.renaming && root.controller ? root.controller.nameText : ""
        readonly property bool renameSelected: spacesApi.renaming !== "" && !!root.controller
          && root.controller.nameSelected && spacesApi.renameText !== ""
        readonly property var palette: root.controller ? root.controller.palette : Spaces.DEFAULT_PALETTE
        readonly property var statusColors: root.controller ? root.controller.statusColors : null

        function goToSpace(id) {
          if (id) Quickshell.execDetached(["space", "go", String(id)])
        }
        function newSpace(dir) {
          if (dir) Quickshell.execDetached(["space", "new", String(dir)])
          else if (root.sidebar) root.sidebar.newSpace()
        }
        function renameSpace(id) {
          if (root.controller) root.controller.renameSpace(id ? String(id) : "")
        }
        function openIconPicker(id, x, y) {
          if (root.controller && id) root.controller.openIconPickerAt(String(id), x, y, base.modelData)
        }
        function closeSpace(id) {
          if (id) Quickshell.execDetached(["space", "close", String(id)])
        }
        function reorderSpace(id, position) {
          if (id) Quickshell.execDetached(["space", "reorder", String(id), String(position)])
        }
        function openSpaceMenu(id, x, y) {
          if (root.controller && id) root.controller.openMenu(String(id), x, y, base.modelData)
        }
        function toggleSidebar() {
          if (root.sidebar) root.sidebar.toggleSidebar(base.modelData)
        }
        function search() {
          if (root.sidebar) root.sidebar.search()
        }
        function endSearch() {
          if (root.sidebar) root.sidebar.endSearch()
        }
        function select(index) {
          if (root.sidebar) root.sidebar.select(index)
        }
        function activate(index) {
          if (root.sidebar) root.sidebar.activate(index)
        }
        function goToTab(target) {
          Quickshell.execDetached(["space", "tab", "go", String(target)])
        }
        function newTab() {
          Quickshell.execDetached(["space", "tab", "new"])
        }
        function setMeta(spaceId, key, value) {
          if (!spaceId || !key) return
          if (value === null || value === undefined) {
            Quickshell.execDetached(["space", "meta", "rm", "-s", String(spaceId), String(key)])
          } else {
            Quickshell.execDetached(["space", "meta", "set", "-s", String(spaceId), String(key), JSON.stringify(value), "--json"])
          }
        }
        function focusWindow(address) {
          if (address) Quickshell.execDetached(["hyprctl", "dispatch", "focuswindow", "address:" + String(address)])
        }
      }

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

        readonly property bool ownSidebar: !root.sidebarWidget || !hostedSidebar.found

        Loader {
          active: sidebarPart.ownSidebar
          width: root.sidebar ? root.sidebar.panelWidth : 0
          height: parent.height
          sourceComponent: SidebarList {
            shownWidth: sidebarPart.width
            screen: base.modelData
            sidebar: root.sidebar
            controller: root.controller
            foreground: surface.foreground
          }
        }

        // Custom sidebar widget from `sidebarWidget` in shell.json: a
        // registered widget by id, or a custom QML module.
        Item {
          id: hostedSidebar
          readonly property var entry: root.sidebarWidget
          readonly property string customUrl: Spaces.customModuleUrl(hostedSidebar.entry, root.home)
          readonly property var registered: {
            var registry = root.barWidgetRegistry
            if (!hostedSidebar.entry || !hostedSidebar.entry.id || hostedSidebar.entry.id === "com.t4t5.spaces" || hostedSidebar.customUrl || !registry) return null
            var revision = registry.revision
            var widgets = registry.widgets
            return widgets && widgets[hostedSidebar.entry.id] ? widgets[hostedSidebar.entry.id].component : null
          }
          readonly property var item: registryLoader.item || moduleLoader.item
          readonly property bool found: !!hostedSidebar.item

          width: root.sidebar ? root.sidebar.panelWidth : 0
          height: parent.height

          Loader {
            id: registryLoader
            active: !!hostedSidebar.registered
            sourceComponent: hostedSidebar.registered
            anchors.fill: parent
            onLoaded: hostedSidebar.inject()
          }

          Loader {
            id: moduleLoader
            active: !!hostedSidebar.entry && hostedSidebar.customUrl !== ""
            source: active ? hostedSidebar.customUrl : ""
            anchors.fill: parent
            onLoaded: hostedSidebar.inject()
            onStatusChanged: if (status === Loader.Error) console.warn("omarchy-spaces: couldn't load sidebarWidget", hostedSidebar.customUrl)
          }

          function inject() {
            var target = hostedSidebar.item
            if (!target || !hostedSidebar.entry) return
            if ("spaces" in target) target.spaces = spacesApi
            if ("settings" in target) target.settings = hostedSidebar.entry.settings
            if ("foreground" in target) target.foreground = surface.foreground
            if ("shownWidth" in target) target.shownWidth = sidebarPart.width
            if ("screen" in target) target.screen = base.modelData
          }

          onEntryChanged: hostedSidebar.inject()
          Connections {
            target: surface
            function onForegroundChanged() {
              if (hostedSidebar.item && "foreground" in hostedSidebar.item) hostedSidebar.item.foreground = surface.foreground
            }
          }
          Connections {
            target: sidebarPart
            function onWidthChanged() {
              if (hostedSidebar.item && "shownWidth" in hostedSidebar.item) hostedSidebar.item.shownWidth = sidebarPart.width
            }
          }

          Timer {
            running: !!hostedSidebar.entry && !hostedSidebar.customUrl && !hostedSidebar.found
            interval: 5000
            onTriggered: {
              var source = hostedSidebar.entry.settings && hostedSidebar.entry.settings.source
              if (hostedSidebar.entry.id === "com.t4t5.spaces") console.warn("omarchy-spaces: sidebarWidget can't hold com.t4t5.spaces")
              else if (source) console.warn("omarchy-spaces: invalid source \"" + source + "\" for sidebarWidget; use an absolute or ~/ path")
              else if (hostedSidebar.entry.settings && hostedSidebar.entry.settings.type && !hostedSidebar.entry.id) console.warn("omarchy-spaces: custom sidebarWidget with type \"" + hostedSidebar.entry.settings.type + "\" needs an id or source")
              else console.warn("omarchy-spaces: no widget \"" + hostedSidebar.entry.id + "\" for sidebarWidget; is it in shell.json's plugins?")
            }
          }
        }
      }
    }
  }
}
