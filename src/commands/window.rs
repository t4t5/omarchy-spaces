//! Window-level commands: `window tab` and `window send`.

use anyhow::Result;

use super::space::{landing_tab, neighbour};
use super::tab::emptied_held;
use super::{SpaceTarget, forget, hold_tabs, remember, selector, take_turn};
use crate::hypr::{self, Hypr};
use crate::live;
use crate::monitors::{Blocks, Strip};
use crate::name::{self, HOME, Location};
use crate::paths;
use crate::state::Last;

/// `space window tab <n>`: the focused window to tab `n` of the active space,
/// following it unless `silent`.
pub fn window_to_tab(slot: u8, silent: bool) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let _tabs = hold_tabs()?;
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    let workspaces = hypr.workspaces()?;
    let key = Strip::new(&workspaces, here.space(), &active, &Blocks::load()).reach(slot);
    if here.key() == Some(key) || !hypr.has_active_window()? {
        return Ok(());
    }

    match &here {
        Location::Space { name, .. } => {
            hypr.dispatch(&hypr::move_window(&selector(name, key), !silent))?
        }
        Location::Home { .. } => {
            let omarchy = if silent {
                format!("o.workspaces.move_window({slot}, false)")
            } else {
                format!("o.workspaces.move_window({slot})")
            };
            hypr.dispatch(&omarchy)
                .or_else(|_| hypr.dispatch(&hypr::move_window(&slot.to_string(), !silent)))?
        }
    }
    if !silent {
        remember(here.space(), key);
    }
    Ok(())
}

/// `space window send <space|next|prev>`: the focused window to another space, on
/// the tab you'd land on going there, and you with it unless `silent`. Sent
/// silently, a tab it leaves empty gives way as closing its last window does.
pub fn send_window(target: SpaceTarget, silent: bool) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let _turn = matches!(target, SpaceTarget::Next | SpaceTarget::Prev)
        .then(take_turn)
        .transpose()?;
    let _tabs = hold_tabs()?;
    if !hypr.has_active_window()? {
        return Ok(());
    }
    let (rows, _) = live::load_rows(Some(&hypr))?;
    let id = match target {
        SpaceTarget::Named(target) => live::resolve(&rows, &target)?,
        SpaceTarget::Next | SpaceTarget::Prev => {
            match neighbour(&rows, target == SpaceTarget::Next) {
                Some(id) => id,
                None => return Ok(()),
            }
        }
    };
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    if here.space() == id {
        return Ok(());
    }

    let last = Last::load(&paths::state_dir())?;
    let key = landing_tab(&hypr, &last, &active, &id)?;
    if id == HOME {
        let slot = name::slot_of(key);
        let omarchy = if silent {
            format!("o.workspaces.move_window({slot}, false)")
        } else {
            format!("o.workspaces.move_window({slot})")
        };
        hypr.dispatch(&omarchy)
            .or_else(|_| hypr.dispatch(&hypr::move_window(&key.to_string(), !silent)))?;
    } else {
        hypr.dispatch(&hypr::move_window(&selector(&id, key), !silent))?;
    }

    if silent {
        return emptied_held(&active.name);
    }
    // Where you left the space, unless that was its last window and it's
    // gone.
    if let Some(left_at) = here.key() {
        let left = here.space();
        let still_there = left == HOME
            || hypr.workspaces()?.iter().any(|ws| {
                ws.windows > 0
                    && matches!(name::locate(ws.id, &ws.name),
                        Location::Space { name, .. } if name == left)
            });
        if still_there {
            remember(left, left_at);
        } else {
            forget(left);
        }
    }
    Ok(())
}
