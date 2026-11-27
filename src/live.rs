//! Live state assembled from Hyprland, terminals and `spaces.json`. Nothing is kept between commands: each call reads Hyprland
//! anew (`snapshot`), resolves terminals (`live_from`), and folds in the store
//! (`rows`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::browser;
use crate::folder::Folder;
use crate::hypr::{self, Hypr};
use crate::layout::Rect;
use crate::monitors::Blocks;
use crate::name::{self, HOME, Location};
use crate::paths;
use crate::spaces::{self, Store};
use crate::state::{self, Last};
use crate::status::{self, Status};
use crate::terminal;

/// A terminal window in a space, the folder its shell is in and what it
/// runs.
#[derive(Debug, Clone)]
pub(crate) struct Terminal {
    pub address: String,
    /// The tab it's on; 0 for a `home` workspace that has no number.
    pub tab: u8,
    pub pid: u32,
    pub shell: terminal::Shell,
}

/// A window on one of a space's workspaces, `home`'s included, which may be
/// a terminal.
#[derive(Debug, Clone)]
pub(crate) struct SpaceWindow {
    pub address: String,
    /// The process that runs it, and which of its windows it is.
    pub place: terminal::Place,
    pub space: String,
    /// The tab it's on, by its key.
    pub tab: Option<u8>,
    pub class: String,
    pub title: String,
    pub rect: Rect,
    pub floating: bool,
    pub fullscreen: u8,
    pub grouped: Vec<String>,
    /// How long ago it had focus.
    pub focus: i64,
}

/// A monitor and the workspace it shows.
#[derive(Debug, Clone)]
pub(crate) struct Screen {
    pub name: String,
    /// Whether it's the one you are on.
    pub focused: bool,
    pub refresh: f64,
    pub shows: Location,
}

/// What a session saves about a tab, besides its windows.
#[derive(Debug, Default)]
pub(crate) struct TabPlace {
    pub monitor: String,
    /// Its monitor's top left corner.
    pub corner: (i32, i32),
    pub dwindle: bool,
}

/// What Hyprland says about every space, before any terminal's folder is
/// read: the monitors, and the windows and workspaces each space holds, all
/// as of one moment. Where you are follows from it rather than being kept
/// beside it.
#[derive(Debug, Default)]
pub(crate) struct Snapshot {
    pub screens: Vec<Screen>,
    /// Which block of ten tabs is each monitor's.
    pub blocks: Blocks,
    windows: BTreeMap<String, u32>,
    /// Each space's tabs, by their keys.
    workspaces: BTreeMap<String, BTreeSet<u8>>,
    /// The windows on spaces' workspaces, `home`'s included.
    pub space_windows: Vec<SpaceWindow>,
    /// Every window, spaces' or not, for the browser's to be told apart.
    pub browser_windows: Vec<browser::Window>,
    /// Where the tabs are, by space and key.
    pub places: BTreeMap<(String, u8), TabPlace>,
    /// How far a window keeps from its tile's edge.
    pub border: i32,
    /// What grouped windows give up at the top for the group's bar.
    pub group_bar: i32,
}

/// The snapshot, which of its windows are terminals and where, and what
/// the agents in them are doing.
#[derive(Default)]
pub(crate) struct Live {
    pub active: Option<String>,
    windows: BTreeMap<String, u32>,
    workspaces: BTreeMap<String, BTreeSet<u8>>,
    pub terminals: BTreeMap<String, Vec<Terminal>>,
    /// Every window's address, by space.
    addresses: BTreeMap<String, Vec<String>>,
    /// Agents' statuses, by window address.
    pub statuses: HashMap<String, Status>,
}

