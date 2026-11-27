//! The spaces that were up, saved so `space restore` can open them again
//! after a restart. `space watch` writes `session.json`; unlike
//! `spaces.json`, it isn't pruned the moment Hyprland drops a space.
//!
//! Logging out closes every window, which looks just like closing every
//! space. So what's new is saved at once (`SETTLE`), but what's gone only
//! once it has been gone for a while (`KEPT`): by then a session that was
//! ending has ended. A session saved under another Hyprland is moved aside to
//! `session-previous.json` before this one's is saved, and stays there until
//! it's restored.
//!
//! Earlier ones are kept in `sessions/` for `space restore <file>`, should
//! one have been saved, or restored, wrong.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, TryLockError};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::agent::Conversation;
use crate::browser::Browser;
use crate::folder::Folder;
use crate::layout::{Rect, Tree};
use crate::state;

/// The version of session files (`session.json`, `session-previous.json`,
/// `sessions/*.json`). Goes up when it changes in a way that an older `space`
/// would read wrong; a new field that it can do without isn't one.
const VERSION: u32 = 1;

/// How long the spaces must stay as they are before they're saved.
const SETTLE: Duration = Duration::from_secs(1);
/// How long something must have been gone before it's saved as gone.
const KEPT: Duration = Duration::from_secs(10);
/// Earlier sessions are kept at most this often, and this many of them.
const BACKUP_EVERY: Duration = Duration::from_secs(15 * 60);
const BACKUPS: usize = 48;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// The Hyprland it was saved under (`HYPRLAND_INSTANCE_SIGNATURE`).
    pub instance: String,
    /// `home` first, then in the sidebar's order.
    pub spaces: Vec<Space>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Space {
    /// Whether it's `home`. Its tabs, as every space's, are known by their
    /// keys, what their workspaces are numbered: every monitor has its own tab
    /// 3 (3 and 13).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub home: bool,
    /// Its id, which it gets back. `None` for `home`, and in sessions from
    /// before ids were kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, serde_json::Value>,
    /// Whether it's the space you were in.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub active: bool,
    /// The tab you were on, or left it on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<u8>,
    /// The names you gave its tabs, by their keys.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tabs: BTreeMap<u8, String>,
    /// How its tabs' windows are arranged, by their keys.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub layouts: BTreeMap<u8, Layout>,
    /// Where new tabs open, for a space without windows.
    #[serde(flatten)]
    pub folder: Option<Folder>,
    /// Its windows, oldest first, then its browser windows.
    pub windows: Vec<Window>,
}

/// How a tab's windows are arranged. Windows are places in the space's
/// `windows`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiles: Option<Tree<usize>>,
    /// The window that had focus last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused: Option<usize>,
    /// The monitor it's on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor: Option<String>,
}

/// A saved window: where it sat, and what kind of window it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    /// The tab it's on, by its key.
    pub tab: u8,
    /// What kind of window it is, and what it held.
    #[serde(flatten)]
    pub kind: WindowKind,
    /// Its window, under the Hyprland it was saved under.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub address: String,
    /// Where it floats, from its monitor's corner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floating: Option<Rect>,
    /// Hyprland's number for how it's fullscreen: 1 maximized, 2 fullscreen.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fullscreen: u8,
}

/// What kind of window was saved: `"terminal": { ... }` or `"browser": { ... }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowKind {
    Terminal(Terminal),
    Browser(Browser),
}

/// A terminal window's folder and any agent conversation in it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Terminal {
    #[serde(flatten)]
    pub folder: Option<Folder>,
    /// The agent's conversation in it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<Conversation>,
}

impl Window {
    pub fn terminal(&self) -> Option<&Terminal> {
        match &self.kind {
            WindowKind::Terminal(terminal) => Some(terminal),
            WindowKind::Browser(_) => None,
        }
    }

    pub fn browser(&self) -> Option<&Browser> {
        match &self.kind {
            WindowKind::Browser(browser) => Some(browser),
            WindowKind::Terminal(_) => None,
        }
    }
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

impl Session {
    /// What was in this window, and its space.
    pub fn find(&self, address: &str) -> Option<(&Space, &Window)> {
        self.spaces.iter().find_map(|space| {
            let window = space.windows.iter().find(|t| t.address == address)?;
            Some((space, window))
        })
    }

