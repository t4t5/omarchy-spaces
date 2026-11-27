//! `space events`: a public, strictly read-only NDJSON event stream for scripts,
//! hooks, notification bridges, and agents. Emits an initial `snapshot` event
//! (using the same space/tab/terminal schema as `space ls --json`), followed by
//! typed diff events (`space.created`, `space.removed`, `space.focused`,
//! `space.changed`, `tab.created`, `tab.removed`, `tab.changed`,
//! `status.changed`) whenever state changes.
//!
//! Unlike `space watch`, `space events` never locks or writes `spaces.json` or
//! `session.json`, so any number of subscribers can run concurrently.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use serde::Serialize;

use crate::hypr::Hypr;
use crate::list::{self, List, Space, Tab, Terminal};
use crate::live;
use crate::name;
use crate::paths;
use crate::spaces::Store;
use crate::state::Last;
use crate::status::{self, Status};
use crate::terminal;
use crate::watch::{Change, follow, replaced, stamp};

/// Event schema version. Goes up only on breaking changes; new event types or
/// fields do not bump it.
const VERSION: u32 = 1;

const FAST: Duration = Duration::from_millis(200);
const SLOW: Duration = Duration::from_secs(2);
const SETTLE: Duration = Duration::from_millis(20);

/// All event type names accepted by `--type`.
pub const ALL_TYPES: &[&str] = &[
    "snapshot",
    "space.created",
    "space.removed",
    "space.focused",
    "space.changed",
    "tab.created",
    "tab.removed",
    "tab.changed",
    "status.changed",
];

