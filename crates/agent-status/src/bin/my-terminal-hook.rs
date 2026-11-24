//! Claude Code hook command: forwards the hook payload to my-terminal.
//!
//! Configure it for any hook event in `~/.claude/settings.json`. Outside
//! my-terminal (no `MY_TERMINAL_*` variables) it does nothing. It always
//! exits 0 and prints nothing, so it can never block or alter Claude Code.

use std::io::Read as _;
use std::path::PathBuf;

use agent_status::{PANE_ENV, SOCKET_ENV, ipc};

/// Payloads larger than this are not forwarded.
const MAX_PAYLOAD: u64 = 256 * 1024;

fn main() {
    let (Some(socket), Some(pane)) = (
        std::env::var_os(SOCKET_ENV).map(PathBuf::from),
        std::env::var(PANE_ENV)
            .ok()
            .and_then(|p| p.parse::<u64>().ok()),
    ) else {
        return;
    };

    let mut payload = String::new();
    if std::io::stdin()
        .take(MAX_PAYLOAD)
        .read_to_string(&mut payload)
        .is_err()
    {
        return;
    }
    // The app may have closed; a hook must never fail because of us.
    let _ = ipc::send(&socket, pane, &payload);
}
