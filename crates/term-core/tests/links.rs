//! Finding the link under a clicked cell.

use term_core::alacritty_terminal::Term;
use term_core::alacritty_terminal::event::VoidListener;
use term_core::alacritty_terminal::index::{Column, Line, Point};
use term_core::alacritty_terminal::term::Config;
use term_core::alacritty_terminal::term::test::TermSize;
use term_core::alacritty_terminal::vte::ansi::Processor;
use term_core::links::{Link, find, link_at};

#[expect(
    clippy::unnecessary_wraps,
    reason = "compared with `find`, which returns an Option"
)]
fn file(path: &str, line: Option<u32>, column: Option<u32>) -> Option<Link> {
    Some(Link::File {
        path: path.to_owned(),
        line,
        column,
    })
}

#[test]
fn finds_urls_without_surrounding_punctuation() {
    let text = "see https://example.com/a?b=1. now";
    assert_eq!(
        find(text, 10),
        Some(Link::Url("https://example.com/a?b=1".into()))
    );
    assert_eq!(
        find("(http://x.dev/p)", 3),
        Some(Link::Url("http://x.dev/p".into()))
    );
    // Clicking the word before or the space: nothing.
    assert_eq!(find(text, 1), None);
    assert_eq!(find(text, 3), None);
}

#[test]
fn finds_paths_with_line_and_column() {
    assert_eq!(
        find("--> src/main.rs:42:7: error", 6),
        file("src/main.rs", Some(42), Some(7))
    );
    assert_eq!(
        find("crates/app/src/workspace.rs:10", 0),
        file("crates/app/src/workspace.rs", Some(10), None)
    );
    assert_eq!(
        find("run ./build.sh now", 6),
        file("./build.sh", None, None)
    );
    assert_eq!(find("~/notes.md", 2), file("~/notes.md", None, None));
    // A bare name needs a line number to count as a file.
    assert_eq!(find("main.rs:12", 1), file("main.rs", Some(12), None));
    assert_eq!(find("hello.world", 2), None);
    assert_eq!(find("plain words", 2), None);
    assert_eq!(find("", 0), None);
    assert_eq!(find("short", 99), None);
}

#[test]
fn link_at_reads_the_clicked_row() {
    let mut term = Term::new(Config::default(), &TermSize::new(40, 3), VoidListener);
    let mut parser: Processor = Processor::new();
    parser.advance(&mut term, "error at src/lib.rs:3\r\nok".as_bytes());
    assert_eq!(
        link_at(&term, Point::new(Line(0), Column(12))),
        file("src/lib.rs", Some(3), None)
    );
    assert_eq!(link_at(&term, Point::new(Line(1), Column(0))), None);
}
