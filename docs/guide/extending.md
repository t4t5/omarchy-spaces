# Extending Spaces

Spaces keeps its core small on purpose — it owns spaces, tabs, Hyprland sync, session restore, and the Wayland sidebar/bar frames, while leaving project workflows (git status, worktrees, agent lists, PRs, ports) to composable layers on top:

- **`space` CLI + `space events` + `space meta`** for scripts, keybindings, and coding agents.
- **`tabsWidget` and `sidebarWidget`** in `~/.config/omarchy/shell.json` to replace the tabs strip or the entire sidebar content while keeping Spaces' Wayland exclusive zones, Hyprland-synced slide animation, blur, menus, icon picker, and keyboard panel.

Nothing about the sidebar forces a single row layout: a `sidebarWidget` owns the whole sidebar surface, so it can nest worktrees under their parent repo, show multiple pills per row, or stack an Agents section below the spaces list.

## Recipe 1: Custom sidebar with git status, nested worktrees, and live agents

Because a `sidebarWidget` is an ordinary Quickshell QML module, it can inspect git folders directly with `Quickshell.Io.Process` (no background daemon needed) and read live agent terminals straight from `spaces.spaces[i].terminals`.

Save this as `~/.config/omarchy/bar/modules/dev-sidebar.qml`:

```qml
import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons

Item {
  id: root
  property var spaces: null
  property var settings: ({})
  property color foreground: Color.bar.text
  property real shownWidth: width

  // folder -> { branch, dirty, mainRoot, isWorktree }
  property var gitByFolder: ({})
  property var queue: []
  property string checking: ""

  readonly property real expansion: spaces ? spaces.expansion : 0
  readonly property int rail: spaces ? spaces.railWidth : 44
  readonly property int inset: spaces ? spaces.inset : 8
  readonly property int cell: spaces ? spaces.cell : 28

  // Re-check git state whenever spaces appear or change folder, and periodically.
  function refreshGit() {
    if (!spaces) return
    var seen = {}
    var next = []
    for (var i = 0; i < spaces.spaces.length; i++) {
      var dir = spaces.spaces[i].folder
      if (dir && !seen[dir]) {
        seen[dir] = true
        next.push(dir)
      }
    }
    root.queue = next
    root.pump()
  }

  function pump() {
    if (gitProc.running || root.queue.length === 0) return
    root.checking = root.queue[0]
    root.queue = root.queue.slice(1)
    gitProc.command = [
      "sh", "-c",
      "dir=\"$1\"; git -C \"$dir\" rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 0; " +
      "top=$(git -C \"$dir\" rev-parse --show-toplevel); " +
      "common=$(git -C \"$dir\" rev-parse --path-format=absolute --git-common-dir); " +
      "main=$(dirname \"$common\"); " +
      "br=$(git -C \"$dir\" symbolic-ref --short HEAD 2>/dev/null || git -C \"$dir\" rev-parse --short HEAD); " +
      "dirty=$(test -n \"$(git -C \"$dir\" status --porcelain 2>/dev/null)\" && echo 1 || echo 0); " +
      "printf '%s\\t%s\\t%s\\t%s\\n' \"$br\" \"$dirty\" \"$main\" \"$(test \"$top\" != \"$main\" && echo 1 || echo 0)\"",
      "_", root.checking
    ]
    gitProc.running = true
  }

  Process {
    id: gitProc
    stdout: StdioCollector {
      onStreamFinished: {
        var line = String(text || "").trim()
        var next = Object.assign({}, root.gitByFolder)
        if (!line) {
          delete next[root.checking]
        } else {
          var parts = line.split("\t")
          next[root.checking] = {
            branch: parts[0] || "",
            dirty: parts[1] === "1",
            mainRoot: parts[2] || "",
            isWorktree: parts[3] === "1",
          }
        }
        root.gitByFolder = next
        root.pump()
      }
    }
  }

  Timer { interval: 10000; running: true; repeat: true; onTriggered: root.refreshGit() }
  Connections {
    target: root.spaces
    function onSpacesChanged() { root.refreshGit() }
  }

  // Group linked worktrees directly beneath their main checkout space.
  readonly property var displayRows: {
    if (!spaces) return []
    var list = spaces.searching ? spaces.filteredSpaces : spaces.spaces
    var byMain = {}
    for (var i = 0; i < list.length; i++) {
      var s = list[i]
      var g = root.gitByFolder[s.folder]
      if (g && !g.isWorktree && g.mainRoot) byMain[g.mainRoot] = s.id
    }
    var children = {}
    var roots = []
    for (var j = 0; j < list.length; j++) {
      var item = list[j]
      var info = root.gitByFolder[item.folder]
      var parentId = info && info.isWorktree ? byMain[info.mainRoot] : ""
      if (parentId && parentId !== item.id) {
        if (!children[parentId]) children[parentId] = []
        children[parentId].push({ space: item, git: info, indent: true })
      } else {
        roots.push({ space: item, git: info || null, indent: false })
      }
    }
    var out = []
    for (var k = 0; k < roots.length; k++) {
      out.push(roots[k])
      var kids = children[roots[k].space.id] || []
      for (var c = 0; c < kids.length; c++) out.push(kids[c])
    }
    return out
  }

  // All terminals currently reporting an agent status across every space.
  readonly property var activeAgents: {
    if (!spaces) return []
    var out = []
    for (var i = 0; i < spaces.spaces.length; i++) {
      var s = spaces.spaces[i]
      for (var j = 0; j < s.terminals.length; j++) {
        var t = s.terminals[j]
        if (t.status) out.push({ space: s, terminal: t })
      }
    }
    return out
  }

  Column {
    anchors.fill: parent
    anchors.margins: root.inset
    spacing: Style.spacing.xs

    Repeater {
      model: root.displayRows

      Rectangle {
        required property var modelData
        readonly property var sp: modelData.space
        readonly property var git: modelData.git
        width: Math.max(root.cell, root.shownWidth - root.inset * 2)
        height: git && root.expansion > 0.5 ? root.cell + 14 : root.cell
        radius: Style.cornerRadius
        color: sp.active ? Color.menu.selectedBackground : rowMouse.containsMouse ? Qt.rgba(Color.menu.selectedBackground.r, Color.menu.selectedBackground.g, Color.menu.selectedBackground.b, 0.5) : "transparent"

        Row {
          anchors.fill: parent
          anchors.leftMargin: modelData.indent && root.expansion > 0.5 ? Style.spacing.md : 0
          spacing: Style.spacing.xs

          Item {
            width: root.cell
            height: parent.height
            Text {
              anchors.centerIn: parent
              textFormat: Text.PlainText
              text: sp.id === "home" ? "\u{f06a1}" : (sp.icon || sp.label.slice(0, 2).toUpperCase())
              color: sp.status === "waiting" ? Color.accent : root.foreground
              font.family: Style.font.family
              font.pixelSize: Style.font.body
            }
          }

          Column {
            anchors.verticalCenter: parent.verticalCenter
            width: Math.max(0, parent.width - root.cell - Style.spacing.md)
            opacity: root.expansion
            visible: root.expansion > 0.05

            Text {
              width: parent.width
              textFormat: Text.PlainText
              text: sp.label
              color: root.foreground
              font.family: Style.font.family
              font.pixelSize: Style.font.body
              font.bold: sp.active
              elide: Text.ElideRight
            }

            Text {
              visible: !!git && git.branch !== ""
              width: parent.width
              textFormat: Text.PlainText
              text: git ? (git.isWorktree ? "󰘬 " : " ") + git.branch + (git.dirty ? " *" : "") : ""
              color: git && git.dirty ? Color.accent : root.foreground
              opacity: git && git.dirty ? 0.9 : 0.55
              font.family: Style.font.family
              font.pixelSize: Style.font.bodySmall
              elide: Text.ElideRight
            }
          }
        }

        MouseArea {
          id: rowMouse
          anchors.fill: parent
          hoverEnabled: true
          acceptedButtons: Qt.LeftButton | Qt.RightButton
          onClicked: function(mouse) {
            if (mouse.button === Qt.RightButton) {
              var at = rowMouse.mapToItem(null, mouse.x, mouse.y)
              spaces.openSpaceMenu(sp.id, at.x, at.y)
            } else {
              spaces.goToSpace(sp.id)
            }
          }
        }
      }
    }

    // Active agents section (only shown when at least one agent is busy/waiting/done).
    Column {
      visible: root.activeAgents.length > 0 && root.expansion > 0.5
      width: parent.width
      topPadding: Style.spacing.md
      spacing: Style.spacing.xxs

      Text {
        text: "Agents"
        color: root.foreground
        opacity: 0.45
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
      }

      Repeater {
        model: root.activeAgents

        Rectangle {
          required property var modelData
          width: parent.width
          height: Style.font.body + Style.spacing.sm * 2
          radius: Style.cornerRadius
          color: agentMouse.containsMouse ? Color.menu.selectedBackground : "transparent"

          Text {
            anchors.verticalCenter: parent.verticalCenter
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.margins: Style.spacing.xs
            textFormat: Text.PlainText
            text: modelData.space.label + " · " + modelData.terminal.label + " (" + modelData.terminal.status + ")"
            color: modelData.terminal.status === "waiting" ? Color.accent : root.foreground
            font.family: Style.font.family
            font.pixelSize: Style.font.bodySmall
            elide: Text.ElideRight
          }

          MouseArea {
            id: agentMouse
            anchors.fill: parent
            hoverEnabled: true
            onClicked: spaces.focusWindow(modelData.terminal.address)
          }
        }
      }
    }
  }
}
```

