use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::event_loop::{EventLoopSender, Msg};

/// Events a [`crate::Session`] reports to the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// New content is available; the UI should redraw.
    Wakeup,
    /// The running program set the window title.
    Title(String),
    /// The running program reset the window title.
    ResetTitle,
    /// The running program rang the bell.
    Bell,
    /// The program asked to copy text to the clipboard.
    ClipboardStore(String),
    /// The child process exited; `None` when it was killed by a signal.
    ///
    /// Emitted after the program's last output is on screen.
    Exited(Option<i32>),
}

/// Bridges `alacritty_terminal` events to a channel. Created only by [`crate::Session`].
///
/// Replies the terminal must send back to the PTY (`PtyWrite`) are written
/// directly, so the UI never has to handle protocol details.
#[derive(Clone)]
pub struct ChannelListener {
    events: Sender<TerminalEvent>,
    pty: Arc<OnceLock<EventLoopSender>>,
    /// `alacritty_terminal` reports `ChildExit` before draining the last
    /// output; the exit is held until the next `Wakeup`.
    pending_exit: Arc<Mutex<Option<TerminalEvent>>>,
}

impl ChannelListener {
    pub(crate) fn new(events: Sender<TerminalEvent>) -> Self {
        Self {
            events,
            pty: Arc::new(OnceLock::new()),
            pending_exit: Arc::new(Mutex::new(None)),
        }
    }

    /// Connects the listener to the PTY once the event loop exists.
    pub(crate) fn attach_pty(&self, sender: EventLoopSender) {
        // A second attach is a programming error but harmless: keep the first.
        let _ = self.pty.set(sender);
    }

    fn take_pending_exit(&self) -> Option<TerminalEvent> {
        self.pending_exit
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    fn emit(&self, event: TerminalEvent) {
        // The receiver is gone only when the session was dropped.
        let _ = self.events.send(event);
    }
}

impl std::fmt::Debug for ChannelListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelListener")
            .field("pty_attached", &self.pty.get().is_some())
            .finish_non_exhaustive()
    }
}

impl EventListener for ChannelListener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => {
                self.emit(TerminalEvent::Wakeup);
                if let Some(exited) = self.take_pending_exit() {
                    self.emit(exited);
                }
            }
            Event::Title(title) => self.emit(TerminalEvent::Title(title)),
            Event::ResetTitle => self.emit(TerminalEvent::ResetTitle),
            Event::Bell => self.emit(TerminalEvent::Bell),
            Event::ClipboardStore(_, text) => self.emit(TerminalEvent::ClipboardStore(text)),
            Event::ChildExit(status) => {
                *self
                    .pending_exit
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(TerminalEvent::Exited(status.code()));
            }
            Event::PtyWrite(text) => {
                if let Some(pty) = self.pty.get() {
                    let _ = pty.send(Msg::Input(text.into_bytes().into()));
                }
            }
            // Not needed yet: clipboard load, color and size queries, cursor
            // shape changes. Handled when the UI supports them.
            _ => {}
        }
    }
}
