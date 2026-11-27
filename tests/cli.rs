//! Runs the `space` binary against a fake Hyprland that speaks the same
//! socket protocol, with its own home, config and state directories.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

#[derive(Default)]
struct Hyprland {
    /// Name of the focused workspace.
    active: String,
    /// What the monitor you aren't on shows, with two plugged in: `eDP-1`,
    /// whose workspaces are numbered 1-10, and `DP-1`, whose are 11-20.
    elsewhere: Option<String>,
    /// Whether the second was unplugged, its workspaces left on the first.
    unplugged: bool,
    /// Workspaces as (id, name).
    workspaces: Vec<(i64, String)>,
    /// Windows, by the name of the workspace each is on.
    windows: Vec<String>,
    /// Each window's address, alongside `windows`.
    addresses: Vec<u32>,
    next_address: u32,
    /// Whether Omarchy's `o.workspaces` Lua module is loaded.
    omarchy: bool,
    /// Whether `hl.dsp.exec_cmd` takes rules, as Hyprland 0.56 does.
    exec_rules: bool,
    /// How long a terminal it runs takes to show its window.
    terminal_start: Duration,
    /// Windows still starting: when each shows up, where, and its address.
    starting: Vec<(Instant, String, u32)>,
    /// The fake home, which commands it runs see as `HOME`.
    home: PathBuf,
    /// Whoever listens to Hyprland's events, as `space watch` does.
    listeners: Arc<Mutex<Vec<UnixStream>>>,
    /// Every dispatch received.
    dispatched: Vec<String>,
    /// Every script received.
    evaluated: Vec<String>,
    /// The class of windows a rule has open somewhere, and where.
    browser_on: Option<(String, String)>,
    /// Windows run by another window's process, by that one's address.
    shared: std::collections::HashMap<u32, u32>,
    /// The classes of windows that aren't terminals.
    classes: std::collections::HashMap<u32, String>,
    /// Whether it says how windows are laid out, as 0.56 does.
    layouts: bool,
    /// Whether it answers with something unreadable.
    garbled: bool,
    /// How long going to a workspace takes it.
    slow: Duration,
    next_named_id: i64,
    /// The address of each workspace that was renamed, by its name now. A
    /// workspace made by `name:s1:2` keeps `s1:2` as its address when renamed
    /// (Hyprland 0.57), and a selector `name:` matches the address.
    addresses_of: std::collections::HashMap<String, String>,
}

/// What runs on the desktop the tests run on: none of it may, as it would open
/// real windows there. Omarchy's launchers run what they're given, and its
/// terminal notes the folder it was opened in, what it was to run and what to
/// pick up; what isn't expected to run at all notes that it did.
const STUBS: &[(&str, &str)] = &[
    ("setsid", "exec \"$@\""),
    ("uwsm-app", "shift; exec \"$@\""),
    (
        "xdg-terminal-exec",
        "pwd >> \"$HOME/launched\"\n\
         shift; echo \"$*\" >> \"$HOME/ran\"\n\
         echo \"$SPACES_RESUME\" >> \"$HOME/resumed\"",
    ),
    ("ghostty", "pwd >> \"$HOME/launched-for-real\""),
    ("kitty", "pwd >> \"$HOME/launched-for-real\""),
    ("chromium", "pwd >> \"$HOME/launched-for-real\""),
    ("chrome-stub", "pwd >> \"$HOME/launched-for-real\""),
];

/// Browsers: their desktop entry, which is their windows' class too, and the
/// program it runs.
const BROWSERS: &[(&str, &str)] = &[("chromium", "chromium"), ("google-chrome", "chrome-stub")];

/// A PATH on which those come first.
fn safe_path(home: &Path) -> String {
    let stubs = home.join("stubs");
    format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap())
}

fn write_stubs(home: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let stubs = home.join("stubs");
    fs::create_dir_all(&stubs).unwrap();
    let entries = home.join(".local/share/applications");
    fs::create_dir_all(&entries).unwrap();
    for (name, program) in BROWSERS {
        let entry = format!("[Desktop Entry]\nExec={program} %U\n");
        fs::write(entries.join(format!("{name}.desktop")), entry).unwrap();
    }
    for (name, script) in STUBS {
        let stub = stubs.join(name);
        fs::write(&stub, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// The monitor a workspace is on, by its number.
fn monitor_of(name: &str) -> &'static str {
    let number = name.rsplit(':').next().unwrap().parse::<u8>().unwrap_or(1);
    if number > 10 { "DP-1" } else { "eDP-1" }
}

impl Hyprland {
    /// Goes to a workspace, and to the monitor it's on.
    fn go(&mut self, name: String) {
        self.ensure(&name);
        if let Some(elsewhere) = &mut self.elsewhere
            && monitor_of(&name) != monitor_of(&self.active)
        {
            *elsewhere = self.active.clone();
        }
        self.active = name;
    }

    fn ensure(&mut self, name: &str) -> i64 {
        if let Some((id, _)) = self.workspaces.iter().find(|(_, n)| n == name) {
            return *id;
        }
        let id = match name.parse::<i64>() {
            Ok(id) => id,
            Err(_) => {
                self.next_named_id -= 1;
                self.next_named_id
            }
        };
        self.workspaces.push((id, name.to_string()));
        id
    }

    /// What `name:` matches: the workspace's address, the name it was made with.
    fn address(&self, name: &str) -> String {
        self.addresses_of
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    /// The name now of the workspace a `name:` selector means: the one with
    /// that address, or, with none, the name a new one is made with.
    fn resolve(&self, selector: String) -> String {
        let selector = selector.strip_prefix("name:").unwrap_or(&selector);
        self.workspaces
            .iter()
            .find(|(_, name)| self.address(name) == selector)
            .map_or_else(|| selector.to_string(), |(_, name)| name.clone())
    }

    fn workspace_json(&mut self, name: &str) -> Value {
        let id = self.ensure(name);
        let windows = self.windows.iter().filter(|w| *w == name).count();
        let layout = if self.layouts { "dwindle" } else { "" };
        let monitor = if self.unplugged {
            "eDP-1"
        } else {
            monitor_of(name)
        };
        let address = self.address(name);
        json!({ "id": id, "address": address, "name": name, "monitor": monitor, "windows": windows, "tiledLayout": layout })
    }

    /// Hyprland destroys empty workspaces that no monitor shows.
    fn tidy(&mut self) {
        let shown = [Some(self.active.clone()), self.elsewhere.clone()];
        let windows = self.windows.clone();
        self.workspaces
            .retain(|(_, n)| shown.contains(&Some(n.clone())) || windows.contains(n));
        let kept: Vec<String> = self.workspaces.iter().map(|(_, n)| n.clone()).collect();
        self.addresses_of.retain(|name, _| kept.contains(name));
    }

    /// Shows the windows that have started by now.
    fn show_started(&mut self) {
        let now = Instant::now();
        let (started, starting) = std::mem::take(&mut self.starting)
            .into_iter()
            .partition(|(at, ..)| *at <= now);
        self.starting = starting;
        let started: Vec<(Instant, String, u32)> = started;
        for (_, workspace, address) in started {
            self.ensure(&workspace);
            self.windows.push(workspace);
            self.addresses.push(address);
        }
    }

    fn handle(&mut self, request: &str) -> String {
        self.show_started();
        if let Some(batch) = request.strip_prefix("[[BATCH]]") {
            if self.garbled {
                return "unknown request".into();
            }
            let replies: Vec<String> = batch.split(';').map(|r| self.handle(r)).collect();
            return replies.join("\n\n\n");
        }
        match request {
            "j/activeworkspace" => {
                let active = self.active.clone();
                self.workspace_json(&active).to_string()
            }
            "j/workspaces" => {
                let names: Vec<String> = self.workspaces.iter().map(|(_, n)| n.clone()).collect();
                Value::Array(names.iter().map(|n| self.workspace_json(n)).collect()).to_string()
            }
            "j/monitors" => {
                let shown = [Some(self.active.clone()), self.elsewhere.clone()];
                let unplugged = self.unplugged;
                let monitors = shown.into_iter().flatten().map(|name| {
                    let id = self.ensure(&name);
                    let monitor = if unplugged { "eDP-1" } else { monitor_of(&name) };
                    let focused = name == self.active;
                    json!({ "name": monitor, "focused": focused, "refreshRate": 60.0, "x": 0, "y": 0, "width": 1920.0, "height": 1080.0, "scale": 1.0, "activeWorkspace": { "id": id, "name": name } })
                });
                Value::Array(monitors.collect()).to_string()
            }
            "j/clients" => {
                let windows = self.windows.clone();
                let addresses = self.addresses.clone();
                Value::Array(
                    windows
                        .iter()
                        .zip(addresses)
                        .map(|(ws, address)| {
                            let id = self.ensure(ws);
                            let class = self.classes.get(&address).cloned().unwrap_or_default();
                            json!({ "address": format!("0x{address:x}"), "class": class, "stableId": format!("{address:x}"), "pid": 1000 + self.shared.get(&address).unwrap_or(&address), "workspace": { "id": id, "name": ws } })
                        })
                        .collect(),
                )
                .to_string()
            }
            // The newest window on the focused workspace has focus.
            "j/activewindow" => match self.windows.iter().rposition(|w| *w == self.active) {
                Some(i) => {
                    let address = self.addresses[i];
                    json!({ "address": format!("0x{address:x}") }).to_string()
                }
                None => "{}".into(),
            },
            _ => match request.strip_prefix("dispatch ") {
                Some(expression) => {
                    self.dispatched.push(expression.to_string());
                    let reply = self.dispatch(expression);
                    self.tidy();
                    reply
                }
                None => match request.strip_prefix("eval ") {
                    Some(script) => {
                        if script.contains("omarchy-spaces-restore") {
                            let on = script.split("workspace = \"").nth(1).unwrap();
                            let on = on.split(" silent").next().unwrap();
                            let on = on.strip_prefix("name:").unwrap_or(on).to_string();
                            let class = script.split("class = \"").nth(1).unwrap();
                            let class = class.split('"').next().unwrap().to_string();
                            self.browser_on =
                                script.contains("enabled = true").then_some((class, on));
                        } else if script.contains("spaces.internal.show(") {
                            let name = script.split("spaces.internal.show(\"").nth(1).unwrap();
                            let name = name.split('"').next().unwrap();
                            assert!(self.workspaces.iter().any(|(_, n)| n == name));
                            self.elsewhere = Some(name.to_string());
                            self.tidy();
                        } else {
                            // Windows moved and workspaces renamed or gone
                            // to by a script, one to a line.
                            for line in script.lines() {
                                let moved = line.strip_prefix("hl.dispatch(").filter(|l| {
                                    l.starts_with("hl.dsp.window.move(")
                                        || l.starts_with("hl.dsp.workspace.rename(")
                                        || l.starts_with("hl.dsp.focus({ workspace")
                                });
                                if let Some(moved) = moved {
                                    self.dispatch(moved);
                                }
                            }
                            self.tidy();
                            self.evaluated.push(script.to_string());
                        }
                        "ok".into()
                    }
                    None => "unknown request".into(),
                },
            },
        }
    }

    fn dispatch(&mut self, expression: &str) -> String {
        let quoted = |e: &str| {
            let start = e.find("workspace = \"")? + "workspace = \"".len();
            let end = start + e[start..].find('"')?;
            Some(e[start..end].to_string())
        };
        let number = |e: &str| {
            let start = e.find('(')? + 1;
            e[start..]
                .split([',', ')'])
                .next()?
                .trim()
                .parse::<u8>()
                .ok()
        };

        let field = |e: &str, key: &str| {
            let start = e.find(&format!("{key} = \""))? + key.len() + 4;
            let end = start + e[start..].find('"')?;
            Some(e[start..end].to_string())
        };

        let browser = BROWSERS
            .iter()
            .find(|(_, program)| expression.contains(&format!("'{program}' '--profile")));
        if expression.starts_with("hl.dsp.exec_cmd(")
            && let Some((class, _)) = browser
        {
            self.next_address += 1;
            let address = self.next_address;
            self.classes.insert(address, class.to_string());
            let rule = self.browser_on.clone();
            let on = rule
                .filter(|(of, _)| *of == format!("^{class}$"))
                .map_or(self.active.clone(), |(_, on)| on);
            self.starting.push((Instant::now(), on.clone(), address));
            self.show_started();
            let listeners = Arc::clone(&self.listeners);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(50));
                for stream in listeners.lock().unwrap().iter_mut() {
                    let _ = writeln!(stream, "openwindow>>{address:x},{on},{class},New Tab");
                }
            });
            return "ok".into();
        }
        if expression.starts_with("hl.dsp.exec_cmd(") {
            if !self.exec_rules {
                return "error: exec_cmd: bad argument 2".into();
            }
            // Run it, as Hyprland does, and open its window where the rule
            // says, without going there.
            let start = "hl.dsp.exec_cmd(\"".len();
            let end = expression.find("\", { workspace").unwrap();
            let command = expression[start..end]
                .replace("\\\"", "\"")
                .replace("\\\\", "\\");
            let workspace = field(expression, "workspace").unwrap();
            let workspace = self.resolve(workspace.trim_end_matches(" silent").to_string());
            std::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .env_clear()
                .env("PATH", safe_path(&self.home))
                .env("HOME", &self.home)
                .status()
                .unwrap();
            self.next_address += 1;
            let address = self.next_address;
            let start = self.terminal_start;
            self.starting
                .push((Instant::now() + start, workspace.clone(), address));
            self.show_started();
            // Hyprland says so once the window maps, a moment later, as a
            // real terminal takes to start: by then whoever asked to be
            // told has been heard.
            let listeners = Arc::clone(&self.listeners);
            thread::spawn(move || {
                thread::sleep(start + Duration::from_millis(50));
                for stream in listeners.lock().unwrap().iter_mut() {
                    let _ = writeln!(stream, "openwindow>>{address:x},{workspace},kitty,zsh");
                }
            });
            return "ok".into();
        }
        if expression.starts_with("o.workspaces.") && !self.omarchy {
            return "error: attempt to index a nil value (global 'o')".into();
        }
        if expression.starts_with("hl.dsp.focus(") {
            if let Some(monitor) = field(expression, "monitor") {
                if monitor_of(&self.active) != monitor {
                    let there = self.elsewhere.clone().expect("a second monitor");
                    self.go(there);
                }
                return "ok".into();
            }
            let target = self.resolve(quoted(expression).unwrap());
            thread::sleep(self.slow);
            self.go(target);
        } else if expression.starts_with("hl.dsp.window.close(") {
            let address = field(expression, "window").unwrap();
            let address = address.strip_prefix("address:0x").unwrap();
            let address = u32::from_str_radix(address, 16).unwrap();
            let Some(index) = self.addresses.iter().position(|a| *a == address) else {
                return "error: no such window".into();
            };
            self.windows.remove(index);
            self.addresses.remove(index);
        } else if expression.starts_with("hl.dsp.workspace.rename(") {
            let from = self.resolve(quoted(expression).unwrap());
            let to = field(expression, "name").unwrap();
            let address = self.address(&from);
            let Some(renamed) = self.workspaces.iter_mut().find(|(_, n)| *n == from) else {
                return "warning: no such workspace".into();
            };
            renamed.1 = to.clone();
            self.addresses_of.remove(&from);
            if address != to {
                self.addresses_of.insert(to.clone(), address);
            }
            for window in self.windows.iter_mut().filter(|w| **w == from) {
                *window = to.clone();
            }
            if self.active == from {
                self.active = to;
            }
        } else if expression.starts_with("hl.dsp.window.move(") && expression.contains("address:") {
            let target = self.resolve(quoted(expression).unwrap());
            let address = field(expression, "window").unwrap();
            let address = address.strip_prefix("address:0x").unwrap();
            let address = u32::from_str_radix(address, 16).unwrap();
            let Some(index) = self.addresses.iter().position(|a| *a == address) else {
                return "error: no such window".into();
            };
            self.ensure(&target);
            self.windows[index] = target;
        } else if expression.starts_with("hl.dsp.window.set_prop(") {
            let address = field(expression, "window").unwrap();
            let address = address.strip_prefix("address:0x").unwrap();
            let address = u32::from_str_radix(address, 16).unwrap();
            if !self.addresses.contains(&address) {
                return "error: no such window".into();
            }
        } else if expression.starts_with("o.workspaces.focus(") {
            let block = if monitor_of(&self.active) == "DP-1" {
                10
            } else {
                0
            };
            self.go((number(expression).unwrap() + block).to_string());
        } else if expression.starts_with("hl.dsp.window.move(")
            || expression.starts_with("o.workspaces.move_window(")
        {
            let (target, follow) = if expression.starts_with("hl.") {
                (
                    self.resolve(quoted(expression).unwrap()),
                    expression.contains("follow = true"),
                )
            } else {
                (
                    number(expression).unwrap().to_string(),
                    !expression.contains("false"),
                )
            };
            let Some(index) = self.windows.iter().position(|w| *w == self.active) else {
                return "error: no window".into();
            };
            self.ensure(&target);
            self.windows[index] = target.clone();
            if follow {
                self.active = target;
            }
        } else {
            return format!("error: unknown dispatch {expression}");
        }
        "ok".into()
    }
}

