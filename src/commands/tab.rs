//! Tab-level commands: `tab new`, `tab go`, `tab next`/`prev`, `tab rename`,
//! `tab reorder`, `tab close`, `tab send`, and `emptied`.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use super::space::{neighbour, switch};
use super::{
    SpaceTarget, focus, forget, fresh_key, hold_tabs, live_selector, moved, open_terminal_then,
    remember, resolve_dir, session_selector, session_workspace, take_turn, target_space,
};
use crate::context;
use crate::hypr::{self, Hypr};
use crate::layout;
use crate::live::{self, Row, Snapshot, SpaceWindow};
use crate::monitors::{Blocks, Strip};
use crate::name::{self, HOME, Location};
use crate::paths;
use crate::reserve;
use crate::restore::{session_layout, session_window};
use crate::session;
use crate::spaces::{self, Store};
use crate::state::Last;

/// Where a tab waits for the others to make room. All of it happens between
/// two frames, so nothing else sees it.
const ASIDE: &str = "name:omarchy-spaces-moving";

/// Where `space tab go`, `next` and `prev` go. A space's workspaces are its
/// tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Slot(u8),
    Next,
    Prev,
}

/// Options for `space tab new`.
#[derive(Debug, Default)]
pub struct NewTabOptions {
    pub space: Option<String>,
    pub dir: Option<PathBuf>,
    pub name: Option<String>,
    pub silent: bool,
    pub json: bool,
}

#[derive(Debug, Serialize)]
struct CreatedTab {
    version: u32,
    space: String,
    id: String,
    slot: u8,
    dir: Option<PathBuf>,
    window: Option<String>,
}

/// `space tab new`: opens a new tab in the target space (explicit `--space`,
/// else the caller's space, else the active space).
pub fn new_tab_cmd(opts: NewTabOptions) -> Result<()> {
    let custom_name = match opts.name {
        Some(raw) => spaces::clean_name(&raw)?,
        None => None,
    };
    let explicit_dir = match opts.dir {
        Some(dir) => Some(resolve_dir(dir)?),
        None => None,
    };

    let hypr = Hypr::from_env()?;
    let (rows, seen) = live::load_rows(Some(&hypr))?;
    let space = target_space(opts.space.clone(), &rows, seen.as_ref())?;
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);

    if space == HOME {
        if !opts.silent && explicit_dir.is_none() && custom_name.is_none() && !opts.json {
            let workspaces = hypr.workspaces()?;
            let strip = Strip::new(&workspaces, HOME, &active, &Blocks::load());
            let key = new_tab_in(&strip, [])?;
            if here.key() != Some(key) {
                focus(&hypr, HOME, key)?;
                remember(HOME, key);
            }
            return Ok(());
        }
        let workspaces = hypr.workspaces()?;
        let strip = Strip::new(&workspaces, HOME, &active, &Blocks::load());
        let key = new_tab_in(&strip, [])?;
        let (dir, window) = if opts.silent || explicit_dir.is_some() {
            let dir = match explicit_dir {
                Some(dir) => dir,
                None => live::here_folder(&hypr)?,
            };
            let env = [(context::SPACE_ID_ENV, HOME)];
            let window = open_terminal_then(&hypr, &dir, &key.to_string(), &env, || {
                if opts.silent {
                    Ok(())
                } else {
                    focus(&hypr, HOME, key)
                }
            })?;
            (Some(dir), window)
        } else {
            if here.key() != Some(key) {
                focus(&hypr, HOME, key)?;
            }
            (None, None)
        };
        if !opts.silent {
            remember(HOME, key);
        }
        if let Some(name) = custom_name {
            let mut store = Store::lock(&paths::state_dir())?;
            store.entry(HOME).tabs.insert(key, name);
            store.save()?;
        }
        if opts.json {
            let mut out = std::io::stdout().lock();
            serde_json::to_writer_pretty(
                &mut out,
                &CreatedTab {
                    version: 1,
                    space: HOME.to_string(),
                    id: name::tab_id(HOME, key),
                    slot: name::slot_of(key),
                    dir,
                    window,
                },
            )?;
            writeln!(out)?;
        }
        return Ok(());
    }

    let dir = match explicit_dir {
        Some(dir) => dir,
        None => live::space_folder(&hypr, &space)?,
    };
    let blocks = Blocks::load();
    let allow_reuse_active = here.space() == space && !opts.silent && opts.space.is_none();
    let (_, key) = reserve::reserve(&paths::reserved_dir(), |reserved| {
        let workspaces = hypr.workspaces()?;
        let opening: Vec<u8> = reserved
            .iter()
            .filter(|(of, _)| of == &space)
            .map(|(_, key)| *key)
            .collect();
        let empty = workspaces
            .iter()
            .any(|ws| ws.name == active.name && ws.windows == 0);
        if allow_reuse_active
            && let Some(key) = here.key()
            && empty
            && !opening.contains(&key)
        {
            return Ok((space.clone(), key));
        }
        let anchor_ws = if here.space() == space {
            &active
        } else {
            workspaces
                .iter()
                .find(|ws| name::locate(ws.id, &ws.name).space() == space)
                .unwrap_or(&active)
        };
        let strip = Strip::new(&workspaces, &space, anchor_ws, &blocks);
        let taken = open_in(&workspaces, &space).into_iter().chain(opening);
        Ok((space.clone(), new_tab_in(&strip, taken)?))
    })?;
    let workspace = name::workspace_name(&space, key);
    let env = [(context::SPACE_ID_ENV, space.as_str())];
    let window = open_terminal_then(&hypr, &dir, &workspace, &env, || {
        if opts.silent {
            return Ok(());
        }
        if here.space() != space
            && let Some(left_at) = here.key()
        {
            remember(here.space(), left_at);
        }
        focus(&hypr, &space, key)
    })?;
    if !opts.silent {
        remember(&space, key);
    }
    if let Some(name) = custom_name {
        let mut store = Store::lock(&paths::state_dir())?;
        store.entry(&space).tabs.insert(key, name);
        store.save()?;
    }
    if opts.json {
        let workspaces = hypr.workspaces().unwrap_or_default();
        let on = workspaces
            .iter()
            .find(|ws| is_tab(&space, key, ws.id, &ws.name))
            .unwrap_or(&active);
        let slot = Strip::new(&workspaces, &space, on, &blocks)
            .slot(key)
            .filter(|slot| *slot > 0)
            .unwrap_or(name::slot_of(key));
        let mut out = std::io::stdout().lock();
        serde_json::to_writer_pretty(
            &mut out,
            &CreatedTab {
                version: 1,
                id: name::tab_id(&space, key),
                space,
                slot,
                dir: Some(dir),
                window,
            },
        )?;
        writeln!(out)?;
    }
    Ok(())
}

