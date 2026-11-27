//! Capturing the running desktop into a `Session` (`session_of`) and opening a
//! saved session back up after a restart (`restore`).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::browser::{Browser, Browsers};
use crate::commands::{
    fresh_space, live_selector, remember, session_selector, session_workspace, wait_for_window,
};
use crate::config::Config;
use crate::folder::Folder;
use crate::hypr::{self, Hypr};
use crate::launch;
use crate::layout::{self, Rect, Tree};
use crate::live::{Live, Row, Snapshot, SpaceWindow};
use crate::monitors::Blocks;
use crate::name::{self, HOME};
use crate::paths;
use crate::reserve;
use crate::session::{self, Session};
use crate::shell;
use crate::spaces::Store;
use crate::state;
use crate::terminal;

/// How long a restored terminal has to show up before the next one opens:
/// longer than usual, as everything is slow right after logging in.
const RESTORE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// How long a browser has to show its window: it may be starting.
const BROWSER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// The browser's windows in sessions, latest first, by address.
pub(crate) fn known_browsers(sessions: &[&Session]) -> HashMap<String, Browser> {
    sessions
        .iter()
        .rev()
        .flat_map(|session| &session.spaces)
        .flat_map(|space| &space.windows)
        .filter_map(|w| Some((w.address.clone(), w.browser()?.clone())))
        .collect()
}

/// The session to save: every space with a terminal or a browser's window, in
/// the sidebar's order.
/// `before` are the ones before it, latest first, for how windows were
/// arranged before one of them went fullscreen.
pub(crate) fn session_of(
    instance: &str,
    rows: &[Row],
    live: &Live,
    snapshot: &Snapshot,
    browsers: &Browsers,
    before: &[&Session],
) -> Session {
    let spaces = rows
        .iter()
        .filter_map(|row| {
            let terminals = live.terminals.get(&row.id).into_iter().flatten();
            let mut windows: Vec<session::Window> = terminals
                .filter_map(|t| {
                    let window = snapshot
                        .space_windows
                        .iter()
                        .find(|w| w.address == t.address)?;
                    let conversation = t.shell.conversation.clone();
                    let folder = match &conversation {
                        Some(conversation) => Folder::here(&conversation.folder),
                        None => t.shell.folder.clone(),
                    };
                    let kind = session::WindowKind::Terminal(session::Terminal {
                        folder: Some(folder),
                        conversation,
                    });
                    session_window(snapshot, window, kind, before)
                })
                .collect();
            for window in snapshot.space_windows.iter().filter(|w| w.space == row.id) {
                if let Some(browser) = browsers.get(&window.address)
                    && let Some(saved) = session_window(
                        snapshot,
                        window,
                        session::WindowKind::Browser(browser.clone()),
                        before,
                    )
                {
                    windows.push(saved);
                }
            }
            if windows.is_empty() {
                return None;
            }
            let tabs: BTreeSet<u8> = windows.iter().map(|w| w.tab).collect();
            let layouts = tabs
                .into_iter()
                .map(|tab| {
                    (
                        tab,
                        session_layout(snapshot, &row.id, tab, &windows, before),
                    )
                })
                .collect();
            let last = if row.active {
                snapshot.active_tab()
            } else {
                (row.id != HOME).then_some(row.last)
            };
            Some(session::Space {
                home: row.id == HOME,
                id: (row.id != HOME).then(|| row.id.clone()),
                name: row.named.then(|| row.label.clone()),
                icon: row.icon.clone(),
                note: row.note.clone(),
                meta: row.meta.clone(),
                active: row.active,
                last,
                tabs: row.tabs.clone(),
                layouts,
                folder: row.folder.clone(),
                windows,
            })
        })
        .collect();
    Session {
        instance: instance.to_string(),
        spaces,
    }
}

