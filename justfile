state := env("XDG_STATE_HOME", env("HOME") / ".local/state") / "omarchy-spaces"

default:
    @just --list

install:
    cargo install --path .

# Add the tabs to the bar and the sidebar to the Omarchy shell, and load the Hyprland part
setup:
    # Not into one that `omarchy plugin add` put there.
    [ -L ~/.config/omarchy/plugins/com.t4t5.spaces ] || [ ! -e ~/.config/omarchy/plugins/com.t4t5.spaces ]
    # What `just uninstall` set aside.
    [ -e {{state}} ] || [ ! -e {{state}}.dev ] || mv {{state}}.dev {{state}}
    ln -sfn {{justfile_directory()}} ~/.config/omarchy/plugins/com.t4t5.spaces
    omarchy-shell shell rescanPlugins
    # The rescan finishes after the call returns; wait until the shell lists the plugin.
    for i in $(seq 50); do omarchy-shell shell listPlugins | grep -q '"com.t4t5.spaces"' && break; sleep 0.1; done
    # First on the bar, the space's logo and tabs take the place of the menu and the workspaces.
    omarchy plugin enable com.t4t5.spaces --section left --index 0
    omarchy plugin disable omarchy.menu
    omarchy plugin disable omarchy.workspaces
    ln -sfn {{justfile_directory()}}/hypr/omarchy-spaces.lua ~/.config/hypr/omarchy-spaces.lua
    for d in ~/.agents/skills ~/.claude/skills ~/.gemini/config/skills; do [ -d "${d%/*}" ] && [ ! -e "$d/omarchy-spaces" ] && mkdir -p "$d" && ln -sfn ~/.config/omarchy/plugins/com.t4t5.spaces/skills/omarchy-spaces "$d/omarchy-spaces" || true; done
    grep -qs 'require("hypr.omarchy-spaces")' ~/.config/hypr/*.lua || echo 'if package.searchpath("hypr.omarchy-spaces", package.path) then require("hypr.omarchy-spaces").bind_tab_keys() end' >> ~/.config/hypr/bindings.lua
    # Hyprland doesn't notice the link by itself.
    hyprctl reload

# Undo `just install` and `just setup`, to try the real install. Your spaces' windows go to the plain workspaces; what Spaces has saved is set aside, and `just setup` brings it back
uninstall:
    #!/usr/bin/env bash
    set -euo pipefail
    plugin=~/.config/omarchy/plugins/com.t4t5.spaces
    link=~/.config/hypr/omarchy-spaces.lua
    [[ -L $plugin ]] || { echo "Not linked by \`just setup\`: use omarchy plugin remove com.t4t5.spaces" >&2; exit 1; }
    [[ ! -e {{state}}.dev ]] || { echo "{{state}}.dev is there from before: move it first" >&2; exit 1; }
    for file in $(grep -ls 'require("hypr.omarchy-spaces")' ~/.config/hypr/*.lua || true); do
      grep -q 'searchpath("hypr.omarchy-spaces"' "$file" && continue
      echo "$file needs this before its require, or Hyprland's config fails without Spaces:" >&2
      echo '  if not package.searchpath("hypr.omarchy-spaces", package.path) then return end' >&2
      exit 1
    done
    omarchy plugin remove com.t4t5.spaces --yes
    rm -f "$link"
    if [[ -d {{state}} ]]; then
      # `space watch` saves over it once it's back, without the spaces.
      cp {{state}}/session.json {{state}}/before-uninstall.json || true
      mv {{state}} {{state}}.dev
    fi
    # The rest as a removed plugin does it, with the bar `just setup` replaced.
    mkdir -p {{state}}
    echo replace > {{state}}/bar
    ./uninstall.sh
    cargo uninstall omarchy-spaces

# Give the plugin that `omarchy plugin add` put there its `space`, built here, while its version has no release to download
install-unreleased:
    #!/usr/bin/env bash
    set -euo pipefail
    plugin=~/.config/omarchy/plugins/com.t4t5.spaces
    [[ -d $plugin && ! -L $plugin ]] || { echo "Add it first: omarchy plugin add https://github.com/t4t5/omarchy-spaces.git --enable" >&2; exit 1; }
    releases={{quote(justfile_directory())}}/target/releases
    cargo build --release --locked
    scripts/package.sh target/release/space "$(uname -m)" "$releases/v$(jq -r .version manifest.json)"
    SPACES_RELEASES="file://$releases" "$plugin/install.sh"
    # It carries on with its setup as it starts.
    omarchy restart shell

# Raise the version of `space` and the plugin, such as `just bump 0.2.0`, commit that and tag it
bump version:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ {{quote(version)}} =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Give a version such as 0.2.0, without the v" >&2; exit 1; }
    sed -i 's/^version = ".*"$/version = "{{version}}"/' Cargo.toml
    sed -i '/^name = "omarchy-spaces"$/{n;s/^version = ".*"$/version = "{{version}}"/}' Cargo.lock
    # install.sh reads it with a sed of its own, so the line stays as it is.
    sed -i 's/^  "version": ".*",$/  "version": "{{version}}",/' manifest.json
    git commit --quiet -m "Release {{version}}" -- Cargo.toml Cargo.lock manifest.json
    git tag v{{version}}
    git show --stat --oneline HEAD

# Make the icon picker's glyphs again, from a Nerd Fonts release's glyph names
glyphs version="v3.4.0":
    mkdir -p target
    curl -sSfL -o target/glyphnames.json https://raw.githubusercontent.com/ryanoasis/nerd-fonts/{{version}}/glyphnames.json
    python3 scripts/glyphs.py target/glyphnames.json omarchy/glyphs.json

# Restart the Omarchy shell, so it picks up changes to the QML. Reloading its
# plugins isn't enough: it keeps keepLoaded services, which the sidebar is.
reload:
    omarchy restart shell

# Go between spaces quickly, here, and check that the sidebar and bars keep up
stress rounds="20":
    python3 scripts/stress.py {{rounds}}
