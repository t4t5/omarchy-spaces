pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import "Spaces.mjs" as Spaces

// Shared interaction and theme state for the plugin: context menus
// (SpaceMenu.qml), the tab move picker (MovePicker.qml), the icon picker
// (IconPicker.qml), inline renaming for spaces and tabs, the theme's badge and
// status colours, and the top-layer panel that holds the keyboard while typing
// (renaming or searching the sidebar).
Item {
  id: root

  // What `space watch` says (Watch.qml), and the sidebar (Sidebar.qml).
  property var watch: null
  property var sidebar: null
  readonly property var now: root.watch ? root.watch.now : Spaces.EMPTY
  readonly property var here: Spaces.focusedMonitor(root.now)
  readonly property var rows: root.sidebar ? root.sidebar.rows : root.now.spaces

  // What is being renamed: a space's id, as `space rename` takes it, or a
  // tab's, as `space tab rename` does. The name typed so far, and whether all
  // of it is selected, as it starts, for typing to replace. The name it had,
  // and whether you had given it that name.
  property string renaming: ""
  property string nameText: ""
  property bool nameSelected: false
  property string nameFrom: ""
  property bool nameWasSet: false
  property double renamedAt: 0
  // Whether the sidebar was expanded only for the rename, to collapse once it
  // ends.
  property bool renameOnly: false
  readonly property bool naming: renaming !== ""
  // Whatever holds the keyboard: a name being typed or the sidebar's search.
  readonly property bool typing: root.naming || (!!root.sidebar && root.sidebar.searching)

  // The id of the space whose menu is open, its row, and where on the screen
  // it opens; or, opened on a tab, its id and the tab, which is only there
  // while you are in its space.
  property string menuFor: ""
  readonly property var menuRow: menuFor ? rows.find(function(row) { return row.id === menuFor }) || null : null
  property string menuTabFor: ""
  readonly property var menuTabAt: root.menuTabFor ? Spaces.findTab(root.now, root.menuTabFor) : null
  readonly property var menuTab: root.menuTabAt ? root.menuTabAt.tab : null
  // Gone to another space, or the tab ended: the menu doesn't come back.
  onMenuTabChanged: if (!root.menuTab && root.menuTabFor) root.closeMenu()
  readonly property var menuActions: root.menuTabFor
    ? Spaces.tabActions(root.menuTab, root.menuTabAt ? root.menuTabAt.monitor : root.here, root.moveTargets.length > 0)
    : Spaces.rowActions(root.menuRow)
  // Whether the tab's menu has given way to asking whether to close it, and
  // what it asks.
  property bool confirmingClose: false
  readonly property string closeWarning: Spaces.closeTabWarning(root.menuTab)
  // Whether it has given way to picking a space to move the tab to
  // (MovePicker.qml), or a key opened that by itself, in the middle of the
  // screen; and the spaces to pick from.
  property bool movingTab: false
  property bool moveCentered: false
  readonly property var moveTargets: root.rows.filter(function(row) { return row.id !== root.here.space })
  property real menuX: 0
  property real menuY: 0
  property var menuScreen: null

  // The id of the space whose icon is being picked, where the picker opens
  // (the menu's place), and the icon its badge shows meanwhile: the one under
  // the picker's cursor, or "" for the one it has.
  property string iconFor: ""
  readonly property var iconRow: iconFor ? rows.find(function(row) { return row.id === iconFor }) || null : null
  property string iconPreview: ""
  property string pickingFileFor: ""
  readonly property string pickIconScript: String(Qt.resolvedUrl("pick-icon.py")).replace(/^file:\/\//, "")

  readonly property string home: Quickshell.env("HOME") || ""
  // The colours spaces' badges take, from the theme's colors.toml.
  property var palette: Spaces.DEFAULT_PALETTE
  // The colours of agents' statuses, from it too (StatusDot.qml).
  property var statusColors: null

  // Runs a `space` command that changes the spaces, beside any still running:
  // `space new` takes until its terminal is up. `space watch` sees what it
  // changes, and prints the list again.
  function run(args) {
    Quickshell.execDetached(args)
  }

  function focusedScreen() {
    var monitor = Hyprland.focusedMonitor
    var name = monitor ? String(monitor.name || "") : ""
    for (var i = 0; i < Quickshell.screens.length; i++) {
      if (Quickshell.screens[i].name === name) return Quickshell.screens[i]
    }
    return Quickshell.screens.length > 0 ? Quickshell.screens[0] : null
  }

  function focusKeys() {
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  // Opens a space's menu at a point on `screen`. The menu takes the keyboard,
  // so a search ends, leaving the sidebar open.
  function openMenu(id, x, y, screen) {
    root.placeMenu(x, y, screen)
    root.menuFor = id
  }

  // Opens the menu of a tab of the space you are in, as it does a space's.
  function openTabMenu(id, x, y, screen) {
    root.placeMenu(x, y, screen)
    root.menuTabFor = id
  }

  function placeMenu(x, y, screen) {
    root.closeMenu()
    root.endNaming(false)
    root.closeIconPicker()
    if (root.sidebar) root.sidebar.cancelSearch()
    root.menuX = x
    root.menuY = y
    root.menuScreen = screen || (root.sidebar ? root.sidebar.screen : null) || root.focusedScreen()
  }

  function closeMenu() {
    root.menuFor = ""
    root.menuTabFor = ""
    root.confirmingClose = false
    root.movingTab = false
    root.moveCentered = false
  }

  // Acts on whatever the menu is open on. A tab with windows asks first.
  function pickAction(id) {
    if (id === "close-tab" && root.closeWarning !== "") root.confirmingClose = true
    else if (id === "close-tab") root.closeTab()
    else if (id === "move-tab") root.movingTab = true
    else root.act(id, root.menuTabFor ? root.menuTab : root.menuRow)
  }

  // Picks a space to send the tab you are on to, from the keyboard; again,
  // it stops.
  function sendTab() {
    if (root.movingTab) {
      root.closeMenu()
      return
    }
    var here = root.here
    if (!here.tab) return
    root.placeMenu(0, 0, root.focusedScreen())
    // Before the tab, so its menu doesn't flash up first.
    root.moveCentered = true
    root.movingTab = true
    root.menuTabFor = here.tab
  }

  // Sends the tab to a space, and you with it.
  function sendTabTo(id) {
    var target = root.menuTabFor
    root.closeMenu()
    if (target) root.run(["space", "tab", "send", target, id])
  }

  // Closes the tab whose menu is open, and its windows.
  function closeTab() {
    var target = root.menuTabFor
    root.closeMenu()
    if (target) root.run(["space", "tab", "close", target])
  }

  // An action from a space's menu (Spaces.rowActions), or a tab's
  // (Spaces.tabActions).
  function act(id, row) {
    root.closeMenu()
    if (id === "rename") root.startRename(row)
    else if (id === "rename-tab") root.renameTab(row.id)
    else if (id === "icon") root.openIconPicker(row)
    else if (id === "clear-icon") root.run(["space", "icon", row.id, ""])
    else if (id === "close") root.run(["space", "close", row.id])
  }

  // Opens the icon picker for a space, where its menu was.
  function openIconPicker(row) {
    root.endNaming(false)
    root.iconPreview = ""
    root.iconFor = row.id
  }

  function closeIconPicker() {
    root.iconFor = ""
    root.iconPreview = ""
  }

  // Gives the space the icon picked in the picker.
  function pickIcon(icon) {
    var id = root.iconFor
    root.closeIconPicker()
    if (id && icon) root.run(["space", "icon", id, icon])
  }

  // Opens the native Files dialog (via xdg-desktop-portal) to pick a PNG/SVG
  // logo for the space, closing the overlay first so the dialog takes focus.
  function pickIconFromFile() {
    var id = root.iconFor
    if (!id || filePicker.running) return
    root.closeIconPicker()
    root.pickingFileFor = id
    filePicker.command = ["python3", root.pickIconScript, id]
    filePicker.running = true
  }

  Process {
    id: filePicker
    stdout: StdioCollector {
      onStreamFinished: {
        var id = root.pickingFileFor
        root.pickingFileFor = ""
        var path = String(text || "").trim()
        if (id && path) root.run(["space", "icon", id, path])
      }
    }
    onExited: function(code) {
      if (code !== 0) root.pickingFileFor = ""
    }
  }

  // Takes the keyboard to type a name over the one a space has. A sidebar
  // shown just for a search stays, as you are now using it.
  function startRename(row) {
    root.startNaming(row.id, row.label, row.named)
  }

  function startNaming(target, label, named) {
    root.endNaming(false)
    root.closeIconPicker()
    if (root.sidebar) root.sidebar.cancelSearch()
    root.renamedAt = Date.now()
    root.renameOnly = false
    root.renaming = target
    root.nameText = label
    root.nameFrom = label
    root.nameWasSet = named
    root.nameSelected = true
    root.focusKeys()
  }

  // Names a tab of the space you are in, by its id, or ("") the one you are
  // on: the bar shows the name as you type it. A sidebar shown just for a
  // search goes.
  function renameTab(id) {
    var target = id || root.here.tab
    var found = Spaces.findTab(root.now, target)
    if (!found) return
    var tab = found.tab
    if (root.renaming === target) return
    if (root.sidebar && root.sidebar.searching && root.sidebar.searchOnly) root.sidebar.hide()
    root.closeMenu()
    root.startNaming(target, tab.label, tab.named)
  }

  // Renames the space you are in, from the keyboard. `home` has no name to
  // change. A sidebar collapsed, or expanded just for a search, is expanded
  // just for the rename.
  function renameSpace() {
    var id = root.here.space
    if (!id || id === "home" || root.renaming === id || !root.sidebar) return
    var only = !root.sidebar.opened || (root.sidebar.searching && root.sidebar.searchOnly)
    if (!root.sidebar.opened) root.sidebar.show()
    var row = root.rows.find(function(r) { return r.id === id })
    if (!row) {
      if (only) root.sidebar.hide()
      return
    }
    root.closeMenu()
    root.startRename(row)
    root.renameOnly = only
  }

  // Enter commits the name typed, Escape (`commit` false) drops it. Leaving
  // an unnamed space's folder name as it was changes nothing; an empty name
  // has a named space called after its folder again. A sidebar expanded just
  // for the rename collapses again.
  function endNaming(commit) {
    if (!root.naming) return
    var target = root.renaming
    var to = root.nameText.trim()
    var unchanged = root.nameWasSet ? to === root.nameFrom : (to === root.nameFrom || to === "")
    root.renaming = ""
    root.nameText = ""
    root.nameSelected = false
    if (commit && !unchanged) root.run(Spaces.renameCommand(target, to))
    if (root.renameOnly) {
      root.renameOnly = false
      if (root.sidebar) root.sidebar.hide()
    }
  }

  // A new theme changes current/; the watch on a file inside it fires for the
  // whole directory, as it does for the bar.
  FileView {
    path: (Quickshell.env("XDG_STATE_HOME") || (root.home + "/.local/state")) + "/omarchy/current/theme/colors.toml"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      root.palette = Spaces.themePalette(text())
      root.statusColors = Spaces.statusColors(text())
    }
    onLoadFailed: {
      root.palette = Spaces.DEFAULT_PALETTE
      root.statusColors = null
    }
  }

  // A click on a window or a switch of tab during a search or a rename moves
  // the keyboard there, which ends it.
  Connections {
    target: Hyprland
    function onRawEvent(event) {
      var searching = !!root.sidebar && root.sidebar.searching
      if (!searching && !root.naming) return
      if (event.name !== "activewindowv2" && event.name !== "workspacev2") return
      if (root.naming && Date.now() - root.renamedAt >= 300) root.endNaming(false)
      if (searching && Date.now() - root.sidebar.searchedAt >= 300) root.sidebar.endSearch()
    }
  }

  // On the screen the sidebar expands on, holding the keyboard during a search
  // or a rename, which the bottom layer can't do the way the top layers can.
  // It reserves nothing.
  PanelWindow {
    id: keys
    screen: (root.sidebar && root.sidebar.screen) || root.focusedScreen()
    anchors { left: true; top: true; bottom: true }
    implicitWidth: 1
    exclusionMode: ExclusionMode.Ignore
    mask: Region {}
    color: "transparent"
    WlrLayershell.namespace: "omarchy-spaces-sidebar-keys"
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.keyboardFocus: root.typing ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    Item {
      id: keyCatcher
      anchors.fill: parent
      focus: true

      Keys.priority: Keys.BeforeItem
      Keys.onPressed: function(event) {
        var ctrl = event.modifiers & Qt.ControlModifier
        var printable = event.text && event.text.length === 1 && !ctrl
            && event.text.charCodeAt(0) >= 32 && event.text.charCodeAt(0) !== 127
        if (root.naming) {
          // With the whole name selected, typing replaces it and deleting
          // clears it; Right or End puts the caret after it, to add to it.
          // Ctrl+A selects it again.
          var selected = root.nameSelected
          var deletes = event.key === Qt.Key_Backspace || event.key === Qt.Key_Delete
              || (ctrl && event.key === Qt.Key_U)
          if (event.key === Qt.Key_Escape) root.endNaming(false)
          else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) root.endNaming(true)
          else if (ctrl && event.key === Qt.Key_A) root.nameSelected = root.nameText !== ""
          else if (selected && deletes) root.nameText = ""
          else if (selected && (event.key === Qt.Key_Right || event.key === Qt.Key_End)) {}
          else if (selected && printable) root.nameText = Spaces.nameInput("", event.text)
          else if (Util.editsFilter(event, root.nameText)) root.nameText = Util.editedFilter(event, root.nameText)
          else if (printable) root.nameText = Spaces.nameInput(root.nameText, event.text)
          else return
          if (!(ctrl && event.key === Qt.Key_A)) root.nameSelected = false
          event.accepted = true
          return
        }
        if (root.sidebar && root.sidebar.searching) {
          root.sidebar.handleSearchKey(event, ctrl, printable)
        }
      }
    }
  }
}