impl Live {
    /// Each window's status from what its agent reported, `seen` saying
    /// whether you've seen its turn end.
    pub fn set_statuses(&mut self, seen: impl Fn(&terminal::Agent) -> bool) {
        self.statuses = self
            .terminals
            .values()
            .flatten()
            .filter_map(|t| Some((t.address.as_str(), t.shell.agent.as_ref()?)))
            .filter_map(|(address, agent)| {
                let status = status::from_report(&agent.word, seen(agent))?;
                Some((address.to_string(), status))
            })
            .collect();
    }

    /// The agents of the windows on screen, by window address.
    pub fn agents(&self) -> impl Iterator<Item = (&str, &terminal::Agent)> {
        self.terminals
            .values()
            .flatten()
            .filter_map(|t| Some((t.address.as_str(), t.shell.agent.as_ref()?)))
    }

    /// The same, to bring up to date.
    pub fn agents_mut(&mut self) -> impl Iterator<Item = &mut terminal::Agent> {
        self.terminals
            .values_mut()
            .flatten()
            .filter_map(|t| t.shell.agent.as_mut())
    }

    /// The spaces Hyprland has, `home` aside.
    pub fn spaces(&self) -> BTreeSet<String> {
        space_ids(&self.workspaces, &self.windows)
    }
}

impl Snapshot {
    /// The workspace you are on.
    fn here(&self) -> Option<&Location> {
        self.screens.iter().find(|s| s.focused).map(|s| &s.shows)
    }

    /// The space you are in.
    pub fn active(&self) -> Option<&str> {
        self.here().map(Location::space)
    }

    /// The tab you are on.
    pub fn active_tab(&self) -> Option<u8> {
        self.here().and_then(Location::key)
    }

    /// The spaces Hyprland has, `home` aside.
    pub fn spaces(&self) -> BTreeSet<String> {
        space_ids(&self.workspaces, &self.windows)
    }

    /// A space's windows, `home`'s included, by tab and then address, so that
    /// the same windows are always listed in the same order.
    pub fn windows_of(&self, space: &str) -> Vec<&SpaceWindow> {
        let mut windows: Vec<&SpaceWindow> = self
            .space_windows
            .iter()
            .filter(|window| window.space == space)
            .collect();
        windows.sort_by(|a, b| (a.tab, &a.address).cmp(&(b.tab, &b.address)));
        windows
    }

    #[cfg(test)]
    pub(crate) fn add_space_for_test(&mut self, id: &str) {
        self.workspaces.entry(id.to_string()).or_default().insert(1);
    }
}

/// The spaces with a workspace or a window, `home` aside.
fn space_ids<A, B>(
    workspaces: &BTreeMap<String, A>,
    windows: &BTreeMap<String, B>,
) -> BTreeSet<String> {
    workspaces
        .keys()
        .chain(windows.keys())
        .filter(|space| *space != HOME)
        .cloned()
        .collect()
}

