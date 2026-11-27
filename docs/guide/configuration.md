# Configuration

Spaces works without a config. To change something, make `~/.config/omarchy-spaces/config.toml`, with only the settings you want. These are all of them, as they are by default:

```toml
[restore]
auto = true       # open your spaces again after you log out or restart; `space restore` always does
agents = true     # with the agents' conversations that were in their terminals
browsers = true   # with your browser's windows; false also stops saving the pages you have open

[sounds]
enabled = false   # play a sound when an agent starts waiting for you, or finishes while you aren't looking
# done = "~/ding.oga"      # your own sounds, in place of the ones it comes with;
# waiting = "~/dong.oga"   # a relative path is from the config's folder
```

Changes count at once. A setting Spaces doesn't know gets a warning, and the others still count.

See [Restoring your spaces](restore.md) and [Agent status](agent-status.md) for what these do.

## Terminal

Spaces opens the terminal you chose in Omarchy (`omarchy default terminal`).

Each Ghostty window gets a process of its own: from one process, Hyprland couldn't open it on the tab it's for. Other terminals have to open a process per window too, so use `foot` rather than `footclient`.