enum Message {
    Hyprland(String),
    HyprlandGone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Focused {
    space: Option<String>,
    tab: Option<String>,
}

/// Canonical state captured at one moment, used for diffing consecutive ticks.
#[derive(Debug, Clone, PartialEq)]
struct State {
    focused: Focused,
    list: List,
}

#[derive(Debug, Serialize)]
struct Envelope {
    version: u32,
    seq: u64,
    #[serde(flatten)]
    payload: Payload,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
enum Payload {
    #[serde(rename = "snapshot")]
    Snapshot {
        focused: Focused,
        spaces: Vec<Space>,
    },
    #[serde(rename = "space.created")]
    SpaceCreated { space: Space },
    #[serde(rename = "space.removed")]
    SpaceRemoved { id: String, space: Space },
    #[serde(rename = "space.focused")]
    SpaceFocused {
        space: Option<String>,
        previous: Option<String>,
        tab: Option<String>,
    },
    #[serde(rename = "space.changed")]
    SpaceChanged {
        space: Space,
        changed: Vec<&'static str>,
    },
    #[serde(rename = "tab.created")]
    TabCreated { space: String, tab: Tab },
    #[serde(rename = "tab.removed")]
    TabRemoved { space: String, id: String, tab: Tab },
    #[serde(rename = "tab.changed")]
    TabChanged {
        space: String,
        tab: Tab,
        changed: Vec<&'static str>,
    },
    #[serde(rename = "status.changed")]
    StatusChanged {
        space: String,
        tab: Option<String>,
        address: String,
        program: Option<String>,
        status: Option<Status>,
        previous: Option<Status>,
    },
}

impl Payload {
    fn kind(&self) -> &'static str {
        match self {
            Payload::Snapshot { .. } => "snapshot",
            Payload::SpaceCreated { .. } => "space.created",
            Payload::SpaceRemoved { .. } => "space.removed",
            Payload::SpaceFocused { .. } => "space.focused",
            Payload::SpaceChanged { .. } => "space.changed",
            Payload::TabCreated { .. } => "tab.created",
            Payload::TabRemoved { .. } => "tab.removed",
            Payload::TabChanged { .. } => "tab.changed",
            Payload::StatusChanged { .. } => "status.changed",
        }
    }
}

/// Validates and parses `--type` filters. `None` means emit all event types.
fn parse_filter(types: Option<Vec<String>>) -> Result<Option<HashSet<String>>> {
    let Some(types) = types else {
        return Ok(None);
    };
    let mut allowed = HashSet::new();
    for raw in types {
        for part in raw.split(',') {
            let name = part.trim();
            if name.is_empty() {
                continue;
            }
            if !ALL_TYPES.contains(&name) {
                bail!(
                    "unknown event type '{name}'; expected one of: {}",
                    ALL_TYPES.join(", ")
                );
            }
            allowed.insert(name.to_string());
        }
    }
    if allowed.is_empty() {
        return Ok(None);
    }
    Ok(Some(allowed))
}

/// Runs the read-only event stream until stdout closes, Hyprland exits, or
/// the `space` binary is replaced.
pub fn events(types: Option<Vec<String>>, no_snapshot: bool) -> Result<()> {
    let filter = parse_filter(types)?;
    let hypr = Hypr::from_env()?;
    let (send, messages) = mpsc::channel();
    let stream = hypr.events()?;
    std::thread::spawn(move || {
        for event in stream {
            if send.send(Message::Hyprland(event)).is_err() {
                return;
            }
        }
        let _ = send.send(Message::HyprlandGone);
    });

    let state_dir = paths::state_dir();
    let mut store = Store::load(&state_dir)?;
    let mut spaces_seen = stamp(&state_dir.join("spaces.json"));
    let mut last = Last::load(&state_dir)?;
    let mut last_seen = stamp(&state_dir.join("last"));
    let mut snapshot = live::snapshot(&hypr)?;
    let mut shells: HashMap<String, (terminal::Place, Option<terminal::Shell>)> = HashMap::new();
    let mut seen: HashMap<String, u64> = HashMap::new();
    let mut turns = status::Turns::default();
    let mut focused_window = hypr.active_window_address()?;
    let mut read_all = true;
    let mut slow_at = Instant::now();
    let mut again_at: Option<Instant> = None;

    let mut prev: Option<State> = None;
    let mut seq: u64 = 0;
    let mut out = std::io::stdout().lock();

    loop {
        let now = Instant::now();
        if now >= slow_at {
            read_all = true;
            slow_at = now + SLOW;
            if replaced() {
                return Ok(());
            }
            refresh(&hypr, &mut snapshot);
        }

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

        let spaces_stamp = stamp(&state_dir.join("spaces.json"));
        if spaces_stamp != spaces_seen
            && let Ok(reloaded) = Store::load(&state_dir)
        {
            store = reloaded;
            spaces_seen = spaces_stamp;
            refresh(&hypr, &mut snapshot);
        }
        let visited = stamp(&state_dir.join("last"));
        if visited != last_seen
            && let Ok(reloaded) = Last::load(&state_dir)
        {
            last = reloaded;
            last_seen = visited;
        }

        let mut live = live::live_from(&snapshot, |window| shells.get(&window.address)?.1.clone());
        turns.follow(live.agents_mut());
        if let Some(focused) = &focused_window {
            for (window, agent) in live.agents() {
                if window == focused {
                    seen.insert(agent.process.clone(), agent.turn);
                }
            }
        }
        let live_keys: HashSet<String> = live.agents().map(|(_, a)| a.process.clone()).collect();
        seen.retain(|key, _| live_keys.contains(key));
        live.set_statuses(|agent| seen.get(&agent.process) == Some(&agent.turn));

        let rows = live::rows_readonly(&store, Some(&live), &last);
        let focused_space = snapshot.active().map(str::to_string);
        let focused_tab = focused_space
            .as_deref()
            .and_then(|space| Some(name::tab_id(space, snapshot.active_tab()?)));
        let next = State {
            focused: Focused {
                space: focused_space,
                tab: focused_tab,
            },
            list: list::list(&rows, Some((&snapshot, &live))),
        };

        let payloads: Vec<Payload> = match &prev {
            None => {
                if no_snapshot {
                    Vec::new()
                } else {
                    vec![Payload::Snapshot {
                        focused: next.focused.clone(),
                        spaces: next.list.spaces.clone(),
                    }]
                }
            }
            Some(before) => diff(before, &next),
        };
        prev = Some(next);

        for payload in payloads {
            if let Some(allowed) = &filter
                && !allowed.contains(payload.kind())
            {
                continue;
            }
            seq += 1;
            let line = serde_json::to_string(&Envelope {
                version: VERSION,
                seq,
                payload,
            })?;
            if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
                return Ok(());
            }
        }

        // Wait for Hyprland events or the next fast shell/store poll.
        let mut changed = Change::Nothing;
        let idle_until = Instant::now() + FAST;
        let idle_until = again_at.map_or(idle_until, |at| at.min(idle_until));
        let mut settled_at: Option<Instant> = None;
        loop {
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
                    let before = focused_window.clone();
                    follow(&event, &mut focused_window);
                    if focused_window != before {
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
        if changed == Change::Place {
            again_at = Some(now + SETTLE);
        } else if again || changed != Change::Nothing {
            read_all = true;
        }
    }
}

fn refresh(hypr: &Hypr, snapshot: &mut live::Snapshot) {
    match live::snapshot(hypr) {
        Ok(read) => *snapshot = read,
        Err(e) => eprintln!("space: warning: {e:#}"),
    }
}

/// Computes typed diff events between two consecutive state snapshots.
fn diff(prev: &State, next: &State) -> Vec<Payload> {
    if prev == next {
        return Vec::new();
    }
    let mut out = Vec::new();

    let prev_spaces: BTreeMap<&str, &Space> = prev
        .list
        .spaces
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect();
    let next_spaces: BTreeMap<&str, &Space> = next
        .list
        .spaces
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect();

    // Created spaces (in sidebar order).
    for space in &next.list.spaces {
        if !prev_spaces.contains_key(space.id.as_str()) {
            out.push(Payload::SpaceCreated {
                space: space.clone(),
            });
        }
    }

    // Removed spaces (in previous sidebar order).
    for space in &prev.list.spaces {
        if !next_spaces.contains_key(space.id.as_str()) {
            out.push(Payload::SpaceRemoved {
                id: space.id.clone(),
                space: space.clone(),
            });
        }
    }

    // Focus change between spaces.
    if prev.focused.space != next.focused.space {
        out.push(Payload::SpaceFocused {
            space: next.focused.space.clone(),
            previous: prev.focused.space.clone(),
            tab: next.focused.tab.clone(),
        });
    }

    // Per-space updates (metadata, tabs, terminal statuses).
    for next_space in &next.list.spaces {
        let Some(&prev_space) = prev_spaces.get(next_space.id.as_str()) else {
            // Also emit status.changed if a brand-new space already has an active status.
            for terminal in &next_space.terminals {
                if terminal.status.is_some() {
                    out.push(Payload::StatusChanged {
                        space: next_space.id.clone(),
                        tab: terminal.tab.clone(),
                        address: terminal.address.clone(),
                        program: terminal.program.clone(),
                        status: terminal.status,
                        previous: None,
                    });
                }
            }
            continue;
        };

        let changed = space_changed_fields(prev_space, next_space);
        if !changed.is_empty() {
            out.push(Payload::SpaceChanged {
                space: next_space.clone(),
                changed,
            });
        }

        diff_tabs(&next_space.id, prev_space, next_space, &mut out);
        diff_statuses(&next_space.id, prev_space, next_space, &mut out);
    }

    out
}

fn space_changed_fields(prev: &Space, next: &Space) -> Vec<&'static str> {
    let mut changed = Vec::new();
    if prev.label != next.label {
        changed.push("label");
    }
    if prev.named != next.named {
        changed.push("named");
    }
    if prev.icon != next.icon {
        changed.push("icon");
    }
    if prev.note != next.note {
        changed.push("note");
    }
    if prev.meta != next.meta {
        changed.push("meta");
    }
    if prev.folder != next.folder {
        changed.push("folder");
    }
    if prev.windows != next.windows {
        changed.push("windows");
    }
    if prev.status != next.status {
        changed.push("status");
    }
    if prev.left_at != next.left_at {
        changed.push("left_at");
    }
    changed
}

fn diff_tabs(space: &str, prev: &Space, next: &Space, out: &mut Vec<Payload>) {
    let prev_tabs = prev.tabs.as_deref().unwrap_or(&[]);
    let next_tabs = next.tabs.as_deref().unwrap_or(&[]);

    let prev_map: BTreeMap<&str, &Tab> = prev_tabs.iter().map(|t| (t.id.as_str(), t)).collect();
    let next_map: BTreeMap<&str, &Tab> = next_tabs.iter().map(|t| (t.id.as_str(), t)).collect();

    for tab in next_tabs {
        if !prev_map.contains_key(tab.id.as_str()) {
            out.push(Payload::TabCreated {
                space: space.to_string(),
                tab: tab.clone(),
            });
        }
    }

    for tab in prev_tabs {
        if !next_map.contains_key(tab.id.as_str()) {
            out.push(Payload::TabRemoved {
                space: space.to_string(),
                id: tab.id.clone(),
                tab: tab.clone(),
            });
        }
    }

    for next_tab in next_tabs {
        let Some(&prev_tab) = prev_map.get(next_tab.id.as_str()) else {
            continue;
        };
        let mut changed = Vec::new();
        if prev_tab.slot != next_tab.slot {
            changed.push("slot");
        }
        if prev_tab.monitor != next_tab.monitor {
            changed.push("monitor");
        }
        if prev_tab.label != next_tab.label {
            changed.push("label");
        }
        if prev_tab.named != next_tab.named {
            changed.push("named");
        }
        if prev_tab.shown != next_tab.shown {
            changed.push("shown");
        }
        if prev_tab.windows != next_tab.windows {
            changed.push("windows");
        }
        if prev_tab.status != next_tab.status {
            changed.push("status");
        }
        if !changed.is_empty() {
            out.push(Payload::TabChanged {
                space: space.to_string(),
                tab: next_tab.clone(),
                changed,
            });
        }
    }
}

fn diff_statuses(space: &str, prev: &Space, next: &Space, out: &mut Vec<Payload>) {
    let prev_terms: BTreeMap<&str, &Terminal> = prev
        .terminals
        .iter()
        .map(|t| (t.address.as_str(), t))
        .collect();
    let next_terms: BTreeMap<&str, &Terminal> = next
        .terminals
        .iter()
        .map(|t| (t.address.as_str(), t))
        .collect();

    let mut addresses: Vec<&str> = prev_terms
        .keys()
        .chain(next_terms.keys())
        .copied()
        .collect();
    addresses.sort_unstable();
    addresses.dedup();

    for address in addresses {
        let prev_term = prev_terms.get(address).copied();
        let next_term = next_terms.get(address).copied();
        let prev_status = prev_term.and_then(|t| t.status);
        let next_status = next_term.and_then(|t| t.status);
        if prev_status != next_status {
            let reference = next_term.or(prev_term).expect("present in one side");
            out.push(Payload::StatusChanged {
                space: space.to_string(),
                tab: reference.tab.clone(),
                address: address.to_string(),
                program: reference.program.clone(),
                status: next_status,
                previous: prev_status,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::Folder;

    fn tab(id: &str, slot: u8, label: &str, shown: bool, status: Option<Status>) -> Tab {
        Tab {
            id: id.into(),
            slot: Some(slot),
            monitor: "eDP-1".into(),
            label: label.into(),
            named: false,
            shown,
            windows: 1,
            status,
        }
    }

    fn term(address: &str, tab: &str, status: Option<Status>) -> Terminal {
        Terminal {
            address: address.into(),
            tab: Some(tab.into()),
            folder: Folder::here("/p/app"),
            program: Some("claude".into()),
            label: "claude".into(),
            status,
        }
    }

    fn space(id: &str, label: &str, active: bool) -> Space {
        Space {
            id: id.into(),
            label: label.into(),
            named: false,
            icon: None,
            note: None,
            meta: BTreeMap::new(),
            folder: Some(Folder::here("/p/app")),
            active,
            windows: Some(1),
            status: None,
            left_at: None,
            tabs: Some(vec![tab(&format!("{id}:1"), 1, "app", true, None)]),
            terminals: vec![term("0x1", &format!("{id}:1"), None)],
        }
    }

    fn state(focused_space: &str, focused_tab: &str, spaces: Vec<Space>) -> State {
        State {
            focused: Focused {
                space: Some(focused_space.into()),
                tab: Some(focused_tab.into()),
            },
            list: List {
                version: list::VERSION,
                spaces,
            },
        }
    }

    #[test]
    fn diffs_space_lifecycle_and_focus() {
        let before = state("home", "home:1", vec![space("home", "home", true)]);
        let after = state(
            "s1",
            "s1:1",
            vec![space("home", "home", false), space("s1", "app", true)],
        );
        let events = diff(&before, &after);
        let kinds: Vec<&str> = events.iter().map(Payload::kind).collect();
        assert_eq!(kinds, ["space.created", "space.focused"]);

        // Removing s1 and focusing back to home.
        let back = diff(&after, &before);
        let kinds: Vec<&str> = back.iter().map(Payload::kind).collect();
        assert_eq!(kinds, ["space.removed", "space.focused"]);
    }

    #[test]
    fn diffs_metadata_note_tabs_and_agent_status() {
        let before = state("s1", "s1:1", vec![space("s1", "app", true)]);
        let mut s1 = space("s1", "app", true);
        s1.note = Some("waiting on review".into());
        s1.meta.insert("pr".into(), serde_json::json!(412));
        s1.status = Some(Status::Waiting);
        s1.tabs.as_mut().unwrap()[0].status = Some(Status::Waiting);
        s1.tabs
            .as_mut()
            .unwrap()
            .push(tab("s1:2", 2, "server", false, None));
        s1.terminals[0].status = Some(Status::Waiting);

        let after = state("s1", "s1:1", vec![s1]);
        let events = diff(&before, &after);
        let kinds: Vec<&str> = events.iter().map(Payload::kind).collect();
        assert_eq!(
            kinds,
            [
                "space.changed",
                "tab.created",
                "tab.changed",
                "status.changed"
            ]
        );

        let Payload::SpaceChanged { changed, .. } = &events[0] else {
            panic!("expected space.changed");
        };
        assert_eq!(changed, &["note", "meta", "status"]);

        let Payload::StatusChanged {
            status, previous, ..
        } = &events[3]
        else {
            panic!("expected status.changed");
        };
        assert_eq!((*previous, *status), (None, Some(Status::Waiting)));
    }

    #[test]
    fn parses_type_filters() {
        assert_eq!(parse_filter(None).unwrap(), None);
        let filter = parse_filter(Some(vec!["space.created,status.changed".into()]))
            .unwrap()
            .unwrap();
        assert!(filter.contains("space.created"));
        assert!(filter.contains("status.changed"));
        assert!(!filter.contains("tab.created"));
        assert!(parse_filter(Some(vec!["not.real".into()])).is_err());
    }
}
