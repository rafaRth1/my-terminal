//! Agent state machine fed by hook events.

use std::path::PathBuf;
use std::time::Instant;

use crate::event::{HookEvent, HookKind};

/// Maximum length of texts shown in the sidebar.
const MAX_TEXT: usize = 60;

/// What the agent is doing, as shown in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    /// Session started, no prompt yet.
    Idle,
    /// Processing a prompt or running tools.
    Working,
    /// Needs the user: permission request or idle input.
    Waiting,
    /// Finished answering.
    Done,
}

impl AgentStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Working => "Working",
            Self::Waiting => "Waiting",
            Self::Done => "Done",
        }
    }
}

/// State of the Claude Code session running in one pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentState {
    pub status: AgentStatus,
    pub session_id: String,
    pub cwd: Option<PathBuf>,
    /// First prompt of the session: what the session is about.
    pub topic: Option<String>,
    /// Transcript file reported by the hooks.
    pub transcript: Option<PathBuf>,
    /// Latest activity: current tool, notification text.
    pub activity: Option<String>,
    /// When `status` last changed.
    pub since: Instant,
}

impl AgentState {
    /// Starts tracking a session from its first event.
    #[must_use]
    pub fn new(event: &HookEvent, now: Instant) -> Self {
        let mut state = Self {
            status: AgentStatus::Idle,
            session_id: event.session_id.clone(),
            cwd: event.cwd.clone(),
            topic: None,
            transcript: None,
            activity: None,
            since: now,
        };
        state.apply(event, now);
        state
    }

    /// Folds an event into the state.
    ///
    /// Returns `false` when the session ended and the state should be dropped.
    pub fn apply(&mut self, event: &HookEvent, now: Instant) -> bool {
        if event.session_id != self.session_id {
            // A new session in the same pane (`/clear`, restart): start over.
            *self = Self::new_empty(event, now);
        }
        if event.cwd.is_some() {
            self.cwd.clone_from(&event.cwd);
        }
        if event.transcript_path.is_some() {
            self.transcript.clone_from(&event.transcript_path);
        }

        let next = match &event.kind {
            HookKind::SessionEnd => return false,
            HookKind::SessionStart => AgentStatus::Idle,
            HookKind::UserPromptSubmit => {
                if let Some(prompt) = event.prompt.as_deref() {
                    if self.topic.is_none() {
                        self.topic = Some(summarize(prompt));
                    }
                    self.activity = Some(summarize(prompt));
                }
                AgentStatus::Working
            }
            HookKind::PreToolUse => {
                self.activity =
                    event
                        .tool
                        .as_deref()
                        .map(|tool| match event.tool_target.as_deref() {
                            Some(target) => summarize(&format!("{tool} {target}")),
                            None => tool.to_owned(),
                        });
                AgentStatus::Working
            }
            HookKind::PostToolUse | HookKind::SubagentStop => AgentStatus::Working,
            HookKind::Notification => {
                self.activity = event.message.as_deref().map(summarize);
                AgentStatus::Waiting
            }
            HookKind::PermissionRequest => AgentStatus::Waiting,
            HookKind::Stop | HookKind::Interrupt | HookKind::StopFailure => AgentStatus::Done,
            HookKind::Other(_) => self.status,
        };

        if next != self.status {
            self.status = next;
            self.since = now;
        }
        true
    }

    fn new_empty(event: &HookEvent, now: Instant) -> Self {
        Self {
            status: AgentStatus::Idle,
            session_id: event.session_id.clone(),
            cwd: event.cwd.clone(),
            topic: None,
            transcript: None,
            activity: None,
            since: now,
        }
    }
}

