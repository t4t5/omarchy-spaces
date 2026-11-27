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

When a space or tab is omitted from a mutating command (`rename`, `icon`, `note`, `meta`, `close`, `tab new`, `tab rename`, `tab close`, `tab send`), `space` resolves where it was invoked from by walking `/proc` to find its live Hyprland window (falling back to the `SPACE_ID` environment variable that `space` injects into launched terminals, and finally to the focused workspace). This means keybinds and interactive terminals act where you are focused, while an agent or background script running in another space acts on its own space instead of hijacking focus. Use `space current --json` to inspect both the caller's location and the focused workspace.

## JSON for scripts

`space ls --json` prints `{ version, spaces }`:

- A space has its `note`, `meta`, `tabs` (each with its `id`, for commands, and its `slot`, the number it shows on its `monitor`, or `null` for none), and `terminals`.
- A terminal's `tab` is its tab's id.
- `status` is `working`, `waiting` or `null`. Live `done` transitions are tracked by `space events` and the plugin.
- `version` goes up only when a change would break a script. New fields can come without.

Pass a tab's id as it is rather than reading or making one up: what it's made of can change, and moving a tab gives it another.

## Events stream

`space events` is a public, read-only NDJSON event stream that never writes `spaces.json` or `session.json`, so any number of scripts or tools can subscribe at once. Each line has `{ version, seq, type, ... }`:

- `snapshot` — emitted first (unless `--no-snapshot`), carrying `focused` (`{ space, tab }`) and `spaces` in the same schema as `space ls --json`.
- `space.created` / `space.removed` — a space appeared or disappeared.
- `space.focused` — focus moved to another space (`space`, `previous`, `tab`).
- `space.changed` — a space's fields changed (`space`, `changed`: list of changed field names such as `label`, `note`, `meta`, `status`).
- `tab.created` / `tab.removed` / `tab.changed` — a tab came, went, or changed (`space`, `tab`, and `changed` for updates).
- `status.changed` — an agent's status transitioned in a terminal (`space`, `tab`, `address`, `program`, `status`, `previous`).

Filter by comma-separated event types with `--type space.created,status.changed`. `space watch` remains private to the Omarchy shell plugin and changes with it.
