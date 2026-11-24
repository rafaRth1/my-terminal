//! Mouse wheel routing: scrollback, alternate-screen arrows, or mouse reports.

use term_core::alacritty_terminal::term::TermMode;
use term_core::mouse::{WheelAction, wheel};

const AT: (usize, usize) = (4, 2);

#[test]
fn no_lines_does_nothing() {
    assert_eq!(wheel(TermMode::empty(), 0, AT), None);
    assert_eq!(wheel(TermMode::MOUSE_REPORT_CLICK, 0, AT), None);
}

#[test]
fn plain_screen_scrolls_history() {
    assert_eq!(
        wheel(TermMode::empty(), 3, AT),
        Some(WheelAction::Scrollback(3))
    );
    assert_eq!(
        wheel(TermMode::empty(), -2, AT),
        Some(WheelAction::Scrollback(-2))
    );
}

#[test]
fn alternate_screen_sends_arrows() {
    let mode = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
    assert_eq!(
        wheel(mode, 2, AT),
        Some(WheelAction::Send(b"\x1b[A\x1b[A".to_vec()))
    );
    assert_eq!(
        wheel(mode, -1, AT),
        Some(WheelAction::Send(b"\x1b[B".to_vec()))
    );
    assert_eq!(
        wheel(mode | TermMode::APP_CURSOR, 1, AT),
        Some(WheelAction::Send(b"\x1bOA".to_vec()))
    );
}

#[test]
fn alternate_screen_without_alternate_scroll_does_nothing() {
    assert_eq!(wheel(TermMode::ALT_SCREEN, 2, AT), None);
}

#[test]
fn sgr_mouse_mode_reports_wheel_buttons() {
    let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
    assert_eq!(
        wheel(mode, 2, AT),
        Some(WheelAction::Send(b"\x1b[<64;5;3M\x1b[<64;5;3M".to_vec()))
    );
    assert_eq!(
        wheel(
            mode | TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL,
            -1,
            AT
        ),
        Some(WheelAction::Send(b"\x1b[<65;5;3M".to_vec()))
    );
}

#[test]
fn legacy_mouse_mode_reports_wheel_buttons() {
    // Button 64 + 32, column 4 + 1 + 32, line 2 + 1 + 32.
    assert_eq!(
        wheel(TermMode::MOUSE_DRAG, 1, AT),
        Some(WheelAction::Send(vec![0x1b, b'[', b'M', 96, 37, 35]))
    );
    assert_eq!(
        wheel(TermMode::MOUSE_MOTION, -1, AT),
        Some(WheelAction::Send(vec![0x1b, b'[', b'M', 97, 37, 35]))
    );
}

#[test]
fn legacy_mouse_mode_skips_cells_it_cannot_encode() {
    assert_eq!(wheel(TermMode::MOUSE_REPORT_CLICK, 1, (223, 0)), None);
    assert_eq!(
        wheel(
            TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE,
            1,
            (300, 0)
        ),
        Some(WheelAction::Send(b"\x1b[<64;301;1M".to_vec()))
    );
}
