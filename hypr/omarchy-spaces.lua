-- Spaces for Hyprland. It binds no keys until asked: bind the ones you want in
-- ~/.config/hypr/bindings.lua, after the require (see docs/guide/keys.md).
--
-- The plugin links this file to ~/.config/hypr/omarchy-spaces.lua (install.sh).
-- The require goes at the end of bindings.lua, which loads after Omarchy's
-- defaults, in an `if` that skips it once the plugin is removed (README.md).

-- Each helper it returns makes an action for `o.bind` and `o.rebind`. What an
-- action is (a command, a function) is up to the helper and can change, so
-- bindings don't hold `space`'s commands themselves.
--
-- spaces.go_to_tab(n) and spaces.move_window_to_tab(n[, follow]) are plain Lua
-- inside Hyprland rather than a call to `space`, so they can't leave you
-- stuck: outside a space (`home`) each one runs the action Omarchy binds to
-- workspace n, and works even if `space` is missing.

-- Every monitor shows the same space. Within it each has tabs of its own,
-- numbered as Omarchy numbers its workspaces: the first monitor's are 1-10,
-- the next one's 11-20, and each shows as 1-10. So tab 2 of `s1` is `s1:2` on
-- the one and `s1:12` on the other.
local SLOTS = 10
local LAST_KEY = 250

-- The space and tab a workspace is, as `space` reads its name (src/name.rs):
-- `s1:n`. `home` and no tab for any other, nil for special ones.
local function locate(workspace)
  local name = workspace and workspace.name
  if not name or name == "" or name:match("^special:") then
    return nil
  end
  local space, key = name:match("^(s[1-9]%d*):([1-9]%d*)$")
  key = tonumber(key)
  if not space or key > LAST_KEY then
    return "home"
  end
  return space, key
end

-- The space the focused workspace belongs to, or nil in `home`.
local function active_space()
  local space = locate(hl.get_active_workspace())
  return space ~= "home" and space or nil
end

-- Which block of ten is a monitor's: Omarchy keeps it with the physical one.
local function block_of(monitor)
  if o.workspaces and o.workspaces.workspace_id then
    return (o.workspaces.workspace_id(monitor, 1) - 1) // SLOTS
  end
  return monitor.id
end