/// `space tab go <n>`, `next` and `prev`: a tab of the active space.
pub fn go_to_tab(target: Target) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let _turn = matches!(target, Target::Next | Target::Prev)
        .then(take_turn)
        .transpose()?;
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    let space = here.space();
    let workspaces = hypr.workspaces()?;
    let strip = Strip::new(&workspaces, space, &active, &Blocks::load());
    let key = match target {
        Target::Slot(slot) => fresh_key(&workspaces, space, strip.reach(slot)),
        Target::Next | Target::Prev => {
            match step_tab(target == Target::Next, &strip.keys(), here.key()) {
                Some(key) => key,
                None => return Ok(()),
            }
        }
    };
    if here.key() == Some(key) {
        return Ok(());
    }

    focus(&hypr, space, key)?;
    remember(space, key);
    Ok(())
}

/// `space tab go <id>`: a tab of any space, on whichever monitor it is.
pub fn go_to_named_tab(tab: String) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (rows, seen) = live::load_rows(Some(&hypr))?;
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    let Tab { id, key, .. } = tab_or_active(&hypr, &rows, seen.as_ref(), Some(&tab), &active)?;
    if is_tab(&id, key, active.id, &active.name) {
        return Ok(());
    }
    if here.space() != id
        && let Some(left_at) = here.key()
    {
        remember(here.space(), left_at);
    }
    hypr.dispatch(&hypr::focus_workspace(&live_selector(&hypr, &id, key)))?;
    remember(&id, key);
    Ok(())
}

/// A new tab for a monitor: see `Strip::new_tab`.
fn new_tab_in(strip: &Strip, taken: impl IntoIterator<Item = u8>) -> Result<u8> {
    strip
        .new_tab(taken)
        .with_context(|| format!("all {} tabs are open; close one first", name::SLOTS))
}

/// The tab after or before `current` among the open ones; `None` when there's
/// nowhere else to go.
fn step_tab(forward: bool, open: &[u8], current: Option<u8>) -> Option<u8> {
    let Some(i) = current.and_then(|c| open.iter().position(|&key| key == c)) else {
        return if forward { open.first() } else { open.last() }.copied();
    };
    let step = if forward { 1 } else { open.len() - 1 };
    (open.len() > 1).then(|| open[(i + step) % open.len()])
}

