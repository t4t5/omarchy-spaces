//! What `space ls --json` prints, for scripts. It's theirs to rely on, so it's
//! apart from what the plugin reads (`view.rs`), which changes as the bar and
//! the sidebar do, and from how spaces are kept.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::folder::{self, Folder};
use crate::live::{Live, Row, Snapshot, TerminalRow};
use crate::name;
use crate::status::Status;
use crate::tabs;

/// Goes up when it changes in a way that breaks a script made for the last
/// one; a new field doesn't.
pub(crate) const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct List {
    pub version: u32,
    /// `home` first, then in the sidebar's order.
    pub spaces: Vec<Space>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Space {
    /// What commands take it by.
    pub id: String,
    /// What to call it: the name you gave it, or its folder's.
    pub label: String,
    /// Whether you gave it the name; otherwise it follows its folder.
    pub named: bool,
    /// The glyph or emoji you picked for its badge, if any.
    pub icon: Option<String>,
    /// Opaque external metadata attached to the space.
    pub meta: BTreeMap<String, serde_json::Value>,
    /// The checkout (or folder) its terminal is in. `null` for `home`, and
    /// for a space that never had a terminal.
    #[serde(flatten, serialize_with = "folder::or_null")]
    pub folder: Option<Folder>,
    /// Whether it's the one you are in.
    pub active: bool,
    /// `null` when Hyprland can't be reached.
    pub windows: Option<u32>,
    /// What most needs you among the agents in its windows, if any is
    /// working, done or waiting.
    pub status: Option<Status>,
    /// When it was last left, in Unix milliseconds, or `null` if never.
    pub left_at: Option<u64>,
    /// Its tabs, monitor by monitor, in the order they show. `null` when
    /// Hyprland can't be reached.
    pub tabs: Option<Vec<Tab>>,
    /// Its terminal windows, oldest first.
    pub terminals: Vec<Terminal>,
    /// Every window it has, by tab and then address, with its title and app.
    /// Empty when Hyprland can't be reached.
    pub clients: Vec<Client>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Tab {
    /// What commands take it by. Not to be taken apart, and another once the
    /// tab is moved.
    pub id: String,
    /// The number it shows on its monitor, 1-10. `null` for none.
    pub slot: Option<u8>,
    /// The monitor it's on, by name.
    pub monitor: String,
    /// The name you gave it, else what its last used window is called. Empty
    /// for an empty tab, and for one of `home`'s that you haven't named.
    pub label: String,
    /// Whether you gave it the name.
    pub named: bool,
    /// Whether its monitor shows it.
    pub shown: bool,
    pub windows: u32,
    /// What most needs you among the agents in its windows.
    pub status: Option<Status>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Terminal {
    /// Hyprland's address for the window: `0x55a4...`.
    pub address: String,
    /// The tab it's on, by its id. `null` on a workspace without a number.
    pub tab: Option<String>,
    /// The folder its shell is in.
    #[serde(flatten)]
    pub folder: Folder,
    /// What runs in it (`claude`, `nvim`), or `null` at the prompt.
    pub program: Option<String>,
    /// What its tab shows: the program, or else the folder's name.
    pub label: String,
    /// What the agent in it is doing, if one reports it and isn't idle.
    pub status: Option<Status>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Client {
    /// Hyprland's address for the window: `0x55a4...`.
    pub address: String,
    /// The tab it's on, by its id. `null` on a workspace without a number.
    pub tab: Option<String>,
    /// The window's app, as Hyprland's `class` says.
    pub class: String,
    /// What the window is titled now.
    pub title: String,
}

/// The spaces in `rows`, with their tabs if Hyprland can be reached.
pub(crate) fn list(rows: &[Row], hyprland: Option<(&Snapshot, &Live)>) -> List {
    List {
        version: VERSION,
        spaces: rows.iter().map(|row| space(row, hyprland)).collect(),
    }
}

fn space(row: &Row, hyprland: Option<(&Snapshot, &Live)>) -> Space {
    Space {
        id: row.id.clone(),
        label: row.label.clone(),
        named: row.named,
        icon: row.icon.clone(),
        meta: row.meta.clone(),
        folder: row.folder.clone(),
        active: row.active,
        windows: row.windows,
        status: row.status,
        left_at: (row.left_at > 0).then_some(row.left_at),
        tabs: hyprland.map(|(snapshot, live)| tabs_of(row, snapshot, live)),
        terminals: row.terminals.iter().map(|t| terminal(&row.id, t)).collect(),
        clients: hyprland
            .map(|(snapshot, _)| clients_of(&row.id, snapshot))
            .unwrap_or_default(),
    }
}

fn clients_of(space: &str, snapshot: &Snapshot) -> Vec<Client> {
    snapshot
        .windows_of(space)
        .into_iter()
        .map(|w| Client {
            address: w.address.clone(),
            tab: w.tab.map(|key| name::tab_id(space, key)),
            class: w.class.clone(),
            title: w.title.clone(),
        })
        .collect()
}

fn tabs_of(row: &Row, snapshot: &Snapshot, live: &Live) -> Vec<Tab> {
    let screens = snapshot.screens.iter().map(|screen| screen.name.as_str());
    let places = snapshot.places.values().map(|place| place.monitor.as_str());
    let mut seen = BTreeSet::new();
    screens
        .chain(places)
        .filter(|monitor| seen.insert(*monitor))
        .flat_map(|monitor| {
            let tabs = tabs::on_monitor(&row.id, monitor, snapshot, live, Some(row));
            tabs.into_iter().map(move |tab| Tab {
                id: tab.id,
                slot: tab.slot,
                monitor: monitor.to_string(),
                label: tab.label,
                named: tab.named,
                shown: tab.shown,
                windows: tab.windows,
                status: tab.status,
            })
        })
        .collect()
}

fn terminal(space: &str, terminal: &TerminalRow) -> Terminal {
    Terminal {
        address: terminal.address.clone(),
        tab: terminal.tab.map(|key| name::tab_id(space, key)),
        folder: terminal.folder.clone(),
        program: terminal.program.clone(),
        label: terminal.label.clone(),
        status: terminal.status,
    }
}
