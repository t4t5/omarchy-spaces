# Command line

Everything you can do with the `space` command. `space help` lists the commands too.

## Spaces

```sh
space new [dir] [--name <name>] [--note <text>] [--silent] [--json]  # open a space with a terminal, in dir or caller's folder
space current [--json]                                               # show the space this command was called from
space go <space>                                                     # go to a space where you left it
space next                                                           # the space below the active one, wrapping around
space prev                                                           # the space above it
space rename [space] <name>                                          # name a space, the caller's or active one by default; "" for its folder's name
space icon [space] <icon>                                            # a glyph, emoji, or image file for a space's badge; "" for its letters
space note [space] <text>                                            # attach a short one-line note to a space; "" clears it
space reorder <space> <n>                                            # move a space to position n in the sidebar, 1 being the first below home
space close [space]                                                  # close a space's windows, the caller's or active one by default
```

## Metadata

Attach opaque JSON metadata to any space for external scripts, hooks, or agents. Metadata is persisted in `spaces.json`, exposed in `space ls --json` and `space events`, and preserved across `space restore`.

```sh
space meta get [-s <space>] [key]                  # print all metadata for a space, or one key's JSON value
space meta set [-s <space>] <key> <value> [--json] # set a metadata key (plain string unless --json is given)
space meta rm  [-s <space>] <key>                  # remove a metadata key
```

## Tabs

```sh
space tab new [-s <space>] [--dir <dir>] [--name <name>] [--silent] [--json]  # open a tab in the target space
space tab go <n>                                                              # go to tab n of the active space on this monitor, or to a tab by its id
space tab next                                                                # the active space's next tab, wrapping around
space tab prev                                                                # its previous tab
space tab rename [tab] <name>                                                 # name a tab, the caller's or current one by default; "" for its window's name
space tab reorder <tab> <n>                                                   # move a tab to position n among its space's tabs
space tab close [tab]                                                         # close a tab's windows, the caller's or current one by default
space tab send [tab] <space|next|prev>                                        # move a tab to another space
```

## Windows

```sh
space window send <space|next|prev> [--silent]  # the window to another space, on the tab you left it on
space window tab <n> [--silent]                 # the window to tab n of the active space on this monitor
```

## The rest

```sh
space ls [--json]                                  # list spaces with window counts; --json, for scripts, with their tabs, note and meta
space events [--type <types>] [--no-snapshot]      # stream NDJSON events for scripts, hooks, and agents (read-only)
space restore                                      # open the spaces from before the last restart again
space restore <file>                               # or those of an earlier session
space terminal [-s <space>]                        # a terminal in the caller's folder, or the target space's
space watch [--sounds]                             # what the plugin runs, and for it alone; --sounds also rings for agents
```

## Names, ids and caller context

A space can be given by what it's called or by its id (`s1`, as `space ls` shows), which stays with it across restarts and is never another's.

A tab is given by its id, as `space ls --json` shows it. `space tab go 2` goes to the tab that shows as 2 on the monitor you are on. In a space, a number past the last tab goes to a new one after it, as tabs have no gaps.

A command given no space or tab (`rename`, `icon`, `note`, `meta`, `close`, `tab new`, `tab rename`, `tab close`, `tab send`) acts on the one it was run from: the space and tab of the window it was run in, found through `/proc`. Run from a key binding or the launcher, which aren't in a window, it acts where you are. So an agent working in another space changes its own space, not the one you're looking at.

A terminal that Spaces opens has `SPACE_ID` set to its space, for when there's no window to go by, as in tmux. That tells the space but not the tab, so there a tab command needs the tab's id rather than acting on the tab you're on. `space current --json` shows where a command would act, and where you are.

## JSON for scripts

`space ls --json` prints `{ version, spaces }`:

- A space has its `note`, `meta`, `tabs` (each with its `id`, for commands, and its `slot`, the number it shows on its `monitor`, or `null` for none), and `terminals`.
- A terminal's `tab` is its tab's id.
- `status` is `working`, `waiting` or `null`. Only `space watch`, which follows agents, can tell that one is done: `space events` says so.
- `version` goes up only when a change would break a script. New fields can come without.

Pass a tab's id as it is rather than reading or making one up: what it's made of can change, and moving a tab gives it another.

## Events

`space events` prints a line of JSON each time something changes, for scripts to follow. Each line has its `version`, `seq` and `type`:

- `snapshot` comes first, with `focused` (`{ space, tab }`) and `spaces`, as `space ls --json` prints them. `--no-snapshot` leaves it out.
- `space.created` and `space.removed`: a space came or went.
- `space.focused`: you went to another space (`space`, `previous`, `tab`).
- `space.changed`: something about a space changed (`space`, and `changed`, the fields that did, like `label`, `note`, `meta` or `status`).
- `tab.created`, `tab.removed` and `tab.changed`: a tab came, went, or changed (`space`, `tab`, and `changed`).
- `status.changed`: an agent in a terminal started or stopped working, waiting or being done (`space`, `tab`, `address`, `program`, `status`, `previous`).

`--type space.created,status.changed` prints only those types.

The events come from `space watch`, which the plugin runs, so they say what the sidebar shows, `done` included. Without a watch, `space events` waits for one. When the watch restarts, as it does when `space` is updated, another `snapshot` comes, as things may have changed in between.

A snapshot's `seq` is that of the last change it takes in, and each change after it counts up by one, so a line `--type` leaves out shows as a gap. A new watch counts from 0 again.

A tab that moves, or is numbered again when one before it closes, gets another id, so it shows as one tab removed and another created.

`space watch` prints what the plugin reads, for it alone: that changes with the plugin.
