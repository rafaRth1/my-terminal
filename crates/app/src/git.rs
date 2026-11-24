//! Read-only view of a git repository for the git panel: branch, changed
//! files and diffs, from the system `git` command.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Branch and changed files of a work tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoStatus {
    /// Branch name, or `(detached)`.
    pub branch: String,
    pub upstream: Option<String>,
    /// Commits ahead of and behind the upstream.
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<FileChange>,
}

/// One changed path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    /// `M` modified, `A` added, `D` deleted, `R` renamed, `U` conflict,
    /// `?` untracked.
    pub status: char,
    /// Previous path of a rename.
    pub old_path: Option<String>,
    /// Lines added and removed against `HEAD`; `None` for binary or
    /// untracked files.
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

/// What a diff line is, for coloring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// `diff --git`, `index`, `---`, `+++` and similar.
    Header,
    /// `@@ -1,2 +1,3 @@`.
    Hunk,
    Added,
    Removed,
    Context,
}

/// Parses `git status --porcelain=v2 --branch -z`.
pub fn parse_status(out: &[u8]) -> RepoStatus {
    let mut status = RepoStatus::default();
    let mut entries = out
        .split(|&b| b == 0)
        .map(String::from_utf8_lossy)
        .filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        if let Some(header) = entry.strip_prefix("# ") {
            read_header(&mut status, header);
            continue;
        }
        let (kind, rest) = entry.split_at(1);
        let rest = rest.trim_start();
        let change = match kind {
            "1" => field(rest, 6).map(|(xy, path)| change(letter(xy), path, None)),
            "2" => field(rest, 7).map(|(_, path)| {
                let old = entries.next().map(std::borrow::Cow::into_owned);
                change('R', path, old)
            }),
            "u" => field(rest, 8).map(|(_, path)| change('U', path, None)),
            "?" => Some(change('?', rest, None)),
            _ => None,
        };
        status.files.extend(change);
    }
    status
}

fn read_header(status: &mut RepoStatus, header: &str) {
    let Some((key, value)) = header.split_once(' ') else {
        return;
    };
    match key {
        "branch.head" => value.clone_into(&mut status.branch),
        "branch.upstream" => status.upstream = Some(value.to_owned()),
        "branch.ab" => {
            for part in value.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    status.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    status.behind = n.parse().unwrap_or(0);
                }
            }
        }
        _ => {}
    }
}

/// The `XY` code and the path of an entry whose path follows `skip` more
/// space-separated fields after `XY` (paths may hold spaces).
fn field(rest: &str, skip: usize) -> Option<(&str, &str)> {
    let mut parts = rest.splitn(skip + 2, ' ');
    let xy = parts.next()?;
    let path = parts.nth(skip)?;
    Some((xy, path))
}

/// One letter for an `XY` code: conflicts, deletions, additions and renames
/// win over plain modifications.
fn letter(xy: &str) -> char {
    for wanted in ['U', 'D', 'A', 'R'] {
        if xy.contains(wanted) {
            return wanted;
        }
    }
    'M'
}

fn change(status: char, path: &str, old_path: Option<String>) -> FileChange {
    FileChange {
        path: path.to_owned(),
        status,
        old_path,
        added: None,
        removed: None,
    }
}

/// Adds the line counts of `git diff HEAD --numstat -z` to `status`.
pub fn apply_numstat(status: &mut RepoStatus, out: &[u8]) {
    let mut entries = out.split(|&b| b == 0).map(String::from_utf8_lossy);
    while let Some(entry) = entries.next() {
        let mut parts = entry.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        // A rename has an empty path, then the old and the new path.
        let path = if path.is_empty() {
            let _old = entries.next();
            entries
                .next()
                .map(std::borrow::Cow::into_owned)
                .unwrap_or_default()
        } else {
            path.to_owned()
        };
        if let Some(file) = status.files.iter_mut().find(|f| f.path == path) {
            file.added = added.parse().ok();
            file.removed = removed.parse().ok();
        }
    }
}

/// Splits a unified diff into colored lines.
pub fn parse_diff(text: &str) -> Vec<(LineKind, String)> {
    let mut in_hunk = false;
    text.lines()
        .map(|line| {
            let kind = if line.starts_with("@@") {
                in_hunk = true;
                LineKind::Hunk
            } else if line.starts_with("diff ") {
                in_hunk = false;
                LineKind::Header
            } else if !in_hunk || line.starts_with('\\') {
                LineKind::Header
            } else if line.starts_with('+') {
                LineKind::Added
            } else if line.starts_with('-') {
                LineKind::Removed
            } else {
                LineKind::Context
            };
            (kind, line.to_owned())
        })
        .collect()
}

