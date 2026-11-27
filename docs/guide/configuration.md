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

## Custom menu actions

Add `[[actions.space]]` or `[[actions.tab]]` entries to put commands of your own in a space's or tab's right-click menu, or bind them with `spaces.action("<id>")` in [Keys](keys.md):

```toml
[[actions.space]]
id = "new-worktree"
label = "New worktree…"
icon = "󰘬"
when = "folder"
command = "~/.config/omarchy-spaces/scripts/new-worktree.sh"

[[actions.space]]
id = "remove-worktree"
label = "Close & remove worktree"
icon = "󰆴"
hint = "git"
when = "meta.worktree"
command = ["~/.config/omarchy-spaces/scripts/rm-worktree.sh", "--force"]

[[actions.tab]]
id = "open-lazygit"
label = "Open lazygit"
icon = "󰊢"
when = "folder"
command = "space tab new -- lazygit"
```

- `label` and `command` are required. `command` is a shell string (run with `sh -c`) or an argument list (with `~/` expanded on the executable).
- `id` is what `space action <id>` takes; left out, it is slugified from `label` (`"New worktree…"` becomes `new-worktree`).
- `icon` is a Nerd Font glyph or emoji, and `hint` is muted text on the right of the row.
- `when` hides the action when it doesn't apply. Comma-separated clauses are ANDed: `folder`, `home`, `status`, `status=<working|done|waiting>`, `meta.<path>`, `meta.<path>=<value>`, `meta.<path>!=<value>`, each optionally negated with `!`.

Actions run in the space's folder (or `~` when it has none) with `SPACE_ID`, `SPACE_LABEL`, `SPACE_DIR`, `SPACE_META` (JSON), `SPACE_STATUS`, `SPACE_ACTION`, and (for tab actions) `SPACE_TAB` in their environment.

## Event hooks

Add `[hooks]` to run shell commands whenever something changes, without maintaining a background daemon of your own:

```toml
[hooks]
"space.created" = "~/.config/omarchy-spaces/scripts/git-meta.sh"
"space.focused" = ["~/.config/omarchy-spaces/scripts/git-meta.sh"]
"space.removed" = "~/.config/omarchy-spaces/scripts/on-close.sh"
"status.changed" = "~/.config/omarchy-spaces/scripts/notify.sh"
```

Each event key (`space.created`, `space.removed`, `space.focused`, `space.changed`, `tab.created`, `tab.removed`, `tab.changed`, `status.changed`) takes a command string or a list of them. Hooks run asynchronously from the primary `space watch` on real changes (never on the initial startup snapshot), receiving the event's `space events` JSON line on stdin and `SPACE_EVENT`, `SPACE_ID`, `SPACE_LABEL`, `SPACE_DIR`, `SPACE_META`, `SPACE_STATUS`, `SPACE_PREVIOUS_STATUS`, `SPACE_TAB`, and `SPACE_CHANGED` in their environment.

Avoid writing `space meta` unconditionally inside a `space.changed` hook, as that triggers another `space.changed` event; hook `space.created` and `space.focused` instead, or check `$SPACE_CHANGED`.

See [Extending Spaces](extending.md) for complete git status and worktree recipes.

## Terminal

Spaces opens the terminal you chose in Omarchy (`omarchy default terminal`).

Each Ghostty window gets a process of its own: from one process, Hyprland couldn't open it on the tab it's for. Other terminals have to open a process per window too, so use `foot` rather than `footclient`.
