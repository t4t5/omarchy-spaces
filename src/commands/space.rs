//! Space-level commands: `new`, `go`, `next`/`prev`, `rename`, `icon`,
//! `reorder`, `close`, and `ls`.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use super::{focus, forget, fresh_space, moved, open_terminal_then, remember, take_turn};
use crate::folder::Folder;
use crate::hypr::{self, Hypr};
use crate::list;
use crate::live::{self, Row};
use crate::monitors::{Blocks, Strip};
use crate::name::{self, HOME, Location};
use crate::paths;
use crate::spaces::{self, Store};
use crate::state::{self, Last};

/// `space new [dir]`: a new space in `dir`, by default the folder you are
/// in, with a terminal open there. It is called after the folder that
/// terminal is in, and follows it.
pub fn new(dir: Option<PathBuf>) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let dir = match dir {
        Some(dir) => {
            let cwd = std::env::current_dir().context("can't read the current folder")?;
            let dir = cwd.join(dir);
            let dir = std::fs::canonicalize(&dir)
                .with_context(|| format!("no folder {}", paths::tilde(&dir)))?;
            if !dir.is_dir() {
                bail!("{} is not a folder", paths::tilde(&dir));
            }
            dir
        }
        None => live::here_folder(&hypr)?,
    };

    // On the monitor you are on.
    let first = name::key_in(Blocks::load().of(&hypr.active_workspace()?), 1);
    let id = fresh_space(&hypr, first, None)?;
    let workspace = name::workspace_name(&id, first);
    open_terminal_then(&hypr, &dir, &workspace, || switch(&hypr, &id))?;

    // The watcher may have seen the space by now, and made its entry.
    let mut store = Store::lock(&paths::state_dir())?;
    let entry = store.entry(&id);
    entry.folder.get_or_insert(Folder::here(&dir));
    if entry.order == 0 {
        entry.order = state::now();
    }
    store.save()?;
    println!("Opened a space in {}", paths::tilde(&dir));
    Ok(())
}

/// `space go <space>`: the space's last-focused workspace, or its first.
pub fn go(target: String) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (rows, _) = live::load_rows(Some(&hypr))?;
    let id = live::resolve(&rows, &target)?;
    switch(&hypr, &id)
}

/// `space next` / `space prev`: the space after or before the active one in
/// the sidebar's order (`home`, then the order they were made in), where you
/// left it. It wraps around at either end.
pub fn step(forward: bool) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let _turn = take_turn()?;
    let (rows, _) = live::load_rows(Some(&hypr))?;
    match neighbour(&rows, forward) {
        Some(id) => switch(&hypr, &id),
        None => Ok(()),
    }
}

/// The space next to the active one, or `None` when there's no other.
pub(super) fn neighbour(rows: &[Row], forward: bool) -> Option<String> {
    let here = rows.iter().position(|r| r.active)?;
    let count = rows.len();
    let there = if forward {
        (here + 1) % count
    } else {
        (here + count - 1) % count
    };
    (there != here).then(|| rows[there].id.clone())
}

/// Leaves the active space, remembering where, for the target's last
/// workspace. Remembered first, so that whoever is told of the switch
/// (`watch.rs`) already finds what you left.
pub(super) fn switch(hypr: &Hypr, target: &str) -> Result<()> {
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    if here.space() == target {
        return Ok(());
    }

    let last = Last::load(&paths::state_dir())?;
    let key = landing_tab(hypr, &last, &active, target)?;
    if let Some(left_at) = here.key() {
        remember(here.space(), left_at);
    }
    focus(hypr, target, key)
}

/// A space's workspaces on a monitor.
fn tabs_on<'a>(
    workspaces: &'a [hypr::Workspace],
    space: &'a str,
    monitor: &'a str,
) -> impl Iterator<Item = &'a hypr::Workspace> {
    workspaces
        .iter()
        .filter(move |ws| ws.monitor == monitor && name::locate(ws.id, &ws.name).space() == space)
}

