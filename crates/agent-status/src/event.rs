//! Parsing of the JSON Claude Code writes to a hook's stdin.

use std::path::PathBuf;

use serde::Deserialize;

/// Hook events the sidebar cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookKind {
    SessionStart,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    Notification,
    Stop,
    SubagentStop,
    SessionEnd,
    /// Any other event, kept by name.
    Other(String),
}

impl From<&str> for HookKind {
    fn from(name: &str) -> Self {
        match name {
            "SessionStart" => Self::SessionStart,
            "UserPromptSubmit" => Self::UserPromptSubmit,
            "PreToolUse" => Self::PreToolUse,
            "PostToolUse" => Self::PostToolUse,
            "Notification" => Self::Notification,
            "Stop" => Self::Stop,
            "SubagentStop" => Self::SubagentStop,
            "SessionEnd" => Self::SessionEnd,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// One hook invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookEvent {
    pub kind: HookKind,
    pub session_id: String,
    pub cwd: Option<PathBuf>,
    /// Text the user submitted (`UserPromptSubmit`).
    pub prompt: Option<String>,
    /// Tool about to run (`PreToolUse`), e.g. `Bash`, `Edit`.
    pub tool: Option<String>,
    /// What the tool acts on: a file path or a command.
    pub tool_target: Option<String>,
    /// Notification text (`Notification`).
    pub message: Option<String>,
    /// The session's transcript file (`~/.claude/projects/.../<id>.jsonl`).
    pub transcript_path: Option<PathBuf>,
}

/// The hook payload is not valid JSON or misses `hook_event_name`.
#[derive(Debug, thiserror::Error)]
#[error("invalid hook payload: {0}")]
pub struct ParseError(#[from] serde_json::Error);

#[derive(Deserialize)]
struct RawEvent {
    hook_event_name: String,
    #[serde(default)]
    session_id: String,
    cwd: Option<PathBuf>,
    prompt: Option<String>,
    tool_name: Option<String>,
    tool_input: Option<RawToolInput>,
    message: Option<String>,
    transcript_path: Option<PathBuf>,
}

#[derive(Deserialize)]
struct RawToolInput {
    file_path: Option<String>,
    command: Option<String>,
    pattern: Option<String>,
    description: Option<String>,
}

impl HookEvent {
    /// Parses the JSON a hook receives on stdin. Unknown fields are ignored.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when the payload is not JSON or lacks
    /// `hook_event_name`.
    pub fn parse(json: &str) -> Result<Self, ParseError> {
        let raw: RawEvent = serde_json::from_str(json)?;
        let tool_target = raw.tool_input.and_then(|input| {
            input
                .file_path
                .or(input.command)
                .or(input.pattern)
                .or(input.description)
        });
        Ok(Self {
            kind: HookKind::from(raw.hook_event_name.as_str()),
            session_id: raw.session_id,
            cwd: raw.cwd,
            prompt: raw.prompt,
            tool: raw.tool_name,
            tool_target,
            message: raw.message,
            transcript_path: raw.transcript_path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prompt_submit() {
        let event = HookEvent::parse(
            r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1",
                "cwd":"/home/u/proj","prompt":"fix the logos","transcript_path":"/x"}"#,
        )
        .unwrap();
        assert_eq!(event.kind, HookKind::UserPromptSubmit);
        assert_eq!(event.session_id, "s1");
        assert_eq!(event.cwd, Some(PathBuf::from("/home/u/proj")));
        assert_eq!(event.prompt.as_deref(), Some("fix the logos"));
        assert_eq!(event.transcript_path, Some(PathBuf::from("/x")));
    }

    #[test]
    fn parses_tool_target_preferring_file_path() {
        let event = HookEvent::parse(
            r#"{"hook_event_name":"PreToolUse","session_id":"s1","tool_name":"Edit",
                "tool_input":{"file_path":"src/main.rs","old_string":"a"}}"#,
        )
        .unwrap();
        assert_eq!(event.tool.as_deref(), Some("Edit"));
        assert_eq!(event.tool_target.as_deref(), Some("src/main.rs"));

        let bash = HookEvent::parse(
            r#"{"hook_event_name":"PreToolUse","session_id":"s1","tool_name":"Bash",
                "tool_input":{"command":"cargo test"}}"#,
        )
        .unwrap();
        assert_eq!(bash.tool_target.as_deref(), Some("cargo test"));
    }

    #[test]
    fn keeps_unknown_event_names() {
        let event = HookEvent::parse(r#"{"hook_event_name":"PreCompact"}"#).unwrap();
        assert_eq!(event.kind, HookKind::Other("PreCompact".into()));
    }

    #[test]
    fn rejects_payload_without_event_name() {
        assert!(HookEvent::parse(r#"{"session_id":"s1"}"#).is_err());
        assert!(HookEvent::parse("not json").is_err());
    }
}