/// Where a window is, as a session has it.
pub(crate) fn session_window(
    snapshot: &Snapshot,
    window: &SpaceWindow,
    kind: session::WindowKind,
    before: &[&Session],
) -> Option<session::Window> {
    let tab = window.tab?;
    let corner = snapshot
        .places
        .get(&(window.space.clone(), tab))
        .map_or((0, 0), |place| place.corner);
    Some(session::Window {
        tab,
        kind,
        address: window.address.clone(),
        floating: match window.fullscreen {
            _ if !window.floating => None,
            0 => Some(Rect {
                x: window.rect.x - corner.0,
                y: window.rect.y - corner.1,
                ..window.rect
            }),
            // It says where the screen is, not where it goes back to.
            _ => before
                .iter()
                .find_map(|session| session.find(&window.address)?.1.floating),
        },
        fullscreen: window.fullscreen,
    })
}

/// How a tab's windows are arranged: how its tiles are split, less those that
/// aren't saved.
pub(crate) fn session_layout(
    snapshot: &Snapshot,
    space: &str,
    tab: u8,
    saved: &[session::Window],
    before: &[&Session],
) -> session::Layout {
    let on_tab = |w: &&SpaceWindow| w.space == space && w.tab == Some(tab);
    let index = |address: &str| saved.iter().position(|t| t.address == address);
    let mut windows: Vec<&SpaceWindow> = snapshot.space_windows.iter().filter(on_tab).collect();
    // The one that shows comes first in its group.
    windows.sort_by_key(|w| w.focus);
    let place = snapshot.places.get(&(space.to_string(), tab));

    let tiled: Vec<&&SpaceWindow> = windows.iter().filter(|w| !w.floating).collect();
    let tiles = if !place.is_some_and(|p| p.dwindle) {
        None
    } else if tiled.iter().any(|w| w.fullscreen != 0) {
        // It says where the screen is, not its tile. What it's among hasn't
        // changed as long as the same windows are.
        before
            .iter()
            .find_map(|session| layout_before(session, tab, saved, &tiled))
    } else {
        let mut groups: Vec<(Vec<String>, Rect)> = Vec::new();
        for window in &tiled {
            match groups
                .iter_mut()
                .find(|(shown, _)| window.grouped.contains(&shown[0]))
            {
                Some((group, _)) => group.push(window.address.clone()),
                None => {
                    let mut rect = window.rect;
                    if window.grouped.len() > 1 {
                        rect.y -= snapshot.group_bar;
                        rect.h += snapshot.group_bar;
                    }
                    groups.push((vec![window.address.clone()], rect));
                }
            }
        }
        layout::find(&groups, snapshot.border)
            .and_then(|tree| tree.filter_map(&|address| index(address)))
    };
    session::Layout {
        tiles,
        focused: windows.iter().find_map(|w| index(&w.address)),
        monitor: place.map(|p| p.monitor.clone()).filter(|m| !m.is_empty()),
    }
}

/// How a tab's tiles were split before, if by the same windows.
fn layout_before(
    before: &Session,
    tab: u8,
    saved: &[session::Window],
    tiled: &[&&SpaceWindow],
) -> Option<Tree<usize>> {
    let there: BTreeSet<&str> = tiled
        .iter()
        .map(|w| w.address.as_str())
        .filter(|address| saved.iter().any(|t| t.address == *address))
        .collect();
    let (space, _) = before.find(there.first()?)?;
    let tiles = space.layouts.get(&tab)?.tiles.as_ref()?;
    let address = |window: &usize| space.windows.get(*window).map(|t| t.address.as_str());
    let were: BTreeSet<&str> = tiles.windows().into_iter().filter_map(address).collect();
    if were != there {
        return None;
    }
    tiles.filter_map(&|window| {
        let address = address(window)?;
        saved.iter().position(|t| t.address == address)
    })
}