    /// What can go away: its windows, and the conversations in them.
    fn parts(&self) -> HashSet<String> {
        self.spaces
            .iter()
            .flat_map(|space| &space.windows)
            .flat_map(|w| {
                let talk = w
                    .terminal()
                    .and_then(|t| t.conversation.as_ref())
                    .map(|c| format!("{} {}", w.address, c.agent));
                [Some(w.address.clone()), talk]
            })
            .flatten()
            .collect()
    }
}

/// `left` before `saved`, with one `home` between them.
fn merge(left: Session, mut saved: Session) -> Session {
    let mut spaces = left.spaces;
    for space in &mut spaces {
        space.active = false;
    }
    if let Some(i) = spaces.iter().position(|s| s.home)
        && let Some(home) = saved.spaces.iter_mut().find(|s| s.home)
    {
        let left = spaces.remove(i);
        let offset = home.windows.len();
        home.windows.extend(left.windows);
        for (tab, mut layout) in left.layouts {
            layout.tiles = layout
                .tiles
                .and_then(|t| t.filter_map(&|window| Some(window + offset)));
            layout.focused = layout.focused.map(|window| window + offset);
            home.layouts.entry(tab).or_insert(layout);
        }
        for (tab, name) in left.tabs {
            home.tabs.entry(tab).or_insert(name);
        }
    }
    saved.spaces.splice(0..0, spaces);
    // `home` first: it's where you are as the rest opens.
    saved.spaces.sort_by_key(|s| !s.home);
    saved
}

fn current_path(state_dir: &Path) -> PathBuf {
    state_dir.join("session.json")
}

fn previous_path(state_dir: &Path) -> PathBuf {
    state_dir.join("session-previous.json")
}

/// Reads a session. One that can't be understood is put aside rather than
/// standing in the way of every later one.
fn read(path: &Path) -> Result<Option<Session>> {
    let text = state::read_optional(path)?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    match state::parse(&text, VERSION) {
        Ok(session) => Ok(Some(session)),
        Err(e) => {
            state::put_aside(path, &e)?;
            Ok(None)
        }
    }
}

/// Replaces a session in one step, on the disk by the time it returns: the
/// power may be about to go.
fn write(path: &Path, session: &Session) -> Result<()> {
    #[derive(Serialize)]
    struct Saved<'a> {
        version: u32,
        #[serde(flatten)]
        session: &'a Session,
    }
    let saved = Saved {
        version: VERSION,
        session,
    };
    let text = serde_json::to_string_pretty(&saved)? + "\n";
    let dir = path.parent().expect("sessions live in a directory");
    fs::create_dir_all(dir).with_context(|| format!("can't create {}", dir.display()))?;
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    // It has the pages you had open: nobody else's business.
    File::options()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut file| {
            file.write_all(text.as_bytes())?;
            file.sync_all()
        })
        .with_context(|| format!("can't write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("can't write {}", path.display()))?;
    // Saved either way; this only makes the rename last too.
    let _ = File::open(dir).and_then(|dir| dir.sync_all());
    Ok(())
}

fn backup_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("sessions")
}

