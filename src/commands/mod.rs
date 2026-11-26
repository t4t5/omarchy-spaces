//! The commands. None of them keeps state about windows: each asks Hyprland
//! where things are, then dispatches. What Hyprland can't hold about a space,
//! its name and the terminal it follows, is in the store (`spaces.rs`).

pub mod space;
pub mod tab;
pub mod window;

pub use crate::restore::restore;
pub use space::{close, go, icon, ls, new, rename, reorder, step};
pub use tab::{
    Target, close_tab, emptied, go_to_named_tab, go_to_tab, rename_tab, renumber, reorder_tab,
    send_tab,
};
pub use window::{send_window, window_to_tab};

use std::path::Path;

use anyhow::Result;

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

/// `space terminal`: a terminal in the folder you are in, so an empty tab of
/// a space opens on its folder rather than home.
pub fn open_terminal() -> Result<()> {
    let dir = match Hypr::from_env() {
        Ok(hypr) => live::here_folder(&hypr)?,
        Err(_) => paths::home(),
    };
    terminal::launch(&launch::terminal(), &dir)
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
    arrive: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let events = hypr.events().ok();
    let command = terminal::command_in(&launch::terminal(), dir, &[]);
    let placed = hypr
        .dispatch(&hypr::exec_on(
            &command,
            &format!("name:{workspace} silent"),
        ))
        .is_ok();
    let window = match (placed, events) {
        (true, Some(events)) => wait_for_window(&events, workspace, OPEN_TIMEOUT, |_| true),
        (true, None) => None,
        (false, _) => {
            terminal::launch(&launch::terminal(), dir)?;
            None
        }
    };
    if window.is_some() {
        reserve::release(&paths::reserved_dir(), workspace);
    }
    arrive()?;
    if let Some(address) = window
        && let Err(e) = hypr.dispatch(&hypr::animate_again(&address))
    {
        eprintln!("space: warning: {e:#}");
    }
    Ok(())
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
        hypr.dispatch(&hypr::focus_workspace(&selector(space, key)))
    }
}

pub(super) fn selector(space: &str, key: u8) -> String {
    format!("name:{}", name::workspace_name(space, key))
}

/// Hyprland's selector for a tab, as a session has it.
pub(crate) fn session_selector(space: &str, tab: u8) -> String {
    format!(
        "{}{}",
        if space == HOME { "" } else { "name:" },
        session_workspace(space, tab)
    )
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