/// The tab you land on going to a space, on the monitor you are on: where you
/// left it, if that was there, else the one with the window you last used
/// there, else its first tab there. The other monitors follow
/// (`hypr/omarchy-spaces.lua`).
pub(super) fn landing_tab(
    hypr: &Hypr,
    last: &Last,
    active: &hypr::Workspace,
    target: &str,
) -> Result<u8> {
    let workspaces = hypr.workspaces()?;
    let strip = Strip::new(&workspaces, target, active, &Blocks::load());
    // There, or where it was, unless another shows there now.
    if let Some(visit) = last.get(target)
        && (strip.slot(visit.key).is_some() || strip.at(name::slot_of(visit.key)) == visit.key)
    {
        return Ok(visit.key);
    }
    if target != HOME || last.get(target).is_some() {
        let tabs: Vec<(u8, &str)> = tabs_on(&workspaces, target, &active.monitor)
            .filter_map(|ws| Some((name::locate(ws.id, &ws.name).key()?, ws.name.as_str())))
            .collect();
        let clients = if tabs.len() > 1 {
            hypr.clients()?
        } else {
            Vec::new()
        };
        let used = |name: &str| {
            let there = clients.iter().filter(|c| c.workspace.name == name);
            there.map(|c| c.focus).filter(|focus| *focus >= 0).min()
        };
        let landing = tabs
            .iter()
            .min_by_key(|(key, name)| (used(name).unwrap_or(i64::MAX), *key));
        if let Some((key, _)) = landing {
            return Ok(*key);
        }
    }
    Ok(strip.at(1))
}

/// `space rename [space] <name>`: gives a space, the active one by default,
/// a name that stays. An empty name has it called after its folder again.
pub fn rename(target: Option<String>, new_name: String) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (mut store, rows, _) = live::lock_rows(Some(&hypr))?;
    let id = match target {
        Some(target) => live::resolve(&rows, &target)?,
        None => rows
            .iter()
            .find(|r| r.active)
            .map(|r| r.id.clone())
            .unwrap_or_else(|| HOME.to_string()),
    };
    if id == HOME {
        bail!("'home' is the plain workspaces; it can't be renamed");
    }
    let new_name = spaces::clean_name(&new_name)?;

    store.entry(&id).name = new_name.clone();
    store.save()?;
    match new_name {
        Some(new_name) => println!("Named the space '{new_name}'"),
        None => println!("The space is called after its folder again"),
    }
    Ok(())
}

/// `space icon [space] <icon>`: gives a space, the active one by default, a
/// glyph or an emoji to show on its badge in place of its letters. An empty
/// icon has it show its letters again.
pub fn icon(target: Option<String>, new_icon: String) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (mut store, rows, _) = live::lock_rows(Some(&hypr))?;
    let id = match target {
        Some(target) => live::resolve(&rows, &target)?,
        None => rows
            .iter()
            .find(|r| r.active)
            .map(|r| r.id.clone())
            .unwrap_or_else(|| HOME.to_string()),
    };
    if id == HOME {
        bail!("'home' is the plain workspaces; it can't have an icon");
    }
    let new_icon = spaces::clean_icon(&new_icon)?;

    store.entry(&id).icon = new_icon.clone();
    store.save()?;
    match new_icon {
        Some(new_icon) if new_icon.starts_with('/') => {
            println!(
                "Gave the space the icon {}",
                paths::tilde(std::path::Path::new(&new_icon))
            );
        }
        Some(new_icon) => println!("Gave the space the icon {new_icon}"),
        None => println!("The space shows its letters again"),
    }
    Ok(())
}

/// `space reorder <space> <position>`: moves a space to `position` in the
/// sidebar, counting from 1 below `home`, which stays first. Past the end
/// puts it last.
pub fn reorder(target: String, position: usize) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (mut store, rows, _) = live::lock_rows(Some(&hypr))?;
    let id = live::resolve(&rows, &target)?;
    if id == HOME {
        bail!("'home' is the plain workspaces; it stays first");
    }
    if position == 0 {
        bail!("positions start at 1, the first space below home");
    }
    let order: Vec<String> = rows
        .into_iter()
        .map(|r| r.id)
        .filter(|r| r != HOME)
        .collect();
    let order = moved(order, id, position - 1);
    restamp(&mut store, &order);
    store.save()?;
    Ok(())
}

/// Puts the spaces in `order`, handing out the places they have again, made
/// to rise strictly so no two tie. New spaces, whose places are the time now,
/// still come after them all.
fn restamp(store: &mut Store, order: &[String]) {
    let mut places: Vec<u64> = order
        .iter()
        .map(|id| store.get(id).map_or(0, |e| e.order))
        .collect();
    places.sort_unstable();
    let mut last = 0;
    for (id, place) in order.iter().zip(places) {
        let place = place.max(last + 1);
        store.entry(id).order = place;
        last = place;
    }
}

