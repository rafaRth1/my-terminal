//! A gpui view that renders one terminal session and forwards input.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Receiver;

use futures::StreamExt as _;
use gpui::{
    App, Bounds, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Point, Render, ScrollWheelEvent, SharedString, Size,
    Styled, StyledText, Task, TextRun, Window, canvas, div, font, px,
};
use term_core::alacritty_terminal::grid::{Dimensions, Scroll};
use term_core::alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use term_core::alacritty_terminal::term::TermMode;
use term_core::alacritty_terminal::term::cell::Flags;
use term_core::alacritty_terminal::vte::ansi::Rgb;
use term_core::mouse::{WheelAction, wheel};
use term_core::selection;
use term_core::{Session, TerminalEvent, TerminalSize};

use crate::input::keystroke_to_bytes;
use crate::theme::{self, FONT_FAMILY, FONT_SIZE, LINE_HEIGHT, PADDING, Theme};

/// Notifications for the view's owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalViewEvent {
    /// The title changed; tab labels should refresh.
    TitleChanged,
    /// The program exited; the owner should close this view.
    Exited,
    /// The user clicked the view; it now has keyboard focus.
    Focused,
}

/// Renders a [`Session`] and sends keyboard input to it.
pub struct TerminalView {
    session: Session,
    focus: FocusHandle,
    theme: Theme,
    size: TerminalSize,
    title: Option<SharedString>,
    /// Laid-out bounds of the view, measured during prepaint.
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Width of one cell at the last render.
    cell_width: Pixels,
    /// Wheel pixels not yet turned into whole lines.
    scroll_px: f32,
    /// A left-button drag is extending the selection.
    selecting: bool,
    _events: Task<()>,
}

impl EventEmitter<TerminalViewEvent> for TerminalView {}

impl TerminalView {
    /// Wraps a running session. Its events drive redraws.
    pub fn new(session: Session, events: Receiver<TerminalEvent>, cx: &mut Context<Self>) -> Self {
        let events = cx.spawn(async move |this, cx| {
            let mut events = crate::bridge::to_stream("terminal-events", events);
            while let Some(event) = events.next().await {
                if this
                    .update(cx, |view, cx| view.on_terminal_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });

        Self {
            session,
            focus: cx.focus_handle(),
            theme: Theme::default(),
            size: TerminalSize::default(),
            title: None,
            bounds: Rc::new(Cell::new(None)),
            cell_width: px(FONT_SIZE * 0.6),
            scroll_px: 0.0,
            selecting: false,
            _events: events,
        }
    }

    fn on_terminal_event(&mut self, event: TerminalEvent, cx: &mut Context<Self>) {
        match event {
            TerminalEvent::Wakeup => cx.notify(),
            TerminalEvent::Title(title) => {
                self.title = Some(title.into());
                cx.emit(TerminalViewEvent::TitleChanged);
                cx.notify();
            }
            TerminalEvent::ResetTitle => {
                self.title = None;
                cx.emit(TerminalViewEvent::TitleChanged);
                cx.notify();
            }
            TerminalEvent::ClipboardStore(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            TerminalEvent::Exited(_) => cx.emit(TerminalViewEvent::Exited),
            TerminalEvent::Bell => {}
        }
    }

    /// Title set by the program, if any.
    pub fn title(&self) -> Option<&SharedString> {
        self.title.as_ref()
    }

    /// Process id of the program the pane started (the shell).
    pub fn child_pid(&self) -> u32 {
        self.session.child_pid()
    }

    /// Types `command` and Enter into the shell, as if the user did.
    pub fn run_command(&self, command: &str) {
        // A closed session already triggered `Exited`; nothing to do.
        let _ = self.session.write(format!("{command}\r").into_bytes());
    }

    /// Current directory of the shell, used to open new panes next to it.
    pub fn working_directory(&self) -> Option<PathBuf> {
        self.session.working_directory()
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let mods = keystroke.modifiers;

        if mods.control && mods.shift && keystroke.key == "v" {
            self.paste(cx);
            cx.stop_propagation();
            return;
        }
        if mods.control && mods.shift && keystroke.key == "c" {
            self.copy(cx);
            cx.stop_propagation();
            return;
        }

        let app_cursor = self
            .session
            .term()
            .lock()
            .mode()
            .contains(TermMode::APP_CURSOR);
        if let Some(bytes) = keystroke_to_bytes(keystroke, app_cursor) {
            let mut term = self.session.term().lock();
            term.scroll_display(Scroll::Bottom);
            // Typing ends the selection, as in other terminals.
            term.selection = None;
            drop(term);
            // A closed session already triggered `Exited`; nothing to do.
            let _ = self.session.write(bytes);
            cx.stop_propagation();
        }
    }

    /// Mouse wheel and touchpad: scrollback, arrows, or a mouse report.
    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let line_height = FONT_SIZE * LINE_HEIGHT;
        self.scroll_px += f32::from(event.delta.pixel_delta(px(line_height)).y);
        let lines = take_lines(&mut self.scroll_px, line_height);
        cx.stop_propagation();
        if lines == 0 {
            return;
        }

        let (cell, _) = self.cell_at(event.position);
        let mut term = self.session.term().lock();
        match wheel(*term.mode(), lines, cell) {
            Some(WheelAction::Scrollback(lines)) => {
                term.scroll_display(Scroll::Delta(lines));
                drop(term);
                cx.notify();
            }
            Some(WheelAction::Send(bytes)) => {
                drop(term);
                // A closed session already triggered `Exited`; nothing to do.
                let _ = self.session.write(bytes);
            }
            None => {}
        }
    }

    /// Viewport cell (column, screen line) under a window position, and which
    /// half of the cell was hit.
    fn cell_at(&self, position: Point<Pixels>) -> ((usize, usize), Side) {
        let origin = self.bounds.get().map_or_else(Point::default, |b| b.origin);
        let x = f32::from(position.x - origin.x) - PADDING;
        let y = f32::from(position.y - origin.y) - PADDING;
        let (column, side) = cell_side(x, f32::from(self.cell_width), self.size.cols);
        let line = cell_index(y, FONT_SIZE * LINE_HEIGHT, self.size.rows);
        ((column, line), side)
    }

    /// Left press: focus the view and start a selection (one click drags,
    /// two select a word, three a line).
    fn on_left_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus);
        cx.emit(TerminalViewEvent::Focused);
        let (cell, side) = self.cell_at(event.position);
        let mut term = self.session.term().lock();
        selection::start(&mut *term, selection::kind(event.click_count), cell, side);
        drop(term);
        self.selecting = true;
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selecting || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let (cell, side) = self.cell_at(event.position);
        selection::extend(&mut *self.session.term().lock(), cell, side);
        cx.notify();
    }

