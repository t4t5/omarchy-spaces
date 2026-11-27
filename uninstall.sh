#!/bin/bash

# Undoes what `install.sh --setup` did, once the plugin is removed. The plugin's
# service runs it as it ends, from the copy that setup keeps beside the state:
# by then the plugin's folder may be gone. It also ends when the shell restarts,
# which changes nothing here, or when the plugin is turned off, which only puts
# back what Spaces replaced on the bar.
#
# It leaves the user's config (~/.config/omarchy-spaces) and their keys alone.

set -uo pipefail

id=com.t4t5.spaces
plugin="$HOME/.config/omarchy/plugins/$id"
state="${XDG_STATE_HOME:-$HOME/.local/state}/omarchy-spaces"
link="$HOME/.config/hypr/omarchy-spaces.lua"
space="$HOME/.local/bin/space"

# `omarchy plugin remove` deletes the folder just after it ends the service.
removed() {
  local try
  for ((try = 0; try < ${SPACES_TRIES:-20}; try++)); do
    [[ -e $plugin || -L $plugin ]] || return 0
    sleep 0.25
  done
  return 1
}

# From the file, as a shell that's starting may not have read it yet. Only if
# it says so: not if it can't be read.
off() {
  jq -e --arg id "$id" 'any(..; . == $id)' "$HOME/.config/omarchy/shell.json" >/dev/null 2>&1
  [[ $? == 1 ]]
}

restore_bar() {
  [[ $(cat "$state/bar" 2>/dev/null) == replace ]] || return 0
  # Either may be back on it by now: `put` leaves one that is where it is.
  omarchy bar put omarchy.menu --section left --index 0
  omarchy bar put omarchy.workspaces --after omarchy.menu
  # So turning it on again asks again.
  rm -f "$state/bar"
}

# Each window to the plain workspace its tab's number goes with, where Omarchy's
# keys reach it.
bring_windows_home() {
  local tab='^s[1-9][0-9]*:[1-9][0-9]*$' address key
  local here
  here=$(hyprctl activeworkspace -j | jq -r --arg tab "$tab" '.name | select(test($tab)) | split(":")[1]')
  hyprctl clients -j |
    jq -r --arg tab "$tab" '.[] | select(.workspace.name | test($tab)) | "\(.address) \(.workspace.name | split(":")[1])"' |
    while read -r address key; do
      hyprctl dispatch "hl.dsp.window.move({ workspace = \"$key\", follow = false, window = \"address:$address\" })" </dev/null
    done
  if [[ -n $here ]]; then
    hyprctl dispatch "hl.dsp.focus({ workspace = \"$here\" })"
  fi
}

# Read whole before it runs, as it deletes its own file.
main() {
  if ! removed; then
    if off; then restore_bar; fi
    return
  fi

  # Without it, Hyprland skips the Spaces keys, so Omarchy's are back. Their
  # own file, or a link elsewhere, stays.
  if [[ -L $link && $(readlink "$link") == "$plugin/"* ]]; then
    rm -f "$link"
  fi
  local skill_link
  for skill_link in \
    "$HOME/.agents/skills/omarchy-spaces" \
    "$HOME/.claude/skills/omarchy-spaces" \
    "$HOME/.gemini/config/skills/omarchy-spaces"; do
    if [[ -L $skill_link && $(readlink "$skill_link") == "$plugin/"* ]]; then
      rm -f "$skill_link"
    fi
  done
  hyprctl reload
  bring_windows_home
  restore_bar

  # One built from source stays, as cargo keeps track of it.
  if [[ -f $space && $(sha256sum "$space" | awk '{print $1}') == "$(cat "$state/installed" 2>/dev/null)" ]]; then
    rm -f "$space"
  fi
  rm -rf "$state"
}

main >/dev/null
exit
