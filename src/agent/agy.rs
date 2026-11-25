//! Antigravity CLI (`agy`). While a session is open it holds open
//! `<app_data_dir>/presence/<uuid>.lock` (and `conversations/<uuid>.db`) and
//! records its run status in `<app_data_dir>/conversation_summaries.db`, so
//! this needs no hooks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use super::{Adapter, Conversation, Noted, Process, is_uuid};

pub const ADAPTER: Adapter = Adapter {
    name: "agy",
    noted,
    resume: |id| is_uuid(id).then(|| format!("agy -r {id} || agy")),
};

type FileStamp = Option<(SystemTime, u64)>;
type DbStamp = (FileStamp, FileStamp);
type StatusCache = HashMap<(PathBuf, String), (DbStamp, &'static str)>;

fn cache() -> &'static Mutex<StatusCache> {
    static CACHE: OnceLock<Mutex<StatusCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn noted(process: &Process) -> Option<Noted> {
    if !is_agy(process.pid) {
        return None;
    }
    let (app_data_dir, id) = session_from_fds(process.pid)?;
    let word = read_status(&app_data_dir, &id);
    let conversation = std::fs::canonicalize(format!("/proc/{}/cwd", process.pid))
        .ok()
        .map(|folder| Conversation {
            agent: ADAPTER.name.into(),
            id,
            folder,
        });
    Some(Noted { conversation, word })
}

/// Whether `pid` is running `agy` / `antigravity`, to skip `/proc/<pid>/fd`
/// scans for unrelated foreground programs.
fn is_agy(pid: u32) -> bool {
    if let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        && matches!(comm.trim(), "agy" | "antigravity")
    {
        return true;
    }
    std::fs::canonicalize(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|exe| {
            let name = exe.file_name()?.to_str()?;
            Some(matches!(name, "agy" | "antigravity"))
        })
        .unwrap_or(false)
}

fn session_from_fds(pid: u32) -> Option<(PathBuf, String)> {
    let entries = std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
    let targets = entries
        .flatten()
        .filter_map(|entry| std::fs::read_link(entry.path()).ok());
    pick_session(targets)
}

/// Picks the active conversation from a process's open file paths, preferring
/// `presence/<uuid>.lock` over `conversations/<uuid>.db`.
fn pick_session(targets: impl IntoIterator<Item = PathBuf>) -> Option<(PathBuf, String)> {
    let mut fallback = None;
    for path in targets {
        if let Some((dir, id, is_presence)) = parse_fd_target(&path) {
            if is_presence {
                return Some((dir, id));
            }
            if fallback.is_none() {
                fallback = Some((dir, id));
            }
        }
    }
    fallback
}

/// Extracts `(app_data_dir, uuid, is_presence)` from an open fd target path.
fn parse_fd_target(path: &Path) -> Option<(PathBuf, String, bool)> {
    let file = path.file_name()?.to_str()?;
    let parent = path.parent()?;
    let kind = parent.file_name()?.to_str()?;
    let app_data_dir = parent.parent()?;
    let (id, is_presence) = match kind {
        "presence" => (file.strip_suffix(".lock")?, true),
        "conversations" => (file.strip_suffix(".db")?, false),
        _ => return None,
    };
    is_uuid(id).then(|| (app_data_dir.to_path_buf(), id.to_string(), is_presence))
}

/// Maps `conversation_summaries.status` into Spaces' status words.
fn parse_status(raw: &str) -> &'static str {
    match raw.trim() {
        "CASCADE_RUN_STATUS_RUNNING"
        | "CASCADE_RUN_STATUS_BUSY"
        | "CASCADE_RUN_STATUS_CANCELING" => "working",
        "CASCADE_RUN_STATUS_IDLE" => "idle",
        _ => "",
    }
}

fn file_stamp(path: &Path) -> FileStamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