    /// Left release: offer the selection to middle-click paste.
    fn on_left_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.selecting) {
            return;
        }
        if let Some(text) = selection::text(&*self.session.term().lock()) {
            cx.write_to_primary(ClipboardItem::new_string(text));
        }
    }

    fn copy(&self, cx: &mut Context<Self>) {
        if let Some(text) = selection::text(&*self.session.term().lock()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let bracketed = self
            .session
            .term()
            .lock()
            .mode()
            .contains(TermMode::BRACKETED_PASTE);
        let bytes = if bracketed {
            // Strip the end marker so pasted text cannot leave paste mode.
            let clean = text.replace("\x1b[201~", "");
            format!("\x1b[200~{clean}\x1b[201~")
        } else {
            text.replace("\r\n", "\r").replace('\n', "\r")
        };
        let _ = self.session.write(bytes.into_bytes());
    }

    /// Fits the grid to the view; resizes the PTY when the size changed.
    fn fit_to(&mut self, area: Size<Pixels>, cell_width: Pixels, line_height: Pixels) {
        let usable_width = f32::from(area.width) - 2.0 * PADDING;
        let usable_height = f32::from(area.height) - 2.0 * PADDING;
        let size = TerminalSize::new(
            cells_that_fit(usable_width, f32::from(cell_width)),
            cells_that_fit(usable_height, f32::from(line_height)),
            cells_that_fit(f32::from(cell_width), 1.0),
            cells_that_fit(f32::from(line_height), 1.0),
        );
        if size != self.size {
            self.size = size;
            let _ = self.session.resize(size);
        }
    }

    /// Builds one styled text element per visible row.
    fn rows(&self, line_height: Pixels) -> Vec<gpui::Div> {
        let term = self.session.term().lock();
        let colors = term.colors();
        let grid = term.grid();
        let offset = i32::try_from(grid.display_offset()).unwrap_or(0);
        let cursor = term
            .mode()
            .contains(TermMode::SHOW_CURSOR)
            .then_some(grid.cursor.point);
        let selected = term.selection.as_ref().and_then(|s| s.to_range(&*term));

        (0..term.screen_lines())
            .map(|row| {
                let line = Line(i32::try_from(row).unwrap_or(0) - offset);
                let mut builder = RowBuilder::default();
                for col in 0..term.columns() {
                    let cell = &grid[line][Column(col)];
                    if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                        continue;
                    }
                    let mut fg = self.theme.resolve(cell.fg, colors);
                    let mut bg = self.theme.resolve(cell.bg, colors);
                    if cell.flags.contains(Flags::INVERSE) {
                        std::mem::swap(&mut fg, &mut bg);
                    }
                    if selected.is_some_and(|r| r.contains(GridPoint::new(line, Column(col)))) {
                        fg = self.theme.foreground;
                        bg = self.theme.selection;
                    }
                    if cursor.is_some_and(|c| c.line == line && c.column == Column(col)) {
                        fg = self.theme.background;
                        bg = self.theme.cursor;
                    }
                    let style = CellStyle {
                        fg,
                        bg: (bg != self.theme.background).then_some(bg),
                        bold: cell.flags.contains(Flags::BOLD),
                        italic: cell.flags.contains(Flags::ITALIC),
                        underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
                    };
                    builder.push(cell.c, cell.zerowidth(), style);
                }
                div()
                    .h(line_height)
                    .whitespace_nowrap()
                    .child(builder.finish())
            })
            .collect()
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let font_size = px(FONT_SIZE);
        let line_height = px(FONT_SIZE * LINE_HEIGHT);
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&font(FONT_FAMILY));
        let cell_width = text_system
            .advance(font_id, font_size, 'm')
            .map_or(px(FONT_SIZE * 0.6), |advance| advance.width);

        // Until the first layout, the size comes from the window.
        let area = self
            .bounds
            .get()
            .map_or_else(|| window.viewport_size(), |bounds| bounds.size);
        self.fit_to(area, cell_width, line_height);
        self.cell_width = cell_width;

        let measured = Rc::clone(&self.bounds);
        let measure = canvas(
            move |bounds, window, _| {
                let resized = measured.get().map(|b| b.size) != Some(bounds.size);
                // The origin maps wheel positions to cells; keep it current.
                measured.set(Some(bounds));
                if resized {
                    // Re-render with the new size on the next frame.
                    window.refresh();
                }
            },
            |_, (), _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        div()
            .id("terminal")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_left_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_left_up))
            .relative()
            .overflow_hidden()
            .size_full()
            .p(px(PADDING))
            .bg(theme::to_hsla(self.theme.background))
            .text_color(theme::to_hsla(self.theme.foreground))
            .font_family(FONT_FAMILY)
            .text_size(font_size)
            .line_height(line_height)
            .child(measure)
            .children(self.rows(line_height))
    }
}

