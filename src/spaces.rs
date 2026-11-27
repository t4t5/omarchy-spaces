//! Spaces: what Spaces remembers about each one, in
//! `~/.local/state/omarchy-spaces/spaces.json`.
//!
//! A space is a set of Hyprland workspaces, `<id>:1` to `<id>:10`, and lives
//! as long as they do: when its last window closes and you leave it,
//! Hyprland drops its workspaces and the space is gone. Hyprland is the
//! source of truth for which spaces exist and their windows; this file only
//! adds what Hyprland can't hold. Entries for spaces Hyprland no longer has
//! are pruned whenever the live list is read.
//!
//! An id is a space's for good: no other is ever given it, and the space gets
//! it back when it's restored (`restore.rs`). So what's still kept by an id,
//! here, in `last` or in a script, is never taken for another space's.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::folder::Folder;
use crate::name;
use crate::state;

/// The version of `spaces.json`. Goes up when it changes in a way that an
/// older `space` would read wrong; a new field that it can do without isn't
/// one.
const VERSION: u32 = 1;

/// What Spaces remembers about one space.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The name you gave it. Without one, it is called after its folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The icon you picked for its badge: a glyph, an emoji, or an image file
    /// path, in place of the letters of its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// A short one-line note or status summary for this space (for example, a
    /// checkpoint written by you or an agent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Opaque metadata owned by external scripts, plugins, or agents. Persisted
    /// across restarts and left untouched by Spaces.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, serde_json::Value>,
    /// The folder of the terminal it follows, last time it was seen; where
    /// it started before that. New terminals in it open here.
    #[serde(flatten)]
    pub folder: Option<Folder>,
    /// The address of the terminal window it follows: its first one, kept
    /// while that window lives, so the name doesn't change with focus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// Its place in the sidebar, lowest first; 0 for none yet. A new space's
    /// is when it was made, in Unix milliseconds, so it comes last and keeps
    /// its place as names change. Moving a space (`space reorder`) hands the
    /// spaces' places out again, in the new order.
    #[serde(default, alias = "created", skip_serializing_if = "is_zero")]
    pub order: u64,
    /// The names you gave its tabs, by what their workspaces are numbered, as
    /// every monitor has its own tab 3 (3 and 13). A tab without one is called
    /// after its most recently focused window. A name lasts as long as its
    /// tab: when Hyprland drops the workspace, the name goes with it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tabs: BTreeMap<u8, String>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    version: u32,
    /// The number of the next new space's id. 0 in files from before ids were
    /// kept from being used again.
    #[serde(default)]
    next: u32,
    #[serde(default)]
    spaces: BTreeMap<String, Entry>,
}

#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    pub spaces: BTreeMap<String, Entry>,
    next: u32,
    /// What was read, to write only when something changed.
    read: BTreeMap<String, Entry>,
    read_next: u32,
    /// Held to save it. A failure to lock only matters then.
    lock: Result<std::fs::File>,
}

impl Store {
    /// Reads it, to look at.
    pub fn load(state_dir: &Path) -> Result<Self> {
        Self::read(state_dir, Err(anyhow!("it was only read")))
    }

    /// Reads it, to change and save: other processes wait to do the same
    /// until it's dropped, so changes made at once are all kept.
    pub fn lock(state_dir: &Path) -> Result<Self> {
        Self::read(state_dir, state::lock(state_dir, ".spaces.lock"))
    }

    fn read(state_dir: &Path, mut lock: Result<std::fs::File>) -> Result<Self> {
        let path = state_dir.join("spaces.json");
        let text = state::read_optional(&path)?;
        let file = if text.trim().is_empty() {
            File::default()
        } else {
            state::parse(&text, VERSION).unwrap_or_else(|e| {
                // One that's going to be saved over is put aside first. Not
                // locked, it may be another by now, and stays.
                if lock.is_ok()
                    && let Err(e) = state::put_aside(&path, &e)
                {
                    lock = Err(e);
                }
                File::default()
            })
        };
        Ok(Self {
            path,
            read: file.spaces.clone(),
            spaces: file.spaces,
            next: file.next,
            read_next: file.next,
            lock,
        })
    }

    /// What the file holds, without the changes not saved yet.
    pub fn saved(&self) -> &BTreeMap<String, Entry> {
        &self.read
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.spaces.get(id)
    }

