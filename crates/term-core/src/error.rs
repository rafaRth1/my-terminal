use std::io;

/// Errors raised while creating or driving a [`crate::Session`].
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The PTY or the child shell could not be created.
    #[error("failed to spawn PTY: {0}")]
    Spawn(#[source] io::Error),

    /// The I/O event loop could not be created.
    #[error("failed to start terminal event loop: {0}")]
    EventLoop(#[source] io::Error),

    /// The event loop already stopped, so the message was not delivered.
    #[error("terminal session is closed")]
    Closed,
}
