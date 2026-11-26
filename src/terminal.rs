//! Terminals: the folder a terminal window's shell is in, which names its
//! space, what it runs, and opening a new terminal in a folder. A window's
//! shell is found in `/proc`.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::agent::{self, Conversation};
use crate::folder::Folder;
use crate::hypr;

/// What the shell of a terminal window is doing: the folder it's in, and
/// the program it's running, unless it's at its prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    /// Its process, to tell it from others of the same terminal app's.
    pub pid: u32,
    pub folder: Folder,
    /// The command running in the terminal's foreground (`claude`, `nvim`),
    /// or `None` at the prompt.
    pub program: Option<String>,
    /// What an agent in this terminal is doing, when one says.
    pub agent: Option<Agent>,
    /// The conversation the agent it's running is in, to pick up again.
    pub conversation: Option<Conversation>,
}

/// What a terminal's agent is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    /// Its process, to tell it from others (`agent::Process`).
    pub process: String,
    /// working, waiting, idle or done; empty when it doesn't say.
    pub word: String,
    /// Goes up each time it stops working, so a `done` seen can be told
    /// from the next.
    pub turn: u64,
}

impl Agent {
    /// As it notes it. Whether it has finished a turn is for `status::Turns`
    /// to tell.
    pub fn new(process: &str, word: &str) -> Self {
        Self {
            process: process.to_string(),
            word: word.to_string(),
            turn: 0,
        }
    }
}

impl Shell {
    /// What a tab shows for this terminal: what it's running, or else the
    /// folder it's in.
    pub fn label(&self) -> String {
        self.program.clone().unwrap_or_else(|| self.folder.name())
    }
}

/// Whose a window is: the process that runs it, and which of that process's
/// windows it is, oldest first. Most terminals are a process each, but
/// Ghostty, `foot --server` and `kitty -1` run all their windows in one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub pid: u32,
    pub nth: usize,
    /// How many windows the process has.
    pub of: usize,
}

/// Every window's place, by address.
pub fn places(clients: &[hypr::Client]) -> HashMap<String, Place> {
    let mut by_pid: HashMap<u32, Vec<&hypr::Client>> = HashMap::new();
    for client in clients {
        if let Ok(pid) = u32::try_from(client.pid) {
            by_pid.entry(pid).or_default().push(client);
        }
    }
    let mut places = HashMap::new();
    for (pid, mut windows) in by_pid {
        windows.sort_by_key(|window| window.opened());
        let of = windows.len();
        for (nth, window) in windows.into_iter().enumerate() {
            places.insert(window.address.clone(), Place { pid, nth, of });
        }
    }
    places
}

/// The folder a terminal window's shell is in, or `None` when the window isn't
/// a terminal.
pub fn shell_folder(place: Place) -> Option<Folder> {
    shell(place, None).map(|shell| shell.folder)
}

/// Which of a terminal app's shells, oldest first, is in a window. Nothing
/// says, but both were opened in the same order, so when there are as many of
/// each, the window's place tells. Otherwise the app has several in a window
/// (tabs or splits), or a window or a shell is only just coming or going: then
/// it's still the one it was `before`, else the newest.
fn in_place(shells: &[u32], place: Place, before: Option<u32>) -> Option<u32> {
    if place.of > 1 {
        if shells.len() == place.of {
            return shells.get(place.nth).copied();
        }
        if let Some(before) = before.filter(|before| shells.contains(before)) {
            return Some(before);
        }
    }
    shells.last().copied()
}

/// A window's shell, or `None` when the window isn't a terminal. `before` is
/// its process, when it was last read.
pub fn shell(place: Place, before: Option<u32>) -> Option<Shell> {
    let shell = in_place(&shells(place.pid), place, before)?;
    if let Some(fake) = fakes() {
        let cwd = std::fs::read_link(fake.join(shell.to_string())).ok()?;
        let program = std::fs::read_to_string(fake.join(format!("{shell}.program")))
            .ok()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        return Some(Shell {
            pid: shell,
            folder: Folder::here(cwd),
            program,
            agent: std::fs::read_to_string(fake.join(format!("{shell}.agent")))
                .ok()
                .map(|word| Agent::new(&shell.to_string(), word.trim())),
            conversation: std::fs::read_to_string(fake.join(format!("{shell}.conversation")))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok()),
        });
    }
    let stat = Stat::read(shell)?;
    let known = std::fs::read_to_string("/etc/shells").unwrap_or_default();
    let cwd = std::fs::canonicalize(format!("/proc/{shell}/cwd")).ok()?;
    let program = if is_shell_binary(shell, &known) {
        foreground(&stat, &known)
    } else {
        // Direct-exec agent terminal (`omarchy agent`, `foot -e claude`, etc.):
        // the session leader itself is the running agent.
        Some(shell)
    };
    let noted = program.and_then(agent::noted);
    Some(Shell {
        pid: shell,
        folder: Folder::here(cwd),
        program: program.and_then(program_name),
        agent: noted
            .as_ref()
            .map(|(process, noted)| Agent::new(&process.key(), noted.word)),
        conversation: noted.and_then(|(_, noted)| noted.conversation),
    })
}