/// `space tab reorder <tab> <position>`: moves a tab to `position` among
/// its space's tabs, counting from 1, past the end being last. The space
/// keeps the numbers it has tabs on: what's on them moves along, with the
/// names you gave them, and you stay on the tab you're on.
pub fn reorder_tab(tab: String, position: usize) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let _tabs = hold_tabs()?;
    let (mut store, rows, seen) = live::lock_rows(Some(&hypr))?;
    if position == 0 {
        bail!("positions start at 1, the space's first tab");
    }
    let active = hypr.active_workspace()?;
    let Tab { id, key, slot } = tab_or_active(&hypr, &rows, seen.as_ref(), Some(&tab), &active)?;
    // The tabs there are, as the bar shows them: its monitor's.
    let workspaces = hypr.workspaces()?;
    let on = workspaces
        .iter()
        .find(|ws| is_tab(&id, key, ws.id, &ws.name))
        .unwrap_or(&active);
    let strip = Strip::new(&workspaces, &id, on, &Blocks::load());
    let tabs = strip.keys();
    let order = moved(tabs.clone(), key, position - 1);
    // Where each tab that moves goes. Among the monitor's guests, they become
    // its own, on the slots they show as: put somewhere by hand, a tab stays.
    let moves: BTreeMap<u8, u8> = order
        .into_iter()
        .zip(tabs)
        .filter(|(from, to)| from != to)
        .map(|(from, to)| match strip.slot(to) {
            Some(slot) if slot > 0 && id != HOME => (from, strip.own(slot)),
            _ => (from, to),
        })
        .collect();
    let elsewhere = |ws: &&hypr::Workspace| ws.monitor != on.monitor;
    if let Some(taken) = workspaces
        .iter()
        .filter(elsewhere)
        .find(|ws| moves.values().any(|to| is_tab(&id, *to, ws.id, &ws.name)))
    {
        bail!(
            "tab {} is on another monitor",
            shown_id(taken.id, &taken.name)
        );
    }
    let Some(&to) = moves.get(&key) else {
        return Ok(());
    };

    if id == HOME {
        reorder_home(&hypr, &active, key, &moves)?;
    } else {
        hypr.eval(&rename_tabs(&hypr.workspaces()?, &id, key, &moves))?;
    }

    if let Some(entry) = store.spaces.get_mut(&id) {
        entry.tabs = std::mem::take(&mut entry.tabs)
            .into_iter()
            .map(|(tab, name)| (*moves.get(&tab).unwrap_or(&tab), name))
            .collect();
        store.save()?;
    }
    drop(store);

    // Where you left the space is still the tab you left.
    if let Err(e) = Last::update(&paths::state_dir(), |last| last.renumber(&id, &moves)) {
        eprintln!("space: warning: {e:#}");
    }
    println!("Moved tab {slot} to {}", name::slot_of(to));
    Ok(())
}

/// The tabs that move in the order to move them, each into the place the one
/// before it left, once the tab `first` is out of the way.
fn in_turn(first: u8, moves: &BTreeMap<u8, u8>) -> Vec<(u8, u8)> {
    let mut waiting = moves.clone();
    waiting.remove(&first);
    let mut steps = Vec::new();
    while let Some((&from, &to)) = waiting.iter().find(|(_, to)| !waiting.contains_key(to)) {
        waiting.remove(&from);
        steps.push((from, to));
    }
    steps
}

/// The Lua that renames a space's workspaces as `moves` says, `first` being
/// the tab the others make room for, which leaves their windows as they are.
/// Each is selected by its address, which a rename leaves as it was, so the
/// one that waits aside is the same selector as before.
fn rename_tabs(
    workspaces: &[hypr::Workspace],
    space: &str,
    first: u8,
    moves: &BTreeMap<u8, u8>,
) -> String {
    let tab = |key: u8| session_selector(workspaces, space, key);
    let name = |key: u8| session_workspace(space, key);
    let aside = ASIDE.trim_start_matches("name:").to_string();
    let mut renames = vec![(tab(first), aside)];
    renames.extend(
        in_turn(first, moves)
            .into_iter()
            .map(|(from, to)| (tab(from), name(to))),
    );
    // The tab that waited aside, by its address if Hyprland says it (0.57),
    // else by the name it was given.
    let addressed = workspaces
        .iter()
        .any(|ws| ws.name == name(first) && !ws.address.is_empty());
    let waiting = if addressed {
        tab(first)
    } else {
        ASIDE.to_string()
    };
    renames.push((waiting, name(moves[&first])));
    renames
        .iter()
        .map(|(from, to)| format!("hl.dispatch({})\n", hypr::rename_workspace(from, to)))
        .collect()
}

