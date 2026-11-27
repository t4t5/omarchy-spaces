//! `~/.config/omarchy-spaces/config.toml`, the global config: what Spaces
//! itself does. What Omarchy has a setting for, such as which terminal to
//! open, is Omarchy's to say (`launch.rs`).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde::de::IgnoredAny;

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
    #[serde(flatten)]
    unknown: Unknown,
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
        [
            ("", &self.unknown),
            ("restore.", &self.restore.unknown),
            ("sounds.", &self.sounds.unknown),
        ]
        .into_iter()
        .flat_map(|(table, unknown)| unknown.keys().map(move |key| format!("{table}{key}")))
        .collect()
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
}