/// Most diff lines shown; the rest of a huge diff is cut.
pub const MAX_DIFF_LINES: usize = 5000;

/// Top folder of the work tree holding `dir`, or `None` outside git.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    let out = git(dir, &["rev-parse", "--show-toplevel"]).ok()?;
    let root = String::from_utf8_lossy(&out).trim().to_owned();
    (!root.is_empty()).then(|| PathBuf::from(root))
}

/// Branch and changed files (with line counts) of the work tree at `root`.
pub fn status(root: &Path) -> Result<RepoStatus, String> {
    let out = git(root, &["status", "--porcelain=v2", "--branch", "-z"])?;
    let mut status = parse_status(&out);
    // A repository without commits has no HEAD to diff against.
    if let Ok(numstat) = git(root, &["diff", "HEAD", "--numstat", "-z", "-M"]) {
        apply_numstat(&mut status, &numstat);
    }
    Ok(status)
}

/// Diff of `file` against `HEAD`; an untracked file shows as all added.
pub fn diff(root: &Path, file: &FileChange) -> Result<Vec<(LineKind, String)>, String> {
    let out = if file.status == '?' {
        // `--no-index` exits 1 when the files differ, which they always do.
        let out = command(
            root,
            &[
                "diff",
                "--no-ext-diff",
                "--no-index",
                "--",
                "/dev/null",
                &file.path,
            ],
        )
        .output()
        .map_err(|err| format!("git: {err}"))?;
        out.stdout
    } else {
        let mut args = vec!["diff", "--no-ext-diff", "HEAD", "-M", "--"];
        args.extend(file.old_path.as_deref());
        args.push(&file.path);
        git(root, &args)?
    };
    let mut lines = parse_diff(&String::from_utf8_lossy(&out));
    lines.truncate(MAX_DIFF_LINES);
    Ok(lines)
}

/// `git -C dir <args>` that never takes optional locks (an agent may be
/// running git in the same repository), never colors and never pages.
fn command(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .arg("--no-optional-locks")
        .args(["-c", "color.ui=false", "-c", "core.quotepath=false"])
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null());
    command
}

