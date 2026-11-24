//! A one-line text input, optionally masked for secrets.
//!
//! Minimal on purpose: typing appends, Backspace deletes the last character,
//! Ctrl+U clears, Ctrl+V pastes. There is no cursor movement or selection.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent,
    Keystroke, MouseButton, ParentElement, Render, SharedString, Styled, Window, div, px,
};

use crate::workspace::chrome;

/// Longest text a field keeps; real tokens are a few hundred characters.
const MAX_CHARS: usize = 4096;
/// Most dots a masked field shows, however long the secret is.
const MAX_DOTS: usize = 24;

/// What the field asks its owner to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFieldEvent {
    /// Enter: submit the form.
    Submit,
    /// Escape: leave the form.
    Cancel,
    /// Tab: move to the next field.
    Next,
    /// The text changed.
    Changed,
}

/// One-line input. `Debug` is not derived so a secret cannot end up in logs.
pub struct TextField {
    text: String,
    focus: FocusHandle,
    placeholder: SharedString,
    masked: bool,
}

impl EventEmitter<TextFieldEvent> for TextField {}

impl TextField {
    pub fn new(placeholder: impl Into<SharedString>, masked: bool, cx: &mut Context<Self>) -> Self {
        Self {
            text: String::new(),
            focus: cx.focus_handle(),
            placeholder: placeholder.into(),
            masked,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.text.clear();
        insert(&mut self.text, text);
        cx.emit(TextFieldEvent::Changed);
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match edit(&mut self.text, &event.keystroke) {
            Edit::Changed => {
                cx.emit(TextFieldEvent::Changed);
                cx.notify();
            }
            Edit::Paste => {
                if let Some(pasted) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    insert(&mut self.text, &pasted);
                    cx.emit(TextFieldEvent::Changed);
                    cx.notify();
                }
            }
            Edit::Submit => cx.emit(TextFieldEvent::Submit),
            Edit::Cancel => cx.emit(TextFieldEvent::Cancel),
            Edit::Next => cx.emit(TextFieldEvent::Next),
            Edit::Ignored => return,
        }
        cx.stop_propagation();
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let (shown, faint) = if self.text.is_empty() {
            (self.placeholder.to_string(), true)
        } else if self.masked {
            (masked(&self.text), false)
        } else {
            (self.text.clone(), false)
        };
        div()
            .id("text-field")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, _| this.focus(window)),
            )
            .flex()
            .items_center()
            .h(px(30.0))
            .px_2()
            .rounded_md()
            .border_1()
            .border_color(chrome::color(if focused {
                chrome::WORKING
            } else {
                chrome::BORDER
            }))
            .bg(chrome::color(chrome::TAB_ACTIVE_BG))
            .overflow_hidden()
            .whitespace_nowrap()
            .child(
                div()
                    .when(faint, |text| {
                        text.text_color(chrome::color(chrome::TEXT_FAINT))
                    })
                    .child(shown),
            )
            .when(focused, |field| {
                field.child(div().w(px(1.0)).h(px(16.0)).bg(chrome::color(chrome::TEXT)))
            })
    }
}

/// What a key press did to the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    Changed,
    Paste,
    Submit,
    Cancel,
    Next,
    Ignored,
}

fn edit(text: &mut String, keystroke: &Keystroke) -> Edit {
    let mods = keystroke.modifiers;
    match keystroke.key.as_str() {
        "enter" => return Edit::Submit,
        "escape" => return Edit::Cancel,
        "tab" => return Edit::Next,
        "backspace" => {
            return if text.pop().is_some() {
                Edit::Changed
            } else {
                Edit::Ignored
            };
        }
        _ => {}
    }
    if mods.control && !mods.alt && keystroke.key == "v" {
        return Edit::Paste;
    }
    if mods.control && !mods.shift && keystroke.key == "u" {
        text.clear();
        return Edit::Changed;
    }
    if mods.control || mods.alt || mods.platform {
        return Edit::Ignored;
    }
    match keystroke.key_char.as_deref() {
        Some(typed) if !typed.is_empty() && !typed.chars().any(char::is_control) => {
            insert(text, typed);
            Edit::Changed
        }
        _ => Edit::Ignored,
    }
}

