//! Client-side window frame: an invisible border that resizes the window,
//! rounded top corners, a thin outline and a drop shadow.
//!
//! The compositor draws nothing around a client-decorated window, so the
//! frame reserves [`INSET`] pixels on every free (non-tiled) side. That band
//! is transparent, holds the shadow and starts a compositor resize on click.
//! Tiled or maximized sides get no band, no rounding and no shadow.

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Bounds, BoxShadow, CursorStyle, Decorations, HitboxBehavior, Hsla,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point, ResizeEdge, Size,
    Styled, Tiling, Window, canvas, div, point, px, rgb, transparent_black,
};

/// Width of the invisible resize band, which also holds the shadow.
pub const INSET: Pixels = px(10.0);
/// How far a corner grab area reaches along each edge.
const CORNER: Pixels = px(24.0);
/// Corner radius of the outline.
const RADIUS: Pixels = px(10.0);
/// Corner radius of the content, which sits inside the 1 px outline.
pub const CONTENT_RADIUS: Pixels = px(9.0);
const OUTLINE: u32 = 0x0003_0303;

/// Which top corners of the window are rounded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rounding {
    pub top_left: bool,
    pub top_right: bool,
}

impl Rounding {
    /// A corner is rounded only when neither side it touches is tiled.
    pub fn from_decorations(decorations: Decorations) -> Self {
        match decorations {
            Decorations::Server => Self::default(),
            Decorations::Client { tiling } => Self {
                top_left: !(tiling.top || tiling.left),
                top_right: !(tiling.top || tiling.right),
            },
        }
    }

    pub fn left_only(self) -> Self {
        Self {
            top_right: false,
            ..self
        }
    }

    pub fn right_only(self) -> Self {
        Self {
            top_left: false,
            ..self
        }
    }

    /// Rounds the matching top corners of `element` with [`CONTENT_RADIUS`].
    pub fn apply<E: Styled + FluentBuilder>(self, element: E) -> E {
        element
            .when(self.top_left, |el| el.rounded_tl(CONTENT_RADIUS))
            .when(self.top_right, |el| el.rounded_tr(CONTENT_RADIUS))
    }
}

/// Wraps the window content in the client-side frame. Owns the hovered edge
/// so the resize cursor is repainted only when it changes.
#[derive(Default)]
pub struct WindowFrame {
    hovered_edge: Rc<Cell<Option<ResizeEdge>>>,
}

impl WindowFrame {
    pub fn render(&self, window: &mut Window, content: impl IntoElement) -> AnyElement {
        let Decorations::Client { tiling } = window.window_decorations() else {
            window.set_client_inset(px(0.0));
            return div().size_full().child(content).into_any_element();
        };
        window.set_client_inset(INSET);
        let rounding = Rounding::from_decorations(Decorations::Client { tiling });
        let hovered = self.hovered_edge.clone();

        // Full-window hitbox; the resize cursor is set only over the band.
        let cursor = canvas(
            |_, window, _| {
                let size = window.window_bounds().get_bounds().size;
                window.insert_hitbox(Bounds::new(Point::default(), size), HitboxBehavior::Normal)
            },
            move |_, hitbox, window, _| {
                let size = window.window_bounds().get_bounds().size;
                if let Some(edge) = resize_edge(window.mouse_position(), size, tiling) {
                    window.set_cursor_style(cursor_for(edge), &hitbox);
                }
            },
        )
        .size_full()
        .absolute();

        div()
            .id("window-frame")
            .size_full()
            .bg(transparent_black())
            .when(!tiling.top, |el| el.pt(INSET))
            .when(!tiling.bottom, |el| el.pb(INSET))
            .when(!tiling.left, |el| el.pl(INSET))
            .when(!tiling.right, |el| el.pr(INSET))
            .child(cursor)
            .on_mouse_move(move |event, window, _| {
                let size = window.window_bounds().get_bounds().size;
                let edge = resize_edge(event.position, size, tiling);
                if hovered.replace(edge) != edge {
                    window.refresh();
                }
            })
            .on_mouse_down(MouseButton::Left, move |event, window, _| {
                let size = window.window_bounds().get_bounds().size;
                if let Some(edge) = resize_edge(event.position, size, tiling) {
                    window.start_window_resize(edge);
                }
            })
            .child(
                div()
                    .size_full()
                    .border_color(rgb(OUTLINE))
                    .when(!tiling.top, Styled::border_t_1)
                    .when(!tiling.bottom, Styled::border_b_1)
                    .when(!tiling.left, Styled::border_l_1)
                    .when(!tiling.right, Styled::border_r_1)
                    .when(rounding.top_left, |el| el.rounded_tl(RADIUS))
                    .when(rounding.top_right, |el| el.rounded_tr(RADIUS))
                    .when(!tiling.is_tiled(), |el| el.shadow(vec![shadow()]))
                    .child(content),
            )
            .into_any_element()
    }
}

