//! Where a `space` command was called from: the Hyprland window whose process
//! tree spawned it (read live from `/proc`), or else `SPACE_ID` from its
//! environment. Walking `/proc` stays accurate even if a window was moved to
//! another space or its tab was renumbered, and works for terminals opened
//! outside `space` (such as `Super+Enter`).

use std::collections::HashSet;

use serde::Serialize;

use crate::live::{Live, Snapshot};
use crate::name::{self, HOME};

/// Set on terminals launched by `space` as an ambient hint and fallback when
/// process ancestry can't be matched to a Hyprland window.
pub const SPACE_ID_ENV: &str = "SPACE_ID";

/// How the caller's location was determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Matched an ancestor process to a live Hyprland window.
    Window,
    /// Read from `SPACE_ID` in the environment.
    Env,
    /// Not called from inside a known space; fell back to the focused workspace.
    Focused,
}

/// The space (and, when matched to a live window, tab and window address) from
/// which `space` was invoked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub space: String,
    pub tab_key: Option<u8>,
    pub window: Option<String>,
    pub source: Source,
}

impl Caller {
    /// The caller's tab id (`s1:2`, `home:1`), if known.
    pub fn tab_id(&self) -> Option<String> {
        self.tab_key.map(|key| name::tab_id(&self.space, key))
    }
}

/// Resolves the caller's live space/tab/window, if `space` was run from inside
/// a space's window or with a valid live `SPACE_ID` in its environment.
pub(crate) fn resolve(snapshot: &Snapshot, live: Option<&Live>) -> Option<Caller> {
    let chain = ancestors(std::process::id());
    if let Some(caller) = from_ancestors(&chain, snapshot, live) {
        return Some(caller);
    }
    let env_id = std::env::var(SPACE_ID_ENV).ok();
    from_env(env_id.as_deref(), snapshot)
}

/// Walks parent PIDs in `/proc` starting from `pid` (nearest ancestor first).
pub(crate) fn ancestors(pid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut current = pid;
    for _ in 0..64 {
        if current <= 1 || !seen.insert(current) {
            break;
        }
        out.push(current);
        let Some(ppid) = parent_pid(current) else {
            break;
        };
        current = ppid;
    }
    out
}

fn parent_pid(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, rest) = stat.rsplit_once(") ")?;
    // After ") ": state, ppid, ...
    let ppid: i64 = rest.split_whitespace().nth(1)?.parse().ok()?;
    u32::try_from(ppid).ok().filter(|&p| p > 0)
}

/// Matches a list of ancestor PIDs (nearest first) against live terminal
/// shells first (which disambiguates single-instance terminal servers), then
/// against Hyprland window PIDs.
pub(crate) fn from_ancestors(
    chain: &[u32],
    snapshot: &Snapshot,
    live: Option<&Live>,
) -> Option<Caller> {
    if let Some(live) = live {
        for &pid in chain {
            for (space, terminals) in &live.terminals {
                if let Some(terminal) = terminals.iter().find(|t| t.shell.pid == pid) {
                    return Some(Caller {
                        space: space.clone(),
                        tab_key: (terminal.tab > 0).then_some(terminal.tab),
                        window: Some(terminal.address.clone()),
                        source: Source::Window,
                    });
                }
            }
        }
    }

    for &pid in chain {
        // Only match by client PID when that process owns a single window, so
        // a shared-process server (like `foot --server`) doesn't guess wrong.
        if let Some(window) = snapshot
            .space_windows
            .iter()
            .find(|w| w.place.pid == pid && w.place.of == 1)
        {
            return Some(Caller {
                space: window.space.clone(),
                tab_key: window.tab,
                window: Some(window.address.clone()),
                source: Source::Window,
            });
        }
    }

    None
}

