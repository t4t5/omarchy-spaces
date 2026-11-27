//! `space events`: what changes about the spaces, as a line of JSON for each
//! change, for scripts, hooks and agents. The first line is a `snapshot` of
//! them all, in `space ls --json`'s shape; then come `space.created`,
//! `space.removed`, `space.focused`, `space.changed`, `tab.created`,
//! `tab.removed`, `tab.changed` and `status.changed`.
//!
//! `space watch`, which the plugin runs, makes them: it already reads all
//! there is to know, and it alone follows which agents are done and seen, so
//! the events say what the sidebar shows. It serves them on a socket
//! (`Hub`), and `space events` reads them from it. When the watch restarts,
//! `space events` waits for the next one, whose first line is a new
//! snapshot.

use std::collections::{BTreeMap, HashSet};
use std::fs::{File, TryLockError};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::list::{self, List, Space, Tab, Terminal};
use crate::live::{Live, Row, Snapshot};
use crate::name;
use crate::paths;
use crate::state;
use crate::status::Status;

/// Goes up only when a change would break a script. New event types or
/// fields don't.
const VERSION: u32 = 1;

/// How often `space events` looks for a watch to come back.
const RETRY: Duration = Duration::from_millis(200);

/// How long a line can wait for its reader before the reader is dropped.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// How many lines a subscriber can fall behind before it's dropped, so a
/// reader that stops reading never holds the watch up.
const BACKLOG: usize = 1024;

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

/// Where the watch serves the events: `$XDG_RUNTIME_DIR/omarchy-spaces/events.sock`.
pub fn socket() -> PathBuf {
    paths::runtime_dir().join("events.sock")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Focused {
    space: Option<String>,
    tab: Option<String>,
}

/// All of it as of one moment, which the events are the differences between.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct State {
    focused: Focused,
    list: List,
}

impl State {
    /// What the watch has just read, with agents' statuses as it follows them.
    pub(crate) fn of(snapshot: &Snapshot, live: &Live, rows: &[Row]) -> Self {
        let space = snapshot.active().map(str::to_string);
        let tab = space
            .as_deref()
            .and_then(|space| Some(name::tab_id(space, snapshot.active_tab()?)));
        Self {
            focused: Focused { space, tab },
            list: list::list(rows, Some((snapshot, live))),
        }
    }
}

#[derive(Debug, Serialize)]
struct Envelope<'a> {
    version: u32,
    seq: u64,
    #[serde(flatten)]
    payload: &'a Payload,
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
    #[cfg(test)]
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

    fn snapshot(state: &State) -> Self {
        Payload::Snapshot {
            focused: state.focused.clone(),
            spaces: state.list.spaces.clone(),
        }
    }
}

/// A line as it goes out, `seq` being the last change it takes in.
fn line(seq: u64, payload: &Payload) -> Arc<str> {
    let envelope = Envelope {
        version: VERSION,
        seq,
        payload,
    };
    // Plain data: it always serializes.
    serde_json::to_string(&envelope)
        .expect("events serialize")
        .into()
}

/// What the watch serves `space events` from.
pub struct Hub {
    path: PathBuf,
    inner: Arc<Mutex<Inner>>,
    /// Held for as long as this watch serves, so it alone has the socket:
    /// another watch waits for it rather than replacing the socket under it.
    /// Fields drop after `drop`, so the socket goes before the lock does.
    _serving: File,
}

#[derive(Default)]
struct Inner {
    /// The last change sent.
    seq: u64,
    /// As of the last change, for a new subscriber's snapshot. `None` until
    /// the watch has first read it all.
    state: Option<State>,
    subscribers: Vec<Subscriber>,
}

struct Subscriber {
    lines: SyncSender<Arc<str>>,
    /// Joined before there was anything to show it: its snapshot is due.
    fresh: bool,
}

