//! `space watch`: the monitors with their tabs, and the spaces (`view.rs`),
//! printed again, one line of JSON each time, whenever they change. The shell plugin draws the bar and the sidebar from
//! it and nothing else, so where you are is decided here alone, each line
//! being the whole of it: the plugin must not put it together from Hyprland's
//! events too, which reach it in another order.
//!
//! Hyprland says when windows and workspaces change, on its event socket, so
//! its state is read again then, and every `SLOW` for what it doesn't say: a
//! window resized, which the saved session (`session.rs`) has. Going somewhere
//! shows at once, and again once the burst it's part of has settled. When
//! Hyprland can't be read, what it last said stands. Nothing
//! says what the terminals' shells do, so those are read from `/proc`,
//! which costs a few small reads: the active space's every `FAST`, the rest
//! every `SLOW`, and all of them at once when a window's title changes, as
//! terminals retitle themselves on `cd` and when a command starts.
//!
//! An agent's `done` shows until you focus its window: then its turn is
//! seen. What agents note has no turns: those are told apart here
//! (`status::Turns`), and a new watch starts over. With `--sounds`, as the
//! plugin runs it, it plays a sound, if the config has them on, when an agent
//! starts needing you or finishes while you aren't looking (`sound.rs`).
//!
//! It exits when Hyprland's event socket closes, when nobody reads what it
//! prints, and when its binary has been replaced (`cargo install`), so that
//! whatever started it can start it again: the plugin restarts it.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Result, bail};

use crate::browser::Browsers;
use crate::config::Config;
use crate::hypr::Hypr;
use crate::live;
use crate::paths;
use crate::restore;
use crate::session;
use crate::spaces::Store;
use crate::state::Last;
use crate::{events, sound, status, terminal, view};

/// How often the active space's terminals' shells are read.
const FAST: Duration = Duration::from_millis(200);
/// How often every other terminal's shell is read, and the binary checked.
const SLOW: Duration = Duration::from_secs(2);
/// How long to let a burst of events settle before reading again: opening a
/// window sends several.
const SETTLE: Duration = Duration::from_millis(20);

/// What wakes the watch.
enum Message {
    Hyprland(String),
    HyprlandGone,
}

