# space-git

Shows each space's git branch under its name in the sidebar, with a dot when it has changes. It's an example of a [space widget](../../docs/guide/sidebar.md#customizing-the-sidebar) fed by a script.

- `space-git` keeps each space's branch in its metadata, as `git`. It needs `jq`.
- `space-git.qml` shows it.

## Try it

1. Copy the script somewhere on your `PATH` and the widget to your bar's modules:

```sh
install -m 755 space-git ~/.local/bin/space-git
cp space-git.qml ~/.config/omarchy/bar/modules/space-git.qml
```

2. Start the script with your session, in `~/.config/hypr/autostart.lua`:

```lua
o.launch_on_start("space-git")
```

Run `space-git &` to try it now. `space meta get git` in a space's terminal shows what it keeps.

3. Add the widget to the `com.t4t5.spaces` entry in `~/.config/omarchy/shell.json`:

```json
"sidebar": {
  "space": {
    "widgets": [{ "id": "space-git", "type": "qml" }]
  }
}
```

A new QML module may need `omarchy restart shell` before it loads.
