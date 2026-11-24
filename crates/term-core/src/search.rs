//! Text search over the screen and history.
//!
//! The query is literal text (no regex); like alacritty's own search it is
//! case-insensitive unless it holds an uppercase letter.

use alacritty_terminal::Term;
use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::{Column, Direction, Point};
use alacritty_terminal::term::search::{RegexIter, RegexSearch};

/// A match: first and last cell, in grid coordinates (history lines are
/// negative).
pub type Match = alacritty_terminal::term::search::Match;

/// Most matches kept (the newest ones).
pub const MAX_MATCHES: usize = 1000;

/// Every match of `query`, top to bottom. Empty or unusable queries find
/// nothing.
#[must_use]
pub fn find_all<T>(term: &Term<T>, query: &str) -> Vec<Match> {
    if query.is_empty() {
        return Vec::new();
    }
    let Ok(mut regex) = RegexSearch::new(&literal(query)) else {
        return Vec::new();
    };
    let grid = term.grid();
    let start = Point::new(grid.topmost_line(), Column(0));
    let end = Point::new(grid.bottommost_line(), grid.last_column());
    // Keep the newest matches: the bottom of the output is what matters.
    let mut kept = std::collections::VecDeque::with_capacity(MAX_MATCHES);
    for found in RegexIter::new(start, end, Direction::Right, term, &mut regex) {
        if kept.len() == MAX_MATCHES {
            kept.pop_front();
        }
        kept.push_back(found);
    }
    kept.into()
}

/// Scrolls the viewport so `found` is visible.
pub fn reveal<T: alacritty_terminal::event::EventListener>(term: &mut Term<T>, found: &Match) {
    term.scroll_to_point(*found.start());
}

/// Part of a match on one viewport row, for painting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// Viewport row, 0 at the top.
    pub row: usize,
    /// First and last column, inclusive.
    pub start: usize,
    pub end: usize,
    /// Index of the match in the list given to [`segments`].
    pub index: usize,
}

/// The visible parts of `matches` for a viewport scrolled up by
/// `display_offset` lines, `screen_lines` tall and `columns` wide. A match
/// that wraps onto several rows gives one segment per row.
#[must_use]
pub fn segments(
    matches: &[Match],
    display_offset: usize,
    screen_lines: usize,
    columns: usize,
) -> Vec<Segment> {
    let offset = i64::try_from(display_offset).unwrap_or(i64::MAX);
    let rows = i64::try_from(screen_lines).unwrap_or(i64::MAX);
    let mut out = Vec::new();
    for (index, found) in matches.iter().enumerate() {
        let (first, last) = (found.start(), found.end());
        for line in first.line.0..=last.line.0 {
            let row = i64::from(line) + offset;
            if !(0..rows).contains(&row) {
                continue;
            }
            let start = if line == first.line.0 {
                first.column.0
            } else {
                0
            };
            let end = if line == last.line.0 {
                last.column.0
            } else {
                columns.saturating_sub(1)
            };
            out.push(Segment {
                row: usize::try_from(row).unwrap_or(0),
                start,
                end,
                index,
            });
        }
    }
    out
}

/// `query` with regex syntax escaped, so it matches as plain text.
fn literal(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for c in query.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
