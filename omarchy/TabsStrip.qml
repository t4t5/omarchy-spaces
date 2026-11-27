pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import qs.Commons
import "Spaces.mjs" as Spaces

// The active space's tabs (`spaces.tabs`): clicks switch tabs, double-clicks
// rename inline, right-clicks open the tab menu, and dragging reorders tabs
// along a horizontal bar.
//
// Inline settings (all optional):
//   slotLabels - array of custom labels for slots 1..10 (e.g. kanji numerals)
//   showLabels - false to show only slot numbers/labels (default: true)
//   showGaps   - "home" (default), true, or false for faded empty numbers
Item {
  id: root

  property var bar: null
  property var spaces: null
  property var settings: ({})
  property string moduleName: "spaces.tabs"
  // Room left before the bar's centre, set by TabsWidget.qml.
  property real maxWidth: Infinity
  // Position within the bar frame's widget row, for the drag overlay.
  property real xInRow: 0

  readonly property bool vertical: root.bar ? root.bar.vertical : false
  readonly property int barSize: root.bar ? root.bar.barSize : Style.bar.sizeHorizontal
  readonly property var controller: root.spaces ? root.spaces.controller : null
  readonly property string screenName: root.spaces ? root.spaces.screenName : ""
  readonly property color foreground: root.spaces ? root.spaces.foreground : Color.bar.text
  readonly property color accent: root.spaces ? root.spaces.accent : Color.accent
  readonly property color hover: root.spaces ? root.spaces.hover : Color.menu.selectedBackground
  readonly property string fontFamily: root.spaces ? root.spaces.fontFamily : Style.font.family
  readonly property int fontSize: root.spaces ? root.spaces.fontSize : Style.font.body

  // The tabs shown: while one is dragged, as they were when it began, so the
  // drag isn't cut short when an agent's status changes; and once dropped, as
  // they will be, until `space watch` has them so.
  readonly property var live: root.spaces ? root.spaces.model : Spaces.monitorNamed(Spaces.EMPTY, "")
  readonly property var model: root.heldTabs || root.live
  property var heldTabs: null
  readonly property bool withGaps: {
    var mode = root.settings ? root.settings.showGaps : undefined
    if (mode === true) return true
    if (mode === false) return false
    return root.model.space === "home"
  }
  readonly property var cells: Spaces.tabCells(root.model.tabs, root.withGaps)

  // The tab being dragged, where it would drop, and how far it has moved.
  property int dragFrom: -1
  property int dragTo: -1
  property real dragDx: 0
  property var dragWidths: []
  property var dragGaps: []
  readonly property var dragShifts: Spaces.tabShifts(root.dragWidths, root.dragFrom, root.dragTo, stripRow.spacing, root.dragGaps)
  readonly property real dragThreshold: Math.round(root.barSize * 0.4)
  // Either side of a tab's label. Tighter than a row's, so the tabs stand
  // about as close as Omarchy's workspaces do.
  readonly property real tabPadding: Style.spacing.lg

  implicitWidth: root.vertical ? root.barSize : stripRow.implicitWidth
  implicitHeight: root.vertical ? stripColumn.implicitHeight : root.barSize

  readonly property real tabCap: {
    var count = root.model.tabs.length
    if (count === 0 || root.maxWidth === Infinity) return Infinity
    var room = root.maxWidth - stripRow.spacing * Math.max(0, count - 1)
    // The empty numbers aren't cut short: what they take is the tabs' less.
    var widths = []
    for (var i = 0; i < tabRepeater.count; i++) {
      var cell = tabRepeater.itemAt(i)
      if (!cell) continue
      if (cell.gap) room -= cell.width + stripRow.spacing
      else widths.push(cell.natural)
    }
    return Math.max(root.tabPadding * 2 + root.fontSize * 2, Spaces.fairCap(widths, room))
  }

  function startDrag(index) {
    if (root.controller) {
      root.controller.closeMenu()
      root.controller.endNaming(false)
    }
    settling.stop()
    var all = []
    var widths = []
    for (var i = 0; i < tabRepeater.count; i++) {
      var cell = tabRepeater.itemAt(i)
      all.push(cell.width)
      if (!cell.gap) widths.push(cell.width)
    }
    root.dragWidths = widths
    root.dragGaps = Spaces.tabGaps(root.cells, all, stripRow.spacing)
    root.heldTabs = root.model
    root.dragTo = index
    root.dragFrom = index
  }

  function drag(dx) {
    root.dragDx = Spaces.tabDragOffset(root.dragWidths, root.dragFrom, dx, stripRow.spacing, root.dragGaps)
    root.dragTo = Spaces.tabDropIndex(root.dragWidths, root.dragFrom, root.dragDx, stripRow.spacing, root.dragGaps)
  }

  function endDrag() {
    root.dragFrom = -1
    root.dragTo = -1
    root.dragDx = 0
    root.heldTabs = null
  }

  // Drops the dragged tab. The bar shows the new order at once rather than
  // waiting for `space watch` to have it, so the tab doesn't jump back first.
  function drop() {
    var from = root.dragFrom
    var to = root.dragTo
    var held = root.heldTabs
    if (from < 0 || !held) return
    root.dragFrom = -1
    root.dragTo = -1
    root.dragDx = 0
    if (from === to) {
      root.heldTabs = null
      return
    }
    root.heldTabs = Spaces.reorderTabs(held, from, to)
    settling.restart()
    if (root.spaces) root.spaces.run(["space", "tab", "reorder", held.tabs[from].id, String(to + 1)])
  }

  onLiveChanged: {
    if (!root.heldTabs) return
    if (root.dragFrom < 0) {
      if (root.heldTabs.space !== root.live.space || Spaces.sameTabs(root.heldTabs, root.live)) root.settle()
      return
    }
    var ids = function(model) { return model.space + " " + model.tabs.map(function(tab) { return tab.id }).join(" ") }
    if (ids(root.heldTabs) !== ids(root.live)) Qt.callLater(root.endDrag)
  }

  function settle() {
    settling.stop()
    if (root.dragFrom < 0) root.heldTabs = null
  }

  Timer {
    id: settling
    interval: 1000
    onTriggered: root.settle()
  }

  // Where the tabs take the mouse. The bar lays an area of its own over each
  // widget, to drag it along the bar, which a press on a tab would go to
  // first; this lies over that one, in the bar's window, where the tabs are.
  Item {
    id: tabMouse
    parent: root.QsWindow.contentItem || root
    visible: root.visible && stripRow.visible
    z: 1
    x: parent === root ? 0 : (root.spaces ? root.spaces.origin.x + root.spaces.tabsX : 0) + root.xInRow
    y: parent === root ? 0 : (root.spaces ? root.spaces.origin.y : 0)
  }

  function gapText(cell) {
    if (cell.run) return "…"
    return Spaces.slotLabel(cell.gap, root.settings ? root.settings.slotLabels : null)
  }

  Row {
    id: stripRow
    visible: !root.vertical
    height: root.barSize
    spacing: Style.space(1)

    Repeater {
      id: tabRepeater
      model: root.vertical ? [] : root.cells

      TabButton {
        id: tabButton
        required property var modelData
        readonly property var tab: modelData.tab || null
        readonly property int tabIndex: tabButton.tab ? modelData.index : -1
        gap: !tabButton.tab
        text: tabButton.tab ? Spaces.tabLabel(tabButton.tab, false, root.settings) : root.gapText(modelData)
        number: tabButton.tab ? Spaces.slotLabel(tabButton.tab.slot, root.settings ? root.settings.slotLabels : null) : ""
        mouseLayer: tabMouse
        dragged: tabButton.tab !== null && root.dragFrom === tabButton.tabIndex
        shift: tabButton.tab ? root.dragShifts.tabs[tabButton.tabIndex] || 0
          : root.dragShifts.gaps[modelData.before] || 0
        onDragMoved: function(dx) {
          if (!tabButton.tab) return
          if (root.dragFrom < 0) {
            if (root.model.tabs.length < 2 || Math.abs(dx) < root.dragThreshold) return
            root.startDrag(tabButton.tabIndex)
          }
          if (tabButton.dragged) root.drag(dx)
        }
        // Later: the new order makes new tabs, this one among them.
        onDropped: Qt.callLater(root.drop)
        onDragCanceled: root.endDrag()
        editing: !!tabButton.tab && !!root.controller && root.controller.renaming === tabButton.tab.id
        status: tabButton.tab ? tabButton.tab.status : ""
        current: !!tabButton.tab && tabButton.tab.active
        cap: tabButton.tab ? root.tabCap : Infinity
        onClicked: if (root.spaces) root.spaces.goToTab(tabButton.tab ? tabButton.tab.id : String(modelData.gap))
        hasMenu: !!tabButton.tab
        menuOpen: !!tabButton.tab && !!root.controller && root.controller.menuTabFor === tabButton.tab.id
          && !!root.controller.menuScreen && root.controller.menuScreen.name === root.screenName
        onDoubleClicked: if (tabButton.tab && root.controller) root.controller.renameTab(tabButton.tab.id)
        onMenuRequested: function(x, y) { if (tabButton.tab && root.spaces) root.spaces.openTabMenu(tabButton.tab.id, x, y) }
      }
    }
  }

  Column {
    id: stripColumn
    visible: root.vertical

    Repeater {
      model: root.vertical ? root.cells : []

      TabButton {
        required property var modelData
        gap: !modelData.tab
        text: modelData.tab ? Spaces.tabLabel(modelData.tab, true, root.settings) : root.gapText(modelData)
        status: modelData.tab ? modelData.tab.status : ""
        current: !!modelData.tab && modelData.tab.active
        onClicked: if (root.spaces) root.spaces.goToTab(modelData.tab ? modelData.tab.id : String(modelData.gap))
      }
    }
  }

  component TabButton: Item {
    id: button
    property string text: ""
    property bool current: false
    property string status: ""
    property string number: ""
    property bool editing: false
    property bool gap: false
    signal clicked()
    signal doubleClicked()
    property bool hasMenu: false
    signal menuRequested(real x, real y)
    property bool menuOpen: false
    property Item mouseLayer: null
    property bool dragged: false
    signal dragMoved(real dx)
    signal dropped()
    signal dragCanceled()
    property real shift: 0
    Behavior on shift { NumberAnimation { duration: Style.duration(120); easing.type: Easing.OutCubic } }
    z: button.dragged ? 1 : 0
    transform: Translate { x: button.dragged ? root.dragDx : button.shift }

    readonly property real dotRoom: button.status !== "" && !root.vertical ? dot.width + Style.spacing.sm : 0
    property real cap: Infinity
    readonly property real natural: Math.min((button.editing ? editor.implicitWidth : label.implicitWidth) + button.dotRoom + root.tabPadding * 2, Style.space(220))
    width: root.vertical ? root.barSize : (button.editing ? button.natural : Math.min(button.natural, button.cap))
    height: root.vertical ? Style.space(26) : root.barSize

    Rectangle {
      anchors.fill: parent
      anchors.margins: Style.spacing.xs
      radius: Style.cornerRadius
      color: button.editing ? Qt.rgba(root.accent.r, root.accent.g, root.accent.b, 0.08)
        : button.menuOpen || button.dragged || (mouse.containsMouse && !button.current && root.dragFrom < 0) ? root.hover : "transparent"
      border.width: button.editing ? Math.max(1, Style.space(1)) : 0
      border.color: Qt.rgba(root.accent.r, root.accent.g, root.accent.b, 0.6)
    }

    Text {
      id: label
      visible: !button.editing
      anchors.fill: parent
      anchors.leftMargin: root.vertical ? 0 : root.tabPadding
      anchors.rightMargin: root.vertical ? 0 : root.tabPadding + button.dotRoom
      verticalAlignment: Text.AlignVCenter
      horizontalAlignment: Text.AlignHCenter
      textFormat: Text.PlainText
      text: button.text
      elide: Text.ElideRight
      color: button.current ? root.accent : root.foreground
      opacity: button.current ? 1 : button.gap ? 0.3 : 0.6
      font.family: root.fontFamily
      font.pixelSize: root.fontSize
      font.bold: button.current
    }

    Row {
      id: editor
      visible: button.editing && !root.vertical
      x: root.tabPadding
      anchors.verticalCenter: parent.verticalCenter

      Text {
        id: editNumber
        textFormat: Text.PlainText
        text: button.number + " "
        color: label.color
        font: label.font
      }

      Text {
        id: editName
        readonly property real room: Style.space(220) - root.tabPadding * 2 - editNumber.implicitWidth - caret.width - 1
        width: text === "" ? placeholder.x + placeholder.implicitWidth : Math.min(implicitWidth, room)
        textFormat: Text.PlainText
        text: button.editing && root.controller ? root.controller.nameText : ""
        elide: Text.ElideLeft
        color: label.color
        font: label.font
        readonly property bool selected: button.editing && !!root.controller && root.controller.nameSelected && text !== ""

        Text {
          id: placeholder
          visible: editName.text === ""
          x: caret.x + caret.width
          anchors.verticalCenter: parent.verticalCenter
          textFormat: Text.PlainText
          text: "name"
          color: root.foreground
          opacity: 0.35
          font.family: root.fontFamily
          font.pixelSize: root.fontSize
        }

        Rectangle {
          visible: editName.selected
          z: -1
          width: editName.contentWidth
          height: root.fontSize + Style.spacing.xxs
          anchors.verticalCenter: parent.verticalCenter
          radius: Math.max(1, Style.space(2))
          color: Qt.rgba(root.accent.r, root.accent.g, root.accent.b, 0.3)
        }

        Rectangle {
          id: caret
          visible: button.editing && !editName.selected
          x: editName.contentWidth + 1
          width: Math.max(1, Style.space(2))
          height: root.fontSize + Style.spacing.xxs
          anchors.verticalCenter: parent.verticalCenter
          color: root.accent

          SequentialAnimation on opacity {
            running: button.editing
            loops: Animation.Infinite
            alwaysRunToEnd: false
            NumberAnimation { to: 1; duration: 0 }
            PauseAnimation { duration: 530 }
            NumberAnimation { to: 0; duration: 0 }
            PauseAnimation { duration: 530 }
          }
        }
      }
    }

    StatusDot {
      id: dot
      status: button.status
      colors: root.controller ? root.controller.statusColors : null
      workingColor: root.foreground
      size: Math.max(5, Math.round(root.fontSize * 0.45))
      x: root.vertical ? parent.width * 0.7
        : button.editing ? editor.x + editor.implicitWidth + Style.spacing.sm
        : label.x + Math.min(label.contentWidth, label.width) + Style.spacing.sm
      y: root.vertical ? Style.spacing.xs : (parent.height - height) / 2
    }

    MouseArea {
      id: mouse
      property real pressX: 0
      property bool wasDrag: false
      parent: button.mouseLayer || button
      x: button.mouseLayer ? button.x : 0
      width: button.width
      height: button.height
      hoverEnabled: true
      preventStealing: true
      cursorShape: button.editing ? Qt.IBeamCursor : button.dragged ? Qt.ClosedHandCursor : Qt.PointingHandCursor
      acceptedButtons: button.hasMenu ? Qt.LeftButton | Qt.RightButton : Qt.LeftButton
      onPressed: function(event) {
        mouse.wasDrag = false
        mouse.pressX = mouse.x + event.x
      }
      onPositionChanged: function(event) {
        if (!button.mouseLayer || button.editing || !(mouse.pressedButtons & Qt.LeftButton)) return
        button.dragMoved(mouse.x + event.x - mouse.pressX)
        if (button.dragged) mouse.wasDrag = true
      }
      onReleased: if (button.dragged) button.dropped()
      onCanceled: if (button.dragged) button.dragCanceled()
      onClicked: function(event) {
        if (button.editing || mouse.wasDrag) return
        if (event.button !== Qt.RightButton) {
          button.clicked()
          return
        }
        var at = mouse.mapToItem(null, event.x, event.y)
        button.menuRequested(at.x, at.y)
      }
      onDoubleClicked: function(event) {
        if (!button.editing && event.button === Qt.LeftButton) button.doubleClicked()
      }
    }
  }
}