pub(crate) fn snapshot(hypr: &Hypr) -> Result<Snapshot> {
    let mut snapshot = Snapshot {
        border: hypr.option("general:border_size").unwrap_or(0),
        blocks: Blocks::load(),
        ..Default::default()
    };
    let hypr::Everything {
        monitors,
        workspaces,
        clients,
    } = hypr.everything()?;
    snapshot.screens = monitors
        .iter()
        .filter_map(|monitor| {
            let shows = monitor.shows.as_ref()?;
            Some(Screen {
                name: monitor.name.clone(),
                focused: monitor.focused,
                refresh: monitor.refresh,
                shows: name::locate(shows.id, &shows.name),
            })
        })
        .collect();
    for ws in workspaces {
        // Special workspaces (scratchpad and the like) belong to no space.
        if ws.name.starts_with("special:") {
            continue;
        }
        let location = name::locate(ws.id, &ws.name);
        let entry = snapshot
            .workspaces
            .entry(location.space().to_string())
            .or_default();
        if let Some(tab) = location.key() {
            entry.insert(tab);
            let monitor = monitors.iter().find(|m| m.name == ws.monitor);
            snapshot.places.insert(
                (location.space().to_string(), tab),
                TabPlace {
                    corner: monitor.map_or((0, 0), |m| (m.x, m.y)),
                    dwindle: ws.layout == "dwindle",
                    monitor: ws.monitor,
                },
            );
        }
    }
    if clients.iter().any(|c| c.grouped.len() > 1) {
        snapshot.group_bar = group_bar(hypr);
    }
    let mut whose = terminal::places(&clients);
    for client in clients {
        if let Ok(pid) = u32::try_from(client.pid) {
            snapshot.browser_windows.push(browser::Window {
                address: client.address.clone(),
                pid,
                class: client.class.clone(),
                title: client.title.clone(),
                opened: client.opened(),
            });
        }
        // Windows not on any workspace yet report id -1 and no name.
        if client.workspace.name.is_empty() || client.workspace.name.starts_with("special:") {
            continue;
        }
        let location = name::locate(client.workspace.id, &client.workspace.name);
        *snapshot
            .windows
            .entry(location.space().to_string())
            .or_default() += 1;
        if let Some(place) = whose.remove(&client.address) {
            snapshot.space_windows.push(SpaceWindow {
                address: client.address,
                place,
                space: location.space().to_string(),
                tab: location.key(),
                class: client.class,
                title: client.title,
                rect: Rect {
                    x: client.at.0,
                    y: client.at.1,
                    w: client.size.0,
                    h: client.size.1,
                },
                floating: client.floating,
                fullscreen: client.fullscreen,
                grouped: client.grouped,
                focus: client.focus,
            });
        }
    }
    Ok(snapshot)
}

/// How high a group's bar is, as Hyprland works it out. A stacked one depends
/// on how many windows it has, and isn't accounted for.
fn group_bar(hypr: &Hypr) -> i32 {
    let option = |name: &str| hypr.option(&format!("group:groupbar:{name}")).unwrap_or(0);
    if option("enabled") == 0 || option("stacked") == 1 {
        return 0;
    }
    let text = option("gradients") == 1 || option("render_titles") == 1;
    option("gaps_out") * (1 + option("keep_upper_gap"))
        + option("indicator_height")
        + option("indicator_gap")
        + if text { option("height") } else { 0 }
}

/// Makes the live view from a snapshot, with `shell` saying which windows
/// are terminals, which folder each one's shell is in and what it runs.
pub(crate) fn live_from(
    snapshot: &Snapshot,
    mut shell: impl FnMut(&SpaceWindow) -> Option<terminal::Shell>,
) -> Live {
    let mut terminals: BTreeMap<String, Vec<Terminal>> = BTreeMap::new();
    let mut addresses: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for window in &snapshot.space_windows {
        addresses
            .entry(window.space.clone())
            .or_default()
            .push(window.address.clone());
        if let Some(shell) = shell(window) {
            terminals
                .entry(window.space.clone())
                .or_default()
                .push(Terminal {
                    address: window.address.clone(),
                    tab: window.tab.unwrap_or(0),
                    pid: window.place.pid,
                    shell,
                });
        }
    }
    // Oldest first, as far as Hyprland lets us tell: by tab, then process.
    for list in terminals.values_mut() {
        list.sort_by_key(|t| (t.tab, t.pid));
    }
    Live {
        active: snapshot.active().map(str::to_string),
        windows: snapshot.windows.clone(),
        workspaces: snapshot.workspaces.clone(),
        terminals,
        addresses,
        statuses: HashMap::new(),
    }
}

/// What Hyprland said, and what was made of it.
pub(crate) type Seen = (Snapshot, Live);

pub(crate) fn inspect(hypr: &Hypr) -> Result<Seen> {
    let snapshot = snapshot(hypr)?;
    let mut live = live_from(&snapshot, |window| terminal::shell(window.place, None));
    // Only `watch` follows agents, to know which are done, and seen.
    live.set_statuses(|_| true);
    Ok((snapshot, live))
}