/// Moves what's on `home`'s tabs as `moves` says, `first` being the tab the
/// others make room for. Their workspaces are Hyprland's numbered ones, which
/// keep their numbers, so each window moves, and the tiles are split again as
/// they were.
fn reorder_home(
    hypr: &Hypr,
    active: &hypr::Workspace,
    first: u8,
    moves: &BTreeMap<u8, u8>,
) -> Result<()> {
    let snapshot = live::snapshot(hypr)?;
    let tab = |key: u8| session_selector(&[], HOME, key);
    let tabs: BTreeMap<u8, MovingTab> = moves
        .keys()
        .map(|key| {
            let on_tab = |w: &&SpaceWindow| w.space == HOME && w.tab == Some(*key);
            let windows: Vec<&SpaceWindow> = snapshot.space_windows.iter().filter(on_tab).collect();
            (*key, MovingTab::of(&snapshot, HOME, *key, &windows))
        })
        .collect();

    let mut script = String::from(
        "local animated = hl.get_config(\"animations.enabled\")\n\
         hl.config({ animations = { enabled = false } })\n\
         local ok, err = pcall(function()\n",
    );
    script.push_str(&tabs[&first].moves(ASIDE));
    for (from, to) in in_turn(first, moves) {
        script.push_str(&tabs[&from].moves(&tab(to)));
    }
    script.push_str(&tabs[&first].moves(&tab(moves[&first])));
    let here = u8::try_from(active.id)
        .ok()
        .filter(|_| matches!(name::locate(active.id, &active.name), Location::Home { .. }));
    if let Some(to) = here.and_then(|here| moves.get(&here)) {
        script.push_str(&format!(
            "hl.dispatch({})\n",
            hypr::focus_workspace(&tab(*to))
        ));
    }
    for moving in tabs.values().filter(|t| !t.layout.is_plain()) {
        script.push_str(&format!("do\n{}end\n", moving.layout.script()));
    }
    script.push_str(
        "end)\n\
         hl.config({ animations = { enabled = animated } })\n\
         if not ok then error(err) end\n",
    );
    hypr.eval(&script)
}

/// A space's tab, and the number it shows.
struct Tab {
    id: String,
    key: u8,
    slot: u8,
}

/// The id of the tab a workspace is, to say which.
fn shown_id(ws_id: i64, ws_name: &str) -> String {
    let location = name::locate(ws_id, ws_name);
    match location.key() {
        Some(key) => name::tab_id(location.space(), key),
        None => ws_name.to_string(),
    }
}

/// The tab given by its id, or with none, the tab of the window the command
/// was run from, else the one you are on. Run from inside a space with no
/// window to tell its tab by (`SPACE_ID` alone, as in tmux), it needs the id:
/// the one you are on may be another space's, or another tab of this one.
fn tab_or_active(
    hypr: &Hypr,
    rows: &[Row],
    seen: Option<&live::Seen>,
    tab: Option<&str>,
    active: &hypr::Workspace,
) -> Result<Tab> {
    let here = name::locate(active.id, &active.name);
    let (id, key) = match tab {
        Some(tab) => {
            let (space, key) = name::parse_tab_id(tab).with_context(|| {
                format!("'{tab}' isn't a tab: give its id, as `space ls --json` shows it")
            })?;
            (space.to_string(), key)
        }
        None => match seen.and_then(|(snapshot, live)| context::resolve(snapshot, Some(live))) {
            Some(caller) => {
                let key = caller.tab_key.with_context(|| {
                    format!(
                        "can't tell which tab of {} this was run from: give its id, as `space ls --json` shows it",
                        caller.space
                    )
                })?;
                (caller.space, key)
            }
            None => (
                here.space().to_string(),
                here.key().context("you aren't on a tab")?,
            ),
        },
    };
    let row = rows.iter().find(|r| r.id == id);
    let open = row.and_then(|r| r.workspaces.as_deref());
    if !open.is_some_and(|open| open.contains(&key)) {
        bail!("there's no tab {}", name::tab_id(&id, key));
    }
    let workspaces = hypr.workspaces()?;
    let on = workspaces
        .iter()
        .find(|ws| is_tab(&id, key, ws.id, &ws.name))
        .unwrap_or(active);
    let shown = Strip::new(&workspaces, &id, on, &Blocks::load()).slot(key);
    let slot = shown.filter(|slot| *slot > 0).unwrap_or(name::slot_of(key));
    Ok(Tab { id, key, slot })
}

/// `space tab rename [tab] <name>`: gives a tab, the one you are on
/// by default, a name that stays, in place of its window's. An empty name has
/// it follow its window again, or, in `home`, show just its number.
pub fn rename_tab(tab: Option<String>, new_name: String) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (mut store, rows, seen) = live::lock_rows(Some(&hypr))?;
    let active = hypr.active_workspace()?;
    let Tab { id, key, slot } =
        tab_or_active(&hypr, &rows, seen.as_ref(), tab.as_deref(), &active)?;
    let new_name = spaces::clean_name(&new_name)?;

    let tabs = &mut store.entry(&id).tabs;
    match &new_name {
        Some(new_name) => tabs.insert(key, new_name.clone()),
        None => tabs.remove(&key),
    };
    store.save()?;
    match (new_name, id == HOME) {
        (Some(new_name), _) => println!("Named tab {slot} '{new_name}'"),
        (None, false) => println!("Tab {slot} is called after its window again"),
        (None, true) => println!("Tab {slot} shows just its number again"),
    }
    Ok(())
}