fn read_status(app_data_dir: &Path, id: &str) -> &'static str {
    if !is_uuid(id) {
        return "";
    }
    let db = app_data_dir.join("conversation_summaries.db");
    let db_stamp = file_stamp(&db);
    if db_stamp.is_none() {
        return "";
    }
    let wal = app_data_dir.join("conversation_summaries.db-wal");
    let stamp = (db_stamp, file_stamp(&wal));
    let key = (db.clone(), id.to_string());
    if let Ok(guard) = cache().lock()
        && let Some((cached_stamp, word)) = guard.get(&key)
        && *cached_stamp == stamp
    {
        return word;
    }

    let sql = format!(
        "SELECT status FROM conversation_summaries WHERE conversation_id = '{id}' LIMIT 1;"
    );
    let word = Command::new("sqlite3")
        .args(["-readonly", "-list", "-noheader", "-cmd", ".timeout 50"])
        .arg(&db)
        .arg(&sql)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| parse_status(&String::from_utf8_lossy(&out.stdout)))
        .unwrap_or("");

    if let Ok(mut guard) = cache().lock() {
        if guard.len() >= 64 {
            guard.clear();
        }
        guard.insert(key, (stamp, word));
    }
    word
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "f99a4cbf-ed47-43bb-9bd2-7e6a14de1b05";
    const OTHER: &str = "121ac7e9-7e43-4596-8021-96e4cb897f64";

    #[test]
    fn makes_a_command_only_from_an_id() {
        assert_eq!((ADAPTER.resume)(ID), Some(format!("agy -r {ID} || agy")));
        assert_eq!((ADAPTER.resume)("x; rm -rf ~"), None);
    }

    #[test]
    fn finds_session_from_open_files() {
        let root = PathBuf::from("/home/u/.gemini/antigravity-cli");
        let files = vec![
            PathBuf::from("/dev/pts/2"),
            root.join(format!("conversations/{OTHER}.db-wal")),
            root.join(format!("conversations/{OTHER}.db")),
            root.join(format!("presence/{ID}.lock")),
        ];
        assert_eq!(
            pick_session(files),
            Some((root.clone(), ID.into())),
            "prefers presence lock over conversation db"
        );
        assert_eq!(
            pick_session([root.join(format!("conversations/{OTHER}.db"))]),
            Some((root.clone(), OTHER.into())),
            "falls back to conversation db"
        );
        assert_eq!(
            pick_session([
                root.join("presence/not-a-uuid.lock"),
                root.join("other/f99a4cbf-ed47-43bb-9bd2-7e6a14de1b05.lock"),
            ]),
            None
        );
    }

    #[test]
    fn reads_what_it_is_doing() {
        assert_eq!(parse_status("CASCADE_RUN_STATUS_RUNNING\n"), "working");
        assert_eq!(parse_status("CASCADE_RUN_STATUS_BUSY"), "working");
        assert_eq!(parse_status("CASCADE_RUN_STATUS_CANCELING"), "working");
        assert_eq!(parse_status("CASCADE_RUN_STATUS_IDLE"), "idle");
        assert_eq!(parse_status("CASCADE_RUN_STATUS_UNSPECIFIED"), "");
        assert_eq!(parse_status(""), "");
    }

    #[test]
    fn reads_status_from_summaries_db() {
        if Command::new("sqlite3").arg("--version").output().is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("conversation_summaries.db");
        let run = |sql: &str| {
            let status = Command::new("sqlite3").arg(&db).arg(sql).status().unwrap();
            assert!(status.success());
        };
        run("CREATE TABLE conversation_summaries (conversation_id TEXT PRIMARY KEY, status TEXT);");
        run(&format!(
            "INSERT INTO conversation_summaries VALUES ('{ID}', 'CASCADE_RUN_STATUS_RUNNING');"
        ));
        assert_eq!(read_status(dir.path(), ID), "working");
        assert_eq!(read_status(dir.path(), OTHER), "");

        // Wait briefly so mtime changes on coarse filesystems, then update status.
        std::thread::sleep(std::time::Duration::from_millis(20));
        run(&format!(
            "UPDATE conversation_summaries SET status = 'CASCADE_RUN_STATUS_IDLE' WHERE conversation_id = '{ID}';"
        ));
        assert_eq!(read_status(dir.path(), ID), "idle");
    }
}