/// Stdout of a git command that must succeed.
fn git(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = command(dir, args)
        .output()
        .map_err(|err| format!("git: {err}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn z(parts: &[&str]) -> Vec<u8> {
        parts.iter().flat_map(|p| p.bytes().chain([0])).collect()
    }

    #[test]
    fn status_reads_branch_and_every_kind_of_change() {
        let out = z(&[
            "# branch.oid 1234567",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -1",
            "1 .M N... 100644 100644 100644 aaa aaa src/lib.rs",
            "1 A. N... 000000 100644 100644 000 bbb src/new.rs",
            "1 .D N... 100644 100644 000000 ccc ccc old.txt",
            "2 R. N... 100644 100644 100644 ddd ddd R100 src/b.rs",
            "src/a.rs",
            "u UU N... 100644 100644 100644 100644 e1 e2 e3 merge.rs",
            "? notes.md",
            "! target/x",
        ]);
        let status = parse_status(&out);
        assert_eq!(status.branch, "main");
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!((status.ahead, status.behind), (2, 1));
        let summary: Vec<(char, &str, Option<&str>)> = status
            .files
            .iter()
            .map(|f| (f.status, f.path.as_str(), f.old_path.as_deref()))
            .collect();
        assert_eq!(
            summary,
            [
                ('M', "src/lib.rs", None),
                ('A', "src/new.rs", None),
                ('D', "old.txt", None),
                ('R', "src/b.rs", Some("src/a.rs")),
                ('U', "merge.rs", None),
                ('?', "notes.md", None),
            ]
        );
    }

    #[test]
    fn status_of_a_new_or_detached_repo() {
        let status = parse_status(&z(&["# branch.oid (initial)", "# branch.head (detached)"]));
        assert_eq!(status.branch, "(detached)");
        assert_eq!(status.upstream, None);
        assert_eq!((status.ahead, status.behind), (0, 0));
        assert_eq!(status.files.len(), 0);
        assert_eq!(parse_status(b""), RepoStatus::default());
    }

    #[test]
    fn numstat_adds_line_counts_including_renames_and_binaries() {
        let mut status = parse_status(&z(&[
            "# branch.head main",
            "1 .M N... 100644 100644 100644 a a src/lib.rs",
            "2 R. N... 100644 100644 100644 d d R90 src/b.rs",
            "src/a.rs",
            "1 .M N... 100644 100644 100644 a a logo.png",
        ]));
        let numstat = z(&[
            "3\t1\tsrc/lib.rs",
            "2\t2\t",
            "src/a.rs",
            "src/b.rs",
            "-\t-\tlogo.png",
        ]);
        apply_numstat(&mut status, &numstat);
        let counts: Vec<(Option<u32>, Option<u32>)> =
            status.files.iter().map(|f| (f.added, f.removed)).collect();
        assert_eq!(
            counts,
            [(Some(3), Some(1)), (Some(2), Some(2)), (None, None)]
        );
    }

    #[test]
    fn diff_lines_are_classified() {
        let text = "diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n same\n-old\n+new\n\\ No newline at end of file\n";
        let kinds: Vec<LineKind> = parse_diff(text).into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            kinds,
            [
                LineKind::Header,
                LineKind::Header,
                LineKind::Header,
                LineKind::Header,
                LineKind::Hunk,
                LineKind::Context,
                LineKind::Removed,
                LineKind::Added,
                LineKind::Header,
            ]
        );
        assert_eq!(parse_diff("")[..], []);
    }

    /// A fresh repository with one commit, in a temp dir.
    struct Repo(PathBuf);

    impl Repo {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("my-terminal-git-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let repo = Self(dir);
            repo.git(&["init", "-q", "-b", "main"]);
            repo.git(&["config", "user.email", "test@example.invalid"]);
            repo.git(&["config", "user.name", "Test"]);
            repo.write("keep.txt", "one\ntwo\nthree\n");
            repo.write("gone.txt", "bye\n");
            repo.write("move.txt", "same\n");
            repo.git(&["add", "."]);
            repo.git(&["commit", "-q", "-m", "init"]);
            repo
        }

        fn git(&self, args: &[&str]) {
            let ok = Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(args)
                .status()
                .unwrap();
            assert!(ok.success(), "git {args:?}");
        }

        fn write(&self, name: &str, text: &str) {
            std::fs::write(self.0.join(name), text).unwrap();
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn real_repo_status_and_diffs() {
        let repo = Repo::new("status");
        std::fs::create_dir_all(repo.0.join("sub")).unwrap();
        assert_eq!(
            repo_root(&repo.0.join("sub")).map(|p| p.canonicalize().unwrap()),
            Some(repo.0.canonicalize().unwrap())
        );
        repo.write("keep.txt", "one\nTWO\nthree\nfour\n");
        std::fs::remove_file(repo.0.join("gone.txt")).unwrap();
        repo.git(&["mv", "move.txt", "moved.txt"]);
        repo.write("new file.md", "hello\nworld\n");

        let status = status(&repo.0).unwrap();
        assert_eq!(status.branch, "main");
        let summary: Vec<(char, &str, Option<u32>, Option<u32>)> = status
            .files
            .iter()
            .map(|f| (f.status, f.path.as_str(), f.added, f.removed))
            .collect();
        assert!(
            summary.contains(&('M', "keep.txt", Some(2), Some(1))),
            "{summary:?}"
        );
        assert!(
            summary.contains(&('D', "gone.txt", Some(0), Some(1))),
            "{summary:?}"
        );
        assert!(
            summary.iter().any(|s| s.0 == 'R' && s.1 == "moved.txt"),
            "{summary:?}"
        );
        assert!(
            summary.contains(&('?', "new file.md", None, None)),
            "{summary:?}"
        );

        let keep = status.files.iter().find(|f| f.path == "keep.txt").unwrap();
        let lines = diff(&repo.0, keep).unwrap();
        assert!(
            lines.contains(&(LineKind::Removed, "-two".into())),
            "{lines:?}"
        );
        assert!(
            lines.contains(&(LineKind::Added, "+TWO".into())),
            "{lines:?}"
        );
        assert!(
            lines.contains(&(LineKind::Added, "+four".into())),
            "{lines:?}"
        );

        let new = status.files.iter().find(|f| f.status == '?').unwrap();
        let lines = diff(&repo.0, new).unwrap();
        let added: Vec<&str> = lines
            .iter()
            .filter(|(k, _)| *k == LineKind::Added)
            .map(|(_, l)| l.as_str())
            .collect();
        assert_eq!(added, ["+hello", "+world"]);
    }

    #[test]
    fn outside_a_repo_there_is_no_root() {
        let dir = std::env::temp_dir().join(format!("my-terminal-nogit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // The temp dir is not inside a repository on a normal machine.
        assert_eq!(repo_root(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
