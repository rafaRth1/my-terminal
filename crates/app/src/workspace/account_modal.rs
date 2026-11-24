//! Claude accounts dialog, opened from the Claude icon: open Claude with an
//! account (it also becomes the one new panes use), sign in the default
//! login, add an account by pasting a token from `claude setup-token`,
//! rename or remove one.

use gpui::{AnyElement, ClickEvent, Div, ElementId, KeyDownEvent, Stateful, Subscription, hsla};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AppContext as _, Context, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use super::{DEFAULT_ACCOUNT, PaneAccount, TOKEN_ENV, Workspace, chrome};
use crate::actions::ACCOUNT_MODAL_CONTEXT;
use crate::agents::{AGENTS, Agent};
use crate::text_field::{TextField, TextFieldEvent};

/// Command a new tab runs to get a long-lived token.
const SETUP_TOKEN_COMMAND: &str = "claude setup-token";
const MODAL_WIDTH: f32 = 440.0;

/// Which page of the dialog is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Page {
    List,
    Add,
    Rename(String),
    Remove(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Token,
}

/// State of the open (or parked) dialog.
pub(super) struct AccountModal {
    page: Page,
    name: Entity<TextField>,
    token: Entity<TextField>,
    focus: FocusHandle,
    error: Option<SharedString>,
    _subscriptions: [Subscription; 2],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ButtonKind {
    Primary,
    Plain,
    Danger,
}

impl Workspace {
    /// Opens the dialog, restoring one parked while getting a token.
    pub(super) fn open_accounts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for skipped in self.accounts.rescan() {
            eprintln!("my-terminal: skipped account: {skipped}");
        }
        let modal = self
            .parked_modal
            .take()
            .unwrap_or_else(|| Self::new_modal(window, cx));
        self.account_modal = Some(modal);
        self.focus_page(window, cx);
        cx.notify();
    }

    fn new_modal(window: &mut Window, cx: &mut Context<Self>) -> AccountModal {
        let name = cx.new(|cx| TextField::new("e.g. work", false, cx));
        let token = cx.new(|cx| TextField::new("paste the token here", true, cx));
        let subscriptions = [
            cx.subscribe_in(&name, window, |this, _, event, window, cx| {
                this.on_field_event(Field::Name, *event, window, cx);
            }),
            cx.subscribe_in(&token, window, |this, _, event, window, cx| {
                this.on_field_event(Field::Token, *event, window, cx);
            }),
        ];
        AccountModal {
            page: Page::List,
            name,
            token,
            focus: cx.focus_handle(),
            error: None,
            _subscriptions: subscriptions,
        }
    }

