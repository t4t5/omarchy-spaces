//! A client for Hyprland's IPC socket, the same one `hyprctl` talks to.
//!
//! Talking to the socket directly instead of shelling out to `hyprctl` keeps
//! a keypress to about a millisecond. Hyprland 0.56 is configured in Lua, and
//! its `dispatch` request takes a Lua expression, which it wraps in
//! `hl.dispatch(...)`.

use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

/// Hyprland 0.57+ omits `id` for named workspaces (`s1:1`, etc.).
fn nullable_i64<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<i64, D::Error> {
    Ok(Option::<i64>::deserialize(deserializer)?.unwrap_or(0))
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    #[serde(default, deserialize_with = "nullable_i64")]
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub windows: u32,
    /// The monitor it's on, by name.
    #[serde(default)]
    pub monitor: String,
    /// The layout its windows are tiled by.
    #[serde(default, rename = "tiledLayout")]
    pub layout: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Client {
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub pid: i64,
    pub workspace: WorkspaceRef,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub title: String,
    /// In hex; goes up with each window opened.
    #[serde(default, rename = "stableId")]
    pub stable_id: String,
    #[serde(default)]
    pub at: (i32, i32),
    #[serde(default)]
    pub size: (i32, i32),
    #[serde(default)]
    pub floating: bool,
    /// 0 when it isn't, 1 maximized, 2 fullscreen.
    #[serde(default)]
    pub fullscreen: u8,
    /// The windows it's grouped with, itself included.
    #[serde(default)]
    pub grouped: Vec<String>,
    /// How long ago it had focus: 0 for the one that has it.
    #[serde(default, rename = "focusHistoryID")]
    pub focus: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Monitor {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    /// Turned a quarter when odd.
    #[serde(default)]
    pub transform: u8,
    /// Whether it's the one you are on.
    #[serde(default)]
    pub focused: bool,
    /// How many frames it shows a second.
    #[serde(default, rename = "refreshRate")]
    pub refresh: f64,
    /// The workspace it shows.
    #[serde(default, rename = "activeWorkspace")]
    pub shows: Option<WorkspaceRef>,
}

impl Client {
    /// Goes up with each window opened.
    pub fn opened(&self) -> u64 {
        u64::from_str_radix(&self.stable_id, 16).unwrap_or(0)
    }
}

impl Monitor {
    /// Its size in logical pixels.
    pub fn size(&self) -> (i32, i32) {
        let scale = if self.scale > 0.0 { self.scale } else { 1.0 };
        let (w, h) = (self.width / scale, self.height / scale);
        let (w, h) = (w.round() as i32, h.round() as i32);
        if self.transform % 2 == 1 {
            (h, w)
        } else {
            (w, h)
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceRef {
    #[serde(default, deserialize_with = "nullable_i64")]
    pub id: i64,
    pub name: String,
}

/// What Hyprland has, as of one moment.
#[derive(Debug)]
pub struct Everything {
    pub monitors: Vec<Monitor>,
    pub workspaces: Vec<Workspace>,
    pub clients: Vec<Client>,
}

pub struct Hypr {
    /// Different each time Hyprland starts.
    pub instance: String,
    socket: PathBuf,
}

impl Hypr {
    /// The running Hyprland instance, from `HYPRLAND_INSTANCE_SIGNATURE`.
    pub fn from_env() -> Result<Self> {
        let signature = env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow!("Hyprland isn't running (HYPRLAND_INSTANCE_SIGNATURE is unset)")
            })?;
        let runtime = env::var_os("XDG_RUNTIME_DIR")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", current_uid())));
        Ok(Self {
            socket: runtime.join("hypr").join(&signature).join(".socket.sock"),
            instance: signature,
        })
    }

    /// Hyprland's events (`openwindow>>...`), one line each, read on a
    /// thread of their own. The channel closes when Hyprland does.
    pub fn events(&self) -> Result<mpsc::Receiver<String>> {
        let path = self.socket.with_file_name(".socket2.sock");
        let stream = UnixStream::connect(&path)
            .with_context(|| format!("can't reach Hyprland's events at {}", path.display()))?;
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(receive)
    }

    /// Sends one request and returns Hyprland's reply.
    pub fn request(&self, request: &str) -> Result<String> {
        let mut stream = UnixStream::connect(&self.socket)
            .with_context(|| format!("can't reach Hyprland at {}", self.socket.display()))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(request.as_bytes())?;
        let mut reply = String::new();
        stream
            .read_to_string(&mut reply)
            .with_context(|| format!("no reply from Hyprland to '{request}'"))?;
        Ok(reply)
    }

    fn query<T: DeserializeOwned>(&self, what: &str) -> Result<T> {
        let reply = self.request(&format!("j/{what}"))?;
        serde_json::from_str(&reply).with_context(|| {
            format!(
                "unexpected reply from Hyprland to '{what}': {}",
                reply.trim()
            )
        })
    }

    pub fn active_workspace(&self) -> Result<Workspace> {
        self.query("activeworkspace")
    }

    pub fn workspaces(&self) -> Result<Vec<Workspace>> {
        self.query("workspaces")
    }

    pub fn clients(&self) -> Result<Vec<Client>> {
        self.query("clients")
    }

    pub fn monitors(&self) -> Result<Vec<Monitor>> {
        self.query("monitors")
    }

    /// The monitors, workspaces and windows, all as of one moment: Hyprland
    /// answers a batch in one go, so nothing changes between them.
    pub fn everything(&self) -> Result<Everything> {
        let reply = self.request("[[BATCH]]j/monitors;j/workspaces;j/clients")?;
        // The answers follow each other, blank lines between.
        let mut answers = serde_json::Deserializer::from_str(&reply);
        let mut read = || -> serde_json::Result<Everything> {
            Ok(Everything {
                monitors: Deserialize::deserialize(&mut answers)?,
                workspaces: Deserialize::deserialize(&mut answers)?,
                clients: Deserialize::deserialize(&mut answers)?,
            })
        };
        read().context("unexpected reply from Hyprland to a batch")
    }

    /// A number from Hyprland's config, `None` when it has none by that name.
    pub fn option(&self, name: &str) -> Option<i32> {
        let option: serde_json::Value = self.query(&format!("getoption {name}")).ok()?;
        let on = option.get("bool").and_then(serde_json::Value::as_bool);
        let number = option.get("int").and_then(serde_json::Value::as_i64);
        on.map(i32::from)
            .or_else(|| number.and_then(|n| i32::try_from(n).ok()))
    }

    /// Runs a Lua script, all of it between two frames.
    pub fn eval(&self, script: &str) -> Result<()> {
        let reply = self.request(&format!("eval {script}"))?;
        let reply = reply.trim();
        if reply == "ok" {
            Ok(())
        } else {
            bail!("Hyprland couldn't run a script: {reply}")
        }
    }

    /// Whether any window has focus. Hyprland answers `{}` when none does.
    pub fn has_active_window(&self) -> Result<bool> {
        let window: serde_json::Value = self.query("activewindow")?;
        Ok(window.get("address").is_some())
    }

    /// The address of the focused window, if a window has focus.
    pub fn active_window_address(&self) -> Result<Option<String>> {
        let window: serde_json::Value = self.query("activewindow")?;
        Ok(window
            .get("address")
            .and_then(serde_json::Value::as_str)
            .map(String::from))
    }

    /// Runs a Lua dispatch expression, such as
    /// `hl.dsp.focus({ workspace = "name:ren:1" })`.
    pub fn dispatch(&self, expression: &str) -> Result<()> {
        let reply = self.request(&format!("dispatch {expression}"))?;
        let reply = reply.trim();
        if reply == "ok" {
            Ok(())
        } else {
            bail!("Hyprland refused `{expression}`: {reply}")
        }
    }
}

/// A Lua string literal.
pub fn lua_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Focus a workspace by Hyprland selector (`name:ren:1`, `3`), creating it if
/// it doesn't exist.
pub fn focus_workspace(selector: &str) -> String {
    format!("hl.dsp.focus({{ workspace = {} }})", lua_string(selector))
}

/// Move the focused window to a workspace by selector.
pub fn move_window(selector: &str, follow: bool) -> String {
    format!(
        "hl.dsp.window.move({{ workspace = {}, follow = {follow} }})",
        lua_string(selector)
    )
}

/// Focus a monitor, by its name.
pub fn focus_monitor(monitor: &str) -> String {
    format!("hl.dsp.focus({{ monitor = {} }})", lua_string(monitor))
}

/// Lua that shows a workspace on the monitor it's on, staying where you are,
/// by the Hyprland part (`hypr/omarchy-spaces.lua`).
pub fn show_workspace(name: &str) -> String {
    format!(
        "local spaces = package.loaded[\"hypr.omarchy-spaces\"]\n\
         if spaces and spaces.internal then spaces.internal.show({}) end",
        lua_string(name)
    )
}

/// Lua that has every monitor follow to the space you are in, if the Hyprland
/// part is loaded (`hypr/omarchy-spaces.lua`).
pub const FOLLOW: &str = "local spaces = package.loaded[\"hypr.omarchy-spaces\"]\n\
     if spaces and spaces.internal then spaces.internal.sync() end";

/// Move a workspace to a monitor, by its name.
pub fn move_workspace(selector: &str, monitor: &str) -> String {
    format!(
        "hl.dsp.workspace.move({{ workspace = {}, monitor = {} }})",
        lua_string(selector),
        lua_string(monitor)
    )
}

/// Rename a workspace: its windows stay as they are.
pub fn rename_workspace(selector: &str, name: &str) -> String {
    format!(
        "hl.dsp.workspace.rename({{ workspace = {}, name = {} }})",
        lua_string(selector),
        lua_string(name)
    )
}

/// Move a window to a workspace by selector, without going along. A grouped
/// one takes its group.
pub fn move_window_to(address: &str, selector: &str) -> String {
    format!(
        "hl.dsp.window.move({{ workspace = {}, follow = false, window = {} }})",
        lua_string(selector),
        lua_string(&format!("address:{address}"))
    )
}

/// Run a shell command, its first window placed on `workspace`, as
/// `name:s1:1 silent`: there, without going there, and drawn whole at once,
/// without its open animation (`animate_again` brings animations back).
pub fn exec_on(command: &str, workspace: &str) -> String {
    format!(
        "hl.dsp.exec_cmd({}, {{ workspace = {}, no_anim = true }})",
        lua_string(command),
        lua_string(workspace)
    )
}

/// Has windows of a class open on `workspace`, as `name:s1:1 silent`, until
/// `enabled` is false.
pub fn open_class_on(class: &str, workspace: &str, enabled: bool) -> String {
    format!(
        "hl.window_rule({{ name = \"omarchy-spaces-restore\", match = {{ class = {} }}, \
         workspace = {}, enabled = {enabled} }})",
        lua_string(class),
        lua_string(workspace)
    )
}

/// Run a shell command.
pub fn exec(command: &str) -> String {
    format!("hl.dsp.exec_cmd({})", lua_string(command))
}

/// Lets a window placed by `exec_on` animate again, as others do. A value
/// set this way outranks the exec rule. (Lua's `false` reads as missing to
/// Hyprland, so it's the string.)
pub fn animate_again(address: &str) -> String {
    format!(
        "hl.dsp.window.set_prop({{ prop = \"no_anim\", value = \"false\", window = {} }})",
        lua_string(&format!("address:{address}"))
    )
}

/// Ask a window to close, by its address, as `super+w` does: the app can
/// still ask about unsaved work.
pub fn close_window(address: &str) -> String {
    format!(
        "hl.dsp.window.close({{ window = {} }})",
        lua_string(&format!("address:{address}"))
    )
}

fn current_uid() -> u32 {
    // Only reached when XDG_RUNTIME_DIR is unset; read the uid from /proc
    // rather than pulling in libc for one call.
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("Uid:"))
                .and_then(|ids| ids.split_whitespace().next()?.parse().ok())
        })
        .unwrap_or(1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_dispatch_expressions() {
        assert_eq!(
            focus_workspace("name:ren:1"),
            r#"hl.dsp.focus({ workspace = "name:ren:1" })"#
        );
        assert_eq!(
            move_window("name:ren:2", false),
            r#"hl.dsp.window.move({ workspace = "name:ren:2", follow = false })"#
        );
        assert_eq!(
            rename_workspace("name:s1:2", "s2:3"),
            r#"hl.dsp.workspace.rename({ workspace = "name:s1:2", name = "s2:3" })"#
        );
        assert_eq!(
            move_window_to("0x55a4", "name:s1:2"),
            r#"hl.dsp.window.move({ workspace = "name:s1:2", follow = false, window = "address:0x55a4" })"#
        );
        assert_eq!(lua_string(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(
            exec_on("kitty", "name:s1:1 silent"),
            r#"hl.dsp.exec_cmd("kitty", { workspace = "name:s1:1 silent", no_anim = true })"#
        );
        assert_eq!(
            animate_again("0x55a4"),
            r#"hl.dsp.window.set_prop({ prop = "no_anim", value = "false", window = "address:0x55a4" })"#
        );
        assert_eq!(
            close_window("0x55a4"),
            r#"hl.dsp.window.close({ window = "address:0x55a4" })"#
        );
    }

    #[test]
    fn parses_hyprland_057_named_workspaces_without_id() {
        let ws: Workspace = serde_json::from_str(
            r#"{"address":"s1:1","type":"normal","name":"s1:1","monitor":"eDP-1","windows":1}"#,
        )
        .unwrap();
        assert_eq!(ws.id, 0);
        assert_eq!(ws.name, "s1:1");

        let ws_null: Workspace =
            serde_json::from_str(r#"{"id":null,"name":"s1:2","monitor":"eDP-1","windows":0}"#)
                .unwrap();
        assert_eq!(ws_null.id, 0);

        let client: Client = serde_json::from_str(
            r#"{"address":"0x55a4","pid":123,"workspace":{"address":"s1:1","type":"normal","name":"s1:1"}}"#,
        )
        .unwrap();
        assert_eq!(client.workspace.id, 0);
        assert_eq!(client.workspace.name, "s1:1");

        let monitor: Monitor = serde_json::from_str(
            r#"{"name":"eDP-1","x":0,"y":0,"width":1920,"height":1080,"scale":1.0,"activeWorkspace":{"address":"s1:1","type":"normal","name":"s1:1"}}"#,
        )
        .unwrap();
        let shows = monitor.shows.unwrap();
        assert_eq!(shows.id, 0);
        assert_eq!(shows.name, "s1:1");
    }
}