/// The earlier sessions kept, oldest first.
fn backups(state_dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(backup_dir(state_dir))
        .into_iter()
        .flatten()
        .flatten()
        .map(|file| file.path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .collect();
    found.sort();
    found
}

/// Keeps a session in case it's wanted later, if it has anything in it, and
/// forgets the oldest ones.
fn back_up(state_dir: &Path, session: &Session) -> Result<()> {
    if session.spaces.is_empty() {
        return Ok(());
    }
    let path = backup_dir(state_dir).join(format!("{}.json", state::now()));
    write(&path, session)?;
    let kept = backups(state_dir);
    for old in &kept[..kept.len().saturating_sub(BACKUPS)] {
        let _ = fs::remove_file(old);
    }
    Ok(())
}

/// Whether it's time to keep another.
fn backup_due(state_dir: &Path) -> bool {
    backups(state_dir)
        .last()
        .and_then(|path| fs::metadata(path).ok()?.modified().ok())
        .is_none_or(|at| {
            SystemTime::now()
                .duration_since(at)
                .is_ok_and(|age| age >= BACKUP_EVERY)
        })
}

/// Puts a kept session in line to be restored.
pub fn set_previous_from(state_dir: &Path, file: &Path) -> Result<()> {
    let text =
        fs::read_to_string(file).with_context(|| format!("can't read {}", file.display()))?;
    let wanted: Session = state::parse(&text, VERSION)
        .with_context(|| format!("can't understand {}", file.display()))?;
    let _lock = state::lock(state_dir, ".session.lock")?;
    let all = match previous(state_dir)? {
        Some(left) => merge(left, wanted),
        None => wanted,
    };
    set_previous(state_dir, &all)
}

/// Held while restoring, until it's dropped. `None` when another `space
/// restore` holds it.
pub fn lock_restore(state_dir: &Path) -> Result<Option<File>> {
    let file = state::lock_file(state_dir, ".restore.lock")?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e).context("can't lock the restore"),
    }
}

/// Moves a session saved under another Hyprland aside, for `restore`, after
/// any spaces an earlier restore didn't get to. `watch` and `restore` both
/// do this as the plugin starts, so one waits for the other.
pub fn set_aside(state_dir: &Path, instance: &str) -> Result<()> {
    let _lock = state::lock(state_dir, ".session.lock")?;
    let path = current_path(state_dir);
    if let Some(mut saved) = read(&path)?
        && saved.instance != instance
        && !saved.spaces.is_empty()
    {
        if let Err(e) = back_up(state_dir, &saved) {
            eprintln!("space: warning: {e:#}");
        }
        if let Some(left) = previous(state_dir)? {
            saved = merge(left, saved);
        }
        write(&previous_path(state_dir), &saved)?;
        std::fs::remove_file(&path).with_context(|| format!("can't remove {}", path.display()))?;
    }
    Ok(())
}

/// The session to restore, if there is one.
pub fn previous(state_dir: &Path) -> Result<Option<Session>> {
    Ok(read(&previous_path(state_dir))?.filter(|s| !s.spaces.is_empty()))
}

/// Saves what is left to restore, so a restore that stops halfway can go on
/// without opening the same spaces twice.
pub fn set_previous(state_dir: &Path, session: &Session) -> Result<()> {
    let path = previous_path(state_dir);
    if !session.spaces.is_empty() {
        return write(&path, session);
    }
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != ErrorKind::NotFound => {
            Err(e).with_context(|| format!("can't remove {}", path.display()))
        }
        _ => Ok(()),
    }
}

fn restored_path(state_dir: &Path) -> PathBuf {
    state_dir.join("session-restored.json")
}

/// Notes a space just restored, with its windows' new addresses, for `watch`
/// to know how a tab it has only ever seen fullscreen is arranged.
pub fn add_restored(state_dir: &Path, instance: &str, space: Space) -> Result<()> {
    let mut session = restored(state_dir, instance)?.unwrap_or_else(|| Session {
        instance: instance.to_string(),
        spaces: Vec::new(),
    });
    session.spaces.push(space);
    write(&restored_path(state_dir), &session)
}

fn restored(state_dir: &Path, instance: &str) -> Result<Option<Session>> {
    Ok(read(&restored_path(state_dir))?.filter(|s| s.instance == instance))
}

/// Saves the session as it changes, once each change has settled.
pub struct Saver {
    state_dir: PathBuf,
    instance: String,
    path: PathBuf,
    /// What was restored under this Hyprland, as of when.
    restored: Option<Session>,
    restored_at: Option<SystemTime>,
    saved: Option<Session>,
    pending: Option<(Session, Instant)>,
    /// What is saved but gone now, and since when.
    gone: HashMap<String, Instant>,
    /// Whether the computer is on its way down.
    ending: fn() -> bool,
}

impl Saver {
    pub fn new(state_dir: &Path, instance: &str) -> Result<Self> {
        set_aside(state_dir, instance)?;
        let path = current_path(state_dir);
        Ok(Self {
            state_dir: state_dir.to_path_buf(),
            instance: instance.to_string(),
            restored: None,
            restored_at: None,
            saved: read(&path)?,
            path,
            pending: None,
            gone: HashMap::new(),
            ending: shutting_down,
        })
    }

