//! Mouse selection over viewport cells.

use std::fmt::Write as _;

use term_core::alacritty_terminal::Term;
use term_core::alacritty_terminal::event::VoidListener;
use term_core::alacritty_terminal::grid::Scroll;
use term_core::alacritty_terminal::index::Side;
use term_core::alacritty_terminal::selection::SelectionType;
use term_core::alacritty_terminal::term::Config;
use term_core::alacritty_terminal::term::test::TermSize;
use term_core::alacritty_terminal::vte::ansi::Processor;
use term_core::selection::{extend, kind, start, text};

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

#[test]
fn click_count_picks_the_kind() {
    assert_eq!(kind(1), SelectionType::Simple);
    assert_eq!(kind(2), SelectionType::Semantic);
    assert_eq!(kind(3), SelectionType::Lines);
    assert_eq!(kind(7), SelectionType::Lines);
    assert_eq!(kind(0), SelectionType::Simple);
}

#[test]
fn drag_selects_the_cells_between() {
    let mut term = term_with(20, 4, "hello world\r\nsecond");
    start(&mut term, SelectionType::Simple, (0, 0), Side::Left);
    extend(&mut term, (4, 0), Side::Right);
    assert_eq!(text(&term).as_deref(), Some("hello"));
}

#[test]
fn drag_across_lines_keeps_the_line_break() {
    let mut term = term_with(20, 4, "hello world\r\nsecond");
    start(&mut term, SelectionType::Simple, (6, 0), Side::Left);
    extend(&mut term, (2, 1), Side::Right);
    assert_eq!(text(&term).as_deref(), Some("world\nsec"));
}

#[test]
fn click_without_drag_selects_nothing() {
    let mut term = term_with(20, 4, "hello world");
    start(&mut term, SelectionType::Simple, (3, 0), Side::Left);
    assert_eq!(text(&term), None);
}

#[test]
fn double_click_selects_the_word() {
    let mut term = term_with(20, 4, "hello world");
    start(&mut term, SelectionType::Semantic, (8, 0), Side::Left);
    assert_eq!(text(&term).as_deref(), Some("world"));
}

#[test]
fn triple_click_selects_the_line() {
    let mut term = term_with(20, 4, "hello world\r\nsecond line");
    start(&mut term, SelectionType::Lines, (3, 1), Side::Left);
    // Whole lines keep their line break, like Alacritty.
    assert_eq!(text(&term).as_deref(), Some("second line\n"));
}

#[test]
fn extend_without_selection_does_nothing() {
    let mut term = term_with(20, 4, "hello world");
    extend(&mut term, (4, 0), Side::Right);
    assert_eq!(text(&term), None);
}

#[test]
fn viewport_cells_follow_the_scrollback() {
    let mut output = String::new();
    for n in 0..12 {
        writeln!(output, "line{n:02}\r").unwrap();
    }
    let mut term = term_with(10, 4, &output);
    // Last screen shows line09..line11 and an empty prompt line.
    term.scroll_display(Scroll::Delta(5));
    start(&mut term, SelectionType::Simple, (0, 0), Side::Left);
    extend(&mut term, (5, 0), Side::Right);
    assert_eq!(text(&term).as_deref(), Some("line04"));
}