    pub fn entry(&mut self, id: &str) -> &mut Entry {
        self.spaces.entry(id.to_string()).or_default()
    }

    /// Forgets spaces Hyprland no longer has. `home` is always there.
    pub fn prune(&mut self, live: &BTreeSet<String>) {
        self.spaces
            .retain(|id, _| id == name::HOME || live.contains(id));
    }

    /// An id for a new space: `s1`, `s2`, ..., the next that never was one's.
    /// `taken` are those in use, which may not all have an entry.
    pub fn fresh_id(&mut self, taken: &BTreeSet<String>) -> String {
        let first = self.next.max(1);
        let (n, id) = (first..)
            .map(|n| (n, format!("s{n}")))
            .find(|(_, id)| !taken.contains(id) && !self.spaces.contains_key(id))
            .expect("some id is free");
        self.next = n.saturating_add(1);
        id
    }

    /// Gives a restored space its id back, unless it's in use.
    pub fn claim_id(&mut self, id: &str, taken: &BTreeSet<String>) -> bool {
        let Some(number) = name::id_number(id).filter(|_| !taken.contains(id)) else {
            return false;
        };
        self.next = self.next.max(number.saturating_add(1));
        true
    }

    pub fn save(&mut self) -> Result<()> {
        if self.spaces == self.read && self.next == self.read_next {
            return Ok(());
        }
        if let Err(e) = &self.lock {
            bail!("can't save {}: {e:#}", self.path.display());
        }
        let text = serde_json::to_string_pretty(&File {
            version: VERSION,
            next: self.next,
            spaces: self.spaces.clone(),
        })?;
        state::write_atomic(&self.path, &(text + "\n"))
            .with_context(|| format!("can't save {}", self.path.display()))?;
        self.read = self.spaces.clone();
        self.read_next = self.next;
        Ok(())
    }
}

/// A name you give a space: any text, trimmed, on one line. Empty clears it.
pub fn clean_name(raw: &str) -> Result<Option<String>> {
    let name = raw.trim();
    if name.chars().any(char::is_control) {
        bail!("a name has to fit on one line");
    }
    if name.chars().count() > 64 {
        bail!("'{name}' is longer than 64 characters");
    }
    Ok((!name.is_empty()).then(|| name.to_string()))
}

/// A short note on a space: any text, trimmed, on one line. Empty clears it.
pub fn clean_note(raw: &str) -> Result<Option<String>> {
    let note = raw.trim();
    if note.chars().any(char::is_control) {
        bail!("a note has to fit on one line");
    }
    if note.chars().count() > 256 {
        bail!("a note can be at most 256 characters");
    }
    Ok((!note.is_empty()).then(|| note.to_string()))
}

/// Maximum number of metadata keys stored on one space.
pub const MAX_META_KEYS: usize = 64;
/// Maximum serialized byte size of one metadata value.
pub const MAX_META_VALUE_BYTES: usize = 64 * 1024;

/// A metadata key: non-empty, no whitespace or control characters.
pub fn clean_meta_key(raw: &str) -> Result<String> {
    let key = raw.trim();
    if key.is_empty() {
        bail!("a metadata key can't be empty");
    }
    if key.chars().any(|c| c.is_control() || c.is_whitespace()) {
        bail!("metadata key '{key}' can't contain spaces or control characters");
    }
    if key.chars().count() > 128 {
        bail!("metadata key '{key}' is longer than 128 characters");
    }
    Ok(key.to_string())
}

/// Ensures a metadata value fits within the per-value size cap.
pub fn validate_meta_value(value: &serde_json::Value) -> Result<()> {
    let len = serde_json::to_string(value)?.len();
    if len > MAX_META_VALUE_BYTES {
        bail!("metadata value is larger than {MAX_META_VALUE_BYTES} bytes");
    }
    Ok(())
}

const IMAGE_EXTENSIONS: &[&str] = &["png", "svg", "jpg", "jpeg", "webp", "ico"];

fn is_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            IMAGE_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

