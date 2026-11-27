import QtQuick
import qs.Commons as Commons

// A dot saying what an agent is doing: waiting for you, in the theme's red;
// done while you were elsewhere, in its green; working, in its yellow (or
// `workingColor` when the theme has none), breathing. Nothing when it's idle.
Rectangle {
  id: root

  // "working", "done", "waiting", or "" when idle.
  property string status: ""
  // The theme's colours for them, from Spaces.statusColors.
  property var colors: null
  property color workingColor: Commons.Color.foreground
  property real size: 6

  visible: root.status !== ""
  implicitWidth: root.size
  implicitHeight: root.size
  width: root.size
  height: root.size
  radius: root.size / 2
  color: {
    var colors = root.colors || {}
    if (root.status === "waiting") return colors.waiting || Commons.Color.urgent
    if (root.status === "done") return colors.done || Commons.Color.accent
    return colors.working || root.workingColor
  }
  opacity: root.status === "working" ? root.breath : 1

  property real breath: 1
  SequentialAnimation on breath {
    running: root.visible && root.status === "working"
    loops: Animation.Infinite
    NumberAnimation { to: 0.25; duration: 900; easing.type: Easing.InOutSine }
    NumberAnimation { to: 1; duration: 900; easing.type: Easing.InOutSine }
  }
}
