//! The commands. None of them keeps state about windows: each asks Hyprland
//! where things are, then dispatches. What Hyprland can't hold about a space,
//! its name and the terminal it follows, is in the store (`spaces.rs`).

pub mod space;
pub mod tab;
pub mod window;

pub use crate::restore::restore;
pub use space::{
    NewOptions, close, current, go, icon, ls, meta_get, meta_rm, meta_set, new, note, rename,
    reorder, step,
};
pub use tab::{
    NewTabOptions, Target, close_tab, emptied, go_to_named_tab, go_to_tab, new_tab_cmd, rename_tab,
    renumber, reorder_tab, send_tab,
};
pub use window::{send_window, window_to_tab};

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::context;
use crate::hypr::{self, Hypr};
use crate::launch;
use crate::live;
use crate::name::{self, HOME};
use crate::paths;
use crate::reserve;
use crate::spaces::Store;
use crate::state::{self, Last};
use crate::terminal;

/// How long a new space or tab waits for its terminal before going there
/// anyway, where the terminal then shows up when it can.
const OPEN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Which space `space window send` and `space tab send` send to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceTarget {
    /// What it's called, or its id; `home` for the plain workspaces.
    Named(String),
    Next,
    Prev,
}

impl SpaceTarget {
    pub fn of(raw: &str) -> Self {
        match raw {
            "next" => SpaceTarget::Next,
            "prev" => SpaceTarget::Prev,
            _ => SpaceTarget::Named(raw.to_string()),
        }
    }
}

/// `space terminal [--space <space>]`: a terminal in the folder you are in,
/// or the target space's, so an empty tab opens on its folder rather than home.
pub fn open_terminal(space: Option<String>) -> Result<()> {
    let (dir, space_id) = match Hypr::from_env() {
        Ok(hypr) => match space {
            Some(target) => {
                let (rows, _) = live::load_rows(Some(&hypr))?;
                let id = live::resolve(&rows, &target)?;
                let dir = live::space_folder(&hypr, &id)?;
                (dir, Some(id))
            }
            None => live::caller_folder(&hypr)?,
        },
        Err(e) if space.is_some() => return Err(e),
        Err(_) => (paths::home(), None),
    };
    let env: Vec<(&str, &str)> = space_id
        .as_deref()
        .map(|id| vec![(context::SPACE_ID_ENV, id)])
        .unwrap_or_default();
    terminal::launch_env(&launch::terminal(), &dir, &env)
}

/// Resolves an optional space argument: explicit target, else the caller's
/// space (from `/proc` or `SPACE_ID`), else the focused space.
pub(super) fn target_space(
    target: Option<String>,
    rows: &[live::Row],
    seen: Option<&live::Seen>,
) -> Result<String> {
    if let Some(target) = target {
        return live::resolve(rows, &target);
    }
    if let Some((snapshot, live)) = seen
        && let Some(caller) = context::resolve(snapshot, Some(live))
    {
        return Ok(caller.space);
    }
    Ok(rows
        .iter()
        .find(|r| r.active)
        .map(|r| r.id.clone())
        .unwrap_or_else(|| HOME.to_string()))
}

/// Canonicalizes a directory argument and verifies it exists and is a folder.
pub(super) fn resolve_dir(dir: PathBuf) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("can't read the current folder")?;
    let dir = cwd.join(dir);
    let dir =
        std::fs::canonicalize(&dir).with_context(|| format!("no folder {}", paths::tilde(&dir)))?;
    if !dir.is_dir() {
        bail!("{} is not a folder", paths::tilde(&dir));
    }
    Ok(dir)
}

/// An id for a new space whose first terminal opens on tab `key`, reserved
/// until that terminal is up: the one it `had`, if it's being restored.
pub(crate) fn fresh_space(hypr: &Hypr, key: u8, had: Option<&str>) -> Result<String> {
    let (id, _) = reserve::reserve(&paths::reserved_dir(), |reserved| {
        let mut taken = live::snapshot(hypr)?.spaces();
        taken.extend(reserved.iter().map(|(space, _)| space.clone()));
        let mut store = Store::lock(&paths::state_dir())?;
        let id = match had.filter(|had| store.claim_id(had, &taken)) {
            Some(had) => had.to_string(),
            None => store.fresh_id(&taken),
        };
        store.save()?;
        Ok((id, key))
    })?;
    Ok(id)
}

/// Opens a terminal in `dir` on `workspace` (a space's tab) while you stay
/// where you are, then goes there with `arrive` once its window is up, so you
/// never see the tab empty. Hyprland places it by an exec rule, which follows
/// the process it starts down to the terminal. The rule also skips its
/// pop-in, which would otherwise still be fading it in from nothing as you
/// arrive; once you're there it animates like any other window. The tab,
/// if reserved, is released once its window is up.
pub(super) fn open_terminal_then(
    hypr: &Hypr,
    dir: &Path,
    workspace: &str,
    env: &[(&str, &str)],
    arrive: impl FnOnce() -> Result<()>,
) -> Result<Option<String>> {
    let events = hypr.events().ok();
    let command = terminal::command_in(&launch::terminal(), dir, env);
    let on = hypr::selector_for(&hypr.workspaces().unwrap_or_default(), workspace);
    let placed = hypr
        .dispatch(&hypr::exec_on(&command, &format!("{on} silent")))
        .is_ok();
    let window = match (placed, events) {
        (true, Some(events)) => wait_for_window(&events, workspace, OPEN_TIMEOUT, |_| true),
        (true, None) => None,
        (false, _) => {
            terminal::launch_env(&launch::terminal(), dir, env)?;
            None
        }
    };
    if window.is_some() {
        reserve::release(&paths::reserved_dir(), workspace);
    }
    arrive()?;
    if let Some(address) = &window
        && let Err(e) = hypr.dispatch(&hypr::animate_again(address))
    {
        eprintln!("space: warning: {e:#}");
    }
    Ok(window)
}

