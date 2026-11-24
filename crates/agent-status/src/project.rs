//! Display helpers: project names and elapsed times.

use std::path::Path;
use std::time::Duration;

/// Name of the project containing `dir`: the nearest directory with a
/// `.git` entry, or `dir` itself when there is none.
#[must_use]
pub fn project_name(dir: &Path) -> String {
    let root = dir
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .unwrap_or(dir);
    root.file_name().map_or_else(
        || root.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Short elapsed time: `27s`, `3m`, `2h`, `5d`.
#[must_use]
pub fn format_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    match secs {
        0..60 => format!("{secs}s"),
        60..3_600 => format!("{}m", secs / 60),
        3_600..86_400 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_name_uses_git_root() {
        let root = std::env::temp_dir().join(format!("agent-status-test-{}", std::process::id()));
        let nested = root.join("repo").join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join("repo").join(".git")).unwrap();

        assert_eq!(project_name(&nested), "repo");
        assert_eq!(
            project_name(&root),
            root.file_name().unwrap().to_string_lossy()
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn project_name_of_root_dir() {
        assert_eq!(project_name(Path::new("/")), "/");
    }

    #[test]
    fn formats_elapsed_units() {
        assert_eq!(format_elapsed(Duration::from_secs(27)), "27s");
        assert_eq!(format_elapsed(Duration::from_secs(21 * 60 + 31)), "21m");
        assert_eq!(format_elapsed(Duration::from_secs(2 * 3_600)), "2h");
        assert_eq!(format_elapsed(Duration::from_secs(5 * 86_400)), "5d");
    }
}