pub fn watch(sounds: bool) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (send, messages) = mpsc::channel();
    let events = hypr.events()?;
    std::thread::spawn(move || {
        for event in events {
            if send.send(Message::Hyprland(event)).is_err() {
                return;
            }
        }
        let _ = send.send(Message::HyprlandGone);
    });
    let state_dir = paths::state_dir();

    // What spaces.json held before the snapshot was taken.
    let mut known = Store::load(&state_dir)?.spaces;
    let mut snapshot = live::snapshot(&hypr)?;
    // What each window's shell was doing when last read, by address.
    let mut shells: HashMap<String, (terminal::Place, Option<terminal::Shell>)> = HashMap::new();
    let mut last = Last::load(&state_dir)?;
    // Saving the session is a convenience; the watch goes on without it.
    let mut saver = session::Saver::new(&state_dir, &hypr.instance)
        .inspect_err(|e| eprintln!("space: warning: {e:#}"))
        .ok();
    // What the browser's windows hold, from before this watch to start with.
    let mut browsers = Browsers::new(
        saver
            .as_mut()
            .map(|saver| restore::known_browsers(&saver.before()))
            .unwrap_or_default(),
    );
    let mut last_seen = stamp(&state_dir.join("last"));
    // The turn each agent was on when you last looked at it.
    let mut seen: HashMap<String, u64> = HashMap::new();
    // What each agent was last doing, to ring on a change.
    let mut words: HashMap<String, String> = HashMap::new();
    let mut turns = status::Turns::default();
    // `None` until the config can be read.
    let mut config = read_config();
    let mut config_seen = stamp(&paths::config_file());
    let mut focused = hypr.active_window_address()?;
    let mut read_all = true;
    let mut slow_at = Instant::now();
    // When to read again after a switch shown before its burst settled.
    let mut again_at: Option<Instant> = None;
    let mut printed = String::new();
    let mut out = std::io::stdout().lock();
    // What `space events` reads. A convenience too: the watch goes on
    // without it, and without it when another watch already serves them,
    // retrying on the slow tick so an overlapping restart takes over cleanly.
    let mut hub = serve_hub();

    loop {
        let now = Instant::now();
        if now >= slow_at {
            read_all = true;
            slow_at = now + SLOW;
            if replaced() {
                return Ok(());
            }
            if hub.is_none() {
                hub = serve_hub();
            }
            // Hyprland has no event for a window resized or a split turned.
            refresh(&hypr, &mut snapshot);
            match config.as_ref().map(|config| config.restore.browsers) {
                Some(true) => browsers.refresh(&snapshot.browser_windows, now),
                Some(false) => browsers.forget(),
                // It may be about to say not to.
                None => {}
            }
        }

        // What the terminals' shells are doing: the active space's now, and
        // new ones', the rest when due.
        let live_addresses: HashSet<String> = snapshot
            .space_windows
            .iter()
            .map(|w| w.address.clone())
            .collect();
        shells.retain(|address, _| live_addresses.contains(address));
        for window in &snapshot.space_windows {
            let known = shells.get(&window.address);
            if read_all
                || snapshot.active() == Some(window.space.as_str())
                || known.is_none_or(|(place, _)| *place != window.place)
            {
                let before = known.and_then(|(_, shell)| Some(shell.as_ref()?.pid));
                let shell = terminal::shell(window.place, before);
                shells.insert(window.address.clone(), (window.place, shell));
            }
        }
        read_all = false;

        // Visits change from other `space` commands.
        let visited = stamp(&state_dir.join("last"));
        if visited != last_seen {
            last = Last::load(&state_dir)?;
            last_seen = visited;
        }
        let configured = stamp(&paths::config_file());
        if configured != config_seen {
            // Halfway through a change, or mistaken, what it last said stands.
            config = read_config().or(config);
            config_seen = configured;
        }

        let mut live = live::live_from(&snapshot, |window| shells.get(&window.address)?.1.clone());
        turns.follow(live.agents_mut());
        // The agent in the window you're looking at is seen.
        if let Some(focused) = &focused {
            for (window, agent) in live.agents() {
                if window == focused {
                    seen.insert(agent.process.clone(), agent.turn);
                }
            }
        }
        // A change in what an agent is doing may call for a sound. Statuses
        // already present when `watch` starts don't ring.
        for (window, agent) in live.agents() {
            let before = words.insert(agent.process.clone(), agent.word.clone());
            if !sounds {
                continue;
            }
            let looking = focused.as_deref() == Some(window);
            if let Some(before) = before
                && let Some(config) = &config
                && let Some(alert) = sound::alert(&before, &agent.word, looking)
            {
                sound::play(config, alert);
            }
        }
        let live_keys: HashSet<String> = live.agents().map(|(_, a)| a.process.clone()).collect();
        seen.retain(|key, _| live_keys.contains(key));
        words.retain(|key, _| live_keys.contains(key));
        live.set_statuses(|agent| seen.get(&agent.process) == Some(&agent.turn));

        let mut store = Store::lock(&state_dir)?;
        // Another command changed the spaces, perhaps adding one that the
        // snapshot is too old to have, which it would then prune.
        if store.spaces != known {
            known = std::mem::take(&mut store.spaces);
            drop(store);
            snapshot = live::snapshot(&hypr)?;
            read_all = true;
            continue;
        }
        let rows = live::rows(&mut store, Some(&live), &last);
        if let Err(e) = store.save() {
            eprintln!("space: warning: {e:#}");
        }
        known = store.saved().clone();
        let session = restore::session_of(
            &hypr.instance,
            &rows,
            &live,
            &snapshot,
            &browsers,
            &saver
                .as_mut()
                .map(session::Saver::before)
                .unwrap_or_default(),
        );
        // Unlocked before printing, which waits for the reader.
        drop(store);
        if let Some(saver) = &mut saver
            && let Err(e) = saver.update(session, Instant::now())
        {
            eprintln!("space: warning: {e:#}");
        }
        if let Some(hub) = &hub {
            hub.publish(events::State::of(&snapshot, &live, &rows));
        }
        let json = serde_json::to_string(&view::state(&snapshot, &live, &rows))?;
        if json != printed {
            // Nobody reading any more: done.
            if writeln!(out, "{json}").and_then(|()| out.flush()).is_err() {
                return Ok(());
            }
            printed = json;
        }

        // Wait for something to happen, then let the burst settle.
        let mut changed = Change::Nothing;
        let idle_until = Instant::now() + FAST;
        let idle_until = again_at.map_or(idle_until, |at| at.min(idle_until));
        let mut settled_at: Option<Instant> = None;
        loop {
            // A switch takes in only what has already come with it.
            let until = match settled_at {
                Some(_) if changed == Change::Place => Instant::now(),
                Some(at) => at,
                None => idle_until,
            };
            let wait = until.saturating_duration_since(Instant::now());
            let message = match messages.recv_timeout(wait) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => bail!("Hyprland's events stopped"),
            };
            settled_at.get_or_insert_with(|| Instant::now() + SETTLE);
            match message {
                Message::Hyprland(event) => {
                    changed = changed.max(Change::of(&event));
                    let before = focused.clone();
                    follow(&event, &mut focused);
                    if focused != before {
                        changed = changed.max(Change::Folders);
                    }
                }
                Message::HyprlandGone => bail!("Hyprland's events stopped"),
            }
        }
        let now = Instant::now();
        let again = again_at.is_some_and(|at| at <= now);
        if again {
            again_at = None;
        }
        if again || changed >= Change::Windows {
            refresh(&hypr, &mut snapshot);
        }
        // Of a switch, only where you are now is read, to show it at once.
        if changed == Change::Place {
            again_at = Some(now + SETTLE);
        } else if again || changed != Change::Nothing {
            read_all = true;
        }
    }
}

