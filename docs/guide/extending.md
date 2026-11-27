# Extending Spaces

Spaces stays small on purpose: instead of bundling git, worktrees, or issue trackers into core, it gives you four composable primitives that fit together cleanly:

1. **`space meta`** — persist any JSON on a space (`space meta set git --json ...`).
2. **`[hooks]` in `config.toml`** — run shell scripts automatically on `space.created`, `space.focused`, `space.removed`, `status.changed`, and other events.
3. **`[[actions.space]]` / `[[actions.tab]]` in `config.toml`** — put commands of your own in right-click menus (conditionally via `when`) and bind them with `spaces.action("<id>")`.
4. **`rowWidget` in `shell.json`** — render `space.meta` under each space's name in the sidebar while keeping native search, drag-to-reorder, rename, and icons.

Below are two complete, copy-pasteable recipes. Put scripts in `~/.config/omarchy-spaces/scripts/` and `chmod +x` them.

## Recipe 1: Git branch & dirty status in the sidebar

### 1. Hook script (`~/.config/omarchy-spaces/scripts/git-meta.sh`)

```sh
#!/usr/bin/env bash
set -euo pipefail

[ -n "${SPACE_ID:-}" ] && [ -n "${SPACE_DIR:-}" ] && [ -d "$SPACE_DIR" ] || exit 0

if ! git -C "$SPACE_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  space meta rm -s "$SPACE_ID" git >/dev/null 2>&1 || true
  exit 0
fi

branch=$(git -C "$SPACE_DIR" symbolic-ref --short HEAD 2>/dev/null || git -C "$SPACE_DIR" rev-parse --short HEAD)
dirty=false
if [ -n "$(git -C "$SPACE_DIR" status --porcelain 2>/dev/null)" ]; then
  dirty=true
fi

space meta set -s "$SPACE_ID" git --json "$(jq -cn --arg branch "$branch" --argjson dirty "$dirty" '{branch: $branch, dirty: $dirty}')" >/dev/null
```

### 2. Wire the hook (`~/.config/omarchy-spaces/config.toml`)

```toml
[hooks]
"space.created" = "~/.config/omarchy-spaces/scripts/git-meta.sh"
"space.focused" = "~/.config/omarchy-spaces/scripts/git-meta.sh"
```

### 3. Render it in the sidebar (`~/.config/omarchy/bar/modules/space-git.qml`)

```qml
import QtQuick
import qs.Commons

Item {
  property var space: null
  property color foreground: Color.bar.text
  property color accent: Color.accent
  property string fontFamily: Style.font.family

  readonly property var git: space && space.meta ? space.meta.git : null
  readonly property var wt: space && space.meta ? space.meta.worktree : null
  readonly property string branch: git && git.branch ? String(git.branch) : ""
  readonly property bool dirty: !!(git && git.dirty)

  visible: branch !== "" || !!wt
  implicitHeight: visible ? label.implicitHeight : 0

  Text {
    id: label
    width: parent.width
    textFormat: Text.PlainText
    text: (wt ? "󰘬 " : " ") + (branch || (wt && wt.branch ? wt.branch : "worktree")) + (dirty ? " *" : "")
    color: dirty ? accent : foreground
    opacity: dirty ? 0.9 : 0.55
    font.family: fontFamily
    font.pixelSize: Style.font.bodySmall
    elide: Text.ElideRight
  }
}
```

### 4. Enable `rowWidget` in `~/.config/omarchy/shell.json`

```json
{
  "id": "com.t4t5.spaces",
  "rowWidget": { "id": "space-git", "type": "qml" }
}
```

Run `omarchy restart shell` to load a new QML module for the first time.

## Recipe 2: Git worktree spaces

### 1. Create a worktree space (`~/.config/omarchy-spaces/scripts/new-worktree.sh`)

```sh
#!/usr/bin/env bash
set -euo pipefail

[ -n "${SPACE_DIR:-}" ] && [ -d "$SPACE_DIR" ] || { echo "Space has no folder" >&2; exit 1; }
repo=$(git -C "$SPACE_DIR" rev-parse --show-toplevel 2>/dev/null) || { echo "Not a git repository" >&2; exit 1; }
common=$(git -C "$repo" rev-parse --path-format=absolute --git-common-dir)
main_root=$(dirname "$common")

branch=$(printf '' | walker --dmenu --placeholder "New worktree branch…" || true)
[ -n "$branch" ] || exit 0

slug=$(printf '%s' "$branch" | tr '/ ' '--')
wt_dir="$main_root/.worktrees/$slug"
mkdir -p "$(dirname "$wt_dir")"

if git -C "$main_root" show-ref --verify --quiet "refs/heads/$branch"; then
  git -C "$main_root" worktree add "$wt_dir" "$branch"
else
  git -C "$main_root" worktree add -b "$branch" "$wt_dir"
fi

# Optional: copy gitignored files listed in .worktreeinclude (e.g. .env)
if [ -f "$main_root/.worktreeinclude" ]; then
  while IFS= read -r rel; do
    [ -n "$rel" ] && [ -e "$main_root/$rel" ] || continue
    mkdir -p "$wt_dir/$(dirname "$rel")"
    cp -a "$main_root/$rel" "$wt_dir/$rel"
  done < "$main_root/.worktreeinclude"
fi

created=$(space new "$wt_dir" --name "$branch" --json)
new_id=$(printf '%s' "$created" | jq -r '.id')
space meta set -s "$new_id" worktree --json "$(jq -cn --arg repo "$main_root" --arg path "$wt_dir" --arg branch "$branch" '{repo: $repo, path: $path, branch: $branch}')" >/dev/null
```

### 2. Close & remove a worktree space (`~/.config/omarchy-spaces/scripts/rm-worktree.sh`)

```sh
#!/usr/bin/env bash
set -euo pipefail

repo=$(printf '%s' "${SPACE_META:-{}}" | jq -r '.worktree.repo // empty')
wt_path=$(printf '%s' "${SPACE_META:-{}}" | jq -r '.worktree.path // empty')
[ -n "$repo" ] && [ -n "$wt_path" ] || { echo "Not a worktree space" >&2; exit 1; }

if [ -n "$(git -C "$wt_path" status --porcelain 2>/dev/null)" ]; then
  echo "Worktree has uncommitted changes" >&2
  exit 1
fi

space close "$SPACE_ID"
git -C "$repo" worktree remove "$wt_path"
```

### 3. Add them to your right-click menu (`~/.config/omarchy-spaces/config.toml`)

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
when = "meta.worktree"
command = "~/.config/omarchy-spaces/scripts/rm-worktree.sh"
```

You can also bind them in `~/.config/hypr/bindings.lua`:

```lua
o.bind("SUPER + SHIFT + W", "New worktree space", spaces.action("new-worktree"))
```