/// Validates a `SPACE_ID` environment variable against the currently live spaces.
pub(crate) fn from_env(raw: Option<&str>, snapshot: &Snapshot) -> Option<Caller> {
    let id = raw?.trim();
    if id == HOME || (name::is_id(id) && snapshot.spaces().contains(id)) {
        return Some(Caller {
            space: id.to_string(),
            tab_key: None,
            window: None,
            source: Source::Env,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::Folder;
    use crate::layout::Rect;
    use crate::live::{SpaceWindow, Terminal};
    use crate::terminal::{Place, Shell};

    fn space_window(address: &str, pid: u32, of: usize, space: &str, tab: u8) -> SpaceWindow {
        SpaceWindow {
            address: address.into(),
            place: Place { pid, nth: 0, of },
            space: space.into(),
            tab: Some(tab),
            class: "terminal".into(),
            rect: Rect::default(),
            floating: false,
            fullscreen: 0,
            grouped: Vec::new(),
            focus: 0,
        }
    }

    #[test]
    fn walks_real_proc_ancestors() {
        let chain = ancestors(std::process::id());
        assert_eq!(chain.first().copied(), Some(std::process::id()));
        assert!(chain.len() >= 2, "expected parent pid in {chain:?}");
    }

    #[test]
    fn matches_nearest_window_or_shell_pid() {
        let mut snapshot = Snapshot::default();
        snapshot
            .space_windows
            .push(space_window("0x1", 200, 1, "s1", 1));
        snapshot
            .space_windows
            .push(space_window("0x2", 300, 1, "s2", 3));
        // Multi-window server: client PID 400 owns 2 windows.
        snapshot
            .space_windows
            .push(space_window("0x3", 400, 2, "s3", 1));
        snapshot
            .space_windows
            .push(space_window("0x4", 400, 2, "s4", 2));

        let mut live = Live::default();
        live.terminals.insert(
            "s4".into(),
            vec![Terminal {
                address: "0x4".into(),
                tab: 2,
                pid: 400,
                shell: Shell {
                    pid: 450,
                    folder: Folder::here("/p"),
                    program: None,
                    agent: None,
                    conversation: None,
                },
            }],
        );

        // Single-process terminal matched by window PID; nearest ancestor wins.
        let nested = from_ancestors(&[999, 300, 200], &snapshot, Some(&live)).unwrap();
        assert_eq!(nested.space, "s2");
        assert_eq!(nested.tab_key, Some(3));
        assert_eq!(nested.window.as_deref(), Some("0x2"));
        assert_eq!(nested.source, Source::Window);

        // Multi-window server matched precisely via shell PID.
        let shared = from_ancestors(&[999, 450, 400], &snapshot, Some(&live)).unwrap();
        assert_eq!(shared.space, "s4");
        assert_eq!(shared.tab_key, Some(2));
        assert_eq!(shared.window.as_deref(), Some("0x4"));

        // Multi-window server without a matching shell PID doesn't guess.
        assert_eq!(from_ancestors(&[999, 400], &snapshot, Some(&live)), None);
        assert_eq!(from_ancestors(&[999, 888], &snapshot, Some(&live)), None);
    }

    #[test]
    fn validates_env_against_live_spaces() {
        let mut snapshot = Snapshot::default();
        snapshot
            .space_windows
            .push(space_window("0x1", 200, 1, "s1", 1));
        snapshot.add_space_for_test("s1");

        let env_s1 = from_env(Some("s1"), &snapshot).unwrap();
        assert_eq!(env_s1.space, "s1");
        assert_eq!(env_s1.source, Source::Env);
        assert_eq!(env_s1.tab_key, None);

        let env_home = from_env(Some("home"), &snapshot).unwrap();
        assert_eq!(env_home.space, "home");

        // Dead or bogus space IDs are ignored.
        assert_eq!(from_env(Some("s99"), &snapshot), None);
        assert_eq!(from_env(Some("not-a-space"), &snapshot), None);
        assert_eq!(from_env(Some(""), &snapshot), None);
        assert_eq!(from_env(None, &snapshot), None);
    }
}