fn looks_like_path(raw: &str) -> bool {
    raw.starts_with(['/', '~'])
        || raw.starts_with("./")
        || raw.starts_with("../")
        || raw.starts_with("file://")
        || is_image_extension(Path::new(raw))
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn clean_image_icon(raw: &str) -> Result<String> {
    let stripped = raw
        .strip_prefix("file://localhost")
        .or_else(|| raw.strip_prefix("file://"))
        .map(percent_decode);
    let expanded = crate::paths::expand(stripped.as_deref().unwrap_or(raw));
    let path = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .context("can't read the current folder")?
            .join(expanded)
    };
    let canonical = std::fs::canonicalize(&path)
        .with_context(|| format!("no image file {}", crate::paths::tilde(&path)))?;
    if !canonical.is_file() {
        bail!("{} is not a file", crate::paths::tilde(&canonical));
    }
    if !is_image_extension(&canonical) {
        bail!("an image icon must be a png, svg, jpg, jpeg, webp, or ico file");
    }
    canonical
        .to_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("image path is not valid UTF-8"))
}

/// An icon you give a space: a Nerd Font glyph, an emoji (which can take
/// several characters, as `👩‍💻` is three), or a path to an image file
/// (`png`, `svg`, `jpg`, `jpeg`, `webp`, `ico`). Empty clears it.
pub fn clean_icon(raw: &str) -> Result<Option<String>> {
    let icon = raw.trim();
    if icon.is_empty() {
        return Ok(None);
    }
    if icon.chars().any(char::is_control) {
        bail!("an icon has to fit on one line");
    }
    if looks_like_path(icon) {
        return clean_image_icon(icon).map(Some);
    }
    if icon.chars().any(char::is_whitespace) {
        bail!("an icon is one glyph or emoji (without spaces), or an image file");
    }
    if icon.chars().count() > 16 {
        bail!("'{icon}' is too long for an icon: use one glyph, emoji, or image file");
    }
    Ok(Some(icon.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_prunes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::lock(dir.path()).unwrap();
        assert!(store.spaces.is_empty());
        store.entry("s1").name = Some("Japanese".into());
        store.entry("s1").icon = Some("🗾".into());
        store.entry("s2").folder = Some(Folder::here("/p/mail"));
        store.entry("s2").tabs.insert(3, "server".into());
        store.entry("home").tabs.insert(13, "mail".into());
        store.save().unwrap();
        drop(store);

        let mut store = Store::load(dir.path()).unwrap();
        assert_eq!(store.get("s1").unwrap().name.as_deref(), Some("Japanese"));
        assert_eq!(store.get("s1").unwrap().icon.as_deref(), Some("🗾"));
        assert_eq!(
            store.get("s2").unwrap().folder,
            Some(Folder::here("/p/mail"))
        );
        assert_eq!(store.get("s2").unwrap().tabs[&3], "server");

        let live: BTreeSet<String> = ["s2".to_string(), "s4".to_string()].into();
        store.prune(&live);
        assert_eq!(store.spaces.keys().collect::<Vec<_>>(), ["home", "s2"]);
    }

    #[test]
    fn hands_out_each_id_once() {
        let dir = tempfile::tempdir().unwrap();
        let set = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect();
        let mut store = Store::lock(dir.path()).unwrap();
        assert_eq!(store.fresh_id(&set(&[])), "s1");
        assert_eq!(store.fresh_id(&set(&["s1", "s2"])), "s3", "s2 is in use");
        store.save().unwrap();
        drop(store);

        // Even once they're gone.
        let mut store = Store::lock(dir.path()).unwrap();
        assert_eq!(store.fresh_id(&set(&[])), "s4");
        // A restored one is its own again, unless it's in use.
        assert!(store.claim_id("s2", &set(&["s1"])));
        assert_eq!(store.fresh_id(&set(&[])), "s5");
        assert!(!store.claim_id("s2", &set(&["s2"])));
        assert!(!store.claim_id("home", &set(&[])));
        assert!(!store.claim_id("", &set(&[])));
        // From before the count was lost, or from another computer.
        assert!(store.claim_id("s20", &set(&[])));
        assert_eq!(store.fresh_id(&set(&[])), "s21");
    }

    #[test]
    fn a_file_it_cant_read_is_put_aside() {
        for text in ["{ nope", r#"{ "version": 99, "spaces": {} }"#] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("spaces.json");
            std::fs::write(&path, text).unwrap();
            assert!(Store::load(dir.path()).unwrap().spaces.is_empty());
            assert!(path.exists(), "only looked at");

            let mut store = Store::lock(dir.path()).unwrap();
            assert!(store.spaces.is_empty());
            store.entry("s1").name = Some("Mail".into());
            store.save().unwrap();
            let kept = std::fs::read_dir(dir.path()).unwrap().flatten();
            let aside = kept
                .map(|file| file.path())
                .find(|path| path.to_string_lossy().contains("spaces.broken-"));
            assert_eq!(std::fs::read_to_string(aside.unwrap()).unwrap(), text);
            let store = Store::load(dir.path()).unwrap();
            assert_eq!(store.get("s1").unwrap().name.as_deref(), Some("Mail"));
        }
    }

    #[test]
    fn says_its_version_and_reads_files_from_before() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spaces.json");
        std::fs::write(&path, r#"{ "spaces": { "s1": { "created": 5 } } }"#).unwrap();
        let mut store = Store::lock(dir.path()).unwrap();
        assert_eq!(store.get("s1").unwrap().order, 5);
        store.entry("s1").order = 6;
        store.save().unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved,
            serde_json::json!({ "version": 1, "next": 0, "spaces": { "s1": { "order": 6 } } })
        );
    }

    #[test]
    fn cleans_names() {
        assert_eq!(clean_name("  Japanese ").unwrap(), Some("Japanese".into()));
        assert_eq!(clean_name("  ").unwrap(), None);
        assert!(clean_name("a\nb").is_err());
        assert!(clean_name(&"x".repeat(65)).is_err());
        assert_eq!(clean_name("日本語").unwrap(), Some("日本語".into()));
    }

    #[test]
    fn cleans_icons() {
        assert_eq!(clean_icon(" 🚀 ").unwrap(), Some("🚀".into()));
        assert_eq!(clean_icon("👩‍💻").unwrap(), Some("👩‍💻".into()));
        assert_eq!(clean_icon("\u{f0415}").unwrap(), Some("\u{f0415}".into()));
        assert_eq!(clean_icon("").unwrap(), None);
        assert!(clean_icon("a b").is_err());
        assert!(clean_icon("a\nb").is_err());
        assert!(clean_icon(&"x".repeat(17)).is_err());

        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("my logo.PNG");
        std::fs::write(&png, b"\x89PNG").unwrap();
        let canonical = std::fs::canonicalize(&png).unwrap();
        let expected = canonical.to_string_lossy().into_owned();
        assert_eq!(
            clean_icon(&png.to_string_lossy()).unwrap(),
            Some(expected.clone())
        );
        let uri = format!("file://{}", png.to_string_lossy().replace(' ', "%20"));
        assert_eq!(clean_icon(&uri).unwrap(), Some(expected));

        let txt = dir.path().join("notes.txt");
        std::fs::write(&txt, "hi").unwrap();
        assert!(clean_icon(&txt.to_string_lossy()).is_err());
        assert!(clean_icon(&dir.path().join("missing.png").to_string_lossy()).is_err());
        let folder = dir.path().join("dir.png");
        std::fs::create_dir(&folder).unwrap();
        assert!(clean_icon(&folder.to_string_lossy()).is_err());
    }

    #[test]
    fn cleans_notes_and_metadata_and_round_trips() {
        assert_eq!(
            clean_note("  PR #412 ready  ").unwrap(),
            Some("PR #412 ready".into())
        );
        assert_eq!(clean_note("   ").unwrap(), None);
        assert!(clean_note("line1\nline2").is_err());
        assert!(clean_note(&"x".repeat(257)).is_err());

        assert_eq!(clean_meta_key(" linear.issue ").unwrap(), "linear.issue");
        assert!(clean_meta_key("").is_err());
        assert!(clean_meta_key("a b").is_err());
        assert!(clean_meta_key("a\nb").is_err());
        assert!(clean_meta_key(&"k".repeat(129)).is_err());

        assert!(validate_meta_value(&serde_json::json!({"ok": true})).is_ok());
        assert!(
            validate_meta_value(&serde_json::Value::String(
                "x".repeat(MAX_META_VALUE_BYTES + 1)
            ))
            .is_err()
        );

        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::lock(dir.path()).unwrap();
        let entry = store.entry("s1");
        entry.note = Some("waiting on CI".into());
        entry.meta.insert("pr".into(), serde_json::json!(412));
        store.save().unwrap();
        drop(store);

        let loaded = Store::load(dir.path()).unwrap();
        assert_eq!(
            loaded.get("s1").unwrap().note.as_deref(),
            Some("waiting on CI")
        );
        assert_eq!(loaded.get("s1").unwrap().meta["pr"], serde_json::json!(412));
    }
}
