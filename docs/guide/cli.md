# Command line

Everything you can do with the `space` command. `space help` lists the commands too.

## Spaces

```sh
space new [dir]              # open a space with a terminal, in the folder you are in or dir
space go <space>             # go to a space where you left it
space next                   # the space below the active one, wrapping around
space prev                   # the space above it
space rename [space] <name>  # name a space, the active one by default; "" for its folder's name
space icon [space] <icon>    # a glyph, emoji, or image file for a space's badge, the active one by default; "" for its letters
space reorder <space> <n>    # move a space to position n in the sidebar, 1 being the first below home
space close [space]          # close a space's windows, the active one by default
```

## Tabs

```sh
space tab new                           # open a tab in the active space, with a terminal in its folder
space tab go <n>                        # go to tab n of the active space on this monitor, or to a tab by its id
space tab next                          # the active space's next tab, wrapping around
space tab prev                          # its previous tab
space tab rename [tab] <name>           # name a tab, the one you are on by default; "" for its window's name
space tab reorder <tab> <n>             # move a tab to position n among its space's tabs
space tab close [tab]                   # close a tab's windows, the one you are on by default, and leave it if you're on it
space tab send [tab] <space|next|prev>  # a tab, the one you are on by default, to another space
```

## Windows

```sh
space window send <space|next|prev> [--silent]  # the window to another space, on the tab you left it on
space window tab <n> [--silent]                 # the window to tab n of the active space on this monitor
```

## The rest

```sh
space ls [--json]            # list spaces with window counts; --json, for scripts, with their tabs
space restore                # open the spaces from before the last restart again
space restore <file>         # or those of an earlier session
space terminal               # a terminal in the folder you are in, or the space's
space watch [--sounds]       # what the plugin runs, and for it alone; --sounds also rings for agents
```

## Names and ids

A space can be given by what it's called or by its id (`s1`, as `space ls` shows), which stays with it across restarts and is never another's.

A tab is given by its id, as `space ls --json` shows it. `space tab go 2` goes to the tab that shows as 2 on the monitor you are on.

## JSON for scripts

`space ls --json` prints `{ version, spaces }`:

- A space has its `tabs`, each with its `id`, for commands, and its `slot`, the number it shows on its `monitor` (`null` for none).
- A terminal's `tab` is its tab's id.
- `status` is `working`, `waiting` or `null`. Only the plugin, which follows agents, can tell that one is done.
- `version` goes up only when a change would break a script. New fields can come without.

Pass a tab's id as it is rather than reading or making one up: what it's made of can change, and moving a tab gives it another.

`space watch` promises nothing of the kind: what it prints is for the plugin, and changes with it.