local function monitors()
  local shown = {}
  for _, monitor in ipairs(hl.get_monitors()) do
    if not monitor.is_mirror then
      shown[#shown + 1] = monitor
    end
  end
  return shown
end

local function tab_name(space, key)
  return space == "home" and tostring(key) or space .. ":" .. key
end

-- Hyprland's selector for a workspace by its name, as a new one is made by
-- it. For one that exists, use `target`: a selector like this matches the
-- workspace's address, the name it was made with, and a renamed one (`space
-- renumber` renames tabs) keeps that.
local function selector(name)
  return name:match("^%d+$") and name or "name:" .. name
end

-- The workspace that has the name now, whatever its address.
local function workspace_named(name)
  for _, workspace in ipairs(hl.get_workspaces()) do
    if workspace.name == name then
      return workspace
    end
  end
end

-- What to dispatch to reach the workspace called `name`: its selector, if that
-- finds it, else the workspace itself (a renamed one on 0.57), else the
-- selector that makes it. Hyprland 0.56 takes no workspace in a dispatch: it
-- goes to workspace 1 instead.
local function target(name)
  local found = hl.get_workspace(selector(name))
  if found and found.name == name then
    return selector(name)
  end
  return workspace_named(name) or selector(name)
end

-- Whether a new workspace can be made called `name`: none has the name, and
-- none a renamed one made by it has as its address.
local function name_is_free(name)
  return not workspace_named(name) and not hl.get_workspace(selector(name))
end

local function move_to_monitor(workspace, monitor)
  hl.dispatch(hl.dsp.workspace.move({ workspace = target(workspace.name), monitor = monitor.name }))
end

-- The tab of a space to show on a monitor: the one with the window you last
-- used there, else its first free one, yet to be made.
local function tab_on(monitor, space)
  local best, best_focus, best_key = nil, nil, nil
  for _, workspace in ipairs(hl.get_workspaces()) do
    local at, key = locate(workspace)
    if at == space and workspace.monitor and workspace.monitor.name == monitor.name then
      key = key or workspace.id or math.huge
      local focus = math.huge
      for _, window in ipairs(workspace:get_windows()) do
        local id = window.focus_history_id
        if id and id >= 0 and id < focus then
          focus = id
        end
      end
      if not best or focus < best_focus or (focus == best_focus and key < best_key) then
        best, best_focus, best_key = workspace.name, focus, key
      end
    end
  end
  if best then
    return best
  end
  local first = block_of(monitor) * SLOTS + 1
  for key = first, first + SLOTS - 1 do
    if name_is_free(tab_name(space, key)) then
      return tab_name(space, key)
    end
  end
  return tab_name(space, first)
end

-- Shows a workspace on the monitor it's on, or makes it there, leaving the pointer
-- and what has focus as they are. It goes there and back: Hyprland makes a
-- workspace only on the focused monitor, and showing one that has windows
-- (`set_workspace`) would give them the keyboard.
local function show(monitor, name)
  local focused = hl.get_active_monitor()
  local no_warps = hl.get_config("cursor.no_warps")
  hl.config({ cursor = { no_warps = true } })
  local ok, err = pcall(function()
    if not focused or focused.name ~= monitor.name then
      hl.dispatch(hl.dsp.focus({ monitor = monitor.name }))
    end
    hl.dispatch(hl.dsp.focus({ workspace = target(name) }))
    if focused and focused.name ~= monitor.name then
      hl.dispatch(hl.dsp.focus({ monitor = focused.name }))
    end
  end)
  hl.config({ cursor = { no_warps = no_warps } })
  if not ok then
    error(err)
  end
end

-- The space every monitor shows, and whether what happens now is our own
-- doing, or Omarchy's, rather than you going to another.
local current = locate(hl.get_active_workspace and hl.get_active_workspace())
local quiet = false

local function quietly(fn, ...)
  local was = quiet
  quiet = true
  local ok, err = pcall(fn, ...)
  quiet = was
  return ok, err
end

-- Brings the monitors that show another space to this one. Those already on
-- it keep the tab they show.
local function follow()
  if not current then
    return
  end
  quietly(function()
    for _, monitor in ipairs(monitors()) do
      local at = locate(monitor.active_workspace)
      if at and at ~= current then
        show(monitor, tab_on(monitor, current))
      end
    end
  end)
end

-- Going to a workspace, however you do, is what changes the space. Moving
-- between monitors isn't: the workspace you find there is already one of it.
hl.on("workspace.active", function(workspace)
  local space = locate(workspace)
  if quiet or not space then
    return
  end
  local focused = hl.get_active_monitor()
  local shown = focused and focused.active_workspace
  if shown and shown.name == workspace.name then
    if space ~= current then
      current = space
      follow()
    end
  elseif space ~= current then
    -- Another monitor was put on another space while you were elsewhere. Bring
    -- it back once whatever did that is done: it may be about to take you
    -- there, or the monitor may only just be arriving.
    hl.timer(follow, { timeout = 1, type = "oneshot" })
  end
end)

-- Puts each tab on the monitor whose it is, those on view elsewhere too if
-- `shown`. One whose monitor isn't there stays where it is.
local function reclaim(shown)
  local owners, on_view = {}, {}
  for _, monitor in ipairs(monitors()) do
    owners[block_of(monitor)] = monitor
    if monitor.active_workspace then
      on_view[monitor.active_workspace.name] = true
    end
  end
  for _, workspace in ipairs(hl.get_workspaces()) do
    local space, key = locate(workspace)
    local owner = space and key and owners[(key - 1) // SLOTS]
    if owner and workspace.monitor and workspace.monitor.name ~= owner.name
      and (shown or not on_view[workspace.name]) then
      move_to_monitor(workspace, owner)
    end
  end
end

-- A space's tabs are numbered 1, 2, 3, ... on each monitor, as a browser's
-- are: one that ends, however it does, takes its number with it, and `space
-- renumber` moves the ones after it up. It runs as any workspace but one of
-- `home`'s comes or goes, so a gap closes whatever left it. `home`'s tabs are
-- Omarchy's workspaces and keep their numbers. A workspace that's gone may
-- not say what it was.
local function renumber(workspace)
  local ok, space = pcall(locate, workspace)
  if ok and space == "home" then
    return
  end
  hl.dispatch(hl.dsp.exec_cmd("space renumber"))
end

hl.on("workspace.removed", renumber)

-- A tab opens on the monitor you are on as its first window shows up, which
-- needn't be the one you opened it on by then.
hl.on("workspace.created", function(workspace)
  local space = locate(workspace)
  if space and space ~= "home" then
    hl.timer(function()
      quietly(reclaim, false)
    end, { timeout = 1, type = "oneshot" })
    renumber(workspace)
  end
end)

-- As monitors come and go and the config reloads, Omarchy
-- (default/hypr/monitor-workspaces.lua) puts every monitor on a numbered
-- workspace, which would drop you out of your space. Let it, then show the
-- space again, on the tabs it was on.
local reconcile = o.workspaces and o.workspaces.reconcile
if reconcile then
  o.workspaces.reconcile = function(...)
    local space = current
    local tabs, focused = {}, hl.get_active_monitor()
    for _, monitor in ipairs(monitors()) do
      if monitor.active_workspace and locate(monitor.active_workspace) == space then
        tabs[monitor.name] = monitor.active_workspace.name
      end
    end
    focused = focused and focused.name

    local ok, err = quietly(reconcile, ...)
    quietly(function()
      -- A monitor that's back has its tabs back, which an unplug left elsewhere.
      reclaim(true)
      if not space or space == "home" then
        return
      end
      for _, monitor in ipairs(monitors()) do
        local tab = tabs[monitor.name]
        local there = tab and workspace_named(tab)
        if there and there.monitor and there.monitor.name ~= monitor.name then
          tab = nil
        end
        show(monitor, tab or tab_on(monitor, space))
      end
      if focused and hl.get_monitor(focused) then
        hl.dispatch(hl.dsp.focus({ monitor = focused }))
      end
    end)
    current = locate(hl.get_active_workspace())
    if not ok then
      error(err)
    end
  end
end

-- The tab that shows as a slot on the focused monitor: one of its own, or a
-- guest, another monitor's left here as it was unplugged. Guests take the
-- lowest slots free, in order, as `space` numbers them (src/monitors.rs).
-- Past the last tab, it's a new one after it, as tabs have no gaps.
local function tab_here(space, slot)
  local monitor = hl.get_active_monitor()
  local block = monitor and block_of(monitor) or 0
  local own, guests = {}, {}
  for _, workspace in ipairs(hl.get_workspaces()) do
    local at, key = locate(workspace)
    if at == space and key and monitor and workspace.monitor and workspace.monitor.name == monitor.name then
      if (key - 1) // SLOTS == block then
        own[(key - 1) % SLOTS + 1] = true
      else
        guests[#guests + 1] = key
      end
    end
  end
  local count = #guests
  for _ in pairs(own) do
    count = count + 1
  end
  slot = math.min(slot, count + 1)
  if not own[slot] then
    table.sort(guests)
    local free = 0
    for n = 1, slot do
      free = free + (own[n] and 0 or 1)
    end
    if guests[free] then
      return target(tab_name(space, guests[free]))
    end
  end

  local first = block * SLOTS + 1
  local name = tab_name(space, first + slot - 1)
  local workspace = workspace_named(name)
  if workspace then
    if workspace.monitor and monitor and workspace.monitor.name ~= monitor.name then
      move_to_monitor(workspace, monitor)
    end
    return target(name)
  end
  -- A new tab. The name it'd have may be a renamed tab's address, which it
  -- would find rather than make: take the next one free, and `space renumber`
  -- numbers it as the next tab.
  local key = first + slot - 1
  while key < first + SLOTS - 1 and not name_is_free(tab_name(space, key)) do
    key = key + 1
  end
  return selector(tab_name(space, key))
end

local function focus(slot)
  local omarchy = o.workspaces.focus(slot)
  return function()
    local space = active_space()
    if not space then
      return omarchy()
    end
    hl.dispatch(hl.dsp.focus({ workspace = tab_here(space, slot) }))
  end
end

local function move_window(slot, follow_it)
  local omarchy = o.workspaces.move_window(slot, follow_it)
  return function()
    local space = active_space()
    if not space then
      return omarchy()
    end
    if hl.get_active_window() then
      hl.dispatch(hl.dsp.window.move({ workspace = tab_here(space, slot), follow = follow_it ~= false }))
    end
  end
end

local PLUGIN = "com.t4t5.spaces"

-- Runs a `space` command.
local function run(command)
  return function()
    return "space " .. command
  end
end

-- Calls the plugin.
local function shell_call(action)
  return function()
    return "omarchy-shell shell call " .. PLUGIN .. " " .. action .. " ''"
  end
end

local next_tab, prev_tab = run("tab next"), run("tab prev")

-- Omarchy's workspace keys (default/hypr/bindings/tiling.lua), for tabs. Its
-- own next and previous go through every space's, as they are all workspaces.
local function bind_tab_keys()
  for n = 1, SLOTS do
    local key = "code:" .. (n + 9)
    o.rebind("SUPER + " .. key, "Go to tab " .. n, focus(n))
    o.rebind("SUPER + SHIFT + " .. key, "Move window to tab " .. n, move_window(n))
    o.rebind("SUPER + SHIFT + ALT + " .. key, "Move window silently to tab " .. n, move_window(n, false))
  end
  o.rebind("SUPER + TAB", "Next tab", next_tab())
  o.rebind("SUPER + SHIFT + TAB", "Previous tab", prev_tab())
end

local function find_space()
  return { panel = PLUGIN }
end

-- With no space to send it to, you pick one.
local pick_for_tab = shell_call("sendTab")
local function send_tab(to)
  if not to then
    return pick_for_tab()
  end
  return run("tab send")() .. " " .. o.shell_quote(to)
end

-- With no space to send it to, you pick one.
local pick_for_window = shell_call("sendWindow")
local function send_window(to, follow_it)
  if not to then
    if follow_it == false then
      return "omarchy-shell shell call " .. PLUGIN .. " sendWindow 'silent'"
    end
    return pick_for_window()
  end
  return run("window send")() .. " " .. o.shell_quote(to) .. (follow_it == false and " --silent" or "")
end

-- Never without a terminal, even if `space` is missing.
local function terminal()
  return run("terminal")() .. " || omarchy-launch-terminal"
end

-- `sync` is for `space`, when it renames the workspace you are on into
-- another space: nothing else says that happened.
local function sync()
  current = locate(hl.get_active_workspace()) or current
  follow()
end

-- `show` too: the tab a monitor you aren't on goes on to.
local function show_tab(name)
  local workspace = workspace_named(name)
  if workspace and workspace.monitor then
    quietly(show, workspace.monitor, name)
  end
end

local M = {
  toggle_sidebar = shell_call("toggleSidebar"),
  find_space = find_space,

  new_space = run("new"),
  next_space = run("next"),
  prev_space = run("prev"),
  rename_space = shell_call("renameSpace"),

  new_tab = run("tab new"),
  next_tab = next_tab,
  prev_tab = prev_tab,
  bind_tab_keys = bind_tab_keys,
  go_to_tab = focus,
  rename_tab = shell_call("renameTab"),
  send_tab = send_tab,

  send_window = send_window,
  move_window_to_tab = move_window,

  terminal = terminal,

  -- For `space` alone (src/hypr.rs).
  internal = { sync = sync, show = show_tab },
}

-- A tab emptied by closing its last window doesn't linger, however the
-- window went (super+w, `exit` in a terminal, an app quitting): `space
-- emptied` has its monitor go on to another of the space's tabs there. With
-- none there, it stays on it, empty, or, with none on any monitor, the space
-- ends and you go back to where you were before it. Only a close gets
-- here, so an empty workspace you go to yourself stays. The closing window
-- still counts until the next loop, so look then.
hl.on("window.close", function(window)
  local name = window and window.workspace and window.workspace.name
  local space = locate(window and window.workspace)
  if not space or space == "home" then
    return
  end
  hl.timer(function()
    for _, monitor in ipairs(monitors()) do
      local shown = monitor.active_workspace
      if shown and shown.name == name and shown.windows == 0 then
        hl.dispatch(hl.dsp.exec_cmd("space emptied " .. name))
      end
    end
  end, { timeout = 1, type = "oneshot" })
end)

-- A glassy bar (double-click it to cycle opaque, glassy, clear) has Hyprland
-- blur what is behind it. The sidebar follows the bar's look, so blur behind
-- it too. `omarchy bar background` adds or removes this toggle file and
-- reloads Hyprland, which runs this again. With xray the blur takes in only
-- the wallpaper, not the windows next to the sidebar and the bar, which would
-- smear into their edges like a shadow; the bar gets it too.
local glassy = io.open(require("default.hypr.paths").state_home .. "/omarchy/toggles/hypr/bar-glassy.lua")
if glassy then
  glassy:close()
  hl.layer_rule({
    match = { namespace = "^(omarchy-spaces-base|omarchy-bar)$" },
    blur = true,
    xray = true,
    ignore_alpha = 0.3,
  })
end

return M
