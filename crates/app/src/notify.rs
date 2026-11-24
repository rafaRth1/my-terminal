//! Desktop notifications for Claude Code sessions that need attention while
//! the window is in the background, sent with `notify-send`.

use agent_status::AgentStatus;

/// Longest title sent, in characters.
const MAX_TITLE_CHARS: usize = 80;

/// Body of the notification for a status change, or `None` when there is
/// nothing to tell: the window is active, or the status did not change to
/// waiting or done.
pub fn message(
    previous: Option<AgentStatus>,
    next: AgentStatus,
    window_active: bool,
) -> Option<&'static str> {
    if window_active || previous == Some(next) {
        return None;
    }
    match next {
        AgentStatus::Waiting => Some("Waiting for you"),
        AgentStatus::Done => Some("Done"),
        AgentStatus::Idle | AgentStatus::Working => None,
    }
}

/// Cuts `text` and escapes the markup some notification servers read.
pub fn escape(text: &str) -> String {
    text.chars()
        .take(MAX_TITLE_CHARS)
        .fold(String::new(), |mut out, c| {
            match c {
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '&' => out.push_str("&amp;"),
                c => out.push(c),
            }
            out
        })
}

/// Shows a notification without blocking: `notify-send` runs in the
/// background and a thread reaps it. Missing `notify-send` is ignored.
pub fn send(summary: &str, body: &str) {
    let child = std::process::Command::new("notify-send")
        .args([
            "--app-name=my-terminal",
            "--icon=utilities-terminal",
            "--",
            &escape(summary),
            &escape(body),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Ok(mut child) = child {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_only_new_waiting_or_done_in_the_background() {
        use AgentStatus::{Done, Idle, Waiting, Working};
        assert_eq!(
            message(Some(Working), Waiting, false),
            Some("Waiting for you")
        );
        assert_eq!(message(Some(Working), Done, false), Some("Done"));
        assert_eq!(message(None, Waiting, false), Some("Waiting for you"));
        // Same status again: already told.
        assert_eq!(message(Some(Waiting), Waiting, false), None);
        assert_eq!(message(Some(Done), Done, false), None);
        // Other statuses never notify.
        assert_eq!(message(Some(Done), Working, false), None);
        assert_eq!(message(Some(Working), Idle, false), None);
        // The window is in front: the sidebar already shows it.
        assert_eq!(message(Some(Working), Waiting, true), None);
    }

    #[test]
    fn escapes_markup_and_cuts_long_text() {
        assert_eq!(escape("a <b> & c"), "a &lt;b&gt; &amp; c");
        assert_eq!(escape(&"x".repeat(200)).chars().count(), MAX_TITLE_CHARS);
    }
}