/// `space tab close [tab]`: closes every window on a tab, the one you
/// are on by default, which ends it. If you are on it, you go to another of
/// the space's tabs, as when its last window closes (`emptied`), and closing
/// its only tab ends the space. `home` doesn't end: you stay on its last tab.
pub fn close_tab(tab: Option<String>) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let _tabs = hold_tabs()?;
    let (rows, seen) = live::load_rows(Some(&hypr))?;
    let active = hypr.active_workspace()?;
    let Tab { id, key, slot } =
        tab_or_active(&hypr, &rows, seen.as_ref(), tab.as_deref(), &active)?;
    let on_tab = |ws_id, ws_name: &str| is_tab(&id, key, ws_id, ws_name);
    let windows: Vec<String> = hypr
        .clients()?
        .into_iter()
        .filter(|c| on_tab(c.workspace.id, &c.workspace.name))
        .map(|c| c.address)
        .collect();

    if on_tab(active.id, &active.name) {
        leave_tab(&hypr, &id, key, &active, true)?;
    }
    for address in &windows {
        hypr.dispatch(&hypr::close_window(address))?;
    }
    match windows.len() {
        0 => println!("No windows open on tab {slot}"),
        1 => println!("Closed 1 window on tab {slot}"),
        n => println!("Closed {n} windows on tab {slot}"),
    }
    Ok(())
}

/// Whether a workspace is a space's tab.
fn is_tab(space: &str, key: u8, ws_id: i64, ws_name: &str) -> bool {
    let location = name::locate(ws_id, ws_name);
    location.space() == space && location.key() == Some(key)
}

/// The tab to go to from a space's tab that is ending, on the workspace
/// `from`: the next one with windows on its monitor, else the one before.
fn tab_beside(
    workspaces: &[hypr::Workspace],
    id: &str,
    key: u8,
    from: &hypr::Workspace,
) -> Option<u8> {
    let full = |key: u8| {
        workspaces
            .iter()
            .any(|ws| ws.windows > 0 && is_tab(id, key, ws.id, &ws.name))
    };
    let tabs = Strip::new(workspaces, id, from, &Blocks::load()).keys();
    let at = tabs.iter().position(|tab| *tab == key)?;
    let after = tabs[at + 1..].iter().find(|tab| full(**tab));
    let before = tabs[..at].iter().rev().find(|tab| full(**tab));
    after.or(before).copied()
}

/// `space tab send [tab] <space|next|prev>`: moves a tab, the one you
/// are on by default, to another space, after the tabs it has, and goes
/// there with it. Its windows stay arranged as they are and it keeps the name
/// you gave it. A space whose only tab it was ends.
pub fn send_tab(tab: Option<String>, to: SpaceTarget) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let _turn = matches!(to, SpaceTarget::Next | SpaceTarget::Prev)
        .then(take_turn)
        .transpose()?;
    let _tabs = hold_tabs()?;
    let (rows, seen) = live::load_rows(Some(&hypr))?;
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    let Tab { id, key, slot } =
        tab_or_active(&hypr, &rows, seen.as_ref(), tab.as_deref(), &active)?;
    let to = match to {
        SpaceTarget::Named(target) => live::resolve(&rows, &target)?,
        SpaceTarget::Next | SpaceTarget::Prev => match neighbour(&rows, to == SpaceTarget::Next) {
            Some(to) => to,
            None => return Ok(()),
        },
    };
    if to == id {
        return Ok(());
    }
    let snapshot = live::snapshot(&hypr)?;
    let windows: Vec<&SpaceWindow> = snapshot
        .space_windows
        .iter()
        .filter(|w| w.space == id && w.tab == Some(key))
        .collect();
    if windows.is_empty() {
        bail!("nothing is open on tab {slot}");
    }
    let workspaces = hypr.workspaces()?;
    // It stays on the monitor it's on.
    let from = workspaces
        .iter()
        .find(|ws| is_tab(&id, key, ws.id, &ws.name))
        .unwrap_or(&active);
    let blocks = Blocks::load();
    let was_here = is_tab(&id, key, active.id, &active.name);
    let beside = tab_beside(&workspaces, &id, key, from);
    // Whether the space has anything else, on any monitor.
    let lives_on = snapshot
        .space_windows
        .iter()
        .any(|w| w.space == id && w.tab != Some(key));

    let label = rows.iter().find(|r| r.id == to).map_or("", |r| &r.label);
    let (to_key, reserved) = if to == HOME {
        let strip = Strip::new(&workspaces, HOME, from, &blocks);
        (key_after(&strip, open_in(&workspaces, HOME), HOME)?, None)
    } else {
        let (_, key) = reserve::reserve(&paths::reserved_dir(), |reserved| {
            let workspaces = hypr.workspaces()?;
            let strip = Strip::new(&workspaces, &to, from, &blocks);
            let opening = reserved.iter().filter(|(of, _)| *of == to);
            let taken = opening.map(|(_, key)| *key);
            let taken = open_in(&workspaces, &to).into_iter().chain(taken);
            Ok((to.clone(), key_after(&strip, taken, label)?))
        })?;
        (key, Some(name::workspace_name(&to, key)))
    };
    if from.monitor != active.monitor {
        hypr.dispatch(&hypr::focus_monitor(&from.monitor))?;
    }
    let moved = move_tab(&hypr, &snapshot, (&id, key), (&to, to_key), &windows);
    let went = moved
        .and_then(|()| hypr.dispatch(&hypr::focus_workspace(&live_selector(&hypr, &to, to_key))))
        // Renamed, the tab you were on says nothing of being in another space.
        .and_then(|()| hypr.eval(hypr::FOLLOW));
    if let Some(reserved) = reserved {
        reserve::release(&paths::reserved_dir(), &reserved);
    }
    went?;

    let row = rows.iter().find(|r| r.id == id);
    if let Some(name) = row.and_then(|r| r.tabs.get(&key)) {
        let mut store = Store::lock(&paths::state_dir())?;
        if let Some(entry) = store.spaces.get_mut(&id) {
            entry.tabs.remove(&key);
        }
        store.entry(&to).tabs.insert(to_key, name.clone());
        store.save()?;
    }

    // Where you left the spaces you did: never on the tab that went.
    if let Some(next) = beside {
        let last = Last::load(&paths::state_dir())?;
        if was_here || last.get(&id).is_some_and(|visit| visit.key == key) {
            remember(&id, next);
        }
    } else if id != HOME && !lives_on {
        forget(&id);
    }
    let ended = here.space() == id && id != HOME && !lives_on;
    if let Some(left_at) = here.key()
        && here.space() != to
        && !was_here
        && !ended
    {
        remember(here.space(), left_at);
    }
    // Renamed into the other space, the tab didn't end as Hyprland sees it,
    // which would have renumbered the ones it left.
    if let Err(e) = renumber_tabs(&hypr) {
        eprintln!("space: warning: {e:#}");
    }
    println!("Moved tab {slot} to '{label}'");
    Ok(())
}

