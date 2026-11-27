//! Custom menu actions (`[[actions]]`) and event hooks (`[hooks]`) from
//! `config.toml`.
//!
//! Hooks run asynchronously from the primary `space watch` whenever a state
//! transition occurs (never on the initial snapshot), receiving the event's
//! JSON line on stdin and `SPACE_*` variables in their environment. Actions run
//! via `space action <id>` from `SpaceMenu.qml`, keybindings, or scripts.

use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::commands::target_space;
use crate::config::{Action, CommandSpec, Config, Hooks};
use crate::context;
use crate::events::Emitted;
use crate::hypr::Hypr;
use crate::live;
use crate::name;
use crate::paths;
use crate::status::Status;

/// Runs a configured action by its id, targeting `--space` (or caller's/active
/// space) and optionally `--tab`.
pub fn run_action(id: &str, space: Option<String>, tab: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let action = find_action(&config, id, tab.is_some())?;

    let hypr = Hypr::from_env()?;
    let (rows, seen) = live::load_rows(Some(&hypr))?;
    let space_id = target_space(space, &rows, seen.as_ref())?;
    let row = rows
        .iter()
        .find(|row| row.id == space_id)
        .with_context(|| format!("no space '{space_id}'"))?;

    let tab_id = tab.or_else(|| {
        let (snapshot, live) = seen.as_ref()?;
        if let Some(caller) = context::resolve(snapshot, Some(live))
            && caller.space == space_id
        {
            return caller.tab_id();
        }
        if snapshot.active() == Some(space_id.as_str()) {
            return snapshot.active_tab().map(|k| name::tab_id(&space_id, k));
        }
        None
    });

    let folder = row.folder.as_ref().map(|f| f.path());
    let cwd = working_dir(folder);
    let mut cmd = build_command(&action.command)?;
    cmd.current_dir(&cwd);
    apply_space_env(
        &mut cmd,
        &row.id,
        &row.label,
        folder,
        &row.meta,
        row.status,
        tab_id.as_deref(),
    );
    cmd.env("SPACE_ACTION", action.resolved_id());

    if std::io::stderr().is_terminal() {
        let status = cmd
            .status()
            .with_context(|| format!("can't run action '{}'", action.resolved_id()))?;
        if !status.success() {
            bail!(
                "action '{}' exited with {}",
                action.resolved_id(),
                exit_description(status)
            );
        }
    } else {
        let output = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .with_context(|| format!("can't run action '{}'", action.resolved_id()))?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if err.is_empty() {
                bail!(
                    "action '{}' exited with {}",
                    action.resolved_id(),
                    exit_description(output.status)
                );
            }
            bail!("action '{}': {err}", action.resolved_id());
        }
    }
    Ok(())
}

/// Fires configured `[hooks]` for each emitted event without blocking the
/// caller. Each hook runs on a background thread that pipes the event JSON to
/// stdin and reaps the child when it exits.
pub fn fire(hooks: &Hooks, events: &[Emitted]) {
    if hooks.is_empty() || events.is_empty() {
        return;
    }
    for event in events {
        let commands = hooks.for_event(event.kind);
        if commands.is_empty() {
            continue;
        }
        for command in commands {
            let command = command.to_string();
            let event = event.clone();
            std::thread::spawn(move || run_hook(&command, &event));
        }
    }
}

fn run_hook(command: &str, event: &Emitted) {
    let spec = CommandSpec::Shell(command.to_string());
    let mut cmd = match build_command(&spec) {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("space: hook '{}' ({}): {e:#}", event.kind, command);
            return;
        }
    };
    let folder = event.folder.as_deref();
    cmd.current_dir(working_dir(folder))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    cmd.env("SPACE_EVENT", event.kind);
    if let Some(space) = &event.space {
        apply_space_env(
            &mut cmd,
            space,
            event.label.as_deref().unwrap_or(space),
            folder,
            &event.meta,
            event.status,
            event.tab.as_deref(),
        );
    } else if let Some(tab) = &event.tab {
        cmd.env("SPACE_TAB", tab);
    }
    if let Some(previous) = event.previous_status {
        cmd.env("SPACE_PREVIOUS_STATUS", status_name(previous));
    }
    if !event.changed.is_empty() {
        cmd.env("SPACE_CHANGED", event.changed.join(","));
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("space: hook '{}' ({}): {e}", event.kind, command);
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{}", event.line);
    }
    match child.wait_with_output() {
        Ok(output) if !output.status.success() => {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if err.is_empty() {
                eprintln!(
                    "space: hook '{}' ({}) exited with {}",
                    event.kind,
                    command,
                    exit_description(output.status)
                );
            } else {
                eprintln!("space: hook '{}' ({}): {err}", event.kind, command);
            }
        }
        Err(e) => eprintln!("space: hook '{}' ({}): {e}", event.kind, command),
        Ok(_) => {}
    }
}