fn live(hypr: &Hypr) -> Result<Seen> {
    inspect(hypr)
}

/// One space, with what's known about it. What's printed of it is up to
/// `view.rs` and `list.rs`.
#[derive(Debug)]
pub struct Row {
    /// What Hyprland's workspaces are named after: `s1` for `s1:1`.
    pub id: String,
    /// What to call it: the name you gave it, or its folder's.
    pub label: String,
    /// Whether you gave it the name; otherwise it follows its folder.
    pub named: bool,
    /// The glyph or emoji you picked for its badge, if any.
    pub icon: Option<String>,
    /// Opaque external metadata attached to the space.
    pub meta: BTreeMap<String, serde_json::Value>,
    /// The checkout (or folder) its terminal is in. `None` for `home`, and
    /// for a space that never had a terminal.
    pub folder: Option<Folder>,
    pub active: bool,
    /// `None` when Hyprland can't be reached.
    pub windows: Option<u32>,
    /// What the space's workspaces that exist right now are numbered: 1-10 on
    /// the first monitor, 11-20 on the next.
    pub workspaces: Option<Vec<u8>>,
    /// The workspace `space go` returns to.
    pub last: u8,
    /// When the space was last left, in Unix milliseconds, or 0 if never.
    /// Larger is more recent; the sidebar starts on the most recent one.
    pub left_at: u64,
    /// Its terminal windows, oldest first. Empty without Hyprland.
    pub terminals: Vec<TerminalRow>,
    /// What most needs you among the agents in its windows, if any is
    /// working, done or waiting.
    pub status: Option<Status>,
    /// The names you gave its tabs, by what their workspaces are numbered.
    pub tabs: BTreeMap<u8, String>,
}

/// A terminal window, as the bar names its tab.
#[derive(Debug)]
pub struct TerminalRow {
    /// Hyprland's address for the window: `0x55a4...`.
    pub address: String,
    /// The tab it's on, by its key: what its workspace is numbered. `None` on
    /// a workspace without a number.
    pub tab: Option<u8>,
    /// The folder its shell is in.
    pub folder: Folder,
    /// What runs in it (`claude`, `nvim`), or `None` at the prompt.
    pub program: Option<String>,
    /// What its tab shows: the program, or else the folder's name.
    pub label: String,
    /// What the agent in it is doing, if one reports it and isn't idle.
    pub status: Option<Status>,
}

impl TerminalRow {
    fn new(t: &Terminal, live: &Live) -> Self {
        Self {
            address: t.address.clone(),
            tab: (t.tab > 0).then_some(t.tab),
            folder: t.shell.folder.clone(),
            program: t.shell.program.clone(),
            label: t.shell.label(),
            status: live.statuses.get(&t.address).copied(),
        }
    }
}

/// Brings what the store knows about a space up to date with its terminals,
/// and says what to call it and where it is. A space follows one terminal,
/// its first, for as long as that window lives, then the next oldest; with
/// none left it keeps the last folder it saw.
fn describe(
    id: &str,
    entry: &mut spaces::Entry,
    terminals: &[Terminal],
) -> (String, Option<Folder>) {
    let followed = entry
        .anchor
        .as_ref()
        .and_then(|anchor| terminals.iter().find(|t| &t.address == anchor))
        .or_else(|| terminals.first());
    if let Some(terminal) = followed {
        entry.anchor = Some(terminal.address.clone());
        entry.folder = Some(terminal.shell.folder.clone());
    } else {
        entry.anchor = None;
    }
    let folder = entry.folder.as_ref().map(Folder::checkout);
    let label = match (&entry.name, &folder) {
        (Some(name), _) => name.clone(),
        (None, Some(folder)) => folder.name(),
        (None, None) => id.to_string(),
    };
    (label, folder)
}

