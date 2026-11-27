pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import qs.Commons
import "Spaces.mjs" as Spaces

// The active space's tabs, Spaces' own in the `tabs` slot: clicks switch
// tabs, double-clicks rename them inline, right-clicks open the tab menu,
// dragging reorders them along a horizontal bar, and the `+` opens a new one.
// Tabs share the room left before the clock fairly.
Item {
  id: root

  property var live: Spaces.monitorNamed(Spaces.EMPTY, "")
  property var controller: null
  property string screenName: ""
  property bool vertical: false
  property int barSize: Style.bar.sizeHorizontal
  property point origin: Qt.point(0, 0)
  property real tabsX: 0
  property real maxWidth: Infinity
  property color foreground: Color.bar.text
  property color accent: Color.accent
  property color hover: Color.menu.selectedBackground
  property string fontFamily: Style.font.family
  property int fontSize: Style.font.body
  property real cornerRadius: Style.cornerRadius
  // From the `tabs` settings.
  property var numerals: null
  property bool labels: true
  property bool newTab: true

  signal goToTab(string target)
  signal newTabRequested()
  signal tabMenuRequested(string id, real x, real y)
  signal reorderRequested(string id, int position)

  // The tabs shown: while one is dragged, as they were when it began, so the
  // drag isn't cut short when an agent's status changes; and once dropped, as
  // they will be, until `space watch` has them so.
  readonly property var model: root.heldTabs || root.live
  property var heldTabs: null
  readonly property var cells: Spaces.tabCells(root.model.tabs, root.model.space === "home")
  readonly property bool showNewTab: !root.vertical && root.newTab && root.model.tabs.length < 10

  // The tab being dragged, where it would drop, and how far it has moved.
  property int dragFrom: -1
  property int dragTo: -1
  property real dragDx: 0
  property var dragWidths: []
  property var dragGaps: []
  readonly property var dragShifts: Spaces.tabShifts(root.dragWidths, root.dragFrom, root.dragTo, stripRow.spacing, root.dragGaps)
  readonly property real dragThreshold: Math.round(root.barSize * 0.4)
  readonly property real tabPadding: Style.spacing.lg

  implicitWidth: root.vertical ? root.barSize : stripRow.implicitWidth
  implicitHeight: root.vertical ? stripColumn.implicitHeight : root.barSize

  readonly property real tabCap: {
    var count = root.model.tabs.length
    if (count === 0 || root.maxWidth === Infinity) return Infinity
    var plusRoom = root.showNewTab ? newTabHost.width + stripRow.spacing : 0
    var room = root.maxWidth - plusRoom - stripRow.spacing * Math.max(0, count - 1)
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
    root.reorderRequested(held.tabs[from].id, to + 1)
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
  // The `+` stays under the bar's, to drag the widget by.
  Item {
    id: tabMouse
    parent: root.QsWindow.contentItem || root
    visible: root.visible && stripRow.visible
    z: 1
    x: parent === root ? 0 : root.origin.x + root.tabsX
    y: parent === root ? 0 : root.origin.y
  }

  Row {
    id: stripRow
    visible: !root.vertical
    height: root.barSize
    spacing: Style.space(1)

    Repeater {
      id: tabRepeater
      model: root.vertical ? [] : root.cells

      TabCell {
        id: cellHost
        required property var modelData
        cell: modelData
        mouseLayer: tabMouse
        cap: cellHost.tab ? root.tabCap : Infinity
        dragged: cellHost.tab !== null && root.dragFrom === cellHost.tabIndex
        shift: cellHost.tab ? root.dragShifts.tabs[cellHost.tabIndex] || 0
          : root.dragShifts.gaps[modelData.before] || 0
        onDragMoved: function(dx) {
          if (!cellHost.tab) return
          if (root.dragFrom < 0) {
            if (root.model.tabs.length < 2 || Math.abs(dx) < root.dragThreshold) return
            root.startDrag(cellHost.tabIndex)
          }
          if (cellHost.dragged) root.drag(dx)
        }
        onDropped: Qt.callLater(root.drop)
        onDragCanceled: root.endDrag()
      }
    }

    Item {
      id: newTabHost
      visible: root.showNewTab
      width: newTabLabel.implicitWidth + root.tabPadding * 2
      height: root.barSize

      Rectangle {
        anchors.fill: parent
        anchors.margins: Style.spacing.xs
        radius: root.cornerRadius
        color: newTabMouse.containsMouse ? root.hover : "transparent"
      }

      Text {
        id: newTabLabel
        anchors.fill: parent
        anchors.leftMargin: root.tabPadding
        anchors.rightMargin: root.tabPadding
        verticalAlignment: Text.AlignVCenter
        horizontalAlignment: Text.AlignHCenter
        textFormat: Text.PlainText
        text: "+"
        color: root.foreground
        opacity: 0.6
        font.family: root.fontFamily
        font.pixelSize: root.fontSize
      }

      MouseArea {
        id: newTabMouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.newTabRequested()
      }
    }
  }

  Column {
    id: stripColumn
    visible: root.vertical

    Repeater {
      model: root.vertical ? root.cells : []

      TabCell {
        required property var modelData
        cell: modelData
      }
    }
  }

  // One tab, or an empty number on `home`: draws its numeral, label, status
  // dot, and inline rename field, and handles clicks, double-clicks, menus,
  // and drag-to-reorder.
  component TabCell: Item {
    id: button

    property var cell: null
    readonly property var tab: cell && cell.tab ? cell.tab : null
    readonly property int tabIndex: button.tab ? cell.index : -1
    readonly property int slot: button.tab ? button.tab.slot : (button.cell ? button.cell.gap : 0)
    readonly property bool gap: !button.tab
    readonly property bool gapRun: button.gap && !!button.cell && !!button.cell.run
    readonly property bool current: !!button.tab && button.tab.active
    readonly property string status: button.tab ? button.tab.status : ""
    readonly property bool editing: !!button.tab && !!root.controller && root.controller.renaming === button.tab.id
    readonly property bool menuOpen: !!button.tab && !!root.controller && root.controller.menuTabFor === button.tab.id
      && !!root.controller.menuScreen && root.controller.menuScreen.name === root.screenName

    readonly property string numberText: Spaces.numeralFor(button.slot, root.numerals)
    readonly property string fallbackSlot: button.slot > 0 ? String(button.slot) : "·"
    readonly property string displayText: {
      if (button.gap) return button.gapRun ? "…" : (button.numberText || button.fallbackSlot)
      if (root.vertical) return button.numberText || button.fallbackSlot
      var text = root.labels && button.tab ? button.tab.label : ""
      if (!button.numberText && !text) return button.fallbackSlot
      return button.numberText + (button.numberText && text ? " " : "") + text
    }

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
      radius: root.cornerRadius
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
      text: button.displayText
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
        visible: button.numberText !== ""
        textFormat: Text.PlainText
        text: button.numberText + " "
        color: label.color
        font: label.font
      }

      Text {
        id: editName
        readonly property real numberRoom: editNumber.visible ? editNumber.implicitWidth : 0
        readonly property real room: Math.max(Style.space(40), Style.space(220) - root.tabPadding * 2 - numberRoom - button.dotRoom - caret.width - 1)
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
      acceptedButtons: !root.vertical && !!button.tab ? Qt.LeftButton | Qt.RightButton : Qt.LeftButton
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
          root.goToTab(button.tab ? button.tab.id : String(button.cell.gap))
          return
        }
        var at = mouse.mapToItem(null, event.x, event.y)
        root.tabMenuRequested(button.tab.id, at.x, at.y)
      }
      onDoubleClicked: function(event) {
        if (!root.vertical && !button.editing && event.button === Qt.LeftButton && button.tab && root.controller) {
          root.controller.renameTab(button.tab.id)
        }
      }
    }
  }
}
