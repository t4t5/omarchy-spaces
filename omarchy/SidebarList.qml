pragma ComponentBehavior: Bound

import QtQuick
import qs.Commons
import qs.Commons as Commons
import "Spaces.mjs" as Spaces

// The spaces list, drawn on the base by the left edge, with its search field
// at the bottom. The sidebar (Sidebar.qml) holds the rows, the search, the
// cursor and the rename; the keyboard reaches it through the sidebar's panel,
// so the field and a name being renamed draw their own caret. A right-click
// opens a space's menu (SpaceMenu.qml) where you clicked.
//
// It is laid out at the sidebar's whole width, and the base cuts it off where
// the windows begin (`shownWidth`). Each row starts with its icon in the
// rail's column, the part that always shows: the toggle, a badge for each
// space, the `+` and the search, each centred on the rail by what it draws,
// in line with the menu on the bar above. What highlights a row ends just
// short of the cut, so collapsed it is a square around the icon, and it
// grows as the windows slide aside. On the rail, the space you're in has its
// badge bordered in the theme's foreground, and the others' badges are
// dimmed; as the sidebar expands, the border fades, they come up, and its
// row's highlight fades in instead.
//
// Drag a space up or down to move it. A press turns into a drag only once it
// has moved a good part of a row, so a click stays a click; the rows between
// slide aside to show where it will drop. `home` stays first, and a search's
// list, in the search's order, can't be dragged.
Item {
  id: root

  property var sidebar: null
  property var controller: null
  property color foreground: Commons.Color.bar.text
  // How much of it shows, from its left edge.
  property real shownWidth: width
  // The screen it is drawn on.
  property var screen: null

  readonly property color accent: Commons.Color.accent
  readonly property color selectedBackground: Commons.Color.menu.selectedBackground
  // Under the pointer or the search's cursor, lighter than the space you're
  // in, which keeps `selectedBackground`.
  readonly property color hoverBackground: Qt.rgba(selectedBackground.r, selectedBackground.g, selectedBackground.b, selectedBackground.a * 0.6)
  readonly property string fontFamily: Style.font.family
  // The rail's geometry, and the rules it keeps, are the sidebar's.
  readonly property int margin: root.sidebar.inset
  // The icon column at the start of each row: a cell, as wide as it is tall.
  readonly property int slot: root.sidebar.cell
  readonly property int badgeSize: root.sidebar.badgeSize
  // How wide a row's highlight is: to just short of the cut, so collapsed,
  // a cell.
  readonly property real highlightWidth: Math.max(root.slot, root.shownWidth - root.margin * 2)
  // How far it has expanded, from 0 at the rail to 1.
  readonly property real expansion: Math.max(0, Math.min(1,
    (root.shownWidth - root.sidebar.railWidth) / Math.max(1, root.sidebar.panelWidth - root.sidebar.railWidth)))
  // Every row is a cell tall, so collapsed, each icon's box is square.
  readonly property int rowHeight: root.slot
  // None when the header is on the bar (TabsWidget.qml).
  readonly property int headerHeight: root.sidebar.headerOnBar ? 0 : root.slot
  readonly property int searchHeight: root.slot
  // How much its text shows: all of it while the sidebar is expanded on this
  // screen, none of it otherwise, fading quickly between, so the rail is
  // icons alone.
  property real textShown: root.sidebar.opened && !!root.sidebar.screen && !!root.screen
    && root.sidebar.screen.name === root.screen.name ? 1 : 0
  Behavior on textShown { NumberAnimation { duration: Style.duration(120) } }
  // Where a row's text starts, past its icon.
  readonly property int textX: root.slot + root.sidebar.textGap

  // The row being dragged, where it would drop, and how far it has moved.
  property int dragFrom: -1
  property int dragTo: -1
  property real dragDy: 0
  // From one row to the next.
  readonly property real pitch: root.rowHeight + list.spacing
  // How far a press has to move up or down to become a drag.
  readonly property real dragThreshold: Math.round(root.rowHeight * 0.4)

  function endDrag() {
    root.dragFrom = -1
    root.dragTo = -1
    root.dragDy = 0
  }

  function reveal() {
    if (root.sidebar.shownRows.length > 0) list.positionViewAtIndex(root.sidebar.selectedIndex, ListView.Contain)
  }

  Connections {
    target: root.sidebar
    function onSelectedIndexChanged() { root.reveal() }
    function onShownRowsChanged() { Qt.callLater(root.reveal) }
  }

  // A glyph centred in its box by the ink it draws, not by the cell it takes:
  // Nerd Font icons overhang their cell, and letters sit on a baseline.
  component Glyph: Item {
    id: glyph
    property alias text: label.text
    property alias color: label.color
    property alias font: label.font

    TextMetrics {
      id: ink
      font: label.font
      text: label.text
    }

    Text {
      id: label
      x: Math.round((glyph.width - ink.tightBoundingRect.width) / 2 - ink.tightBoundingRect.x)
      y: Math.round((glyph.height - ink.tightBoundingRect.height) / 2 - (label.baselineOffset + ink.tightBoundingRect.y))
      textFormat: Text.PlainText
    }
  }

  // An icon centred in the icon column.
  component SlotIcon: Glyph {
    property bool lit: false
    width: root.slot
    height: parent.height
    color: lit ? root.accent : root.foreground
    opacity: lit ? 1 : 0.6
    font.family: root.fontFamily
    font.pixelSize: Style.font.icon
  }

  // A control's highlight: a box `controlSize` wide, centred in its cell,
  // lighter under the pointer (`lit`), steadier while in use (`held`).
  component ControlBox: Rectangle {
    property bool lit: false
    property bool held: false
    x: (root.slot - width) / 2
    y: (root.slot - height) / 2
    width: root.sidebar.controlSize
    height: root.sidebar.controlSize
    radius: root.sidebar.badgeRadius
    color: held ? root.selectedBackground : lit ? root.hoverBackground : "transparent"
  }

  // A row's highlight, ending just short of the cut: lighter under the
  // pointer (`lit`), steady for the space you're in (`held`) once the
  // sidebar expands, where the badge alone marks it on the rail.
  component Highlight: Rectangle {
    property bool lit: false
    property bool held: false
    width: Math.min(parent.width, root.highlightWidth)
    height: parent.height
    radius: Style.cornerRadius
    color: {
      var base = root.selectedBackground
      var alpha = Math.max(held ? base.a * root.expansion : 0, lit ? root.hoverBackground.a : 0)
      return Qt.rgba(base.r, base.g, base.b, alpha)
    }
  }

  Column {
    anchors.fill: parent
    anchors.margins: root.margin
    spacing: root.margin

    // The title, then the toggle. On the rail the toggle takes the icon
    // column; as the sidebar expands it rides the cut to the right, where a
    // row's highlight ends, and the title shows in the room it leaves,
    // starting in line with the badges.
    Item {
      visible: !root.sidebar.headerOnBar
      width: parent.width
      height: root.headerHeight

      Item {
        readonly property real start: (root.slot - root.badgeSize) / 2
        x: start
        width: Math.max(0, toggle.x - start)
        height: parent.height
        clip: true

        // As wide as the room it has once expanded, so while the sidebar
        // slides it is uncovered rather than cut short.
        Text {
          width: root.width - root.margin * 2 - root.slot - parent.x
          height: parent.height
          verticalAlignment: Text.AlignVCenter
          textFormat: Text.PlainText
          text: "Spaces"
          color: root.foreground
          opacity: 0.5 * root.textShown
          font.family: root.fontFamily
          font.pixelSize: Style.font.subtitle
          elide: Text.ElideRight
        }
      }

      Item {
        id: toggle
        x: root.highlightWidth - root.slot
        width: root.slot
        height: parent.height

        ControlBox {
          lit: toggleMouse.containsMouse
        }

        SlotIcon {
          // The sidebar, docked on the left (nf-cod-layout_sidebar_left).
          text: "\u{ebf3}"
          lit: toggleMouse.containsMouse
        }

        MouseArea {
          id: toggleMouse
          anchors.fill: parent
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: root.sidebar.toggleSidebar(root.screen)
        }
      }
    }

    Item {
      width: parent.width
      // The column spaces only the rows it shows.
      height: parent.height - root.headerHeight - root.searchHeight - parent.spacing * (root.sidebar.headerOnBar ? 1 : 2)

      ListView {
        id: list
        anchors.fill: parent
        model: root.sidebar.shownRows
        clip: true
        spacing: Style.spacing.xxs
        boundsBehavior: Flickable.StopAtBounds
        interactive: root.dragFrom < 0

        delegate: Item {
          id: rowItem
          required property var modelData
          required property int index
          readonly property bool selected: index === root.sidebar.selectedIndex
          // Its name is being typed.
          readonly property bool renaming: !!root.controller && root.controller.renaming !== "" && root.controller.renaming === modelData.id
          readonly property bool menuOpen: !!root.controller && root.controller.menuFor !== "" && root.controller.menuFor === modelData.id
          // Its icon is being picked: the badge shows the one under the
          // picker's cursor.
          readonly property bool picking: !!root.controller && root.controller.iconFor !== "" && root.controller.iconFor === modelData.id
          readonly property string icon: picking && root.controller.iconPreview ? root.controller.iconPreview : modelData.icon
          readonly property bool imageIcon: !home && Spaces.isImageIcon(icon)
          readonly property bool home: modelData.id === "home"
          readonly property color hue: rowItem.home ? root.foreground
            : (Spaces.badgeColor(rowItem.modelData.label, root.controller ? root.controller.palette : Spaces.DEFAULT_PALETTE) || root.foreground)
          readonly property bool dragged: root.dragFrom === index
          // Aside, to open the gap a dragged row would drop into.
          property real shift: Spaces.dragShift(index, root.dragFrom, root.dragTo, root.pitch)
          Behavior on shift { NumberAnimation { duration: Style.duration(120); easing.type: Easing.OutCubic } }

          width: ListView.view.width
          height: root.rowHeight
          z: dragged ? 1 : 0
          transform: Translate { y: rowItem.dragged ? root.dragDy : rowItem.shift }

          // The cursor shows while searching; otherwise only under the
          // pointer, or while the row is being renamed or has its menu open.
          Highlight {
            held: rowItem.modelData.active
            lit: rowItem.renaming || rowItem.menuOpen || rowItem.picking || rowItem.dragged
              || (!(root.controller && root.controller.menuRow) && !(root.controller && root.controller.iconRow) && rowItem.selected && (root.sidebar.searching || rowMouse.containsMouse))
            border.width: rowItem.renaming ? 1 : 0
            border.color: root.accent
          }

          // Its badge: two letters on a tint of its colour, or the icon you
          // picked for it, a glyph in its colour or an emoji; a house for
          // home, which is no folder's.
          Rectangle {
            id: badge
            x: (root.slot - width) / 2
            anchors.verticalCenter: parent.verticalCenter
            width: root.badgeSize
            height: root.badgeSize
            radius: root.sidebar.badgeRadius
            color: Qt.rgba(rowItem.hue.r, rowItem.hue.g, rowItem.hue.b, rowItem.home || rowItem.imageIcon ? 0.1 : 0.16)
            opacity: rowItem.modelData.active ? 1 : 1 - 0.45 * (1 - root.expansion)

            // The space you're in, on the rail: bordered in the theme's
            // foreground, fading as the sidebar expands. Drawn over the badge
            // rather than as its border, which would leave the badge's tint
            // a ring short as it faded.
            Rectangle {
              anchors.fill: parent
              visible: rowItem.modelData.active && root.expansion < 1
              radius: parent.radius
              color: "transparent"
              border.width: 1
              border.color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.6 * (1 - root.expansion))
            }

            Image {
              id: badgeImage
              visible: rowItem.imageIcon && status === Image.Ready
              anchors.fill: parent
              anchors.margins: Math.max(2, Math.round(root.badgeSize * 0.1))
              source: rowItem.imageIcon ? Spaces.fileUrl(rowItem.icon) : ""
              fillMode: Image.PreserveAspectFit
              smooth: true
              mipmap: true
              asynchronous: true
              cache: true
              sourceSize.width: root.badgeSize * 2
              sourceSize.height: root.badgeSize * 2
            }

            Glyph {
              readonly property bool letters: !rowItem.home && (!rowItem.icon || rowItem.imageIcon)
              visible: !rowItem.imageIcon || badgeImage.status !== Image.Ready
              anchors.fill: parent
              // A house (nf-md-home_outline).
              text: rowItem.home ? "\u{f06a1}" : (!rowItem.imageIcon && rowItem.icon) || Spaces.monogram(rowItem.modelData.label)
              color: rowItem.hue
              opacity: rowItem.home ? 0.8 : 1
              font.family: root.fontFamily
              font.pixelSize: rowItem.home ? Style.font.icon
                : letters ? Math.round(root.badgeSize * 0.42)
                : Math.round(root.badgeSize * (Spaces.isGlyph(rowItem.icon) ? 0.55 : 0.6))
              font.bold: letters
            }
          }

          // What most needs you among its agents, on the badge's corner, at
          // full strength however dimmed the badge is.
          StatusDot {
            status: rowItem.modelData.status
            colors: root.controller ? root.controller.statusColors : null
            workingColor: root.foreground
            size: Math.max(6, Math.round(root.badgeSize * 0.28))
            x: badge.x + badge.width - width * 0.7
            y: badge.y - height * 0.3
          }

          Column {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.leftMargin: root.textX
            anchors.rightMargin: Style.spacing.rowPaddingX
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.spacing.xxs
            opacity: root.textShown

            Text {
              id: nameText
              width: parent.width
              textFormat: Text.PlainText
              text: rowItem.renaming ? root.controller.nameText : rowItem.modelData.label
              // The search's cursor; otherwise the highlight says which space you're in.
              color: rowItem.selected && root.sidebar.searching ? root.accent : root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.body
              font.bold: rowItem.modelData.active
              elide: rowItem.renaming ? Text.ElideLeft : Text.ElideRight

              // The keyboard reaches the rename through the controller's
              // panel, so the name draws its own selection, under the text,
              // and its own caret, which a selection hides.
              Rectangle {
                visible: rowItem.renaming && root.controller.nameSelected
                z: -1
                width: Math.min(nameText.contentWidth, nameText.width)
                height: Style.font.subtitle + Style.spacing.xxs
                anchors.verticalCenter: parent.verticalCenter
                radius: Math.max(1, Style.space(2))
                color: Qt.rgba(root.accent.r, root.accent.g, root.accent.b, 0.3)
              }

              Rectangle {
                visible: rowItem.renaming && !root.controller.nameSelected
                x: Math.min(nameText.contentWidth, nameText.width) + 1
                width: Math.max(1, Style.space(2))
                height: Style.font.subtitle + Style.spacing.xxs
                anchors.verticalCenter: parent.verticalCenter
                color: root.accent

                SequentialAnimation on opacity {
                  running: rowItem.renaming
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

          // Click to go there, right-click for the space's menu, drag to
          // move it.
          MouseArea {
            id: rowMouse
            // Where the press was, in the list, which doesn't move with the
            // row; and whether it became a drag, which isn't a click.
            property real pressY: 0
            property bool wasDrag: false
            anchors.fill: parent
            hoverEnabled: true
            preventStealing: true
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            cursorShape: rowItem.renaming ? Qt.IBeamCursor
              : rowItem.dragged ? Qt.ClosedHandCursor : Qt.PointingHandCursor
            onContainsMouseChanged: if (containsMouse && root.dragFrom < 0) root.sidebar.select(rowItem.index)
            onPressed: function(mouse) {
              rowMouse.wasDrag = false
              rowMouse.pressY = rowMouse.mapToItem(list, mouse.x, mouse.y).y
            }
            onPositionChanged: function(mouse) {
              if (!(rowMouse.pressedButtons & Qt.LeftButton)) return
              var dy = rowMouse.mapToItem(list, mouse.x, mouse.y).y - rowMouse.pressY
              if (root.dragFrom < 0) {
                if (rowItem.renaming || root.sidebar.filterText !== ""
                    || !Spaces.startsDrag(rowItem.modelData, dy, root.dragThreshold)) return
                if (root.controller) root.controller.closeMenu()
                root.dragFrom = rowItem.index
                rowMouse.wasDrag = true
              }
              if (!rowItem.dragged) return
              root.dragDy = dy
              root.dragTo = Spaces.dropIndex(rowItem.index, dy, root.pitch, list.count)
            }
            onReleased: {
              if (!rowItem.dragged) return
              var from = root.dragFrom
              var to = root.dragTo
              root.endDrag()
              // Later: the new order makes new rows, this one among them.
              Qt.callLater(root.sidebar.reorder, from, to)
            }
            onCanceled: if (rowItem.dragged) root.endDrag()
            onClicked: function(mouse) {
              if (rowItem.renaming || rowMouse.wasDrag) return
              if (mouse.button !== Qt.RightButton) {
                root.sidebar.activate(rowItem.index)
              } else if (root.controller && Spaces.rowActions(rowItem.modelData).length > 0) {
                // The base covers the screen, so its coordinates are the
                // screen's, which the menu's surface shares.
                var at = rowMouse.mapToItem(null, mouse.x, mouse.y)
                root.controller.openMenu(rowItem.modelData.id, at.x, at.y, root.screen)
              }
            }
          }
        }

        // The `+`: a new space with a terminal in the folder you are in.
        footer: Item {
          width: ListView.view ? ListView.view.width : 0
          height: root.rowHeight + Style.spacing.xxs

          Item {
            y: Style.spacing.xxs
            width: parent.width
            height: parent.height - y

            ControlBox {
              lit: plusMouse.containsMouse
            }

            SlotIcon {
              // A plus (nf-md-plus).
              text: "\u{f0415}"
              lit: plusMouse.containsMouse
            }

            Text {
              x: root.textX
              width: parent.width - x
              height: parent.height
              verticalAlignment: Text.AlignVCenter
              textFormat: Text.PlainText
              text: "New space"
              color: plusMouse.containsMouse ? root.accent : root.foreground
              opacity: (plusMouse.containsMouse ? 1 : 0.6) * root.textShown
              font.family: root.fontFamily
              font.pixelSize: Style.font.body
              elide: Text.ElideRight
            }

            MouseArea {
              id: plusMouse
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: root.sidebar.newSpace()
            }
          }
        }
      }

      Text {
        anchors.centerIn: parent
        width: parent.width
        visible: root.sidebar.shownRows.length === 0 && (root.sidebar.error !== "" || root.sidebar.filterText !== "")
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.Wrap
        textFormat: Text.PlainText
        text: root.sidebar.error !== "" ? root.sidebar.error : "No space matches “" + root.sidebar.filterText + "”"
        color: root.foreground
        opacity: 0.7
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
      }
    }

    // Click it, or press the key for it, to search. Its magnifier alone until you
    // point at it or search; then it is one input, a control's box that
    // starts where the other controls' do and stretches to as far short of
    // the cut, with the magnifier in it: a square on the rail, the whole row
    // when expanded.
    Item {
      id: searchRow
      readonly property bool open: searchMouse.containsMouse || root.sidebar.searching
      readonly property real boxInset: (root.slot - root.sidebar.controlSize) / 2
      width: parent.width
      height: root.searchHeight

      Rectangle {
        id: searchField
        x: searchRow.boxInset
        y: searchRow.boxInset
        width: Math.min(parent.width, root.highlightWidth) - searchRow.boxInset * 2
        height: root.sidebar.controlSize
        radius: root.sidebar.badgeRadius
        color: root.sidebar.searching ? root.selectedBackground
          : searchMouse.containsMouse ? root.hoverBackground : "transparent"
      }

      SlotIcon {
        // A magnifying glass (nf-md-magnify).
        text: "\u{f0349}"
        lit: searchRow.open
        font.pixelSize: root.sidebar.searchIconSize
      }

      // What you type follows the magnifier as in any input, rather than
      // lining up with the spaces' names above.
      Item {
        x: Math.round(root.slot / 2 + root.sidebar.searchIconSize / 2 + Style.spacing.md)
        width: searchField.x + searchField.width - x - Style.spacing.rowPaddingX
        height: parent.height
        clip: true
        opacity: searchRow.open ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Style.duration(120) } }

        Row {
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.verticalCenter: parent.verticalCenter
          spacing: 0
          clip: true

          Text {
            id: query
            visible: root.sidebar.filterText !== ""
            width: Math.min(implicitWidth, parent.width - caret.width)
            textFormat: Text.PlainText
            text: root.sidebar.filterText
            color: root.foreground
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            elide: Text.ElideLeft
          }

          Rectangle {
            id: caret
            visible: root.sidebar.searching
            width: Math.max(1, Style.space(2))
            height: Style.font.body + Style.spacing.xxs
            anchors.verticalCenter: parent.verticalCenter
            color: root.accent

            SequentialAnimation on opacity {
              running: caret.visible
              loops: Animation.Infinite
              alwaysRunToEnd: false
              NumberAnimation { to: 1; duration: 0 }
              PauseAnimation { duration: 530 }
              NumberAnimation { to: 0; duration: 0 }
              PauseAnimation { duration: 530 }
            }
          }

        }

        // The placeholder lies under the caret, as in any input, so it stays
        // put when the search starts rather than stepping aside for it.
        Text {
          z: -1
          visible: root.sidebar.filterText === ""
          anchors.left: parent.left
          anchors.verticalCenter: parent.verticalCenter
          textFormat: Text.PlainText
          text: "Search"
          color: root.foreground
          opacity: 0.4
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
        }
      }

      MouseArea {
        id: searchMouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.IBeamCursor
        onClicked: root.sidebar.search()
      }
    }
  }
}
