# Agent status

An agent in a terminal says what it's doing, and its tab and its space's badge show it as a dot:

| Dot | Means |
|---|---|
| Yellow, breathing | It's working |
| Red | It's waiting for you: a permission prompt, a question |
| Green | It finished while you were elsewhere. It clears once you focus the agent's window |

A space shows whichever of its agents most needs you.

## Which agents show

- **Claude Code** works out of the box. Spaces reads what it notes about itself in `~/.claude/sessions/`, or that of the account it runs as, so there's nothing to set up.
- **Antigravity CLI (`agy`)** works out of the box. Spaces matches each running `agy` process to its open session lock and reads its run status from `conversation_summaries.db`.

Other agents show as any program does, by name, without a status.

## Sounds

Spaces can play a sound when an agent starts waiting for you, or finishes while you aren't looking. They're off until you turn them on in the [config](configuration.md):

```toml
[sounds]
enabled = true
```
