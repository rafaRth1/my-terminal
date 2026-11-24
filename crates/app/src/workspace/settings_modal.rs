//! Settings dialog, opened from the gear in the title bar: font family,
//! font size and theme. Changes apply at once to every pane and are saved
//! to `config.toml`.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, BorrowAppContext as _, ClickEvent, Context, Div, ElementId,
    Entity, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    SharedString, Stateful, StatefulInteractiveElement, Styled, Subscription, Window, div, hsla,
    px,
};

use super::{Workspace, chrome};
use crate::actions::SETTINGS_MODAL_CONTEXT;
use crate::config::{self, ThemeName};
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::{TerminalSettings, Theme};

const MODAL_WIDTH: f32 = 400.0;

pub(super) struct SettingsModal {
    family: Entity<TextField>,
    focus: FocusHandle,
    /// Where the settings were saved, or why they could not be.
    status: Option<SharedString>,
    _subscription: Subscription,
}

impl Workspace {
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = cx.global::<TerminalSettings>().font_family.to_string();
        let family = cx.new(|cx| TextField::new("Font family", false, cx));
        family.update(cx, |field, cx| field.set_text(&current, cx));
        let subscription =
            cx.subscribe_in(&family, window, |this, _, event, window, cx| match *event {
                TextFieldEvent::Submit | TextFieldEvent::SubmitBack => {
                    this.apply_family(window, cx);
                }
                TextFieldEvent::Cancel => this.close_settings(window, cx),
                TextFieldEvent::Changed | TextFieldEvent::Next => {}
            });
        let focus = cx.focus_handle();
        window.focus(&focus);
        self.settings_modal = Some(SettingsModal {
            family,
            focus,
            status: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_modal = None;
        self.focus_active(window, cx);
        cx.notify();
    }

    fn on_settings_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key == "escape" {
            self.close_settings(window, cx);
            cx.stop_propagation();
        }
    }

    fn apply_family(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = &self.settings_modal else {
            return;
        };
        let family = modal.family.read(cx).text().trim().to_owned();
        if family.is_empty() {
            self.set_settings_status("Font family cannot be empty".into(), cx);
            return;
        }
        self.change_settings(window, cx, |settings| settings.font_family = family.into());
    }

    /// Applies `change` to every pane and saves the result.
    fn change_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut TerminalSettings),
    ) {
        cx.update_global::<TerminalSettings, _>(|settings, _| change(settings));
        window.refresh();
        let status = match config::default_path() {
            None => "Not saved: no config directory".to_owned(),
            Some(path) => match config::save(&path, &cx.global::<TerminalSettings>().to_config()) {
                Ok(()) => format!("Saved to {}", path.display()),
                Err(err) => format!("Not saved: {err}"),
            },
        };
        self.set_settings_status(status.into(), cx);
    }

    fn set_settings_status(&mut self, status: SharedString, cx: &mut Context<Self>) {
        if let Some(modal) = self.settings_modal.as_mut() {
            modal.status = Some(status);
        }
        cx.notify();
    }

    fn step_font_size(&mut self, steps: i32, window: &mut Window, cx: &mut Context<Self>) {
        self.change_settings(window, cx, |settings| {
            let size = config::zoom(settings.base_font_size, steps);
            settings.base_font_size = size;
            settings.font_size = size;
        });
    }

    fn pick_theme(&mut self, name: ThemeName, window: &mut Window, cx: &mut Context<Self>) {
        self.change_settings(window, cx, |settings| {
            settings.theme_name = name;
            settings.theme = Theme::from_name(name);
        });
    }

    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.settings_modal.as_ref()?;
        let settings = cx.global::<TerminalSettings>();
        let (size, theme) = (settings.base_font_size, settings.theme_name);
        let theme_button = |name: ThemeName, label: &'static str| {
            button(
                ElementId::Name(format!("theme-{label}").into()),
                label,
                theme == name,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.pick_theme(name, window, cx);
            }))
        };
        let card = div()
            .id("settings-modal")
            .key_context(SETTINGS_MODAL_CONTEXT)
            .track_focus(&modal.focus)
            .on_key_down(cx.listener(Self::on_settings_key))
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Settings"),
                    )
                    .child(button("close-settings", "✕", false).on_click(cx.listener(
                        |this, _: &ClickEvent, window, cx| this.close_settings(window, cx),
                    ))),
            )
            .child(label("Font family (Enter to apply)"))
            .child(modal.family.clone())
            .child(label("Font size"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(button("font-smaller", "−", false).on_click(cx.listener(
                        |this, _: &ClickEvent, window, cx| this.step_font_size(-1, window, cx),
                    )))
                    .child(div().min_w(px(32.0)).child(format!("{size}")))
                    .child(button("font-bigger", "+", false).on_click(cx.listener(
                        |this, _: &ClickEvent, window, cx| this.step_font_size(1, window, cx),
                    ))),
            )
            .child(label("Theme"))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(theme_button(ThemeName::Dark, "Dark"))
                    .child(theme_button(ThemeName::Light, "Light")),
            )
            .child(faint(
                "Changes apply to every pane at once. The theme colors the terminal; \
                 Ctrl+= / Ctrl+- zoom only until the app closes.",
            ))
            .when_some(modal.status.clone(), |card, status| {
                card.child(faint(status))
            });
        Some(
            div()
                .id("settings-overlay")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(0.0, 0.0, 0.0, 0.55))
                .occlude()
                .on_mouse_down_out(
                    cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                )
                .child(card)
                .into_any_element(),
        )
    }
}

fn button(id: impl Into<ElementId>, text: &'static str, selected: bool) -> Stateful<Div> {
    div()
        .id(id.into())
        .flex_none()
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(chrome::color(if selected {
            chrome::WORKING
        } else {
            chrome::BORDER
        }))
        .when(selected, |b| b.bg(chrome::color(chrome::SELECTED_BG)))
        .text_color(chrome::color(chrome::TEXT))
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
