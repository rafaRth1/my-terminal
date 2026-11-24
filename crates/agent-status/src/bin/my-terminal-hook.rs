//! Agent hook command: forwards the hook payload to my-terminal.
//!
//! Configure it for the hook events of Claude Code (`~/.claude/settings.json`),
//! Codex (`~/.codex/hooks.json`) or Qwen Code (`~/.qwen/settings.json`) as is,
//! and with `--agent agy` for Antigravity CLI, whose payloads are reshaped
//! first. Outside my-terminal (no `MY_TERMINAL_*` variables) nothing is
//! forwarded. It always exits 0. It prints nothing, except for Antigravity,
//! which requires a JSON reply: `{}`, or `{"decision":"allow"}` for `Stop`,
//! which lets the agent stop.

use std::io::{Read as _, Write as _};
use std::path::PathBuf;

use agent_status::adapt::{self, Source};
use agent_status::{PANE_ENV, SOCKET_ENV, ipc};

/// Payloads larger than this are not forwarded.
const MAX_PAYLOAD: u64 = 256 * 1024;

fn main() {
    let source = agent_arg().map_or(Source::Claude, |name| Source::from_arg(&name));

    let mut payload = String::new();
    let read = std::io::stdin()
        .take(MAX_PAYLOAD)
        .read_to_string(&mut payload);
    let hook: serde_json::Value = serde_json::from_str(&payload).unwrap_or_default();

    // The reply comes first: the CLI may wait for it even outside my-terminal.
    if let Some(reply) = adapt::reply(source, &hook) {
        let _ = std::io::stdout().write_all(reply.as_bytes());
    }

    let (Some(socket), Some(pane)) = (
        std::env::var_os(SOCKET_ENV).map(PathBuf::from),
        std::env::var(PANE_ENV)
            .ok()
            .and_then(|p| p.parse::<u64>().ok()),
    ) else {
        return;
    };
    if read.is_err() {
        return;
    }
    let Some(shaped) = adapt::to_claude(source, hook) else {
        return;
    };
    // The app may have closed; a hook must never fail because of us.
    let _ = ipc::send(&socket, pane, &shaped.to_string());
}

/// The value of `--agent <name>` (or `--agent=<name>`), if given.
fn agent_arg() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--agent" {
            return args.next();
        }
        if let Some(name) = arg.strip_prefix("--agent=") {
            return Some(name.to_owned());
        }
    }
    None
}
