//! Sessions saved between runs. The open sessions are written to a small
//! JSON file whenever they change; at start they come back as dormant
//! sidebar rows that start nothing until clicked.

use std::fs;
use std::io::{self, Write as _};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agents::{AGENTS, Agent};

/// Most sessions kept in the file; older ones are dropped.
pub const MAX_SESSIONS: usize = 200;
/// Longest title kept, in characters.
const MAX_TITLE_CHARS: usize = 200;
/// Longest Claude session id accepted.
const MAX_SESSION_ID: usize = 128;
const FILE_NAME: &str = "sessions.json";

/// One session as written to disk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSession {
    pub title: String,
    #[serde(default)]
    pub dir: Option<PathBuf>,
    /// Id of the agent that was running ([`Agent::id`]).
    #[serde(default)]
    pub agent: Option<String>,
    /// Claude Code session id, for `claude --resume`.
    #[serde(default)]
    pub claude_session: Option<String>,
    /// Account name; `None` for the default login.
    #[serde(default)]
    pub account: Option<String>,
}

/// Everything in the file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedState {
    #[serde(default)]
    pub sessions: Vec<SavedSession>,
    /// Projects whose sidebar group is collapsed.
    #[serde(default)]
    pub collapsed: Vec<String>,
}

/// `$XDG_STATE_HOME/my-terminal/sessions.json`, defaulting to
/// `~/.local/state/my-terminal/sessions.json`.
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    Some(base.join("my-terminal").join(FILE_NAME))
}

/// Reads the file. A missing file is an empty state. A file that cannot be
/// parsed is renamed to `sessions.json.corrupt` (so the next save does not
/// destroy it) and the reason is returned with an empty state.
pub fn load(path: &Path) -> (SavedState, Option<String>) {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return (SavedState::default(), None),
        Err(err) => {
            return (
                SavedState::default(),
                Some(format!("{}: {err}", path.display())),
            );
        }
    };
    match serde_json::from_str::<SavedState>(&text) {
        Ok(state) => (clean(state), None),
        Err(err) => {
            let mut corrupt = path.as_os_str().to_owned();
            corrupt.push(".corrupt");
            let moved = fs::rename(path, &corrupt).map_or_else(
                |rename| format!(" (could not move it aside: {rename})"),
                |()| format!(" (moved to {})", Path::new(&corrupt).display()),
            );
            (
                SavedState::default(),
                Some(format!("{}: {err}{moved}", path.display())),
            )
        }
    }
}

/// Writes `state` to `path` (mode 0600, directory 0700) through a temporary
/// file, so a crash never leaves a half-written file.
pub fn save(path: &Path, state: &SavedState) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent directory"))?;
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    let text = serde_json::to_string_pretty(state).map_err(io::Error::other)?;
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(".tmp-{}", std::process::id()));
    let temp = PathBuf::from(temp);
    let written = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)
        .and_then(|mut file| {
            file.write_all(text.as_bytes())?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&temp, path));
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// Drops what the app would never write: too many sessions, long titles,
/// unknown agents and session ids that are not plain identifiers.
fn clean(mut state: SavedState) -> SavedState {
    state.sessions.truncate(MAX_SESSIONS);
    for session in &mut state.sessions {
        session.title = session.title.chars().take(MAX_TITLE_CHARS).collect();
        if session.agent.as_deref().and_then(agent).is_none() {
            session.agent = None;
        }
        if !session
            .claude_session
            .as_deref()
            .is_some_and(valid_session_id)
        {
            session.claude_session = None;
        }
    }
    state
}

fn agent(id: &str) -> Option<&'static Agent> {
    AGENTS.iter().find(|agent| agent.id == id)
}

/// Letters, digits, `-` and `_` only: the id is typed into a shell.
fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_SESSION_ID
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Command typed into the new shell to bring the session back: Claude Code
/// resumes its conversation when the session id is known; other agents
/// start fresh; plain shells run nothing.
pub fn resume_command(session: &SavedSession) -> Option<String> {
    let agent = agent(session.agent.as_deref()?)?;
    match session.claude_session.as_deref() {
        Some(id) if agent.id == "claude" && valid_session_id(id) => {
            Some(format!("{} --resume {id}", agent.program))
        }
        _ => Some(agent.program.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "my-terminal-saved-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn claude(id: Option<&str>) -> SavedSession {
        SavedSession {
            title: "Fix login".into(),
            dir: Some("/home/u/project".into()),
            agent: Some("claude".into()),
            claude_session: id.map(str::to_owned),
            account: Some("work".into()),
        }
    }

    #[test]
    fn save_then_load_round_trips_privately() {
        let dir = temp_dir();
        let path = dir.join("state").join(FILE_NAME);
        let state = SavedState {
            sessions: vec![claude(Some("0b1c-2d")), SavedSession::default()],
            collapsed: vec!["midterm".into()],
        };
        save(&path, &state).unwrap();
        assert_eq!(load(&path), (state, None));
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        // No temporary file is left behind.
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = temp_dir();
        assert_eq!(load(&dir.join(FILE_NAME)), (SavedState::default(), None));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = temp_dir();
        let path = dir.join(FILE_NAME);
        fs::write(&path, "{not json").unwrap();
        let (state, error) = load(&path);
        assert_eq!(state, SavedState::default());
        assert!(error.unwrap().contains("sessions.json.corrupt"));
        assert!(!path.exists());
        assert_eq!(
            fs::read_to_string(dir.join("sessions.json.corrupt")).unwrap(),
            "{not json"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_drops_what_the_app_never_writes() {
        let dir = temp_dir();
        let path = dir.join(FILE_NAME);
        let mut sessions: Vec<SavedSession> = (0..MAX_SESSIONS + 5)
            .map(|_| SavedSession::default())
            .collect();
        sessions[0] = SavedSession {
            title: "x".repeat(MAX_TITLE_CHARS + 50),
            agent: Some("rm -rf".into()),
            claude_session: Some("id; rm -rf ~".into()),
            ..SavedSession::default()
        };
        let text = serde_json::to_string(&SavedState {
            sessions,
            collapsed: Vec::new(),
        })
        .unwrap();
        fs::write(&path, text).unwrap();
        let (state, error) = load(&path);
        assert_eq!(error, None);
        assert_eq!(state.sessions.len(), MAX_SESSIONS);
        let first = &state.sessions[0];
        assert_eq!(first.title.chars().count(), MAX_TITLE_CHARS);
        assert_eq!(first.agent, None);
        assert_eq!(first.claude_session, None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn resume_command_per_agent() {
        assert_eq!(
            resume_command(&claude(Some("0b1c-2d_e"))).as_deref(),
            Some("claude --resume 0b1c-2d_e")
        );
        // Without a usable id Claude starts a new conversation.
        assert_eq!(resume_command(&claude(None)).as_deref(), Some("claude"));
        assert_eq!(
            resume_command(&claude(Some("x; rm -rf ~"))).as_deref(),
            Some("claude")
        );
        let codex = SavedSession {
            agent: Some("codex".into()),
            claude_session: Some("ignored".into()),
            ..SavedSession::default()
        };
        assert_eq!(resume_command(&codex).as_deref(), Some("codex"));
        let shell = SavedSession::default();
        assert_eq!(resume_command(&shell), None);
        let unknown = SavedSession {
            agent: Some("evil".into()),
            ..SavedSession::default()
        };
        assert_eq!(resume_command(&unknown), None);
    }
}
