//! The tabs of a space on a monitor, as its bar shows them. What's printed of
//! them is up to who prints it: `view.rs` for the plugin, `list.rs` for
//! scripts. Neither gives away a tab's key, which is ours to change: only its
//! id and the number it shows.

use std::collections::{BTreeMap, BTreeSet};

use crate::browser;
use crate::live::{Live, Row, Snapshot};
use crate::monitors;
use crate::name::{self, HOME};
use crate::status::Status;

#[derive(Debug)]
pub(crate) struct Tab {
    pub id: String,
    /// The number it shows, 1-10, `None` past the last.
    pub slot: Option<u8>,
    /// The name you gave it, else what its last used window is called. One of
    /// `home`'s has only the name you gave it.
    pub label: String,
    /// Whether you gave it the name.
    pub named: bool,
    /// What most needs you among the agents in its windows.
    pub status: Option<Status>,
    pub windows: u32,
    /// Whether its monitor shows it.
    pub shown: bool,
}

/// A space's tabs on a monitor, in the order they show.
pub(crate) fn on_monitor(
    space: &str,
    monitor: &str,
    snapshot: &Snapshot,
    live: &Live,
    row: Option<&Row>,
) -> Vec<Tab> {
    let shown = shown_on(space, monitor, snapshot);
    slots(space, monitor, snapshot)
        .into_iter()
        .map(|(key, slot)| Tab {
            id: name::tab_id(space, key),
            slot: (slot > 0).then_some(slot),
            shown: shown == Some(key),
            ..tab(space, key, snapshot, live, row)
        })
        .collect()
}

/// The number the bar shows a tab as, by its key: on the monitor it's on, the
/// same numbering `on_monitor` gives. `None` for a tab with no number, or one
/// no monitor is known to have.
pub(crate) fn slot_of(space: &str, key: u8, snapshot: &Snapshot) -> Option<u8> {
    let monitor = &snapshot.places.get(&(space.to_string(), key))?.monitor;
    slots(space, monitor, snapshot)
        .into_iter()
        .find_map(|(k, slot)| (k == key && slot > 0).then_some(slot))
}

/// The space's tab that a monitor shows, by its key.
fn shown_on(space: &str, monitor: &str, snapshot: &Snapshot) -> Option<u8> {
    let screen = snapshot.screens.iter().find(|s| s.name == monitor)?;
    screen.shows.key().filter(|_| screen.shows.space() == space)
}

/// The tabs' keys and the slots they show as, 0 for none.
fn slots(space: &str, monitor: &str, snapshot: &Snapshot) -> Vec<(u8, u8)> {
    let keys: BTreeSet<u8> = snapshot
        .places
        .iter()
        .filter(|((of, _), place)| of == space && place.monitor == monitor)
        .map(|((_, key), _)| *key)
        .chain(shown_on(space, monitor, snapshot))
        .collect();
    let shown: BTreeMap<u8, u8> = match snapshot.blocks.block(monitor) {
        // Omarchy numbers `home`'s guests itself.
        Some(block) if space != HOME => monitors::slots(keys, block),
        _ => {
            let mut taken = BTreeSet::new();
            let numbered = keys.into_iter().map(|key| (key, name::slot_of(key)));
            numbered
                .map(|(key, slot)| (key, if taken.insert(slot) { slot } else { 0 }))
                .collect()
        }
    };
    let mut shown: Vec<(u8, u8)> = shown.into_iter().collect();
    shown.sort_by_key(|(key, slot)| (*slot == 0, *slot, *key));
    shown
}

fn tab(space: &str, key: u8, snapshot: &Snapshot, live: &Live, row: Option<&Row>) -> Tab {
    let windows: Vec<_> = snapshot
        .space_windows
        .iter()
        .filter(|w| w.space == space && w.tab == Some(key))
        .collect();
    let name = row.and_then(|row| row.tabs.get(&key)).cloned();
    let label = || {
        // Windows that never had focus say -1, and go last.
        let last_used = windows
            .iter()
            .min_by_key(|w| u64::try_from(w.focus).unwrap_or(u64::MAX))?;
        let terminal = row?
            .terminals
            .iter()
            .find(|t| t.address == last_used.address);
        Some(terminal.map_or_else(|| app_name(&last_used.class), |t| t.label.clone()))
    };
    Tab {
        id: name::tab_id(space, key),
        slot: None,
        named: name.is_some(),
        label: match name {
            Some(name) => name,
            None if space == HOME => String::new(),
            None => label().unwrap_or_default(),
        },
        status: windows
            .iter()
            .filter_map(|w| live.statuses.get(&w.address).copied())
            .max(),
        windows: windows.len().try_into().unwrap_or(u32::MAX),
        shown: false,
    }
}

/// A short app name from a window's class: `org.omarchy.quick-terminal` is
/// `Quick-terminal`, Omarchy's `chrome-app.hey.com__-Default` web apps are
/// `Hey.com`.
fn app_name(class: &str) -> String {
    let name = match browser::web_app_site(class) {
        Some(site) => ["app.", "www."]
            .iter()
            .find_map(|prefix| site.strip_prefix(prefix))
            .unwrap_or(site),
        None => class.rsplit('.').next().unwrap_or(class),
    };
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::Screen;

    #[test]
    fn names_apps_from_window_classes() {
        assert_eq!(app_name("chromium"), "Chromium");
        assert_eq!(app_name("org.omarchy.quick-terminal"), "Quick-terminal");
        assert_eq!(app_name("com.mitchellh.ghostty"), "Ghostty");
        assert_eq!(app_name("chrome-app.hey.com__-Default"), "Hey.com");
        assert_eq!(app_name("chromium-www.x.com__-Default"), "X.com");
        assert_eq!(app_name(""), "");
    }

    #[test]
    fn home_tabs_with_the_same_number_show_it_once() {
        let mut snapshot = Snapshot::default();
        snapshot.screens.push(Screen {
            name: "eDP-1".into(),
            focused: true,
            refresh: 60.0,
            shows: name::locate(3, "3"),
        });
        for key in [2, 12] {
            let place = snapshot.places.entry((HOME.into(), key)).or_default();
            place.monitor = "eDP-1".into();
        }
        let tabs = on_monitor(HOME, "eDP-1", &snapshot, &Live::default(), None);
        let shown: Vec<_> = tabs
            .iter()
            .map(|t| (t.id.as_str(), t.slot, t.shown))
            .collect();
        assert_eq!(
            shown,
            [
                ("home:2", Some(2), false),
                ("home:3", Some(3), true),
                ("home:12", None, false)
            ]
        );
    }
}
