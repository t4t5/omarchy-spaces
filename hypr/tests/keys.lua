-- The helpers for your keys, against stubs of Hyprland and Omarchy:
--   lua hypr/tests/keys.lua

local dispatched, omarchy = {}, {}
local active = { name = "1" }

-- `hl.dsp.window.move({ ... })` and the like, as { "window.move", { ... } }.
local function dispatcher(path)
  return setmetatable({}, {
    __index = function(_, key)
      return dispatcher(path and path .. "." .. key or key)
    end,
    __call = function(_, args)
      return { path, args }
    end,
  })
end

local monitor = { name = "eDP-1", id = 0 }
-- The workspaces there are, all on that monitor.
local workspaces = {}
local function tabs(space, count)
  for key = 1, count do
    workspaces[#workspaces + 1] = { name = space .. ":" .. key, monitor = monitor }
  end
end
hl = {
  on = function() end,
  dsp = dispatcher(),
  dispatch = function(action)
    dispatched[#dispatched + 1] = action
  end,
  get_active_workspace = function()
    return active
  end,
  get_active_monitor = function()
    return monitor
  end,
  get_active_window = function()
    return { address = "0x1" }
  end,
  get_monitors = function()
    return { monitor }
  end,
  get_workspaces = function()
    return workspaces
  end,
  get_workspace = function() end,
}
o = {
  shell_quote = function(value)
    return "'" .. tostring(value):gsub("'", "'\\''") .. "'"
  end,
  workspaces = {
    workspace_id = function(_, slot)
      return slot
    end,
    focus = function(slot)
      return function()
        omarchy[#omarchy + 1] = "focus " .. slot
      end
    end,
    move_window = function(slot, follow)
      return function()
        omarchy[#omarchy + 1] = "move " .. slot .. " " .. tostring(follow)
      end
    end,
    reconcile = function() end,
  },
}
package.preload["default.hypr.paths"] = function()
  return { state_home = "/nonexistent" }
end

local here = arg[0]:match("^(.*)/tests/[^/]*$") or ".."
local spaces = dofile(here .. "/omarchy-spaces.lua")

local function equal(got, want)
  if got ~= want then
    error("got " .. tostring(got) .. ", want " .. tostring(want), 2)
  end
end

-- `space`'s commands.
equal(spaces.new_space(), "space new")
equal(spaces.next_space(), "space next")
equal(spaces.prev_space(), "space prev")
equal(spaces.new_tab(), "space tab new")
equal(spaces.next_tab(), "space tab next")
equal(spaces.prev_tab(), "space tab prev")
equal(spaces.terminal(), "space terminal || omarchy-launch-terminal")
equal(spaces.send_window("next"), "space window send 'next'")
equal(spaces.send_window("prev", false), "space window send 'prev' --silent")
equal(spaces.send_window("it's", true), "space window send 'it'\\''s'")
equal(spaces.send_tab("next"), "space tab send 'next'")

-- The plugin's.
local call = "omarchy-shell shell call com.t4t5.spaces "
equal(spaces.send_window(), call .. "sendWindow ''")
equal(spaces.send_window(nil, false), call .. "sendWindow 'silent'")
equal(spaces.send_tab(), call .. "sendTab ''")
equal(spaces.toggle_sidebar(), call .. "toggleSidebar ''")
equal(spaces.rename_space(), call .. "renameSpace ''")
equal(spaces.rename_tab(), call .. "renameTab ''")
equal(spaces.find_space().panel, "com.t4t5.spaces")

-- In `home`, Omarchy's own.
spaces.go_to_tab(3)()
spaces.move_window_to_tab(4, false)()
equal(table.concat(omarchy, ", "), "focus 3, move 4 false")
equal(#dispatched, 0)

-- In a space, its tabs.
active = { name = "s1:1" }
tabs("s1", 10)
tabs("s12", 10)
spaces.go_to_tab(3)()
spaces.move_window_to_tab(4)()
spaces.move_window_to_tab(5, false)()
equal(#omarchy, 2)
equal(dispatched[1][1], "focus")
equal(dispatched[1][2].workspace, "name:s1:3")
equal(dispatched[2][1], "window.move")
equal(dispatched[2][2].workspace, "name:s1:4")
equal(dispatched[2][2].follow, true)
equal(dispatched[3][2].follow, false)

-- Omarchy's workspace keys, all of them.
local bound, count = {}, 0
o.rebind = function(keys, _, action)
  bound[keys] = action
  count = count + 1
end
spaces.bind_tab_keys()
equal(count, 32)
equal(bound["SUPER + TAB"], "space tab next")
equal(bound["SUPER + SHIFT + TAB"], "space tab prev")
bound["SUPER + code:10"]()
bound["SUPER + SHIFT + code:19"]()
bound["SUPER + SHIFT + ALT + code:11"]()
local silent, follow, go = table.remove(dispatched), table.remove(dispatched), table.remove(dispatched)
equal(go[2].workspace, "name:s1:1")
equal(follow[2].workspace, "name:s1:10")
equal(follow[2].follow, true)
equal(silent[2].workspace, "name:s1:2")
equal(silent[2].follow, false)

-- A workspace something else named is `home`'s, as Omarchy has it.
for _, name in ipairs({ "web:1", "s0:1", "s1:0", "s1:251", "S1:1", "home:1" }) do
  active = { name = name }
  spaces.go_to_tab(6)()
  equal(table.remove(omarchy), "focus 6")
end
equal(#dispatched, 3)
active = { name = "s12:13" }
spaces.go_to_tab(6)()
equal(dispatched[4][2].workspace, "name:s12:6")

-- Past the last tab, a new one after it: tabs have no gaps.
workspaces = {}
tabs("s1", 2)
active = { name = "s1:2" }
spaces.go_to_tab(6)()
spaces.move_window_to_tab(9)()
equal(dispatched[5][2].workspace, "name:s1:3")
equal(dispatched[6][2].workspace, "name:s1:3")
spaces.go_to_tab(2)()
equal(dispatched[7][2].workspace, "name:s1:2")

-- What `space` itself calls (src/hypr.rs).
equal(type(spaces.internal.sync), "function")
equal(type(spaces.internal.show), "function")

-- The docs' Keys has every helper, and neither it nor the README has others.
local function documented(path)
  local file = assert(io.open(here .. "/../" .. path))
  local text = file:read("a")
  file:close()
  local names = {}
  for name in text:gmatch("spaces%.([a-z_]+)%(") do
    assert(spaces[name], path .. " has spaces." .. name .. ", which isn't there")
    names[name] = true
  end
  return names
end
documented("README.md")
local keys = documented("docs/guide/keys.md")
for name in pairs(spaces) do
  assert(name == "internal" or keys[name], "docs/guide/keys.md doesn't have spaces." .. name)
end

print("ok")
