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
    run_hook_with(&[], PAYLOAD, env)
}

/// Runs the hook with `args` and `env`, feeding `payload`.
fn run_hook_with(
    args: &[&str],
    payload: &str,
    env: &[(&str, &str)],
) -> (Option<i32>, String, String) {
    let mut command = Command::new(HOOK);
    command
        .args(args)
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
        .write_all(payload.as_bytes())
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

#[test]
fn antigravity_always_gets_a_reply_that_lets_it_stop() {
    let stop = r#"{"hookEventName":"Stop","conversationId":"c-1","fullyIdle":true}"#;
    let (code, stdout, stderr) = run_hook_with(&["--agent", "agy"], stop, &[]);
    assert_eq!(
        (code, stdout.as_str(), stderr.as_str()),
        (Some(0), r#"{"decision":"allow"}"#, "")
    );
}

#[test]
fn antigravity_events_reach_the_app_reshaped() {
    let socket = std::env::temp_dir().join(format!("hook-agy-{}.sock", std::process::id()));
    let (_server, events) = HookServer::bind(&socket).unwrap();
    let payload = r#"{"hookEventName":"PreInvocation","conversationId":"c-1","workspacePaths":["/w"],"lastUserInput":"fix it"}"#;
    let (code, stdout, _) = run_hook_with(
        &["--agent", "agy"],
        payload,
        &[(SOCKET_ENV, socket.to_str().unwrap()), (PANE_ENV, "7")],
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "{}"));
    let received = events.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(received.pane, 7);
    assert_eq!(received.event.kind, HookKind::UserPromptSubmit);
    assert_eq!(received.event.session_id, "c-1");
    assert_eq!(received.event.prompt.as_deref(), Some("fix it"));
}

#[test]
fn non_antigravity_sources_never_print() {
    let arg_sets: [&[&str]; 4] = [
        &[],
        &["--agent", "qwen"],
        &["--agent", "codex"],
        &["--agent"],
    ];
    for args in arg_sets {
        for payload in ["", "garbage", r#"{"hook_event_name":"Stop"}"#] {
            let (code, stdout, stderr) = run_hook_with(args, payload, &[]);
            assert_eq!(
                (code, stdout.as_str(), stderr.as_str()),
                (Some(0), "", ""),
                "{args:?} {payload}"
            );
        }
    }
}

#[test]
fn antigravity_replies_even_to_malformed_input() {
    let (code, stdout, _) = run_hook_with(&["--agent", "agy"], "not json", &[]);
    assert_eq!((code, stdout.as_str()), (Some(0), "{}"));
}
