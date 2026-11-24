//! End-to-end tests: real PTY, real `/bin/sh`.
//!
//! Programs that print and exit within milliseconds can lose their last bytes
//! on Linux (the PTY reports `EIO` before the kernel flushes its buffer). The
//! tests therefore keep the program alive with `read _` until the output is
//! seen, then press Enter to let it exit.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use term_core::alacritty_terminal::grid::Dimensions;
use term_core::{Session, SessionConfig, TerminalEvent, TerminalSize};

const TIMEOUT: Duration = Duration::from_secs(5);

/// A running session plus its event stream.
struct Running {
    session: Session,
    events: Receiver<TerminalEvent>,
}

impl Running {
    fn start(config: SessionConfig) -> Self {
        let (session, events) =
            Session::spawn(config, TerminalSize::default()).expect("spawn session");
        Self { session, events }
    }

    /// Waits until the program exits and returns its exit code.
    fn wait_exit(&self) -> Option<i32> {
        let deadline = Instant::now() + TIMEOUT;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            if let Ok(TerminalEvent::Exited(code)) = self.events.recv_timeout(left) {
                return code;
            }
        }
        panic!(
            "program did not exit within {TIMEOUT:?}; screen:\n{}",
            self.session.screen_text()
        );
    }

    /// Waits until the visible screen contains `needle`.
    fn wait_screen(&self, needle: &str) {
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            if self.session.screen_text().contains(needle) {
                return;
            }
            let _ = self.events.recv_timeout(Duration::from_millis(50));
        }
        panic!(
            "screen never contained {needle:?}; screen:\n{}",
            self.session.screen_text()
        );
    }

    /// Presses Enter so the program exits, then checks its exit code.
    fn finish(&self, expected: Option<i32>) {
        // The program may already have exited; a closed session is fine here.
        let _ = self.session.write(&b"\r"[..]);
        assert_eq!(self.wait_exit(), expected);
    }
}

fn sh(script: &str) -> SessionConfig {
    SessionConfig {
        program: Some(("/bin/sh".into(), vec!["-c".into(), script.into()])),
        ..SessionConfig::default()
    }
}

#[test]
fn renders_program_output_on_screen() {
    let run = Running::start(sh("printf 'hello from pty'; read _"));
    run.wait_screen("hello from pty");
    run.finish(Some(0));
}

#[test]
fn reports_program_exit_code() {
    let run = Running::start(sh("read _; exit 3"));
    run.session.write(&b"\r"[..]).expect("write input");
    assert_eq!(run.wait_exit(), Some(3));
}

#[test]
fn forwards_keyboard_input_to_program() {
    let run = Running::start(sh("read line; echo \"got:$line\"; read _"));
    run.session.write(&b"abc\r"[..]).expect("write input");
    run.wait_screen("got:abc");
    run.finish(Some(0));
}

#[test]
fn advertises_color_terminal_to_programs() {
    let run = Running::start(sh("printf '%s %s' \"$TERM\" \"$COLORTERM\"; read _"));
    run.wait_screen("xterm-256color truecolor");
    run.finish(Some(0));
}

#[test]
fn resize_updates_grid_and_program_size() {
    // The program waits for input so the resize lands before `stty size`.
    let run = Running::start(sh("read _; stty size; read _"));
    run.session
        .resize(TerminalSize::new(100, 30, 8, 16))
        .expect("resize");
    {
        let term = run.session.term().lock();
        assert_eq!((term.columns(), term.screen_lines()), (100, 30));
    }
    run.session.write(&b"\r"[..]).expect("write input");
    run.wait_screen("30 100");
    run.finish(Some(0));
}

#[test]
fn custom_environment_reaches_program() {
    let mut config = sh("printf '%s' \"$CLAUDE_CONFIG_DIR\"; read _");
    config
        .env
        .insert("CLAUDE_CONFIG_DIR".into(), "/tmp/account-b".into());
    let run = Running::start(config);
    run.wait_screen("/tmp/account-b");
    run.finish(Some(0));
}

#[test]
fn reports_working_directory_of_program() {
    let dir = std::env::temp_dir().canonicalize().expect("temp dir");
    let run = Running::start(SessionConfig {
        working_directory: Some(dir.clone()),
        ..sh("printf ready; read _")
    });
    run.wait_screen("ready");
    assert_eq!(run.session.working_directory(), Some(dir));
    run.finish(Some(0));
}

#[test]
fn config_debug_hides_env_values() {
    let mut config = SessionConfig::default();
    config
        .env
        .insert("CLAUDE_CODE_OAUTH_TOKEN".into(), "secret-value".into());
    let printed = format!("{config:?}");
    assert!(printed.contains("CLAUDE_CODE_OAUTH_TOKEN"), "{printed}");
    assert!(!printed.contains("secret-value"), "{printed}");
}

#[test]
fn env_remove_hides_variables_from_the_program() {
    let mut config = sh("printf '[%s][%s]' \"${MT_DROP-unset}\" \"$MT_KEEP\"; read _");
    config.env.insert("MT_DROP".into(), "x".into());
    config.env.insert("MT_KEEP".into(), "kept".into());
    config.env_remove = vec!["MT_DROP".into()];
    let run = Running::start(config);
    run.wait_screen("[unset][kept]");
    run.finish(Some(0));
}