/// `space restore`: the spaces that were up before Hyprland last restarted,
/// with their ids, in new windows arranged as they were, or those of an earlier session
/// kept in `file`. Each is struck off as it opens, so none opens twice. It
/// takes you back to the space you were in, unless you have started on
/// something else. The plugin runs it as it starts.
pub fn restore(file: Option<PathBuf>, auto: bool) -> Result<()> {
    // A mistake in the config shouldn't cost you your spaces.
    let config = Config::load_or_default();
    if auto && !config.restore.auto {
        return Ok(());
    }
    let hypr = Hypr::from_env()?;
    let state_dir = paths::state_dir();
    let Some(_lock) = session::lock_restore(&state_dir)? else {
        println!("Already restoring");
        return Ok(());
    };
    session::set_aside(&state_dir, &hypr.instance)?;
    if let Some(file) = file {
        session::set_previous_from(&state_dir, &file)?;
    }
    let Some(mut session) = session::previous(&state_dir)? else {
        println!("Nothing to restore");
        return Ok(());
    };

    let start = hypr.active_workspace()?;
    let mut opened: HashSet<String> = HashSet::new();
    // The terminals of every space first: a browser takes a while to start.
    let mut spaces: Vec<Restoring> = Vec::new();
    let now = state::now();
    while !session.spaces.is_empty() {
        let mut space = Restoring {
            addresses: vec![None; session.spaces[0].windows.len()],
            saved: session.spaces[0].clone(),
            id: String::new(),
            // In the sidebar, they keep their order.
            order: now + spaces.len() as u64,
        };
        if !space.saved.windows.is_empty() {
            space.open_terminals(&hypr, &config, &mut opened, || {
                let mut left = session.clone();
                left.spaces.remove(0);
                session::set_previous(&state_dir, &left)
            })?;
            if !space.has_browser(None) {
                space.note_restored(&hypr, config.restore.browsers)?;
            }
            for key in space.tabs() {
                if !space.has_browser(Some(key)) {
                    space.arrange(&hypr, key);
                }
            }
            if space.addresses.iter().any(Option::is_some) {
                space.describe()?;
            }
            spaces.push(space);
        }
        session.spaces.remove(0);
        session::set_previous(&state_dir, &session)?;
    }

    let active = spaces.iter().find(|space| space.saved.active);
    let back = active.and_then(Restoring::back);
    let mut landed = go_back(&hypr, &start, &opened, back.as_deref(), false)?;
    let mut broken = HashSet::new();
    for space in spaces.iter_mut().filter(|s| s.has_browser(None)) {
        if config.restore.browsers {
            space.open_browsers(&hypr, &mut opened, &mut broken);
        }
        space.note_restored(&hypr, config.restore.browsers)?;
        for key in space.tabs() {
            if space.has_browser(Some(key)) {
                space.arrange(&hypr, key);
            }
        }
        if space.addresses.iter().any(Option::is_some) {
            space.describe()?;
        }
        if space.saved.active && !landed {
            landed = go_back(&hypr, &start, &opened, space.back().as_deref(), true)?;
        }
    }
    let count = spaces.len();
    let plural = if count == 1 { "" } else { "s" };
    println!("Restored {count} space{plural}");
    Ok(())
}

/// Takes you to the tab you were on, unless you have started on something
/// else or, unless `empty` will do, nothing is open there yet. Whether it did.
fn go_back(
    hypr: &Hypr,
    start: &hypr::Workspace,
    opened: &HashSet<String>,
    back: Option<&str>,
    empty: bool,
) -> Result<bool> {
    let Some(back) = back else {
        return Ok(false);
    };
    let name = back.strip_prefix("name:").unwrap_or(back);
    let there = |ws: &hypr::Workspace| ws.name == name && ws.windows > 0;
    if !empty && !hypr.workspaces()?.iter().any(there) {
        return Ok(false);
    }
    if still_at(hypr, start, opened)? {
        // A tab that isn't there yet is made on the monitor you are on.
        let blocks = Blocks::load();
        let key = name::locate(back.parse().unwrap_or(0), name).key();
        if let Some(monitor) = key.and_then(|key| blocks.monitor(key))
            && monitor != start.monitor
        {
            hypr.dispatch(&hypr::focus_monitor(monitor))?;
        }
        let selector = hypr::selector_for(&hypr.workspaces()?, name);
        hypr.dispatch(&hypr::focus_workspace(&selector))?;
    }
    Ok(true)
}

/// Whether you are where you started, on a workspace that had nothing on it
/// and has nothing but what was `opened` now.
fn still_at(hypr: &Hypr, start: &hypr::Workspace, opened: &HashSet<String>) -> Result<bool> {
    let now = hypr.active_workspace()?;
    Ok(now.name == start.name
        && hypr
            .clients()?
            .iter()
            .filter(|c| c.workspace.name == now.name)
            .all(|c| opened.contains(&c.address)))
}