/// Which edge or corner of a window of `size` the point `pos` would resize.
/// `None` inside the content or on a tiled side.
pub fn resize_edge(pos: Point<Pixels>, size: Size<Pixels>, tiling: Tiling) -> Option<ResizeEdge> {
    let near = |reach: Pixels| {
        (
            !tiling.top && pos.y < reach,
            !tiling.bottom && pos.y >= size.height - reach,
            !tiling.left && pos.x < reach,
            !tiling.right && pos.x >= size.width - reach,
        )
    };
    let (top, bottom, left, right) = near(INSET);
    let (near_top, near_bottom, near_left, near_right) = near(CORNER);

    let edge = if (top && near_left) || (left && near_top) {
        ResizeEdge::TopLeft
    } else if (top && near_right) || (right && near_top) {
        ResizeEdge::TopRight
    } else if (bottom && near_left) || (left && near_bottom) {
        ResizeEdge::BottomLeft
    } else if (bottom && near_right) || (right && near_bottom) {
        ResizeEdge::BottomRight
    } else if top {
        ResizeEdge::Top
    } else if bottom {
        ResizeEdge::Bottom
    } else if left {
        ResizeEdge::Left
    } else if right {
        ResizeEdge::Right
    } else {
        return None;
    };
    Some(edge)
}

fn cursor_for(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

fn shadow() -> BoxShadow {
    BoxShadow {
        color: Hsla {
            h: 0.0,
            s: 0.0,
            l: 0.0,
            a: 0.45,
        },
        blur_radius: INSET / 2.0,
        spread_radius: px(0.0),
        offset: point(px(0.0), px(1.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    fn window() -> Size<Pixels> {
        size(px(800.0), px(600.0))
    }

    fn at(x: f32, y: f32) -> Option<ResizeEdge> {
        resize_edge(point(px(x), px(y)), window(), Tiling::default())
    }

    #[test]
    fn interior_does_not_resize() {
        assert_eq!(at(400.0, 300.0), None);
        assert_eq!(at(10.0, 300.0), None, "first content column");
        assert_eq!(at(400.0, 589.9), None, "last content row");
    }

    #[test]
    fn bands_map_to_edges() {
        assert_eq!(at(400.0, 0.0), Some(ResizeEdge::Top));
        assert_eq!(at(400.0, 595.0), Some(ResizeEdge::Bottom));
        assert_eq!(at(3.0, 300.0), Some(ResizeEdge::Left));
        assert_eq!(at(795.0, 300.0), Some(ResizeEdge::Right));
    }

    #[test]
    fn corners_reach_along_both_edges() {
        assert_eq!(at(2.0, 2.0), Some(ResizeEdge::TopLeft));
        assert_eq!(at(20.0, 2.0), Some(ResizeEdge::TopLeft), "along top");
        assert_eq!(at(2.0, 20.0), Some(ResizeEdge::TopLeft), "along left");
        assert_eq!(at(780.0, 5.0), Some(ResizeEdge::TopRight));
        assert_eq!(at(5.0, 590.0), Some(ResizeEdge::BottomLeft));
        assert_eq!(at(795.0, 580.0), Some(ResizeEdge::BottomRight));
        assert_eq!(at(30.0, 2.0), Some(ResizeEdge::Top), "past the corner");
    }

    #[test]
    fn tiled_sides_never_resize() {
        let top_tiled = Tiling {
            top: true,
            ..Tiling::default()
        };
        let edge = |x: f32, y: f32| resize_edge(point(px(x), px(y)), window(), top_tiled);
        assert_eq!(edge(400.0, 2.0), None);
        assert_eq!(edge(2.0, 2.0), Some(ResizeEdge::Left), "corner falls back");
        assert_eq!(
            resize_edge(point(px(2.0), px(2.0)), window(), Tiling::tiled()),
            None,
            "maximized"
        );
    }

    #[test]
    fn rounding_follows_tiling() {
        let free = Decorations::Client {
            tiling: Tiling::default(),
        };
        assert_eq!(
            Rounding::from_decorations(free),
            Rounding {
                top_left: true,
                top_right: true
            }
        );
        let right_tiled = Decorations::Client {
            tiling: Tiling {
                right: true,
                ..Tiling::default()
            },
        };
        assert_eq!(
            Rounding::from_decorations(right_tiled),
            Rounding {
                top_left: true,
                top_right: false
            }
        );
        assert_eq!(
            Rounding::from_decorations(Decorations::Server),
            Rounding::default()
        );
        assert_eq!(
            Rounding::from_decorations(free).left_only(),
            Rounding {
                top_left: true,
                top_right: false
            }
        );
    }
}
