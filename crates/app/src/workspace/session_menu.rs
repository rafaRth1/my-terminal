//! Right-click menu of a sidebar session: adds a session that is not on
//! screen to the visible split, takes one out of it, renames it, or forgets
//! a saved session that was not resumed.

use gpui::{AnyElement, Context, ElementId, Pixels, Point, Window};

use super::Workspace;
use super::menu::{context_menu, menu_item};

#[derive(Clone, Copy, Debug)]
enum SessionAction {
    AddToSplit,
    RemoveFromSplit,
    Rename,
    /// Drop a saved session that was not resumed.
    Forget,
}

impl SessionAction {
    fn label(self) -> &'static str {
        match self {
            Self::AddToSplit => "Add to split",
            Self::RemoveFromSplit => "Remove from split",
            Self::Rename => "Rename",
            Self::Forget => "Forget session",
        }
    }
}

#[derive(Clone)]
pub(super) struct SessionMenu {
    /// Sidebar item id of the session.
    pane: u64,
    /// Title shown in the row, the starting text of a rename.
    title: String,
    actions: Vec<SessionAction>,
    position: Point<Pixels>,
}

impl Workspace {
    /// Opens the menu of sidebar item `pane`: saved sessions can be renamed
    /// or forgotten; live ones renamed, and added to the split when off
    /// screen or removed from a split of two or more.
    pub(super) fn open_session_menu(
        &mut self,
        pane: u64,
        title: String,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let actions = if self.is_dormant(pane) {
            vec![SessionAction::Rename, SessionAction::Forget]
        } else {
            let Some(view) = self.pane_by_id(pane) else {
                return;
            };
            let split = self.tabs.active_tab().map(crate::tabs::Tab::panes);
            let split_action = match split {
                Some(panes) if panes.contains(&view) => {
                    (panes.len() > 1).then_some(SessionAction::RemoveFromSplit)
                }
                _ => Some(SessionAction::AddToSplit),
            };
            split_action
                .into_iter()
                .chain([SessionAction::Rename])
                .collect()
        };
        self.session_menu = Some(SessionMenu {
            pane,
            title,
            actions,
            position,
        });
        cx.notify();
    }

    fn run_session_action(
        &mut self,
        pane: u64,
        title: &str,
        action: SessionAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session_menu = None;
        match action {
            SessionAction::Forget => self.forget_dormant(pane, cx),
            SessionAction::Rename => self.start_rename(pane, title, window, cx),
            SessionAction::AddToSplit => {
                if let Some(view) = self.pane_by_id(pane)
                    && self.tabs.move_into_active_split(&view)
                {
                    self.focus_active(window, cx);
                }
                cx.notify();
            }
            SessionAction::RemoveFromSplit => {
                if let Some(view) = self.pane_by_id(pane) {
                    self.remove_from_split(&view, window, cx);
                }
            }
        }
    }

    pub(super) fn render_session_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.session_menu.clone()?;
        let items = menu
            .actions
            .iter()
            .map(|&action| {
                let title = menu.title.clone();
                let pane = menu.pane;
                menu_item(
                    ElementId::Name(format!("session-menu-{action:?}").into()),
                    action.label(),
                    cx,
                    move |this, window, cx| {
                        this.run_session_action(pane, &title, action, window, cx);
                    },
                )
            })
            .collect();
        Some(context_menu(
            "session-menu",
            menu.position,
            items,
            cx,
            |this| this.session_menu = None,
        ))
    }
}