/// Waits until a window that `ours` accepts by its address opens on
/// `workspace` (Hyprland's `openwindow` event: address, workspace, class,
/// title) and returns its address, or `None` once `timeout` passes.
pub(crate) fn wait_for_window(
    events: &std::sync::mpsc::Receiver<String>,
    workspace: &str,
    timeout: std::time::Duration,
    ours: impl Fn(&str) -> bool,
) -> Option<String> {
    let deadline = std::time::Instant::now() + timeout;
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        let event = events.recv_timeout(left).ok()?;
        if let Some(data) = event.strip_prefix("openwindow>>") {
            let mut fields = data.split(',');
            if let (Some(address), Some(on)) = (fields.next(), fields.next())
                && on == workspace
            {
                let address = format!("0x{}", address.trim_start_matches("0x"));
                if ours(&address) {
                    return Some(address);
                }
            }
        }
    }
    None
}

/// Waits for any other step to the next or previous space or tab to finish,
/// until it's dropped. Each is from where you are, so two keypresses handled
/// at once would both go from the same place, and one be lost.
pub(super) fn take_turn() -> Result<std::fs::File> {
    state::lock(&paths::state_dir(), ".step.lock")
}

/// Held by the commands that can end a tab while they run, until it's
/// dropped. `space renumber`, which Hyprland runs as a tab ends, waits for
/// it, so whatever they note about the tabs, by their numbers, is renumbered
/// along with them rather than after.
pub(super) fn hold_tabs() -> Result<std::fs::File> {
    state::lock(&paths::state_dir(), ".tabs.lock")
}

/// `order` with `item` taken out and put back at `index`, or last.
pub(super) fn moved<T: PartialEq>(mut order: Vec<T>, item: T, index: usize) -> Vec<T> {
    order.retain(|other| *other != item);
    let index = index.min(order.len());
    order.insert(index, item);
    order
}

/// Focuses a space's tab. In `home`, that is Omarchy's own workspace on the
/// focused monitor, or the plain workspace outside Omarchy.
pub(super) fn focus(hypr: &Hypr, space: &str, key: u8) -> Result<()> {
    if space == HOME {
        hypr.dispatch(&format!("o.workspaces.focus({})", name::slot_of(key)))
            .or_else(|_| hypr.dispatch(&hypr::focus_workspace(&key.to_string())))
    } else {
        let selector = selector(&hypr.workspaces()?, space, key);
        hypr.dispatch(&hypr::focus_workspace(&selector))
    }
}

/// `key`, or, if it'd be a new tab whose number is a renamed tab's address, so
/// that going to it would find that tab, the next free one in its block:
/// `space renumber` numbers it as the next tab.
pub(super) fn fresh_key(workspaces: &[hypr::Workspace], space: &str, key: u8) -> u8 {
    if space == HOME {
        return key;
    }
    let usable = |key: u8| {
        let name = name::workspace_name(space, key);
        workspaces.iter().any(|ws| ws.name == name) || hypr::name_is_free(workspaces, &name)
    };
    let block = name::block_of(key);
    (key..=u8::MAX)
        .take_while(|next| name::block_of(*next) == block)
        .find(|next| usable(*next))
        .unwrap_or(key)
}

/// Hyprland's selector for a space's tab, found by its name as `workspaces`
/// have it now (see `hypr::selector_for`). It's also how a tab that isn't
/// there yet is made, so it skips a number that's a renamed tab's address
/// (`fresh_key`): every command that goes to a tab or puts a window on one
/// does so by this.
pub(super) fn selector(workspaces: &[hypr::Workspace], space: &str, key: u8) -> String {
    let key = fresh_key(workspaces, space, key);
    hypr::selector_for(workspaces, &name::workspace_name(space, key))
}

/// Hyprland's selector for a tab, as a session has it.
pub(crate) fn session_selector(workspaces: &[hypr::Workspace], space: &str, tab: u8) -> String {
    hypr::selector_for(workspaces, &session_workspace(space, tab))
}

/// `session_selector`, by the workspaces Hyprland has now.
pub(crate) fn live_selector(hypr: &Hypr, space: &str, tab: u8) -> String {
    session_selector(&hypr.workspaces().unwrap_or_default(), space, tab)
}

/// Hyprland's name for a tab, as a session has it.
pub(crate) fn session_workspace(space: &str, tab: u8) -> String {
    if space == HOME {
        tab.to_string()
    } else {
        name::workspace_name(space, tab)
    }
}

/// Records the tab the active space is on. Failing to is only a warning: the
/// switch itself has happened.
pub(crate) fn remember(space: &str, key: u8) {
    if let Err(e) = Last::update(&paths::state_dir(), |last| last.set(space, key)) {
        eprintln!("space: warning: {e:#}");
    }
}

/// Forgets where a space was left, as it's gone.
pub(super) fn forget(space: &str) {
    if let Err(e) = Last::update(&paths::state_dir(), |last| last.remove(space)) {
        eprintln!("space: warning: {e:#}");
    }
}
