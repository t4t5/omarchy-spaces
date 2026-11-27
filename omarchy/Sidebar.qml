pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import "Spaces.mjs" as Spaces

// The spaces sidebar: its rows, search and cursor, geometry, slide animation,
// and exclusive-zone reservation. The base (Base.qml) draws it by the left
// edge, under the windows. Collapsed, it is a rail down every screen, as wide
// as the badges each row starts with, so the spaces always have their column
// and the windows and the bar's tabs sit beside it. Expanding it claims its
// whole width on one screen, so Hyprland slides the windows aside and they
// uncover the rest of each row; collapsing slides them back over it. Where
// nothing covers it, it is cut off where the windows would be. Picking a space
// runs `space go`, which stays the only thing that knows how spaces map to
// workspaces.
//
// Expanding it (with a key, or the toggle at its top) is only visual: the
// keyboard stays with the windows. Searching it takes the keyboard
// through Controller.qml to filter the list; a pick, Escape, or focus moving
// to a window ends the search. A sidebar expanded just for the search
// collapses again with it.
Item {
  id: root

  // Emitted when the search ends by itself (a pick, Escape, focus moving to a
  // window), so the service can tell the shell.
  signal searchEnded()

  // What `space watch` says (Watch.qml) and the shared plugin controller
  // (Controller.qml).
  property var watch: null
  property var controller: null
  readonly property var now: root.watch ? root.watch.now : Spaces.EMPTY

  // Whether the sidebar is expanded, and whether you are searching it, which
  // holds the keyboard. `searchOnly` marks one expanded only for the search.
  property bool opened: false
  property bool searching: false
  property bool searchOnly: false
  // The screen it expands on, or last expanded on.
  property var screen: null
  // Hyprland's window motion, read from `hyprctl animations -j`.
  property var motion: ({ duration: 379, bezier: [0.23, 1, 0.32, 1] })
  property string filterText: ""
  // The order the spaces were dragged into, by their ids, until `space watch`
  // has them so.
  property var pendingOrder: null
  readonly property var rows: Spaces.ordered(root.now.spaces, root.pendingOrder)
  readonly property var shownRows: Spaces.filterRows(root.rows, root.filterText)
  // The space you moved the cursor to, by its id, which a fresh list then
  // keeps it on; "" while it's where it starts.
  property string picked: ""
  readonly property int selectedIndex: Spaces.cursorIndex(root.shownRows, root.filterText, root.picked)
  readonly property string error: root.watch && root.rows.length === 0 ? root.watch.error : ""
  property double searchedAt: 0
  // Enter pressed before there was a list, to act on once it arrives.
  property bool pending: false
  // Whether its header, the toggle and title, is on the bar, the tabs being
  // first there (TabsWidget.qml), so the list starts without one.
  property bool headerOnBar: false

  property int customWidth: 0
  readonly property int panelWidth: root.customWidth > 0 ? Math.max(root.railWidth, Style.space(root.customWidth)) : Style.space(200)

  // The rail's geometry, from which the list's follows (SidebarList.qml).
  // Whatever `inset` and `badgeScale` are set to, these hold:
  //  1. The rail's icons share one centre line, `axis`, with the menu's logo
  //     on a top or bottom bar above or below them. Omarchy's bar sets the
  //     logo in from the screen's edge by its left section's margin (8, in
  //     its Bar.qml) and half the menu's slot, so the rail is twice that
  //     wide and its icons centre in it.
  //  2. Collapsed, every icon's box is a square `cell`: the toggle, each
  //     space's row and its highlight, the `+` and the search.
  //  3. One padding, `inset`, sets the cell in from the rail's edges, the
  //     list in from the sidebar's, and a row's highlight short of the cut.
  //  4. A space's row highlights its whole cell; a control (the toggle, the
  //     `+`, the search) highlights a smaller box, `controlSize`, between a
  //     badge and a cell, centred in its cell. The search's box stretches
  //     from there with the sidebar, as far short of the cut as it starts in.
  //  5. On the rail, the space you're in has its badge bordered in the
  //     theme's foreground.
  readonly property real axis: Style.space(8) + Style.bar.iconSlot / 2
  readonly property int railWidth: Math.round(axis * 2)
  readonly property int inset: Style.spacing.sm
  readonly property int cell: railWidth - inset * 2
  readonly property real badgeScale: 0.7
  readonly property int badgeSize: Math.round(cell * badgeScale)
  readonly property int badgeRadius: Math.round(badgeSize / 4)
  readonly property int controlSize: Math.round((badgeSize + cell) / 2)
  // From the icon column to a row's text: the spaces' names, and the title
  // in the header. None: the badge's own margin in its cell is the gap.
  readonly property int textGap: 0

  // The search's magnifier, drawn larger than the rail's other icons.
  readonly property int searchIconSize: Math.round(badgeSize * 0.72)
  // Hyprland's outer gap on the left, from `hyprctl getoption`.
  property int gapLeft: 0
  // How far from its screen's left edge the windows begin by the rail.
  readonly property int railEdge: railWidth + gapLeft
  // How much of it the windows leave uncovered on its screen, from the rail's
  // width to its whole width, on the curve Hyprland slides them on, starting
  // as they start. It is worked out from the clock each frame, rather than by
  // a NumberAnimation, so it can run `lead` ahead: Hyprland draws the windows
  // where they are in the frame it shows, while what the shell draws reaches
  // the screen a frame after, so the bar is told where they will be by then.
  // Measured on a recording, the bar lagged the windows by one frame of the
  // screen, either way.
  property real uncovered: railWidth
  // How much of it to draw: never less than the windows uncover, so no gap
  // opens between its edge and theirs. Ahead of them as they uncover more,
  // and behind them, under them, as they cover it again.
  property real revealed: railWidth
  readonly property real slideTo: opened ? panelWidth : railWidth
  property real slideFrom: railWidth
  property double slideStart: 0
  // With no windows to move, nothing to keep up with but itself, so the bar
  // and the sidebar keep to each other frame for frame.
  property bool leading: true
  readonly property int lead: {
    if (!root.leading) return 0
    var rate = root.screen ? Spaces.monitorNamed(root.now, root.screen.name).refresh : 0
    return rate > 0 ? Math.round(1000 / rate) : 8
  }
  // Turned around partway, Hyprland slides the windows from where they are,
  // taking the whole time again.
  onSlideToChanged: {
    root.slideFrom = root.uncovered
    root.slideStart = Date.now()
    slide.start()
    root.step()
  }

  // How much is uncovered `elapsed` milliseconds into the slide.
  function uncoveredAt(elapsed) {
    var duration = root.motion.duration
    var progress = duration > 0 ? elapsed / duration : 1
    return root.slideFrom + (root.slideTo - root.slideFrom) * Spaces.ease(root.motion.bezier, progress)
  }

  function step() {
    var elapsed = Date.now() - root.slideStart
    root.uncovered = root.uncoveredAt(elapsed + root.lead)
    root.revealed = root.slideTo > root.slideFrom ? root.uncovered : root.uncoveredAt(elapsed)
    if (elapsed >= root.motion.duration) slide.stop()
  }

  FrameAnimation {
    id: slide
    onTriggered: root.step()
  }
  // Where the windows' left edge is on its screen as they slide: the bar
  // shows the background up to it and keeps the space's tabs past it
  // (TabsWidget.qml), so the bar parts along the same line as the screen.
  // Every other screen parts at `railEdge`.
  readonly property real edge: uncovered + gapLeft

  // Expands it on `screen`, or the focused one.
  function show(screen) {
    if (root.opened) return
    root.screen = screen || (root.controller ? root.controller.focusedScreen() : null)
    root.leading = root.windowsToMove()
    root.opened = true
    root.picked = ""
  }

  function hide() {
    if (!root.opened) return
    var wasSearching = root.searching
    root.stopSearching()
    root.leading = root.windowsToMove()
    root.opened = false
    if (root.controller) {
      root.controller.endNaming(false)
      root.controller.closeMenu()
      root.controller.closeIconPicker()
    }
    if (wasSearching) root.searchEnded()
  }

  function toggleSidebar(screen) {
    if (root.opened) root.hide()
    else root.show(screen)
  }

  // Takes the keyboard to search the list, showing the sidebar for it if it
  // is hidden.
  function search() {
    if (root.searching) return
    if (root.controller) {
      root.controller.endNaming(false)
      root.controller.closeMenu()
      root.controller.closeIconPicker()
    }
    root.searchOnly = !root.opened
    root.show()
    root.filterText = ""
    root.searchedAt = Date.now()
    root.picked = ""
    root.pending = false
    root.searching = true
    if (root.controller) root.controller.focusKeys()
  }

  // Hands the keyboard back to the windows, and hides a sidebar that was
  // shown just for the search.
  function endSearch() {
    if (!root.searching) return
    if (root.searchOnly) {
      root.hide()
      return
    }
    root.stopSearching()
    root.searchEnded()
  }

  // Ends a search while keeping the sidebar open (e.g. when a menu or rename
  // takes over).
  function cancelSearch() {
    if (!root.searching) return
    root.searchOnly = false
    root.stopSearching()
    root.searchEnded()
  }

  function stopSearching() {
    root.searching = false
    root.searchOnly = false
    root.pending = false
    if (root.filterText) root.setFilter("")
  }

  function handleSearchKey(event, ctrl, printable) {
    if (event.key === Qt.Key_Escape) {
      if (root.filterText) root.setFilter("")
      else root.endSearch()
    } else if (Util.editsFilter(event, root.filterText)) {
      root.setFilter(Util.editedFilter(event, root.filterText))
    } else if (event.key === Qt.Key_Up || (ctrl && event.key === Qt.Key_P) || event.key === Qt.Key_Backtab) {
      root.move(-1)
    } else if (event.key === Qt.Key_Down || (ctrl && event.key === Qt.Key_N) || event.key === Qt.Key_Tab) {
      root.move(1)
    } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
      root.activate(root.selectedIndex)
    } else if (printable) {
      root.setFilter(root.filterText + event.text)
    } else {
      return
    }
    event.accepted = true
  }

  onNowChanged: {
    if (root.pendingOrder && Spaces.orderSettled(root.now.spaces, root.pendingOrder)) root.settleOrder()
    // An Enter pressed before there was a list, once all that follows from it
    // is in place.
    if (root.pending) Qt.callLater(function() {
      if (!root.pending || root.shownRows.length === 0) return
      root.pending = false
      root.activate(root.selectedIndex)
    })
  }

  function setFilter(next) {
    root.filterText = next
    root.picked = ""
  }

  function move(delta) {
    var count = root.shownRows.length
    if (count === 0) return
    root.picked = root.shownRows[(root.selectedIndex + delta + count) % count].id
  }

  function select(index) {
    if (index >= 0 && index < root.shownRows.length) root.picked = root.shownRows[index].id
  }

  function activate(index) {
    if (root.searching && root.shownRows.length === 0 && !(root.watch && root.watch.ready)) {
      root.pending = true
      return
    }
    if (index < 0 || index >= root.shownRows.length) return
    var row = root.shownRows[index]
    if (root.controller) root.controller.closeMenu()
    root.endSearch()
    // It says why in a notification if it can't.
    Quickshell.execDetached(["space", "go", row.id])
  }

  // Moves the space at `from` in the list to `to`, where a drag dropped it.
  // The list shows the new order at once rather than waiting for `space
  // watch` to print it, so the row doesn't jump back first. Only the whole
  // list is dragged, never a search's, whose order is the search's own.
  function reorder(from, to) {
    if (from === to || root.filterText !== "") return
    var row = root.shownRows[from]
    if (!row || row.id === "home" || to < 1) return
    root.pendingOrder = Spaces.moveRow(root.rows, from, to).map(function(r) { return r.id })
    root.picked = row.id
    ordering.restart()
    // Positions count from the first space below home.
    Quickshell.execDetached(["space", "reorder", row.id, String(to)])
  }

  function settleOrder() {
    ordering.stop()
    root.pendingOrder = null
  }

  // `space watch` might never have it, if something else changed meanwhile or
  // the space couldn't move.
  Timer {
    id: ordering
    interval: 1000
    onTriggered: root.settleOrder()
  }

  // The `+`: a new space with a terminal in the folder you are in.
  function newSpace() {
    if (root.controller) {
      root.controller.closeMenu()
      root.controller.endNaming(false)
    }
    Quickshell.execDetached(["space", "new"])
  }

  // Whether there are tiled windows on its screen, which Hyprland moves when
  // the sidebar claims or returns its width. Floating ones stay put.
  function windowsToMove() {
    return !!root.screen && Spaces.monitorNamed(root.now, root.screen.name).tiled
  }

  Process {
    id: motionProc
    command: ["hyprctl", "animations", "-j"]
    stdout: StdioCollector {
      onStreamFinished: {
        var motion = Spaces.windowMotion(text)
        if (motion) root.motion = motion
      }
    }
  }

  Process {
    id: gapsProc
    command: ["hyprctl", "getoption", "general:gaps_out", "-j"]
    stdout: StdioCollector {
      onStreamFinished: {
        var gap = Spaces.gapLeft(text)
        if (gap !== null) root.gapLeft = gap
      }
    }
  }

  Component.onCompleted: {
    motionProc.running = true
    gapsProc.running = true
  }

  Connections {
    target: Hyprland
    function onRawEvent(event) {
      if (event.name === "configreloaded") {
        if (!motionProc.running) motionProc.running = true
        if (!gapsProc.running) gapsProc.running = true
      }
    }
  }

  // On each screen, a one-pixel-wide panel on a top layer, drawing nothing
  // and taking no clicks, that reserves the sidebar's width, the rail's or the
  // whole of it, which makes Hyprland move the windows. The shell makes the
  // bar first, so this reserves room below a top bar and the bar never moves.
  Variants {
    model: Quickshell.screens

    PanelWindow {
      required property var modelData
      readonly property bool expanded: root.opened && !!root.screen && root.screen.name === modelData.name
      screen: modelData
      anchors { left: true; top: true; bottom: true }
      implicitWidth: 1
      exclusionMode: ExclusionMode.Normal
      exclusiveZone: expanded ? root.panelWidth : root.railWidth
      mask: Region {}
      color: "transparent"
      WlrLayershell.namespace: "omarchy-spaces-sidebar-room"
      WlrLayershell.layer: WlrLayer.Top
      WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    }
  }
}
