//! Right-click menu of a sidebar session: adds a session that is not on
//! screen to the visible split, takes one out of it, or forgets a saved
//! session that was not resumed.

use gpui::{AnyElement, Context, Pixels, Point, Window};

use super::Workspace;
use super::menu::{context_menu, menu_item};

#[derive(Clone, Copy)]
enum SessionAction {
    AddToSplit,
    RemoveFromSplit,
    /// Drop a saved session that was not resumed.
    Forget,
}

#[derive(Clone, Copy)]
pub(super) struct SessionMenu {
    /// Sidebar item id of the session.
    pane: u64,
    action: SessionAction,
    position: Point<Pixels>,
}

impl Workspace {
    /// Opens the menu when the session has something to offer: sessions off
    /// screen can join the split, sessions in a split of two or more can
    /// leave it.
    pub(super) fn open_session_menu(
        &mut self,
        pane: u64,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.is_dormant(pane) {
            self.session_menu = Some(SessionMenu {
                pane,
                action: SessionAction::Forget,
                position,
            });
            cx.notify();
            return;
        }
        let Some(view) = self.pane_by_id(pane) else {
            return;
        };
        let split = self.tabs.active_tab().map(crate::tabs::Tab::panes);
        let action = match split {
            Some(panes) if panes.contains(&view) => {
                if panes.len() < 2 {
                    return;
                }
                SessionAction::RemoveFromSplit
            }
            _ => SessionAction::AddToSplit,
        };
        self.session_menu = Some(SessionMenu {
            pane,
            action,
            position,
        });
        cx.notify();
    }

    fn run_session_action(
        &mut self,
        menu: SessionMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session_menu = None;
        if let SessionAction::Forget = menu.action {
            self.forget_dormant(menu.pane, cx);
            return;
        }
        let Some(pane) = self.pane_by_id(menu.pane) else {
            cx.notify();
            return;
        };
        match menu.action {
            SessionAction::AddToSplit => {
                if self.tabs.move_into_active_split(&pane) {
                    self.focus_active(window, cx);
                }
                cx.notify();
            }
            SessionAction::RemoveFromSplit => self.remove_from_split(&pane, window, cx),
            SessionAction::Forget => {}
        }
    }

    pub(super) fn render_session_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.session_menu?;
        let label = match menu.action {
            SessionAction::AddToSplit => "Add to split",
            SessionAction::RemoveFromSplit => "Remove from split",
            SessionAction::Forget => "Forget session",
        };
        let items = vec![menu_item(
            "session-menu-action",
            label,
            cx,
            move |this, window, cx| this.run_session_action(menu, window, cx),
        )];
        Some(context_menu(
            "session-menu",
            menu.position,
            items,
            cx,
            |this| this.session_menu = None,
        ))
    }
}
