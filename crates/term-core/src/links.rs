//! Links in terminal output: URLs and file paths (optionally with
//! `:line:column`), found under a clicked cell.

use alacritty_terminal::Term;
use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::term::cell::Flags;

/// What a click can open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// An `http`, `https` or `file` URL.
    Url(String),
    /// A path as printed (maybe relative or starting with `~`), with the
    /// line and column that followed it.
    File {
        path: String,
        line: Option<u32>,
        column: Option<u32>,
    },
}

/// Characters that end a link besides whitespace.
const DELIMITERS: &str = "\"'`<>()[]{}|";
/// Punctuation trimmed from the end: sentence marks after a link. A leading
/// `.` stays (`./build.sh`).
const TRIM: &[char] = &['.', ',', ';', ':', '!', '?'];
const SCHEMES: [&str; 3] = ["https://", "http://", "file://"];

/// The link in `text` that covers character `index`, if any.
#[must_use]
pub fn find(text: &str, index: usize) -> Option<Link> {
    let chars: Vec<char> = text.chars().collect();
    let is_part = |c: char| !c.is_whitespace() && !DELIMITERS.contains(c);
    if !chars.get(index).copied().is_some_and(is_part) {
        return None;
    }
    let start = chars[..index]
        .iter()
        .rposition(|&c| !is_part(c))
        .map_or(0, |i| i + 1);
    let end = chars[index..]
        .iter()
        .position(|&c| !is_part(c))
        .map_or(chars.len(), |i| index + i);
    let token: String = chars[start..end].iter().collect();
    let token = token
        .trim_end_matches(TRIM)
        .trim_start_matches(|c: char| c != '.' && TRIM.contains(&c));
    if token.is_empty() {
        return None;
    }
    if SCHEMES.iter().any(|scheme| token.starts_with(scheme)) {
        return Some(Link::Url(token.to_owned()));
    }
    file_link(token)
}

/// `path[:line[:column]]`, when it looks like a file: it has a `/`, or a
/// line number and a dot (`main.rs:12`).
fn file_link(token: &str) -> Option<Link> {
    let mut parts = token.split(':');
    let path = parts.next()?;
    let line = parts.next().and_then(|n| n.parse::<u32>().ok());
    let column = line.and(parts.next()).and_then(|n| n.parse::<u32>().ok());
    let looks_like_file = path.contains('/') || (line.is_some() && path.contains('.'));
    (!path.is_empty() && looks_like_file).then(|| Link::File {
        path: path.to_owned(),
        line,
        column,
    })
}

/// The link under grid cell `point`.
#[must_use]
pub fn link_at<T>(term: &Term<T>, point: Point) -> Option<Link> {
    let grid = term.grid();
    if point.line < grid.topmost_line() || point.line > grid.bottommost_line() {
        return None;
    }
    let row = &grid[point.line];
    let mut text = String::new();
    // Character index of each column (a wide char's spacer maps to it).
    let mut index_of = Vec::with_capacity(term.columns());
    let mut count = 0usize;
    for column in 0..term.columns() {
        let cell = &row[Column(column)];
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            index_of.push(count.saturating_sub(1));
            continue;
        }
        index_of.push(count);
        text.push(cell.c);
        count += 1;
    }
    find(&text, *index_of.get(point.column.0)?)
}
