use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use clap::{Parser, Subcommand};

use omarchy_spaces::{commands, name, shell, watch};

/// Spaces for Hyprland: a set of tabs for each thing you work on.
#[derive(Parser)]
#[command(name = "space", version)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open a new space with a terminal, in the folder you are in (the
    /// focused terminal's, else the active space's, else home). It is called
    /// after the folder its terminal is in.
    New {
        /// Open it here instead.
        dir: Option<PathBuf>,
    },
    /// Go to a space where you left it.
    Go {
        /// What it's called, or its id; 'home' for the plain workspaces.
        space: String,
    },
    /// Go to the next space down the sidebar, wrapping around to `home`.
    Next,
    /// Go to the space before, up the sidebar, wrapping around to the last.
    Prev,
    /// List spaces, the active one marked, with their window counts.
    Ls {
        /// Print JSON, for scripts.
        #[arg(long)]
        json: bool,
    },
    /// Name a space, the active one unless you say which. An empty name has it
    /// called after its folder again.
    #[command(override_usage = "space rename [SPACE] <NAME>")]
    Rename {
        #[arg(value_name = "SPACE")]
        first: String,
        #[arg(value_name = "NAME")]
        second: Option<String>,
    },
    /// Give a space, the active one unless you say which, a glyph, an emoji, or
    /// an image file for its badge, in place of its letters. An empty icon
    /// brings the letters back.
    #[command(override_usage = "space icon [SPACE] <ICON>")]
    Icon {
        #[arg(value_name = "SPACE")]
        first: String,
        #[arg(value_name = "ICON")]
        second: Option<String>,
    },
    /// Move a space up or down the sidebar, to a position counting from 1
    /// below `home`, which stays first.
    Reorder {
        /// What it's called, or its id.
        space: String,
        position: usize,
    },
    /// Close every window of a space, the active one unless you say which.
    Close { space: Option<String> },
    /// Open the spaces that were up before Hyprland last restarted, with
    /// their terminals arranged as they were.
    Restore {
        /// An earlier session to open instead, from
        /// ~/.local/state/omarchy-spaces/sessions/.
        file: Option<PathBuf>,
        /// Run by the plugin on startup: does nothing when `[restore]` sets
        /// `auto = false`.
        #[arg(long, hide = true)]
        auto: bool,
    },
    /// Open a terminal in the folder you are in, or the active space's.
    Terminal,
    /// Print what the plugin shows, as a line of JSON each time it changes. For
    /// the plugin alone, it changes with it: scripts read `space ls --json`.
    Watch {
        /// Play a sound when an agent needs you or finishes unseen.
        #[arg(long)]
        sounds: bool,
    },
    /// Run as the program of a terminal that's restored: your own shell, in
    /// the conversation that was in it.
    #[command(hide = true)]
    Shell,
    /// Run by Hyprland when closing a window empties the workspace you are
    /// on: go to another tab, or end the space.
    #[command(hide = true)]
    Emptied { workspace: String },
    /// Run by Hyprland as a workspace comes or goes: number each space's tabs
    /// 1, 2, 3, ... again, closing the gap one that ended left.
    #[command(hide = true)]
    Renumber,
    /// Open, go to, name, arrange, close and move tabs. A tab is given by its
    /// id, as `space ls --json` shows it.
    #[command(subcommand)]
    Tab(TabCmd),
    /// Move the focused window to another space or tab.
    #[command(subcommand)]
    Window(WindowCmd),
}

#[derive(Subcommand)]
enum TabCmd {
    /// Open a new tab in the active space, with a terminal in its folder; in
    /// `home`, an empty one.
    New,
    /// Go to tab N of the active space on this monitor, or a tab by its id.
    Go {
        #[arg(value_name = "N", value_parser = tab)]
        tab: String,
    },
    /// Go to the active space's next tab, wrapping around.
    Next,
    /// Go to the active space's tab before, wrapping around.
    Prev,
    /// Name a tab, the one you are on unless you say which. An empty name has
    /// it called after its window again (in `home`, just its number).
    #[command(override_usage = "space tab rename [TAB] <NAME>")]
    Rename {
        #[arg(value_name = "TAB")]
        first: String,
        #[arg(value_name = "NAME")]
        second: Option<String>,
    },
    /// Move a tab to a position among its space's tabs, counting from 1.
    Reorder {
        #[arg(value_name = "TAB")]
        tab: String,
        position: usize,
    },
    /// Close every window of a tab, the one you are on unless you say which.
    Close {
        #[arg(value_name = "TAB")]
        tab: Option<String>,
    },
    /// Move a tab, the one you are on unless you say which, to another space
    /// (or next or prev), after the tabs there, and go with it.
    #[command(override_usage = "space tab send [TAB] <SPACE>")]
    Send {
        #[arg(value_name = "TAB")]
        first: String,
        #[arg(value_name = "SPACE")]
        second: Option<String>,
    },
}

