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

/// The window a command moves, and the workspace it is on.
struct Mover {
    /// Where it is now: the active workspace, or the named window's.
    from: hypr::Workspace,
    /// Its address, when the caller named it rather than taking the focused one.
    address: Option<String>,
}

impl Mover {
    /// The focused window, or the one at `address`; `None` when there's no
    /// such window. A picker names the window it opened on, as focus can move
    /// meanwhile (to whatever is under the pointer, when it closes).
    fn find(hypr: &Hypr, address: Option<&str>) -> Result<Option<Self>> {
        let Some(address) = address else {
            if !hypr.has_active_window()? {
                return Ok(None);
            }
            return Ok(Some(Self {
                from: hypr.active_workspace()?,
                address: None,
            }));
        };
        let clients = hypr.clients()?;
        let Some(client) = clients
            .iter()
            .find(|client| client.address.eq_ignore_ascii_case(address))
        else {
            return Ok(None);
        };
        // A window in a special workspace (the scratchpad) isn't in any
        // space: it goes from where you are.
        if client.workspace.name.starts_with("special:") {
            return Ok(Some(Self {
                from: hypr.active_workspace()?,
                address: Some(client.address.clone()),
            }));
        }
        let from = hypr
            .workspaces()?
            .into_iter()
            .find(|ws| ws.name == client.workspace.name);
        Ok(from.map(|from| Self {
            from,
            address: Some(client.address.clone()),
        }))
    }

    /// Moves it to a workspace by selector, and goes along unless `silent`.
    fn to(&self, hypr: &Hypr, selector: &str, silent: bool) -> Result<()> {
        hypr.dispatch(&match &self.address {
            Some(address) => hypr::move_window_of(address, selector, !silent),
            None => hypr::move_window(selector, !silent),
        })
    }

    /// Moves it to the plain workspace numbered `number`, the way Omarchy
    /// does for the focused window, by its `slot` on the monitor. A named
    /// window goes by number alone: Omarchy's own move also pulls a workspace
    /// that is shown on another monitor back, which this doesn't.
    fn to_home(&self, hypr: &Hypr, slot: u8, number: u8, silent: bool) -> Result<()> {
        if self.address.is_some() {
            return self.to(hypr, &number.to_string(), silent);
        }
        let omarchy = if silent {
            format!("o.workspaces.move_window({slot}, false)")
        } else {
            format!("o.workspaces.move_window({slot})")
        };
        hypr.dispatch(&omarchy)
            .or_else(|_| self.to(hypr, &number.to_string(), silent))
    }
}

/// `space window tab <n>`: the focused window, or the one at `window`, to tab
/// `n` of the space it is in, following it unless `silent`.
pub fn window_to_tab(slot: u8, silent: bool, window: Option<&str>) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let _tabs = hold_tabs()?;
    let Some(mover) = Mover::find(&hypr, window)? else {
        return Ok(());
    };
    let active = &mover.from;
    let here = name::locate(active.id, &active.name);
    let workspaces = hypr.workspaces()?;
    let key = Strip::new(&workspaces, here.space(), active, &Blocks::load()).reach(slot);
    if here.key() == Some(key) {
        return Ok(());
    }

    match &here {
        Location::Space { name, .. } => {
            mover.to(&hypr, &selector(&workspaces, name, key), silent)?
        }
        Location::Home { .. } => mover.to_home(&hypr, slot, key, silent)?,
    }
    if !silent {
        remember(here.space(), key);
    }
    Ok(())
}

/// `space window send <space|next|prev>`: the focused window, or the one at
/// `window`, to another space, on the tab you'd land on going there, and you
/// with it unless `silent`. Sent silently, a tab it leaves empty gives way as
/// closing its last window does.
pub fn send_window(target: SpaceTarget, silent: bool, window: Option<&str>) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let _turn = matches!(target, SpaceTarget::Next | SpaceTarget::Prev)
        .then(take_turn)
        .transpose()?;
    let _tabs = hold_tabs()?;
    let Some(mover) = Mover::find(&hypr, window)? else {
        return Ok(());
    };
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
    // Where the window is, which isn't where you are when it was named.
    let active = mover.from.clone();
    let here = name::locate(active.id, &active.name);
    if here.space() == id {
        return Ok(());
    }

    let last = Last::load(&paths::state_dir())?;
    let key = landing_tab(&hypr, &last, &active, &id)?;
    if id == HOME {
        mover.to_home(&hypr, name::slot_of(key), key, silent)?;
    } else {
        mover.to(&hypr, &selector(&hypr.workspaces()?, &id, key), silent)?;
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
