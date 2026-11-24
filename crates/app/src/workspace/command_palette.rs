//! Command palette (Ctrl+Shift+P): a search field over every action, open
//! session, saved session, agent and Claude account. ↑/↓ move, Enter runs,
//! Esc or a click outside closes.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, Context, ElementId, Entity, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, div, hsla, px,
};

use super::{Workspace, chrome};
use crate::actions::{
    COMMAND_PALETTE_CONTEXT, ClosePane, CloseTab, FindInTerminal, FocusSearch, NewTab, NextTab,
    PrevTab, SplitRight, ToggleSidebar,
};
use crate::agents::Click;
use crate::palette;
use crate::text_field::{TextField, TextFieldEvent};

const PALETTE_WIDTH: f32 = 560.0;
/// Rows shown at once; the search narrows the rest.
const MAX_ROWS: usize = 12;

/// What an entry does.
#[derive(Clone, Debug)]
enum Command {
    NewSession,
    SplitRight,
    RemoveFromSplit,
    ClosePane,
    CloseTab,
    NextTab,
    PrevTab,
    ToggleSidebar,
    ToggleGitPanel,
    FindInTerminal,
    SearchSessions,
    Settings,
    Accounts,
    Zoom(Option<i32>),
    SwitchAccount(String),
    OpenAgent(&'static str),
    GoTo(u64),
    Resume(u64),
}

struct Entry {
    label: String,
    hint: Option<&'static str>,
    command: Command,
}

pub(super) struct CommandPalette {
    field: Entity<TextField>,
    entries: Vec<Entry>,
    /// Index into the filtered list.
    selected: usize,
    _subscription: Subscription,
}

impl Workspace {
    pub(super) fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            self.focus_active(window, cx);
            cx.notify();
            return;
        }
        let field = cx.new(|cx| TextField::new("Type a command or a session", false, cx));
        let subscription =
            cx.subscribe_in(&field, window, |this, _, event, window, cx| match *event {
                TextFieldEvent::Changed => {
                    if let Some(palette) = this.palette.as_mut() {
                        palette.selected = 0;
                    }
                    cx.notify();
                }
                TextFieldEvent::Submit | TextFieldEvent::SubmitBack => {
                    this.run_selected(window, cx);
                }
                TextFieldEvent::Cancel => this.close_palette(window, cx),
                TextFieldEvent::Next => this.move_selection(1, cx),
            });
        field.read(cx).focus(window);
        self.palette = Some(CommandPalette {
            field,
            entries: self.palette_entries(cx),
            selected: 0,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Everything the palette offers right now.
    fn palette_entries(&self, cx: &Context<Self>) -> Vec<Entry> {
        let entry = |label: &str, hint: Option<&'static str>, command: Command| Entry {
            label: label.to_owned(),
            hint,
            command,
        };
        let split = self
            .tabs
            .active_tab()
            .is_some_and(|tab| tab.panes().len() > 1);
        let mut entries = vec![
            entry("New session", Some("Ctrl+Shift+T"), Command::NewSession),
            entry("Split right", Some("Ctrl+Shift+D"), Command::SplitRight),
        ];
        if split {
            entries.push(entry("Remove from split", None, Command::RemoveFromSplit));
        }
        entries.extend([
            entry("Close pane", Some("Ctrl+Shift+W"), Command::ClosePane),
            entry("Close tab", Some("Ctrl+Shift+Q"), Command::CloseTab),
            entry("Next tab", Some("Ctrl+Tab"), Command::NextTab),
            entry("Previous tab", Some("Ctrl+Shift+Tab"), Command::PrevTab),
            entry(
                "Toggle sidebar",
                Some("Ctrl+Shift+B"),
                Command::ToggleSidebar,
            ),
            entry(
                "Toggle git panel",
                Some("Ctrl+Shift+G"),
                Command::ToggleGitPanel,
            ),
            entry(
                "Find in terminal",
                Some("Ctrl+Shift+F"),
                Command::FindInTerminal,
            ),
            entry(
                "Search sessions",
                Some("Ctrl+Shift+K"),
                Command::SearchSessions,
            ),
            entry("Settings", None, Command::Settings),
            entry("Claude accounts", None, Command::Accounts),
            entry("Zoom in", Some("Ctrl+="), Command::Zoom(Some(1))),
            entry("Zoom out", Some("Ctrl+-"), Command::Zoom(Some(-1))),
            entry("Reset zoom", Some("Ctrl+0"), Command::Zoom(None)),
        ]);
        let active = self.accounts.active_name().to_owned();
        entries.extend(
            self.accounts
                .names()
                .into_iter()
                .filter(|name| *name != active)
                .map(|name| {
                    entry(
                        &format!("Switch Claude account: {name}"),
                        None,
                        Command::SwitchAccount(name.to_owned()),
                    )
                }),
        );
        entries.extend(
            self.agent_bar
                .iter()
                .filter(|status| status.installed)
                .map(|status| {
                    entry(
                        &format!("Open {}", status.agent.name),
                        None,
                        Command::OpenAgent(status.agent.id),
                    )
                }),
        );
        for item in self.sidebar_items(cx) {
            let (verb, command) = if item.dormant {
                ("Resume", Command::Resume(item.pane))
            } else {
                ("Go to", Command::GoTo(item.pane))
            };
            entries.push(entry(
                &format!("{verb}: {} — {}", item.title, item.project),
                None,
                command,
            ));
        }
        entries
    }

    /// Indexes of the entries matching the query, best first.
    fn palette_matches(palette: &CommandPalette, cx: &Context<Self>) -> Vec<usize> {
        let query = palette.field.read(cx).text().to_owned();
        palette::filter(&query, palette.entries.iter().map(|e| e.label.as_str()))
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.as_ref() else {
            return;
        };
        let count = Self::palette_matches(palette, cx).len().min(MAX_ROWS);
        if let Some(palette) = self.palette.as_mut()
            && count > 0
        {
            let current = isize::try_from(palette.selected).unwrap_or(0);
            let len = isize::try_from(count).unwrap_or(1);
            palette.selected = usize::try_from((current + delta).rem_euclid(len)).unwrap_or(0);
        }
        cx.notify();
    }

    fn on_palette_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = match event.keystroke.key.as_str() {
            "down" => 1,
            "up" => -1,
            _ => return,
        };
        self.move_selection(delta, cx);
        cx.stop_propagation();
    }

