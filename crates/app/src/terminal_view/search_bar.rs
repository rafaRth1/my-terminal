//! Search bar of a terminal pane (Ctrl+Shift+F): finds text in the screen
//! and history, highlights every match and steps through them. Enter goes to
//! the previous (older) match, Shift+Enter to the next one, Esc closes.

use gpui::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};
use term_core::search::{self, Match};

use super::TerminalView;
use crate::text_field::{TextField, TextFieldEvent};
use crate::workspace::chrome;

const BAR_WIDTH: f32 = 320.0;

pub(super) struct SearchBar {
    field: Entity<TextField>,
    pub(super) matches: Vec<Match>,
    /// Index into `matches` of the match the view shows.
    pub(super) current: Option<usize>,
    _subscription: Subscription,
}

impl TerminalView {
    /// Opens the search bar, or focuses it when already open.
    pub fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(bar) = &self.search {
            bar.field.read(cx).focus(window);
            return;
        }
        let field = cx.new(|cx| TextField::new("Find", false, cx));
        let subscription = cx.subscribe_in(&field, window, |this, _, event, window, cx| {
            this.on_search_event(*event, window, cx);
        });
        field.read(cx).focus(window);
        self.search = Some(SearchBar {
            field,
            matches: Vec::new(),
            current: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn on_search_event(
        &mut self,
        event: TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Changed => self.find(true, cx),
            TextFieldEvent::Submit => self.step(false),
            TextFieldEvent::SubmitBack => self.step(true),
            TextFieldEvent::Cancel => self.close_search(window),
            TextFieldEvent::Next => return,
        }
        cx.notify();
    }

    fn close_search(&mut self, window: &mut Window) {
        self.search = None;
        window.focus(&self.focus);
    }

    /// Runs the query again. With `jump`, shows the newest match; otherwise
    /// keeps the current index (new output only adds lines at the bottom).
    pub(super) fn find(&mut self, jump: bool, cx: &Context<Self>) {
        let Some(bar) = self.search.as_mut() else {
            return;
        };
        let query = bar.field.read(cx).text().to_owned();
        let mut term = self.session.term().lock();
        bar.matches = search::find_all(&*term, &query);
        let last = bar.matches.len().checked_sub(1);
        bar.current = if jump {
            last
        } else {
            bar.current.filter(|&i| i < bar.matches.len()).or(last)
        };
        if jump && let Some(found) = bar.current.and_then(|i| bar.matches.get(i)) {
            search::reveal(&mut *term, found);
        }
    }

    /// Moves to the next older (`down` false) or newer match, wrapping.
    fn step(&mut self, down: bool) {
        let Some(bar) = self.search.as_mut() else {
            return;
        };
        let len = bar.matches.len();
        let Some(current) = bar.current.filter(|_| len > 0) else {
            return;
        };
        let next = if down {
            (current + 1) % len
        } else {
            (current + len - 1) % len
        };
        bar.current = Some(next);
        search::reveal(&mut *self.session.term().lock(), &bar.matches[next]);
    }

    pub(super) fn render_search_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = self.search.as_ref()?;
        let has_query = !bar.field.read(cx).text().is_empty();
        let count: SharedString = match (bar.current, bar.matches.len()) {
            (_, 0) if has_query => "No results".into(),
            (_, 0) => SharedString::default(),
            (Some(i), n) => format!("{}/{n}", i + 1).into(),
            (None, n) => format!("{n}").into(),
        };
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .flex_none()
                .px_1()
                .rounded_sm()
                .text_color(chrome::color(chrome::TEXT_MUTED))
                .hover(|b| b.bg(chrome::color(chrome::HOVER_BG)))
                .child(label)
        };
        Some(
            div()
                .id("search-bar")
                .occlude()
                .absolute()
                .top(px(6.0))
                .right(px(12.0))
                .w(px(BAR_WIDTH))
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(chrome::color(chrome::BORDER))
                .bg(chrome::color(chrome::TITLEBAR_BG))
                .shadow_lg()
                .font_family(".SystemUIFont")
                .text_size(px(12.0))
                .child(div().flex_1().min_w_0().child(bar.field.clone()))
                .child(
                    div()
                        .flex_none()
                        .text_color(chrome::color(chrome::TEXT_FAINT))
                        .child(count),
                )
                .child(
                    button("search-prev", "↑").on_click(cx.listener(|this, _, _, cx| {
                        this.step(false);
                        cx.notify();
                    })),
                )
                .child(
                    button("search-next", "↓").on_click(cx.listener(|this, _, _, cx| {
                        this.step(true);
                        cx.notify();
                    })),
                )
                .child(
                    button("search-close", "✕").on_click(cx.listener(|this, _, window, cx| {
                        this.close_search(window);
                        cx.notify();
                    })),
                )
                .into_any_element(),
        )
    }
}