/// Whole cells of `cell` pixels that fit in `available` pixels (at least 1).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value is clamped to 1..=u16::MAX before the cast"
)]
fn cells_that_fit(available: f32, cell: f32) -> u16 {
    if cell <= 0.0 {
        return 1;
    }
    (available / cell).floor().clamp(1.0, f32::from(u16::MAX)) as u16
}

/// Most lines one wheel event may scroll; bounds a runaway touchpad fling.
const MAX_WHEEL_LINES: f32 = 10_000.0;

/// Takes the whole lines out of the wheel pixels accumulated so far, keeping
/// the remainder so slow touchpad scrolls still add up.
#[allow(
    clippy::cast_possible_truncation,
    reason = "value is truncated and clamped to MAX_WHEEL_LINES before the cast"
)]
fn take_lines(accumulated: &mut f32, line_height: f32) -> i32 {
    if line_height <= 0.0 {
        return 0;
    }
    let lines = (*accumulated / line_height)
        .trunc()
        .clamp(-MAX_WHEEL_LINES, MAX_WHEEL_LINES);
    *accumulated -= lines * line_height;
    lines as i32
}

/// Cell `offset` pixels into a row of `count` cells, and which half of it
/// was hit; past either end it is the outer half of the edge cell.
fn cell_side(offset: f32, cell: f32, count: u16) -> (usize, Side) {
    let index = cell_index(offset, cell, count);
    if cell <= 0.0 || offset < 0.0 {
        return (index, Side::Left);
    }
    let cells = offset / cell;
    let side = if cells >= f32::from(count) || cells.fract() >= 0.5 {
        Side::Right
    } else {
        Side::Left
    };
    (index, side)
}

