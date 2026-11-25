pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Spaces.mjs" as Spaces

// The space to move a tab to, picked from a list (Sidebar.qml's `movingTab`):
// from the tab's menu (SpaceMenu.qml), where the menu was, or from a key, in
// the middle of the screen. It is made for the keyboard: it opens with the
// cursor on the space you left last, typing filters, Up and Down (or Tab, or
// Ctrl+N and Ctrl+P) move, Enter picks, and Escape clears what you typed, then
// closes. Like the menu, it has a surface of its own above the windows,
// covering the screen: a click outside the card closes it.
PanelWindow {
  id: root

  property var controller: null

  readonly property var targets: controller ? controller.moveTargets : []
  property string query: ""
  readonly property var shown: Spaces.filterRows(targets, query)
  property int current: 0

  readonly property color foreground: Commons.Color.popups.text
  readonly property color accent: Commons.Color.accent
  readonly property color selectedBackground: Commons.Color.menu.selectedBackground
  readonly property var borderSpec: Border.localOrSurfaceSpec("popups", "border", Commons.Color.popups.border, Commons.Color.popups.border, Style.normalBorderWidth)
  readonly property int edge: Style.spacing.md
  readonly property int rowHeight: Math.max(Style.spacing.popupRowHeight, Style.font.body + Style.spacing.controlPaddingY * 2)
  readonly property int badgeSize: Math.round(rowHeight * 0.75)
  // Nothing to move, or nowhere to move it.
  readonly property string note: {
    if (!controller || !controller.menuTab) return ""
    if (controller.menuTab.windows === 0) return "Nothing is open on this tab"
    if (root.targets.length === 0) return "There are no other spaces"
    return root.shown.length === 0 ? "No space matches “" + root.query.trim() + "”" : ""
  }

  visible: !!controller && controller.movingTab && !!controller.menuTab && !!controller.menuScreen
  screen: controller ? controller.menuScreen : null
  anchors { top: true; bottom: true; left: true; right: true }
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "omarchy-spaces-move"
  WlrLayershell.layer: WlrLayer.Overlay
  WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive

  onVisibleChanged: {
    if (!visible) return
    root.setQuery("")
    Qt.callLater(function() { keys.forceActiveFocus() })
  }

  // A space ending meanwhile takes its row with it.
  onShownChanged: if (root.current >= root.shown.length) root.current = Math.max(0, root.shown.length - 1)
  onCurrentChanged: list.positionViewAtIndex(root.current, ListView.Contain)

  function setQuery(text) {
    root.query = text
    root.current = text.trim() !== "" ? 0 : Spaces.initialIndex(root.shown)
  }

  function move(by) {
    var count = root.shown.length
    if (count > 0) root.current = (root.current + by + count) % count
  }

  function pick(index) {
    var row = root.shown[index]
    if (row && root.note === "") root.controller.sendTabTo(row.id)
  }

  // Anywhere outside the card closes it.
  MouseArea {
    anchors.fill: parent
    acceptedButtons: Qt.AllButtons
    onPressed: root.controller.closeMenu()
  }

  Item {
    id: keys
    anchors.fill: parent
    focus: true
    Keys.priority: Keys.BeforeItem
    Keys.onPressed: function(event) {
      var ctrl = event.modifiers & Qt.ControlModifier
      var printable = event.text && event.text.length === 1 && !ctrl
          && event.text.charCodeAt(0) >= 32 && event.text.charCodeAt(0) !== 127
      if (event.key === Qt.Key_Escape) {
        if (root.query) root.setQuery("")
        else root.controller.closeMenu()
      } else if (event.key === Qt.Key_Up || event.key === Qt.Key_Backtab || (ctrl && event.key === Qt.Key_P)) {
        root.move(-1)
      } else if (event.key === Qt.Key_Down || event.key === Qt.Key_Tab || (ctrl && event.key === Qt.Key_N)) {
        root.move(1)
      } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
        // Not one held from picking "Move to space…".
        if (!event.isAutoRepeat) root.pick(root.current)
      } else if (Util.editsFilter(event, root.query)) {
        root.setQuery(Util.editedFilter(event, root.query))
      } else if (printable) {
        root.setQuery(root.query + event.text)
      } else {
        return
      }
      event.accepted = true
    }
  }

  BorderSurface {
    id: card
    readonly property bool centered: !!root.controller && root.controller.moveCentered
    // Where the menu was, kept on the screen: above the pointer when there
    // isn't room below it.
    x: centered ? Math.round((root.width - width) / 2)
      : Math.max(root.edge, Math.min(root.controller ? root.controller.menuX : 0, root.width - width - root.edge))
    y: {
      // Its top stays put as typing shortens the list.
      if (centered) return Math.round(root.height * 0.25)
      var at = root.controller ? root.controller.menuY : 0
      return at + height + root.edge <= root.height ? at : Math.max(root.edge, at - height)
    }
    width: Math.min(Style.space(280), root.width - root.edge * 2)
    height: content.implicitHeight + contentTopInset + contentBottomInset
    color: Commons.Color.popups.background
    borderSpec: root.borderSpec
    radius: Style.cornerRadius
    padding: Style.spacing.xs

    // Clicks inside stay in the card.
    MouseArea {
      anchors.fill: parent
      acceptedButtons: Qt.AllButtons
    }

    Column {
      id: content
      x: card.contentLeftInset
      y: card.contentTopInset
      width: card.width - card.contentLeftInset - card.contentRightInset
      spacing: Style.spacing.xxs

      // What you've typed and a caret, or what it's for.
      Item {
        width: parent.width
        height: root.rowHeight

        Row {
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.leftMargin: Style.spacing.controlPaddingX
          anchors.rightMargin: Style.spacing.controlPaddingX
          anchors.verticalCenter: parent.verticalCenter
          clip: true

          Text {
            visible: root.query !== ""
            width: Math.min(implicitWidth, parent.width - caret.width)
            textFormat: Text.PlainText
            text: root.query
            color: root.foreground
            font.family: Style.font.family
            font.pixelSize: Style.font.body
            elide: Text.ElideLeft
          }

          Rectangle {
            id: caret
            width: Math.max(1, Style.space(2))
            height: Style.font.body + Style.spacing.xxs
            anchors.verticalCenter: parent.verticalCenter
            color: root.accent

            SequentialAnimation on opacity {
              running: root.visible
              loops: Animation.Infinite
              alwaysRunToEnd: false
              NumberAnimation { to: 1; duration: 0 }
              PauseAnimation { duration: 530 }
              NumberAnimation { to: 0; duration: 0 }
              PauseAnimation { duration: 530 }
            }
          }

          Text {
            visible: root.query === ""
            leftPadding: Style.spacing.xxs
            textFormat: Text.PlainText
            text: root.controller && root.controller.menuTab ? (root.controller.menuTab.slot > 0 ? "Move tab " + root.controller.menuTab.slot : "Move tab") + " to space" : ""
            color: root.foreground
            opacity: 0.4
            font.family: Style.font.family
            font.pixelSize: Style.font.body
          }
        }
      }

      Rectangle {
        width: parent.width - Style.spacing.controlPaddingX * 2
        x: Style.spacing.controlPaddingX
        height: Math.max(1, Style.spacing.hairline)
        color: root.foreground
        opacity: 0.15
      }

      ListView {
        id: list
        visible: root.note === ""
        width: parent.width
        height: Math.min(contentHeight, root.height * 0.5)
        model: root.shown
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        // The search changes the model; the cursor places the view.
        currentIndex: -1

        delegate: Rectangle {
          id: item
          required property var modelData
          required property int index
          readonly property bool hot: index === root.current
          readonly property bool home: modelData.id === "home"
          readonly property bool imageIcon: !home && Spaces.isImageIcon(modelData.icon)
          readonly property bool letters: !home && (!modelData.icon || imageIcon)
          readonly property color hue: home || !root.controller ? root.foreground
            : (Spaces.badgeColor(modelData.label, root.controller.palette) || root.foreground)

          width: list.width
          height: root.rowHeight
          radius: Style.cornerRadius
          color: hot ? Style.hoverFillFor(root.foreground, root.accent) : "transparent"

          // Its badge, as the sidebar draws it.
          Rectangle {
            id: badge
            x: Style.spacing.controlPaddingX
            anchors.verticalCenter: parent.verticalCenter
            width: root.badgeSize
            height: root.badgeSize
            radius: Math.round(root.badgeSize / 4)
            color: Qt.rgba(item.hue.r, item.hue.g, item.hue.b, item.home || item.imageIcon ? 0.1 : 0.16)

            Image {
              id: badgeImage
              visible: item.imageIcon && status === Image.Ready
              anchors.fill: parent
              anchors.margins: Math.max(2, Math.round(root.badgeSize * 0.1))
              source: item.imageIcon ? Spaces.fileUrl(item.modelData.icon) : ""
              fillMode: Image.PreserveAspectFit
              smooth: true
              mipmap: true
              asynchronous: true
              cache: true
              sourceSize.width: root.badgeSize * 2
              sourceSize.height: root.badgeSize * 2
            }

            Text {
              visible: !item.imageIcon || badgeImage.status !== Image.Ready
              anchors.centerIn: parent
              textFormat: Text.PlainText
              // A house (nf-md-home_outline).
              text: item.home ? "\u{f06a1}" : (!item.imageIcon && item.modelData.icon) || Spaces.monogram(item.modelData.label)
              color: item.hue
              font.family: Style.font.family
              font.pixelSize: Math.round(root.badgeSize * (item.letters ? 0.42 : 0.6))
              font.bold: item.letters
            }
          }

          Text {
            anchors.left: badge.right
            anchors.leftMargin: Style.spacing.controlGap
            anchors.right: parent.right
            anchors.rightMargin: Style.spacing.controlPaddingX
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            text: item.modelData.label
            color: item.hot ? Style.hoverStateColor(root.foreground, root.accent) : root.foreground
            font.family: Style.font.family
            font.pixelSize: Style.font.body
            elide: Text.ElideRight
          }

          MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onPositionChanged: root.current = item.index
            onClicked: root.pick(item.index)
          }
        }
      }

      Text {
        visible: root.note !== ""
        width: parent.width
        leftPadding: Style.spacing.controlPaddingX
        rightPadding: Style.spacing.controlPaddingX
        topPadding: Style.spacing.controlPaddingY
        bottomPadding: Style.spacing.controlPaddingY
        wrapMode: Text.Wrap
        textFormat: Text.PlainText
        text: root.note
        color: root.foreground
        opacity: 0.7
        font.family: Style.font.family
        font.pixelSize: Style.font.body
      }
    }
  }
}