/// Reads Hyprland again. When it can't be, what it last said stands, rather
/// than half of it: the next event or slow round reads it anew, and if it has
/// gone, its events end.
fn refresh(hypr: &Hypr, snapshot: &mut live::Snapshot) {
    match live::snapshot(hypr) {
        Ok(read) => *snapshot = read,
        Err(e) => eprintln!("space: warning: {e:#}"),
    }
}

/// What a Hyprland event can change about the spaces, least to most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Change {
    Nothing,
    /// A title changed, which terminals do on `cd` and as a command starts:
    /// read their shells.
    Folders,
    /// Windows or workspaces came, went or moved: read Hyprland's state.
    Windows,
    /// You, or a monitor, went to another workspace: the same, at once.
    Place,
}

impl Change {
    fn of(event: &str) -> Self {
        let name = event.split_once(">>").map_or(event, |(name, _)| name);
        match name {
            "windowtitle" | "windowtitlev2" => Change::Folders,
            "workspace" | "workspacev2" | "focusedmon" | "focusedmonv2" => Change::Place,
            "openwindow" | "closewindow" | "movewindow" | "movewindowv2" | "createworkspace"
            | "createworkspacev2" | "destroyworkspace" | "destroyworkspacev2"
            | "renameworkspace" | "moveworkspace" | "moveworkspacev2" | "monitoradded"
            | "monitoraddedv2" | "monitorremoved" | "monitorremovedv2" | "changefloatingmode"
            | "configreloaded" => Change::Windows,
            _ => Change::Nothing,
        }
    }
}

/// Notes the focused window from Hyprland's events.
fn follow(event: &str, focused: &mut Option<String>) {
    if let Some(("activewindowv2", data)) = event.split_once(">>") {
        // The events give addresses without Hyprland's `0x`.
        let data = data.trim().trim_start_matches("0x");
        *focused = (!data.is_empty() && data != ",").then(|| format!("0x{data}"));
    }
}

/// Opens the events socket unless another watch is already serving there.
fn serve_hub() -> Option<events::Hub> {
    events::Hub::serve(&events::socket())
        .inspect_err(|e| eprintln!("space: warning: {e:#}"))
        .ok()
        .flatten()
}

/// The config, or `None` when it can't be read.
fn read_config() -> Option<Config> {
    Config::load()
        .inspect_err(|e| eprintln!("space: warning: {e:#}"))
        .ok()
}

/// When a file last changed, and its size, to notice another `space`
/// writing it.
fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Whether this program's binary has been replaced since it started, as
/// `cargo install` does: the kernel marks the running one deleted.
fn replaced() -> bool {
    std::fs::read_link("/proc/self/exe")
        .is_ok_and(|exe| exe.to_string_lossy().ends_with(" (deleted)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_events_by_what_they_change() {
        assert_eq!(Change::of("windowtitlev2>>55a4,~/ren"), Change::Folders);
        assert_eq!(
            Change::of("openwindow>>55a4,s1:1,kitty,zsh"),
            Change::Windows
        );
        assert_eq!(Change::of("workspacev2>>-1337,s1:2"), Change::Place);
        assert_eq!(Change::of("focusedmonv2>>DP-1,11"), Change::Place);
        assert_eq!(Change::of("monitorremoved>>DP-1"), Change::Windows);
        assert_eq!(Change::of("activewindowv2>>55a4"), Change::Nothing);
        assert_eq!(Change::of("garbage"), Change::Nothing);
        assert!(Change::Place > Change::Windows && Change::Windows > Change::Folders);
        assert!(Change::Folders > Change::Nothing);
    }

    #[test]
    fn follows_the_focused_window() {
        let mut focused = None;
        follow("activewindowv2>>55a4", &mut focused);
        assert_eq!(focused.as_deref(), Some("0x55a4"));
        follow("openwindow>>66b5,s1:1,kitty,zsh", &mut focused);
        assert_eq!(focused.as_deref(), Some("0x55a4"));
        follow("activewindowv2>>", &mut focused);
        assert_eq!(focused, None);
    }

    #[test]
    fn this_binary_is_not_replaced() {
        assert!(!replaced());
    }
}
