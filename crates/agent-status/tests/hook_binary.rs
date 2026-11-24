//! Runs the real `my-terminal-hook` binary.

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::time::Duration;

use agent_status::ipc::HookServer;
use agent_status::{HookKind, PANE_ENV, SOCKET_ENV};

const HOOK: &str = env!("CARGO_BIN_EXE_my-terminal-hook");
const PAYLOAD: &str =
    r#"{"hook_event_name":"Notification","session_id":"s1","message":"needs permission"}"#;

/// Runs the hook with `env`, feeding `PAYLOAD`; returns (exit code, stdout, stderr).
fn run_hook(env: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let mut command = Command::new(HOOK);
    command
        .env_remove(PANE_ENV)
        .env_remove(SOCKET_ENV)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn hook");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(PAYLOAD.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn forwards_payload_to_app_inside_my_terminal() {
    let socket = std::env::temp_dir().join(format!("hook-bin-{}.sock", std::process::id()));
    let (_server, events) = HookServer::bind(&socket).unwrap();

    let (code, stdout, stderr) =
        run_hook(&[(SOCKET_ENV, socket.to_str().unwrap()), (PANE_ENV, "42")]);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (Some(0), "", ""));

    let received = events.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(received.pane, 42);
    assert_eq!(received.event.kind, HookKind::Notification);
    assert_eq!(received.event.message.as_deref(), Some("needs permission"));
}

#[test]
fn does_nothing_outside_my_terminal() {
    let (code, stdout, stderr) = run_hook(&[]);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (Some(0), "", ""));
}

#[test]
fn exits_cleanly_when_app_is_gone() {
    let socket = std::env::temp_dir().join(format!("hook-gone-{}.sock", std::process::id()));
    let (code, stdout, stderr) =
        run_hook(&[(SOCKET_ENV, socket.to_str().unwrap()), (PANE_ENV, "1")]);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (Some(0), "", ""));
}
