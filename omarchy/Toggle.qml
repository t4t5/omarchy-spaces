import QtQuick

// The overlay entry the shell summons and hides: the sidebar's search.
// The sidebar itself lives in the service, mapped for good, so
// this only forwards. The shell counts it open while you are searching.
Item {
  property string omarchyPath: ""
  property var shell: null
  property var manifest: null
  property var service: null

  readonly property bool opened: !!service && service.searching === true

  function open(payloadJson) {
    if (service) service.search()
  }

  function close() {
    if (service) service.endSearch()
  }

  // Show or hide the sidebar without taking the keyboard, as the
  // logo on the bar does. Called through `omarchy-shell shell call
  // com.t4t5.spaces toggleSidebar`.
  function toggleSidebar(arg) {
    if (service) service.toggleSidebar()
  }

  // Rename the space you are in, typing over its name in the sidebar.
  // Called through `omarchy-shell shell call
  // com.t4t5.spaces renameSpace`.
  function renameSpace(arg) {
    if (service) service.renameSpace()
  }

  // Name the tab you are on, typing on the bar. Called
  // through `omarchy-shell shell call com.t4t5.spaces renameTab`.
  function renameTab(arg) {
    if (service) service.renameTab()
  }

  // Pick a space to send the tab you are on to, and go there with it. Called
  // through `omarchy-shell shell call com.t4t5.spaces sendTab`.
  function sendTab(arg) {
    if (service) service.sendTab()
  }

  // Pick a space to send the focused window to (pass 'silent' to stay put).
  // Called through `omarchy-shell shell call com.t4t5.spaces sendWindow`.
  function sendWindow(arg) {
    if (service) service.sendWindow(arg)
  }

  // What shows, for `just stress`. Called through `omarchy-shell shell call
  // com.t4t5.spaces shown`.
  function shown(arg) {
    return service ? service.shown() : ""
  }
}