/// A space being restored.
struct Restoring {
    saved: session::Space,
    id: String,
    /// The windows of `saved`, those open by now.
    addresses: Vec<Option<String>>,
    order: u64,
}

impl Restoring {
    /// Whether any of its windows, or of those on a tab, is a browser's.
    fn has_browser(&self, tab: Option<u8>) -> bool {
        self.saved
            .windows
            .iter()
            .any(|w| w.browser().is_some() && tab.is_none_or(|tab| w.tab == tab))
    }

    fn tabs(&self) -> BTreeSet<u8> {
        self.saved.windows.iter().map(|w| w.tab).collect()
    }

    /// The tab you were on.
    fn back(&self) -> Option<String> {
        let first = self.saved.windows.first()?.tab;
        Some(session_selector(
            &[],
            &self.id,
            self.saved.last.unwrap_or(first),
        ))
    }

    /// Opens its terminals, calling `opening` once the first is on its way, or
    /// when there are none. Their windows are added to `opened`.
    fn open_terminals(
        &mut self,
        hypr: &Hypr,
        config: &Config,
        opened: &mut HashSet<String>,
        opening: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.id = match self.saved.windows.first() {
            Some(first) if !self.saved.home => {
                fresh_space(hypr, first.tab, self.saved.id.as_deref())?
            }
            _ => HOME.to_string(),
        };
        let mut opening = Some(opening);
        for n in 0..self.saved.windows.len() {
            let window = &self.saved.windows[n];
            let Some(terminal) = window.terminal() else {
                continue;
            };
            let token = format!("{}-{}-{n}", std::process::id(), self.id);
            let opening = || opening.take().map_or(Ok(()), |opening| opening());
            let address = reopen_terminal(
                hypr, config, &self.id, window.tab, terminal, &token, opening,
            )?;
            self.opened(n, address, opened);
        }
        opening.take().map_or(Ok(()), |opening| opening())
    }

    /// Opens its browser windows, but for `broken` browsers, which haven't
    /// shown one.
    fn open_browsers(
        &mut self,
        hypr: &Hypr,
        opened: &mut HashSet<String>,
        broken: &mut HashSet<String>,
    ) {
        for n in 0..self.saved.windows.len() {
            let window = &self.saved.windows[n];
            let Some(browser) = window.browser().filter(|b| !broken.contains(&b.name)) else {
                continue;
            };
            // Nothing it can open: no fault of the browser's.
            let Some(command) = browser.command(launch::app_launcher()) else {
                continue;
            };
            // Spaces are worth having without it.
            let address = open_browser(hypr, &self.id, window, browser, &command)
                .inspect_err(|e| eprintln!("space: warning: {e:#}"))
                .unwrap_or(None);
            if address.is_none() {
                broken.insert(browser.name.clone());
            }
            self.opened(n, address, opened);
        }
    }

    fn opened(&mut self, n: usize, address: Option<String>, opened: &mut HashSet<String>) {
        if let Some(address) = &address {
            let workspace = session_workspace(&self.id, self.saved.windows[n].tab);
            reserve::release(&paths::reserved_dir(), &workspace);
            opened.insert(address.clone());
        }
        self.addresses[n] = address;
    }

    /// Notes it for `watch`, with what its browser windows held if that's to
    /// be saved.
    fn note_restored(&self, hypr: &Hypr, browsers: bool) -> Result<()> {
        let mut restored = self.saved.clone();
        for (window, address) in restored.windows.iter_mut().zip(&self.addresses) {
            window.address = address.clone().unwrap_or_default();
        }
        if !browsers {
            let mut kept = Vec::new();
            let mut remap = HashMap::new();
            for (i, window) in restored.windows.into_iter().enumerate() {
                if window.browser().is_none() {
                    remap.insert(i, kept.len());
                    kept.push(window);
                }
            }
            restored.windows = kept;
            for layout in restored.layouts.values_mut() {
                layout.tiles = layout
                    .tiles
                    .as_ref()
                    .and_then(|t| t.filter_map(&|i| remap.get(i).copied()));
                layout.focused = layout.focused.and_then(|i| remap.get(&i).copied());
            }
        }
        session::add_restored(&paths::state_dir(), &hypr.instance, restored)
    }

