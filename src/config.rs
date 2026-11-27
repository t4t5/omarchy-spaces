//! `~/.config/omarchy-spaces/config.toml`, the global config: what Spaces
//! itself does. What Omarchy has a setting for, such as which terminal to
//! open, is Omarchy's to say (`launch.rs`).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::events;
use crate::paths;

/// Settings that aren't ours: misspelled, or an older or newer Spaces'. Each
/// table takes its own, to warn of them; the rest of the file still counts.
type Unknown = BTreeMap<String, IgnoredAny>;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// What comes back after a restart (`space restore`).
    pub restore: Restore,
    /// The sounds when an agent needs you (`sound.rs`).
    pub sounds: Sounds,
    /// Custom menu actions (`[[actions.space]]`, `[[actions.tab]]`).
    pub actions: Actions,
    /// Event hooks (`[hooks]`), fired by `space watch` when things change.
    pub hooks: Hooks,
    #[serde(flatten)]
    unknown: Unknown,
}

/// `[actions]`: user-defined commands shown in space and tab menus and runnable
/// via `space action <id>`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Actions {
    pub space: Vec<Action>,
    pub tab: Vec<Action>,
    #[serde(flatten)]
    unknown: Unknown,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Action {
    #[serde(default)]
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub hint: Option<String>,
    #[serde(default)]
    pub when: Option<String>,
    pub command: CommandSpec,
    #[serde(flatten)]
    unknown: Unknown,
}

impl Action {
    /// The id used by `space action <id>` and menus: the configured `id`, or a
    /// slug of `label` when left out.
    pub fn resolved_id(&self) -> String {
        let trimmed = self.id.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
        slug(&self.label)
    }
}

fn slug(label: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in label.chars() {
        if ch.is_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.extend(ch.to_lowercase());
        } else {
            dash = true;
        }
    }
    if out.is_empty() { "action".into() } else { out }
}

/// A command in `config.toml`: either a shell string (`sh -c`) or an argv list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum CommandSpec {
    Shell(String),
    Argv(Vec<String>),
}

/// `[hooks]`: shell commands run on event types (`space.created`,
/// `space.focused`, `space.removed`, `status.changed`, …).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Hooks {
    #[serde(flatten)]
    entries: BTreeMap<String, HookCommands>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum HookCommands {
    One(String),
    Many(Vec<String>),
}

impl Hooks {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn for_event(&self, event: &str) -> Vec<&str> {
        match self.entries.get(event) {
            Some(HookCommands::One(cmd)) => vec![cmd.as_str()],
            Some(HookCommands::Many(cmds)) => cmds.iter().map(String::as_str).collect(),
            None => Vec::new(),
        }
    }

    fn unknown_events(&self) -> impl Iterator<Item = &String> {
        self.entries
            .keys()
            .filter(|key| !events::HOOK_TYPES.contains(&key.as_str()))
    }
}

/// `[restore]`: whether the plugin restores on start, and what comes back.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Restore {
    /// Whether the plugin opens your last spaces again after a restart. `space
    /// restore` does either way.
    pub auto: bool,
    /// Whether a restart picks up the agents' conversations again.
    pub agents: bool,
    /// Whether a restart opens the browser's windows again, and so whether
    /// what they hold is saved.
    pub browsers: bool,
    #[serde(flatten)]
    unknown: Unknown,
}

impl Default for Restore {
    fn default() -> Self {
        Self {
            auto: true,
            agents: true,
            browsers: true,
            unknown: Unknown::new(),
        }
    }
}

/// `[sounds]`: whether to play them, and files in place of the defaults.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Sounds {
    pub enabled: bool,
    /// When an agent finishes while you aren't looking.
    pub done: Option<String>,
    /// When an agent starts waiting for you.
    pub waiting: Option<String>,
    #[serde(flatten)]
    unknown: Unknown,
}

impl Config {
    /// Reads the config file, or the defaults when there is none.
    pub fn load() -> Result<Self> {
        Self::load_from(&paths::config_file())
    }