impl Hub {
    /// Listens on `path`, unless another watch is already serving there, as
    /// when you run one beside the plugin's, or the plugin starts a new one
    /// before the last has ended: then `None`, and that one goes on serving.
    /// Cheap enough to try again and again until it's let go.
    pub fn serve(path: &Path) -> Result<Option<Hub>> {
        let dir = path.parent().unwrap_or(Path::new("."));
        let serving = state::lock_file(dir, ".events.lock")?;
        match serving.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(e)) => return Err(e).context("can't lock the events"),
        }
        // Holding the lock, no other watch is serving: a socket there was
        // left by one that didn't get to tidy up.
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != ErrorKind::NotFound => {
                return Err(e).with_context(|| format!("can't replace {}", paths::tilde(path)));
            }
            _ => {}
        }
        let listener = UnixListener::bind(path)
            .with_context(|| format!("can't listen on {}", paths::tilde(path)))?;
        let inner = Arc::new(Mutex::new(Inner::default()));
        let shared = Arc::clone(&inner);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                join(&shared, stream);
            }
        });
        Ok(Some(Hub {
            path: path.to_path_buf(),
            inner,
            _serving: serving,
        }))
    }

    /// Sends each subscriber what changed since the last `next`, or, to one
    /// that just joined, all of it.
    pub(crate) fn publish(&self, next: State) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.state.as_ref() == Some(&next) {
            return;
        }
        let changes = match &inner.state {
            Some(before) => diff(before, &next),
            None => Vec::new(),
        };
        let first = inner.seq + 1;
        inner.seq += changes.len() as u64;
        if inner.subscribers.is_empty() {
            inner.state = Some(next);
            return;
        }
        let lines: Vec<Arc<str>> = (first..)
            .zip(&changes)
            .map(|(seq, payload)| line(seq, payload))
            .collect();
        let snapshot = inner
            .subscribers
            .iter()
            .any(|s| s.fresh)
            .then(|| line(inner.seq, &Payload::snapshot(&next)));
        inner.subscribers.retain_mut(|subscriber| {
            if subscriber.fresh {
                subscriber.fresh = false;
                return send(subscriber, snapshot.iter());
            }
            send(subscriber, lines.iter())
        });
        inner.state = Some(next);
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A new subscriber: it's sent all of it as it stands, then each change, by
/// a thread of its own, so a slow reader slows only itself.
fn join(inner: &Mutex<Inner>, stream: UnixStream) {
    let (lines, queued) = mpsc::sync_channel::<Arc<str>>(BACKLOG);
    // A reader that has stopped reading is let go of in the end.
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    std::thread::spawn(move || {
        let mut out = stream;
        for line in queued {
            if writeln!(out, "{line}").is_err() {
                return;
            }
        }
    });
    let mut inner = inner.lock().unwrap_or_else(|e| e.into_inner());
    let mut subscriber = Subscriber { lines, fresh: true };
    if let Some(state) = &inner.state {
        let snapshot = line(inner.seq, &Payload::snapshot(state));
        subscriber.fresh = false;
        if !send(&subscriber, std::iter::once(&snapshot)) {
            return;
        }
    }
    inner.subscribers.push(subscriber);
}

/// Queues `lines` for a subscriber; `false` once it's gone or too far behind.
fn send<'a>(subscriber: &Subscriber, lines: impl Iterator<Item = &'a Arc<str>>) -> bool {
    for line in lines {
        match subscriber.lines.try_send(Arc::clone(line)) {
            Ok(()) => {}
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => return false,
        }
    }
    true
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

/// Just the type of a line, to filter it by.
#[derive(Deserialize)]
struct Kind {
    #[serde(rename = "type")]
    kind: String,
}

/// `space events`: prints the watch's events until nobody reads them. When
/// there's no watch, as before the plugin starts or while it restarts one,
/// it waits for one.
pub fn events(types: Option<Vec<String>>, no_snapshot: bool) -> Result<()> {
    let filter = parse_filter(types)?;
    let path = socket();
    let mut out = std::io::stdout().lock();
    // Only the first snapshot is skipped: after a restart, one says what may
    // have changed in between.
    let mut skip_snapshot = no_snapshot;
    let mut told = false;
    loop {
        let stream = match UnixStream::connect(&path) {
            Ok(stream) => stream,
            Err(_) => {
                if !told {
                    eprintln!("space: waiting for `space watch`, which the plugin runs");
                    told = true;
                }
                std::thread::sleep(RETRY);
                continue;
            }
        };
        told = false;
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            let Ok(Kind { kind }) = serde_json::from_str(&line) else {
                continue;
            };
            if kind == "snapshot" && std::mem::take(&mut skip_snapshot) {
                continue;
            }
            if filter.as_ref().is_some_and(|f| !f.contains(&kind)) {
                continue;
            }
            if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
                return Ok(());
            }
        }
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
            meta: BTreeMap::new(),
            folder: Some(Folder::here("/p/app")),
            active,
            windows: Some(1),
            status: None,
            left_at: None,
            tabs: Some(vec![tab(&format!("{id}:1"), 1, "app", true, None)]),
            terminals: vec![term("0x1", &format!("{id}:1"), None)],
            clients: Vec::new(),
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
    fn diffs_metadata_tabs_and_agent_status() {
        let before = state("s1", "s1:1", vec![space("s1", "app", true)]);
        let mut s1 = space("s1", "app", true);
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
        assert_eq!(changed, &["meta", "status"]);

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

    #[test]
    fn one_watch_serves_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.sock");
        let first = Hub::serve(&path).unwrap().unwrap();
        assert!(path.exists());
        // Another watch waits its turn, leaving the first's socket be.
        assert!(Hub::serve(&path).unwrap().is_none());
        assert!(UnixStream::connect(&path).is_ok());

        // Once the first has gone, the next takes over.
        drop(first);
        assert!(!path.exists());
        let second = Hub::serve(&path).unwrap().unwrap();
        assert!(UnixStream::connect(&path).is_ok());

        // A socket left by a watch that was killed, which let go of the lock
        // as it died, is replaced.
        drop(second);
        drop(UnixListener::bind(&path).unwrap());
        assert!(UnixStream::connect(&path).is_err());
        let third = Hub::serve(&path).unwrap().unwrap();
        assert!(UnixStream::connect(&path).is_ok());
        drop(third);
    }
}