/// For tests, `SPACES_FAKE_CWDS` names a folder that stands in for `/proc`: a
/// symlink per shell, pointing at the folder it's in, and `<shell>.program`,
/// `<shell>.conversation` and `<shell>.agent` (what its agent is doing, as
/// `agent::Noted` has it) files holding what it runs. A terminal's shell
/// is called as its window's process, or, when it has several,
/// `<pid>.shells` holds what they are, oldest first.
fn fakes() -> Option<std::path::PathBuf> {
    std::env::var_os("SPACES_FAKE_CWDS").map(Into::into)
}

/// The shells (or directly launched agents) the terminal app `pid` runs, oldest
/// first. As `omarchy-cmd-terminal-cwd` decides: a terminal's shell is a child
/// that leads its own session on a tty and is a login shell (listed in
/// `/etc/shells`). Terminals launched directly into a known agent (such as
/// `omarchy agent` running `foot -e claude`) also qualify via `agent::noted`.
/// Other apps' children, such as kitty's `kitten` helpers or a browser's, don't.
fn shells(pid: u32) -> Vec<u32> {
    if let Some(fake) = fakes() {
        let shells = std::fs::read_to_string(fake.join(format!("{pid}.shells")));
        return match shells {
            Ok(text) => text
                .split_whitespace()
                .filter_map(|shell| shell.parse().ok())
                .collect(),
            Err(_) => vec![pid],
        };
    }
    let known = std::fs::read_to_string("/etc/shells").unwrap_or_default();
    let mut shells: Vec<(u64, u32)> = std::fs::read_dir(format!("/proc/{pid}/task"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|task| std::fs::read_to_string(task.path().join("children")).ok())
        .flat_map(|list| {
            list.split_whitespace()
                .filter_map(|c| c.parse().ok())
                .collect::<Vec<u32>>()
        })
        .filter_map(|child| {
            let stat = Stat::read(child)?;
            is_shell(child, &stat, &known).then_some((stat.started, child))
        })
        .collect();
    shells.sort_unstable();
    shells.into_iter().map(|(_, shell)| shell).collect()
}

/// The fields of `/proc/<pid>/stat` that say how a process sits on its tty.
struct Stat {
    pgrp: i64,
    session: i64,
    tty: i64,
    /// The process group in the foreground of its tty.
    tpgid: i64,
    /// When it started, in ticks since boot.
    started: u64,
}

impl Stat {
    fn read(pid: u32) -> Option<Self> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // The command name, in parentheses, can hold spaces; the fields after
        // it are state, ppid, pgrp, session, tty_nr, tpgid.
        let (_, rest) = stat.rsplit_once(") ")?;
        let started = rest.split_whitespace().nth(19)?.parse().ok()?;
        let fields: Vec<i64> = rest
            .split_whitespace()
            .skip(1)
            .take(5)
            .map(|f| f.parse().ok())
            .collect::<Option<_>>()?;
        let &[_, pgrp, session, tty, tpgid] = fields.as_slice() else {
            return None;
        };
        Some(Self {
            pgrp,
            session,
            tty,
            tpgid,
            started,
        })
    }
}

fn is_shell(pid: u32, stat: &Stat, shells: &str) -> bool {
    stat.session == i64::from(pid)
        && stat.tty != 0
        && (is_shell_binary(pid, shells) || agent::noted(pid).is_some())
}

/// Whether `pid` runs one of the shells `/etc/shells` lists.
fn is_shell_binary(pid: u32, shells: &str) -> bool {
    let Ok(exe) = std::fs::canonicalize(format!("/proc/{pid}/exe")) else {
        return false;
    };
    shells.lines().any(|line| {
        let line = line.trim();
        !line.starts_with('#')
            && !line.is_empty()
            && std::fs::canonicalize(line).is_ok_and(|shell| shell == exe)
    })
}

/// What runs in the foreground of the shell's tty, when it isn't the shell
/// at its prompt: the leader of the foreground process group, which is the
/// command you ran, or the first of a pipeline. Another shell started from
/// this one (`bash`, `nix develop`) counts as a prompt too.
fn foreground(shell: &Stat, shells: &str) -> Option<u32> {
    if shell.tpgid <= 0 || shell.tpgid == shell.pgrp {
        return None;
    }
    let leader = u32::try_from(shell.tpgid).ok()?;
    (!is_shell_binary(leader, shells)).then_some(leader)
}

/// What to call the program `pid` runs: its command name, as `ps` shows it,
/// read in full from its arguments when the kernel cut it short, and up to
/// any `: ` (tmux calls its client `tmux: client`).
fn program_name(pid: u32) -> Option<String> {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let args: Vec<String> = cmdline
        .split(|&b| b == 0)
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect();
    display_name(comm.trim_end_matches('\n'), args.iter().map(String::as_str))
}