    /// Closes the dialog and forgets anything typed, the token included.
    fn close_accounts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.account_modal = None;
        self.parked_modal = None;
        self.focus_active(window, cx);
        cx.notify();
    }

    fn show_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = self.account_modal.as_mut() else {
            return;
        };
        let name = match &page {
            Page::Rename(old) => old.clone(),
            _ => String::new(),
        };
        modal.name.update(cx, |field, cx| field.set_text(&name, cx));
        modal.token.update(cx, |field, cx| field.set_text("", cx));
        modal.error = None;
        modal.page = page;
        self.focus_page(window, cx);
        cx.notify();
    }

    /// Puts the keyboard where the current page expects it.
    fn focus_page(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = &self.account_modal else {
            return;
        };
        match modal.page {
            Page::Add | Page::Rename(_) => modal.name.read(cx).focus(window),
            Page::List | Page::Remove(_) => window.focus(&modal.focus),
        }
    }

    fn on_field_event(
        &mut self,
        field: Field,
        event: TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Submit | TextFieldEvent::SubmitBack => self.submit_page(window, cx),
            TextFieldEvent::Cancel => self.back(window, cx),
            TextFieldEvent::Changed => {}
            TextFieldEvent::Next => {
                let Some(modal) = &self.account_modal else {
                    return;
                };
                if modal.page == Page::Add {
                    let next = match field {
                        Field::Name => &modal.token,
                        Field::Token => &modal.name,
                    };
                    next.read(cx).focus(window);
                }
            }
        }
    }

    /// Escape: from a form back to the list, from the list out.
    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.account_modal.as_ref().map(|modal| &modal.page) {
            Some(Page::List) | None => self.close_accounts(window, cx),
            Some(_) => self.show_page(Page::List, window, cx),
        }
    }

    fn on_modal_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => self.back(window, cx),
            "enter" => self.submit_page(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Saves the current form. On success the list shows again; on failure
    /// the form stays with the reason.
    fn submit_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = &self.account_modal else {
            return;
        };
        let name = modal.name.read(cx).text().to_owned();
        let result = match modal.page.clone() {
            Page::List => {
                self.open_claude(None, window, cx);
                return;
            }
            Page::Add => {
                let token = modal.token.read(cx).text().to_owned();
                self.accounts.add(&name, &token)
            }
            Page::Rename(old) => self.accounts.rename(&old, &name),
            Page::Remove(old) => self.accounts.remove(&old),
        };
        match result {
            Ok(()) => {
                self.account_error = None;
                self.show_page(Page::List, window, cx);
            }
            Err(err) => {
                if let Some(modal) = self.account_modal.as_mut() {
                    modal.error = Some(err.to_string().into());
                }
                cx.notify();
            }
        }
    }

    fn select_account(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.accounts.select(name) {
            self.account_error = None;
        }
        cx.notify();
    }

    /// Opens Claude in a new tab with `name` (or the active account), which
    /// becomes the account new panes use, and closes the dialog. On failure
    /// the dialog stays with the reason.
    fn open_claude(&mut self, name: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(name) = name {
            self.select_account(name, cx);
        }
        let pane = self.spawn_pane(window, cx);
        self.start_claude(pane, |agent| agent.program, window, cx);
    }

    /// Runs `claude auth login` on the stored login in a new tab.
    fn sign_in_default(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.spawn_pane_as(PaneAccount::default(), window, cx);
        self.start_claude(pane, |agent| agent.login, window, cx);
    }

    fn start_claude(
        &mut self,
        pane: Option<super::Pane>,
        command: fn(&Agent) -> &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let claude = AGENTS.iter().find(|agent| agent.accounts);
        let (Some(pane), Some(claude)) = (pane, claude) else {
            let reason = self
                .account_error
                .clone()
                .unwrap_or_else(|| "cannot open a new tab".into());
            if let Some(modal) = self.account_modal.as_mut() {
                modal.error = Some(reason);
            }
            cx.notify();
            return;
        };
        pane.read(cx).run_command(command(claude));
        self.account_modal = None;
        self.parked_modal = None;
        self.tabs.open_tab(pane);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Hides the dialog and opens a tab running `claude setup-token` on the
    /// stored login, so the user can sign in and copy the token. Reopening
    /// the dialog brings the form back.
    fn open_setup_token_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.spawn_pane_as(PaneAccount::default(), window, cx) else {
            if let Some(modal) = self.account_modal.as_mut() {
                modal.error = Some("cannot open a new tab".into());
            }
            cx.notify();
            return;
        };
        pane.read(cx).run_command(SETUP_TOKEN_COMMAND);
        // Keep the typed name, never a pasted secret, while the dialog waits.
        if let Some(modal) = &self.account_modal {
            modal.token.update(cx, |field, cx| field.set_text("", cx));
        }
        self.parked_modal = self.account_modal.take();
        self.tabs.open_tab(pane);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// The dialog over the whole window, if open.
    pub(super) fn render_accounts(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.account_modal.as_ref()?;
        let body = match &modal.page {
            Page::List => self.render_list(cx),
            Page::Add => Self::render_add(modal, cx),
            Page::Rename(old) => Self::render_rename(modal, old, cx),
            Page::Remove(name) => Self::render_remove(name, cx),
        };
        let card = div()
            .id("accounts-modal")
            .key_context(ACCOUNT_MODAL_CONTEXT)
            .track_focus(&modal.focus)
            .on_key_down(cx.listener(Self::on_modal_key))
            .w(px(MODAL_WIDTH))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(chrome::color(chrome::BORDER))
            .bg(chrome::color(chrome::SIDEBAR_BG))
            .text_size(px(13.0))
            .text_color(chrome::color(chrome::TEXT))
            .shadow_lg()
            .child(body)
            .when_some(modal.error.clone(), |card, error| {
                card.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(chrome::color(chrome::WAITING))
                        .child(error),
                )
            });
        Some(
            div()
                .id("accounts-overlay")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(0.0, 0.0, 0.0, 0.55))
                .occlude()
                .child(card)
                .into_any_element(),
        )
    }

    fn render_list(&self, cx: &mut Context<Self>) -> Div {
        let active = self.accounts.active_name().to_owned();
        let rows: Vec<AnyElement> = self
            .accounts
            .names()
            .into_iter()
            .map(str::to_owned)
            .enumerate()
            .map(|(index, name)| {
                let active = name == active;
                self.render_row(index, name, active, cx)
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(header("Claude accounts", cx))
            .child(faint(
                "Open starts Claude with that account; it stays active (●) for new \
                 tabs. Enter opens the active one. Running panes keep theirs.",
            ))
            .child(div().flex().flex_col().gap_1().children(rows))
            .child(div().flex().justify_end().child(
                button("add-account", "Add account", ButtonKind::Primary).on_click(
                    cx.listener(|this, _, window, cx| this.show_page(Page::Add, window, cx)),
                ),
            ))
    }

    fn render_row(
        &self,
        index: usize,
        name: String,
        active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_default = name == DEFAULT_ACCOUNT;
        let select_name = name.clone();
        let open_name = name.clone();
        let default_note = if self.inherited_token {
            format!("Token from {TOKEN_ENV}")
        } else {
            "Login stored by claude (/login)".to_owned()
        };
        div()
            .id(ElementId::NamedInteger("account-row".into(), index as u64))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1p5()
            .rounded_md()
            .when(active, |row| row.bg(chrome::color(chrome::SELECTED_BG)))
            .hover(|row| row.bg(chrome::color(chrome::HOVER_BG)))
            .on_click(cx.listener(move |this, _, _, cx| this.select_account(&select_name, cx)))
            .child(
                div()
                    .flex_none()
                    .text_color(chrome::color(if active {
                        chrome::WORKING
                    } else {
                        chrome::TEXT_FAINT
                    }))
                    .child(if active { "●" } else { "○" }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(div().overflow_hidden().text_ellipsis().child(name.clone()))
                    .when(is_default, |col| col.child(faint(default_note))),
            )
            .when(is_default, |row| {
                row.child(
                    button("sign-in-default", "Sign in", ButtonKind::Plain).on_click(cx.listener(
                        |this, _: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            this.sign_in_default(window, cx);
                        },
                    )),
                )
            })
            .when(!is_default, |row| {
                let rename = name.clone();
                let remove = name;
                row.child(
                    button(
                        ElementId::NamedInteger("rename-account".into(), index as u64),
                        "Rename",
                        ButtonKind::Plain,
                    )
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            this.show_page(Page::Rename(rename.clone()), window, cx);
                        },
                    )),
                )
                .child(
                    button(
                        ElementId::NamedInteger("remove-account".into(), index as u64),
                        "Remove",
                        ButtonKind::Danger,
                    )
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            this.show_page(Page::Remove(remove.clone()), window, cx);
                        },
                    )),
                )
            })
            .child(
                button(
                    ElementId::NamedInteger("open-account".into(), index as u64),
                    "Open",
                    ButtonKind::Primary,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_claude(Some(&open_name), window, cx);
                })),
            )
            .into_any_element()
    }

    fn render_add(modal: &AccountModal, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(header("Add account", cx))
            .child(label("Name"))
            .child(modal.name.clone())
            .child(label("Token"))
            .child(modal.token.clone())
            .child(faint(
                "1. Open a tab with claude setup-token and sign in with this account \
                 in the browser. 2. Select the token it prints and copy it with \
                 Ctrl+Shift+C. 3. Come back here (Claude icon) and paste it with Ctrl+V.",
            ))
            .child(div().flex().child(
                button("setup-token", "Open claude setup-token", ButtonKind::Plain).on_click(
                    cx.listener(|this, _, window, cx| {
                        this.open_setup_token_tab(window, cx);
                    }),
                ),
            ))
            .child(form_buttons("Save", ButtonKind::Primary, cx))
    }

    fn render_rename(modal: &AccountModal, old: &str, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(header(format!("Rename {old}"), cx))
            .child(label("New name"))
            .child(modal.name.clone())
            .child(form_buttons("Save", ButtonKind::Primary, cx))
    }

    fn render_remove(name: &str, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(header(format!("Remove {name}?"), cx))
            .child(faint(
                "Its token file is deleted. Panes already running with it keep \
                 working until you close them.",
            ))
            .child(form_buttons("Remove", ButtonKind::Danger, cx))
    }
}

