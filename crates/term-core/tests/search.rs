//! Searching the screen and history for text.

use std::fmt::Write as _;

use term_core::alacritty_terminal::Term;
use term_core::alacritty_terminal::event::VoidListener;
use term_core::alacritty_terminal::term::Config;
use term_core::alacritty_terminal::term::test::TermSize;
use term_core::alacritty_terminal::vte::ansi::Processor;
use term_core::search::{Segment, find_all, reveal, segments};

fn term_with(columns: usize, lines: usize, output: &str) -> Term<VoidListener> {
    let mut term = Term::new(
        Config::default(),
        &TermSize::new(columns, lines),
        VoidListener,
    );
    let mut parser: Processor = Processor::new();
    parser.advance(&mut term, output.as_bytes());
    term
}

fn starts(matches: &[term_core::search::Match]) -> Vec<(i32, usize)> {
    matches
        .iter()
        .map(|m| (m.start().line.0, m.start().column.0))
        .collect()
}

#[test]
fn finds_every_match_in_history_and_screen_in_order() {
    // 6 lines on a 3-line screen: the first 3 are history (lines -3..-1).
    let term = term_with(
        20,
        3,
        "error one\r\nok\r\nerror two\r\nok\r\nERROR three\r\nok",
    );
    let matches = find_all(&term, "error");
    // Lowercase query: case-insensitive.
    assert_eq!(starts(&matches), [(-3, 0), (-1, 0), (1, 0)]);
    assert_eq!(matches[0].end().column.0, 4);
    // An uppercase letter makes it case-sensitive.
    assert_eq!(starts(&find_all(&term, "ERROR")), [(1, 0)]);
}

#[test]
fn query_is_literal_text_not_a_regex() {
    let term = term_with(20, 3, "a.b axb (x)");
    assert_eq!(starts(&find_all(&term, "a.b")), [(0, 0)]);
    assert_eq!(starts(&find_all(&term, "(x)")), [(0, 8)]);
    assert_eq!(find_all(&term, "").len(), 0);
    assert_eq!(find_all(&term, "missing").len(), 0);
}

#[test]
fn segments_map_matches_to_viewport_rows() {
    // "abcdef" wraps on a 4-column screen: "abcd" / "ef".
    let term = term_with(4, 3, "x\r\nabcdef");
    let matches = find_all(&term, "cde");
    assert_eq!(matches.len(), 1);
    // Display offset 0: rows are screen lines.
    assert_eq!(
        segments(&matches, 0, 3, 4),
        [
            Segment {
                row: 1,
                start: 2,
                end: 3,
                index: 0
            },
            Segment {
                row: 2,
                start: 0,
                end: 0,
                index: 0
            },
        ]
    );
    // Scrolled up by one: everything moves one row down; row 3 is off screen.
    assert_eq!(
        segments(&matches, 1, 3, 4),
        [Segment {
            row: 2,
            start: 2,
            end: 3,
            index: 0
        }]
    );
}

#[test]
fn reveal_scrolls_a_match_into_view() {
    let output: String = std::iter::once("needle\r\n".to_owned())
        .chain((0..20).map(|n| format!("line {n}\r\n")))
        .collect();
    let mut term = term_with(20, 5, &output);
    let matches = find_all(&term, "needle");
    assert_eq!(matches.len(), 1);
    assert_eq!(term.grid().display_offset(), 0);
    reveal(&mut term, &matches[0]);
    let offset = term.grid().display_offset();
    let row = matches[0].start().line.0 + i32::try_from(offset).unwrap();
    assert!((0..5).contains(&row), "row {row} offset {offset}");
}

#[test]
fn keeps_the_newest_matches_when_there_are_too_many() {
    let output = (0..term_core::search::MAX_MATCHES + 5).fold(String::new(), |mut out, n| {
        let _ = write!(out, "hit {n}\r\n");
        out
    });
    let term = term_with(20, 5, &output);
    let matches = find_all(&term, "hit");
    assert_eq!(matches.len(), term_core::search::MAX_MATCHES);
    // The last match is the bottom-most one.
    let last = matches.last().unwrap().start().line;
    assert!(last.0 >= 3, "newest match kept, got line {}", last.0);
}