struct Env {
    root: tempfile::TempDir,
    hypr: Arc<Mutex<Hyprland>>,
    /// Whoever listens to Hyprland's events, as `space watch` does.
    listeners: Arc<Mutex<Vec<UnixStream>>>,
}

impl Env {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let socket_dir = root.path().join("run/hypr/test");
        fs::create_dir_all(&socket_dir).unwrap();
        fs::create_dir_all(root.path().join("home/Projects")).unwrap();
        let listener = UnixListener::bind(socket_dir.join(".socket.sock")).unwrap();

        let listeners = Arc::new(Mutex::new(Vec::new()));
        let hypr = Arc::new(Mutex::new(Hyprland {
            active: "1".into(),
            omarchy: true,
            exec_rules: true,
            home: root.path().join("home"),
            listeners: Arc::clone(&listeners),
            ..Default::default()
        }));
        hypr.lock().unwrap().ensure("1");
        let served = Arc::clone(&hypr);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                // Like Hyprland, read one request without waiting for EOF.
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let reply = served.lock().unwrap().handle(&request);
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        let events = UnixListener::bind(socket_dir.join(".socket2.sock")).unwrap();
        let joined = Arc::clone(&listeners);
        thread::spawn(move || {
            for stream in events.incoming() {
                let Ok(stream) = stream else { break };
                joined.lock().unwrap().push(stream);
            }
        });
        fs::create_dir_all(root.path().join("cwds")).unwrap();
        let env = Self {
            root,
            hypr,
            listeners,
        };
        write_stubs(&env.home());
        env
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn state(&self) -> PathBuf {
        self.home().join(".local/state/omarchy-spaces")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_space"));
        command
            .args(args)
            .current_dir(self.home())
            .env_clear()
            .env("PATH", safe_path(&self.home()))
            .env("HOME", self.home())
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("HYPRLAND_INSTANCE_SIGNATURE", "test")
            .env("SPACES_FAKE_CWDS", self.cwds());
        command
    }

    /// Sends a Hyprland event (`openwindow>>...`) to whoever listens.
    fn event(&self, line: &str) {
        for stream in self.listeners.lock().unwrap().iter_mut() {
            let _ = writeln!(stream, "{line}");
        }
    }

    /// Hyprland goes away: its event socket closes.
    fn end_events(&self) {
        for stream in self.listeners.lock().unwrap().drain(..) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    /// Where each fake terminal's folder is: a symlink per pid.
    fn cwds(&self) -> PathBuf {
        self.root.path().join("cwds")
    }

    /// Opens a terminal window whose shell is in `dir`; returns its address.
    fn add_terminal(&self, workspace: &str, dir: &Path) -> String {
        let address = self.add_window(workspace);
        self.cd(&address, dir);
        address
    }

    /// The terminal at `address` changes folder.
    fn cd(&self, address: &str, dir: &Path) {
        let pid = 1000 + u32::from_str_radix(address.trim_start_matches("0x"), 16).unwrap();
        let link = self.cwds().join(pid.to_string());
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(dir, link).unwrap();
    }

    /// The terminal at `address` starts running `program`, or goes back to
    /// its prompt with `None`.
    fn run_program(&self, address: &str, program: Option<&str>) {
        let pid = 1000 + u32::from_str_radix(address.trim_start_matches("0x"), 16).unwrap();
        let file = self.cwds().join(format!("{pid}.program"));
        match program {
            Some(program) => fs::write(file, program).unwrap(),
            None => {
                let _ = fs::remove_file(file);
            }
        }
    }

    /// The agent in the terminal at `address` notes what it's doing.
    fn agent_is(&self, address: &str, word: &str) {
        let file = self.agent_file(address);
        // In one step, as a reader would see half of it otherwise.
        let tmp = file.with_extension("tmp");
        fs::write(&tmp, word).unwrap();
        fs::rename(tmp, file).unwrap();
    }

    fn agent_exits(&self, address: &str) {
        fs::remove_file(self.agent_file(address)).unwrap();
    }

    fn agent_file(&self, address: &str) -> PathBuf {
        let pid = 1000 + u32::from_str_radix(address.trim_start_matches("0x"), 16).unwrap();
        self.cwds().join(format!("{pid}.agent"))
    }

    /// The address of the newest window, such as the terminal `space new`
    /// opened.
    fn newest_window(&self) -> String {
        format!(
            "0x{:x}",
            self.hypr.lock().unwrap().addresses.last().unwrap()
        )
    }

    fn close_window(&self, address: &str) {
        let mut hypr = self.hypr.lock().unwrap();
        let address = u32::from_str_radix(address.trim_start_matches("0x"), 16).unwrap();
        let i = hypr.addresses.iter().position(|a| *a == address).unwrap();
        hypr.windows.remove(i);
        hypr.addresses.remove(i);
        hypr.tidy();
    }

    /// The folders the first `count` terminals were opened in, waiting a
    /// moment for them: the terminal command runs on its own.
    fn launched(&self, count: usize) -> Vec<String> {
        let file = self.home().join("launched");
        let mut lines = Vec::new();
        for _ in 0..100 {
            let text = fs::read_to_string(&file).unwrap_or_default();
            lines = text.lines().map(str::to_string).collect();
            if lines.len() >= count && text.ends_with('\n') {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(20));
        }
        lines
    }

    /// A folder under the fake home, made if needed.
    fn dir(&self, path: &str) -> PathBuf {
        let dir = self.home().join(path);
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(dir).unwrap()
    }

    fn ls_json(&self) -> Vec<Value> {
        let mut list: Value = serde_json::from_str(&self.ok(&["ls", "--json"])).unwrap();
        assert_eq!(list["version"], 1);
        match list["spaces"].take() {
            Value::Array(rows) => rows,
            other => panic!("{other}"),
        }
    }

    /// The names given to tabs, by their ids, of the space in row `n`.
    fn tab_names(&self, n: usize) -> Value {
        let rows = self.ls_json();
        let named = rows[n]["tabs"].as_array().unwrap().iter();
        named
            .filter(|tab| tab["named"] == true)
            .map(|tab| (tab["id"].as_str().unwrap().into(), tab["label"].clone()))
            .collect::<serde_json::Map<_, _>>()
            .into()
    }

    /// The ids of the first space's tabs and the numbers they show.
    fn tab_slots(&self) -> Value {
        let rows = self.ls_json();
        let tabs = rows[1]["tabs"].as_array().unwrap().iter();
        tabs.map(|tab| json!([tab["id"], tab["slot"]])).collect()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    #[track_caller]
    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "space {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    #[track_caller]
    fn fails(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(!out.status.success(), "space {args:?} should have failed");
        String::from_utf8(out.stderr).unwrap()
    }

    fn active(&self) -> String {
        self.hypr.lock().unwrap().active.clone()
    }

    fn set_active(&self, name: &str) {
        let mut hypr = self.hypr.lock().unwrap();
        hypr.ensure(name);
        hypr.active = name.into();
    }

    /// Plugs in a second monitor, showing a workspace, numbered 11-20.
    fn plug_in(&self, shows: &str) {
        fs::create_dir_all(self.home().join(".local/state/omarchy")).unwrap();
        fs::write(
            self.home().join(".local/state/omarchy/monitor-workspaces"),
            "0\teDP-1\tBOE\n1\tDP-1\tStudioDisplay\n",
        )
        .unwrap();
        let mut hypr = self.hypr.lock().unwrap();
        hypr.ensure(shows);
        hypr.unplugged = false;
        hypr.elsewhere = Some(shows.into());
    }

    /// Leaves one monitor, with everything on it.
    fn unplug(&self) {
        fs::create_dir_all(self.home().join(".local/state/omarchy")).unwrap();
        fs::write(
            self.home().join(".local/state/omarchy/monitor-workspaces"),
            "0\teDP-1\tBOE\n1\t\tStudioDisplay\n",
        )
        .unwrap();
        let mut hypr = self.hypr.lock().unwrap();
        hypr.unplugged = true;
        hypr.elsewhere = None;
    }

    /// What the monitor you aren't on shows.
    fn elsewhere(&self) -> String {
        self.hypr.lock().unwrap().elsewhere.clone().unwrap()
    }

    fn add_window(&self, workspace: &str) -> String {
        let mut hypr = self.hypr.lock().unwrap();
        hypr.ensure(workspace);
        hypr.windows.push(workspace.into());
        hypr.next_address += 1;
        let address = hypr.next_address;
        hypr.addresses.push(address);
        format!("0x{address:x}")
    }

    fn windows(&self) -> Vec<String> {
        self.hypr.lock().unwrap().windows.clone()
    }

    fn dispatched(&self) -> Vec<String> {
        std::mem::take(&mut self.hypr.lock().unwrap().dispatched)
    }

    fn write_config(&self, text: &str) {
        let dir = self.home().join(".config/omarchy-spaces");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), text).unwrap();
    }
}

#[test]
fn tab_go_in_home_is_omarchys_workspace() {
    let env = Env::new();
    env.ok(&["tab", "go", "3"]);
    assert_eq!(env.dispatched(), ["o.workspaces.focus(3)"]);
    assert_eq!(env.active(), "3");

    // Already there: nothing to do.
    env.ok(&["tab", "go", "3"]);
    assert!(env.dispatched().is_empty());
}

#[test]
fn tab_go_in_home_without_omarchy_is_the_plain_workspace() {
    let env = Env::new();
    env.hypr.lock().unwrap().omarchy = false;
    env.ok(&["tab", "go", "4"]);
    assert_eq!(
        env.dispatched(),
        [
            "o.workspaces.focus(4)",
            r#"hl.dsp.focus({ workspace = "4" })"#
        ]
    );
    assert_eq!(env.active(), "4");
}

#[test]
fn tab_go_stays_inside_the_space() {
    let env = Env::new();
    env.add_window("s1:1");
    env.set_active("s1:1");
    env.ok(&["tab", "go", "2"]);
    assert_eq!(
        env.dispatched(),
        [r#"hl.dsp.focus({ workspace = "name:s1:2" })"#]
    );
    assert_eq!(env.active(), "s1:2");
    // Past the last tab, a new one after it: tabs have no gaps.
    env.ok(&["tab", "go", "10"]);
    assert_eq!(env.active(), "s1:3");

    let last = fs::read_to_string(env.state().join("last")).unwrap();
    assert!(last.starts_with("s1\t3\t"), "{last}");
}

#[test]
fn renumber_closes_the_gaps_a_tab_leaves() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_window("s1:3");
    env.add_window("s1:4");
    env.add_window("s2:2");
    env.add_window("2");
    env.add_window("5");
    env.set_active("s1:4");
    env.ok(&["tab", "rename", "s1:4", "server"]);
    env.ok(&["go", "s2"]);

    env.ok(&["renumber"]);
    assert_eq!(
        env.windows(),
        ["s1:1", "s1:2", "s1:3", "s2:1", "2", "5"],
        "home's keep theirs"
    );
    assert_eq!(env.tab_names(1), json!({ "s1:3": "server" }));
    // Going back lands on the tab you left, as it's numbered now.
    env.ok(&["go", "s1"]);
    assert_eq!(env.active(), "s1:3");

    // Nothing to close.
    let renames = |env: &Env| {
        let hypr = env.hypr.lock().unwrap();
        hypr.evaluated
            .iter()
            .filter(|script| script.contains("workspace.rename"))
            .count()
    };
    let before = renames(&env);
    env.ok(&["renumber"]);
    assert_eq!(renames(&env), before);
}

#[test]
fn tab_go_rejects_bad_numbers() {
    let env = Env::new();
    for bad in ["0", "11", "x"] {
        let err = env.fails(&["tab", "go", bad]);
        assert!(err.contains("isn't a tab"), "{err}");
    }
}

#[test]
fn window_tab_takes_the_window_within_the_space() {
    let env = Env::new();
    env.set_active("s1:1");
    env.add_window("s1:1");
    env.add_window("s1:1");

    env.ok(&["window", "tab", "2"]);
    assert_eq!(
        env.dispatched(),
        [r#"hl.dsp.window.move({ workspace = "name:s1:2", follow = true })"#]
    );
    assert_eq!(env.active(), "s1:2");

    env.set_active("s1:1");
    env.ok(&["window", "tab", "3", "--silent"]);
    assert_eq!(
        env.dispatched(),
        [r#"hl.dsp.window.move({ workspace = "name:s1:3", follow = false })"#]
    );
    assert_eq!(env.active(), "s1:1");
    assert_eq!(env.windows(), ["s1:2", "s1:3"]);
}

#[test]
fn window_tab_in_home_uses_omarchy_and_ignores_empty_workspaces() {
    let env = Env::new();
    env.ok(&["window", "tab", "2"]);
    assert!(env.dispatched().is_empty(), "no window to move");

    env.add_window("1");
    env.ok(&["window", "tab", "2", "--silent"]);
    assert_eq!(env.dispatched(), ["o.workspaces.move_window(2, false)"]);
    assert_eq!(env.windows(), ["2"]);
    assert_eq!(env.active(), "1");
}

#[test]
fn send_takes_the_window_to_another_space_where_you_left_it() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_terminal("s2:1", &env.dir("mail"));
    env.add_window("s2:3");
    env.ok(&["go", "mail"]);
    env.ok(&["tab", "go", "3"]);
    env.ok(&["go", "ren"]);
    env.add_window("s1:1");
    env.dispatched();

    env.ok(&["window", "send", "mail"]);
    assert_eq!(
        env.dispatched(),
        [r#"hl.dsp.window.move({ workspace = "name:s2:3", follow = true })"#]
    );
    assert_eq!(env.active(), "s2:3", "the tab you left mail on");
    let mut windows = env.windows();
    windows.sort();
    assert_eq!(windows, ["s1:1", "s2:1", "s2:3", "s2:3"]);
    env.ok(&["go", "ren"]);
    assert_eq!(env.active(), "s1:1", "ren remembers where you left it");

    // Already there, or no window: nothing to do.
    env.ok(&["window", "send", "s1"]);
    env.set_active("s1:5");
    env.ok(&["window", "send", "mail"]);
    assert!(env.dispatched().iter().all(|d| !d.contains("window.move")));
}

impl Env {
    /// The names of the workspaces there are, as `space` reads them.
    fn workspace_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .hypr
            .lock()
            .unwrap()
            .workspaces
            .iter()
            .map(|(_, name)| name.clone())
            .collect();
        names.sort();
        names
    }

    /// The tabs of space `id` as `renumber` leaves them after its first tab
    /// ended: the workspaces made as `s<n>:2` and `s<n>:3` are named `:1` and
    /// `:2`, and keep their addresses (Hyprland 0.57), so `name:id:1` means no
    /// workspace, and `name:id:2` the one named `:1`.
    fn renamed_space(&self, id: &str, dir: &Path) {
        self.add_terminal(&format!("{id}:2"), dir);
        self.add_window(&format!("{id}:3"));
        self.add_window(&format!("{id}:3"));
        self.ok(&["renumber"]);
        assert_eq!(
            self.hypr.lock().unwrap().address(&format!("{id}:1")),
            format!("{id}:2")
        );
    }
}

#[test]
fn tabs_renamed_by_renumber_are_found_by_their_names() {
    let env = Env::new();
    env.renamed_space("s1", &env.dir("ren"));
    env.set_active("s1:2");
    let windows = env.windows();

    // The first tab is the one with a window, not an empty new one.
    env.ok(&["tab", "go", "1"]);
    assert_eq!(env.active(), "s1:1");
    env.ok(&["tab", "go", "2"]);
    assert_eq!(env.active(), "s1:2");
    assert_eq!(env.workspace_names(), ["s1:1", "s1:2"]);

    // Past the last tab, a new one: not the one whose address its number is.
    env.ok(&["tab", "go", "3"]);
    assert_eq!(
        env.workspace_names().len(),
        3,
        "{:?}",
        env.workspace_names()
    );
    assert_eq!(env.windows(), windows, "no window moved");
    env.ok(&["renumber"]);
    assert_eq!(env.active(), "s1:3");
    assert_eq!(env.workspace_names(), ["s1:1", "s1:2", "s1:3"]);
}

#[test]
fn send_and_window_to_tab_find_renamed_tabs_by_their_names() {
    let env = Env::new();
    env.renamed_space("s1", &env.dir("ren"));
    env.add_terminal("s2:1", &env.dir("mail"));
    env.add_window("s2:1");
    env.add_window("s2:1");
    let tabs = ["s1:1", "s1:2", "s2:1"];
    env.set_active("s1:1");
    env.add_window("s1:1");
    env.dispatched();

    // Within a space whose tabs are renamed: the one the second key means.
    env.ok(&["window", "tab", "2", "--silent"]);
    assert_eq!(env.workspace_names(), tabs);
    let on = |env: &Env, tab: &str| env.windows().iter().filter(|w| *w == tab).count();
    assert_eq!(on(&env, "s1:2"), 3);

    // Into it from another space, with you and silently: onto a tab that is
    // there, not a second one of the same name.
    for silent in [false, true] {
        env.set_active("s2:1");
        let before = on(&env, "s1:1") + on(&env, "s1:2");
        let mut args = vec!["window", "send", "s1"];
        if silent {
            args.push("--silent");
        }
        env.ok(&args);
        assert_eq!(env.workspace_names(), tabs, "silent: {silent}");
        assert_eq!(on(&env, "s1:1") + on(&env, "s1:2"), before + 1);
        env.ok(&["go", "s1"]);
        assert!(["s1:1", "s1:2"].contains(&env.active().as_str()));
    }
    // Past the last tab: a new one, not the tab whose address is s1:3.
    env.set_active("s1:1");
    env.add_window("s1:1");
    let before = on(&env, "s1:2");
    env.ok(&["window", "tab", "3", "--silent"]);
    assert_eq!(on(&env, "s1:2"), before, "not onto the tab named s1:2");
    assert_eq!(env.windows().iter().filter(|w| *w == "s1:4").count(), 1);
}

#[test]
fn send_next_and_prev_and_home() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_terminal("s2:2", &env.dir("mail"));
    env.add_window("1");
    env.set_active("1");

    env.ok(&["window", "send", "next"]);
    assert_eq!(env.active(), "s1:1");
    env.ok(&["window", "send", "prev"]);
    assert_eq!(
        env.dispatched().last().unwrap(),
        "o.workspaces.move_window(1)"
    );
    assert_eq!(env.active(), "1");
    env.ok(&["window", "send", "prev", "--silent"]);
    assert_eq!(env.active(), "1");
    let mut windows = env.windows();
    windows.sort();
    assert_eq!(windows, ["s1:1", "s2:2", "s2:2"]);
}