#[derive(Subcommand)]
enum WindowCmd {
    /// Send the focused window to another space, on the tab you'd land on
    /// going there, and go with it.
    Send {
        /// What it's called, or its id; 'home' for the plain workspaces;
        /// next or prev for the space below or above, wrapping around.
        #[arg(value_parser = space_target)]
        space: commands::SpaceTarget,
        /// Stay where you are instead of following the window.
        #[arg(long)]
        silent: bool,
    },
    /// Move the focused window to tab N of the active space, and go with it.
    Tab {
        #[arg(value_parser = slot)]
        n: u8,
        /// Stay where you are instead of following the window.
        #[arg(long)]
        silent: bool,
    },
}

fn slot(raw: &str) -> Result<u8, String> {
    name::parse_slot(raw).ok_or_else(|| format!("'{raw}' isn't a tab: use 1-{}", name::SLOTS))
}

/// A slot, or a tab's id.
fn tab(raw: &str) -> Result<String, String> {
    if !raw.contains(':') {
        slot(raw)?;
    }
    Ok(raw.to_string())
}

fn space_target(raw: &str) -> Result<commands::SpaceTarget, String> {
    Ok(commands::SpaceTarget::of(raw))
}

fn run_tab(command: TabCmd) -> anyhow::Result<()> {
    match command {
        TabCmd::New => commands::go_to_tab(commands::Target::New),
        TabCmd::Go { tab } => match name::parse_slot(&tab) {
            Some(n) => commands::go_to_tab(commands::Target::Slot(n)),
            None => commands::go_to_named_tab(tab),
        },
        TabCmd::Next => commands::go_to_tab(commands::Target::Next),
        TabCmd::Prev => commands::go_to_tab(commands::Target::Prev),
        TabCmd::Rename { first, second } => match second {
            Some(new_name) => commands::rename_tab(Some(first), new_name),
            None => commands::rename_tab(None, first),
        },
        TabCmd::Reorder { tab, position } => commands::reorder_tab(tab, position),
        TabCmd::Close { tab } => commands::close_tab(tab),
        TabCmd::Send { first, second } => match second {
            Some(to) => commands::send_tab(Some(first), commands::SpaceTarget::of(&to)),
            None => commands::send_tab(None, commands::SpaceTarget::of(&first)),
        },
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Run from a keybind or the sidebar, nobody sees stderr, so say it in a
    // notification.
    // `watch` ends whenever Hyprland restarts, and the plugin restarts it: not
    // worth a notification. Nor is `renumber`, which Hyprland runs as every
    // tab comes and goes: one that fails would fail again each time, and the
    // tabs' numbers show it.
    let unseen = !matches!(
        cli.command,
        Cmd::Ls { .. } | Cmd::Watch { .. } | Cmd::Shell | Cmd::Renumber
    ) && !std::io::stderr().is_terminal();

    let result = match cli.command {
        Cmd::New { dir } => commands::new(dir),
        Cmd::Go { space } => commands::go(space),
        Cmd::Next => commands::step(true),
        Cmd::Prev => commands::step(false),
        Cmd::Ls { json } => commands::ls(json),
        Cmd::Rename { first, second } => match second {
            Some(new_name) => commands::rename(Some(first), new_name),
            None => commands::rename(None, first),
        },
        Cmd::Icon { first, second } => match second {
            Some(icon) => commands::icon(Some(first), icon),
            None => commands::icon(None, first),
        },
        Cmd::Reorder { space, position } => commands::reorder(space, position),
        Cmd::Close { space } => commands::close(space),
        Cmd::Restore { file, auto } => commands::restore(file, auto),
        Cmd::Terminal => commands::open_terminal(),
        Cmd::Watch { sounds } => watch::watch(sounds),
        Cmd::Shell => shell::shell(),
        Cmd::Emptied { workspace } => commands::emptied(&workspace),
        Cmd::Renumber => commands::renumber(),
        Cmd::Tab(command) => run_tab(command),
        Cmd::Window(WindowCmd::Send { space, silent }) => commands::send_window(space, silent),
        Cmd::Window(WindowCmd::Tab { n, silent }) => commands::window_to_tab(n, silent),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        // `space ls | head` closing the pipe early isn't an error.
        Err(e)
            if e.chain().any(|cause| {
                cause
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe)
            }) =>
        {
            ExitCode::SUCCESS
        }
        Err(e) => {
            let message = format!("{e:#}");
            eprintln!("space: {message}");
            if unseen {
                notify(&message);
            }
            ExitCode::FAILURE
        }
    }
}

fn notify(message: &str) {
    let _ = Command::new("notify-send")
        .args(["--app-name=Spaces", "Spaces", message])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}
