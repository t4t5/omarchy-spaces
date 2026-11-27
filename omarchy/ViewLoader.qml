pragma ComponentBehavior: Bound

import QtQuick
import Quickshell.Io
import "Spaces.mjs" as Spaces

// Loads a swappable view node (`Spaces.parseViewNode`), validating its
// `api` version and falling back to `fallbackComponent` on any load error or
// mismatch so a broken custom view never blanks the bar or sidebar.
//
// Custom files are watched for edits and reloaded with a cache-busting query
// string so saving a local `.qml` view updates live without restarting the
// shell. Errors are deduplicated through `service.reportViewError`.
Item {
  id: root

  property var node: ({ mode: "default", path: "", url: "", children: {}, error: "" })
  property Component fallbackComponent: null
  property int expectedApi: Spaces.VIEW_API
  property var props: ({})
  property var service: null

  property int revision: 0
  property bool loadFailed: false
  property bool apiInvalid: false

  readonly property bool wantCustom: !!root.node && root.node.mode === "custom" && root.node.url !== ""
  readonly property bool customOk: root.wantCustom && !root.loadFailed && !root.apiInvalid
    && customLoader.status === Loader.Ready && !!customLoader.item
  readonly property bool useFallback: !root.customOk
  readonly property var activeItem: root.customOk ? customLoader.item : fallbackLoader.item

  implicitWidth: activeItem ? activeItem.implicitWidth : 0
  implicitHeight: activeItem ? activeItem.implicitHeight : 0

  onNodeChanged: {
    root.loadFailed = false
    root.apiInvalid = false
    root.revision = 0
    if (root.node && root.node.mode === "error" && root.node.error && root.service) {
      root.service.reportViewError("config:" + root.node.path, root.node.error)
    }
  }

  onPropsChanged: root.applyProps()

  function applyProps() {
    var target = root.activeItem
    var map = root.props
    if (!target || !map) return
    for (var key in map) {
      if (!(key in target)) continue
      if (target[key] !== map[key]) target[key] = map[key]
    }
  }

  // Only instantiated when a custom file is configured.
  Loader {
    active: root.wantCustom && !!root.node.path
    sourceComponent: FileView {
      path: root.node.path
      watchChanges: true
      printErrors: false
      onFileChanged: {
        root.loadFailed = false
        root.apiInvalid = false
        root.revision++
      }
    }
  }

  Loader {
    id: customLoader
    active: root.wantCustom
    source: active ? Spaces.versionedUrl(root.node.url, root.revision) : ""
    visible: root.customOk
    width: root.width
    height: root.height

    onLoaded: {
      if (!item) return
      if (item.api !== root.expectedApi) {
        root.apiInvalid = true
        if (root.service) {
          var got = item.api === undefined ? "none" : String(item.api)
          root.service.reportViewError(root.node.path, root.node.path + ": api is " + got + " (expected " + root.expectedApi + ")")
        }
        return
      }
      root.apiInvalid = false
      root.loadFailed = false
      if (root.service) root.service.clearViewError(root.node.path)
      root.applyProps()
    }

    onStatusChanged: {
      if (status === Loader.Error) {
        root.loadFailed = true
        if (root.service) {
          var detail = typeof customLoader.errorString === "function" ? customLoader.errorString() : ""
          root.service.reportViewError(root.node.path, detail || ("Could not load " + root.node.path))
        }
      }
    }
  }

  Loader {
    id: fallbackLoader
    active: root.useFallback && !!root.fallbackComponent
    sourceComponent: root.fallbackComponent
    visible: root.useFallback
    width: root.width
    height: root.height
    onLoaded: root.applyProps()
  }
}