    /// The sessions there were before, latest first: the one last seen, and
    /// what was restored.
    pub fn before(&mut self) -> Vec<&Session> {
        let path = restored_path(&self.state_dir);
        let at = fs::metadata(&path).and_then(|m| m.modified()).ok();
        if at != self.restored_at {
            self.restored_at = at;
            self.restored = restored(&self.state_dir, &self.instance).unwrap_or_default();
        }
        let pending = self.pending.as_ref().map(|(session, _)| session);
        [pending, self.saved.as_ref(), self.restored.as_ref()]
            .into_iter()
            .flatten()
            .collect()
    }

    pub fn update(&mut self, session: Session, now: Instant) -> Result<()> {
        if self.saved.as_ref() == Some(&session) {
            self.pending = None;
            self.gone.clear();
            return Ok(());
        }
        let there = session.parts();
        let before = self.saved.as_ref().map(Session::parts).unwrap_or_default();
        self.gone
            .retain(|part, _| before.contains(part) && !there.contains(part));
        for part in before.into_iter().filter(|part| !there.contains(part)) {
            self.gone.entry(part).or_insert(now);
        }
        match &self.pending {
            Some((pending, _)) if *pending == session => {}
            _ => self.pending = Some((session, now)),
        }
        let Some((session, since)) = &self.pending else {
            return Ok(());
        };
        if now.duration_since(*since) < SETTLE
            || self.gone.values().any(|at| now.duration_since(*at) < KEPT)
        {
            return Ok(());
        }
        if !self.gone.is_empty() {
            // A shutdown held up by some app can outlast any wait.
            if (self.ending)() {
                self.gone.values_mut().for_each(|at| *at = now);
                return Ok(());
            }
            if let Some(saved) = &self.saved
                && backup_due(&self.state_dir)
                && let Err(e) = back_up(&self.state_dir, saved)
            {
                eprintln!("space: warning: {e:#}");
            }
        }
        write(&self.path, session)?;
        self.saved = self.pending.take().map(|(session, _)| session);
        self.gone.clear();
        Ok(())
    }
}

