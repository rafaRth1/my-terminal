//! User settings from `$XDG_CONFIG_HOME/my-terminal/config.toml`:
//!
//! ```toml
//! font_family = "FiraCode Nerd Font Mono"
//! font_size = 14
//! theme = "dark"   # or "light"
//! ```
//!
//! Every key is optional. Bad values fall back to the default and are
//! reported; the file is read once at start.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::theme::{FONT_FAMILY, FONT_SIZE};

pub const MIN_FONT_SIZE: f32 = 6.0;
pub const MAX_FONT_SIZE: f32 = 48.0;

/// Built-in color themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeName {
    #[default]
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub font_family: String,
    pub font_size: f32,
    pub theme: ThemeName,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font_family: FONT_FAMILY.to_owned(),
            font_size: FONT_SIZE,
            theme: ThemeName::Dark,
        }
    }
}

/// `$XDG_CONFIG_HOME/my-terminal/config.toml`, defaulting to
/// `~/.config/my-terminal/config.toml`.
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("my-terminal").join("config.toml"))
}

/// Reads `path`. A missing file gives the defaults and no problems.
pub fn load(path: &Path) -> (Config, Vec<String>) {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (Config::default(), Vec::new()),
        Err(err) => (
            Config::default(),
            vec![format!("{}: {err}", path.display())],
        ),
    }
}

/// Parses the file text; each problem is described and its key keeps the
/// default.
pub fn parse(text: &str) -> (Config, Vec<String>) {
    let mut config = Config::default();
    let table: toml::Table = match text.parse() {
        Ok(table) => table,
        Err(err) => return (config, vec![toml::de::Error::message(&err).to_owned()]),
    };
    let mut problems = Vec::new();
    for (key, value) in table {
        let problem = match key.as_str() {
            "font_family" => match value.as_str().map(str::trim) {
                Some("") => Some("font_family is empty".to_owned()),
                Some(family) => {
                    family.clone_into(&mut config.font_family);
                    None
                }
                None => Some("font_family must be text".to_owned()),
            },
            "font_size" => font_size(&value).map_or_else(
                || Some("font_size must be a number".to_owned()),
                |size| {
                    let clamped = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
                    config.font_size = clamped;
                    (clamped.to_bits() != size.to_bits()).then(|| {
                        format!(
                            "font_size {size} is outside {MIN_FONT_SIZE}..={MAX_FONT_SIZE}; using {clamped}"
                        )
                    })
                },
            ),
            "theme" => match value.as_str() {
                Some("dark") => {
                    config.theme = ThemeName::Dark;
                    None
                }
                Some("light") => {
                    config.theme = ThemeName::Light;
                    None
                }
                _ => Some(format!("theme {value} is unknown; use \"dark\" or \"light\"")),
            },
            other => Some(format!("unknown key {other}")),
        };
        problems.extend(problem);
    }
    (config, problems)
}

/// A finite number (integer or float) as `f32`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "font sizes are small; the result is clamped"
)]
fn font_size(value: &toml::Value) -> Option<f32> {
    let number = value
        .as_float()
        .or_else(|| value.as_integer().map(|n| n as f64))?;
    number.is_finite().then_some(number as f32)
}

/// Writes `config` to `path` (creating its directory) through a temporary
/// file, so a crash never leaves half a file.
pub fn save(path: &Path, config: &Config) -> std::io::Result<()> {
    #[derive(Serialize)]
    struct Out<'a> {
        font_family: &'a str,
        font_size: f32,
        theme: &'a str,
    }
    let out = Out {
        font_family: &config.font_family,
        font_size: config.font_size,
        theme: match config.theme {
            ThemeName::Dark => "dark",
            ThemeName::Light => "light",
        },
    };
    let body = toml::to_string(&out).map_err(std::io::Error::other)?;
    let text =
        format!("# my-terminal settings; also edited from the gear in the title bar.\n{body}");
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no parent directory"))?;
    std::fs::create_dir_all(dir)?;
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(".tmp-{}", std::process::id()));
    let temp = PathBuf::from(temp);
    let written = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// `size` moved by `steps` points, kept within the allowed range.
pub fn zoom(size: f32, steps: i32) -> f32 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "steps are a handful of key presses"
    )]
    let delta = steps as f32;
    (size + delta).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_or_missing_file_gives_defaults() {
        assert_eq!(parse(""), (Config::default(), Vec::new()));
        let missing = Path::new("/nonexistent/my-terminal/config.toml");
        assert_eq!(load(missing), (Config::default(), Vec::new()));
    }

    #[test]
    fn reads_every_key() {
        let (config, problems) =
            parse("font_family = \"JetBrains Mono\"\nfont_size = 16\ntheme = \"light\"\n");
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(
            config,
            Config {
                font_family: "JetBrains Mono".into(),
                font_size: 16.0,
                theme: ThemeName::Light,
            }
        );
    }

    #[test]
    fn bad_values_keep_defaults_and_are_reported() {
        let (config, problems) = parse("font_size = 200\ntheme = \"neon\"\nfont_family = \"  \"\n");
        assert_eq!(config.font_size, MAX_FONT_SIZE);
        assert_eq!(config.theme, ThemeName::Dark);
        assert_eq!(config.font_family, FONT_FAMILY);
        assert_eq!(problems.len(), 3, "{problems:?}");

        let (config, problems) = parse("font_size = [");
        assert_eq!(config, Config::default());
        assert_eq!(problems.len(), 1);

        let (_, problems) = parse("fontsize = 12");
        assert_eq!(problems.len(), 1, "unknown keys are reported");
    }

    #[test]
    fn zoom_steps_one_point_within_limits() {
        assert_eq!(zoom(14.0, 1), 15.0);
        assert_eq!(zoom(14.0, -2), 12.0);
        assert_eq!(zoom(MAX_FONT_SIZE, 1), MAX_FONT_SIZE);
        assert_eq!(zoom(MIN_FONT_SIZE, -1), MIN_FONT_SIZE);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("my-terminal-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("my-terminal").join("config.toml");
        let config = Config {
            font_family: "JetBrains \"Mono\"".into(),
            font_size: 17.0,
            theme: ThemeName::Light,
        };
        save(&path, &config).unwrap();
        assert_eq!(load(&path), (config, Vec::new()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn one_bad_key_keeps_the_good_ones() {
        let (config, problems) = parse("font_size = 20\ntheme = \"light\"\nfontsize = 1\n");
        assert_eq!(config.font_size, 20.0);
        assert_eq!(config.theme, ThemeName::Light);
        assert_eq!(problems.len(), 1, "{problems:?}");
        let (config, problems) = parse("font_size = \"big\"\ntheme = \"light\"\n");
        assert_eq!(config.font_size, FONT_SIZE);
        assert_eq!(config.theme, ThemeName::Light);
        assert_eq!(problems.len(), 1, "{problems:?}");
        let (config, problems) = parse("font_size = nan\n");
        assert_eq!(config.font_size, FONT_SIZE);
        assert_eq!(problems.len(), 1, "{problems:?}");
    }
}
