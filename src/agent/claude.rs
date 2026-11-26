//! Claude Code. It notes what it's doing, and the conversation it's in, for
//! each of its processes, in `~/.claude/sessions/<pid>.json`, so this needs no
//! hooks.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use super::{Adapter, Conversation, Noted, Process, is_uuid};
use crate::paths;

pub const ADAPTER: Adapter = Adapter {
    name: "claude",
    noted: |process| {
        // Omarchy runs it with a folder of its own for each account beyond
        // the first, so ours doesn't tell.
        let dir = process
            .var("CLAUDE_CONFIG_DIR")
            .map_or_else(|| paths::home().join(".claude"), PathBuf::from);
        read(&dir, process)
    },
    // One you never said anything in isn't kept, and can't be resumed: that
    // fails, and a new one takes its place. Leaving one isn't a failure.
    resume: |id| is_uuid(id).then(|| format!("claude --resume {id} || claude")),
};

/// What it notes about one of its processes.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Note {
    pid: u32,
    session_id: String,
    cwd: PathBuf,
    /// When the process started, as the kernel counts it.
    #[serde(default)]
    proc_start: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    status: Option<String>,
    /// What it's waiting for, when it is.
    #[serde(default)]
    waiting_for: Option<String>,
}

impl Note {
    /// Its status, in our words.
    fn word(&self) -> &'static str {
        match (self.status.as_deref(), self.waiting_for.as_deref()) {
            // You opened it yourself, as with `/config`.
            (Some("waiting"), Some("dialog open")) => "",
            (Some("waiting"), _) => "waiting",
            (Some("busy"), _) => "working",
            // `shell` is a turn that has ended with a command left running.
            (Some("idle" | "shell"), _) => "idle",
            _ => "",
        }
    }
}

fn read(dir: &Path, process: &Process) -> Option<Noted> {
    let note = read_note(&dir.join(format!("sessions/{}.json", process.pid)))?;
    // Left behind by another process that had the same pid.
    if note.pid != process.pid
        || note
            .proc_start
            .as_deref()
            .is_some_and(|start| start != process.started)
    {
        return None;
    }
    if note
        .kind
        .as_deref()
        .is_some_and(|kind| kind != "interactive")
    {
        return None;
    }
    let word = note.word();
    Some(Noted {
        conversation: is_uuid(&note.session_id).then(|| Conversation {
            agent: ADAPTER.name.into(),
            id: note.session_id,
            folder: note.cwd,
        }),
        word,
    })
}

/// It writes it in place, so it can be caught half written: then again, a
/// moment later.
fn read_note(path: &Path) -> Option<Note> {
    for _ in 0..3 {
        let text = std::fs::read_to_string(path).ok()?;
        if let Ok(note) = serde_json::from_str(&text) {
            return Some(note);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "be6fb713-2beb-4233-86c2-266ee407ec56";

    fn process(pid: u32, started: &str) -> Process {
        Process {
            pid,
            started: started.into(),
            env: Vec::new(),
        }
    }

    #[test]
    fn makes_a_command_only_from_an_id() {
        assert_eq!(
            (ADAPTER.resume)(ID),
            Some(format!("claude --resume {ID} || claude"))
        );
        assert_eq!((ADAPTER.resume)("x; rm -rf ~"), None);
    }

    #[test]
    fn reads_what_it_notes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sessions")).unwrap();
        let write = |text: String| {
            std::fs::write(dir.path().join("sessions/7.json"), text).unwrap();
        };
        let note = |kind: &str| {
            format!(
                r#"{{"pid":7,"sessionId":"{ID}","cwd":"/p","procStart":"42","kind":"{kind}","new":1}}"#
            )
        };
        let read = |pid, started| read(dir.path(), &process(pid, started));
        write(note("interactive"));
        assert_eq!(
            read(7, "42"),
            Some(Noted {
                conversation: Some(Conversation {
                    agent: "claude".into(),
                    id: ID.into(),
                    folder: "/p".into(),
                }),
                word: "",
            })
        );
        assert_eq!(read(7, "43"), None, "another process's");
        assert_eq!(read(8, "42"), None);
        write(note("background"));
        assert_eq!(read(7, "42"), None);
        write("{".into());
        assert_eq!(read(7, "42"), None);
    }

    #[test]
    fn looks_where_it_was_told_to_note() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sessions")).unwrap();
        std::fs::write(
            dir.path().join("sessions/7.json"),
            format!(r#"{{"pid":7,"sessionId":"{ID}","cwd":"/p","status":"busy"}}"#),
        )
        .unwrap();
        let mut process = process(7, "42");
        assert_eq!((ADAPTER.noted)(&process), None);
        process.env = format!("A=b\0CLAUDE_CONFIG_DIR={}\0", dir.path().display()).into_bytes();
        assert_eq!((ADAPTER.noted)(&process).unwrap().word, "working");
    }

    #[test]
    fn reads_what_it_is_doing() {
        let word = |status: &str, waiting_for: &str| {
            let note = format!(
                r#"{{"pid":7,"sessionId":"{ID}","cwd":"/p","status":"{status}","waitingFor":"{waiting_for}"}}"#
            );
            serde_json::from_str::<Note>(&note).unwrap().word()
        };
        assert_eq!(word("busy", ""), "working");
        assert_eq!(word("waiting", "permission prompt"), "waiting");
        assert_eq!(word("waiting", "input needed"), "waiting");
        assert_eq!(word("waiting", "dialog open"), "", "you opened it");
        assert_eq!(word("idle", ""), "idle");
        assert_eq!(word("shell", ""), "idle");
        assert_eq!(word("dreaming", ""), "");
    }
}
