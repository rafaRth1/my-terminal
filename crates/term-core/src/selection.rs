//! Mouse selection.
//!
//! The UI works in viewport cells (what is on screen); the grid works in
//! points that include the scrollback. These helpers convert between the two
//! so a selection keeps pointing at the same text while the view scrolls.

use alacritty_terminal::Term;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::viewport_to_point;

/// What a run of clicks selects: one click drags, two pick a word, three a line.
#[must_use]
pub fn kind(click_count: usize) -> SelectionType {
    match click_count {
        2 => SelectionType::Semantic,
        n if n >= 3 => SelectionType::Lines,
        _ => SelectionType::Simple,
    }
}

/// Starts a new selection at a viewport cell (column, screen line),
/// replacing any previous one.
pub fn start<T>(term: &mut Term<T>, kind: SelectionType, cell: (usize, usize), side: Side) {
    let point = to_point(term, cell);
    term.selection = Some(Selection::new(kind, point, side));
}

/// Moves the end of the current selection to a viewport cell; does nothing
/// without a selection.
pub fn extend<T>(term: &mut Term<T>, cell: (usize, usize), side: Side) {
    let point = to_point(term, cell);
    if let Some(selection) = term.selection.as_mut() {
        selection.update(point, side);
    }
}

/// The selected text, or `None` when nothing is selected.
#[must_use]
pub fn text<T>(term: &Term<T>) -> Option<String> {
    term.selection_to_string().filter(|text| !text.is_empty())
}

/// Grid point for a viewport cell, clamped to the screen.
fn to_point<T>(term: &Term<T>, (column, line): (usize, usize)) -> Point {
    let column = column.min(term.columns().saturating_sub(1));
    let line = line.min(term.screen_lines().saturating_sub(1));
    viewport_to_point(
        term.grid().display_offset(),
        Point::new(line, Column(column)),
    )
}
