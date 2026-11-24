//! Opening a Ctrl+clicked link: URLs in the browser, files in an editor at
//! their line.

use std::path::{Path, PathBuf};

/// `path` as printed, made absolute: `~` is the home directory, a relative
/// path is relative to the pane's directory.
pub fn resolve(path: &str, cwd: Option<&Path>, home: Option<&Path>) -> PathBuf {
    if let (Some(rest), Some(home)) = (path.strip_prefix("~/"), home) {
        return home.join(rest);
    }
    match cwd {
        Some(cwd) => cwd.join(path),
        // Never let a relative path start with `-`: the editor would read
        // it as an option.
        None if path.starts_with('-') => Path::new(".").join(path),
        None => PathBuf::from(path),
    }
}

/// The editor for files: `$VISUAL`, then `$EDITOR`, then `nvim` when it is
/// installed.
pub fn editor(visual: Option<&str>, editor: Option<&str>, has_nvim: bool) -> Option<String> {
    [visual, editor]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| has_nvim.then(|| "nvim".to_owned()))
}

/// Shell command that opens `path` in `editor` at `line` (`+N`, understood
/// by vi, vim, nvim, nano, emacs and micro).
pub fn editor_command(editor: &str, path: &Path, line: Option<u32>) -> String {
    let path = quote(&path.to_string_lossy());
    match line {
        Some(line) => format!("{editor} +{line} {path}"),
        None => format!("{editor} {path}"),
    }
}

/// `text` in single quotes, safe to type into a shell.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// What a Ctrl+click on an existing path opens.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    /// A folder, in the file manager (the default app for folders).
    Folder(String),
    /// A file, typed into a new tab as this editor command.
    Editor(String),
    /// A file with no editor configured, in its default app.
    DefaultApp(String),
}

/// Where `path` (absolute, existing) opens.
pub fn target(path: &Path, is_dir: bool, editor: Option<&str>, line: Option<u32>) -> Target {
    if is_dir {
        return Target::Folder(file_url(path));
    }
    match editor {
        Some(editor) => Target::Editor(editor_command(editor, path, line)),
        None => Target::DefaultApp(file_url(path)),
    }
}

/// `file://` URL of an absolute path, percent-encoded.
pub fn file_url(path: &Path) -> String {
    use std::fmt::Write as _;
    use std::os::unix::ffi::OsStrExt as _;
    let mut url = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            url.push(char::from(byte));
        } else {
            let _ = write!(url, "%{byte:02X}");
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_expands_home_and_relative_paths() {
        let cwd = Path::new("/work/project");
        let home = Path::new("/home/u");
        assert_eq!(
            resolve("src/a.rs", Some(cwd), Some(home)),
            Path::new("/work/project/src/a.rs")
        );
        assert_eq!(
            resolve("./b.sh", Some(cwd), Some(home)),
            Path::new("/work/project/./b.sh")
        );
        assert_eq!(
            resolve("~/notes.md", Some(cwd), Some(home)),
            Path::new("/home/u/notes.md")
        );
        assert_eq!(
            resolve("/etc/hosts", Some(cwd), Some(home)),
            Path::new("/etc/hosts")
        );
        // Without a known directory the path stays as printed.
        assert_eq!(resolve("src/a.rs", None, None), Path::new("src/a.rs"));
        assert_eq!(
            resolve("~/x", Some(cwd), None),
            Path::new("/work/project/~/x")
        );
    }

    #[test]
    fn editor_prefers_visual_then_editor_then_nvim() {
        assert_eq!(editor(Some("hx"), Some("vim"), true).as_deref(), Some("hx"));
        assert_eq!(editor(None, Some("vim"), true).as_deref(), Some("vim"));
        assert_eq!(editor(Some(" "), None, true).as_deref(), Some("nvim"));
        assert_eq!(editor(None, None, false), None);
    }

    #[test]
    fn editor_command_quotes_the_path() {
        assert_eq!(
            editor_command("nvim", Path::new("/w/src/main.rs"), Some(42)),
            "nvim +42 '/w/src/main.rs'"
        );
        assert_eq!(
            editor_command("vim", Path::new("/w/a b.txt"), None),
            "vim '/w/a b.txt'"
        );
        // A quote in the name cannot end the quoting.
        assert_eq!(
            editor_command("nvim", Path::new("/w/it's;rm -rf ~"), None),
            r"nvim '/w/it'\''s;rm -rf ~'"
        );
    }

    #[test]
    fn folders_open_in_the_file_manager_files_in_the_editor() {
        let dir = Path::new("/home/u/Downloads");
        assert_eq!(
            target(dir, true, Some("nvim"), Some(3)),
            Target::Folder("file:///home/u/Downloads".into())
        );
        let file = Path::new("/w/a.rs");
        assert_eq!(
            target(file, false, Some("nvim"), Some(3)),
            Target::Editor("nvim +3 '/w/a.rs'".into())
        );
        assert_eq!(
            target(file, false, None, Some(3)),
            Target::DefaultApp("file:///w/a.rs".into())
        );
    }

    #[test]
    fn file_urls_are_percent_encoded() {
        assert_eq!(
            file_url(Path::new("/a b/c#d%e.txt")),
            "file:///a%20b/c%23d%25e.txt"
        );
        assert_eq!(file_url(Path::new("/ñandú/x")), "file:///%C3%B1and%C3%BA/x");
        assert_eq!(file_url(Path::new("/ok-._~/X1")), "file:///ok-._~/X1");
    }

    #[test]
    fn relative_paths_never_look_like_options() {
        assert_eq!(resolve("-c/x", None, None), Path::new("./-c/x"));
        assert_eq!(
            editor_command("nvim", &resolve("-c/x", None, None), None),
            "nvim './-c/x'"
        );
    }
}
