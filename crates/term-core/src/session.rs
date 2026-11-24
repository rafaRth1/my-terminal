use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{self, Term};
use alacritty_terminal::tty::{self, Shell};

use crate::error::SessionError;
use crate::event::{ChannelListener, TerminalEvent};
use crate::size::TerminalSize;

/// Shared handle to the terminal grid of a session.
pub type SharedTerm = Arc<FairMutex<Term<ChannelListener>>>;

/// How to start the program inside a [`Session`].
///
/// `Debug` prints only the environment keys: values may hold secrets such as
/// login tokens.
#[derive(Clone, Default)]
pub struct SessionConfig {
    /// Program and arguments; `None` runs the user's login shell.
    pub program: Option<(String, Vec<String>)>,
    /// Working directory; `None` inherits the app's directory.
    pub working_directory: Option<PathBuf>,
    /// Extra environment variables for the child.
    pub env: HashMap<String, String>,
    /// Variables the child must not see, even when the app has them. The
    /// program then starts through `env -u NAME ... --` (`env` replaces
    /// itself with the program, so the child pid stays the program's).
    pub env_remove: Vec<String>,
}

impl fmt::Debug for SessionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut env_keys: Vec<&String> = self.env.keys().collect();
        env_keys.sort();
        f.debug_struct("SessionConfig")
            .field("program", &self.program)
            .field("working_directory", &self.working_directory)
            .field("env_keys", &env_keys)
            .field("env_remove", &self.env_remove)
            .finish()
    }
}

/// A program running in a PTY, with its screen state.
///
/// Dropping the session shuts down its I/O thread.
pub struct Session {
    term: SharedTerm,
    pty: EventLoopSender,
    child_pid: u32,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("child_pid", &self.child_pid)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// Spawns the configured program in a new PTY.
    ///
    /// Returns the session and the receiver of its events. The receiver is
    /// separate so the UI can move it to its own thread or task.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::Spawn`] when the PTY or child cannot be
    /// created, and [`SessionError::EventLoop`] when the I/O loop fails.
    pub fn spawn(
        config: SessionConfig,
        size: TerminalSize,
    ) -> Result<(Self, Receiver<TerminalEvent>), SessionError> {
        let (events_tx, events) = mpsc::channel();
        let listener = ChannelListener::new(events_tx);

        let term = Arc::new(FairMutex::new(Term::new(
            term::Config::default(),
            &size,
            listener.clone(),
        )));

        let pty = tty::new(&pty_options(config), size.into(), 0).map_err(SessionError::Spawn)?;
        let child_pid = pty.child().id();

        let event_loop = EventLoop::new(Arc::clone(&term), listener.clone(), pty, false, false)
            .map_err(SessionError::EventLoop)?;
        let sender = event_loop.channel();
        listener.attach_pty(sender.clone());
        // The thread ends when the child exits or on `Msg::Shutdown`.
        let _io_thread = event_loop.spawn();

        let session = Self {
            term,
            pty: sender,
            child_pid,
        };
        Ok((session, events))
    }

    /// Sends bytes (keyboard input, paste) to the program.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::Closed`] when the I/O loop already stopped.
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) -> Result<(), SessionError> {
        self.send(Msg::Input(bytes.into()))
    }

    /// Resizes the grid and notifies the program (`SIGWINCH`).
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::Closed`] when the I/O loop already stopped.
    pub fn resize(&self, size: TerminalSize) -> Result<(), SessionError> {
        self.term.lock().resize(size);
        self.send(Msg::Resize(size.into()))
    }

    /// Shared handle to the grid, for rendering.
    #[must_use]
    pub fn term(&self) -> &SharedTerm {
        &self.term
    }

    /// Process id of the program started in the PTY.
    #[must_use]
    pub fn child_pid(&self) -> u32 {
        self.child_pid
    }

    /// Current directory of the program, read from `/proc` (Linux only).
    ///
    /// Returns `None` when the process is gone or the platform has no `/proc`.
    #[must_use]
    pub fn working_directory(&self) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{}/cwd", self.child_pid)).ok()
    }

    /// Visible screen as text, one line per row, trailing spaces trimmed.
    #[must_use]
    pub fn screen_text(&self) -> String {
        let term = self.term.lock();
        let last_column = Column(term.columns().saturating_sub(1));
        (0..term.screen_lines())
            .map(|row| {
                let line = Line(i32::try_from(row).unwrap_or(i32::MAX));
                let text = term
                    .bounds_to_string(Point::new(line, Column(0)), Point::new(line, last_column));
                text.trim_end().to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn send(&self, msg: Msg) -> Result<(), SessionError> {
        self.pty.send(msg).map_err(|_| SessionError::Closed)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.pty.send(Msg::Shutdown);
    }
}

fn pty_options(config: SessionConfig) -> tty::Options {
    let mut env = config.env;
    env.entry("TERM".into())
        .or_insert_with(|| "xterm-256color".into());
    env.entry("COLORTERM".into())
        .or_insert_with(|| "truecolor".into());

    let program = if config.env_remove.is_empty() {
        config.program
    } else {
        let (program, args) = config.program.unwrap_or_else(|| {
            // What alacritty runs by default: the user's shell, no arguments.
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            (shell, Vec::new())
        });
        let mut env_args: Vec<String> = config
            .env_remove
            .into_iter()
            .flat_map(|name| ["-u".to_owned(), name])
            .collect();
        env_args.push("--".into());
        env_args.push(program);
        env_args.extend(args);
        Some(("env".into(), env_args))
    };

    tty::Options {
        shell: program.map(|(program, args)| Shell::new(program, args)),
        working_directory: config.working_directory,
        drain_on_exit: true,
        env,
    }
}