/// What to call a program from its command name (`comm`) and arguments.
fn display_name<'a>(comm: &str, args: impl Iterator<Item = &'a str>) -> Option<String> {
    // The kernel keeps 15 bytes of it.
    let full = if comm.len() >= 15 {
        args.map(|arg| arg.rsplit('/').next().unwrap_or_default())
            .find(|base| base.starts_with(comm))
            .map(String::from)
    } else {
        None
    };
    let name = full.as_deref().unwrap_or(comm);
    let name = name
        .split_once(": ")
        .map_or(name, |(before, _)| before)
        .trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The terminal `command` as one shell command that runs it in `dir` with
/// `SPACES_DIR` and `env` set, for Hyprland to run (`launch` sets them
/// itself).
pub fn command_in(command: &str, dir: &Path, env: &[(&str, &str)]) -> String {
    let dir = shell_quote(&dir.to_string_lossy());
    let mut set = format!("SPACES_DIR={dir}");
    for (key, value) in env {
        set.push_str(&format!(" {key}={}", shell_quote(value)));
    }
    format!("cd -- {dir} && export {set} && {command}")
}

/// Whether the window run by `pid` was started with `key` set to `value`.
pub fn was_given(pid: u32, key: &str, value: &str) -> bool {
    // Tests' windows have no processes.
    if fakes().is_some() {
        return true;
    }
    let given = format!("{key}={value}");
    std::fs::read(format!("/proc/{pid}/environ"))
        .is_ok_and(|env| env.split(|&b| b == 0).any(|var| var == given.as_bytes()))
}

/// `value` in single quotes for `sh`.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Opens a terminal in `dir` with `command` (`launch.rs`), run by `sh -c` in
/// that folder with `SPACES_DIR` set to it. It runs on its own, outliving
/// `space`.
pub fn launch(command: &str, dir: &Path) -> Result<()> {
    Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .env("SPACES_DIR", dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("can't open a terminal with `{command}`"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_the_terminal_command_in_a_folder() {
        assert_eq!(
            command_in("exec kitty", Path::new("/p/it's here"), &[("A", "b c")]),
            r#"cd -- '/p/it'\''s here' && export SPACES_DIR='/p/it'\''s here' A='b c' && exec kitty"#
        );
        let tmp = tempfile::tempdir().unwrap();
        let odd = tmp.path().join("a 'b' $c");
        std::fs::create_dir_all(&odd).unwrap();
        let out = Command::new("sh")
            .arg("-c")
            .arg(command_in("pwd; echo \"$SPACES_DIR\"", &odd, &[]))
            .output()
            .unwrap();
        let odd = odd.display().to_string();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!("{odd}\n{odd}\n")
        );
    }

    #[test]
    fn finds_no_shell_for_a_non_terminal() {
        // This test process's children aren't session-leading shells.
        for pid in [std::process::id(), u32::MAX] {
            let place = Place { pid, nth: 0, of: 1 };
            assert_eq!(shell(place, None), None);
        }
    }

    #[test]
    fn tells_which_shell_is_in_a_window_of_several() {
        let of = |nth, of| Place { pid: 1, nth, of };
        assert_eq!(in_place(&[10, 20], of(0, 1), None), Some(20), "the newest");
        assert_eq!(in_place(&[10, 20], of(0, 1), Some(10)), Some(20));
        assert_eq!(in_place(&[10, 20], of(0, 2), None), Some(10));
        assert_eq!(in_place(&[10, 20], of(1, 2), Some(10)), Some(20));
        // A tab more than there are windows, or a window yet without one.
        assert_eq!(in_place(&[10, 20, 30], of(0, 2), Some(10)), Some(10));
        assert_eq!(in_place(&[10, 20], of(0, 3), Some(10)), Some(10));
        assert_eq!(in_place(&[10, 20, 30], of(0, 2), Some(5)), Some(30), "gone");
        assert_eq!(in_place(&[10, 20, 30], of(0, 2), None), Some(30));
        assert_eq!(in_place(&[], of(0, 2), Some(10)), None);
    }

    #[test]
    fn names_programs_by_their_command() {
        assert_eq!(
            program_name(std::process::id())
                .as_deref()
                .map(str::is_empty),
            Some(false)
        );
        let mut child = Command::new("sleep").arg("5").spawn().unwrap();
        // `spawn` can return just before the kernel gives it its new name.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut name = program_name(child.id());
        while name.as_deref() != Some("sleep") && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
            name = program_name(child.id());
        }
        assert_eq!(name.as_deref(), Some("sleep"));
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(program_name(u32::MAX), None);
    }

    #[test]
    fn labels_a_terminal_by_its_program_else_its_folder() {
        let at = |cwd: &Path, program: Option<&str>| Shell {
            pid: 1,
            folder: Folder::here(cwd),
            program: program.map(String::from),
            agent: None,
            conversation: None,
        };
        assert_eq!(at(Path::new("/p/ren"), Some("claude")).label(), "claude");
        assert_eq!(at(Path::new("/p/ren/src"), None).label(), "src");
        assert_eq!(at(&crate::paths::home(), None).label(), "~");
    }
}
