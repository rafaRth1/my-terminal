//! my-terminal desktop app.

mod accounts;
mod actions;
mod agents;
mod assets;
mod bridge;
mod decorations;
mod input;
mod saved;
mod sidebar;
mod tabs;
mod terminal_view;
mod text_field;
mod theme;
mod transcript;
mod workspace;

use gpui::{
    App, AppContext as _, Application, Bounds, WindowBackgroundAppearance, WindowBounds,
    WindowDecorations, WindowOptions, px, size,
};

use crate::workspace::Workspace;

fn main() {
    Application::new()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            actions::bind_default_keys(cx);

            let bounds = Bounds::centered(None, size(px(1100.0), px(700.0)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_decorations: Some(WindowDecorations::Client),
                // The client-side frame's resize band around the window is see-through.
                window_background: WindowBackgroundAppearance::Transparent,
                app_id: Some("my-terminal".into()),
                ..WindowOptions::default()
            };
            let opened = cx.open_window(options, |window, cx| {
                cx.new(|cx| Workspace::new(window, cx))
            });
            if let Err(err) = opened {
                eprintln!("my-terminal: cannot open window: {err}");
                cx.quit();
            }
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
