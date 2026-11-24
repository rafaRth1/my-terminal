//! The active tab's panes side by side, with draggable dividers between
//! them, and the title bar buttons that split and unsplit panes.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, Context, DragMoveEvent, ElementId, EmptyView, InteractiveElement,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px, svg,
};

use super::agent_bar::TextTooltip;
use super::{Workspace, chrome};
use crate::actions::SplitRight;

/// Width of the grab area between two panes; the visible line is 1 px.
const DIVIDER_WIDTH: f32 = 5.0;
/// Line on top of the focused pane when the tab has several.
const FOCUS_LINE: f32 = 2.0;
const BUTTON_GLYPH: f32 = 15.0;

/// Drag payload: index of the divider being moved.
struct PaneDivider(usize);

impl Workspace {
    pub(super) fn render_panes(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(tab) = self.tabs.active_tab() else {
            return div().id("panes").flex_1().min_h_0();
        };
        let count = tab.panes().len();
        let active = tab.active_pane().clone();
        let mut children: Vec<AnyElement> = Vec::with_capacity(count * 2);
        for (index, (view, &width)) in tab.panes().iter().zip(tab.widths()).enumerate() {
            if index > 0 {
                children.push(divider(index - 1));
            }
            let focused = *view == active;
            let mut pane = div()
                .flex_basis(px(0.0))
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .when(count > 1, |pane| {
                    pane.child(
                        div()
                            .flex_none()
                            .h(px(FOCUS_LINE))
                            .when(focused, |line| line.bg(chrome::color(chrome::WORKING))),
                    )
                })
                .child(div().flex_1().min_h_0().child(view.clone()));
            pane.style().flex_grow = Some(width);
            children.push(pane.into_any_element());
        }

        div()
            .id("panes")
            .flex()
            .flex_1()
            .min_h_0()
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<PaneDivider>, _, cx| {
                    let divider = event.drag(cx).0;
                    // Dividers take fixed space; the panes share the rest.
                    let dividers = px(DIVIDER_WIDTH * small(count - 1));
                    let space = event.bounds.size.width - dividers;
                    if space <= px(0.0) {
                        return;
                    }
                    let before = px(DIVIDER_WIDTH * small(divider) + DIVIDER_WIDTH / 2.0);
                    let x = event.event.position.x - event.bounds.left() - before;
                    this.tabs.move_divider(divider, x / space);
                    cx.notify();
                }),
            )
            .children(children)
    }

    /// Moves `pane` out of the split into its own tab; the session keeps
    /// running and stays in the sidebar.
    pub(super) fn remove_from_split(
        &mut self,
        pane: &super::Pane,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.move_out_of_split(pane) {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Split right and remove-from-split buttons for the title bar.
    pub(super) fn render_pane_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let split = self
            .tabs
            .active_tab()
            .is_some_and(|tab| tab.panes().len() > 1);
        div()
            .flex()
            .flex_none()
            .h_full()
            .child(
                pane_button(
                    "split-right",
                    "ui/split-right.svg",
                    "Split right (Ctrl+Shift+D)",
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.split_right(&SplitRight, window, cx);
                })),
            )
            .when(split, |row| {
                row.child(
                    pane_button(
                        "remove-from-split",
                        "ui/unsplit.svg",
                        "Remove from split (the session keeps running)",
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(pane) = this.tabs.active_pane().cloned() {
                            this.remove_from_split(&pane, window, cx);
                        }
                    })),
                )
            })
    }
}

/// Grab area between pane `index` and the next one.
fn divider(index: usize) -> AnyElement {
    div()
        .id(ElementId::NamedInteger("pane-divider".into(), index as u64))
        .flex()
        .flex_none()
        .justify_center()
        .w(px(DIVIDER_WIDTH))
        .h_full()
        .cursor_col_resize()
        .group("pane-divider")
        .child(
            div()
                .w(px(1.0))
                .h_full()
                .bg(chrome::color(chrome::BORDER))
                .group_hover("pane-divider", |line| {
                    line.bg(chrome::color(chrome::WORKING))
                }),
        )
        .on_drag(PaneDivider(index), |_, _, _, cx| cx.new(|_| EmptyView))
        .into_any_element()
}

fn pane_button(
    id: &'static str,
    icon: &'static str,
    tooltip: &'static str,
) -> gpui::Stateful<gpui::Div> {
    let tooltip = SharedString::from(tooltip);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(super::TITLEBAR_HEIGHT))
        .h_full()
        .hover(|style| style.bg(chrome::color(chrome::HOVER_BG)))
        .tooltip(move |_, cx| cx.new(|_| TextTooltip(tooltip.clone())).into())
        .child(
            svg()
                .path(icon)
                .size(px(BUTTON_GLYPH))
                .text_color(chrome::color(chrome::TEXT_MUTED)),
        )
}

/// A pane count or index as `f32`; tabs never hold 65 535 panes.
fn small(n: usize) -> f32 {
    f32::from(u16::try_from(n).unwrap_or(u16::MAX))
}