fn find_action<'a>(config: &'a Config, id: &str, prefer_tab: bool) -> Result<&'a Action> {
    let lists: [&[Action]; 2] = if prefer_tab {
        [&config.actions.tab, &config.actions.space]
    } else {
        [&config.actions.space, &config.actions.tab]
    };
    for list in lists {
        if let Some(action) = list.iter().find(|a| a.resolved_id() == id) {
            return Ok(action);
        }
    }
    let available: Vec<String> = config
        .actions
        .space
        .iter()
        .chain(&config.actions.tab)
        .map(Action::resolved_id)
        .collect();
    if available.is_empty() {
        bail!(
            "no action '{id}' (define [[actions.space]] or [[actions.tab]] in {})",
            paths::tilde(&paths::config_file())
        );
    }
    bail!("no action '{id}'; configured: {}", available.join(", "));
}

fn build_command(spec: &CommandSpec) -> Result<Command> {
    match spec {
        CommandSpec::Shell(script) => {
            let trimmed = script.trim();
            if trimmed.is_empty() {
                bail!("action command is empty");
            }
            let mut cmd = Command::new("sh");
            cmd.args(["-c", trimmed]);
            Ok(cmd)
        }
        CommandSpec::Argv(argv) => {
            let Some((program, args)) = argv.split_first() else {
                bail!("action command is empty");
            };
            if program.trim().is_empty() {
                bail!("action command is empty");
            }
            let mut cmd = Command::new(paths::expand(program));
            cmd.args(args);
            Ok(cmd)
        }
    }
}

fn working_dir(folder: Option<&Path>) -> PathBuf {
    match folder {
        Some(dir) if dir.is_dir() => dir.to_path_buf(),
        _ => paths::home(),
    }
}

fn apply_space_env(
    cmd: &mut Command,
    id: &str,
    label: &str,
    folder: Option<&Path>,
    meta: &BTreeMap<String, Value>,
    status: Option<Status>,
    tab: Option<&str>,
) {
    cmd.env("SPACE_ID", id);
    cmd.env("SPACE_LABEL", label);
    match folder {
        Some(dir) => cmd.env("SPACE_DIR", dir),
        None => cmd.env_remove("SPACE_DIR"),
    };
    let meta_json = serde_json::to_string(meta).unwrap_or_else(|_| "{}".into());
    cmd.env("SPACE_META", meta_json);
    cmd.env("SPACE_STATUS", status.map_or("", status_name));
    if let Some(tab) = tab {
        cmd.env("SPACE_TAB", tab);
    } else {
        cmd.env_remove("SPACE_TAB");
    }
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Working => "working",
        Status::Done => "done",
        Status::Waiting => "waiting",
    }
}

fn exit_description(status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => code.to_string(),
        None => status.to_string(),
    }
}

/// Evaluates a `when` condition from `[[actions]]` against a space's live
/// fields. Comma-separated clauses are ANDed; each clause is one of:
/// `folder`, `home`, `status`, `status=<v>`, `meta.<path>`, `meta.<path>=<v>`,
/// `meta.<path>!=<v>`, optionally prefixed with `!`.
pub fn matches_when(
    expr: Option<&str>,
    id: &str,
    folder: Option<&Path>,
    meta: &BTreeMap<String, Value>,
    status: Option<Status>,
) -> bool {
    let Some(expr) = expr else {
        return true;
    };
    expr.split(',')
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
        .all(|clause| match_clause(clause, id, folder, meta, status))
}

