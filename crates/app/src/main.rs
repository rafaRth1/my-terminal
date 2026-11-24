//! my-terminal desktop app.

mod accounts;
mod actions;
mod agents;
mod assets;
mod bridge;
mod config;
mod decorations;
mod git;
mod input;
mod instance;
mod notify;
mod open_link;
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
    let Start::Run(listener) = claim_instance() else {
        return;
    };
    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            if let Some(listener) = listener {
                bring_to_front_on_request(listener, cx);
            }
            actions::bind_default_keys(cx);
            let config = config::default_path().map_or_else(config::Config::default, |path| {
                let (config, problems) = config::load(&path);
                for problem in problems {
                    eprintln!("my-terminal: {}: {problem}", path.display());
                }
                config
            });
            cx.set_global(theme::TerminalSettings::from_config(&config));

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

/// Whether this start runs the app.
enum Start {
    /// Run, listening on the socket when there is one.
    Run(Option<std::os::unix::net::UnixListener>),
    /// Another app runs and was asked to come to the front.
    Exit,
}

fn claim_instance() -> Start {
    let Some(path) = instance::default_path() else {
        return Start::Run(None);
    };
    match instance::claim(&path) {
        Ok(instance::Claim::Secondary) => {
            eprintln!("my-terminal: already running; brought its window to the front");
            Start::Exit
        }
        Ok(instance::Claim::Primary(listener, lock)) => {
            // The lock must last as long as the process; the OS releases it
            // when the process ends.
            std::mem::forget(lock);
            Start::Run(Some(listener))
        }
        Err(err) => {
            eprintln!("my-terminal: running without single-instance: {err}");
            Start::Run(None)
        }
    }
}

/// Activates the window whenever another start asks for it.
fn bring_to_front_on_request(listener: std::os::unix::net::UnixListener, cx: &mut App) {
    use futures::StreamExt as _;
    let (sender, receiver) = std::sync::mpsc::channel();
    instance::listen(listener, sender);
    let mut requests = bridge::to_stream("instance-requests", receiver);
    cx.spawn(async move |cx| {
        while requests.next().await.is_some() {
            let _ = cx.update(|cx| {
                if let Some(window) = cx.windows().first() {
                    let _ = window.update(cx, |_, window, _| window.activate_window());
                }
            });
        }
    })
    .detach();
}
