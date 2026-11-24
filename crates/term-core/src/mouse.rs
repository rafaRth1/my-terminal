//! Mouse wheel routing.
//!
//! A wheel turn means different things depending on what the program asked
//! for: programs that track the mouse get wheel button reports, full-screen
//! programs without mouse tracking (`less`, `man`) get arrow keys, and a plain
//! shell scrolls the scrollback history.

use alacritty_terminal::term::TermMode;

/// Largest 0-based cell coordinate the legacy X10 encoding can carry
/// (one byte, offset by 32 and 1).
const LEGACY_MAX_CELL: usize = 222;
const WHEEL_UP: u8 = 64;
const WHEEL_DOWN: u8 = 65;

/// What a wheel turn should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WheelAction {
    /// Move the visible part of the history; positive goes back in time.
    Scrollback(i32),
    /// Write these bytes to the program.
    Send(Vec<u8>),
}

/// Routes a wheel turn of `lines` (positive = up) over `cell`
/// (0-based column, line on screen).
#[must_use]
pub fn wheel(mode: TermMode, lines: i32, cell: (usize, usize)) -> Option<WheelAction> {
    if lines == 0 {
        return None;
    }
    let count = lines.unsigned_abs() as usize;
    let up = lines > 0;

    if mode.intersects(TermMode::MOUSE_MODE) {
        let button = if up { WHEEL_UP } else { WHEEL_DOWN };
        let report = report(mode, button, cell)?;
        return Some(WheelAction::Send(report.repeat(count)));
    }
    if mode.contains(TermMode::ALT_SCREEN) {
        if !mode.contains(TermMode::ALTERNATE_SCROLL) {
            return None;
        }
        let prefix: &[u8] = if mode.contains(TermMode::APP_CURSOR) {
            b"\x1bO"
        } else {
            b"\x1b["
        };
        let arrow = [prefix, if up { b"A" } else { b"B" }].concat();
        return Some(WheelAction::Send(arrow.repeat(count)));
    }
    Some(WheelAction::Scrollback(lines))
}

/// One mouse button press report, SGR when the program asked for it.
fn report(mode: TermMode, button: u8, (col, line): (usize, usize)) -> Option<Vec<u8>> {
    if mode.contains(TermMode::SGR_MOUSE) {
        return Some(format!("\x1b[<{button};{};{}M", col + 1, line + 1).into_bytes());
    }
    if col > LEGACY_MAX_CELL || line > LEGACY_MAX_CELL {
        return None;
    }
    let encode = |n: usize| u8::try_from(n + 1 + 32).ok();
    Some(vec![
        0x1b,
        b'[',
        b'M',
        button + 32,
        encode(col)?,
        encode(line)?,
    ])
}
