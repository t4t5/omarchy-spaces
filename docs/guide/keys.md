# Keys

Spaces binds no keys by itself. Bind the ones you want in `~/.config/hypr/bindings.lua`. The [README](../../README.md#install) has the two to start from:

```lua
if package.searchpath("hypr.omarchy-spaces", package.path) then
  local spaces = require("hypr.omarchy-spaces")

  spaces.bind_tab_keys()
  o.rebind("SUPER + B", "Show or hide spaces", spaces.toggle_sidebar())
end
```

The `if` skips them when Spaces isn't there, so that removing it gives you Omarchy's keys back. Put any others you want inside it too. These keys are free in Omarchy:

```lua
  o.rebind("SUPER + N", "New space", spaces.new_space())
  o.rebind("SUPER + SHIFT + J", "Next space", spaces.next_space())
  o.rebind("SUPER + SHIFT + K", "Previous space", spaces.prev_space())
```

`o.rebind` replaces whatever is on that key. To see what's taken:

```sh
omarchy menu keybindings --print
```

Bind these actions rather than `space` commands: they stay the same if the commands change.

## Spaces

| Action | Does |
|---|---|
| `spaces.toggle_sidebar()` | Expand or collapse the spaces sidebar |
| `spaces.find_space()` | Find a space, by its name or a window's title or app: type to filter, Enter to switch, Escape to stop. Expands the sidebar for the search if it's collapsed |
| `spaces.next_space()` / `spaces.prev_space()` | Next / previous space, down and up the sidebar. They wrap around, with `home` at the top |
| `spaces.new_space()` | New space, with a terminal in the folder you are in |
| `spaces.rename_space()` | Rename the space you are in: type over its name in the sidebar, Enter to keep it, Escape to stop. An empty name has it called after its folder again |

## Tabs

| Action | Does |
|---|---|
| `spaces.new_tab()` | New tab, with a terminal in the space's folder |
| `spaces.bind_tab_keys()` | Binds Omarchy's workspace keys to tabs, by itself: `super+1`–`0` to go to one, with `shift` to move the window there, with `shift+alt` to move it and stay where you are, and `super+tab` and `super+shift+tab` for the next and previous tab. For other keys, bind the actions below |
| `spaces.go_to_tab(n)` | Go to tab N, or past the last tab, to a new one after it |
| `spaces.next_tab()` / `spaces.prev_tab()` | Next / previous tab |
| `spaces.rename_tab()` | Name the tab you are on: type over its name on the bar, Enter to keep it, Escape to stop. An empty name has it called after its window again |
| `spaces.send_tab()` | Move the tab you are on to another space, and go with it: type to filter the spaces, Enter to pick, Escape to stop. It starts on the space you left last |
| `spaces.send_tab("next")` / `spaces.send_tab("prev")` | Move the tab you are on to the next / previous space, or to one by name, and go with it |

## Windows

| Action | Does |
|---|---|
| `spaces.terminal()` | Terminal in the folder you are in; on an empty tab, the space's folder. If that fails, Omarchy's own |
| `spaces.move_window_to_tab(n)` | Move the window to tab N, or past the last tab, to a new one after it; with `false` after it, staying where you are |
| `spaces.send_window()` | Move the focused window to another space: type to filter, press `1`–`0` for instant pick, Enter to send (Shift+Enter silently), Escape to stop. Pass `(nil, false)` to send silently by default |
| `spaces.send_window("next")` / `spaces.send_window("prev")` | Send the window to the next / previous space, or to one by name, and go with it; with `false` after it, staying where you are |

In `home`, `go_to_tab` and `move_window_to_tab` behave exactly as Omarchy's workspace keys, so a new tab there opens empty.
