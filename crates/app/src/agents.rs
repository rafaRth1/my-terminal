//! Coding-agent CLIs shown in the sidebar's agent bar.
//!
//! my-terminal never reads or stores agent credentials. It asks each CLI for
//! its own login status, or checks that its credentials file exists, and
//! signs in by typing the CLI's own login command into a new tab.

use std::ffi::OsStr;
use std::io::Read as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Longest a status command may run before it is killed.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Whether an agent has saved credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignIn {
    SignedIn,
    SignedOut,
    /// The status command failed or printed something unexpected.
    Unknown,
}

/// What a click on an agent icon does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// Start the agent in a new tab.
    Open,
    /// Run its login command in a new tab.
    SignIn,
    /// Open its install or repair instructions in the browser.
    InstallPage,
    /// Show the menu with every action.
    Menu,
    /// Show the accounts dialog, where an account is picked and opened.
    Accounts,
}

/// How to learn an agent's sign-in state without touching its secrets.
#[derive(Debug)]
enum Probe {
    /// Run the program with these arguments and parse what it prints.
    Command(&'static [&'static str], fn(bool, &str) -> SignIn),
    /// Signed in when this file exists under `$HOME`, or this variable is set.
    CredentialsFile {
        path: &'static str,
        env: Option<&'static str>,
    },
    /// No way to ask: the CLI signs in during its own onboarding.
    None,
}

/// One supported agent CLI.
#[derive(Debug)]
pub struct Agent {
    pub id: &'static str,
    pub name: &'static str,
    /// Embedded monochrome SVG, tinted with [`Agent::color`].
    pub icon: &'static str,
    /// Icon color, `0xRRGGBB`.
    pub color: u32,
    pub program: &'static str,
    /// Typed into a new tab to sign in.
    pub login: &'static str,
    /// Shown in the tooltip when signed out.
    pub login_hint: &'static str,
    /// Official install instructions, also used to repair an installation.
    pub install_url: &'static str,
    /// Has token accounts: a click opens the accounts dialog.
    pub accounts: bool,
    probe: Probe,
}

/// Agents in the order the bar shows them.
pub const AGENTS: [Agent; 6] = [
    Agent {
        id: "claude",
        name: "Claude Code",
        icon: "icons/claude.svg",
        color: 0x00d9_7757,
        program: "claude",
        install_url: "https://code.claude.com/docs/en/setup",
        login: "claude auth login",
        login_hint: "sign in with `claude auth login`",
        accounts: true,
        probe: Probe::Command(&["auth", "status"], parse_claude),
    },
    Agent {
        id: "codex",
        name: "Codex",
        icon: "icons/openai.svg",
        color: 0x00e6_e6e6,
        program: "codex",
        install_url: "https://github.com/openai/codex",
        login: "codex login",
        login_hint: "sign in with `codex login`",
        accounts: false,
        probe: Probe::Command(&["login", "status"], parse_codex),
    },
    Agent {
        id: "gemini",
        name: "Gemini CLI",
        icon: "icons/googlegemini.svg",
        color: 0x0042_85f4,
        program: "gemini",
        install_url: "https://github.com/google-gemini/gemini-cli",
        login: "gemini",
        login_hint: "gemini asks you to sign in when it starts (or type /auth)",
        accounts: false,
        probe: Probe::CredentialsFile {
            path: ".gemini/oauth_creds.json",
            env: Some("GEMINI_API_KEY"),
        },
    },
    Agent {
        id: "qwen",
        name: "Qwen Code",
        icon: "icons/qwen.svg",
        color: 0x0061_5ced,
        program: "qwen",
        install_url: "https://github.com/QwenLM/qwen-code",
        login: "qwen",
        login_hint: "type /auth inside qwen to sign in",
        accounts: false,
        probe: Probe::CredentialsFile {
            path: ".qwen/oauth_creds.json",
            env: None,
        },
    },
    Agent {
        id: "opencode",
        name: "OpenCode",
        icon: "icons/opencode.svg",
        color: 0x00f2_a65a,
        program: "opencode",
        install_url: "https://opencode.ai/docs",
        login: "opencode auth login",
        login_hint: "sign in with `opencode auth login`",
        accounts: false,
        probe: Probe::Command(&["auth", "list"], parse_opencode),
    },
    Agent {
        id: "antigravity",
        name: "Antigravity CLI",
        icon: "icons/antigravity.svg",
        color: 0x0042_85f4,
        program: "agy",
        install_url: "https://antigravity.google/docs/getting-started",
        login: "agy",
        login_hint: "sign in during agy's first-run onboarding",
        accounts: false,
        probe: Probe::None,
    },
];

