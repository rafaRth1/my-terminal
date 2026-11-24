//! Title of a Claude Code session, read from its transcript: the latest
//! `custom-title` entry, the same title Claude's own session lists show.
//! The file is read incrementally: each poll reads only what was appended.

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Most bytes read in one poll; a larger backlog starts from its tail.
const MAX_READ: u64 = 8 * 1024 * 1024;
/// Longest title kept, in characters.
const MAX_TITLE_CHARS: usize = 120;

/// Follows one transcript file.
#[derive(Debug)]
pub struct TitleReader {
    path: PathBuf,
    /// Bytes already scanned (always at a line start).
    offset: u64,
    title: Option<String>,
}

#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
}

impl TitleReader {
    /// `None` unless `path` names a `.jsonl` file: the path comes from a
    /// hook message, so anything else is not a transcript.
    pub fn new(path: &Path) -> Option<Self> {
        if path.extension().is_none_or(|ext| ext != "jsonl") {
            return None;
        }
        Some(Self {
            path: path.to_owned(),
            offset: 0,
            title: None,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Reads what was appended since the last poll. Returns `true` when the
    /// title changed. Errors (missing file, not a regular file) change
    /// nothing; the next poll tries again.
    pub fn poll(&mut self) -> bool {
        let Ok(mut file) = File::open(&self.path) else {
            return false;
        };
        let Ok(meta) = file.metadata() else {
            return false;
        };
        if !meta.is_file() {
            return false;
        }
        let len = meta.len();
        if len < self.offset {
            // Rewritten or truncated: start over.
            self.offset = 0;
        }
        let start = self.offset.max(len.saturating_sub(MAX_READ));
        if start >= len || file.seek(SeekFrom::Start(start)).is_err() {
            return false;
        }
        let mut bytes = Vec::new();
        if file.take(len - start).read_to_end(&mut bytes).is_err() {
            return false;
        }
        // Only whole lines; a line still being written waits for next time.
        let Some(end) = bytes.iter().rposition(|&b| b == b'\n') else {
            return false;
        };
        // A tail read may start mid-line: skip to the first full line.
        let first = if start == self.offset {
            0
        } else {
            bytes
                .iter()
                .position(|&b| b == b'\n')
                .map_or(end, |i| i + 1)
        };
        self.offset = start + end as u64 + 1;
        let found = bytes[first..=end]
            .rsplit(|&b| b == b'\n')
            .find_map(custom_title);
        match found {
            Some(title) if self.title.as_deref() != Some(title.as_str()) => {
                self.title = Some(title);
                true
            }
            _ => false,
        }
    }
}

/// The title of a `custom-title` line, trimmed and cut; other lines are
/// skipped without a full JSON parse.
fn custom_title(line: &[u8]) -> Option<String> {
    const MARKER: &[u8] = b"\"custom-title\"";
    if !line.windows(MARKER.len()).any(|w| w == MARKER) {
        return None;
    }
    let entry: Entry = serde_json::from_slice(line).ok()?;
    if entry.kind != "custom-title" {
        return None;
    }
    let title: String = entry
        .custom_title?
        .lines()
        .find(|l| !l.trim().is_empty())?
        .trim()
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect();
    Some(title)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::Write as _;

    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "my-terminal-transcript-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("s1.jsonl")
    }

    fn append(path: &Path, text: &str) {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    fn title_line(title: &str) -> String {
        format!(
            "{}\n",
            serde_json::json!({"type": "custom-title", "customTitle": title, "sessionId": "s1"})
        )
    }

    #[test]
    fn follows_the_latest_custom_title() {
        let path = temp_file("latest");
        append(&path, "{\"type\":\"user\",\"message\":\"hi\"}\n");
        let mut reader = TitleReader::new(&path).unwrap();
        assert!(!reader.poll());
        assert_eq!(reader.title(), None);

        append(&path, &title_line("Fix login"));
        append(&path, "{\"type\":\"assistant\"}\n");
        assert!(reader.poll());
        assert_eq!(reader.title(), Some("Fix login"));
        // Nothing new: no change.
        assert!(!reader.poll());

        append(&path, &title_line("Fix login and logout"));
        assert!(reader.poll());
        assert_eq!(reader.title(), Some("Fix login and logout"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn waits_for_a_line_to_be_complete() {
        let path = temp_file("partial");
        let line = title_line("Half written");
        let (head, tail) = line.split_at(20);
        append(&path, head);
        let mut reader = TitleReader::new(&path).unwrap();
        assert!(!reader.poll());
        append(&path, tail);
        assert!(reader.poll());
        assert_eq!(reader.title(), Some("Half written"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn ignores_other_lines_and_bad_titles() {
        let path = temp_file("other");
        append(
            &path,
            "{\"type\":\"user\",\"text\":\"\\\"custom-title\\\"\"}\n",
        );
        append(&path, "not json \"custom-title\"\n");
        append(&path, &title_line("   \n"));
        let mut reader = TitleReader::new(&path).unwrap();
        assert!(!reader.poll());
        assert_eq!(reader.title(), None);
        append(&path, &title_line(&"x".repeat(MAX_TITLE_CHARS + 30)));
        assert!(reader.poll());
        assert_eq!(reader.title().unwrap().chars().count(), MAX_TITLE_CHARS);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn only_jsonl_files_and_missing_files_are_harmless() {
        assert!(TitleReader::new(Path::new("/etc/passwd")).is_none());
        assert!(TitleReader::new(Path::new("/dev/zero")).is_none());
        let mut missing = TitleReader::new(Path::new("/nonexistent/x.jsonl")).unwrap();
        assert!(!missing.poll());
    }
}
