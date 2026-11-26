# AGENTS.md

## Checking visual changes

After changing anything you can see (the QML in `omarchy/`), check it on screen before calling it done:

1. Run `just reload`. It restarts the Omarchy shell, which takes a few seconds. Reloading the shell's plugins isn't enough, because the sidebar lives in the plugin's service, which `keepLoaded` keeps through a plugin reload.
2. Take a screenshot with `grim` into your scratchpad, and look at it. `grim -g "0,0 700x450" <file>` captures the top left, where the sidebar and the tabs are.
3. To see the expanded sidebar, open it before the screenshot and close it after, so the screen is left as it was: `omarchy-shell shell call com.t4t5.spaces toggleSidebar ''`, then wait about a second for it to slide open.

## Commands that end or move tabs

A space's tabs are always numbered 1, 2, 3, … on each monitor: Hyprland runs `space renumber` as a space's workspace comes or goes, and it closes the gaps (`src/commands/tab.rs`). Two rules keep that right for a new command:

- A command that can end a tab (closing windows, moving them away, sending the tab elsewhere) holds `hold_tabs()` while it runs, so `renumber` waits for it rather than renumbering the tabs under it. The lock can't be taken twice in one process: a command that already holds it calls the `_held` version of another, as `send_window` calls `emptied_held`.
- A command that renames a tab out of a space calls `renumber_tabs` before it finishes, as `send_tab` does. A rename isn't a workspace going, so Hyprland doesn't run `renumber` for it.