/// `space close [space]`: closes every window of a space, the active one by
/// default, which ends it. If you are in it, you go back to the space you
/// were in before, or `home`. Each app is asked, as `super+w` asks it, so it
/// can still stop to save.
pub fn close(target: Option<String>) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (rows, live) = live::load_rows(Some(&hypr))?;
    let (_, live) = live.expect("read from Hyprland");
    let id = match target {
        Some(target) => live::resolve(&rows, &target)?,
        None => live.active.clone().unwrap_or_else(|| HOME.to_string()),
    };
    if id == HOME {
        bail!("'home' is the plain workspaces; close its windows one by one");
    }
    let label = rows
        .iter()
        .find(|r| r.id == id)
        .map_or(id.clone(), |r| r.label.clone());

    let windows: Vec<String> = hypr
        .clients()?
        .into_iter()
        .filter(|c| {
            matches!(name::locate(c.workspace.id, &c.workspace.name),
                Location::Space { name, .. } if name == id)
        })
        .map(|c| c.address)
        .collect();

    if live.active.as_deref() == Some(id.as_str()) {
        let last = Last::load(&paths::state_dir())?;
        let spaces = live.spaces();
        let to = last
            .latest_except(&id, |s| s == HOME || spaces.contains(s))
            .unwrap_or(HOME)
            .to_string();
        switch(&hypr, &to)?;
    }
    for address in &windows {
        hypr.dispatch(&hypr::close_window(address))?;
    }
    forget(&id);
    match windows.len() {
        0 => println!("No windows open in '{label}'"),
        1 => println!("Closed 1 window in '{label}'"),
        n => println!("Closed {n} windows in '{label}'"),
    }
    Ok(())
}

/// `space ls`: every space, the active one marked, with a window count.
pub fn ls(json: bool) -> Result<()> {
    let hypr = match Hypr::from_env() {
        Ok(hypr) => Some(hypr),
        Err(e) => {
            eprintln!("space: no window counts: {e:#}");
            None
        }
    };
    let (rows, live) = match live::load_rows(hypr.as_ref()) {
        Ok(read) => read,
        Err(e) if hypr.is_some() => {
            eprintln!("space: no window counts: {e:#}");
            live::load_rows(None)?
        }
        Err(e) => return Err(e),
    };

    let mut out = std::io::stdout().lock();
    if json {
        let hyprland = live.as_ref().map(|(snapshot, live)| (snapshot, live));
        serde_json::to_writer_pretty(&mut out, &list::list(&rows, hyprland))?;
        writeln!(out)?;
    } else {
        write!(out, "{}", format_rows(&rows))?;
    }
    Ok(())
}

fn format_rows(rows: &[Row]) -> String {
    let id_width = rows.iter().map(|r| r.id.len()).max().unwrap_or(0);
    let width = rows
        .iter()
        .map(|r| r.label.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for row in rows {
        let marker = if row.active { '*' } else { ' ' };
        let windows = match row.windows {
            Some(1) => "1 window".to_string(),
            Some(n) => format!("{n} windows"),
            None => "-".to_string(),
        };
        let folder = row.folder.as_ref().map(Folder::display).unwrap_or_default();
        let line = format!(
            "{marker} {:id_width$}  {:width$}  {windows:<11}  {folder}",
            row.id, row.label
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorders_spaces() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let order = ids(&["s1", "s2", "s3"]);
        assert_eq!(
            moved(order.clone(), "s3".to_string(), 0),
            ids(&["s3", "s1", "s2"])
        );
        assert_eq!(
            moved(order.clone(), "s1".to_string(), 1),
            ids(&["s2", "s1", "s3"])
        );
        assert_eq!(
            moved(order.clone(), "s1".to_string(), 9),
            ids(&["s2", "s3", "s1"])
        );

        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load(dir.path()).unwrap();
        store.entry("s1").order = 10;
        store.entry("s2").order = 10;
        store.entry("s3").order = 30;
        restamp(&mut store, &ids(&["s3", "s1", "s2"]));
        let place = |id| store.get(id).unwrap().order;
        assert!(place("s3") < place("s1") && place("s1") < place("s2"));
        assert!(place("s2") <= 30, "{}", place("s2"));
    }
}
