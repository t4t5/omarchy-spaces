# Architecture

Spaces has three parts: a command in Rust, an Omarchy shell plugin in QML, and a piece of Hyprland config in Lua.

## `src/`: the `space` command

It keeps no state about windows: it asks Hyprland over its socket, then dispatches.

- **Terminals**: their folders and foreground programs come from their shells in `/proc`, as Omarchy's `omarchy-cmd-terminal-cwd` does, plus directly launched agent sessions (like `omarchy agent`). A shell terminal's program is the leader of the foreground process group on its shell's tty.
- **Agents**: what an agent is doing, and the conversation it's in, come from its adapter in `src/agent/`, which reads what the agent itself keeps on disk, so none needs hooks.
- **Browsers**: what browser windows hold comes from an adapter in `src/browser/`. Chromium's reads the session files that it and those made from it keep open in their profile, matching them to Hyprland's windows by title, or the order they were opened in.

Nothing else knows of any one agent or browser, so another is a new file there.

Windows of one process (Ghostty's that you open yourself, `foot --server`'s) are told apart by the order they and their shells were opened in, as long as each has one shell. With tabs or splits in them, some show the newest shell's folder.

## `omarchy/`: the shell plugin

The tabs are a bar widget (`TabsWidget.qml`), and `Base.qml` draws the sidebar under the windows. Their logic is in `Controller.qml`, `Sidebar.qml`, `SidebarList.qml` and `Spaces.mjs`.

Both draw what one `space watch` says (`src/view.rs`). The plugin's service runs it (`Watch.qml`) and restarts it when it ends, as it does when Hyprland restarts or `space` is reinstalled. Each line is all of it as of one moment: where you are, each monitor's tabs and the spaces. The plugin binds to the last one, and neither copies it nor asks Hyprland where you are as well, as the two would come apart.

The same watch serves `space events` (`src/events.rs`), on `$XDG_RUNTIME_DIR/omarchy-spaces/events.sock`. It sends each subscriber a snapshot when it joins, then the changes between one moment and the next. It's the only one that follows which agents are done and seen, so the events and the sidebar can't disagree. A subscriber that falls far behind is dropped rather than holding the watch up. The watch that serves holds `.events.lock` beside the socket. Another one, run beside the plugin's or started by the plugin before the last has ended, tries for the lock every two seconds and takes over once it gets it.

### When `space watch` reads again

| What | When |
|---|---|
| Hyprland | At once when you go somewhere, when Hyprland's events say something changed, and every two seconds, as nothing says when a window is resized |
| The active space's terminals' shells | Five times a second, as a shell changing folder or starting a program sends no event |
| The other spaces' shells | Every two seconds |
| All shells | When a window's title changes, as terminals retitle themselves on `cd` and as a command starts |
| Windows' titles, for the search | With Hyprland's state, so a new title shows within two seconds; the line is printed again only if it differs |

## `hypr/omarchy-spaces.lua`: the Hyprland part

It holds:

- the keybind helpers (tab actions and plugin calls),
- the hook that takes the other monitors along when you go to another space,
- the hook that moves on from a tab emptied by closing its last window (`space emptied`),
- the hook that numbers each space's tabs 1, 2, 3, … again as a workspace comes or goes (`space renumber`); the commands that can end a tab hold `.tabs.lock` while they run, so it waits for them,
- the blur behind a glassy sidebar and bar.

A tab is found by its name, as the bar shows it, never by a `name:` selector made from it. Hyprland (0.57) gives a workspace the name it was made with as its address, which `name:` matches, and a rename (`space renumber`) changes the name only, so after one `name:s1:1` can mean another tab, or none. `src/hypr.rs`'s `selector_for` and the Lua part's `target` look the name up and aim at the workspace that has it. A new tab never takes a number that is a renamed tab's address, for the same reason: `space renumber` numbers it as the next tab.

## State

`~/.local/state/omarchy-spaces/spaces.json` holds what Hyprland can't about each live space: the name, icon and metadata you gave it, the terminal it follows and that terminal's last folder. The saved sessions are beside it (see [Restoring your spaces](../guide/restore.md#whats-saved)); `space watch` saves them as they change, and after a restart the plugin runs `space restore`.

All of these say their `version`. One that can't be read, or that a newer `space` made, is put aside as `<name>.broken-<time>` beside it rather than saved over.
