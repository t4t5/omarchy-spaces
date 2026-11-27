//! How a tab's windows are tiled, to save and build again after a restart.
//!
//! Hyprland doesn't say how its dwindle layout splits a tab, only where each
//! window ended up. But every dwindle layout can be cut in two, again and
//! again, until each window is alone, so the splits are found back from the
//! windows' positions. `rebuild` then makes them again, in one script that
//! Hyprland runs between two frames: steering dwindle takes focusing windows,
//! and this way nobody sees it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::hypr::lua_string;

/// Where a window is, in Hyprland's logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn start(&self, way: Way) -> i32 {
        match way {
            Way::Right => self.x,
            Way::Down => self.y,
        }
    }

    fn end(&self, way: Way) -> i32 {
        match way {
            Way::Right => self.x + self.w,
            Way::Down => self.y + self.h,
        }
    }
}

/// How far neighbours may overlap, in pixels.
const OVERLAP: i32 = 4;

/// Which side of the first half a split's second half is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Way {
    Right,
    Down,
}

/// A tab's tiles. `T` is what a tile holds: which windows, several when
/// they're grouped, the one showing first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Tree<T> {
    Tile(Vec<T>),
    Split {
        split: Way,
        /// How much of it the first half takes, from 0 to 1.
        share: f64,
        first: Box<Tree<T>>,
        second: Box<Tree<T>>,
    },
}

impl<T> Tree<T> {
    /// The first tile's window that shows.
    fn first(&self) -> Option<&T> {
        match self {
            Tree::Tile(windows) => windows.first(),
            Tree::Split { first, .. } => first.first(),
        }
    }

    /// The windows, in order.
    pub fn windows(&self) -> Vec<&T> {
        match self {
            Tree::Tile(windows) => windows.iter().collect(),
            Tree::Split { first, second, .. } => {
                let mut all = first.windows();
                all.extend(second.windows());
                all
            }
        }
    }