#[test]
fn sending_a_spaces_last_window_away_ends_it() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_terminal("s2:1", &env.dir("mail"));
    env.add_window("s2:2");
    env.ok(&["go", "ren"]);
    env.ok(&["go", "mail"]);
    env.ok(&["tab", "go", "2"]);

    // Silently: the emptied tab gives way to the next one with windows.
    env.ok(&["window", "send", "ren", "--silent"]);
    assert_eq!(env.active(), "s2:1");

    // Following: mail is gone, and so is where you left it.
    env.ok(&["window", "send", "ren"]);
    assert_eq!(env.active(), "s1:1");
    let labels: Vec<String> = env
        .ls_json()
        .iter()
        .map(|r| r["label"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(labels, ["home", "ren"]);
    let visits = fs::read_to_string(env.state().join("last")).unwrap();
    assert!(!visits.contains("s2\t"), "{visits}");
}

#[test]
fn ws_new_next_and_prev_move_between_a_spaces_tabs() {
    let env = Env::new();
    env.add_window("s1:1");
    env.add_window("s1:3");
    env.add_window("s2:2");
    env.set_active("s1:1");

    env.ok(&["tab", "next"]);
    assert_eq!(env.active(), "s1:3");
    env.ok(&["tab", "next"]);
    assert_eq!(env.active(), "s1:1", "wraps, skipping other spaces");
    env.ok(&["tab", "prev"]);
    assert_eq!(env.active(), "s1:3");
    env.ok(&["tab", "new"]);
    assert_eq!(env.active(), "s1:4", "after the last tab");
    env.ok(&["tab", "go", "1"]);
    env.ok(&["tab", "new"]);
    assert_eq!(env.active(), "s1:5", "the new tab kept its terminal");
}

#[test]
fn ws_new_opens_a_tab_with_a_terminal_in_the_spaces_folder() {
    let env = Env::new();
    let ren = env.dir("Projects/ren");
    fs::create_dir_all(ren.join(".git")).unwrap();
    let src = env.dir("Projects/ren/src");
    env.add_terminal("s1:1", &src);
    env.set_active("s1:1");
    env.ok(&["ls"]);
    env.dispatched();

    env.ok(&["tab", "new"]);
    assert_eq!(env.active(), "s1:2");
    assert_eq!(env.windows().last().unwrap(), "s1:2");
    // The space's checkout, not the subfolder its terminal is in.
    assert_eq!(env.launched(1), [ren.display().to_string()]);
    // As with a new space: the terminal opens there first, then you go.
    let dispatched = env.dispatched();
    assert!(
        dispatched[0].ends_with(r#"{ workspace = "name:s1:2 silent", no_anim = true })"#),
        "{dispatched:?}"
    );
    assert_eq!(
        dispatched[1],
        r#"hl.dsp.focus({ workspace = "name:s1:2" })"#
    );
    let window = env.newest_window();
    assert_eq!(
        dispatched[2],
        format!(
            r#"hl.dsp.window.set_prop({{ prop = "no_anim", value = "false", window = "address:{window}" }})"#
        )
    );
}

#[test]
fn ws_new_in_home_uses_omarchys_next_free_workspace() {
    let env = Env::new();
    env.add_window("1");
    env.add_window("2");
    env.ok(&["tab", "new"]);
    assert_eq!(env.dispatched(), ["o.workspaces.focus(3)"]);

    // A single tab has no next.
    let env = Env::new();
    env.ok(&["tab", "next"]);
    assert!(env.dispatched().is_empty());
}

#[test]
fn new_opens_a_space_with_a_terminal_in_the_focused_terminals_folder() {
    let env = Env::new();
    let ren = env.dir("Projects/ren");
    fs::create_dir_all(ren.join(".git")).unwrap();
    let src = env.dir("Projects/ren/src");
    env.add_terminal("1", &src);

    let out = env.ok(&["new"]);
    assert_eq!(out, "Opened a space in ~/Projects/ren/src\n");
    assert_eq!(env.active(), "s1:1");
    assert_eq!(env.launched(1), [src.display().to_string()]);
    // The terminal opened there first, without its pop-in, which would still
    // be fading it in from nothing as you arrived; only then did you go
    // there, so the space never shows empty. Then it animates as usual.
    let dispatched = env.dispatched();
    assert!(
        dispatched[0].starts_with("hl.dsp.exec_cmd("),
        "{dispatched:?}"
    );
    assert!(
        dispatched[0].ends_with(r#"{ workspace = "name:s1:1 silent", no_anim = true })"#),
        "{dispatched:?}"
    );
    let window = env.newest_window();
    assert_eq!(
        dispatched[2],
        format!(
            r#"hl.dsp.window.set_prop({{ prop = "no_anim", value = "false", window = "address:{window}" }})"#
        )
    );
    assert_eq!(
        dispatched[1],
        r#"hl.dsp.focus({ workspace = "name:s1:1" })"#
    );
    assert_eq!(env.windows().last().unwrap(), "s1:1");

    // Until its terminal shows up, it goes by the folder it opened in, as
    // it will after: by that folder's checkout.
    let rows = env.ls_json();
    assert_eq!(rows[1]["id"], "s1");
    assert_eq!(rows[1]["label"], "ren");
    // Its terminal's shell starts.
    env.cd(&env.newest_window(), &src);
    let rows = env.ls_json();
    assert_eq!(rows[1]["label"], "ren");
    assert_eq!(rows[1]["named"], false);
    assert_eq!(rows[1]["folder"], ren.display().to_string());
    assert_eq!(rows[1]["active"], true);
}

#[test]
fn spaces_opened_at_once_each_get_their_own() {
    let env = Env::new();
    env.hypr.lock().unwrap().terminal_start = Duration::from_millis(400);
    // Pressed again before the first one's terminal is up.
    let mut presses: Vec<_> = (0..3)
        .map(|_| env.command(&["new"]).spawn().unwrap())
        .collect();
    for press in &mut presses {
        assert!(press.wait().unwrap().success());
    }

    let mut windows = env.windows();
    windows.sort();
    assert_eq!(windows, ["s1:1", "s2:1", "s3:1"]);
    assert_eq!(env.ls_json().len(), 4, "and home");
}

#[test]
fn tabs_opened_at_once_each_get_their_own() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.set_active("s1:1");
    env.hypr.lock().unwrap().terminal_start = Duration::from_millis(400);
    let mut presses: Vec<_> = (0..3)
        .map(|_| env.command(&["tab", "new"]).spawn().unwrap())
        .collect();
    for press in &mut presses {
        assert!(press.wait().unwrap().success());
    }

    let mut windows = env.windows();
    windows.sort();
    assert_eq!(windows, ["s1:1", "s1:2", "s1:3", "s1:4"]);
}

#[test]
fn changes_made_at_once_are_all_kept() {
    let env = Env::new();
    let ids: Vec<String> = (1..=6).map(|n| format!("s{n}")).collect();
    for id in &ids {
        env.add_window(&format!("{id}:1"));
    }
    env.ok(&["ls"]);
    let _watch = Watch::start(&env);
    for round in 0..10 {
        let icon = round.to_string();
        let names: Vec<String> = ids.iter().map(|id| format!("{id} {round}")).collect();
        let mut renames: Vec<_> = ids
            .iter()
            .zip(&names)
            .flat_map(|(id, name)| {
                [["rename", id, name], ["icon", id, &icon]]
                    .map(|args| env.command(&args).spawn().unwrap())
            })
            .collect();
        for rename in &mut renames {
            assert!(rename.wait().unwrap().success());
        }
        let rows = env.ls_json();
        for (row, name) in rows[1..].iter().zip(&names) {
            assert_eq!(row["label"], name.as_str());
            assert_eq!(row["icon"], icon.as_str());
        }
        assert_eq!(rows.len(), names.len() + 1, "and home");
    }
}

#[test]
fn the_first_space_opens_at_home() {
    let env = Env::new();
    env.ok(&["new"]);
    assert_eq!(env.launched(1), [env.dir("").display().to_string()]);
    assert_eq!(env.ls_json()[1]["label"], "~");
}

#[test]
fn restore_opens_the_last_sessions_spaces_once() {
    let env = Env::new();
    let (notes, mail) = (env.dir("notes"), env.dir("mail"));
    let gone = mail.join("gone");
    let state = env.home().join(".local/state/omarchy-spaces");
    fs::create_dir_all(&state).unwrap();
    let saved = json!({ "instance": "before", "spaces": [
        { "name": "Work", "icon": "x", "tabs": { "2": "logs" }, "windows": [
            { "tab": 1, "terminal": { "folder": notes } },
            { "tab": 2, "terminal": { "folder": mail } } ] },
        { "windows": [{ "tab": 3, "terminal": { "folder": gone } }] },
    ] });
    fs::write(state.join("session.json"), saved.to_string()).unwrap();

    // A space already open keeps its id.
    env.add_window("s1:1");
    env.ok(&["restore"]);
    assert_eq!(env.active(), "1");
    assert_eq!(
        env.hypr.lock().unwrap().windows,
        ["s1:1", "s2:1", "s2:2", "s3:3"]
    );
    let mail = mail.display().to_string();
    assert_eq!(
        env.launched(3),
        [notes.display().to_string(), mail.clone(), mail]
    );
    let rows = env.ls_json();
    let work = rows.iter().find(|r| r["id"] == "s2").unwrap();
    assert_eq!(work["label"], "Work");
    assert_eq!(work["icon"], "x");
    let tab = &work["tabs"][1];
    assert_eq!(
        (&tab["id"], &tab["label"]),
        (&json!("s2:2"), &json!("logs"))
    );

    env.ok(&["restore"]);
    assert_eq!(env.hypr.lock().unwrap().windows.len(), 4);
}

#[test]
fn restored_spaces_keep_their_ids_and_new_ones_get_their_own() {
    let env = Env::new();
    let (notes, mail) = (env.dir("notes"), env.dir("mail"));
    fs::create_dir_all(env.state()).unwrap();
    // As a restart leaves them, with a space ended since: the sidebar's order
    // isn't the ids', and nothing has pruned the entries.
    let entries = json!({ "version": 1, "next": 5, "spaces": {
        "s1": { "name": "Taxes", "icon": "T", "order": 20 },
        "s3": { "order": 10 },
        "s4": { "name": "Over", "order": 30 } } });
    fs::write(env.state().join("spaces.json"), entries.to_string()).unwrap();
    fs::write(env.state().join("last"), "s1\t2\t100\ns4\t7\t200\n").unwrap();
    let saved = json!({ "version": 1, "instance": "before", "spaces": [
        { "id": "s3", "windows": [{ "tab": 1, "terminal": { "folder": notes } }] },
        { "id": "s1", "name": "Taxes", "icon": "T", "windows": [
            { "tab": 1, "terminal": { "folder": mail } } ] },
    ] });
    fs::write(env.state().join("session.json"), saved.to_string()).unwrap();

    env.ok(&["restore"]);
    let spaces = |env: &Env| -> Vec<(Value, Value)> {
        let rows = env.ls_json();
        let rows = rows[1..].iter();
        rows.map(|r| (r["id"].clone(), r["named"].clone()))
            .collect()
    };
    assert_eq!(
        spaces(&env),
        [(json!("s3"), json!(false)), (json!("s1"), json!(true))]
    );

    // Not `s2`, which some space once was, nor `s4`.
    env.ok(&["new", notes.to_str().unwrap()]);
    let rows = spaces(&env);
    assert_eq!(rows[2], (json!("s5"), json!(false)));
    env.close_window(&env.newest_window());
    env.ok(&["new", notes.to_str().unwrap()]);
    assert_eq!(spaces(&env)[2].0, "s6");
}

#[test]
fn restore_gives_a_space_another_id_when_its_own_is_in_use() {
    let env = Env::new();
    let notes = env.dir("notes");
    fs::create_dir_all(env.state()).unwrap();
    env.add_window("s1:1");
    env.ok(&["rename", "s1", "Here"]);
    let saved = json!({ "version": 1, "instance": "before", "spaces": [
        { "id": "s1", "name": "Before", "windows": [{ "tab": 1, "terminal": { "folder": notes } }] },
    ] });
    fs::write(env.state().join("session.json"), saved.to_string()).unwrap();
    env.ok(&["restore"]);
    let rows = env.ls_json();
    let names: Vec<_> = rows[1..].iter().map(|r| (&r["id"], &r["label"])).collect();
    assert_eq!(
        names,
        [
            (&json!("s1"), &json!("Here")),
            (&json!("s2"), &json!("Before"))
        ]
    );
}

#[test]
fn restore_brings_back_home_and_where_you_were() {
    let env = Env::new();
    let (notes, mail) = (env.dir("notes"), env.dir("mail"));
    fs::create_dir_all(env.state()).unwrap();
    let saved = json!({ "instance": "before", "spaces": [
        { "home": true, "tabs": { "3": "notes" }, "windows": [
            { "tab": 1, "terminal": { "folder": mail } },
            { "tab": 3, "terminal": { "folder": notes } } ] },
        { "active": true, "last": 2, "windows": [
            { "tab": 1, "terminal": { "folder": notes } },
            { "tab": 2, "terminal": { "folder": mail } } ] },
    ] });
    fs::write(env.state().join("session.json"), saved.to_string()).unwrap();

    env.ok(&["restore"]);
    assert_eq!(env.hypr.lock().unwrap().windows, ["1", "3", "s1:1", "s1:2"]);
    assert_eq!(env.tab_names(0), json!({ "home:3": "notes" }));
    assert_eq!(env.active(), "s1:2");
    assert!(env.hypr.lock().unwrap().evaluated.is_empty());
}

#[test]
fn restore_leaves_you_where_you_have_started_on_something() {
    let env = Env::new();
    let notes = env.dir("notes");
    fs::create_dir_all(env.state()).unwrap();
    let saved = json!({ "instance": "before", "spaces": [
        { "active": true, "last": 1, "windows": [{ "tab": 1, "terminal": { "folder": notes } }] },
    ] });
    fs::write(env.state().join("session.json"), saved.to_string()).unwrap();
    env.add_window("1");
    env.ok(&["restore"]);
    assert_eq!(env.hypr.lock().unwrap().windows, ["1", "s1:1"]);
    assert_eq!(env.active(), "1");
}

#[test]
fn restore_arranges_windows_as_they_were() {
    let env = Env::new();
    let notes = env.dir("notes");
    env.hypr.lock().unwrap().layouts = true;
    fs::create_dir_all(env.state()).unwrap();
    let saved = json!({ "instance": "before", "spaces": [{
        "layouts": { "1": {
            "tiles": { "split": "down", "share": 0.7, "first": [1], "second": [0] },
            "focused": 0,
        } },
        "windows": [
            { "tab": 1, "terminal": { "folder": notes } },
            { "tab": 1, "terminal": { "folder": notes } },
            { "tab": 2, "terminal": { "folder": notes }, "fullscreen": 1 },
        ],
    }] });
    fs::write(env.state().join("session.json"), saved.to_string()).unwrap();
    env.ok(&["restore"]);
    let windows = env.hypr.lock().unwrap().addresses.clone();
    let [a, b, c] = windows[..] else {
        panic!("{windows:?}")
    };
    let scripts = env.hypr.lock().unwrap().evaluated.clone();
    assert_eq!(scripts.len(), 2, "one a tab");
    let split = format!(r#"split("address:0x{b:x}", "address:0x{a:x}", "d", 1.4000)"#);
    assert!(scripts[0].contains(&split), "{}", scripts[0]);
    assert!(scripts[0].contains(&format!(r#"focus("address:0x{a:x}")"#)));
    let full = format!(r#"mode = "maximized", action = "set", window = "address:0x{c:x}""#);
    assert!(scripts[1].contains(&full), "{}", scripts[1]);
}

#[test]
fn restore_picks_up_conversations_where_they_were() {
    let id = "be6fb713-2beb-4233-86c2-266ee407ec56";
    for resume in [true, false] {
        let env = Env::new();
        let notes = env.dir("notes");
        env.write_config(&format!("[restore]\nagents = {resume}\n"));
        fs::create_dir_all(env.state()).unwrap();
        let talk =
            |id: &str, folder: &Path| json!({ "agent": "claude", "id": id, "folder": folder });
        let saved = json!({ "instance": "before", "spaces": [{ "windows": [
            { "tab": 1, "terminal": { "folder": notes, "conversation": talk(id, &notes) } },
            { "tab": 2, "terminal": { "folder": notes, "conversation": talk("x; rm -rf ~", &notes) } },
            { "tab": 3, "terminal": { "folder": notes.join("gone"),
              "conversation": talk(id, &notes.join("gone")) } },
        ] }] });
        fs::write(env.state().join("session.json"), saved.to_string()).unwrap();
        env.ok(&["restore"]);
        let noted = |file: &str| fs::read_to_string(env.home().join(file)).unwrap();
        let (first, runs) = if resume {
            let space = env!("CARGO_BIN_EXE_space");
            (format!("claude {id}"), format!("{space} shell"))
        } else {
            Default::default()
        };
        assert_eq!(noted("resumed"), format!("{first}\n\n\n"));
        assert_eq!(noted("ran"), format!("{runs}\n\n\n"));
    }
}

#[test]
fn restore_opens_the_browsers_windows_among_the_terminals() {
    for wanted in [true, false] {
        let env = Env::new();
        let notes = env.dir("notes");
        env.hypr.lock().unwrap().layouts = true;
        env.write_config(&format!("[restore]\nbrowsers = {wanted}\n"));
        let entry = env.home().join(".local/share/applications/sh.desktop");
        fs::write(entry, "[Desktop Entry]\nExec=sh\n").unwrap();
        fs::create_dir_all(env.state()).unwrap();
        let saved = json!({ "instance": "before", "spaces": [
            {
                "layouts": { "1": { "tiles":
                    { "split": "right", "share": 0.4, "first": [1], "second": [0] } } },
                "windows": [
                    { "tab": 1, "terminal": { "folder": notes } },
                    { "tab": 1, "browser": { "name": "chromium", "profile": "Default",
                        "urls": ["https://a/", "https://b/"], "tab": 1 } },
                ],
            },
            { "name": "Reading", "windows": [{ "tab": 2, "browser":
                { "name": "chromium", "profile": "Default", "urls": ["https://c/"] } }] },
            { "windows": [
                { "tab": 1, "browser": { "name": "google-chrome", "class": "google-chrome",
                    "profile": "Profile 1", "urls": ["https://d/"] } },
                // An app that's installed, but no browser.
                { "tab": 2, "browser": { "name": "sh", "profile": "Default",
                    "urls": ["https://e/"] } },
            ] },
        ] });
        fs::write(env.state().join("session.json"), saved.to_string()).unwrap();
        env.ok(&["restore"]);
        let hypr = env.hypr.lock().unwrap();
        assert_eq!(hypr.browser_on, None, "the rule is gone");
        // What they held is only kept if it's to be opened again.
        let kept = fs::read_to_string(env.state().join("session-restored.json")).unwrap();
        assert_eq!(kept.contains("https://"), wanted, "{kept}");
        if !wanted {
            assert_eq!(hypr.windows, ["s1:1"]);
            continue;
        }
        assert_eq!(hypr.windows, ["s1:1", "s1:1", "s2:2", "s3:1"]);
        let (terminal, browser) = (hypr.addresses[0], hypr.addresses[1]);
        let split =
            format!(r#"split("address:0x{browser:x}", "address:0x{terminal:x}", "r", 0.8000)"#);
        assert!(hypr.evaluated[0].contains(&split), "{}", hypr.evaluated[0]);
        let opened = "'--new-window' 'https://b/' 'https://a/'";
        assert!(hypr.dispatched.iter().any(|d| d.contains(opened)));
        let opened = "'chrome-stub' '--profile-directory=Profile 1' '--new-window' 'https://d/'";
        assert!(hypr.dispatched.iter().any(|d| d.contains(opened)));
    }
}

#[test]
fn restore_gets_by_without_a_config_it_can_read() {
    let env = Env::new();
    let notes = env.dir("notes");
    env.write_config("[restore]\nauto = 'perhaps'\n");
    fs::create_dir_all(env.state()).unwrap();
    let saved = json!({ "instance": "before", "spaces": [
        { "windows": [{ "tab": 1, "terminal": { "folder": notes } }] },
    ] });
    fs::write(env.state().join("session.json"), saved.to_string()).unwrap();
    env.ok(&["restore"]);
    assert_eq!(env.launched(1), [notes.display().to_string()]);
}

#[test]
fn restore_auto_skips_when_disabled_but_manual_still_works() {
    let env = Env::new();
    let notes = env.dir("notes");
    env.write_config("[restore]\nauto = false\n");
    fs::create_dir_all(env.state()).unwrap();
    let saved = json!({ "instance": "before", "spaces": [
        { "name": "Work", "windows": [{ "tab": 1, "terminal": { "folder": notes } }] },
    ] });
    fs::write(env.state().join("session.json"), saved.to_string()).unwrap();

    // Startup restore (`--auto`) leaves the session alone.
    env.ok(&["restore", "--auto"]);
    assert!(env.windows().is_empty());

    // Running `space restore` by hand still restores it.
    env.ok(&["restore"]);
    assert_eq!(env.ls_json()[1]["label"], "Work");
}

#[test]
fn restore_opens_an_earlier_session_from_its_file() {
    let env = Env::new();
    let notes = env.dir("notes");
    let file = env.home().join("earlier.json");
    let saved = json!({ "instance": "test", "spaces": [
        { "name": "Work", "windows": [{ "tab": 1, "terminal": { "folder": notes } }] },
    ] });
    fs::write(&file, saved.to_string()).unwrap();
    env.ok(&["restore", file.to_str().unwrap()]);
    assert_eq!(env.ls_json()[1]["label"], "Work");
    assert!(
        env.fails(&["restore", "missing.json"])
            .contains("missing.json")
    );
}

#[test]
fn new_from_a_window_that_isnt_a_terminal_uses_the_spaces_folder() {
    let env = Env::new();
    let mail = env.dir("mail");
    env.ok(&["new", "mail"]);
    env.cd(&env.newest_window(), &mail);
    env.add_window("s1:1");
    env.ok(&["ls"]);

    env.ok(&["new"]);
    assert_eq!(env.active(), "s2:1");
    assert_eq!(
        env.launched(2),
        [mail.display().to_string(), mail.display().to_string()]
    );

    let err = env.fails(&["new", "nowhere"]);
    assert!(err.contains("no folder ~/nowhere"), "{err}");
}

#[test]
fn a_space_follows_its_first_terminal_from_folder_to_folder() {
    let env = Env::new();
    let (notes, japanese) = (env.dir("notes"), env.dir("japanese"));
    env.set_active("s1:1");
    let first = env.add_terminal("s1:1", &notes);
    assert_eq!(env.ls_json()[1]["label"], "notes");

    env.cd(&first, &japanese);
    assert_eq!(env.ls_json()[1]["label"], "japanese");

    // A second terminal, wherever it is, doesn't change it.
    let second = env.add_terminal("s1:2", &notes);
    assert_eq!(env.ls_json()[1]["label"], "japanese");

    // Once the first closes, the next one names it.
    env.close_window(&first);
    assert_eq!(env.ls_json()[1]["label"], "notes");
    env.close_window(&second);
    assert_eq!(env.ls_json()[1]["label"], "notes", "the last folder it saw");
}

#[test]
fn rename_gives_a_name_that_stays_until_cleared() {
    let env = Env::new();
    let notes = env.dir("notes");
    env.set_active("s1:1");
    let terminal = env.add_terminal("s1:1", &notes);

    assert_eq!(
        env.ok(&["rename", "Japanese"]),
        "Named the space 'Japanese'\n"
    );
    env.cd(&terminal, &env.dir("elsewhere"));
    let rows = env.ls_json();
    assert_eq!(rows[1]["label"], "Japanese");
    assert_eq!(rows[1]["named"], true);

    // By what it's called, from elsewhere; then back to its folder's name.
    env.set_active("1");
    env.ok(&["rename", "japanese", "  "]);
    assert_eq!(env.ls_json()[1]["label"], "elsewhere");

    let err = env.fails(&["rename", "home", "x"]);
    assert!(err.contains("can't be renamed"), "{err}");
    let err = env.fails(&["rename", "x"]);
    assert!(
        err.contains("can't be renamed"),
        "in home, nothing to rename: {err}"
    );
    let err = env.fails(&["rename", "s1", "a\nb"]);
    assert!(err.contains("one line"), "{err}");
}

#[test]
fn icon_shows_on_the_badge_until_cleared() {
    let env = Env::new();
    let notes = env.dir("notes");
    env.set_active("s1:1");
    env.add_terminal("s1:1", &notes);
    assert_eq!(env.ls_json()[1]["icon"], Value::Null);

    assert_eq!(env.ok(&["icon", "🚀"]), "Gave the space the icon 🚀\n");
    let rows = env.ls_json();
    assert_eq!(rows[1]["icon"], "🚀");
    assert_eq!(rows[1]["label"], "notes", "the name stays its folder's");

    // By what it's called, from elsewhere; then back to its letters.
    env.set_active("1");
    env.ok(&["icon", "notes", "\u{f0415}"]);
    assert_eq!(env.ls_json()[1]["icon"], "\u{f0415}");
    assert_eq!(
        env.ok(&["icon", "s1", ""]),
        "The space shows its letters again\n"
    );
    assert_eq!(env.ls_json()[1]["icon"], Value::Null);

    let err = env.fails(&["icon", "home", "🏠"]);
    assert!(err.contains("can't have an icon"), "{err}");
    let err = env.fails(&["icon", "s1", "a b"]);
    assert!(err.contains("without spaces"), "{err}");

    let logo = env.dir("notes").join("logo.png");
    std::fs::write(&logo, b"\x89PNG").unwrap();
    let canonical = std::fs::canonicalize(&logo).unwrap();
    env.ok(&["icon", "notes", &logo.to_string_lossy()]);
    assert_eq!(
        env.ls_json()[1]["icon"],
        canonical.to_string_lossy().as_ref()
    );
}

#[test]
fn tab_rename_names_a_tab_until_cleared_or_closed() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    let second = env.add_window("s1:2");
    env.set_active("s1:1");
    let tabs = |env: &Env| env.tab_names(1);

    assert_eq!(
        env.ok(&["tab", "rename", "s1:2", "server"]),
        "Named tab 2 'server'\n"
    );
    assert_eq!(tabs(&env), json!({ "s1:2": "server" }));
    env.ok(&["tab", "rename", "s1:1", "editor"]);
    assert_eq!(tabs(&env), json!({ "s1:1": "editor", "s1:2": "server" }));
    assert_eq!(
        env.ok(&["tab", "rename", "s1:1", ""]),
        "Tab 1 is called after its window again\n"
    );
    assert_eq!(tabs(&env), json!({ "s1:2": "server" }));

    // The name goes with its tab, so a new tab there starts without it.
    env.close_window(&second);
    assert_eq!(tabs(&env), serde_json::json!({}));

    let err = env.fails(&["tab", "rename", "s1:4", "x"]);
    assert!(err.contains("there's no tab s1:4"), "{err}");
}

#[test]
fn tab_rename_names_home_tabs_by_workspace() {
    let env = Env::new();
    let mail = env.add_window("3");
    let other = env.add_window("13");
    env.set_active("3");
    let tabs = |env: &Env| env.tab_names(0);

    // The number is the workspace's, as the second monitor's tab 3 is 13.
    assert_eq!(
        env.ok(&["tab", "rename", "home:3", "mail"]),
        "Named tab 3 'mail'\n"
    );
    assert_eq!(tabs(&env), json!({ "home:3": "mail" }));

    // From a space too.
    env.add_window("s1:1");
    env.set_active("s1:1");
    assert_eq!(
        env.ok(&["tab", "rename", "home:13", "x"]),
        "Named tab 3 'x'\n"
    );
    env.close_window(&other);
    env.ok(&["tab", "rename", "home:3", "inbox"]);
    assert_eq!(tabs(&env), json!({ "home:3": "inbox" }));

    assert_eq!(
        env.ok(&["tab", "rename", "home:3", ""]),
        "Tab 3 shows just its number again\n"
    );
    assert_eq!(tabs(&env), serde_json::json!({}));
    let err = env.fails(&["tab", "rename", "home:5", "x"]);
    assert!(err.contains("there's no tab home:5"), "{err}");

    // The name goes with its workspace, once Hyprland drops it.
    env.ok(&["tab", "rename", "home:3", "mail"]);
    env.close_window(&mail);
    assert_eq!(tabs(&env), serde_json::json!({}));
}

#[test]
fn go_finds_spaces_by_id_or_by_what_they_are_called() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_terminal("s2:3", &env.dir("mail"));

    env.ok(&["go", "mail"]);
    assert_eq!(env.active(), "s2:3");
    env.ok(&["go", "s1"]);
    assert_eq!(env.active(), "s1:1");
    env.ok(&["go", "Mail"]);
    assert_eq!(env.active(), "s2:3", "ignoring case");
    env.ok(&["go", "home"]);
    assert_eq!(env.active(), "1");

    let err = env.fails(&["go", "rne"]);
    assert!(
        err.contains("no space called 'rne'. Did you mean ren?"),
        "{err}"
    );

    // Two spaces in the same folder are told apart by id.
    env.add_terminal("s3:1", &env.dir("ren"));
    let err = env.fails(&["go", "ren"]);
    assert!(
        err.contains("2 spaces are called 'ren'; pick one by id: s1, s3"),
        "{err}"
    );
}

#[test]
fn next_and_prev_move_down_and_up_the_spaces_wrapping_around() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_terminal("s2:3", &env.dir("mail"));
    env.set_active("1");

    env.ok(&["next"]);
    assert_eq!(env.active(), "s1:1");
    env.ok(&["next"]);
    assert_eq!(env.active(), "s2:3", "its only tab");
    env.ok(&["next"]);
    assert_eq!(env.active(), "1", "wraps to home");
    env.ok(&["prev"]);
    assert_eq!(env.active(), "s2:3", "wraps to the last");
    env.ok(&["prev"]);
    assert_eq!(env.active(), "s1:1");
}

#[test]
fn next_with_only_home_stays_put() {
    let env = Env::new();
    env.set_active("2");
    env.ok(&["next"]);
    env.ok(&["prev"]);
    assert_eq!(env.active(), "2");
}

#[test]
fn spaces_end_with_their_windows_and_keep_the_order_they_were_made_in() {
    let env = Env::new();
    env.ok(&["new"]);
    let first = env.newest_window();
    env.cd(&first, &env.dir("zebra"));
    env.ok(&["new"]);
    env.cd(&env.newest_window(), &env.dir("apple"));
    let labels = |env: &Env| -> Vec<String> {
        env.ls_json()
            .iter()
            .map(|r| r["label"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(labels(&env), ["home", "zebra", "apple"]);

    env.close_window(&first);
    assert_eq!(labels(&env), ["home", "apple"]);
    let store = fs::read_to_string(env.state().join("spaces.json")).unwrap();
    assert!(!store.contains("\"s1\""), "{store}");

    // Its id isn't another's.
    env.ok(&["new"]);
    assert_eq!(env.active(), "s3:1");
}

#[test]
fn close_closes_the_spaces_windows_and_leaves_it() {
    let env = Env::new();
    env.add_window("1");
    env.add_terminal("s1:1", &env.dir("tax"));
    env.add_terminal("s2:1", &env.dir("ren"));
    env.add_window("s2:2");
    env.ok(&["go", "tax"]);
    env.ok(&["go", "ren"]);
    env.ok(&["tab", "go", "2"]);

    assert_eq!(env.ok(&["close"]), "Closed 2 windows in 'ren'\n");
    assert_eq!(env.windows(), ["1", "s1:1"]);
    assert_eq!(env.active(), "s1:1", "back to the space before");

    assert_eq!(env.ok(&["close", "tax"]), "Closed 1 window in 'tax'\n");
    assert_eq!(env.windows(), ["1"]);
    assert_eq!(env.active(), "1", "then home");

    let err = env.fails(&["close", "home"]);
    assert!(err.contains("one by one"), "{err}");
    let err = env.fails(&["close", "nope"]);
    assert!(err.contains("no space called 'nope'"), "{err}");
}

#[test]
fn tab_send_renames_a_tab_into_another_space_and_goes_there() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_window("s1:2");
    env.add_window("s1:2");
    env.add_terminal("s2:1", &env.dir("mail"));
    env.set_active("s1:2");
    env.ok(&["tab", "rename", "s1:2", "server"]);
    env.dispatched();

    assert_eq!(env.ok(&["tab", "send", "mail"]), "Moved tab 2 to 'mail'\n");
    assert_eq!(
        env.dispatched()[0],
        r#"hl.dsp.workspace.rename({ workspace = "name:s1:2", name = "s2:2" })"#
    );
    assert_eq!(env.active(), "s2:2", "after mail's tabs");
    assert_eq!(env.windows(), ["s1:1", "s2:2", "s2:2", "s2:1"]);
    let rows = env.ls_json();
    assert_eq!(rows.len(), 3);
    assert_eq!(env.tab_names(1), json!({}));
    assert_eq!(env.tab_names(2), json!({ "s2:2": "server" }));

    // Going back lands on a tab ren still has.
    env.ok(&["go", "ren"]);
    assert_eq!(env.active(), "s1:1");

    // Another tab than the one you're on, and ren's only one: it ends.
    env.ok(&["go", "mail"]);
    env.ok(&["tab", "send", "s1:1", "s2"]);
    assert_eq!(env.active(), "s2:3");
    assert_eq!(env.ls_json().len(), 2);
    let visits = fs::read_to_string(env.state().join("last")).unwrap();
    assert!(!visits.contains("s1\t"), "{visits}");

    // Already there, empty, or not a tab.
    env.dispatched();
    env.ok(&["tab", "send", "mail"]);
    assert!(env.dispatched().is_empty());
    env.set_active("s2:5");
    let err = env.fails(&["tab", "send", "home"]);
    assert!(err.contains("nothing is open on tab 5"), "{err}");
    let err = env.fails(&["tab", "send", "mail", "home"]);
    assert!(err.contains("isn't a tab"), "{err}");
}

#[test]
fn tab_send_moves_the_windows_of_homes_tabs() {
    let env = Env::new();
    env.add_window("1");
    env.add_window("3");
    env.add_window("3");
    env.add_terminal("s1:1", &env.dir("ren"));
    env.set_active("3");
    env.ok(&["tab", "rename", "home:3", "mail"]);

    env.ok(&["tab", "send", "next"]);
    assert_eq!(env.windows(), ["1", "s1:2", "s1:2", "s1:1"]);
    assert_eq!(env.active(), "s1:2");
    assert_eq!(
        env.tab_names(1),
        json!({ "s1:2": "mail" }),
        "it keeps its name"
    );

    env.ok(&["tab", "send", "home"]);
    assert_eq!(env.windows(), ["1", "2", "2", "s1:1"]);
    assert_eq!(env.active(), "2");
    assert_eq!(env.tab_names(0), json!({ "home:2": "mail" }));
    env.ok(&["go", "ren"]);
    assert_eq!(env.active(), "s1:1");
}

#[test]
fn tab_reorder_moves_a_tab_among_its_spaces() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_window("s1:2");
    env.add_window("s1:4");
    env.add_window("s2:1");
    env.set_active("s1:2");
    env.ok(&["tab", "rename", "s1:4", "server"]);
    let tabs = |env: &Env| env.tab_names(1);

    // Last to first: the others make room, on the numbers the space has.
    assert_eq!(
        env.ok(&["tab", "reorder", "s1:4", "1"]),
        "Moved tab 4 to 1\n"
    );
    assert_eq!(env.windows(), ["s1:2", "s1:4", "s1:1", "s2:1"]);
    assert_eq!(env.active(), "s1:4", "still on the same tab");
    assert_eq!(tabs(&env), json!({ "s1:1": "server" }));

    // And back, past the end being last.
    env.ok(&["tab", "reorder", "s1:1", "9"]);
    assert_eq!(env.windows(), ["s1:1", "s1:2", "s1:4", "s2:1"]);
    assert_eq!(env.active(), "s1:2");
    assert_eq!(tabs(&env), json!({ "s1:4": "server" }));

    // Going back to the space lands on the tab you left, wherever it is now.
    env.ok(&["go", "s2"]);
    env.ok(&["tab", "reorder", "s1:2", "1"]);
    assert_eq!(env.windows(), ["s1:2", "s1:1", "s1:4", "s2:1"]);
    assert_eq!(env.active(), "s2:1");
    env.ok(&["go", "s1"]);
    assert_eq!(env.active(), "s1:1");

    // Where it is already, nowhere, or no tab.
    env.ok(&["tab", "reorder", "s1:1", "1"]);
    assert_eq!(env.windows(), ["s1:2", "s1:1", "s1:4", "s2:1"]);
    let err = env.fails(&["tab", "reorder", "s1:1", "0"]);
    assert!(err.contains("positions start at 1"), "{err}");
    let err = env.fails(&["tab", "reorder", "s1:3", "1"]);
    assert!(err.contains("there's no tab s1:3"), "{err}");
}

#[test]
fn tab_reorder_renames_the_tabs_it_means_when_they_were_renamed_before() {
    let env = Env::new();
    env.renamed_space("s1", &env.dir("ren"));
    env.set_active("s1:2");
    // One window on the first tab, two on the second.
    let mut windows = env.windows();
    windows.sort();
    assert_eq!(windows, ["s1:1", "s1:2", "s1:2"]);

    env.ok(&["tab", "reorder", "s1:2", "1"]);
    let mut windows = env.windows();
    windows.sort();
    assert_eq!(windows, ["s1:1", "s1:1", "s1:2"], "the tabs swapped");
    assert_eq!(env.active(), "s1:1", "still on the same tab");
    assert_eq!(env.workspace_names(), ["s1:1", "s1:2"]);
}

#[test]
fn tab_reorder_moves_the_windows_of_homes_tabs() {
    let env = Env::new();
    env.add_window("1");
    env.add_window("2");
    env.add_window("2");
    env.add_window("13");
    // Tab 3 is empty, but it's the one you're on.
    env.set_active("3");
    env.ok(&["tab", "rename", "home:2", "mail"]);

    env.ok(&["tab", "reorder", "home:3", "1"]);
    assert_eq!(env.windows(), ["2", "3", "3", "13"]);
    assert_eq!(env.active(), "1");
    assert_eq!(env.tab_names(0), json!({ "home:3": "mail" }));

    env.ok(&["tab", "reorder", "home:3", "2"]);
    assert_eq!(env.windows(), ["3", "2", "2", "13"]);
    assert_eq!(env.active(), "1");
    assert_eq!(env.tab_names(0), json!({ "home:2": "mail" }));
}

#[test]
fn tab_send_needs_a_free_slot() {
    let env = Env::new();
    env.add_window("1");
    for n in 1..=10 {
        env.add_window(&format!("s1:{n}"));
    }
    env.set_active("1");
    let err = env.fails(&["tab", "send", "s1"]);
    assert!(err.contains("all 10 tabs of 's1' are open"), "{err}");
    assert_eq!(env.active(), "1");
}

#[test]
fn tab_close_closes_a_tabs_windows_and_moves_on_to_another() {
    let env = Env::new();
    env.add_window("1");
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_window("s1:2");
    env.add_window("s1:2");
    env.add_window("s1:4");
    env.set_active("s1:1");

    assert_eq!(
        env.ok(&["tab", "close", "s1:2"]),
        "Closed 2 windows on tab 2\n"
    );
    assert_eq!(env.windows(), ["1", "s1:1", "s1:4"]);
    assert_eq!(env.active(), "s1:1", "you stay where you are");

    // The one you're on: the next tab, or with none after it, the one before.
    assert_eq!(
        env.ok(&["tab", "close", "s1:1"]),
        "Closed 1 window on tab 1\n"
    );
    assert_eq!(env.active(), "s1:4");
    let err = env.fails(&["tab", "close", "s1:9"]);
    assert!(err.contains("there's no tab s1:9"), "{err}");

    // Empty, it's only left.
    env.set_active("s1:6");
    assert_eq!(
        env.ok(&["tab", "close", "s1:6"]),
        "No windows open on tab 6\n"
    );
    assert_eq!(env.active(), "s1:4");

    // Its only tab: the space ends.
    env.ok(&["tab", "close", "s1:4"]);
    assert_eq!(env.windows(), ["1"]);
    assert_eq!(env.active(), "1");
    assert_eq!(env.ls_json().len(), 1);
}

#[test]
fn tab_rename_and_close_take_the_tab_you_are_on() {
    let env = Env::new();
    env.add_window("1");
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_window("s1:2");
    env.set_active("s1:2");

    assert_eq!(
        env.ok(&["tab", "rename", "server"]),
        "Named tab 2 'server'\n"
    );
    assert_eq!(env.tab_names(1), json!({ "s1:2": "server" }));
    assert_eq!(env.ok(&["tab", "close"]), "Closed 1 window on tab 2\n");
    assert_eq!(env.windows(), ["1", "s1:1"]);
    assert_eq!(env.active(), "s1:1");
}

#[test]
fn spaces_and_tabs_have_their_own_commands() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_window("s1:2");
    env.set_active("s1:1");

    // A tab isn't a space, nor a space a tab.
    for args in [
        &["rename", "s1:2", "x"][..],
        &["reorder", "s1:2", "1"],
        &["close", "s1:2"],
    ] {
        let err = env.fails(args);
        assert!(err.contains("no space called 's1:2'"), "{err}");
    }
    for args in [
        &["tab", "rename", "s1", "x"][..],
        &["tab", "reorder", "s1", "1"],
        &["tab", "close", "s1"],
        // Nor is what a space is called with a number one.
        &["tab", "close", "ren:2"],
    ] {
        let err = env.fails(args);
        assert!(err.contains("isn't a tab"), "{err}");
    }
    assert_eq!(env.windows(), ["s1:1", "s1:2"]);

    // Even for a space called as a tab would be given.
    env.ok(&["rename", "s1", "s1:2"]);
    env.ok(&["tab", "rename", "s1:2", "server"]);
    assert_eq!(env.tab_names(1), json!({ "s1:2": "server" }));
    env.ok(&["close", "s1:2"]);
    assert!(env.windows().is_empty());
}

#[test]
fn tab_close_closes_a_home_tab_on_this_monitor() {
    let env = Env::new();
    env.add_window("1");
    env.add_window("3");
    env.add_window("13");
    env.set_active("3");

    assert_eq!(
        env.ok(&["tab", "close", "home:3"]),
        "Closed 1 window on tab 3\n"
    );
    assert_eq!(env.windows(), ["1", "13"], "not the other monitor's");
    assert_eq!(env.active(), "1");

    // Its last tab on this monitor: home doesn't end.
    assert_eq!(
        env.ok(&["tab", "close", "home:1"]),
        "Closed 1 window on tab 1\n"
    );
    assert_eq!(env.windows(), ["13"]);
    assert_eq!(env.active(), "1");
}

#[test]
fn terminal_opens_in_the_focused_terminals_folder_or_the_spaces() {
    let env = Env::new();
    let (ren, docs) = (env.dir("ren"), env.dir("ren/docs"));
    env.add_terminal("s1:1", &ren);
    env.set_active("s1:1");
    env.ok(&["ls"]);

    // Each opens on its own, so is waited for.
    let terminal = |count: usize| {
        env.ok(&["terminal"]);
        env.launched(count).pop().unwrap()
    };
    // An empty tab of the space: its folder.
    env.set_active("s1:2");
    assert_eq!(terminal(1), ren.display().to_string());
    // A tab whose terminal is elsewhere: that terminal's folder.
    env.add_terminal("s1:2", &docs);
    assert_eq!(terminal(2), docs.display().to_string());
    // Home with nothing focused: home.
    env.ok(&["go", "home"]);
    assert_eq!(terminal(3), env.dir("").display().to_string());
}

#[test]
fn ls_lists_spaces_with_counts() {
    let env = Env::new();
    let ren = env.dir("Projects/ren");
    env.add_window("1");
    env.add_terminal("s1:1", &ren);
    env.add_window("s1:2");
    env.add_window("special:scratchpad");
    env.set_active("s1:2");
    // One named by something else isn't a space.
    env.add_window("tax:1");
    let firefox = "org.mozilla.firefox".to_string();
    env.hypr.lock().unwrap().classes.insert(3, firefox);

    assert_eq!(
        env.ok(&["ls"]),
        "  home  home  2 windows\n\
         * s1    ren   2 windows    ~/Projects/ren\n"
    );
    let rows = env.ls_json();
    assert_eq!(
        rows[1]["tabs"],
        json!([
            { "id": "s1:1", "slot": 1, "monitor": "eDP-1", "label": "ren", "named": false, "shown": false, "windows": 1, "status": null },
            { "id": "s1:2", "slot": 2, "monitor": "eDP-1", "label": "Firefox", "named": false, "shown": true, "windows": 1, "status": null },
        ])
    );
    assert_eq!(rows[0]["folder"], Value::Null);
    assert_eq!(rows[1]["left_at"], Value::Null, "never left");
    assert_eq!(rows.len(), 2);

    env.ok(&["go", "home"]);
    assert!(env.ls_json()[1]["left_at"].as_u64().is_some_and(|t| t > 0));
}

#[test]
fn windows_of_one_process_are_told_apart() {
    let env = Env::new();
    let (a, b) = (env.dir("a"), env.dir("b"));
    let first = env.add_terminal("s1:1", &a);
    let second = env.add_terminal("s2:1", &b);
    let address = |window: &str| u32::from_str_radix(&window[2..], 16).unwrap();
    let (one, two) = (address(&first), address(&second));
    env.hypr.lock().unwrap().shared.insert(two, one);
    let shells = |pids: &[u32]| {
        let pids: Vec<String> = pids.iter().map(u32::to_string).collect();
        let file = env.cwds().join(format!("{}.shells", 1000 + one));
        fs::write(file, pids.join(" ")).unwrap();
    };
    shells(&[1000 + one, 1000 + two]);
    let folders = |rows: &[Value]| -> Vec<Value> {
        rows[1..]
            .iter()
            .map(|r| r["terminals"][0]["folder"].clone())
            .collect()
    };
    let (a, b) = (json!(a), json!(b));
    assert_eq!(folders(&env.ls_json()), [a.clone(), b.clone()]);
    assert_eq!(env.ls_json()[2]["label"], "b");

    // A new terminal opens where the focused one is.
    env.set_active("s1:1");
    env.ok(&["terminal"]);
    assert_eq!(json!(env.launched(1)[0]), a);

    // A shell more than there are windows, as a tab or a window on its way:
    // the watch knows which were theirs.
    let watch = Watch::start(&env);
    watch.settled(Duration::from_millis(400));
    std::os::unix::fs::symlink(env.dir("c"), env.cwds().join("5")).unwrap();
    shells(&[1000 + one, 1000 + two, 5]);
    env.cd(&second, &env.dir("d"));
    let state = watch.settled(Duration::from_millis(2500));
    assert_eq!(state["spaces"][1]["label"], "a");
    assert_eq!(state["spaces"][2]["label"], "d");
}

#[test]
fn ls_lists_each_spaces_terminals_by_what_they_run() {
    let env = Env::new();
    let ren = env.dir("Projects/ren");
    let notes = env.add_terminal("1", &env.dir(""));
    env.add_window("2");
    let agent = env.add_terminal("s1:1", &ren);
    let shell = env.add_terminal("s1:2", &ren.join("src"));
    env.run_program(&agent, Some("claude"));

    let rows = env.ls_json();
    assert_eq!(
        rows[0]["terminals"],
        json!([{ "address": notes, "tab": "home:1", "folder": env.dir("").display().to_string(), "program": null, "label": "~", "status": null }])
    );
    assert_eq!(
        rows[1]["terminals"],
        json!([
            { "address": agent, "tab": "s1:1", "folder": ren.display().to_string(), "program": "claude", "label": "claude", "status": null },
            { "address": shell, "tab": "s1:2", "folder": ren.join("src").display().to_string(), "program": null, "label": "src", "status": null },
        ])
    );
    // The space still goes by its first terminal's checkout.
    assert_eq!(rows[1]["label"], "ren");

    env.run_program(&agent, None);
    assert_eq!(env.ls_json()[1]["terminals"][0]["label"], "ren");
}

#[test]
fn shell_is_your_own_after_the_conversation() {
    let env = Env::new();
    let shell = env.root.path().join("myshell");
    fs::write(&shell, "#!/bin/sh\necho \"$0 $*\"; env | grep SPACES_R\n").unwrap();
    fs::set_permissions(&shell, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let run = |resume: &str| {
        let out = env
            .command(&["shell"])
            .env("SHELL", &shell)
            .env("SPACES_RESUME", resume)
            .env("SPACES_RESTORE", "1")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let path = shell.display();
    let id = "be6fb713-2beb-4233-86c2-266ee407ec56";
    assert_eq!(
        run(&format!("claude {id}")),
        format!("{path} -i -c claude --resume {id} || claude; exec '{path}'")
    );
    // Nothing that can be picked up: just the shell.
    assert_eq!(run("claude x; rm -rf ~"), path.to_string());
}

#[test]
fn ls_shows_what_an_agent_is_doing() {
    let env = Env::new();
    let window = env.add_terminal("s1:1", &env.dir("app"));
    env.set_active("1");
    for (word, shown) in [
        ("idle", Value::Null),
        ("working", json!("working")),
        ("waiting", json!("waiting")),
        ("", Value::Null),
    ] {
        env.agent_is(&window, word);
        let rows = env.ls_json();
        assert_eq!(rows[1]["status"], shown, "{word}");
        assert_eq!(rows[1]["terminals"][0]["status"], shown, "{word}");
    }
}

#[test]
fn watch_shows_what_an_agent_is_doing_and_clears_done_once_seen() {
    use std::time::Duration;
    let env = Env::new();
    let app = env.dir("app");
    let agent = env.add_terminal("s1:1", &app);
    let other = env.add_terminal("s1:2", &app);
    env.add_window("1");
    env.set_active("s1:1");
    env.agent_is(&agent, "idle");
    env.agent_is(&other, "working");

    let watch = Watch::start(&env);
    let tabs = |wanted: [Value; 2]| {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while watch.next_tabs(Duration::from_secs(3)) != wanted {
            assert!(std::time::Instant::now() < deadline, "never {wanted:?}");
        }
    };
    // It hasn't started: that's not done.
    tabs([Value::Null, json!("working")]);
    watch.joined(&env);
    // Its tabs show, and you are in neither window.
    env.event("activewindowv2>>");

    env.agent_is(&agent, "waiting");
    tabs([json!("waiting"), json!("working")]);
    // You answered.
    env.agent_is(&agent, "working");
    tabs([json!("working"), json!("working")]);
    env.agent_is(&agent, "idle");
    tabs([json!("done"), json!("working")]);
    env.agent_is(&other, "idle");
    tabs([json!("done"), json!("done")]);

    // Focusing the first clears its `done`, and leaves the other's alone.
    env.event(&format!(
        "activewindowv2>>{}",
        agent.trim_start_matches("0x")
    ));
    tabs([Value::Null, json!("done")]);

    // Its next turn ends while you're elsewhere: `done` again.
    env.event("activewindowv2>>");
    env.agent_is(&agent, "working");
    tabs([json!("working"), json!("done")]);
    env.agent_is(&agent, "idle");
    tabs([json!("done"), json!("done")]);

    // It exits, or is killed.
    env.agent_exits(&agent);
    tabs([Value::Null, json!("done")]);
}

#[test]
fn watch_rings_when_an_agent_needs_you_or_finishes_unseen() {
    use std::time::Duration;
    let env = Env::new();
    let bin = env.root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let played = env.root.path().join("played");
    fs::write(
        bin.join("pw-play"),
        format!("#!/bin/sh\necho \"$1\" >> '{}'\n", played.display()),
    )
    .unwrap();
    fs::set_permissions(
        bin.join("pw-play"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    // Its own sound for done; Herdr's, built in, for waiting.
    env.write_config("[sounds]\nenabled = true\ndone = \"/s/done.oga\"\n");
    let heard = || {
        thread::sleep(Duration::from_millis(300));
        fs::read_to_string(&played)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect::<Vec<_>>()
    };

    let agent = env.add_terminal("s1:1", &env.dir("app"));
    // Waiting since before `watch` started: no sound for that.
    env.agent_is(&agent, "waiting");
    env.add_window("1");
    env.set_active("s1:1");

    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let mut command = env.command(&["watch", "--sounds"]);
    command.env("PATH", path);
    let watch = Watch::run(command);
    assert_eq!(
        watch.next_rows(Duration::from_secs(3))[1]["status"],
        "waiting"
    );
    watch.joined(&env);
    env.event("activewindowv2>>");
    assert!(heard().is_empty());

    let set = |word: &str| {
        env.agent_is(&agent, word);
        let _ = watch.next_rows(Duration::from_secs(1));
    };
    set("working");
    set("waiting");
    let request = env.root.path().join("run/omarchy-spaces/request.mp3");
    assert_eq!(heard(), [request.display().to_string()]);
    assert_eq!(
        fs::read(&request).unwrap(),
        fs::read("assets/sounds/request.mp3").unwrap()
    );

    // Asking again in the same turn rings again.
    set("working");
    set("waiting");
    assert_eq!(heard().len(), 2);

    set("working");
    set("idle");
    assert_eq!(heard().len(), 3);
    assert_eq!(heard()[2], "/s/done.oga");

    // Finishing while you're looking at the window: silent.
    env.event(&format!(
        "activewindowv2>>{}",
        agent.trim_start_matches("0x")
    ));
    let _ = watch.next_rows(Duration::from_secs(1));
    set("working");
    set("idle");
    assert_eq!(heard().len(), 3);

    // Unless you turn them on, there are none.
    env.write_config("[sounds]\ndone = \"/s/done.oga\"\n");
    thread::sleep(Duration::from_millis(300));
    set("working");
    set("waiting");
    assert_eq!(heard().len(), 3);
}

#[test]
fn ls_without_hyprland_lists_what_it_remembers() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.ok(&["ls"]);
    let out = env
        .command(&["ls"])
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "  home  home  -\n  s1    ren   -            ~/ren\n"
    );
}

#[test]
fn watch_saves_what_browsers_hold_only_if_its_to_be_opened_again() {
    for (config, kept) in [
        ("", true),
        ("[restore]\nbrowsers = false\n", false),
        // It may be about to say not to: what was known stands.
        ("[restore]\nbrowsers = ", true),
    ] {
        let env = Env::new();
        env.write_config(config);
        env.add_terminal("s1:1", &env.dir("app"));
        let window = env.add_window("s1:1");
        let address = u32::from_str_radix(window.trim_start_matches("0x"), 16).unwrap();
        let mut hypr = env.hypr.lock().unwrap();
        hypr.classes.insert(address, "chromium".into());
        drop(hypr);
        env.set_active("s1:1");
        // What a watch before this one saved.
        fs::create_dir_all(env.state()).unwrap();
        let session = env.state().join("session-restored.json");
        let saved = json!({ "instance": "test", "spaces": [{ "windows": [
            { "tab": 1, "address": window, "browser":
                { "name": "chromium", "profile": "Default", "urls": ["https://a/"] } },
        ] }] });
        fs::write(&session, saved.to_string()).unwrap();

        let watch = Watch::start(&env);
        watch.next_rows(Duration::from_secs(3));
        let file = env.state().join("session.json");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !file.exists() {
            assert!(Instant::now() < deadline, "never saved");
            thread::sleep(Duration::from_millis(20));
        }
        let text = fs::read_to_string(&file).unwrap();
        assert_eq!(text.contains("https://a/"), kept, "{config}: {text}");
    }
}

/// A running `space watch`, its lines read on a thread.
struct Watch {
    child: std::process::Child,
    lines: std::sync::mpsc::Receiver<String>,
}

impl Watch {
    fn start(env: &Env) -> Self {
        Self::run(env.command(&["watch"]))
    }

    fn run(mut command: Command) -> Self {
        let mut child = command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = std::sync::mpsc::channel();
        thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Self { child, lines }
    }

    /// The labels in the next list it prints within `within`.
    #[track_caller]
    fn next(&self, within: std::time::Duration) -> Vec<String> {
        self.next_rows(within)
            .iter()
            .map(|r| r["label"].as_str().unwrap().to_string())
            .collect()
    }

    /// The spaces in the next line it prints within `within`.
    #[track_caller]
    fn next_rows(&self, within: std::time::Duration) -> Vec<Value> {
        self.next_state(within)["spaces"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// What the agents in the first monitor's tabs are doing, in the next line
    /// it prints within `within`.
    #[track_caller]
    fn next_tabs(&self, within: std::time::Duration) -> Vec<Value> {
        let state = self.next_state(within);
        let tabs = state["monitors"][0]["tabs"].as_array().unwrap().iter();
        tabs.map(|tab| tab["status"].clone()).collect()
    }

    /// The next line it prints within `within`.
    #[track_caller]
    fn next_state(&self, within: std::time::Duration) -> Value {
        let line = self
            .lines
            .recv_timeout(within)
            .expect("space watch printed nothing new in time");
        serde_json::from_str(&line).unwrap()
    }

    /// The last line it prints before saying nothing for `quiet`.
    #[track_caller]
    fn settled(&self, quiet: std::time::Duration) -> Value {
        let mut last = None;
        while let Ok(line) = self.lines.recv_timeout(quiet) {
            last = Some(line);
        }
        serde_json::from_str(&last.expect("space watch printed nothing")).unwrap()
    }

    /// Waits for its listener to join, so events reach it.
    fn joined(&self, env: &Env) {
        for _ in 0..100 {
            if !env.listeners.lock().unwrap().is_empty() {
                return;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("space watch never listened to events");
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn watch_prints_the_spaces_again_as_they_change() {
    use std::time::Duration;
    let env = Env::new();
    let (notes, japanese, mail, inbox) = (
        env.dir("notes"),
        env.dir("japanese"),
        env.dir("mail"),
        env.dir("inbox"),
    );
    let watch = Watch::start(&env);
    assert_eq!(watch.next(Duration::from_secs(2)), ["home"]);
    watch.joined(&env);

    // A window opens: Hyprland says so.
    let first = env.add_terminal("s1:1", &notes);
    env.event("openwindow>>1,s1:1,kitty,zsh");
    assert_eq!(watch.next(Duration::from_secs(1)), ["home", "notes"]);

    // In the space you're in, a `cd` shows within a fraction of a second,
    // though nothing says so.
    env.set_active("s1:1");
    env.event("workspacev2>>-1,s1:1");
    let _ = watch.next(Duration::from_secs(1));
    env.cd(&first, &japanese);
    assert_eq!(watch.next(Duration::from_millis(800)), ["home", "japanese"]);

    // Elsewhere, a terminal's new title is enough, well before the slow
    // round would read it.
    let other = env.add_terminal("s2:1", &mail);
    env.event("openwindow>>2,s2:1,kitty,zsh");
    assert_eq!(
        watch.next(Duration::from_secs(1)),
        ["home", "japanese", "mail"]
    );
    env.cd(&other, &inbox);
    env.event("windowtitlev2>>2,~/inbox");
    assert_eq!(
        watch.next(Duration::from_millis(1500)),
        ["home", "japanese", "inbox"]
    );

    // A name given by another `space` command.
    env.ok(&["rename", "s2", "Inbox"]);
    assert_eq!(
        watch.next(Duration::from_secs(1)),
        ["home", "japanese", "Inbox"]
    );

    // Nothing changed: nothing printed.
    assert!(
        watch
            .lines
            .recv_timeout(Duration::from_millis(500))
            .is_err()
    );
}

/// The space each monitor shows, and the one that's active, in a line of
/// `space watch`.
fn places(state: &Value) -> (Vec<(String, String)>, Vec<String>) {
    let text = |v: &Value| v.as_str().unwrap().to_string();
    let monitors = state["monitors"].as_array().unwrap().iter();
    let spaces = state["spaces"].as_array().unwrap().iter();
    (
        monitors
            .map(|m| (text(&m["name"]), text(&m["space"])))
            .collect(),
        spaces
            .filter(|row| row["active"] == true)
            .map(|row| text(&row["id"]))
            .collect(),
    )
}

#[test]
fn watch_shows_where_you_are_after_switching_quickly() {
    let env = Env::new();
    env.add_window("s1:1");
    env.add_window("s2:1");
    env.add_window("s3:1");
    env.set_active("s1:1");
    let watch = Watch::start(&env);
    watch.joined(&env);
    let _ = watch.settled(Duration::from_millis(400));

    for round in 0..10 {
        let to = ["s2:1", "s3:1", "s1:1"];
        for name in to {
            env.set_active(name);
            env.event(&format!("workspacev2>>-1,{name}"));
            thread::sleep(Duration::from_millis(round * 5));
        }
        // Each line is right in itself, and the last one is where you are.
        let mut last = None;
        while let Ok(line) = watch.lines.recv_timeout(Duration::from_millis(300)) {
            let state: Value = serde_json::from_str(&line).unwrap();
            let (monitors, active) = places(&state);
            assert_eq!(monitors, [("eDP-1".to_string(), active[0].clone())]);
            assert_eq!(active.len(), 1);
            last = Some(active[0].clone());
        }
        // Nothing printed is nothing changed: back before it looked.
        if let Some(last) = last {
            assert_eq!(last, "s1");
        }
    }
}

#[test]
fn watch_shows_a_switch_and_the_space_it_left_at_once() {
    let env = Env::new();
    env.add_window("s1:1");
    env.add_window("s2:1");
    env.set_active("s1:1");
    let watch = Watch::start(&env);
    watch.joined(&env);
    let _ = watch.settled(Duration::from_millis(400));

    env.ok(&["go", "s2"]);
    env.event("workspacev2>>-1,s2:1");
    let state = watch.next_state(Duration::from_secs(1));
    assert_eq!(places(&state).1, ["s2"]);
    let left = |i: usize| state["spaces"][i]["left_at"].as_u64().unwrap();
    assert!(
        left(1) > 0,
        "the space you left, for the search to start on"
    );
    assert!(left(1) > left(2));
}

#[test]
fn watch_keeps_what_it_knew_when_hyprland_cant_be_read() {
    let env = Env::new();
    env.add_window("s1:1");
    env.add_window("s2:1");
    env.set_active("s1:1");
    let mut watch = Watch::start(&env);
    watch.joined(&env);
    let _ = watch.settled(Duration::from_millis(400));

    env.hypr.lock().unwrap().garbled = true;
    env.set_active("s2:1");
    env.event("workspacev2>>-1,s2:1");
    let quiet = watch.lines.recv_timeout(Duration::from_millis(400));
    assert!(quiet.is_err(), "{quiet:?}");
    assert!(watch.child.try_wait().unwrap().is_none(), "and goes on");

    // The slow round catches up without another event.
    env.hypr.lock().unwrap().garbled = false;
    let state = watch.next_state(Duration::from_secs(4));
    assert_eq!(places(&state).1, ["s2"]);
}

#[test]
fn watch_shows_each_monitors_tabs() {
    let env = Env::new();
    let (ren, notes) = (env.dir("ren"), env.dir("notes"));
    env.add_terminal("s1:1", &ren);
    let agent = env.add_terminal("s1:3", &notes);
    env.add_window("s1:12");
    env.add_window("s2:1");
    env.add_window("2");
    env.set_active("s1:3");
    env.plug_in("s1:12");
    let class = |address: u32, class: &str| {
        let mut hypr = env.hypr.lock().unwrap();
        hypr.classes.insert(address, class.into());
    };
    class(3, "org.mozilla.firefox");
    env.ok(&["tab", "rename", "s1:1", "server"]);

    let tabs = |state: &Value, monitor: usize| -> Vec<Value> {
        let tabs = state["monitors"][monitor]["tabs"].as_array().unwrap();
        tabs.iter()
            .map(|t| {
                json!([
                    t["id"],
                    t["slot"],
                    t["label"],
                    t["named"],
                    t["windows"],
                    t["active"]
                ])
            })
            .collect()
    };
    let watch = Watch::start(&env);
    let state = watch.settled(Duration::from_millis(400));
    assert_eq!(state["version"], 2);
    assert_eq!(state["focused"], "eDP-1");
    assert_eq!(state["monitors"][0]["space"], "s1");
    assert_eq!(state["monitors"][0]["tab"], "s1:3");
    assert_eq!(state["monitors"][0]["tiled"], true);
    assert_eq!(
        tabs(&state, 0),
        [
            json!(["s1:1", 1, "server", true, 1, false]),
            json!(["s1:3", 3, "notes", false, 1, true]),
        ]
    );
    assert_eq!(state["monitors"][1]["name"], "DP-1");
    assert_eq!(
        tabs(&state, 1),
        [json!(["s1:12", 2, "Firefox", false, 1, true])],
        "another monitor's are its own"
    );

    env.agent_is(&agent, "waiting");
    let state = watch.settled(Duration::from_millis(600));
    let shown: Vec<_> = state["monitors"][0]["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .collect();
    assert_eq!(shown[0]["status"], Value::Null);
    assert_eq!(shown[1]["status"], "waiting");
    assert_eq!(state["spaces"][1]["status"], "waiting");

    // A new tab is empty until something opens on it.
    watch.joined(&env);
    env.set_active("s1:4");
    env.event("workspacev2>>-1,s1:4");
    let state = watch.settled(Duration::from_millis(400));
    assert_eq!(tabs(&state, 0)[2], json!(["s1:4", 4, "", false, 0, true]));
    assert_eq!(state["monitors"][0]["tiled"], false);

    // `home`'s are numbers until named.
    env.set_active("2");
    env.event("workspacev2>>2,2");
    let state = watch.settled(Duration::from_millis(400));
    assert_eq!(state["monitors"][0]["space"], "home");
    assert_eq!(tabs(&state, 0)[1], json!(["home:2", 2, "", false, 1, true]));

    // Unplugged, its tabs come after the ones left.
    env.set_active("s1:3");
    env.unplug();
    env.event("monitorremoved>>DP-1");
    let state = watch.settled(Duration::from_millis(400));
    assert_eq!(state["monitors"].as_array().unwrap().len(), 1);
    let keys: Vec<Value> = tabs(&state, 0)
        .iter()
        .map(|t| json!([t[0], t[1]]))
        .filter(|t| t[0] != "s1:4")
        .collect();
    assert_eq!(
        keys,
        [json!(["s1:1", 1]), json!(["s1:12", 2]), json!(["s1:3", 3])]
    );
}

#[test]
fn stepping_twice_at_once_goes_two_spaces_on() {
    let env = Env::new();
    for name in ["s1:1", "s2:1", "s3:1", "s4:1"] {
        env.add_window(name);
    }
    env.set_active("s1:1");
    env.hypr.lock().unwrap().slow = Duration::from_millis(30);
    let mut steps: Vec<_> = (0..3)
        .map(|_| env.command(&["next"]).spawn().unwrap())
        .collect();
    for step in &mut steps {
        assert!(step.wait().unwrap().success());
    }
    assert_eq!(env.active(), "s4:1");
}

#[test]
fn watch_ends_with_hyprland() {
    let env = Env::new();
    let mut watch = Watch::start(&env);
    let _ = watch.next(std::time::Duration::from_secs(2));
    watch.joined(&env);
    env.end_events();
    for _ in 0..100 {
        if let Some(status) = watch.child.try_wait().unwrap() {
            assert!(
                !status.success(),
                "it ends with an error, to be started again"
            );
            return;
        }
        thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("space watch kept running without Hyprland");
}

#[test]
fn new_without_exec_rules_goes_there_and_then_opens_the_terminal() {
    let env = Env::new();
    env.hypr.lock().unwrap().exec_rules = false;
    env.ok(&["new"]);
    assert_eq!(env.active(), "s1:1");
    assert_eq!(env.launched(1), [env.dir("").display().to_string()]);
}

#[test]
fn an_emptied_tab_gives_way_to_the_next_one_then_the_one_before() {
    let env = Env::new();
    env.add_window("s1:1");
    env.add_window("s1:3");
    let closing = env.add_window("s1:2");
    env.set_active("s1:2");
    env.close_window(&closing);

    env.ok(&["emptied", "s1:2"]);
    assert_eq!(env.active(), "s1:3", "the next tab with windows");

    let closing = env.add_window("s1:4");
    env.set_active("s1:4");
    env.close_window(&closing);
    env.ok(&["emptied", "s1:4"]);
    assert_eq!(env.active(), "s1:3", "none after it: the one before");
}

#[test]
fn closing_a_spaces_last_window_ends_it() {
    let env = Env::new();
    env.add_window("1");
    env.add_terminal("s2:1", &env.dir("mail"));
    let last = env.add_terminal("s1:2", &env.dir("notes"));
    env.ok(&["go", "mail"]);
    env.ok(&["go", "notes"]);
    assert_eq!(env.active(), "s1:2");

    env.close_window(&last);
    env.ok(&["emptied", "s1:2"]);
    assert_eq!(env.active(), "s2:1", "back to the space you were in before");
    let labels: Vec<String> = env
        .ls_json()
        .iter()
        .map(|r| r["label"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(labels, ["home", "mail"]);
    let visits = fs::read_to_string(env.state().join("last")).unwrap();
    assert!(!visits.contains("s1\t"), "{visits}");

    // The last space of all: home.
    let only = env.newest_window();
    assert_eq!(env.windows().last().unwrap(), "s2:1");
    env.close_window(&only);
    env.ok(&["emptied", "s2:1"]);
    assert_eq!(env.active(), "1");
}

#[test]
fn emptied_does_nothing_once_things_have_moved_on() {
    let env = Env::new();
    env.add_window("s1:1");
    env.set_active("s1:2");
    // You already left it, or it isn't empty, or it's one of home's.
    env.ok(&["emptied", "s1:3"]);
    env.ok(&["emptied", "s1:1"]);
    env.set_active("s1:1");
    env.ok(&["emptied", "s1:1"]);
    env.set_active("2");
    env.ok(&["emptied", "2"]);
    assert!(env.dispatched().is_empty());
    assert_eq!(env.active(), "2");
}

#[test]
fn each_monitor_has_its_own_tabs() {
    let env = Env::new();
    env.add_window("s1:1");
    env.add_window("s1:2");
    env.add_window("s1:11");
    env.set_active("s1:2");
    env.plug_in("s1:11");

    // Next and previous keep to the monitor's own.
    env.ok(&["tab", "next"]);
    assert_eq!(env.active(), "s1:1");

    // On the second monitor, its tab 2 is s1:12, and a new one comes after.
    env.hypr.lock().unwrap().go("s1:11".into());
    env.ok(&["tab", "go", "2"]);
    assert_eq!(env.active(), "s1:12");
    assert_eq!(env.elsewhere(), "s1:1");
    env.ok(&["tab", "go", "1"]);
    env.ok(&["tab", "new"]);
    assert_eq!(env.active(), "s1:12");
    env.ok(&["window", "tab", "3"]);
    assert_eq!(env.windows().last().unwrap(), "s1:13");

    // A tab by its workspace is the one, wherever you are.
    env.ok(&["tab", "go", "s1:2"]);
    assert_eq!(env.active(), "s1:2");
    env.ok(&["tab", "rename", "s1:13", "logs"]);
    assert_eq!(env.tab_names(1), json!({ "s1:13": "logs" }));
    let err = env.fails(&["tab", "go", "s1:9"]);
    assert!(err.contains("there's no tab s1:9"), "{err}");
}

#[test]
fn a_space_opens_where_you_are_on_the_monitor_you_are_on() {
    let env = Env::new();
    env.add_window("s1:2");
    env.add_window("s1:12");
    env.add_window("s1:13");
    env.add_window("s2:1");
    env.set_active("s1:2");
    env.plug_in("s1:12");

    // Left from the second monitor, gone back to from the first: where you
    // were on the first.
    env.hypr.lock().unwrap().go("s1:12".into());
    env.ok(&["go", "s2"]);
    assert_eq!(env.active(), "s2:11", "nothing there yet, on this monitor");
    env.hypr.lock().unwrap().go("s2:1".into());
    env.ok(&["go", "s1"]);
    assert_eq!(env.active(), "s1:2");

    // Back from the second, the tab it was left on.
    env.hypr.lock().unwrap().go("s2:11".into());
    env.ok(&["go", "s1"]);
    assert!(env.active().starts_with("s1:1"), "{}", env.active());
    assert_ne!(env.active(), "s1:1");
}

#[test]
fn a_new_space_and_a_new_tab_open_on_this_monitor() {
    let env = Env::new();
    env.add_window("1");
    env.add_window("11");
    env.set_active("11");
    env.hypr.lock().unwrap().elsewhere = Some("1".into());
    env.plug_in("1");
    env.ok(&["new", env.dir("ren").to_str().unwrap()]);
    assert_eq!(env.active(), "s1:11");

    // An empty tab, as the other monitor has, takes the new one's terminal.
    env.hypr.lock().unwrap().go("s1:1".into());
    env.ok(&["tab", "new"]);
    assert_eq!(env.active(), "s1:1");
    assert_eq!(env.windows().last().unwrap(), "s1:1");
    env.ok(&["tab", "new"]);
    assert_eq!(env.active(), "s1:2");
}

#[test]
fn a_space_ends_only_once_no_monitor_has_any_of_its_windows() {
    let env = Env::new();
    env.add_window("1");
    let here = env.add_window("s1:1");
    let there = env.add_window("s1:12");
    let beside = env.add_window("s1:11");
    env.set_active("s1:1");
    env.plug_in("s1:12");

    // Its last one on this monitor: you stay, as the other has more.
    env.close_window(&here);
    env.ok(&["emptied", "s1:1"]);
    assert_eq!(env.active(), "s1:1");

    // The other monitor goes on to its next tab, without you.
    env.close_window(&there);
    env.ok(&["emptied", "s1:12"]);
    assert_eq!(env.active(), "s1:1");
    assert_eq!(env.elsewhere(), "s1:11");

    // The last of all ends it, on whichever monitor.
    env.close_window(&beside);
    env.ok(&["emptied", "s1:11"]);
    assert_eq!(env.active(), "1");
}

#[test]
fn a_tab_sent_to_another_space_stays_on_its_monitor() {
    let env = Env::new();
    env.add_window("s1:1");
    env.add_window("s1:12");
    env.add_window("s2:1");
    env.set_active("s1:1");
    env.plug_in("s1:12");

    assert_eq!(
        env.ok(&["tab", "send", "s1:12", "s2"]),
        "Moved tab 2 to 's2'\n"
    );
    assert_eq!(env.active(), "s2:11", "the first of that monitor's");
    assert_eq!(env.windows(), ["s1:1", "s2:11", "s2:1"]);
    let synced = env.hypr.lock().unwrap().evaluated.clone();
    assert!(synced.last().unwrap().contains("spaces.internal.sync()"));

    // Reordered among its own monitor's.
    env.add_window("s2:12");
    env.ok(&["tab", "reorder", "s2:12", "1"]);
    assert_eq!(env.windows(), ["s1:1", "s2:12", "s2:1", "s2:11"]);
}

#[test]
fn an_unplugged_monitors_tabs_are_numbered_among_the_ones_left() {
    let env = Env::new();
    env.add_window("s1:1");
    env.add_window("s1:2");
    env.add_window("s1:11");
    env.add_window("s1:11");
    env.set_active("s1:2");
    env.unplug();
    env.ok(&["tab", "rename", "s1:11", "test"]);
    assert_eq!(
        env.tab_slots(),
        json!([["s1:1", 1], ["s1:2", 2], ["s1:11", 3]])
    );

    // By its number, or stepping to it as they show.
    env.ok(&["tab", "go", "3"]);
    assert_eq!(env.active(), "s1:11");
    env.ok(&["tab", "next"]);
    assert_eq!(env.active(), "s1:1");
    env.ok(&["tab", "prev"]);
    assert_eq!(env.active(), "s1:11");

    // A window to it, and from it; a new tab leaves it its number.
    env.ok(&["window", "tab", "2"]);
    assert_eq!(env.active(), "s1:2");
    env.ok(&["window", "tab", "3"]);
    assert_eq!(env.active(), "s1:11");
    env.ok(&["tab", "new"]);
    assert_eq!(env.active(), "s1:4");
    assert_eq!(env.tab_slots()[2], json!(["s1:11", 3]));

    // Where you left the space is still there.
    env.ok(&["tab", "go", "3"]);
    env.ok(&["go", "home"]);
    env.ok(&["go", "s1"]);
    assert_eq!(env.active(), "s1:11");

    // Back, it's the other monitor's tab 1 again, with the name it had.
    env.plug_in("s1:11");
    assert!(
        env.tab_slots()
            .as_array()
            .unwrap()
            .contains(&json!(["s1:11", 1]))
    );
    assert_eq!(env.tab_names(1), json!({ "s1:11": "test" }));
}

#[test]
fn an_unplugged_monitors_tab_put_somewhere_by_hand_stays() {
    let env = Env::new();
    env.add_window("s1:1");
    env.add_window("s1:2");
    env.add_window("s1:11");
    env.set_active("s1:2");
    env.unplug();
    env.ok(&["tab", "rename", "s1:11", "test"]);

    assert_eq!(
        env.ok(&["tab", "reorder", "s1:11", "1"]),
        "Moved tab 3 to 1\n"
    );
    assert_eq!(env.windows(), ["s1:2", "s1:3", "s1:1"]);
    assert_eq!(env.active(), "s1:3", "with you on the tab you were on");
    assert_eq!(
        env.tab_slots(),
        json!([["s1:1", 1], ["s1:2", 2], ["s1:3", 3]])
    );
    assert_eq!(env.tab_names(1), json!({ "s1:1": "test" }));
}

#[test]
fn the_plugin_has_the_same_version_as_space() {
    let manifest: Value =
        serde_json::from_str(include_str!("../manifest.json")).expect("manifest.json");
    assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in manifest["entryPoints"].as_object().unwrap().values() {
        assert!(root.join(file.as_str().unwrap()).is_file(), "{file}");
    }
}

#[test]
fn install_takes_the_release_of_the_plugins_version() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let version = env!("CARGO_PKG_VERSION");
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let installed = home.join(".local/bin/space");
    let release = tmp.path().join(format!("v{version}"));
    let arch = std::env::consts::ARCH;
    let install = || {
        Command::new(root.join("install.sh"))
            .env("HOME", &home)
            .env_remove("XDG_STATE_HOME")
            .env(
                "SPACES_RELEASES",
                format!("file://{}", tmp.path().display()),
            )
            .output()
            .unwrap()
    };

    let missing = install();
    assert!(!missing.status.success());
    let said = String::from_utf8_lossy(&missing.stderr).into_owned();
    assert!(said.contains("is version"), "{said}");
    assert!(!installed.exists());

    let package = Command::new(root.join("scripts/package.sh"))
        .arg(env!("CARGO_BIN_EXE_space"))
        .arg(arch)
        .arg(&release)
        .output()
        .unwrap();
    assert!(package.status.success(), "{package:?}");
    for again in [false, true] {
        let done = install();
        assert!(done.status.success(), "{again}: {done:?}");
        let space = Command::new(&installed).arg("--version").output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&space.stdout),
            format!("space {version}\n")
        );
    }

    // Not what was released: the one there stays.
    let archive = release.join(format!("space-{version}-{arch}-linux.tar.gz"));
    fs::write(&archive, "something else").unwrap();
    let before = fs::read(&installed).unwrap();
    assert!(!install().status.success());
    assert!(fs::read(&installed).unwrap() == before);

    // Nor does it replace another program of that name.
    fs::write(&installed, "#!/bin/sh\n").unwrap();
    let said = String::from_utf8_lossy(&install().stderr).into_owned();
    assert!(said.contains("is another program"), "{said}");
}

#[test]
fn install_update_replaces_only_its_own_outdated_binary() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let version = env!("CARGO_PKG_VERSION");
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let installed = home.join(".local/bin/space");
    let plugin = tmp.path().join("plugin");
    let arch = std::env::consts::ARCH;
    fs::create_dir_all(&plugin).unwrap();
    fs::copy(root.join("install.sh"), plugin.join("install.sh")).unwrap();
    let set_manifest = |ver: &str| {
        let text = format!("{{\n  \"version\": \"{ver}\",\n  \"id\": \"com.t4t5.spaces\"\n}}\n");
        fs::write(plugin.join("manifest.json"), text).unwrap();
    };
    let pack = |ver: &str, bin: &Path| {
        let release = tmp.path().join(format!("v{ver}"));
        let stage = tmp.path().join(format!("stage-{ver}"));
        fs::create_dir_all(&stage).unwrap();
        fs::copy(root.join("LICENSE"), stage.join("LICENSE")).unwrap();
        fs::create_dir_all(stage.join("assets/sounds")).unwrap();
        fs::copy(
            root.join("assets/sounds/LICENSE"),
            stage.join("assets/sounds/LICENSE"),
        )
        .unwrap();
        fs::create_dir_all(stage.join("scripts")).unwrap();
        fs::copy(
            root.join("scripts/package.sh"),
            stage.join("scripts/package.sh"),
        )
        .unwrap();
        fs::write(
            stage.join("manifest.json"),
            format!("{{\n  \"version\": \"{ver}\",\n  \"id\": \"com.t4t5.spaces\"\n}}\n"),
        )
        .unwrap();
        let out = Command::new("bash")
            .arg(stage.join("scripts/package.sh"))
            .arg(bin)
            .arg(arch)
            .arg(&release)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    };
    let run_install = |args: &[&str]| {
        Command::new("bash")
            .arg(plugin.join("install.sh"))
            .args(args)
            .env("HOME", &home)
            .env_remove("XDG_STATE_HOME")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    home.join(".local/bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env(
                "SPACES_RELEASES",
                format!("file://{}", tmp.path().display()),
            )
            .output()
            .unwrap()
    };

    // Unmanaged (never installed by install.sh): `--update` does nothing.
    set_manifest(version);
    let out = run_install(&["--update"]);
    assert!(out.status.success(), "{out:?}");
    assert!(!installed.exists());

    // First install records the marker; `--update` on the same version is a no-op.
    let old_bin = tmp.path().join("space-old");
    fs::write(
        &old_bin,
        "#!/bin/sh\n# omarchy-spaces\necho 'space 0.0.1'\n",
    )
    .unwrap();
    pack("0.0.1", &old_bin);
    set_manifest("0.0.1");
    assert!(run_install(&[]).status.success());
    let noop = run_install(&["--update"]);
    assert!(noop.status.success());
    assert!(
        noop.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&noop.stdout)
    );

    // Plugin bumps version: `--update` fetches and installs the matching release.
    pack(version, Path::new(env!("CARGO_BIN_EXE_space")));
    set_manifest(version);
    let updated = run_install(&["--update"]);
    assert!(updated.status.success(), "{updated:?}");
    assert!(
        String::from_utf8_lossy(&updated.stdout).contains(&format!("Installed space {version}"))
    );
    let space = Command::new(&installed).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&space.stdout),
        format!("space {version}\n")
    );

    // User replaces `~/.local/bin/space` with a local/source build: `--update` leaves it alone.
    fs::write(
        &installed,
        "#!/bin/sh\n# omarchy-spaces custom build\necho 'space 0.0.9'\n",
    )
    .unwrap();
    set_manifest("0.0.1");
    let skipped = run_install(&["--update"]);
    assert!(skipped.status.success());
    assert!(skipped.stdout.is_empty());
    let kept = fs::read_to_string(&installed).unwrap();
    assert!(kept.contains("custom build"), "{kept}");
}

#[test]
fn install_setup_replaces_omarchys_menu_and_workspaces_once() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let version = env!("CARGO_PKG_VERSION");
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let installed = home.join(".local/bin/space");
    let link = home.join(".config/hypr/omarchy-spaces.lua");
    let claude_skill = home.join(".claude/skills/omarchy-spaces");
    let gemini_skill = home.join(".gemini/config/skills/omarchy-spaces");
    let agents_skill = home.join(".agents/skills/omarchy-spaces");
    let replaced = home.join(".local/state/omarchy-spaces/bar");
    let plugin = tmp.path().join("plugin");
    fs::create_dir_all(plugin.join("hypr")).unwrap();
    fs::create_dir_all(plugin.join("skills/omarchy-spaces")).unwrap();
    for file in [
        "install.sh",
        "uninstall.sh",
        "manifest.json",
        "hypr/omarchy-spaces.lua",
        "skills/omarchy-spaces/SKILL.md",
    ] {
        fs::copy(root.join(file), plugin.join(file)).unwrap();
    }

    // Omarchy's commands, which note what they were asked for.
    let stubs = tmp.path().join("stubs");
    let log = tmp.path().join("log");
    fs::create_dir_all(&stubs).unwrap();
    let stub = |name: &str, body: &str| {
        let path = stubs.join(name);
        fs::write(&path, format!("#!/bin/bash\n{body}\n")).unwrap();
        fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    };
    let note = format!("echo \"${{0##*/}} $*\" >>'{}'", log.display());
    stub("omarchy", &note);
    stub("omarchy-notification-send", &note);
    stub("hyprctl", &note);
    stub(
        "omarchy-shell",
        &format!("cat '{}/'$2.json", tmp.path().display()),
    );
    let shell = |left: &[&str], workspaces: bool, spaces: bool| {
        let left: Vec<Value> = left.iter().map(|id| json!({ "id": id })).collect();
        let config = json!({ "bar": { "layout": { "left": left } } });
        fs::write(tmp.path().join("listShellConfig.json"), config.to_string()).unwrap();
        let plugins = json!([
            { "id": "com.t4t5.spaces", "enabled": spaces },
            { "id": "omarchy.menu", "enabled": false },
            { "id": "omarchy.workspaces", "enabled": workspaces },
        ]);
        fs::write(tmp.path().join("listPlugins.json"), plugins.to_string()).unwrap();
    };
    let run = |dir: &Path, args: &[&str]| {
        fs::write(&log, "").unwrap();
        let out = Command::new("bash")
            .arg(dir.join("install.sh"))
            .args(args)
            .env("HOME", &home)
            .env_remove("XDG_STATE_HOME")
            .env("PATH", format!("{}:/usr/bin:/bin", stubs.display()))
            .env(
                "SPACES_RELEASES",
                format!("file://{}", tmp.path().display()),
            )
            .output()
            .unwrap();
        (out, fs::read_to_string(&log).unwrap())
    };
    shell(&["omarchy.workspaces", "com.t4t5.spaces"], true, true);

    // Nothing in a clone someone works in.
    let clone = tmp.path().join("clone");
    std::os::unix::fs::symlink(&plugin, &clone).unwrap();
    let (out, did) = run(&clone, &["--setup"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(did, "");
    assert!(!home.exists());

    // Offline: the keys and skills for present agents have what they load, but the bar isn't brought up.
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::create_dir_all(home.join(".gemini/config")).unwrap();
    let (out, did) = run(&plugin, &["--setup"]);
    assert!(!out.status.success());
    assert!(did.contains("Couldn't install"), "{did}");
    assert!(!did.contains("omarchy plugin"), "{did}");
    assert!(did.contains("hyprctl reload"), "{did}");
    assert_eq!(
        fs::read_link(&link).unwrap(),
        plugin.join("hypr/omarchy-spaces.lua")
    );
    assert_eq!(
        fs::read_link(&claude_skill).unwrap(),
        plugin.join("skills/omarchy-spaces")
    );
    assert_eq!(
        fs::read_link(&gemini_skill).unwrap(),
        plugin.join("skills/omarchy-spaces")
    );
    assert!(!agents_skill.exists(), "doesn't create absent agent dirs");

    let package = Command::new(root.join("scripts/package.sh"))
        .arg(env!("CARGO_BIN_EXE_space"))
        .arg(std::env::consts::ARCH)
        .arg(tmp.path().join(format!("v{version}")))
        .output()
        .unwrap();
    assert!(package.status.success(), "{package:?}");
    let (out, did) = run(&plugin, &["--setup"]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("Installed space "));
    assert!(installed.exists());
    assert_eq!(
        did,
        "omarchy plugin enable com.t4t5.spaces --section left --index 0\n\
         omarchy plugin disable omarchy.menu\n\
         omarchy plugin disable omarchy.workspaces\n"
    );
    assert_eq!(fs::read_to_string(&replaced).unwrap(), "replace\n");

    // Only once, and it installs only once: putting the workspaces back later
    // keeps them.
    let (out, did) = run(&plugin, &["--setup"]);
    assert!(out.stdout.is_empty(), "{out:?}");
    assert_eq!(did, "");

    // Not with the tabs elsewhere on the bar, or nothing to replace.
    fs::remove_file(&replaced).unwrap();
    shell(&["omarchy.workspaces"], true, true);
    assert_eq!(run(&plugin, &["--setup"]).1, "");
    shell(&["com.t4t5.spaces"], false, true);
    assert_eq!(run(&plugin, &["--setup"]).1, "");
    assert!(!replaced.exists());

    // There's no asking, and nothing else to pass.
    shell(&["omarchy.workspaces", "com.t4t5.spaces"], true, true);
    let (out, did) = run(&plugin, &["--bar", "replace"]);
    assert!(!out.status.success());
    assert_eq!(did, "");

    // A Hyprland file or skill that's the user's own stays.
    fs::remove_file(&link).unwrap();
    fs::write(&link, "-- mine\n").unwrap();
    fs::remove_file(&claude_skill).unwrap();
    fs::create_dir_all(&claude_skill).unwrap();
    fs::write(claude_skill.join("SKILL.md"), "mine\n").unwrap();
    assert!(run(&plugin, &["--setup"]).0.status.success());
    assert_eq!(fs::read_to_string(&link).unwrap(), "-- mine\n");
    assert!(!claude_skill.is_symlink());
    assert_eq!(
        fs::read_to_string(claude_skill.join("SKILL.md")).unwrap(),
        "mine\n"
    );
}

#[test]
fn uninstall_undoes_the_setup_once_the_plugin_is_gone() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let plugin = home.join(".config/omarchy/plugins/com.t4t5.spaces");
    let link = home.join(".config/hypr/omarchy-spaces.lua");
    let skill_link = home.join(".claude/skills/omarchy-spaces");
    let config = home.join(".config/omarchy/shell.json");
    let state = home.join(".local/state/omarchy-spaces");
    let space = home.join(".local/bin/space");
    let mine = home.join(".config/omarchy-spaces/config.toml");
    for dir in [&link, &skill_link, &config, &space, &mine] {
        fs::create_dir_all(dir.parent().unwrap()).unwrap();
    }
    fs::create_dir_all(&state).unwrap();
    fs::write(&mine, "").unwrap();

    let stubs = tmp.path().join("stubs");
    let log = tmp.path().join("log");
    fs::create_dir_all(&stubs).unwrap();
    let stub = |name: &str, body: &str| {
        let path = stubs.join(name);
        let note = format!("echo \"${{0##*/}} $*\" >>'{}'", log.display());
        fs::write(&path, format!("#!/bin/bash\n{note}\n{body}\n")).unwrap();
        fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    };
    stub("omarchy", "");
    let clients = json!([
        { "address": "0x1", "workspace": { "name": "s1:2" } },
        { "address": "0x2", "workspace": { "name": "3" } },
        { "address": "0x3", "workspace": { "name": "special:s1:2" } },
        { "address": "0x4", "workspace": { "name": "s12:11" } },
    ]);
    stub(
        "hyprctl",
        &format!(
            "case $1 in clients) echo '{clients}' ;; activeworkspace) echo '{{\"name\":\"s1:2\"}}' ;; esac"
        ),
    );

    let built = "#!/bin/sh\n# omarchy-spaces\n";
    let set_up = || {
        fs::create_dir_all(plugin.join("hypr")).unwrap();
        fs::create_dir_all(plugin.join("skills/omarchy-spaces")).unwrap();
        fs::create_dir_all(&state).unwrap();
        fs::copy(root.join("uninstall.sh"), state.join("uninstall.sh")).unwrap();
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(plugin.join("hypr/omarchy-spaces.lua"), &link).unwrap();
        let _ = fs::remove_file(&skill_link);
        std::os::unix::fs::symlink(plugin.join("skills/omarchy-spaces"), &skill_link).unwrap();
        fs::write(&space, built).unwrap();
        let sum = Command::new("sha256sum").arg(&space).output().unwrap();
        let sum = String::from_utf8(sum.stdout).unwrap();
        let sum = sum.split_whitespace().next().unwrap();
        fs::write(state.join("installed"), format!("{sum}\n")).unwrap();
        fs::write(state.join("bar"), "replace\n").unwrap();
    };
    let run = || {
        fs::write(&log, "").unwrap();
        let out = Command::new("bash")
            .arg(state.join("uninstall.sh"))
            .env("HOME", &home)
            .env_remove("XDG_STATE_HOME")
            .env("SPACES_TRIES", "1")
            .env("PATH", format!("{}:/usr/bin:/bin", stubs.display()))
            .output()
            .unwrap();
        assert!(out.stderr.is_empty(), "{out:?}");
        fs::read_to_string(&log).unwrap()
    };
    let put_back = "omarchy bar put omarchy.menu --section left --index 0\n\
                    omarchy bar put omarchy.workspaces --after omarchy.menu\n";

    // The shell restarting, or its config not to be read, changes nothing.
    set_up();
    for text in [
        r#"{"bar":{"layout":{"left":[{"id":"com.t4t5.spaces"}]}}}"#,
        "{",
        "",
    ] {
        fs::write(&config, text).unwrap();
        assert_eq!(run(), "", "{text}");
    }
    fs::remove_file(&config).unwrap();
    assert_eq!(run(), "");

    // Turned off, it only puts the bar back, once.
    fs::write(&config, r#"{"bar":{"layout":{"left":[]}}}"#).unwrap();
    assert_eq!(run(), put_back);
    assert_eq!(run(), "");
    assert!(link.is_symlink() && space.exists() && state.exists());

    // Removed, it undoes all of it.
    set_up();
    fs::remove_dir_all(&plugin).unwrap();
    let did = run();
    let moved = |address: &str, key: &str| {
        format!(
            "hyprctl dispatch hl.dsp.window.move({{ workspace = \"{key}\", follow = false, window = \"address:{address}\" }})\n"
        )
    };
    assert!(did.contains(&moved("0x1", "2")), "{did}");
    assert!(did.contains(&moved("0x4", "11")), "{did}");
    assert_eq!(did.matches("window.move").count(), 2, "{did}");
    assert!(did.contains("hl.dsp.focus({ workspace = \"2\" })"), "{did}");
    assert!(did.ends_with(put_back), "{did}");
    assert!(!link.is_symlink() && !skill_link.is_symlink() && !space.exists() && !state.exists());
    assert!(mine.exists());

    // But not what it didn't make, nor a bar that kept Omarchy's.
    set_up();
    fs::remove_dir_all(&plugin).unwrap();
    fs::remove_file(&link).unwrap();
    fs::write(&link, "-- mine\n").unwrap();
    fs::write(&space, "#!/bin/sh\n# omarchy-spaces, built here\n").unwrap();
    fs::remove_file(state.join("bar")).unwrap();
    let did = run();
    assert!(!did.contains("omarchy bar"), "{did}");
    assert!(link.exists() && space.exists() && !state.exists());
}

#[test]
fn the_keys_run_commands_there_are() {
    let lua = include_str!("../hypr/omarchy-spaces.lua");
    let env = Env::new();
    let commands: Vec<&str> = lua
        .split("run(\"")
        .skip(1)
        .filter_map(|rest| rest.split_once('"'))
        .map(|(command, _)| command)
        .collect();
    assert!(commands.contains(&"tab new"), "{commands:?}");
    for command in commands {
        let mut args: Vec<&str> = command.split(' ').collect();
        args.push("--help");
        env.ok(&args);
    }
    env.ok(&["window", "send", "--silent", "--help"]);
}
