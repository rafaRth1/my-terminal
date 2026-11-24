//! Renaming a session from the sidebar: the row's title becomes a text
//! field; Enter keeps the name, Esc cancels, an empty name goes back to the
//! automatic title. Names are saved with the session.

use gpui::{
    AnyElement, AppContext as _, Context, Entity, IntoElement, SharedString, Subscription, Window,
};

use super::Workspace;
use crate::saved;
use crate::text_field::{TextField, TextFieldEvent};

/// The row being renamed.
pub(super) struct Renaming {
    /// Sidebar item id.
    pane: u64,
    field: Entity<TextField>,
    _subscription: Subscription,
}

impl Workspace {
    /// Turns the title of sidebar item `pane` into a text field holding
    /// `current`.
    pub(super) fn start_rename(
        &mut self,
        pane: u64,
        current: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let field = cx.new(|cx| TextField::new("Session name", false, cx));
        field.update(cx, |field, cx| field.set_text(current, cx));
        let subscription = cx.subscribe_in(&field, window, |this, _, event, window, cx| {
            this.on_rename_event(*event, window, cx);
        });
        field.read(cx).focus(window);
        self.renaming = Some(Renaming {
            pane,
            field,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn on_rename_event(
        &mut self,
        event: TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Submit | TextFieldEvent::SubmitBack => {
                if let Some(renaming) = self.renaming.take() {
                    let name = saved::clean_name(renaming.field.read(cx).text());
                    self.set_session_name(renaming.pane, name);
                    self.save_sessions(cx);
                }
            }
            TextFieldEvent::Cancel => self.renaming = None,
            TextFieldEvent::Changed | TextFieldEvent::Next => return,
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Sets (or with `None` clears) the name of a live or saved session.
    fn set_session_name(&mut self, pane: u64, name: Option<String>) {
        if let Some(meta) = self.panes.values_mut().find(|meta| meta.id == pane) {
            meta.name = name.map(SharedString::from);
        } else if let Some(dormant) = self.dormant.iter_mut().find(|d| d.id == pane) {
            dormant.session.name = name;
        }
    }

    /// The text field when sidebar item `pane` is being renamed.
    pub(super) fn rename_field(&self, pane: u64) -> Option<AnyElement> {
        self.renaming
            .as_ref()
            .filter(|renaming| renaming.pane == pane)
            .map(|renaming| renaming.field.clone().into_any_element())
    }
}
