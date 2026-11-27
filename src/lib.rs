//! Spaces for Omarchy. A space is a place you work, called after
//! the folder its terminal is in, with its own set of workspaces, `<id>:1` to
//! `<id>:10` on the first monitor, and as many on each other. See
//! docs/internals/architecture.md.

pub mod agent;
pub mod browser;
pub mod commands;
pub mod config;
pub mod context;
pub mod events;
pub mod folder;
pub mod hypr;
pub mod launch;
pub mod layout;
pub mod list;
pub mod live;
pub mod monitors;
pub mod name;
pub mod paths;
pub mod reserve;
pub mod restore;
pub mod session;
pub mod shell;
pub mod sound;
pub mod spaces;
pub mod state;
pub mod status;
pub mod tabs;
pub mod terminal;
pub mod view;
pub mod watch;
