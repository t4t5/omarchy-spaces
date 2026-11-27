# Restoring your spaces

After you log out or restart, your spaces open again by themselves, `home` included, and you're back in the space you were in, unless you've started on something else.

## What comes back

- **Spaces and tabs**: their ids, names, icons, notes, metadata and tab names.
- **Terminals**: each in its folder and arranged as it was (splits and their sizes, groups, floating and fullscreen windows, monitors).
- **Agents**: a terminal that was running [one Spaces knows](agent-status.md#which-agents-show) picks its conversation up again, or starts a new one if there was nothing in it yet.
- **Browser windows and web apps** of Chromium, Chrome, Brave and Edge, with their tabs, each in the browser it was in. The tab you were on comes first, and the tabs load anew, without their history, groups or pins.

## What doesn't

- Other programs in the terminals, and what they had printed.
- Other apps' windows. The windows beside one take its room.
- Firefox and Zen, for now.
- Incognito windows, and windows of a browser on another data folder.

If the browser is set to continue where you left off, its windows are left to it. Splits are restored in Hyprland's dwindle layout; in others, windows come back in the order they were opened.

An app you have Hyprland start when you log in (`~/.config/hypr/autostart.lua`) opens twice if Spaces opens it again too: a terminal or a browser.

Each part can be turned off in the [config](configuration.md).

## What's saved

To open your spaces again, Spaces saves what's in them as you go: the terminals' folders, the conversations, and the addresses of the pages open in your browser, except in incognito windows. It all stays in `~/.local/state/omarchy-spaces`, for only you to read.

| File | Holds |
|---|---|
| `session.json` | The session as it is now. What's new is saved within seconds, but what's gone only once it has been gone for ten seconds, and never while the computer shuts down, as that closes everything too |
| `session-previous.json` | The session from before the restart, until it has been restored |
| `sessions/` | The last 48 sessions: the one before each restart, and at most one every 15 minutes besides |

With `browsers = false`, no more addresses are saved, but the sessions already kept still have them until you delete `session*.json` and `sessions/`.

## Opening an earlier session

`space restore` opens the spaces from before the last restart again, even with `auto = false`. To open an earlier session, give it one of the files in `sessions/`:

```sh
space restore ~/.local/state/omarchy-spaces/sessions/<file>
```