/// Appends pasted or typed text, dropping trailing line breaks. Keeps one
/// character past [`MAX_CHARS`] so an over-long paste is rejected on submit
/// instead of being cut silently.
fn insert(text: &mut String, added: &str) {
    let room = (MAX_CHARS + 1).saturating_sub(text.chars().count());
    text.extend(added.trim_end_matches(['\r', '\n']).chars().take(room));
}

/// Dots plus the length, so a paste can be checked without showing it.
fn masked(text: &str) -> String {
    let count = text.chars().count();
    format!("{} ({count} characters)", "•".repeat(count.min(MAX_DOTS)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Keystroke;

    fn key(text: &str) -> Keystroke {
        Keystroke::parse(text).unwrap()
    }

    /// A key press that types `ch`, as the platform reports it.
    fn typed(text: &str, ch: &str) -> Keystroke {
        Keystroke {
            key_char: Some(ch.to_owned()),
            ..key(text)
        }
    }

    #[test]
    fn printable_keys_append() {
        let mut text = String::from("ab");
        assert_eq!(edit(&mut text, &typed("c", "c")), Edit::Changed);
        assert_eq!(edit(&mut text, &typed("shift-a", "A")), Edit::Changed);
        assert_eq!(edit(&mut text, &typed("space", " ")), Edit::Changed);
        assert_eq!(edit(&mut text, &typed("ñ", "ñ")), Edit::Changed);
        assert_eq!(text, "abcA ñ");
    }

    #[test]
    fn backspace_and_ctrl_u_delete() {
        let mut text = String::from("añb");
        assert_eq!(edit(&mut text, &key("backspace")), Edit::Changed);
        assert_eq!(text, "añ");
        assert_eq!(edit(&mut text, &key("backspace")), Edit::Changed);
        assert_eq!(text, "a");
        assert_eq!(edit(&mut text, &key("ctrl-u")), Edit::Changed);
        assert_eq!(text, "");
        assert_eq!(edit(&mut text, &key("backspace")), Edit::Ignored);
    }

    #[test]
    fn control_keys_map_to_actions() {
        let mut text = String::from("x");
        assert_eq!(edit(&mut text, &key("enter")), Edit::Submit);
        assert_eq!(edit(&mut text, &key("escape")), Edit::Cancel);
        assert_eq!(edit(&mut text, &key("tab")), Edit::Next);
        assert_eq!(edit(&mut text, &key("ctrl-v")), Edit::Paste);
        assert_eq!(edit(&mut text, &key("ctrl-shift-v")), Edit::Paste);
        assert_eq!(edit(&mut text, &typed("ctrl-c", "c")), Edit::Ignored);
        assert_eq!(edit(&mut text, &typed("alt-x", "x")), Edit::Ignored);
        assert_eq!(edit(&mut text, &key("left")), Edit::Ignored);
        assert_eq!(text, "x");
    }

    #[test]
    fn paste_drops_trailing_line_breaks_and_caps_length() {
        let mut text = String::from("a");
        insert(&mut text, "tok-123\r\n");
        assert_eq!(text, "atok-123");
        insert(&mut text, "in\nner");
        assert_eq!(
            text, "atok-123in\nner",
            "inner breaks are kept for validation"
        );
        insert(&mut text, &"z".repeat(MAX_CHARS * 2));
        // One past the limit, so the form can still tell it was too long.
        assert_eq!(text.chars().count(), MAX_CHARS + 1);
    }

    #[test]
    fn masked_display_never_shows_the_text() {
        assert_eq!(masked("secret"), "•••••• (6 characters)");
        let long = masked(&"q".repeat(300));
        assert!(!long.contains('q'));
        assert!(long.ends_with("(300 characters)"));
        assert_eq!(long.chars().filter(|c| *c == '•').count(), MAX_DOTS);
    }
}