/// Index of the cell `offset` pixels into a row of `count` cells.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value is clamped to 0..count before the cast"
)]
fn cell_index(offset: f32, cell: f32, count: u16) -> usize {
    if cell <= 0.0 || count == 0 {
        return 0;
    }
    (offset / cell).floor().clamp(0.0, f32::from(count - 1)) as usize
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct CellStyle {
    fg: Rgb,
    bg: Option<Rgb>,
    bold: bool,
    italic: bool,
    underline: bool,
}

/// Accumulates a row's text, merging neighbor cells with the same style.
#[derive(Default)]
struct RowBuilder {
    text: String,
    runs: Vec<(usize, CellStyle)>,
}

impl RowBuilder {
    fn push(&mut self, ch: char, zerowidth: Option<&[char]>, style: CellStyle) {
        let start = self.text.len();
        self.text.push(ch);
        for &extra in zerowidth.unwrap_or_default() {
            self.text.push(extra);
        }
        let len = self.text.len() - start;
        match self.runs.last_mut() {
            Some((run_len, last)) if *last == style => *run_len += len,
            _ => self.runs.push((len, style)),
        }
    }

    fn finish(self) -> StyledText {
        let runs = self
            .runs
            .into_iter()
            .map(|(len, style)| {
                let mut run_font = font(FONT_FAMILY);
                if style.bold {
                    run_font.weight = FontWeight::BOLD;
                }
                if style.italic {
                    run_font.style = gpui::FontStyle::Italic;
                }
                TextRun {
                    len,
                    font: run_font,
                    color: theme::to_hsla(style.fg),
                    background_color: style.bg.map(theme::to_hsla),
                    underline: style.underline.then(|| gpui::UnderlineStyle {
                        thickness: px(1.0),
                        color: None::<Hsla>,
                        wavy: false,
                    }),
                    strikethrough: None,
                }
            })
            .collect();
        StyledText::new(self.text).with_runs(runs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_lines_keeps_the_remainder() {
        let mut acc: f32 = 25.0;
        assert_eq!(take_lines(&mut acc, 10.0), 2);
        assert!((acc - 5.0).abs() < f32::EPSILON);
        acc -= 17.0;
        assert_eq!(take_lines(&mut acc, 10.0), -1);
        assert!((acc + 2.0).abs() < f32::EPSILON);
        assert_eq!(take_lines(&mut acc, 10.0), 0);
        assert_eq!(take_lines(&mut acc, 0.0), 0);
    }

    #[test]
    fn cell_side_splits_each_cell_in_halves() {
        assert_eq!(cell_side(22.0, 10.0, 80), (2, Side::Left));
        assert_eq!(cell_side(27.0, 10.0, 80), (2, Side::Right));
        assert_eq!(cell_side(-4.0, 10.0, 80), (0, Side::Left));
        assert_eq!(cell_side(5000.0, 10.0, 80), (79, Side::Right));
        assert_eq!(cell_side(5.0, 0.0, 80), (0, Side::Left));
    }

    #[test]
    fn cell_index_clamps_to_the_grid() {
        assert_eq!(cell_index(25.0, 10.0, 80), 2);
        assert_eq!(cell_index(-4.0, 10.0, 80), 0);
        assert_eq!(cell_index(5000.0, 10.0, 80), 79);
        assert_eq!(cell_index(5.0, 0.0, 80), 0);
    }

    #[test]
    fn cells_that_fit_floors_and_clamps() {
        assert_eq!(cells_that_fit(100.0, 8.0), 12);
        assert_eq!(cells_that_fit(-5.0, 8.0), 1);
        assert_eq!(cells_that_fit(100.0, 0.0), 1);
    }

    #[test]
    fn row_builder_merges_same_style_runs() {
        let style = CellStyle {
            fg: Rgb { r: 1, g: 1, b: 1 },
            bg: None,
            bold: false,
            italic: false,
            underline: false,
        };
        let bold = CellStyle {
            bold: true,
            ..style
        };
        let mut row = RowBuilder::default();
        row.push('a', None, style);
        row.push('ñ', None, style);
        row.push('b', None, bold);
        assert_eq!(row.text, "añb");
        assert_eq!(row.runs, vec![(3, style), (1, bold)]);
    }
}
