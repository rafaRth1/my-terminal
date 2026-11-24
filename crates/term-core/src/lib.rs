//! Headless terminal sessions.
//!
//! A [`Session`] runs a shell inside a PTY and keeps its screen state in an
//! [`alacritty_terminal`] grid. It has no UI: the app crate renders the grid
//! and forwards keyboard input through [`Session::write`].

mod error;
mod event;
pub mod keys;
pub mod mouse;
pub mod selection;
mod session;
mod size;

pub use error::SessionError;
pub use event::{ChannelListener, TerminalEvent};
pub use session::{Session, SessionConfig, SharedTerm};
pub use size::TerminalSize;

/// Re-exported so the UI can read grid cells without a direct dependency.
pub use alacritty_terminal;