/// The tabs of a space that have a workspace now, on any monitor, and those
/// whose number a new tab can't have: a renamed tab's workspace is still
/// addressed by the number it was made with, which `name:` would find instead
/// of making a new one. `space renumber` numbers it as the next tab.
fn open_in(workspaces: &[hypr::Workspace], space: &str) -> BTreeSet<u8> {
    let by_name = workspaces
        .iter()
        .map(|ws| name::locate(ws.id, &ws.name))
        .filter(|location| location.space() == space);
    let by_address = workspaces
        .iter()
        .filter(|ws| ws.id <= 0)
        .map(|ws| name::locate(0, &ws.address))
        .filter(|location| matches!(location, Location::Space { name, .. } if name == space));
    by_name.chain(by_address).filter_map(|l| l.key()).collect()
}

/// A new tab for a monitor, in the space called `label`.
fn key_after(strip: &Strip, taken: impl IntoIterator<Item = u8>, label: &str) -> Result<u8> {
    strip.new_tab(taken).with_context(|| {
        format!(
            "all {} tabs of '{label}' are open; close one first",
            name::SLOTS
        )
    })
}

/// Moves a tab's windows to another. Between
/// spaces its workspace is renamed, which leaves them just as they are.
/// Hyprland's numbered workspaces, `home`'s, can't become named ones or the
/// other way around, so then each window moves, and the tiles are split again
/// as they were, all between two frames.
fn move_tab(
    hypr: &Hypr,
    snapshot: &Snapshot,
    (from, key): (&str, u8),
    (to, to_key): (&str, u8),
    windows: &[&SpaceWindow],
) -> Result<()> {
    let workspaces = hypr.workspaces()?;
    let selector = session_selector(&workspaces, to, to_key);
    if from != HOME && to != HOME {
        return hypr.dispatch(&hypr::rename_workspace(
            &session_selector(&workspaces, from, key),
            &session_workspace(to, to_key),
        ));
    }

    let moving = MovingTab::of(snapshot, from, key, windows);
    let mut script = moving.moves(&selector);
    // There first: the script comes back to where you were, which may be the
    // tab that just went.
    script.push_str(&format!(
        "hl.dispatch({})\n",
        hypr::focus_workspace(&selector)
    ));
    if !moving.layout.is_plain() {
        script.push_str(&moving.layout.script());
    }
    hypr.eval(&script)
}

/// A tab whose windows move one by one to another.
struct MovingTab {
    /// The windows to move: a group moves as one, by the one that shows.
    windows: Vec<String>,
    /// How its tiles are split, to split them again there.
    layout: layout::Tab,
}

