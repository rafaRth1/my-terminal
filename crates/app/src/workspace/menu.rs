//! Right-click menus: a small card at the click position, drawn above
//! everything else, that closes on an outside click.

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, InteractiveElement, IntoElement, ParentElement,
    Pixels, Point, StatefulInteractiveElement, Styled, Window, anchored, deferred, div, px,
};

use super::{Workspace, chrome};

/// One menu entry.
pub(super) fn menu_item(
    id: impl Into<ElementId>,
    label: impl Into<gpui::SharedString>,
    cx: &mut Context<Workspace>,
    on_click: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) -> AnyElement {
    div()
        .id(id.into())
        .px_3()
        .py_1p5()
        .rounded_md()
        .whitespace_nowrap()
        .hover(|item| item.bg(chrome::color(chrome::HOVER_BG)))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            on_click(this, window, cx);
        }))
        .child(label.into())
        .into_any_element()
}

/// The menu card with `items` at `position`; `dismiss` runs on an outside
/// click.
pub(super) fn context_menu(
    id: &'static str,
    position: Point<Pixels>,
    items: Vec<AnyElement>,
    cx: &mut Context<Workspace>,
    dismiss: impl Fn(&mut Workspace) + 'static,
) -> AnyElement {
    deferred(
        anchored().position(position).child(
            div()
                .id(id)
                .occlude()
                .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                    dismiss(this);
                    cx.notify();
                }))
                .flex()
                .flex_col()
                .p_1()
                .rounded_md()
                .border_1()
                .border_color(chrome::color(chrome::BORDER))
                .bg(chrome::color(chrome::TITLEBAR_BG))
                .shadow_lg()
                .text_size(px(12.0))
                .text_color(chrome::color(chrome::TEXT))
                .children(items),
        ),
    )
    .with_priority(1)
    .into_any_element()
}