/// An agent and what the last probe found.
#[derive(Debug, Clone, Copy)]
pub struct AgentStatus {
    pub agent: &'static Agent,
    pub installed: bool,
    pub sign_in: SignIn,
}

/// Finds and asks every agent. Blocking: runs status commands, so call it
/// off the UI thread.
pub fn probe_all(path: &OsStr, home: Option<&Path>) -> Vec<AgentStatus> {
    AGENTS
        .iter()
        .map(|agent| {
            let Some(program) = find_program(agent.program, path) else {
                return AgentStatus {
                    agent,
                    installed: false,
                    sign_in: SignIn::Unknown,
                };
            };
            let sign_in = match &agent.probe {
                Probe::Command(args, parse) => {
                    run_with_timeout(&program.to_string_lossy(), args, PROBE_TIMEOUT)
                        .map_or(SignIn::Unknown, |(ok, out)| parse(ok, &out))
                }
                Probe::None => SignIn::Unknown,
                Probe::CredentialsFile { path, env } => {
                    if env.is_some_and(|var| std::env::var_os(var).is_some_and(|v| !v.is_empty())) {
                        SignIn::SignedIn
                    } else {
                        home.map_or(SignIn::Unknown, |home| file_state(home, path))
                    }
                }
            };
            AgentStatus {
                agent,
                installed: true,
                sign_in,
            }
        })
        .collect()
}

/// A missing agent opens its install page. Otherwise right click shows the
/// menu; left click opens the accounts dialog of an agent with accounts, and
/// otherwise opens a signed-in (or unknown) agent or signs in a signed-out one.
pub fn click_action(status: AgentStatus, right_click: bool) -> Click {
    if !status.installed {
        Click::InstallPage
    } else if right_click {
        Click::Menu
    } else if status.agent.accounts {
        Click::Accounts
    } else if status.sign_in == SignIn::SignedOut {
        Click::SignIn
    } else {
        Click::Open
    }
}

/// Entries of the right-click menu, in order.
pub fn menu_items(installed: bool) -> Vec<Click> {
    if installed {
        vec![Click::Open, Click::SignIn, Click::InstallPage]
    } else {
        vec![Click::InstallPage]
    }
}

/// Programs that run an agent's script as their first argument.
const INTERPRETERS: [&str; 4] = ["node", "bun", "deno", "python3"];
/// How deep below the shell to look for an agent.
const MAX_TREE_DEPTH: usize = 4;

/// The agent a command line runs: its program name, or the script name when
/// the program is an interpreter (`node .../bin/gemini`).
pub fn agent_for_command(args: &[String]) -> Option<&'static Agent> {
    let base = |arg: &String| {
        let name = Path::new(arg).file_name()?.to_str()?;
        Some(
            name.trim_end_matches(".js")
                .trim_end_matches(".mjs")
                .trim_end_matches(".cjs")
                .to_owned(),
        )
    };
    let program = base(args.first()?)?;
    let name = if INTERPRETERS.contains(&program.as_str()) {
        base(args.get(1)?)?
    } else {
        program
    };
    AGENTS.iter().find(|agent| agent.program == name)
}

/// The first agent found below `root` (a pane's shell), closest first.
/// Reads `/proc`, so it is Linux only; `None` elsewhere or when nothing runs.
pub fn agent_in_process_tree(root: u32) -> Option<&'static Agent> {
    let mut level = children(root);
    for _ in 0..MAX_TREE_DEPTH {
        if level.is_empty() {
            return None;
        }
        if let Some(agent) = level
            .iter()
            .find_map(|pid| agent_for_command(&cmdline(*pid)))
        {
            return Some(agent);
        }
        level = level.iter().flat_map(|pid| children(*pid)).collect();
    }
    None
}

fn children(pid: u32) -> Vec<u32> {
    let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return Vec::new();
    };
    tasks
        .filter_map(Result::ok)
        .filter_map(|task| std::fs::read_to_string(task.path().join("children")).ok())
        .flat_map(|list| {
            list.split_whitespace()
                .filter_map(|pid| pid.parse().ok())
                .collect::<Vec<u32>>()
        })
        .collect()
}