    /// The same tiles, with what `keep` makes of their windows. A tile left
    /// with none goes, and what it was split from takes its room.
    pub fn filter_map<U>(&self, keep: &impl Fn(&T) -> Option<U>) -> Option<Tree<U>> {
        match self {
            Tree::Tile(windows) => {
                let kept: Vec<U> = windows.iter().filter_map(keep).collect();
                (!kept.is_empty()).then_some(Tree::Tile(kept))
            }
            Tree::Split {
                split,
                share,
                first,
                second,
            } => match (first.filter_map(keep), second.filter_map(keep)) {
                (Some(first), Some(second)) => Some(Tree::Split {
                    split: *split,
                    share: *share,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (first, second) => first.or(second),
            },
        }
    }
}

/// Finds back how `tiles` were split, or `None` when they weren't: windows
/// that overlap, as the scrolling layout's can. `border` is how far a window
/// keeps from its tile's edge on the outside, where there's no gap to share.
pub fn find<T: Clone>(tiles: &[(Vec<T>, Rect)], border: i32) -> Option<Tree<T>> {
    let start = |way| tiles.iter().map(|(_, r)| r.start(way)).min();
    let end = |way| tiles.iter().map(|(_, r)| r.end(way)).max();
    let room = Room {
        x: (
            f64::from(start(Way::Right)? - border),
            f64::from(end(Way::Right)? + border),
        ),
        y: (
            f64::from(start(Way::Down)? - border),
            f64::from(end(Way::Down)? + border),
        ),
    };
    find_in(tiles.to_vec(), room)
}

/// The room a tile or a split takes, gaps and all.
#[derive(Clone, Copy)]
struct Room {
    x: (f64, f64),
    y: (f64, f64),
}

fn find_in<T: Clone>(mut tiles: Vec<(Vec<T>, Rect)>, room: Room) -> Option<Tree<T>> {
    if tiles.len() == 1 {
        return tiles.pop().map(|(windows, _)| Tree::Tile(windows));
    }
    for split in [Way::Right, Way::Down] {
        tiles.sort_by_key(|(_, rect)| (rect.start(split), rect.end(split)));
        let mut reach = i32::MIN;
        for i in 1..tiles.len() {
            reach = reach.max(tiles[i - 1].1.end(split));
            let next = tiles[i].1.start(split);
            // Negative gaps can make neighbours overlap a little.
            if next < reach - OVERLAP {
                continue;
            }
            let cut = f64::from(reach + next) / 2.0;
            let (from, to) = match split {
                Way::Right => room.x,
                Way::Down => room.y,
            };
            let (mut one, mut two) = (room, room);
            match split {
                Way::Right => (one.x.1, two.x.0) = (cut, cut),
                Way::Down => (one.y.1, two.y.0) = (cut, cut),
            }
            let second = tiles.split_off(i);
            return Some(Tree::Split {
                split,
                share: ((cut - from) / (to - from) * 10_000.0).round() / 10_000.0,
                first: Box::new(find_in(tiles, one)?),
                second: Box::new(find_in(second, two)?),
            });
        }
    }
    None
}

/// What to make of one tab's windows, by their addresses.
#[derive(Debug, Default)]
pub struct Tab {
    /// Its tiles, on a tab tiled by dwindle.
    pub tiles: Option<Tree<String>>,
    /// Where its floating windows go, in Hyprland's coordinates.
    pub floating: BTreeMap<String, Rect>,
    /// Windows that fill the screen (`fullscreen`) or the tab (`maximized`).
    pub fullscreen: BTreeMap<String, &'static str>,
    /// The window to leave with the tab's focus.
    pub focused: Option<String>,
}

impl Tab {
    /// Whether opening its windows has made all of it already.
    pub fn is_plain(&self) -> bool {
        let one = self.tiles.as_ref().is_none_or(|t| t.windows().len() < 2);
        one && self.floating.is_empty() && self.fullscreen.is_empty()
    }

    /// The Lua that makes it, for Hyprland's `eval`. Its windows float first
    /// and are tiled again one by one, each splitting a given one. It leaves
    /// you, and the cursor, where you were. Animations are off meanwhile, so
    /// going to the tab and back isn't seen. What it notes about where you
    /// were is copied: a workspace it leaves empty goes, and then its object
    /// has nothing.
    pub fn script(&self) -> String {
        let mut lua = String::from(
            "local before, at = hl.get_active_window(), hl.get_active_workspace()\n\
             before = before and \"address:\" .. before.address\n\
             at = at and { name = at.name, id = at.id }\n\
             local pos = hl.get_cursor_pos()\n\
             local animated = hl.get_config(\"animations.enabled\")\n\
             local function go(a) pcall(function() hl.dispatch(a) end) end\n\
             local function focus(w) go(hl.dsp.focus({ window = w })) end\n\
             local function float(w, action) go(hl.dsp.window.float({ action = action, window = w })) end\n\
             local function split(from, w, way, ratio)\n\
             focus(from)\n\
             go(hl.dsp.layout(\"preselect \" .. way))\n\
             float(w, \"unset\")\n\
             focus(from)\n\
             go(hl.dsp.layout(\"splitratio \" .. ratio .. \" exact\"))\n\
             end\n\
             local function group(w, into)\n\
             focus(into)\n\
             go(hl.dsp.layout(\"preselect r\"))\n\
             float(w, \"unset\")\n\
             go(hl.dsp.window.move({ into_group = \"l\", window = w }))\n\
             end\n\
             hl.config({ animations = { enabled = false } })\n\
             local ok, err = pcall(function()\n",
        );
        if let Some(tiles) = self.tiles.as_ref().filter(|t| t.windows().len() > 1) {
            for address in tiles.windows() {
                lua.push_str(&format!("float({}, \"set\")\n", window(address)));
            }
            if let Some(first) = tiles.first() {
                lua.push_str(&format!("float({}, \"unset\")\n", window(first)));
            }
            split(tiles, &mut lua);
            group(tiles, &mut lua);
        }
        for (address, rect) in &self.floating {
            let (w, Rect { x, y, w: wide, h }) = (window(address), rect);
            lua.push_str(&format!(
                "float({w}, \"set\")\n\
                 go(hl.dsp.window.resize({{ x = {wide}, y = {h}, window = {w} }}))\n\
                 go(hl.dsp.window.move({{ x = {x}, y = {y}, window = {w} }}))\n"
            ));
        }
        for (address, mode) in &self.fullscreen {
            lua.push_str(&format!(
                "go(hl.dsp.window.fullscreen({{ mode = \"{mode}\", action = \"set\", window = {} }}))\n",
                window(address)
            ));
        }
        if let Some(focused) = &self.focused {
            lua.push_str(&format!("focus({})\n", window(focused)));
        }
        lua.push_str(
            "end)\n\
             local back, why = pcall(function()\n\
             local now = hl.get_active_workspace()\n\
             if at and (not now or now.name ~= at.name) then\n\
             local to = (at.id or 0) > 0 and tostring(at.id) or \"name:\" .. at.name\n\
             local found = hl.get_workspace(to)\n\
             if not (found and found.name == at.name) then\n\
             for _, w in ipairs(hl.get_workspaces()) do if w.name == at.name then to = w end end\n\
             end\n\
             go(hl.dsp.focus({ workspace = to }))\n\
             end\n\
             if before and not (now and at and now.name == at.name) then focus(before) end\n\
             if pos then go(hl.dsp.cursor.move({ x = pos.x, y = pos.y })) end\n\
             end)\n\
             hl.config({ animations = { enabled = animated } })\n\
             if not ok then error(err) end\n\
             if not back then error(why) end\n",
        );
        lua
    }
}

/// Splits the room its first window has, until every tile has its own.
fn split(tree: &Tree<String>, lua: &mut String) {
    if let Tree::Split {
        split: way,
        share,
        first,
        second,
    } = tree
        && let (Some(from), Some(new)) = (first.first(), second.first())
    {
        let way = match way {
            Way::Right => "r",
            Way::Down => "d",
        };
        // Dwindle counts from 0 to 2, and takes nothing near the ends.
        let ratio = (share * 2.0).clamp(0.1, 1.9);
        lua.push_str(&format!(
            "split({}, {}, \"{way}\", {ratio:.4})\n",
            window(from),
            window(new)
        ));
        split(first, lua);
        split(second, lua);
    }
}

/// Puts grouped windows together, once their tile is where it goes.
fn group(tree: &Tree<String>, lua: &mut String) {
    match tree {
        Tree::Tile(windows) => {
            let Some((shown, rest)) = windows.split_first() else {
                return;
            };
            if rest.is_empty() {
                return;
            }
            let shown = window(shown);
            lua.push_str(&format!(
                "go(hl.dsp.group.toggle({{ window = {shown} }}))\n"
            ));
            for address in rest {
                lua.push_str(&format!("group({}, {shown})\n", window(address)));
                lua.push_str(&format!("focus({shown})\n"));
            }
        }
        Tree::Split { first, second, .. } => {
            group(first, lua);
            group(second, lua);
        }
    }
}

fn window(address: &str) -> String {
    lua_string(&format!("address:{address}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    fn tile(name: &str, rect: Rect) -> (Vec<String>, Rect) {
        (vec![name.to_string()], rect)
    }

    fn only(name: &str) -> Box<Tree<String>> {
        Box::new(Tree::Tile(vec![name.to_string()]))
    }

    #[test]
    fn finds_the_splits_from_where_windows_are() {
        // As Hyprland 0.56 laid them out, at ratios 1.5, 1 and 0.8, with
        // gaps of 5 and 10 and a border of 2.
        let tiles = [
            tile("c", rect(722, 12, 226, 199)),
            tile("a", rect(12, 12, 696, 251)),
            tile("d", rect(722, 225, 226, 303)),
            tile("b", rect(12, 277, 696, 251)),
        ];
        assert_eq!(
            find(&tiles, 2),
            Some(Tree::Split {
                split: Way::Right,
                share: 0.75,
                first: Box::new(Tree::Split {
                    split: Way::Down,
                    share: 0.5,
                    first: only("a"),
                    second: only("b"),
                }),
                second: Box::new(Tree::Split {
                    split: Way::Down,
                    share: 0.4,
                    first: only("c"),
                    second: only("d"),
                }),
            })
        );
    }

    #[test]
    fn finds_nothing_in_windows_that_overlap() {
        let tiles = [
            tile("a", rect(0, 0, 100, 100)),
            tile("b", rect(50, 50, 100, 100)),
        ];
        assert_eq!(find(&tiles, 0), None);
        assert_eq!(find::<String>(&[], 0), None);
    }

    #[test]
    fn a_window_left_out_gives_its_room_to_what_it_was_split_from() {
        let tiles = [
            tile("a", rect(0, 0, 50, 100)),
            tile("browser", rect(50, 0, 50, 50)),
            tile("b", rect(50, 50, 50, 50)),
        ];
        let tree = find(&tiles, 0).unwrap();
        let kept = tree.filter_map(&|w| (w != "browser").then(|| w.clone()));
        assert_eq!(
            kept,
            Some(Tree::Split {
                split: Way::Right,
                share: 0.5,
                first: only("a"),
                second: only("b"),
            })
        );
    }

    #[test]
    fn saves_as_plain_json() {
        let tree: Tree<usize> = Tree::Split {
            split: Way::Down,
            share: 0.25,
            first: Box::new(Tree::Tile(vec![0, 2])),
            second: Box::new(Tree::Tile(vec![1])),
        };
        let json = r#"{"split":"down","share":0.25,"first":[0,2],"second":[1]}"#;
        assert_eq!(serde_json::to_string(&tree).unwrap(), json);
        assert_eq!(serde_json::from_str::<Tree<usize>>(json).unwrap(), tree);
    }

    #[test]
    fn builds_the_splits_from_the_outside_in() {
        let tab = Tab {
            tiles: Some(Tree::Split {
                split: Way::Right,
                share: 0.75,
                first: Box::new(Tree::Split {
                    split: Way::Down,
                    share: 0.5,
                    first: only("0xa"),
                    second: only("0xb"),
                }),
                second: Box::new(Tree::Tile(vec!["0xc".into(), "0xd".into()])),
            }),
            floating: [("0xe".to_string(), rect(1, 2, 300, 200))].into(),
            fullscreen: [("0xb".to_string(), "maximized")].into(),
            focused: Some("0xb".into()),
        };
        let script = tab.script();
        let order = [
            r#"float("address:0xa", "unset")"#,
            r#"split("address:0xa", "address:0xc", "r", 1.5000)"#,
            r#"split("address:0xa", "address:0xb", "d", 1.0000)"#,
            r#"hl.dsp.group.toggle({ window = "address:0xc" })"#,
            r#"group("address:0xd", "address:0xc")"#,
            r#"resize({ x = 300, y = 200, window = "address:0xe" })"#,
            r#"mode = "maximized", action = "set", window = "address:0xb""#,
            r#"focus("address:0xb")"#,
        ];
        let found: Vec<usize> = order.iter().map(|o| script.find(o).expect(o)).collect();
        assert!(found.is_sorted(), "{script}");
    }

    #[test]
    fn one_window_needs_nothing_done() {
        let mut tab = Tab {
            tiles: Some(Tree::Tile(vec!["0xa".into()])),
            ..Default::default()
        };
        assert!(tab.is_plain());
        tab.fullscreen.insert("0xa".into(), "fullscreen");
        assert!(!tab.is_plain());
    }
}