impl MovingTab {
    fn of(snapshot: &Snapshot, space: &str, key: u8, windows: &[&SpaceWindow]) -> Self {
        let mut shown: Vec<&SpaceWindow> = windows.to_vec();
        shown.sort_by_key(|w| w.focus);
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        shown.retain(|w| {
            let first = !seen.contains(w.address.as_str());
            seen.extend(w.grouped.iter().map(String::as_str));
            first
        });
        let saved: Vec<session::Window> = shown
            .iter()
            .filter_map(|w| {
                session_window(
                    snapshot,
                    w,
                    session::WindowKind::Terminal(Default::default()),
                    &[],
                )
            })
            .collect();
        let layout = session_layout(snapshot, space, key, &saved, &[]);
        let address = |n: &usize| saved.get(*n).map(|w| w.address.clone());
        Self {
            windows: shown.iter().map(|w| w.address.clone()).collect(),
            layout: layout::Tab {
                tiles: layout.tiles.as_ref().and_then(|t| t.filter_map(&address)),
                focused: layout.focused.as_ref().and_then(address),
                ..Default::default()
            },
        }
    }

    /// The Lua that moves its windows to a workspace.
    fn moves(&self, selector: &str) -> String {
        self.windows
            .iter()
            .map(|w| format!("hl.dispatch({})\n", hypr::move_window_to(w, selector)))
            .collect()
    }
}

/// Has the monitor showing a space's tab that is ending, on the workspace
/// `from`, go on to its next tab there with windows, else the one before.
/// With none left there, it stays, empty, as long as the space has windows on
/// another monitor. With none on any, the space ends and you go back to the
/// space you were in before it, or `home`, whose own last tab you stay on.
/// `focused` says whether that monitor is the one you are on.
fn leave_tab(hypr: &Hypr, id: &str, key: u8, from: &hypr::Workspace, focused: bool) -> Result<()> {
    let workspaces = hypr.workspaces()?;
    if let Some(next) = tab_beside(&workspaces, id, key, from) {
        if focused {
            focus(hypr, id, next)?;
            remember(id, next);
        } else if let Some(next) = workspaces
            .iter()
            .find(|ws| is_tab(id, next, ws.id, &ws.name))
        {
            hypr.eval(&hypr::show_workspace(&next.name))?;
        }
        return Ok(());
    }
    let elsewhere = |ws: &hypr::Workspace| {
        ws.windows > 0
            && !is_tab(id, key, ws.id, &ws.name)
            && name::locate(ws.id, &ws.name).space() == id
    };
    if id == HOME || workspaces.iter().any(elsewhere) {
        return Ok(());
    }

    let snapshot = live::snapshot(hypr)?;
    let others: BTreeSet<&str> = snapshot
        .space_windows
        .iter()
        .map(|w| w.space.as_str())
        .filter(|space| *space != id)
        .collect();
    let to = Last::load(&paths::state_dir())?
        .latest_except(id, |s| s == HOME || others.contains(s))
        .unwrap_or(HOME)
        .to_string();
    switch(hypr, &to)?;
    // The space is gone (switching noted where you left it): nothing to go
    // back to.
    forget(id);
    Ok(())
}

/// `space emptied <workspace>`, run by Hyprland (`hypr/omarchy-spaces.lua`) when
/// closing a window left a workspace that a monitor shows empty. An emptied
/// tab doesn't linger: see `leave_tab`. Nothing happens if things moved on
/// since: no monitor shows that workspace, or something opened on it.
pub fn emptied(workspace: &str) -> Result<()> {
    let _tabs = hold_tabs()?;
    emptied_held(workspace)
}

/// `emptied`, by a command already holding the tabs.
pub(super) fn emptied_held(workspace: &str) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let active = hypr.active_workspace()?;
    let workspaces = hypr.workspaces()?;
    let Some(emptied) = workspaces.iter().find(|ws| ws.name == workspace) else {
        return Ok(());
    };
    let focused = active.name == workspace;
    let shown = focused
        || hypr.monitors()?.iter().any(|monitor| {
            let shows = monitor.shows.as_ref();
            shows.is_some_and(|ws| ws.name == workspace)
        });
    if !shown || emptied.windows > 0 {
        return Ok(());
    }
    let Location::Space { name: id, key } = name::locate(emptied.id, &emptied.name) else {
        return Ok(());
    };
    leave_tab(&hypr, &id, key, emptied, focused)
}

/// `space renumber`, run by Hyprland (`hypr/omarchy-spaces.lua`) whenever a
/// workspace comes or goes: numbers each space's tabs 1, 2, 3, ... on each
/// monitor again, as a browser's are. A tab that ends, however it does (its
/// last window closing, even unseen, or moving away, or the tab sent to
/// another space), takes its number with it and the tabs after it move up.
/// Each moves by renaming its workspace, which leaves its windows as they
/// are, all between two frames. `home`'s tabs are Omarchy's numbered
/// workspaces, which other things know by their numbers: they keep theirs.
/// Nothing happens while a session is being restored, as that opens tabs by
/// the numbers it saved.
pub fn renumber() -> Result<()> {
    let _tabs = hold_tabs()?;
    let Some(_restoring) = session::lock_restore(&paths::state_dir())? else {
        return Ok(());
    };
    renumber_tabs(&Hypr::from_env()?)
}

