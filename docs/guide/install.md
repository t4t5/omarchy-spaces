# Installation

You need Omarchy 4.5 or newer, on x86-64 or ARM64. To install, follow the steps in the [README](../../README.md#install).

## What gets installed

| Part | Where | What it does |
|---|---|---|
| The plugin | `~/.config/omarchy/plugins/com.t4t5.spaces` | Draws the sidebar and the tabs |
| The `space` command | `~/.local/bin/space` | Does the work. Downloaded as the plugin first starts: the build for the plugin's version from its GitHub release, checked against the release's checksum |
| The Hyprland part | `~/.config/hypr/omarchy-spaces.lua`, a link into the plugin | Keeps every monitor on the same space and holds the [actions for your keys](keys.md) |

The plugin runs `install.sh --setup` as it starts, which puts in place what's missing of these. It binds no keys.

If the download fails, as it does offline, it tries again the next time the shell starts. To try at once:

```sh
omarchy restart shell
```

## The bar

If you put the tabs in the left section, they take the place of Omarchy's menu and workspaces as the plugin first starts. A right-click on the logo still opens the menu. Spaces does this only once, so if you put Omarchy's back, they stay:

```sh
omarchy plugin enable omarchy.menu --section left --index 0
omarchy plugin enable omarchy.workspaces --after omarchy.menu
```

## Update

```sh
omarchy plugin update com.t4t5.spaces
```

If the plugin installed `space`, it sees the new version in `manifest.json`, runs `install.sh --update` to download the matching release binary and switches to it. If the plugin's UI changed, it notifies you to run `omarchy restart shell` (otherwise the new UI loads on your next login). Builds installed from source are left alone — update those with `cargo install` below.

## Build from source

With Rust (`sudo pacman -S rust` if `cargo --version` fails), run this once the plugin is added, and again after each update:

```sh
cargo install --locked --force --root ~/.local --target-dir ~/.cache/omarchy-spaces --path ~/.config/omarchy/plugins/com.t4t5.spaces
```

It builds outside the plugin's folder, as the shell watches that for changes.

## Uninstall

```sh
omarchy plugin remove com.t4t5.spaces
```

The plugin undoes what it set up, with `uninstall.sh`. A few things stay:

- Your own settings, in `~/.config/omarchy-spaces`.
- Your keys in `~/.config/hypr/bindings.lua`. The `if` around them skips them once Spaces is gone. Without it, take them out before you remove the plugin, or Hyprland reports an error.
- A `space` you built from source. Before you remove the plugin, run:

  ```sh
  cargo uninstall --root ~/.local omarchy-spaces
  rm -rf ~/.cache/omarchy-spaces
  ```

Turning the plugin off with `omarchy plugin disable com.t4t5.spaces` only puts Omarchy's menu and workspaces back on the bar. If you removed it while it was off, the rest is still there. To finish:

```sh
~/.local/state/omarchy-spaces/uninstall.sh
```