fn cmdline(pid: u32) -> Vec<String> {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|bytes| {
            bytes
                .split(|b| *b == 0)
                .filter(|arg| !arg.is_empty())
                .map(|arg| String::from_utf8_lossy(arg).into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// `claude auth status` prints JSON with a `loggedIn` boolean.
fn parse_claude(success: bool, output: &str) -> SignIn {
    if !success {
        return SignIn::Unknown;
    }
    let Some(after) = output.split_once("\"loggedIn\"").map(|(_, rest)| rest) else {
        return SignIn::Unknown;
    };
    let value = after.trim_start().strip_prefix(':').map(str::trim_start);
    match value {
        Some(v) if v.starts_with("true") => SignIn::SignedIn,
        Some(v) if v.starts_with("false") => SignIn::SignedOut,
        _ => SignIn::Unknown,
    }
}

/// `codex login status` prints "Logged in using ..." or "Not logged in".
fn parse_codex(success: bool, output: &str) -> SignIn {
    if output.contains("Not logged in") {
        SignIn::SignedOut
    } else if success && output.contains("Logged in") {
        SignIn::SignedIn
    } else {
        SignIn::Unknown
    }
}

/// `opencode auth list` ends with "N credentials".
fn parse_opencode(success: bool, output: &str) -> SignIn {
    if !success {
        return SignIn::Unknown;
    }
    let count = output.lines().find_map(|line| {
        let words: Vec<&str> = line.split_whitespace().collect();
        words
            .windows(2)
            .find(|pair| pair[1].starts_with("credential"))
            .and_then(|pair| pair[0].parse::<u32>().ok())
    });
    match count {
        Some(0) => SignIn::SignedOut,
        Some(_) => SignIn::SignedIn,
        None => SignIn::Unknown,
    }
}

fn file_state(home: &Path, relative: &str) -> SignIn {
    if home.join(relative).is_file() {
        SignIn::SignedIn
    } else {
        SignIn::SignedOut
    }
}

/// First executable file named `program` in the directories of `path`.
fn find_program(program: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(program))
        .find(|candidate| {
            std::fs::metadata(candidate)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

/// Runs a command with no input and returns whether it succeeded and what it
/// printed (stdout, then stderr). `None` if it cannot start or outlives
/// `timeout`, in which case it is killed.
fn run_with_timeout(program: &str, args: &[&str], timeout: Duration) -> Option<(bool, String)> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let readers = [
        child.stdout.take().map(|pipe| read_all(Box::new(pipe))),
        child.stderr.take().map(|pipe| read_all(Box::new(pipe))),
    ];
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(POLL_INTERVAL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let output: String = readers
        .into_iter()
        .flatten()
        .map(|reader| reader.join().unwrap_or_default())
        .collect();
    status.map(|status| (status.success(), output))
}

/// Drains a pipe on its own thread so a chatty command cannot block on it.
fn read_all(mut pipe: Box<dyn std::io::Read + Send>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// Set by Claude Code in every process it starts.
const CLAUDE_CODE_MARKER: &str = "CLAUDECODE";
/// Inherited variables that stay even when the app runs inside Claude Code:
/// user settings, not session state.
const KEEP_CLAUDE_VARS: [&str; 2] = ["CLAUDE_CONFIG_DIR", crate::accounts::TOKEN_ENV];

/// Variables the app inherited from a Claude Code session that started it
/// (for example `CLAUDE_CODE_CHILD_SESSION`, which turns transcript saving
/// off): `CLAUDECODE` and every `CLAUDE_*` except [`KEEP_CLAUDE_VARS`].
/// Empty when the app was not started by Claude Code. Sorted.
pub fn parent_session_vars(keys: impl IntoIterator<Item = String>) -> Vec<String> {
    let keys: Vec<String> = keys.into_iter().collect();
    if !keys.iter().any(|key| key == CLAUDE_CODE_MARKER) {
        return Vec::new();
    }
    let mut strip: Vec<String> = keys
        .into_iter()
        .filter(|key| {
            (key == CLAUDE_CODE_MARKER || key.starts_with("CLAUDE_"))
                && !KEEP_CLAUDE_VARS.contains(&key.as_str())
        })
        .collect();
    strip.sort();
    strip
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "my-terminal-agents-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self, name: &str, mode: u32) -> PathBuf {
            let path = self.0.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, "x").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn claude_status_reads_logged_in_from_json() {
        let out = r#"{ "loggedIn": true, "authMethod": "claude.ai" }"#;
        assert_eq!(parse_claude(true, out), SignIn::SignedIn);
        assert_eq!(
            parse_claude(true, r#"{"loggedIn": false}"#),
            SignIn::SignedOut
        );
        assert_eq!(parse_claude(false, "error: boom"), SignIn::Unknown);
        assert_eq!(parse_claude(true, "not json"), SignIn::Unknown);
    }

    #[test]
    fn codex_status_reads_its_message() {
        assert_eq!(
            parse_codex(true, "Logged in using ChatGPT\n"),
            SignIn::SignedIn
        );
        assert_eq!(parse_codex(false, "Not logged in\n"), SignIn::SignedOut);
        assert_eq!(parse_codex(true, "Not logged in\n"), SignIn::SignedOut);
        assert_eq!(parse_codex(false, "panic"), SignIn::Unknown);
    }

    #[test]
    fn opencode_status_counts_credentials() {
        let none = "┌  Credentials ~/.local/share/opencode/auth.json\n│\n└  0 credentials\n";
        assert_eq!(parse_opencode(true, none), SignIn::SignedOut);
        assert_eq!(parse_opencode(true, "└  2 credentials\n"), SignIn::SignedIn);
        assert_eq!(parse_opencode(true, "└  1 credential\n"), SignIn::SignedIn);
        assert_eq!(parse_opencode(true, "something else"), SignIn::Unknown);
        assert_eq!(parse_opencode(false, "└  0 credentials"), SignIn::Unknown);
    }

    #[test]
    fn credentials_file_presence_decides() {
        let home = TempDir::new();
        assert_eq!(
            file_state(&home.0, ".gemini/oauth_creds.json"),
            SignIn::SignedOut
        );
        home.file(".gemini/oauth_creds.json", 0o600);
        assert_eq!(
            file_state(&home.0, ".gemini/oauth_creds.json"),
            SignIn::SignedIn
        );
    }

    #[test]
    fn find_program_needs_an_executable_file_on_path() {
        let a = TempDir::new();
        let b = TempDir::new();
        a.file("tool", 0o644);
        let wanted = b.file("tool", 0o755);
        fs::create_dir(a.0.join("dir-tool")).unwrap();
        let path = std::env::join_paths([&a.0, &b.0]).unwrap();

        assert_eq!(find_program("tool", &path), Some(wanted));
        assert_eq!(find_program("dir-tool", &path), None);
        assert_eq!(find_program("missing", &path), None);
        assert_eq!(find_program("tool", std::ffi::OsStr::new("")), None);
    }

    #[test]
    fn click_opens_signs_in_or_installs() {
        let codex = AGENTS.iter().find(|agent| agent.id == "codex").unwrap();
        let installed = |sign_in| AgentStatus {
            agent: codex,
            installed: true,
            sign_in,
        };
        assert_eq!(
            click_action(installed(SignIn::SignedIn), false),
            Click::Open
        );
        assert_eq!(click_action(installed(SignIn::Unknown), false), Click::Open);
        assert_eq!(
            click_action(installed(SignIn::SignedOut), false),
            Click::SignIn
        );
        // Right click shows the menu (open, sign in, install page).
        assert_eq!(click_action(installed(SignIn::SignedIn), true), Click::Menu);

        let missing = AgentStatus {
            installed: false,
            ..installed(SignIn::Unknown)
        };
        assert_eq!(click_action(missing, false), Click::InstallPage);
        assert_eq!(click_action(missing, true), Click::InstallPage);
    }

    #[test]
    fn claude_click_opens_the_accounts_dialog() {
        let claude = AGENTS.iter().find(|agent| agent.id == "claude").unwrap();
        assert!(claude.accounts);
        assert_eq!(
            AGENTS.iter().filter(|agent| agent.accounts).count(),
            1,
            "only Claude has token accounts"
        );
        let status = |installed, sign_in| AgentStatus {
            agent: claude,
            installed,
            sign_in,
        };
        // Signed in or out: the dialog picks the account (and signs in).
        for sign_in in [SignIn::SignedIn, SignIn::SignedOut, SignIn::Unknown] {
            assert_eq!(click_action(status(true, sign_in), false), Click::Accounts);
        }
        assert_eq!(
            click_action(status(true, SignIn::SignedIn), true),
            Click::Menu
        );
        assert_eq!(
            click_action(status(false, SignIn::Unknown), false),
            Click::InstallPage
        );
    }

    #[test]
    fn menu_offers_every_action_for_installed_agents() {
        assert_eq!(
            menu_items(true),
            [Click::Open, Click::SignIn, Click::InstallPage]
        );
        assert_eq!(menu_items(false), [Click::InstallPage]);
    }

    #[test]
    fn run_with_timeout_captures_output_and_gives_up() {
        let ok = run_with_timeout(
            "sh",
            &["-c", "echo out; echo err >&2"],
            Duration::from_secs(5),
        );
        assert_eq!(ok, Some((true, "out\nerr\n".to_owned())));
        let failed = run_with_timeout("sh", &["-c", "exit 3"], Duration::from_secs(5));
        assert_eq!(failed, Some((false, String::new())));

        let started = std::time::Instant::now();
        assert_eq!(
            run_with_timeout("sleep", &["5"], Duration::from_millis(200)),
            None
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "slow command is killed"
        );
        assert_eq!(
            run_with_timeout("/nonexistent/agent", &[], Duration::from_secs(1)),
            None
        );
    }

    #[test]
    fn antigravity_is_found_without_a_status_probe() {
        let bin = TempDir::new();
        bin.file("agy", 0o755);
        let home = TempDir::new();
        let statuses = probe_all(bin.0.as_os_str(), Some(&home.0));
        let agy = statuses
            .iter()
            .find(|status| status.agent.id == "antigravity")
            .expect("antigravity listed");
        assert!(agy.installed);
        // agy signs in during its own onboarding and has no status command.
        assert_eq!(agy.sign_in, SignIn::Unknown);
        assert!(
            statuses
                .iter()
                .filter(|s| s.agent.id != "antigravity")
                .all(|s| !s.installed)
        );
    }

    fn ids(args: &[&str]) -> Option<&'static str> {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        agent_for_command(&args).map(|agent| agent.id)
    }

    #[test]
    fn command_line_names_the_agent() {
        assert_eq!(ids(&["claude"]), Some("claude"));
        assert_eq!(
            ids(&["/home/u/.local/bin/agy", "--continue"]),
            Some("antigravity")
        );
        assert_eq!(
            ids(&["node", "/x/lib/node_modules/.bin/gemini"]),
            Some("gemini")
        );
        assert_eq!(
            ids(&["/usr/bin/node", "/x/@openai/codex/bin/codex.js"]),
            Some("codex")
        );
        assert_eq!(ids(&["bun", "/x/bin/opencode.mjs"]), Some("opencode"));
        assert_eq!(ids(&["/usr/bin/bash"]), None);
        assert_eq!(ids(&["node", "/x/server.js"]), None);
        assert_eq!(
            ids(&["vim", "claude"]),
            None,
            "only argv0, or argv1 of an interpreter"
        );
        assert_eq!(ids(&[]), None);
    }

    #[test]
    fn process_tree_finds_an_agent_below_the_shell() {
        let mut shell = Command::new("bash")
            .args(["-c", "bash -c 'exec -a codex sleep 30' & wait"])
            .spawn()
            .unwrap();
        let root = shell.id();
        let started = Instant::now();
        let mut found = None;
        while found.is_none() && started.elapsed() < Duration::from_secs(3) {
            found = agent_in_process_tree(root).map(|agent| agent.id);
            std::thread::sleep(POLL_INTERVAL);
        }
        let _ = Command::new("pkill")
            .args(["-P", &root.to_string()])
            .status();
        let _ = shell.kill();
        let _ = shell.wait();
        assert_eq!(found, Some("codex"));
        assert!(agent_in_process_tree(u32::MAX).is_none(), "missing process");
    }

    #[test]
    fn agent_list_is_well_formed() {
        let mut ids: Vec<_> = AGENTS.iter().map(|agent| agent.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), AGENTS.len(), "unique ids");
        for agent in &AGENTS {
            assert!(
                !agent.program.is_empty() && !agent.login.is_empty(),
                "{}",
                agent.id
            );
            assert!(agent.install_url.starts_with("https://"), "{}", agent.id);
            assert!(
                agent.icon.starts_with("icons/")
                    && std::path::Path::new(agent.icon)
                        .extension()
                        .is_some_and(|ext| ext == "svg")
            );
        }
    }

    #[test]
    fn parent_session_vars_only_inside_claude_code() {
        let keys = |list: &[&str]| list.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            parent_session_vars(keys(&["CLAUDE_CODE_CHILD_SESSION", "PATH"])),
            Vec::<String>::new()
        );
        assert_eq!(
            parent_session_vars(keys(&[
                "PATH",
                "CLAUDE_PID",
                "CLAUDECODE",
                "CLAUDE_CODE_CHILD_SESSION",
                "CLAUDE_CONFIG_DIR",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "ANTHROPIC_API_KEY",
                "MY_CLAUDE",
            ])),
            ["CLAUDECODE", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_PID"]
        );
    }
}
