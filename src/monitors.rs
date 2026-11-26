//! Which block of ten tabs is each monitor's. Omarchy gives every monitor
//! workspaces of its own, the first 1-10, the next one 11-20, and keeps which
//! is whose in `~/.local/state/omarchy/monitor-workspaces`, for the bar to
//! read: `block<TAB>connector<TAB>identity`, a line per monitor ever seen, the
//! connector empty while it's unplugged. A space's tabs follow the same.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::hypr::Workspace;
use crate::name::{self, HOME};
use crate::paths;
use crate::state;

/// The blocks of the monitors plugged in now, by connector.
#[derive(Debug, Default)]
pub struct Blocks(Vec<(String, u8)>);

impl Blocks {
    /// None known when Omarchy hasn't said.
    pub fn load() -> Self {
        Self::read(&paths::omarchy_monitors())
    }

    fn read(path: &Path) -> Self {
        let text = state::read_optional(path).unwrap_or_default();
        Self(
            text.lines()
                .filter_map(|line| {
                    let mut fields = line.split('\t');
                    let block = fields.next()?.parse().ok()?;
                    let connector = fields.next().filter(|c| !c.is_empty())?;
                    Some((connector.to_string(), block))
                })
                .collect(),
        )
    }

    /// The block of the monitor a workspace is on, else the one it's in
    /// itself.
    pub fn of(&self, workspace: &Workspace) -> u8 {
        self.block(&workspace.monitor).unwrap_or_else(|| {
            name::locate(workspace.id, &workspace.name)
                .key()
                .map_or(0, name::block_of)
        })
    }

    /// The block of a monitor, by name.
    pub fn block(&self, monitor: &str) -> Option<u8> {
        let known = self.0.iter().find(|(name, _)| name == monitor);
        known.map(|(_, block)| *block)
    }

    /// The monitor whose tab a key is, if it's plugged in.
    pub fn monitor(&self, key: u8) -> Option<&str> {
        let block = name::block_of(key);
        let known = self.0.iter().find(|(_, of)| *of == block);
        known.map(|(name, _)| name.as_str())
    }
}

/// The slots a monitor's tabs of a space show as, by key. Its own show as
/// theirs. Another monitor's, left here as it was unplugged, are its guests:
/// they take the lowest slots free, in order, or none (0) past the last. So a
/// guest keeps its key, by which it goes back once its monitor does, and its
/// number follows from what's on this one, with nothing to remember or undo.
/// `hypr/omarchy-spaces.lua` numbers them the same.
pub fn slots(keys: impl IntoIterator<Item = u8>, block: u8) -> BTreeMap<u8, u8> {
    let keys: BTreeSet<u8> = keys.into_iter().collect();
    let mut free = (1..=name::SLOTS).filter(|slot| !keys.contains(&name::key_in(block, *slot)));
    keys.iter()
        .map(|key| match name::block_of(*key) == block {
            true => (*key, name::slot_of(*key)),
            false => (*key, free.next().unwrap_or(0)),
        })
        .collect()
}

/// A space's tabs on a monitor, as its bar shows them.
#[derive(Debug)]
pub struct Strip {
    block: u8,
    slots: BTreeMap<u8, u8>,
    /// Whether it's `home`'s, whose tabs keep their numbers.
    home: bool,
}

impl Strip {
    /// Those on the monitor that `on` is on, `on` itself included if it's one.
    pub fn new(workspaces: &[Workspace], space: &str, on: &Workspace, blocks: &Blocks) -> Self {
        let block = blocks.of(on);
        let keys: Vec<u8> = workspaces
            .iter()
            .filter(|ws| ws.monitor == on.monitor)
            .chain([on])
            .map(|ws| name::locate(ws.id, &ws.name))
            .filter(|location| location.space() == space)
            .filter_map(|location| location.key())
            .collect();
        // Omarchy numbers `home`'s guests itself.
        let slots = if space == HOME {
            keys.into_iter()
                .map(|key| (key, name::slot_of(key)))
                .collect()
        } else {
            slots(keys, block)
        };
        Self {
            block,
            slots,
            home: space == HOME,
        }
    }

    /// The tabs, by key, in the order they show.
    pub fn keys(&self) -> Vec<u8> {
        let mut keys: Vec<u8> = self.slots.keys().copied().collect();
        keys.sort_by_key(|key| (self.slots[key] == 0, self.slots[key], *key));
        keys
    }

    /// The slot a tab shows as.
    pub fn slot(&self, key: u8) -> Option<u8> {
        self.slots.get(&key).copied()
    }

    /// The tab to go to for a slot, as `super+N` does: the one that shows as
    /// it, or past the last a new one after it, as a space's tabs have no
    /// gaps (`space renumber`). `home`'s are Omarchy's workspaces, any of
    /// them.
    pub fn reach(&self, slot: u8) -> u8 {
        if self.home {
            return self.at(slot);
        }
        let after_last = u8::try_from(self.slots.len() + 1).unwrap_or(u8::MAX);
        self.at(slot.min(after_last))
    }

    /// The tab that shows as a slot, or would.
    pub fn at(&self, slot: u8) -> u8 {
        let there = self.slots.iter().find(|(_, shown)| **shown == slot);
        there.map_or_else(|| self.own(slot), |(key, _)| *key)
    }

