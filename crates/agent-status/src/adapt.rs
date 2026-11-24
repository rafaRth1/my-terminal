//! Hook payloads of other agent CLIs, reshaped into the Claude Code shape
//! the parser reads (`hook_event_name`, `session_id`, `cwd`, ...).

use serde_json::{Map, Value, json};

/// Which CLI ran the hook (`my-terminal-hook --agent <name>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Claude Code and the CLIs that copy its payload: Codex, Qwen Code
    /// and the `OpenCode` plugin.
    Claude,
    /// Antigravity CLI: camelCase fields.
    Agy,
}

impl Source {
    /// From the `--agent` value; unknown names read as Claude-shaped.
    #[must_use]
    pub fn from_arg(name: &str) -> Self {
        match name {
            "agy" | "antigravity" => Self::Agy,
            _ => Self::Claude,
        }
    }
}

/// `hook` in the Claude Code shape, or `None` when it is not an object.
#[must_use]
pub fn to_claude(source: Source, hook: Value) -> Option<Value> {
    let Value::Object(mut fields) = hook else {
        return None;
    };
    match source {
        Source::Claude => {}
        Source::Agy => fields = agy(&fields),
    }
    Some(Value::Object(fields))
}

/// An Antigravity payload (camelCase) in the Claude Code shape.
fn agy(fields: &Map<String, Value>) -> Map<String, Value> {
    let text = |key: &str| fields.get(key).and_then(Value::as_str);
    let event = match text("hookEventName").unwrap_or_default() {
        "PreInvocation" => "UserPromptSubmit",
        // The turn goes on after a model call.
        "PostInvocation" => "PostToolUse",
        other => other,
    };
    let cwd = text("cwd").or_else(|| {
        fields
            .get("workspacePaths")
            .and_then(Value::as_array)
            .and_then(|paths| paths.first())
            .and_then(Value::as_str)
    });
    let tool = fields
        .get("toolCall")
        .and_then(|call| call.get("name"))
        .and_then(Value::as_str);
    let mut out = Map::new();
    out.insert("hook_event_name".into(), json!(event));
    out.insert(
        "session_id".into(),
        json!(text("conversationId").unwrap_or_default()),
    );
    for (key, value) in [
        ("cwd", cwd),
        ("transcript_path", text("transcriptPath")),
        ("prompt", text("lastUserInput")),
        ("tool_name", tool),
    ] {
        if let Some(value) = value {
            out.insert(key.into(), json!(value));
        }
    }
    out
}

/// What the hook prints on stdout for the CLI that ran it.
#[must_use]
pub fn reply(source: Source, hook: &Value) -> Option<String> {
    if source != Source::Agy {
        return None;
    }
    // Stop requires a decision; anything but "continue" lets the agent stop.
    let stop = hook.get("hookEventName").and_then(Value::as_str) == Some("Stop");
    Some(
        if stop {
            r#"{"decision":"allow"}"#
        } else {
            "{}"
        }
        .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_from_arg() {
        assert_eq!(Source::from_arg("gemini"), Source::Claude);
        assert_eq!(Source::from_arg("agy"), Source::Agy);
        assert_eq!(Source::from_arg("antigravity"), Source::Agy);
        assert_eq!(Source::from_arg("codex"), Source::Claude);
        assert_eq!(Source::from_arg(""), Source::Claude);
    }

    #[test]
    fn claude_shaped_payloads_pass_through() {
        let hook = json!({"hook_event_name": "PermissionRequest", "session_id": "s", "cwd": "/p"});
        assert_eq!(to_claude(Source::Claude, hook.clone()), Some(hook));
        assert_eq!(to_claude(Source::Claude, json!("text")), None);
    }

    #[test]
    fn agy_camel_case_is_reshaped() {
        let hook = json!({
            "hookEventName": "PreInvocation",
            "conversationId": "c-1",
            "workspacePaths": ["/w/project"],
            "transcriptPath": "/t/c-1.jsonl",
            "lastUserInput": "fix it",
        });
        let out = to_claude(Source::Agy, hook).unwrap();
        assert_eq!(out["hook_event_name"], "UserPromptSubmit");
        assert_eq!(out["session_id"], "c-1");
        assert_eq!(out["cwd"], "/w/project");
        assert_eq!(out["transcript_path"], "/t/c-1.jsonl");
        assert_eq!(out["prompt"], "fix it");

        let tool = json!({
            "hookEventName": "PostToolUse",
            "conversationId": "c-1",
            "cwd": "/w/other",
            "workspacePaths": ["/w/project"],
            "toolCall": {"name": "run_command", "args": {"CommandLine": "ls"}},
        });
        let out = to_claude(Source::Agy, tool).unwrap();
        assert_eq!(out["hook_event_name"], "PostToolUse");
        assert_eq!(out["cwd"], "/w/other", "cwd wins over workspacePaths");
        assert_eq!(out["tool_name"], "run_command");

        let names = [
            ("SessionStart", "SessionStart"),
            ("PostInvocation", "PostToolUse"),
            ("Stop", "Stop"),
            ("Something", "Something"),
        ];
        for (agy, claude) in names {
            let out = to_claude(
                Source::Agy,
                json!({"hookEventName": agy, "conversationId": "c"}),
            );
            assert_eq!(out.unwrap()["hook_event_name"], claude, "{agy}");
        }
    }

    #[test]
    fn only_agy_gets_a_reply_and_stop_never_continues() {
        let stop = json!({"hookEventName": "Stop"});
        assert_eq!(
            reply(Source::Agy, &stop).as_deref(),
            Some(r#"{"decision":"allow"}"#)
        );
        let other = json!({"hookEventName": "PreInvocation"});
        assert_eq!(reply(Source::Agy, &other).as_deref(), Some("{}"));
        assert_eq!(reply(Source::Claude, &stop), None);
    }
}
