# Command line

Everything you can do with the `space` command. `space help` lists the commands too.

## Spaces

```sh
space new [dir]              # open a space with a terminal, in the folder you are in or dir
space go <space>             # go to a space where you left it
space next                   # the space below the active one, wrapping around
space prev                   # the space above it
space rename [space] <name>  # name a space; "" for its folder's name
space icon [space] <icon>    # a glyph, emoji, or image file for a space's badge; "" for its letters
space reorder <space> <n>    # move a space to position n in the sidebar, 1 being the first below home
space close [space]          # close a space's windows
space current                # the space a command run here acts on
```

A space left out is the one the command is run from (see [Where a command acts](#where-a-command-acts)).

`space new` takes `--name <name>` to name the space right away, `--silent` to open it without going there, `--json` to print its id, and `-- <cmd...>` to run a command in its terminal (returning to your shell when it exits).

## Tabs

```sh
space tab new                           # open a tab, with a terminal in its space's folder
space tab go <n>                        # go to tab n of the active space on this monitor, or to a tab by its id
space tab next                          # the active space's next tab, wrapping around
space tab prev                          # its previous tab
space tab rename [tab] <name>           # name a tab; "" for its window's name
space tab reorder <tab> <n>             # move a tab to position n among its space's tabs
space tab close [tab]                   # close a tab's windows, and leave it if you're on it
space tab send [tab] <space|next|prev>  # a tab to another space
```

A tab left out is the one the command is run from.

`space tab new` takes `-s <space>` to open it in another space, `--dir <dir>` to open its terminal there, and `--name`, `--silent`, `--json` and `-- <cmd...>` as `space new` does.

## Windows

```sh
space window send <space|next|prev> [--silent]  # the window to another space, on the tab you left it on
space window tab <n> [--silent]                 # the window to tab n of the active space on this monitor
```

## Data of your own

A space can hold data for your scripts and agents: any JSON, by key. It shows in `space ls --json` and `space events`, and comes back after a restart. Spaces itself never reads it.

```sh
space meta get [key]          # all of a space's data, or one key's
space meta set <key> <value>  # set a key to a string, or to JSON with --json
space meta rm <key>           # remove a key
```

Each takes `-s <space>` for a space other than the one it's run from.

## The rest

```sh
space ls [--json]            # list spaces with window counts; --json, for scripts, with all there is
space events                 # a line of JSON each time something changes, for scripts
space restore                # open the spaces from before the last restart again
space restore <file>         # or those of an earlier session
space terminal [-s <space>] [-- <cmd...>]  # a terminal in the folder you are in, or a space's; optionally runs a command
space watch [--sounds]       # what the plugin runs, and for it alone; --sounds also rings for agents
```

## Names and ids

A space can be given by what it's called or by its id (`s1`, as `space ls` shows), which stays with it across restarts and is never another's.

A tab is given by its id, as `space ls --json` shows it. `space tab go 2` goes to the tab that shows as 2 on the monitor you are on. In a space, a number past the last tab goes to a new one after it, as tabs have no gaps.

## Where a command acts

A command given no space or tab acts on the one it's run from: the window it runs in. So an agent working in another space names or closes its own space, not the one you're looking at. Run from a key binding or the launcher, which aren't in a window, it acts where you are.

A terminal that Spaces opens has `SPACE_ID` set to its space, for when there's no window to go by, as in tmux. That gives the space but not the tab, so there a tab command needs the tab's id.

`space current` prints the space a command would act on. `--json` adds its tab and window, and where you are.

## JSON for scripts

`space ls --json` prints `{ version, spaces }`:

- A space has its `tabs`, each with its `id`, for commands, and its `slot`, the number it shows on its `monitor` (`null` for none). It has its `meta` too.
- A terminal's `tab` is its tab's id.
- `clients` is every window a space has, `home`'s too, by tab and then address, each with its `address`, its `tab` (the id, `null` on a workspace without a number), its `class` (the app) and its `title`, as it is now. Windows on special workspaces aren't listed.
- `status` is `working`, `waiting` or `null`. Only `space watch`, which follows agents, can tell that one is done, so `space events` says so but `space ls` can't.
- `version` goes up only when a change would break a script. New fields can come without.

Pass a tab's id as it is rather than reading or making one up: what it's made of can change, and moving a tab gives it another.

## Events

`space events` prints a line of JSON each time something changes. The first is a `snapshot` of it all: where you are, as `focused`, and the `spaces`, as `space ls --json` prints them. Each line after it is one change, by its `type`:

- `space.created`, `space.removed`
- `space.focused`: you went to another space
- `space.changed`, with what did, as `changed`: its `meta` or its `status`, say
- `tab.created`, `tab.removed`, `tab.changed`
- `status.changed`: an agent in a terminal is working, waiting or done, or no longer

`--type status.changed,space.focused` prints only those, and `--no-snapshot` leaves the snapshot out.

The `snapshot` and the `space.*` events carry each space's `clients`, as `space ls --json` has them.

The events come from `space watch`, which the plugin runs, so they say what the sidebar shows. Without it, `space events` waits for it. When it restarts, as it does after an update, another snapshot comes, as things may have changed in between.

Each line has a `seq`. The changes count up by one from the snapshot's, and a new watch starts again from 0.

A tab that moves, or is numbered again, gets another id, so it shows as one tab removed and another created.

`space watch` prints what the plugin reads, for it alone: it changes with the plugin.