fn match_clause(
    clause: &str,
    id: &str,
    folder: Option<&Path>,
    meta: &BTreeMap<String, Value>,
    status: Option<Status>,
) -> bool {
    if let Some((lhs, rhs)) = clause.split_once("!=") {
        return !equals_value(lhs.trim(), rhs.trim(), id, folder, meta, status);
    }
    if let Some((lhs, rhs)) = clause.split_once('=') {
        return equals_value(lhs.trim(), rhs.trim(), id, folder, meta, status);
    }
    let (negated, key) = match clause.strip_prefix('!') {
        Some(rest) => (true, rest.trim()),
        None => (false, clause),
    };
    let truthy = match key {
        "folder" => folder.is_some(),
        "home" => id == crate::name::HOME,
        "status" => status.is_some(),
        _ => match key.strip_prefix("meta.") {
            Some(path) => lookup_meta(meta, path).is_some_and(is_truthy),
            None => false,
        },
    };
    if negated { !truthy } else { truthy }
}

fn equals_value(
    key: &str,
    expected: &str,
    id: &str,
    folder: Option<&Path>,
    meta: &BTreeMap<String, Value>,
    status: Option<Status>,
) -> bool {
    match key {
        "status" => status.map_or("", status_name) == expected,
        "id" => id == expected,
        "folder" => folder.and_then(Path::to_str) == Some(expected),
        _ => match key.strip_prefix("meta.") {
            Some(path) => lookup_meta(meta, path).is_some_and(|v| value_text(v) == expected),
            None => false,
        },
    }
}

fn lookup_meta<'a>(meta: &'a BTreeMap<String, Value>, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut current = meta.get(first)?;
    for part in parts {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn evaluates_when_predicates() {
        let mut meta = BTreeMap::new();
        meta.insert(
            "worktree".into(),
            json!({ "branch": "feat/x", "dirty": true }),
        );
        meta.insert("count".into(), json!(0));
        meta.insert("empty".into(), json!(""));
        let dir = Some(Path::new("/tmp/repo"));

        assert!(matches_when(None, "s1", dir, &meta, None));
        assert!(matches_when(Some("folder"), "s1", dir, &meta, None));
        assert!(!matches_when(Some("folder"), "home", None, &meta, None));
        assert!(matches_when(Some("!home, folder"), "s1", dir, &meta, None));
        assert!(!matches_when(Some("!home"), "home", None, &meta, None));
        assert!(matches_when(Some("meta.worktree"), "s1", dir, &meta, None));
        assert!(matches_when(
            Some("meta.worktree.dirty"),
            "s1",
            dir,
            &meta,
            None
        ));
        assert!(matches_when(
            Some("meta.worktree.branch=feat/x"),
            "s1",
            dir,
            &meta,
            None
        ));
        assert!(matches_when(
            Some("meta.worktree.branch!=main"),
            "s1",
            dir,
            &meta,
            None
        ));
        assert!(!matches_when(Some("meta.count"), "s1", dir, &meta, None));
        assert!(!matches_when(Some("meta.empty"), "s1", dir, &meta, None));
        assert!(matches_when(Some("!meta.missing"), "s1", dir, &meta, None));
        assert!(matches_when(
            Some("status=waiting"),
            "s1",
            dir,
            &meta,
            Some(Status::Waiting)
        ));
        assert!(!matches_when(
            Some("status=waiting"),
            "s1",
            dir,
            &meta,
            Some(Status::Working)
        ));
    }

    #[test]
    fn fires_hooks_with_env_and_stdin_json() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("hook.out");
        let script = tmp.path().join("hook.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nbody=$(cat)\nprintf '%s|%s|%s|%s|%s\\n' \"$SPACE_EVENT\" \"$SPACE_ID\" \"$SPACE_DIR\" \"$SPACE_CHANGED\" \"$body\" > {}\n",
                out.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = Config::load_from(&{
            let cfg = tmp.path().join("config.toml");
            std::fs::write(
                &cfg,
                format!("[hooks]\n\"space.created\" = \"{}\"\n", script.display()),
            )
            .unwrap();
            cfg
        })
        .unwrap();

        let emitted = vec![Emitted {
            kind: "space.created",
            line: r#"{"type":"space.created","space":{"id":"s1"}}"#.into(),
            space: Some("s1".into()),
            label: Some("repo".into()),
            folder: Some(tmp.path().to_path_buf()),
            meta: BTreeMap::new(),
            tab: None,
            status: None,
            previous_status: None,
            changed: Vec::new(),
        }];
        fire(&config.hooks, &emitted);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !out.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let written = std::fs::read_to_string(&out).unwrap();
        assert_eq!(
            written.trim(),
            format!(
                "space.created|s1|{}||{}",
                tmp.path().display(),
                r#"{"type":"space.created","space":{"id":"s1"}}"#
            )
        );
    }
}