/// Every space: `home` first, then the rest in the order they were made.
/// Without `live` (no Hyprland), the spaces the store remembers. Updates the
/// store: new spaces, the terminals they follow, and gone ones pruned.
pub(crate) fn rows(store: &mut Store, live: Option<&Live>, last: &Last) -> Vec<Row> {
    let ids: Vec<String> = match live {
        Some(live) => {
            let spaces = live.spaces();
            store.prune(&spaces);
            for id in &spaces {
                let open = live.workspaces.get(id);
                let entry = store.entry(id);
                if entry.order == 0 {
                    entry.order = state::now();
                }
                // A tab's name goes with its tab.
                entry
                    .tabs
                    .retain(|tab, _| open.is_some_and(|open| open.contains(tab)));
            }
            if let Some(home) = store.spaces.get_mut(HOME) {
                let open = live.workspaces.get(HOME);
                home.tabs
                    .retain(|tab, _| open.is_some_and(|open| open.contains(tab)));
            }
            let mut ids: Vec<String> = spaces.into_iter().collect();
            ids.sort_by_key(|id| (store.get(id).map_or(0, |e| e.order), id.clone()));
            ids
        }
        None => {
            let mut ids: Vec<String> = store
                .spaces
                .keys()
                .filter(|id| *id != HOME)
                .cloned()
                .collect();
            ids.sort_by_key(|id| (store.get(id).map_or(0, |e| e.order), id.clone()));
            ids
        }
    };

    let row = |id: &str,
               label: String,
               named: bool,
               icon: Option<String>,
               meta: BTreeMap<String, serde_json::Value>,
               folder: Option<Folder>,
               tabs| {
        let visit = last.get(id);
        Row {
            id: id.to_string(),
            label,
            named,
            icon,
            meta,
            folder,
            active: live.is_some_and(|l| l.active.as_deref() == Some(id)),
            windows: live.map(|l| l.windows.get(id).copied().unwrap_or(0)),
            workspaces: live.map(|l| {
                l.workspaces
                    .get(id)
                    .map(|slots| slots.iter().copied().collect())
                    .unwrap_or_default()
            }),
            last: visit.map(|v| v.key).unwrap_or(1),
            left_at: visit.map(|v| v.at).unwrap_or(0),
            terminals: live
                .and_then(|l| Some((l, l.terminals.get(id)?)))
                .map(|(l, list)| list.iter().map(|t| TerminalRow::new(t, l)).collect())
                .unwrap_or_default(),
            status: live.and_then(|l| {
                l.addresses
                    .get(id)?
                    .iter()
                    .filter_map(|address| l.statuses.get(address).copied())
                    .max()
            }),
            tabs,
        }
    };

    let home_entry = store.get(HOME);
    let home_tabs = home_entry.map(|e| e.tabs.clone()).unwrap_or_default();
    let home_meta = home_entry.map(|e| e.meta.clone()).unwrap_or_default();
    let mut out = vec![row(
        HOME,
        HOME.to_string(),
        false,
        None,
        home_meta,
        None,
        home_tabs,
    )];
    for id in ids {
        let terminals = live
            .and_then(|l| l.terminals.get(&id))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let entry = store.entry(&id);
        let named = entry.name.is_some();
        let (label, folder) = match live {
            Some(_) => describe(&id, entry, terminals),
            // Without Hyprland, as last seen, changing nothing.
            None => describe(&id, &mut entry.clone(), &[]),
        };
        let icon = entry.icon.clone();
        let meta = entry.meta.clone();
        let tabs = entry.tabs.clone();
        out.push(row(&id, label, named, icon, meta, folder, tabs));
    }
    out
}

/// Reads the spaces and saves what changed about them. A failure to save is
/// only a warning: the list itself is right.
pub(crate) fn load_rows(hypr: Option<&Hypr>) -> Result<(Vec<Row>, Option<Seen>)> {
    let (_, rows, live) = lock_rows(hypr)?;
    Ok((rows, live))
}

