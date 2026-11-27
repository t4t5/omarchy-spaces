#!/bin/bash

# Installs the `space` command that goes with this checkout of the plugin, from
# its GitHub release, into ~/.local/bin.
#
#   --update              only if this script installed the current binary and
#                         its version differs from the plugin's
#   --setup               what's missing of the rest: the plugin's service runs
#                         it as it starts. Links the Hyprland part, installs
#                         `space` if there is none, and puts the tabs in place
#                         of Omarchy's menu and workspaces on the bar

set -euo pipefail

fail() {
  echo "install.sh: $*" >&2
  exit 1
}

update=0
case "$*" in
"" | --setup) ;;
--update) update=1 ;;
*) fail "usage: install.sh [--update | --setup]" ;;
esac

plugin_dir=$(dirname "${BASH_SOURCE[0]}")
cd "$plugin_dir"
id=com.t4t5.spaces
releases=${SPACES_RELEASES:-https://github.com/t4t5/omarchy-spaces/releases/download}
version=$(sed -n 's/^  "version": "\([0-9][0-9.]*\)",\?$/\1/p' manifest.json)
[[ -n $version ]] || fail "no version in manifest.json"

arch=$(uname -m)
case $arch in
x86_64 | aarch64) ;;
*) fail "there's no build for $arch: build it from source (see docs/guide/install.md)" ;;
esac

bin="$HOME/.local/bin"
state="${XDG_STATE_HOME:-$HOME/.local/state}/omarchy-spaces"
marker="$state/installed"
replaced="$state/bar"

# Whether a plugin is on: for a widget, on the bar.
enabled() {
  omarchy-shell shell listPlugins | jq -e --arg id "$1" 'any(.[]; .id == $id and .enabled)' >/dev/null
}

# The shell shows notifications itself, so as it starts it may not be ready.
notify() {
  local try
  for try in {1..20}; do
    omarchy-notification-send --app-name Spaces "$@" 2>/dev/null && return
    sleep 0.5
  done
}

is_space() {
  [[ -x $1 ]] && grep -q omarchy-spaces "$1"
}

setup() {
  # A clone someone works in has `just setup`.
  [[ ! -L $plugin_dir ]] || return 0

  # What undoes all this, kept where removing the plugin leaves it.
  install -Dm755 uninstall.sh "$state/uninstall.sh"

  # Before the rest, as it needs no network and the user's keys load it. One that's
  # theirs stays; one that leads nowhere is from an earlier install.
  local link="$HOME/.config/hypr/omarchy-spaces.lua"
  if [[ ! -e $link ]]; then
    mkdir -p "${link%/*}"
    ln -sfn "$PWD/hypr/omarchy-spaces.lua" "$link"
    # Keys there already were skipped without it.
    hyprctl reload >/dev/null || true
  fi

  # Link the agent skill into any installed agent's skill directory.
  if [[ -d "$PWD/skills/omarchy-spaces" ]]; then
    local skill_dir
    for skill_dir in "$HOME/.agents/skills" "$HOME/.claude/skills" "$HOME/.gemini/config/skills"; do
      if [[ -d "${skill_dir%/*}" && ! -e "$skill_dir/omarchy-spaces" ]]; then
        mkdir -p "$skill_dir"
        ln -sfn "$PWD/skills/omarchy-spaces" "$skill_dir/omarchy-spaces"
      fi
    done
  fi

  local said
  if ! is_space "$(command -v space || true)" && ! is_space "$bin/space"; then
    if ! said=$(./install.sh 2>&1); then
      notify -u normal "Couldn't install the space command" "${said##*install.sh: }"
      fail "${said##*install.sh: }"
    fi
    echo "$said"
  fi

  # Once: the bar is the user's after that, so putting the workspaces back keeps
  # them. Not if the tabs were put elsewhere on the bar: they only take the
  # others' place in the left section.
  [[ ! -e $replaced ]] || return 0
  omarchy-shell shell listShellConfig |
    jq -e --arg id "$id" 'any(.bar.layout.left[]?; (.id? // .) == $id)' >/dev/null || return 0
  enabled omarchy.menu || enabled omarchy.workspaces || return 0
  omarchy plugin enable "$id" --section left --index 0
  omarchy plugin disable omarchy.menu
  omarchy plugin disable omarchy.workspaces
  mkdir -p "$state"
  echo replace >"$replaced"
}

case ${1:-} in
--setup)
  setup
  exit
  ;;
esac

if (( update )); then
  [[ ! -L $plugin_dir ]] || exit 0
  [[ -f $marker && -x $bin/space ]] || exit 0
  found=$(command -v space || true)
  [[ -z $found || $found == "$bin/space" ]] || exit 0
  current=$(sha256sum "$bin/space" | awk '{print $1}')
  [[ $current == "$(<"$marker")" ]] || exit 0
  [[ $("$bin/space" --version 2>/dev/null || true) != "space $version" ]] || exit 0
fi

if [[ -e $bin/space ]] && ! grep -q omarchy-spaces "$bin/space"; then
  fail "$bin/space is another program: move it first"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
archive="space-$version-$arch-linux.tar.gz"
for file in "$archive" "$archive.sha256"; do
  curl -fsSL --proto-redir =https -o "$tmp/$file" "$releases/v$version/$file" ||
    fail "can't download $file: is version $version released yet? Nothing was changed"
done
(cd "$tmp" && sha256sum --quiet --check "$archive.sha256") ||
  fail "$archive isn't what the release says it is. Nothing was changed"
tar -xzf "$tmp/$archive" -C "$tmp" space

# In one step, so that nothing runs half of it.
install -Dm755 "$tmp/space" "$bin/.space.$$"
mv "$bin/.space.$$" "$bin/space"
mkdir -p "$state"
sha256sum "$bin/space" | awk '{print $1}' >"$marker.tmp.$$"
mv "$marker.tmp.$$" "$marker"
echo "Installed space $version into $bin"

if (( ! update )); then
  found=$(command -v space || true)
  if [[ -z $found ]]; then
    echo "install.sh: $bin isn't on your PATH, so nothing finds it there yet" >&2
  elif [[ $found != "$bin/space" ]]; then
    echo "install.sh: $found comes first on your PATH, so that one runs instead" >&2
  fi
fi