    /// Arranges a tab's windows as they were.
    fn arrange(&self, hypr: &Hypr, key: u8) {
        // Everything is open; only where is off.
        if let Err(e) = arrange(hypr, &self.id, key, &self.saved, &self.addresses) {
            eprintln!("space: warning: {e:#}");
        }
    }

    /// Gives it back its name, its icon and its tabs' names. Again once more
    /// tabs are up, as the watcher forgets the names of tabs that aren't, and
    /// may have made the space's entry by now.
    fn describe(&self) -> Result<()> {
        let saved = &self.saved;
        let mut store = Store::lock(&paths::state_dir())?;
        let entry = store.entry(&self.id);
        for (tab, name) in &saved.tabs {
            entry.tabs.entry(*tab).or_insert_with(|| name.clone());
        }
        for (key, value) in &saved.meta {
            entry
                .meta
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        if !saved.home {
            entry.name = entry.name.take().or_else(|| saved.name.clone());
            entry.icon = entry.icon.take().or_else(|| saved.icon.clone());
            entry.note = entry.note.take().or_else(|| saved.note.clone());
            entry.folder = entry.folder.take().or_else(|| saved.folder.clone());
            entry.order = self.order;
        }
        store.save()?;
        drop(store);
        if let Some(last) = saved.last.filter(|_| !saved.home) {
            remember(&self.id, last);
        }
        Ok(())
    }
}

/// Opens a saved terminal, calling `opening` once it's on its way, and
/// returns its window once it's up. `token` tells its window from others.
fn reopen_terminal(
    hypr: &Hypr,
    config: &Config,
    id: &str,
    tab: u8,
    terminal: &session::Terminal,
    token: &str,
    opening: impl FnOnce() -> Result<()>,
) -> Result<Option<String>> {
    let workspace = session_workspace(id, tab);
    let events = hypr.events()?;
    hypr.dispatch(&hypr::exec_on(
        &restored_command(config, id, terminal, token),
        &format!("{} silent", live_selector(hypr, id, tab)),
    ))?;
    opening()?;
    // You may be opening windows of your own there by now.
    let ours = |address: &str| {
        hypr.clients().is_ok_and(|clients| {
            clients
                .iter()
                .find(|c| c.address == address)
                .and_then(|c| u32::try_from(c.pid).ok())
                .is_some_and(|pid| terminal::was_given(pid, shell::RESTORE, token))
        })
    };
    let window = wait_for_window(&events, &workspace, RESTORE_TIMEOUT, ours);
    match &window {
        Some(address) => {
            if let Err(e) = hypr.dispatch(&hypr::animate_again(address)) {
                eprintln!("space: warning: {e:#}");
            }
        }
        None => eprintln!("space: warning: no terminal yet on {workspace}"),
    }
    Ok(window)
}

/// Opens a saved browser window, as `reopen_terminal` does a terminal. A
/// browser that's running already opens it itself, from a process Hyprland
/// can't tell is ours, so a rule has the next such window open there.
fn open_browser(
    hypr: &Hypr,
    id: &str,
    window: &session::Window,
    browser: &Browser,
    command: &str,
) -> Result<Option<String>> {
    if browser.opens_itself() {
        let name = &browser.name;
        eprintln!("space: warning: {name} opens its last windows itself; leaving it to that");
        return Ok(None);
    }
    let workspace = session_workspace(id, window.tab);
    let rule = |enabled| {
        let on = format!("{} silent", live_selector(hypr, id, window.tab));
        hypr.eval(&hypr::open_class_on(&browser.class_pattern(), &on, enabled))
    };
    let events = hypr.events()?;
    rule(true)?;
    let launched = hypr.dispatch(&hypr::exec(command));
    let opened = launched.map(|()| {
        let ours = |address: &str| {
            hypr.clients().is_ok_and(|clients| {
                clients
                    .iter()
                    .any(|c| c.address == address && browser.is_class(&c.class))
            })
        };
        wait_for_window(&events, &workspace, BROWSER_TIMEOUT, ours)
    });
    rule(false)?;
    let opened = opened?;
    if opened.is_none() {
        eprintln!("space: warning: no browser window yet on {workspace}");
    }
    Ok(opened)
}

/// The command for a saved terminal.
fn restored_command(
    config: &Config,
    id: &str,
    terminal: &session::Terminal,
    token: &str,
) -> String {
    // A folder that's gone gives way to the nearest one still there.
    let dir = terminal
        .folder
        .as_ref()
        .map(Folder::path)
        .and_then(|dir| dir.ancestors().find(|dir| dir.is_dir()))
        .map_or_else(paths::home, Path::to_path_buf);
    let resume = terminal
        .conversation
        .as_ref()
        // Its conversations are by folder.
        .filter(|c| config.restore.agents && dir == c.folder)
        .filter(|c| c.command().is_some())
        .map(|c| format!("{} {}", c.agent, c.id));
    let mut env = vec![(shell::RESTORE, token), (crate::context::SPACE_ID_ENV, id)];
    let command = match &resume {
        Some(resume) => {
            env.push((shell::RESUME, resume));
            launch::resuming_terminal()
        }
        None => launch::terminal(),
    };
    terminal::command_in(&command, &dir, &env)
}

/// Arranges a restored tab's windows as they were. `addresses` are the
/// terminals' windows, in order, those that showed up.
fn arrange(
    hypr: &Hypr,
    id: &str,
    key: u8,
    saved: &session::Space,
    addresses: &[Option<String>],
) -> Result<()> {
    let default = session::Layout::default();
    let was = saved.layouts.get(&key).unwrap_or(&default);
    let name = session_workspace(id, key);
    let monitors = hypr.monitors().unwrap_or_default();
    let find = || -> Result<Option<hypr::Workspace>> {
        Ok(hypr.workspaces()?.into_iter().find(|ws| ws.name == name))
    };
    let Some(mut workspace) = find()? else {
        return Ok(());
    };
    // On the monitor whose tab it is, whichever its connector is now. `home`'s
    // are where Omarchy has them.
    let blocks = Blocks::load();
    if let Some(monitor) = blocks.monitor(key).or(was.monitor.as_deref())
        && !saved.home
        && workspace.monitor != monitor
        && monitors.iter().any(|m| m.name == monitor)
    {
        hypr.dispatch(&hypr::move_workspace(
            &live_selector(hypr, id, key),
            monitor,
        ))?;
        workspace = find()?.unwrap_or(workspace);
    }

    let window = |n: &usize| addresses.get(*n).cloned().flatten();
    let mut tab = layout::Tab {
        tiles: was
            .tiles
            .as_ref()
            .filter(|_| workspace.layout == "dwindle")
            .and_then(|tiles| tiles.filter_map(&window)),
        focused: was.focused.as_ref().and_then(window),
        ..Default::default()
    };
    let monitor = monitors.iter().find(|m| m.name == workspace.monitor);
    for (n, terminal) in saved.windows.iter().enumerate() {
        let Some(address) = window(&n).filter(|_| terminal.tab == key) else {
            continue;
        };
        if let Some(rect) = terminal.floating {
            tab.floating
                .insert(address.clone(), on_monitor(rect, monitor));
        }
        match terminal.fullscreen {
            0 => {}
            1 => drop(tab.fullscreen.insert(address, "maximized")),
            _ => drop(tab.fullscreen.insert(address, "fullscreen")),
        }
    }
    if tab.is_plain() {
        return Ok(());
    }
    hypr.eval(&tab.script())
}

/// A floating window's place on its monitor, which may be smaller than the
/// one it was on.
fn on_monitor(rect: Rect, monitor: Option<&hypr::Monitor>) -> Rect {
    let Some(monitor) = monitor else {
        return rect;
    };
    let (wide, high) = monitor.size();
    let (w, h) = (rect.w.min(wide), rect.h.min(high));
    Rect {
        x: monitor.x + rect.x.clamp(0, wide - w),
        y: monitor.y + rect.y.clamp(0, high - h),
        w,
        h,
    }
}