    fn run_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.as_ref() else {
            return;
        };
        let matches = Self::palette_matches(palette, cx);
        let Some(&index) = matches.get(palette.selected) else {
            return;
        };
        let command = palette.entries[index].command.clone();
        self.run_entry(command, window, cx);
    }

    /// Closes the palette, then does `command`.
    fn run_entry(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        self.close_palette(window, cx);
        match command {
            Command::NewSession => self.new_tab(&NewTab, window, cx),
            Command::SplitRight => self.split_right(&SplitRight, window, cx),
            Command::RemoveFromSplit => {
                if let Some(pane) = self.tabs.active_pane().cloned() {
                    self.remove_from_split(&pane, window, cx);
                }
            }
            Command::ClosePane => self.close_pane(&ClosePane, window, cx),
            Command::CloseTab => self.close_tab(&CloseTab, window, cx),
            Command::NextTab => self.next_tab(&NextTab, window, cx),
            Command::PrevTab => self.prev_tab(&PrevTab, window, cx),
            Command::ToggleSidebar => self.toggle_sidebar(&ToggleSidebar, window, cx),
            Command::ToggleGitPanel => self.toggle_git_panel(cx),
            Command::FindInTerminal => self.find_in_terminal(&FindInTerminal, window, cx),
            Command::SearchSessions => self.focus_search(&FocusSearch, window, cx),
            Command::Settings => self.open_settings(window, cx),
            Command::Accounts => self.open_accounts(window, cx),
            Command::Zoom(steps) => Self::zoom(steps, window, cx),
            Command::SwitchAccount(name) => {
                if self.accounts.select(&name) {
                    self.account_error = None;
                }
                cx.notify();
            }
            Command::OpenAgent(id) => {
                if let Some(status) = self.agent_bar.iter().copied().find(|s| s.agent.id == id) {
                    self.run_agent_action(status, Click::Open, window, cx);
                }
            }
            Command::GoTo(pane) => self.focus_pane_by_id(pane, window, cx),
            Command::Resume(pane) => self.resume_dormant(pane, window, cx),
        }
    }

    pub(super) fn render_palette(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let palette = self.palette.as_ref()?;
        let matches = Self::palette_matches(palette, cx);
        let rows: Vec<AnyElement> = matches
            .iter()
            .take(MAX_ROWS)
            .enumerate()
            .map(|(row, &index)| {
                let entry = &palette.entries[index];
                let command = entry.command.clone();
                let selected = row == palette.selected;
                div()
                    .id(ElementId::NamedInteger("palette-row".into(), row as u64))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1p5()
                    .rounded_md()
                    .when(selected, |r| r.bg(chrome::color(chrome::SELECTED_BG)))
                    .hover(|r| r.bg(chrome::color(chrome::HOVER_BG)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_entry(command.clone(), window, cx);
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(entry.label.clone()),
                    )
                    .when_some(entry.hint, |r, hint| {
                        r.child(
                            div()
                                .flex_none()
                                .text_size(px(11.0))
                                .text_color(chrome::color(chrome::TEXT_FAINT))
                                .child(hint),
                        )
                    })
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let card = div()
            .id("command-palette")
            .key_context(COMMAND_PALETTE_CONTEXT)
            .on_key_down(cx.listener(Self::on_palette_key))
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close_palette(window, cx)))
            .w(px(PALETTE_WIDTH))
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded_lg()
            .border_1()
            .border_color(chrome::color(chrome::BORDER))
            .bg(chrome::color(chrome::SIDEBAR_BG))
            .shadow_lg()
            .text_size(px(13.0))
            .text_color(chrome::color(chrome::TEXT))
            .child(palette.field.clone())
            .when(empty, |card| {
                card.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_color(chrome::color(chrome::TEXT_FAINT))
                        .child(SharedString::from("No matching command")),
                )
            })
            .children(rows);
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .justify_center()
                .pt(px(72.0))
                .bg(hsla(0.0, 0.0, 0.0, 0.25))
                .child(card)
                .into_any_element(),
        )
    }
}
