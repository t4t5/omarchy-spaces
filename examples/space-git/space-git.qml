import QtQuick
import qs.Commons

// A space's git branch, under its name in the sidebar, from the `git` key the
// space-git script keeps in its metadata. A dot when it has changes. Nothing
// for a space that isn't in a repo.
Item {
  id: root

  property var space: null
  property color foreground: Color.bar.text
  property color accent: Color.accent

  readonly property var git: root.space && root.space.meta ? root.space.meta.git : null

  visible: !!root.git && !!root.git.branch
  implicitWidth: label.implicitWidth

  Text {
    id: label
    width: root.width
    anchors.verticalCenter: parent.verticalCenter
    textFormat: Text.PlainText
    // A branch (nf-dev-git_branch).
    text: root.git ? "\u{e725} " + root.git.branch + (root.git.dirty ? " ●" : "") : ""
    color: root.git && root.git.dirty ? root.accent : root.foreground
    opacity: root.git && root.git.dirty ? 0.9 : 0.55
    font.family: Style.font.family
    font.pixelSize: Style.font.bodySmall
    elide: Text.ElideRight
  }
}