/// `renumber`, by a command already holding the tabs.
fn renumber_tabs(hypr: &Hypr) -> Result<()> {
    let mut open: BTreeMap<String, BTreeSet<u8>> = BTreeMap::new();
    let workspaces = hypr.workspaces()?;
    for ws in &workspaces {
        if let Location::Space { name, key } = name::locate(ws.id, &ws.name) {
            open.entry(name).or_default().insert(key);
        }
    }
    let moves: BTreeMap<String, BTreeMap<u8, u8>> = open
        .iter()
        .map(|(space, keys)| (space.clone(), closing_moves(keys)))
        .filter(|(_, moves)| !moves.is_empty())
        .collect();
    if moves.is_empty() {
        return Ok(());
    }
    // Lowest first, each into a number that's free by then.
    let by_address = &workspaces;
    let script: String = moves
        .iter()
        .flat_map(|(space, moves)| {
            moves.iter().map(move |(from, to)| {
                let rename = hypr::rename_workspace(
                    &session_selector(by_address, space, *from),
                    &session_workspace(space, *to),
                );
                format!("hl.dispatch({rename})\n")
            })
        })
        .collect();
    hypr.eval(&script)?;

    // The names you gave them go along. Those of tabs that have ended go,
    // so as not to land on another.
    let mut store = Store::lock(&paths::state_dir())?;
    for (space, moves) in &moves {
        if let Some(entry) = store.spaces.get_mut(space) {
            entry.tabs = std::mem::take(&mut entry.tabs)
                .into_iter()
                .filter(|(key, _)| open[space].contains(key))
                .map(|(key, name)| (*moves.get(&key).unwrap_or(&key), name))
                .collect();
        }
    }
    store.save()?;
    drop(store);
    // Where you left each space is still the tab you left.
    Last::update(&paths::state_dir(), |last| {
        for (space, moves) in &moves {
            last.renumber(space, moves);
        }
    })
}

/// Where each of a space's tabs, by key, moves to number them 1, 2, 3, ...
/// in each monitor's block, in the order they're in. Those already in place
/// aren't in it.
fn closing_moves(keys: &BTreeSet<u8>) -> BTreeMap<u8, u8> {
    let mut moves = BTreeMap::new();
    let mut taken: BTreeMap<u8, u8> = BTreeMap::new();
    for &key in keys {
        let block = name::block_of(key);
        let slot = taken.entry(block).or_insert(0);
        *slot += 1;
        let to = name::key_in(block, *slot);
        if to != key {
            moves.insert(key, to);
        }
    }
    moves
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_through_the_open_tabs() {
        let step = step_tab;
        assert_eq!(step(true, &[1, 3, 4], Some(3)), Some(4));
        assert_eq!(step(true, &[1, 3, 4], Some(4)), Some(1), "wraps");
        assert_eq!(step(false, &[1, 3, 4], Some(1)), Some(4), "wraps");
        assert_eq!(step(false, &[1, 3, 4], Some(3)), Some(1));
        assert_eq!(step(true, &[1, 11, 3], Some(11)), Some(3), "as they show");
        assert_eq!(step(true, &[2], Some(2)), None, "only one tab");
        assert_eq!(step(true, &[2, 5], None), Some(2));
    }

    #[test]
    fn closes_the_gaps_among_tabs() {
        let moves = |keys: &[u8]| closing_moves(&keys.iter().copied().collect());
        assert_eq!(moves(&[1, 2, 3]), BTreeMap::new(), "none");
        assert_eq!(moves(&[1, 3, 4]), [(3, 2), (4, 3)].into());
        assert_eq!(moves(&[2, 6]), [(2, 1), (6, 2)].into(), "from 1");
        assert_eq!(
            moves(&[1, 3, 12, 15]),
            [(3, 2), (12, 11), (15, 12)].into(),
            "each monitor's block apart"
        );
        assert_eq!(moves(&[]), BTreeMap::new());
    }

    #[test]
    fn moves_tabs_into_the_places_others_leave() {
        // A rotation: 3 to the front.
        let moves = [(1, 2), (2, 3), (3, 1)].into();
        assert_eq!(in_turn(3, &moves), [(2, 3), (1, 2)]);
        // A guest to the front, of which nothing takes the place.
        let moves = [(1, 2), (2, 3), (11, 1)].into();
        assert_eq!(in_turn(11, &moves), [(2, 3), (1, 2)]);
        // A tab past a guest.
        let moves = [(1, 3), (2, 1), (11, 2)].into();
        assert_eq!(in_turn(1, &moves), [(2, 1), (11, 2)]);
    }
}