    /// The monitor's own tab that shows as a slot.
    pub fn own(&self, slot: u8) -> u8 {
        name::key_in(self.block, slot)
    }

    /// A new tab for the monitor, after its last, else in its first free slot.
    /// `taken` are the space's that aren't here, or not yet.
    pub fn new_tab(&self, taken: impl IntoIterator<Item = u8>) -> Option<u8> {
        let taken = taken
            .into_iter()
            .filter(|key| name::block_of(*key) == self.block);
        let open: Vec<u8> = self
            .slots
            .values()
            .copied()
            .chain(taken.map(name::slot_of))
            .collect();
        let after_last = open.iter().max().map_or(1, |last| last + 1);
        let slot = (after_last..=name::SLOTS)
            .chain(1..=name::SLOTS)
            .find(|slot| !open.contains(slot))?;
        Some(self.own(slot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(keys: &[u8], block: u8) -> Strip {
        Strip {
            block,
            slots: slots(keys.iter().copied(), block),
            home: false,
        }
    }

    #[test]
    fn guests_take_the_lowest_free_slots() {
        let shown = |keys: &[u8], block| {
            let slots = slots(keys.iter().copied(), block);
            slots.into_iter().collect::<Vec<_>>()
        };
        assert_eq!(shown(&[1, 2], 0), [(1, 1), (2, 2)]);
        assert_eq!(shown(&[11, 12], 1), [(11, 1), (12, 2)], "its own");
        assert_eq!(
            shown(&[1, 2, 3, 11], 0),
            [(1, 1), (2, 2), (3, 3), (11, 4)],
            "after the monitor's own"
        );
        assert_eq!(
            shown(&[1, 12, 3, 21], 0),
            [(1, 1), (3, 3), (12, 2), (21, 4)],
            "a gap among them first"
        );
        assert_eq!(shown(&[11, 11], 0), [(11, 1)]);
        // A tab opened after it leaves it its slot.
        assert_eq!(shown(&[1, 11], 0), [(1, 1), (11, 2)]);
        assert_eq!(strip(&[1, 11], 0).new_tab([]), Some(3));
        assert_eq!(shown(&[1, 3, 11], 0), [(1, 1), (3, 3), (11, 2)]);
        let full: Vec<u8> = (1..=10).chain([11]).collect();
        assert_eq!(shown(&full, 0)[10], (11, 0), "no room");
        assert_eq!(strip(&full, 0).new_tab([]), None);
        assert_eq!(strip(&full, 0).keys().last(), Some(&11));
    }

    #[test]
    fn finds_the_tab_a_slot_shows() {
        let strip = strip(&[1, 2, 11], 0);
        assert_eq!((strip.at(2), strip.at(3), strip.at(4)), (2, 11, 4));
        assert_eq!((strip.slot(11), strip.slot(4)), (Some(3), None));
        assert_eq!(strip.keys(), [1, 2, 11]);
    }

    #[test]
    fn goes_no_further_than_a_new_tab_after_the_last() {
        let two = strip(&[1, 2], 0);
        assert_eq!((two.reach(2), two.reach(3), two.reach(6)), (2, 3, 3));
        assert_eq!(strip(&[11], 1).reach(5), 12);
        assert_eq!(strip(&[], 0).reach(4), 1);
        let home = Strip {
            home: true,
            ..strip(&[1, 2], 0)
        };
        assert_eq!(home.reach(6), 6, "home's keep their numbers");
    }

    #[test]
    fn new_tabs_are_the_monitors_own() {
        let new = |keys: &[u8], block| strip(keys, block).new_tab([]);
        assert_eq!(new(&[1], 0), Some(2));
        assert_eq!(new(&[1, 2, 5], 0), Some(6));
        assert_eq!(new(&[2, 10], 0), Some(1), "a gap once 10 is taken");
        assert_eq!(new(&[11], 1), Some(12));
        assert_eq!(new(&[], 1), Some(11));
        assert_eq!(new(&(1..=10).collect::<Vec<_>>(), 0), None);
        assert_eq!(strip(&[1], 0).new_tab([2, 12]), Some(3), "2 is opening");
    }

    fn workspace(name: &str, monitor: &str) -> Workspace {
        Workspace {
            id: -1337,
            name: name.into(),
            windows: 0,
            monitor: monitor.into(),
            layout: String::new(),
        }
    }

    #[test]
    fn reads_omarchys_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("monitor-workspaces");
        std::fs::write(
            &path,
            "0\teDP-1\tBOE\n1\tDP-7\tStudioDisplay\n2\t\tFALLBACK\nbroken\n",
        )
        .unwrap();
        let blocks = Blocks::read(&path);
        assert_eq!(blocks.of(&workspace("s1:12", "DP-7")), 1);
        // A tab left behind by an unplugged monitor is on this one.
        assert_eq!(blocks.of(&workspace("s1:21", "eDP-1")), 0);
        assert_eq!(blocks.of(&workspace("s1:21", "HDMI-1")), 2);
        assert_eq!(blocks.monitor(12), Some("DP-7"));
        assert_eq!(blocks.monitor(21), None);
    }

    #[test]
    fn without_them_a_workspace_is_in_its_own() {
        let blocks = Blocks::read(Path::new("/nonexistent"));
        assert_eq!(blocks.of(&workspace("s1:12", "DP-7")), 1);
        assert_eq!(blocks.of(&workspace("chat", "DP-7")), 0);
    }
}