/// Title row with a close button.
fn header(title: impl Into<SharedString>, cx: &mut Context<Workspace>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(15.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .child(
            button("close-accounts", "✕", ButtonKind::Plain)
                .on_click(cx.listener(|this, _, window, cx| this.close_accounts(window, cx))),
        )
}

/// Cancel and confirm buttons of a form.
fn form_buttons(confirm: &'static str, kind: ButtonKind, cx: &mut Context<Workspace>) -> Div {
    div()
        .flex()
        .justify_end()
        .gap_2()
        .child(
            button("cancel-form", "Cancel", ButtonKind::Plain).on_click(
                cx.listener(|this, _, window, cx| this.show_page(Page::List, window, cx)),
            ),
        )
        .child(
            button("confirm-form", confirm, kind)
                .on_click(cx.listener(|this, _, window, cx| this.submit_page(window, cx))),
        )
}

fn button(id: impl Into<ElementId>, text: &'static str, kind: ButtonKind) -> Stateful<Div> {
    let (fg, border) = match kind {
        ButtonKind::Primary => (chrome::TEXT, chrome::WORKING),
        ButtonKind::Plain => (chrome::TEXT_MUTED, chrome::BORDER),
        ButtonKind::Danger => (chrome::CLOSE_HOVER_BG, chrome::BORDER),
    };
    div()
        .id(id.into())
        .flex_none()
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(chrome::color(border))
        .text_color(chrome::color(fg))
        .when(kind == ButtonKind::Primary, |b| {
            b.bg(chrome::color(chrome::SELECTED_BG))
        })
        .hover(|b| b.bg(chrome::color(chrome::HOVER_BG)))
        .child(text)
}

fn label(text: &'static str) -> Div {
    div()
        .text_size(px(12.0))
        .text_color(chrome::color(chrome::TEXT_MUTED))
        .child(text)
}

fn faint(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.0))
        .text_color(chrome::color(chrome::TEXT_FAINT))
        .child(text.into())
}