/// `load_rows`, with the store it read still locked, to change it further.
/// The windows and their terminals are read before it's locked, so a slow one
/// holds up only this command.
pub(crate) fn lock_rows(hypr: Option<&Hypr>) -> Result<(Store, Vec<Row>, Option<Seen>)> {
    let state_dir = paths::state_dir();
    let mut known = Store::load(&state_dir)?.spaces;
    loop {
        let live = match hypr {
            Some(hypr) => Some(live(hypr)?),
            None => None,
        };
        let mut store = Store::lock(&state_dir)?;
        // Another command changed the spaces meanwhile, perhaps adding one
        // that `live` is too old to have, which it would then prune.
        if store.spaces != known {
            known = store.spaces;
            continue;
        }
        let last = Last::load(&state_dir)?;
        let rows = rows(&mut store, live.as_ref().map(|(_, live)| live), &last);
        if let Err(e) = store.save() {
            eprintln!("space: warning: {e:#}");
        }
        return Ok((store, rows, live));
    }
}

/// The space `target` means: `home`, an id, or what a space is called (its
/// name, or its folder's), exactly or else ignoring case.
pub(crate) fn resolve(rows: &[Row], target: &str) -> Result<String> {
    if let Some(row) = rows.iter().find(|r| r.id == target) {
        return Ok(row.id.clone());
    }
    for same in [
        (|a: &str, b: &str| a == b) as fn(&str, &str) -> bool,
        |a, b| a.to_lowercase() == b.to_lowercase(),
    ] {
        let found: Vec<&Row> = rows.iter().filter(|r| same(&r.label, target)).collect();
        match found.as_slice() {
            [] => continue,
            [row] => return Ok(row.id.clone()),
            many => bail!(
                "{} spaces are called '{target}'; pick one by id: {}",
                many.len(),
                many.iter()
                    .map(|r| r.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
    let mut message = format!("no space called '{target}'");
    if let Some(close) = name::suggest(target, rows.iter().map(|r| r.label.as_str())) {
        message.push_str(&format!(". Did you mean {close}?"));
    }
    bail!(message)
}

/// The folder you are in: the focused terminal's, else the active space's,
/// else home.
pub(crate) fn here_folder(hypr: &Hypr) -> Result<PathBuf> {
    if let Some(address) = hypr.active_window_address()?
        && let Some(place) = terminal::places(&hypr.clients()?).get(&address)
        && let Some(folder) = terminal::shell_folder(*place)
    {
        return Ok(folder.path().to_path_buf());
    }
    let (store, rows, _) = lock_rows(Some(hypr))?;
    Ok(rows
        .iter()
        .find(|r| r.active)
        .and_then(|r| store.get(&r.id))
        .and_then(|entry| Some(entry.folder.as_ref()?.path()))
        .filter(|dir| dir.is_dir())
        .map_or_else(paths::home, Path::to_path_buf))
}

/// The folder for a command without an explicit `--dir`: the caller's terminal
/// (or space) if run from inside a space window, else `here_folder`. Also
/// returns the resolved space id when known.
pub(crate) fn caller_folder(hypr: &Hypr) -> Result<(PathBuf, Option<String>)> {
    let (snapshot, live) = inspect(hypr)?;
    if let Some(caller) = crate::context::resolve(&snapshot, Some(&live)) {
        if let Some(window) = &caller.window
            && let Some(dir) = live
                .terminals
                .get(&caller.space)
                .and_then(|list| list.iter().find(|t| &t.address == window))
                .map(|t| t.shell.folder.path())
                .filter(|dir| dir.is_dir())
        {
            return Ok((dir.to_path_buf(), Some(caller.space)));
        }
        let dir = space_folder(hypr, &caller.space)?;
        return Ok((dir, Some(caller.space)));
    }
    let space = snapshot.active().map(str::to_string);
    Ok((here_folder(hypr)?, space))
}

/// The folder a space's new tab opens in: the space's own, its checkout's
/// when it's in one, else the folder you are in.
pub(crate) fn space_folder(hypr: &Hypr, space: &str) -> Result<PathBuf> {
    let (rows, _) = load_rows(Some(hypr))?;
    match rows
        .iter()
        .find(|r| r.id == space)
        .and_then(|r| Some(r.folder.as_ref()?.path()))
        .filter(|dir| dir.is_dir())
    {
        Some(dir) => Ok(dir.to_path_buf()),
        None => here_folder(hypr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal(address: &str, tab: u8, pid: u32, cwd: &str) -> Terminal {
        Terminal {
            address: address.into(),
            tab,
            pid,
            shell: terminal::Shell {
                pid: 1,
                folder: Folder::here(cwd),
                program: None,
                agent: None,
                conversation: None,
            },
        }
    }

    #[test]
    fn a_space_follows_its_first_terminal() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("ren");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        let (repo_s, src_s, notes_s) = (
            repo.to_str().unwrap(),
            repo.join("src").to_str().unwrap().to_string(),
            notes.to_str().unwrap(),
        );

        let mut entry = spaces::Entry::default();
        let (label, folder) = describe("s1", &mut entry, &[]);
        assert_eq!(
            (label.as_str(), folder),
            ("s1", None),
            "never had a terminal"
        );

        // The oldest terminal names it; inside a checkout, the checkout.
        let terminals = [
            terminal("0xa", 1, 10, &src_s),
            terminal("0xb", 1, 20, notes_s),
        ];
        let (label, folder) = describe("s1", &mut entry, &terminals);
        assert_eq!(label, "ren");
        assert_eq!(folder, Some(Folder::here(repo_s)));
        assert_eq!(entry.anchor.as_deref(), Some("0xa"));

        // It keeps following that window even when it's no longer first.
        let terminals = [
            terminal("0xc", 1, 5, notes_s),
            terminal("0xa", 2, 10, notes_s),
        ];
        assert_eq!(describe("s1", &mut entry, &terminals).0, "notes");
        assert_eq!(entry.anchor.as_deref(), Some("0xa"));

        // When it closes, the next oldest; with none, the last folder seen.
        let terminals = [terminal("0xc", 1, 5, repo_s)];
        assert_eq!(describe("s1", &mut entry, &terminals).0, "ren");
        assert_eq!(describe("s1", &mut entry, &[]).0, "ren");
        assert_eq!(entry.anchor, None);

        // A name you give wins over the folder.
        entry.name = Some("Japanese".into());
        assert_eq!(describe("s1", &mut entry, &terminals).0, "Japanese");
    }

    fn row(id: &str, label: &str) -> Row {
        Row {
            id: id.into(),
            label: label.into(),
            named: false,
            icon: None,
            meta: BTreeMap::new(),
            folder: None,
            active: false,
            windows: None,
            workspaces: None,
            last: 1,
            left_at: 0,
            terminals: Vec::new(),
            status: None,
            tabs: BTreeMap::new(),
        }
    }

    #[test]
    fn resolves_ids_and_names() {
        let rows = [
            row("home", "home"),
            row("s1", "ren"),
            row("s2", "Mail"),
            row("s3", "mail"),
        ];
        assert_eq!(resolve(&rows, "s1").unwrap(), "s1");
        assert_eq!(resolve(&rows, "ren").unwrap(), "s1");
        assert_eq!(resolve(&rows, "home").unwrap(), "home");
        assert_eq!(resolve(&rows, "Mail").unwrap(), "s2", "exact case first");
        assert_eq!(resolve(&rows, "REN").unwrap(), "s1");
        let err = resolve(&rows, "MAIL").unwrap_err().to_string();
        assert!(
            err.contains("2 spaces are called 'MAIL'") && err.contains("s2, s3"),
            "{err}"
        );
        let err = resolve(&rows, "rne").unwrap_err().to_string();
        assert!(err.contains("Did you mean ren?"), "{err}");
    }
}