    /// Reads the config file, warning and falling back to defaults when it
    /// can't be read.
    pub fn load_or_default() -> Self {
        Self::load().unwrap_or_else(|e| {
            eprintln!("space: warning: {e:#}");
            Self::default()
        })
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => {
                let config: Self = toml::from_str(&text)
                    .with_context(|| format!("can't read {}", paths::tilde(path)))?;
                for setting in config.unknown() {
                    eprintln!(
                        "space: warning: there's no setting '{setting}', in {}",
                        paths::tilde(path)
                    );
                }
                Ok(config)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("can't read {}", paths::tilde(path))),
        }
    }

    /// The settings that aren't ours, as `sounds.bell`.
    fn unknown(&self) -> Vec<String> {
        let mut out: Vec<String> = [
            ("", &self.unknown),
            ("restore.", &self.restore.unknown),
            ("sounds.", &self.sounds.unknown),
            ("actions.", &self.actions.unknown),
        ]
        .into_iter()
        .flat_map(|(table, unknown)| unknown.keys().map(move |key| format!("{table}{key}")))
        .collect();
        for (i, action) in self.actions.space.iter().enumerate() {
            for key in action.unknown.keys() {
                out.push(format!("actions.space[{i}].{key}"));
            }
        }
        for (i, action) in self.actions.tab.iter().enumerate() {
            for key in action.unknown.keys() {
                out.push(format!("actions.tab[{i}].{key}"));
            }
        }
        for key in self.hooks.unknown_events() {
            out.push(format!("hooks.{key}"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Result<Config> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, text).unwrap();
        Config::load_from(&path)
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load_from(&dir.path().join("config.toml")).unwrap();
        let restore = config.restore;
        assert!(restore.auto && restore.agents && restore.browsers);
        assert!(!config.sounds.enabled, "no new noises unless you ask");
    }

    #[test]
    fn reads_restore() {
        let restore = read("[restore]\nbrowsers = false\n").unwrap().restore;
        assert!(restore.auto && restore.agents && !restore.browsers);

        let restore = read("[restore]\nauto = false\nagents = false\n")
            .unwrap()
            .restore;
        assert!(!restore.auto && !restore.agents && restore.browsers);
    }

    #[test]
    fn reads_sounds() {
        let sounds = read("[sounds]\nenabled = true\ndone = \"~/ding.oga\"\n")
            .unwrap()
            .sounds;
        assert!(sounds.enabled);
        assert_eq!(sounds.done.as_deref(), Some("~/ding.oga"));
        assert_eq!(sounds.waiting, None);
    }

    #[test]
    fn unknown_settings_leave_the_others_be() {
        let config = read(
            "roots = [\"~/dev\"]\n\
             [restore]\nbrowsers = false\napps = true\n\
             [sounds]\nenabled = true\nbell = { a = 1.5 }\n\
             [icons]\nsize = 2\n",
        )
        .unwrap();
        assert!(!config.restore.browsers && config.restore.auto);
        assert!(config.sounds.enabled);
        assert_eq!(
            config.unknown(),
            ["icons", "roots", "restore.apps", "sounds.bell"]
        );

        // A value of the wrong kind is still a mistake.
        assert!(read("[sounds]\nenabled = \"no\"\n").is_err());
    }

    #[test]
    fn reads_actions_and_hooks() {
        let config = read(
            r#"
[[actions.space]]
label = "New worktree…"
icon = "󰘬"
when = "folder"
command = "~/.config/omarchy-spaces/scripts/new-wt.sh"

[[actions.space]]
id = "rm-wt"
label = "Remove worktree"
when = "meta.worktree"
command = ["~/scripts/rm.sh", "--force"]

[[actions.tab]]
label = "Lazygit"
command = "space tab new -- lazygit"

[hooks]
"space.created" = "~/scripts/git-meta.sh"
"space.focused" = ["~/scripts/git-meta.sh", "~/scripts/ping.sh"]
"bogus.event" = "true"
"#,
        )
        .unwrap();

        assert_eq!(config.actions.space.len(), 2);
        assert_eq!(config.actions.space[0].resolved_id(), "new-worktree");
        assert_eq!(config.actions.space[1].resolved_id(), "rm-wt");
        assert_eq!(
            config.actions.space[1].command,
            CommandSpec::Argv(vec!["~/scripts/rm.sh".into(), "--force".into()])
        );
        assert_eq!(config.actions.tab.len(), 1);
        assert_eq!(config.actions.tab[0].resolved_id(), "lazygit");

        assert_eq!(
            config.hooks.for_event("space.created"),
            vec!["~/scripts/git-meta.sh"]
        );
        assert_eq!(
            config.hooks.for_event("space.focused"),
            vec!["~/scripts/git-meta.sh", "~/scripts/ping.sh"]
        );
        assert_eq!(config.unknown(), ["hooks.bogus.event"]);
    }
}
