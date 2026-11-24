//! Unix socket between `my-terminal-hook` and the app.
//!
//! Protocol: one connection per hook event carrying one JSON line
//! `{"pane": <u64>, "hook": <raw hook JSON>}`.
//!
//! The socket lives in `$XDG_RUNTIME_DIR` (mode 0700, owned by the user), so
//! only the user's own processes can connect.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::event::HookEvent;

/// Largest message accepted, to bound memory per connection.
const MAX_MESSAGE: u64 = 256 * 1024;
const IO_TIMEOUT: Duration = Duration::from_millis(500);

/// A hook event tagged with the pane it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneEvent {
    pub pane: u64,
    pub event: HookEvent,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    pane: u64,
    hook: serde_json::Value,
}

/// Default socket path for the app with process id `pid`.
///
/// Uses `$XDG_RUNTIME_DIR`, falling back to the temp dir.
#[must_use]
pub fn default_socket_path(pid: u32) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    dir.join(format!("my-terminal-{pid}.sock"))
}

/// Removes sockets in `dir` left by app instances that are no longer running
/// (killed before they could clean up). Returns how many were removed.
///
/// Only files named `my-terminal-<pid>.sock` whose pid is not alive are
/// touched.
pub fn remove_stale_sockets(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let pid = name
                .to_str()
                .and_then(|n| n.strip_prefix("my-terminal-"))
                .and_then(|n| n.strip_suffix(".sock"))
                .and_then(|pid| pid.parse::<u32>().ok());
            pid.is_some_and(|pid| !Path::new(&format!("/proc/{pid}")).exists())
        })
        .filter(|entry| std::fs::remove_file(entry.path()).is_ok())
        .count()
}

/// Listens for hook events. Removes the socket file on drop.
#[derive(Debug)]
pub struct HookServer {
    path: PathBuf,
}

impl HookServer {
    /// Binds `path` and starts a thread that forwards events to the receiver.
    ///
    /// A stale socket file at `path` is replaced.
    ///
    /// # Errors
    ///
    /// Returns an error when the socket cannot be bound or the thread cannot
    /// start.
    pub fn bind(path: &Path) -> io::Result<(Self, Receiver<PaneEvent>)> {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        let listener = UnixListener::bind(path)?;
        let (tx, rx) = mpsc::channel();

        std::thread::Builder::new()
            .name("hook-server".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    if let Some(event) = read_event(stream) {
                        if tx.send(event).is_err() {
                            break; // The app is gone.
                        }
                    }
                }
            })?;

        Ok((
            Self {
                path: path.to_owned(),
            },
            rx,
        ))
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for HookServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Reads one message; malformed input is dropped silently.
fn read_event(stream: UnixStream) -> Option<PaneEvent> {
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok()?;
    let mut line = String::new();
    BufReader::new(stream.take(MAX_MESSAGE))
        .read_line(&mut line)
        .ok()?;
    let envelope: Envelope = serde_json::from_str(&line).ok()?;
    let event = HookEvent::parse(&envelope.hook.to_string()).ok()?;
    Some(PaneEvent {
        pane: envelope.pane,
        event,
    })
}

/// Sends one raw hook payload to the app at `socket`.
///
/// # Errors
///
/// Returns an error when the payload is not JSON or the app is unreachable.
pub fn send(socket: &Path, pane: u64, raw_hook: &str) -> io::Result<()> {
    let hook: serde_json::Value = serde_json::from_str(raw_hook)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let mut line = serde_json::to_string(&Envelope { pane, hook })
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    line.push('\n');

    let mut stream = UnixStream::connect(socket)?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    stream.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::HookKind;

    fn temp_socket(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("agent-status-{name}-{}.sock", std::process::id()))
    }

    #[test]
    fn delivers_events_from_send_to_server() {
        let path = temp_socket("deliver");
        let (server, events) = HookServer::bind(&path).unwrap();

        send(
            &path,
            7,
            r#"{"hook_event_name":"Stop","session_id":"s1","cwd":"/p"}"#,
        )
        .unwrap();

        let received = events.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(received.pane, 7);
        assert_eq!(received.event.kind, HookKind::Stop);
        drop(server);
        assert!(!path.exists(), "socket file removed on drop");
    }

    #[test]
    fn ignores_malformed_messages() {
        let path = temp_socket("malformed");
        let (_server, events) = HookServer::bind(&path).unwrap();

        UnixStream::connect(&path)
            .unwrap()
            .write_all(b"garbage\n")
            .unwrap();
        send(&path, 1, r#"{"hook_event_name":"Stop","session_id":"s"}"#).unwrap();

        let received = events.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(received.pane, 1, "garbage skipped, next event delivered");
    }

    #[test]
    fn rebinding_replaces_stale_socket() {
        let path = temp_socket("stale");
        let first = HookServer::bind(&path).unwrap();
        std::mem::forget(first); // Simulate a crash: file left behind.
        assert!(path.exists());
        let (_server, _events) = HookServer::bind(&path).unwrap();
    }

    #[test]
    fn removes_only_sockets_of_dead_processes() {
        let dir = std::env::temp_dir().join(format!("agent-status-stale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let alive = dir.join(format!("my-terminal-{}.sock", std::process::id()));
        // Pid far above the default pid_max (4194304): never alive.
        let dead = dir.join("my-terminal-4294967.sock");
        let unrelated = dir.join("other-1.sock");
        for path in [&alive, &dead, &unrelated] {
            std::fs::write(path, b"").unwrap();
        }

        assert_eq!(remove_stale_sockets(&dir), 1);
        assert!(alive.exists() && unrelated.exists() && !dead.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn send_fails_without_server() {
        let path = temp_socket("absent");
        assert!(send(&path, 1, r#"{"hook_event_name":"Stop"}"#).is_err());
    }
}