/// Whether logind is shutting the computer down. No when it can't say.
fn shutting_down() -> bool {
    Command::new("busctl")
        .args([
            "--system",
            "--timeout=1",
            "get-property",
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "PreparingForShutdown",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|out| out.stdout.starts_with(b"b true"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(instance: &str, folders: &[&str]) -> Session {
        Session {
            instance: instance.into(),
            spaces: folders
                .iter()
                .map(|folder| Space {
                    windows: vec![Window {
                        tab: 1,
                        kind: WindowKind::Terminal(Terminal {
                            folder: Some(Folder::here(folder)),
                            conversation: None,
                        }),
                        address: format!("0x{folder}"),
                        floating: None,
                        fullscreen: 0,
                    }],
                    ..Default::default()
                })
                .collect(),
        }
    }

    fn saver(dir: &Path) -> Saver {
        let mut saver = Saver::new(dir, "a").unwrap();
        saver.ending = || false;
        saver
    }

    #[test]
    fn saves_whats_new_at_once_and_whats_gone_later() {
        let dir = tempfile::tempdir().unwrap();
        let path = current_path(dir.path());
        let start = Instant::now();
        let at = |secs| start + Duration::from_secs(secs);
        let mut saver = saver(dir.path());
        let one = session("a", &["/x"]);
        let both = session("a", &["/x", "/y"]);
        saver.update(one.clone(), at(0)).unwrap();
        assert_eq!(read(&path).unwrap(), None);
        saver.update(one.clone(), at(1)).unwrap();
        assert_eq!(read(&path).unwrap(), Some(one));
        saver.update(both.clone(), at(2)).unwrap();
        saver.update(both.clone(), at(3)).unwrap();
        assert_eq!(read(&path).unwrap(), Some(both.clone()));

        // Logging out: they go one by one, then the watch does.
        saver.update(session("a", &["/y"]), at(10)).unwrap();
        saver.update(session("a", &[]), at(15)).unwrap();
        saver.update(session("a", &[]), at(24)).unwrap();
        assert_eq!(read(&path).unwrap(), Some(both.clone()));

        // Closed on purpose, and still closed.
        saver.update(session("a", &[]), at(25)).unwrap();
        assert_eq!(read(&path).unwrap(), Some(session("a", &[])));
        let kept = backups(dir.path());
        assert_eq!(kept.len(), 1);
        assert_eq!(read(&kept[0]).unwrap(), Some(both));
    }

    #[test]
    fn what_keeps_changing_after_a_close_is_saved_in_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = current_path(dir.path());
        let start = Instant::now();
        let at = |secs| start + Duration::from_secs(secs);
        let mut saver = saver(dir.path());
        let both = session("a", &["/x", "/y"]);
        saver.update(both.clone(), at(0)).unwrap();
        saver.update(both, at(1)).unwrap();
        // /y closes, while /x goes from folder to folder.
        for secs in 2..=13 {
            let mut now = session("a", &["/x"]);
            now.spaces[0].windows[0].kind = WindowKind::Terminal(Terminal {
                folder: Some(Folder::here(format!("/x/{}", secs / 2))),
                conversation: None,
            });
            saver.update(now, at(secs)).unwrap();
        }
        let saved = read(&path).unwrap().unwrap();
        assert_eq!(saved.spaces.len(), 1);
    }

    #[test]
    fn a_conversation_that_ends_is_kept_for_a_while() {
        let dir = tempfile::tempdir().unwrap();
        let path = current_path(dir.path());
        let start = Instant::now();
        let at = |secs| start + Duration::from_secs(secs);
        let mut saver = saver(dir.path());
        let mut talking = session("a", &["/x"]);
        talking.spaces[0].windows[0].kind = WindowKind::Terminal(Terminal {
            folder: Some(Folder::here("/x")),
            conversation: Some(Conversation {
                agent: "claude".into(),
                id: "id".into(),
                folder: "/x".into(),
            }),
        });
        saver.update(talking.clone(), at(0)).unwrap();
        saver.update(talking.clone(), at(1)).unwrap();
        let quit = session("a", &["/x"]);
        saver.update(quit.clone(), at(2)).unwrap();
        saver.update(quit.clone(), at(11)).unwrap();
        assert_eq!(read(&path).unwrap(), Some(talking));
        saver.update(quit.clone(), at(12)).unwrap();
        assert_eq!(read(&path).unwrap(), Some(quit));
    }

    #[test]
    fn nothing_goes_while_the_computer_shuts_down() {
        let dir = tempfile::tempdir().unwrap();
        let path = current_path(dir.path());
        let start = Instant::now();
        let at = |secs| start + Duration::from_secs(secs);
        let mut saver = saver(dir.path());
        saver.ending = || true;
        let one = session("a", &["/x"]);
        saver.update(one.clone(), at(0)).unwrap();
        saver.update(one.clone(), at(1)).unwrap();
        for secs in 2..90 {
            saver.update(session("a", &[]), at(secs)).unwrap();
        }
        assert_eq!(read(&path).unwrap(), Some(one));
    }

    #[test]
    fn a_broken_session_is_put_aside() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(current_path(dir.path()), "{ half").unwrap();
        let mut saver = saver(dir.path());
        let one = session("a", &["/x"]);
        saver.update(one.clone(), Instant::now()).unwrap();
        let later = Instant::now() + SETTLE;
        saver.update(one.clone(), later).unwrap();
        assert_eq!(read(&current_path(dir.path())).unwrap(), Some(one));
        let kept = fs::read_dir(dir.path()).unwrap().flatten();
        let broken = kept
            .map(|f| f.file_name().to_string_lossy().into_owned())
            .find(|name| name.starts_with("session.broken-"));
        assert_eq!(
            fs::read_to_string(dir.path().join(broken.unwrap())).unwrap(),
            "{ half"
        );
    }

    #[test]
    fn serializes_windows_by_kind() {
        let dir = tempfile::tempdir().unwrap();
        let path = current_path(dir.path());
        let saved = r#"{"version":1,"instance":"a","spaces":[{"windows":[
            {"tab":1,"terminal":{"folder":"/x"},"address":"0x/x"}]}]}"#;
        fs::write(&path, saved).unwrap();
        assert_eq!(read(&path).unwrap(), Some(session("a", &["/x"])));
    }

    #[test]
    fn says_its_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = current_path(dir.path());
        write(&path, &session("a", &["/x"])).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (&saved["version"], &saved["instance"]),
            (&1.into(), &"a".into())
        );
        assert_eq!(read(&path).unwrap(), Some(session("a", &["/x"])));

        // One from a newer `space` is left as it is to restore.
        let newer = dir.path().join("newer.json");
        fs::write(&newer, r#"{"version":99,"instance":"a","spaces":[]}"#).unwrap();
        assert!(set_previous_from(dir.path(), &newer).is_err());
        assert!(newer.exists());
    }

    #[test]
    fn keeps_only_so_many_earlier_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let kept = backup_dir(dir.path());
        fs::create_dir_all(&kept).unwrap();
        for n in 0..BACKUPS + 3 {
            fs::write(kept.join(format!("{}.json", 1000 + n)), "{}").unwrap();
        }
        back_up(dir.path(), &session("a", &["/x"])).unwrap();
        assert_eq!(backups(dir.path()).len(), BACKUPS);
        assert!(!kept.join("1000.json").exists());
        assert!(!backup_due(dir.path()));
        back_up(dir.path(), &session("a", &[])).unwrap();
        assert_eq!(backups(dir.path()).len(), BACKUPS, "nothing in it");
    }

    #[test]
    fn restores_one_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let held = lock_restore(dir.path()).unwrap();
        assert!(held.is_some());
        assert!(lock_restore(dir.path()).unwrap().is_none());
        drop(held);
        assert!(lock_restore(dir.path()).unwrap().is_some());
    }

    #[test]
    fn sets_another_hyprlands_session_aside_until_restored() {
        let dir = tempfile::tempdir().unwrap();
        let path = current_path(dir.path());
        write(&path, &session("a", &["/x"])).unwrap();
        set_aside(dir.path(), "a").unwrap();
        assert_eq!(previous(dir.path()).unwrap(), None);
        set_aside(dir.path(), "b").unwrap();
        assert_eq!(previous(dir.path()).unwrap(), Some(session("a", &["/x"])));

        // A session that never had a space doesn't take its place.
        write(&path, &session("b", &[])).unwrap();
        set_aside(dir.path(), "c").unwrap();
        assert_eq!(previous(dir.path()).unwrap().unwrap().instance, "a");

        // One that ended before everything was restored keeps the rest.
        write(&path, &session("c", &["/y"])).unwrap();
        set_aside(dir.path(), "d").unwrap();
        assert_eq!(
            previous(dir.path()).unwrap(),
            Some(session("c", &["/x", "/y"]))
        );

        set_previous(dir.path(), &session("a", &[])).unwrap();
        assert_eq!(previous(dir.path()).unwrap(), None);
    }

    #[test]
    fn one_home_is_made_of_two() {
        let home = |folder: &str| {
            let mut session = session("a", &[folder]);
            session.spaces[0].home = true;
            session.spaces[0].active = true;
            session.spaces[0].layouts.insert(
                if folder == "/x" { 1 } else { 2 },
                Layout {
                    tiles: Some(Tree::Tile(vec![0])),
                    ..Default::default()
                },
            );
            session
        };
        let mut saved = home("/y");
        saved
            .spaces
            .insert(0, session("a", &["/z"]).spaces.remove(0));
        let all = merge(home("/x"), saved);
        assert_eq!(all.spaces.len(), 2);
        assert!(all.spaces[0].home && all.spaces[0].active);
        let folders: Vec<_> = all.spaces[0]
            .windows
            .iter()
            .map(|w| w.terminal().and_then(|t| t.folder.clone()))
            .collect();
        assert_eq!(folders, ["/y", "/x"].map(|dir| Some(Folder::here(dir))));
        assert_eq!(
            all.spaces[0].layouts[&1].tiles,
            Some(Tree::Tile(vec![1])),
            "its window moved along"
        );
        assert_eq!(all.spaces[0].layouts[&2].tiles, Some(Tree::Tile(vec![0])));
    }
}
