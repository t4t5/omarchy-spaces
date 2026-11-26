//! Agents: what they're doing, and the conversation they're in, to pick up
//! again after a restart. How each says so is its adapter's alone to know: a
//! file here, named in `ADAPTERS`. An agent without one is a program like any
//! other.

mod agy;
mod claude;

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const ADAPTERS: &[Adapter] = &[claude::ADAPTER, agy::ADAPTER];

fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

struct Adapter {
    /// What conversations are saved under.
    name: &'static str,
    /// What a process notes about itself, if it's this agent's.
    noted: fn(&Process) -> Option<Noted>,
    /// The command that picks up the conversation with this id, or `None` if
    /// it can't be one.
    resume: fn(&str) -> Option<String>,
}

/// A conversation to go on with. What runs is made from its id, which is
/// checked, so a saved session can't hold a command of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    /// Which agent: its adapter's name, as `claude`.
    pub agent: String,
    pub id: String,
    /// The folder it was started in, where it has to be picked up.
    pub folder: PathBuf,
}

impl Conversation {
    /// The command that picks it up, or `None` for one that can't be.
    pub fn command(&self) -> Option<String> {
        resume_command(&self.agent, &self.id)
    }
}

/// `claude <id>` as `SPACES_RESUME` hands it to `space shell`.
pub fn parse_resume(value: &str) -> Option<String> {
    let (agent, id) = value.split_once(' ')?;
    resume_command(agent, id)
}

fn resume_command(agent: &str, id: &str) -> Option<String> {
    let adapter = ADAPTERS.iter().find(|adapter| adapter.name == agent)?;
    (adapter.resume)(id)
}

/// What an agent's process notes about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Noted {
    pub conversation: Option<Conversation>,
    /// What it's doing: working, waiting or idle; empty when it doesn't say.
    pub word: &'static str,
}

/// A process that may be an agent's.
pub struct Process {
    pid: u32,
    /// When it started, as `/proc/<pid>/stat` counts it.
    started: String,
    /// Its environment, as `/proc/<pid>/environ` has it.
    env: Vec<u8>,
}

impl Process {
    fn read(pid: u32) -> Option<Self> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (_, rest) = stat.rsplit_once(") ")?;
        Some(Self {
            pid,
            started: rest.split_whitespace().nth(19)?.to_string(),
            env: std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default(),
        })
    }

    /// Which process it is, to tell it from another that gets its pid.
    pub fn key(&self) -> String {
        format!("{}/{}", self.pid, self.started)
    }

    /// A variable of its environment, which needn't be ours, unless empty.
    fn var(&self, name: &str) -> Option<&OsStr> {
        self.env
            .split(|&b| b == 0)
            .find_map(|var| var.strip_prefix(name.as_bytes())?.strip_prefix(b"="))
            .filter(|value| !value.is_empty())
            .map(OsStr::from_bytes)
    }
}

/// What the process `pid` notes about itself, if it's an agent we can tell.
pub fn noted(pid: u32) -> Option<(Process, Noted)> {
    let process = Process::read(pid)?;
    let noted = ADAPTERS.iter().find_map(|a| (a.noted)(&process))?;
    Some((process, noted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_up_only_what_an_adapter_knows() {
        let id = "be6fb713-2beb-4233-86c2-266ee407ec56";
        assert_eq!(
            parse_resume(&format!("claude {id}")),
            Some(format!("claude --resume {id} || claude"))
        );
        assert_eq!(
            parse_resume(&format!("agy {id}")),
            Some(format!("agy -r {id} || agy"))
        );
        assert_eq!(parse_resume("claude x; rm -rf ~"), None);
        assert_eq!(parse_resume("agy x; rm -rf ~"), None);
        assert_eq!(parse_resume(&format!("sh {id}")), None);
        assert_eq!(parse_resume(id), None);
    }

    #[test]
    fn reads_a_process() {
        let process = Process::read(std::process::id()).unwrap();
        assert!(process.started.parse::<u64>().unwrap() > 0);
        assert!(Process::read(u32::MAX).is_none());

        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .env("SPACES_TEST", "its own")
            .spawn()
            .unwrap();
        // `spawn` can return just before the kernel gives it its environment.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let var = || {
            let process = Process::read(child.id())?;
            process.var("SPACES_TEST").map(OsStr::to_os_string)
        };
        let mut found = var();
        while found.is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
            found = var();
        }
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(found.unwrap(), "its own");
    }

    #[test]
    fn reads_variables() {
        let process = Process {
            pid: 1,
            started: "2".into(),
            env: b"MY_DIR=/x\0DIR=/h\0EMPTY=\0".to_vec(),
        };
        assert_eq!(process.var("DIR"), Some(OsStr::new("/h")));
        assert_eq!(process.var("EMPTY"), None);
        assert_eq!(process.var("D"), None);
        assert_eq!(process.key(), "1/2");
    }
}
