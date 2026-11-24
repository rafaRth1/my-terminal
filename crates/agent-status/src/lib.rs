//! Agent status for the sidebar.
//!
//! Claude Code runs hook commands on lifecycle events. The
//! `my-terminal-hook` binary forwards each event to the app over a Unix
//! socket ([`ipc`]); the app parses it ([`HookEvent`]) and folds it into an
//! [`AgentState`] per pane.

pub mod adapt;
mod event;
pub mod ipc;
mod project;
mod state;

pub use event::{HookEvent, HookKind, ParseError};
pub use project::{format_elapsed, project_name};
pub use state::{AgentState, AgentStatus};

/// Environment variable with the id of the pane a program runs in.
pub const PANE_ENV: &str = "MY_TERMINAL_PANE";
/// Environment variable with the path of the app's hook socket.
pub const SOCKET_ENV: &str = "MY_TERMINAL_SOCKET";