/// First line of `text`, cut to [`MAX_TEXT`] characters with an ellipsis.
fn summarize(text: &str) -> String {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    if line.chars().count() <= MAX_TEXT {
        line.to_owned()
    } else {
        let cut: String = line.chars().take(MAX_TEXT - 1).collect();
        format!("{}…", cut.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn event(kind: HookKind) -> HookEvent {
        HookEvent {
            kind,
            session_id: "s1".into(),
            cwd: Some("/p".into()),
            prompt: None,
            tool: None,
            tool_target: None,
            message: None,
            transcript_path: None,
        }
    }

    #[test]
    fn prompt_sets_topic_once_and_working() {
        let now = Instant::now();
        let mut state = AgentState::new(&event(HookKind::SessionStart), now);
        assert_eq!(state.status, AgentStatus::Idle);

        let first = HookEvent {
            prompt: Some("Logos que no se muestran\nmore".into()),
            ..event(HookKind::UserPromptSubmit)
        };
        assert!(state.apply(&first, now));
        let second = HookEvent {
            prompt: Some("otra cosa".into()),
            ..event(HookKind::UserPromptSubmit)
        };
        state.apply(&second, now);
        assert_eq!(state.status, AgentStatus::Working);
        assert_eq!(state.topic.as_deref(), Some("Logos que no se muestran"));
        assert_eq!(state.activity.as_deref(), Some("otra cosa"));
    }

    #[test]
    fn lifecycle_working_waiting_done() {
        let start = Instant::now();
        let later = start + Duration::from_secs(5);
        let mut state = AgentState::new(&event(HookKind::UserPromptSubmit), start);

        let tool = HookEvent {
            tool: Some("Edit".into()),
            tool_target: Some("src/vision.rs".into()),
            ..event(HookKind::PreToolUse)
        };
        state.apply(&tool, later);
        assert_eq!(state.activity.as_deref(), Some("Edit src/vision.rs"));
        assert_eq!(state.since, start, "status unchanged keeps its start time");

        let ask = HookEvent {
            message: Some("Claude needs your permission".into()),
            ..event(HookKind::Notification)
        };
        state.apply(&ask, later);
        assert_eq!(state.status, AgentStatus::Waiting);
        assert_eq!(state.since, later);

        state.apply(&event(HookKind::Stop), later);
        assert_eq!(state.status, AgentStatus::Done);
    }

    #[test]
    fn session_end_drops_state() {
        let now = Instant::now();
        let mut state = AgentState::new(&event(HookKind::SessionStart), now);
        assert!(!state.apply(&event(HookKind::SessionEnd), now));
    }

    #[test]
    fn new_session_id_resets_topic() {
        let now = Instant::now();
        let mut state = AgentState::new(
            &HookEvent {
                prompt: Some("old topic".into()),
                ..event(HookKind::UserPromptSubmit)
            },
            now,
        );
        let fresh = HookEvent {
            session_id: "s2".into(),
            ..event(HookKind::SessionStart)
        };
        state.apply(&fresh, now);
        assert_eq!(state.session_id, "s2");
        assert_eq!(state.topic, None);
        assert_eq!(state.status, AgentStatus::Idle);
    }

    #[test]
    fn summarize_cuts_long_text() {
        let long = "a".repeat(100);
        let short = summarize(&long);
        assert_eq!(short.chars().count(), MAX_TEXT);
        assert!(short.ends_with('…'));
        assert_eq!(summarize("\n\n  hola  \nmundo"), "hola");
    }

    #[test]
    fn keeps_the_latest_transcript_path() {
        let now = Instant::now();
        let mut start = event(HookKind::SessionStart);
        start.transcript_path = Some("/t/s1.jsonl".into());
        let mut state = AgentState::new(&start, now);
        assert_eq!(state.transcript, Some(PathBuf::from("/t/s1.jsonl")));
        // Events without a path keep the known one.
        state.apply(&event(HookKind::Stop), now);
        assert_eq!(state.transcript, Some(PathBuf::from("/t/s1.jsonl")));
    }

    #[test]
    fn permission_requests_wait_and_interrupts_finish() {
        let now = Instant::now();
        let mut state = AgentState::new(&event(HookKind::UserPromptSubmit), now);
        assert_eq!(state.status, AgentStatus::Working);
        state.apply(&event(HookKind::from("PermissionRequest")), now);
        assert_eq!(state.status, AgentStatus::Waiting);
        state.apply(&event(HookKind::from("Interrupt")), now);
        assert_eq!(state.status, AgentStatus::Done);
        state.apply(&event(HookKind::UserPromptSubmit), now);
        state.apply(&event(HookKind::from("StopFailure")), now);
        assert_eq!(state.status, AgentStatus::Done);
    }
}