Enable it in `~/.config/omarchy/shell.json`:

```json
{
  "id": "com.t4t5.spaces",
  "sidebar": { "width": 240 },
  "sidebarWidget": { "id": "dev-sidebar", "type": "qml" }
}
```

Run `omarchy restart shell` to load a new QML module for the first time.

## Recipe 2: Creating & cleaning up git worktree spaces

Worktree creation composes directly on top of `space new` and `space meta` — bind it in `~/.config/hypr/bindings.lua` or call it from a button in your `sidebarWidget`:

```sh
#!/usr/bin/env bash
set -euo pipefail

# Resolve the current space's folder via `space ls --json`.
cur=$(space current)
dir=$(space ls --json | jq -r --arg id "$cur" '.spaces[] | select(.id == $id) | .folder // empty')
[ -n "$dir" ] && [ -d "$dir" ] || { echo "Space has no folder" >&2; exit 1; }

common=$(git -C "$dir" rev-parse --path-format=absolute --git-common-dir)
main_root=$(dirname "$common")

branch=$(printf '' | walker --dmenu --placeholder "New worktree branch…" || true)
[ -n "$branch" ] || exit 0

wt_dir="$main_root/.worktrees/$(printf '%s' "$branch" | tr '/ ' '--')"
mkdir -p "$(dirname "$wt_dir")"
if git -C "$main_root" show-ref --verify --quiet "refs/heads/$branch"; then
  git -C "$main_root" worktree add "$wt_dir" "$branch"
else
  git -C "$main_root" worktree add -b "$branch" "$wt_dir"
fi

# Optional: copy gitignored files listed in .worktreeinclude (e.g. .env).
if [ -f "$main_root/.worktreeinclude" ]; then
  while IFS= read -r rel; do
    [ -n "$rel" ] && [ -e "$main_root/$rel" ] || continue
    mkdir -p "$wt_dir/$(dirname "$rel")"
    cp -a "$main_root/$rel" "$wt_dir/$rel"
  done < "$main_root/.worktreeinclude"
fi

created=$(space new "$wt_dir" --name "$branch" --json -- claude)
new_id=$(printf '%s' "$created" | jq -r '.id')
space meta set -s "$new_id" worktree --json "$(jq -cn --arg repo "$main_root" --arg path "$wt_dir" --arg branch "$branch" '{repo: $repo, path: $path, branch: $branch}')" >/dev/null
```

## Starting from the default sidebar

If you want Spaces' exact default look (badge monograms, drag-to-reorder, search bar, inline rename) with your own tweaks, copy `~/.config/omarchy/plugins/com.t4t5.spaces/omarchy/SidebarList.qml` into `~/.config/omarchy/bar/modules/my-sidebar.qml`, rewrite its bindings to use the public `spaces` API ([sidebar.md](sidebar.md#customizing-the-sidebar)), and point `sidebarWidget` at it. Plugin updates will continue updating the built-in default while leaving your copy untouched.
