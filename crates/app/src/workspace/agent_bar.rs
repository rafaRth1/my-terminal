//! Agent bar at the bottom of the sidebar: one icon per agent CLI, with a dot
//! when it is signed in. Click opens the agent in a new tab (or its sign-in
//! when signed out); Claude's click opens the accounts dialog instead. Right
//! click shows a menu with open, sign in and the install/repair page. Agents
//! that are not installed show dimmed and open their install page.

use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, Context, ElementId, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, Pixels, Point, Render, SharedString, StatefulInteractiveElement,
    Styled, Window, div, px, svg,
};

use super::menu::{context_menu, menu_item};
use super::{PaneAccount, Workspace, chrome};
use crate::agents::{self, AgentStatus, Click, SignIn};

/// How often sign-in states are checked again while the app runs.
pub(super) const AGENT_PROBE_INTERVAL: Duration = Duration::from_secs(300);
const ICON_SIZE: f32 = 28.0;
const DOT_SIZE: f32 = 8.0;
/// Size of the logo inside the icon.
const ICON_GLYPH: f32 = 16.0;
/// Opacity of agents that are not installed.
const MISSING_OPACITY: f32 = 0.35;

/// Right-click menu of one agent, at the click position.
#[derive(Clone, Copy)]
pub(super) struct AgentMenu {
    status: AgentStatus,
    position: Point<Pixels>,
}

impl Workspace {
    /// Checks every agent off the UI thread; one probe at a time.
    pub(super) fn probe_agents(&mut self, cx: &mut Context<Self>) {
        if self.probing_agents {
            return;
        }
        self.probing_agents = true;
        let path = std::env::var_os("PATH").unwrap_or_default();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        cx.spawn(async move |this, cx| {
            let statuses = cx
                .background_spawn(async move { agents::probe_all(&path, home.as_deref()) })
                .await;
            let _ = this.update(cx, |workspace, cx| {
                workspace.agent_bar = statuses;
                workspace.probing_agents = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn on_agent_click(
        &mut self,
        status: AgentStatus,
        right_click: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match agents::click_action(status, right_click.is_some()) {
            Click::Menu => {
                self.agent_menu = right_click.map(|position| AgentMenu { status, position });
                cx.notify();
            }
            click => self.run_agent_action(status, click, window, cx),
        }
    }

    pub(super) fn run_agent_action(
        &mut self,
        status: AgentStatus,
        click: Click,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agent_menu = None;
        let agent = status.agent;
        let (pane, command) = match click {
            // The agent starts with the account new panes use.
            Click::Open => (self.spawn_pane(window, cx), agent.program),
            // Sign-in runs on the stored login, never with a token account.
            Click::SignIn => (
                self.spawn_pane_as(PaneAccount::default(), window, cx),
                agent.login,
            ),
            Click::InstallPage => {
                cx.open_url(agent.install_url);
                cx.notify();
                return;
            }
            Click::Accounts => {
                self.open_accounts(window, cx);
                return;
            }
            Click::Menu => return,
        };
        let Some(pane) = pane else {
            return;
        };
        pane.read(cx).run_command(command);
        self.tabs.open_tab(pane);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// The open right-click menu, drawn above everything else.
    pub(super) fn render_agent_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.agent_menu?;
        let status = menu.status;
        let items = agents::menu_items(status.installed)
            .into_iter()
            .map(|click| {
                let label = match click {
                    Click::Open => format!("Open {}", status.agent.name),
                    Click::SignIn => format!("Sign in ({})", status.agent.login),
                    Click::InstallPage => "Install or repair (web page)".to_owned(),
                    Click::Accounts => "Accounts".to_owned(),
                    Click::Menu => String::new(),
                };
                menu_item(
                    ElementId::Name(format!("agent-menu-{click:?}").into()),
                    label,
                    cx,
                    move |this, window, cx| this.run_agent_action(status, click, window, cx),
                )
            })
            .collect();
        Some(context_menu(
            "agent-menu",
            menu.position,
            items,
            cx,
            |this| this.agent_menu = None,
        ))
    }

    /// The icon row; empty until the first probe finishes.
    pub(super) fn render_agent_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let icons: Vec<AnyElement> = self
            .agent_bar
            .iter()
            .copied()
            .map(|status| self.render_agent_icon(status, cx))
            .collect();
        if icons.is_empty() {
            return div().into_any_element();
        }
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .gap_2()
            .mx_2()
            .mb_2()
            .px_1()
            .children(icons)
            .into_any_element()
    }

    fn render_agent_icon(&self, status: AgentStatus, cx: &mut Context<Self>) -> AnyElement {
        let agent = status.agent;
        let dot = match status.sign_in {
            SignIn::SignedIn => Some(chrome::DONE),
            SignIn::SignedOut => Some(chrome::TEXT_FAINT),
            SignIn::Unknown => None,
        };
        let tooltip: SharedString = if status.installed && agent.accounts {
            let state = match status.sign_in {
                SignIn::SignedIn => "default login signed in",
                SignIn::SignedOut => "default login signed out",
                SignIn::Unknown => "default login state unknown",
            };
            format!(
                "{}: account {} ({state}). Click to pick an account and open.",
                agent.name,
                self.accounts.active_name()
            )
        } else if status.installed {
            let state = match status.sign_in {
                SignIn::SignedIn => "signed in. Click to open".to_owned(),
                SignIn::SignedOut => format!("signed out. Click to {}", agent.login_hint),
                SignIn::Unknown => "sign-in state unknown. Click to open".to_owned(),
            };
            format!("{}: {state}. Right-click for more.", agent.name)
        } else {
            format!(
                "{}: not installed. Click to open its install page.",
                agent.name
            )
        }
        .into();
        div()
            .id(ElementId::Name(agent.id.into()))
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(ICON_SIZE))
            .rounded_md()
            .border_1()
            .border_color(chrome::color(chrome::BORDER))
            .bg(chrome::color(chrome::TAB_ACTIVE_BG))
            .hover(|icon| icon.bg(chrome::color(chrome::HOVER_BG)))
            .when(!status.installed, |icon| icon.opacity(MISSING_OPACITY))
            .child(
                svg()
                    .path(agent.icon)
                    .size(px(ICON_GLYPH))
                    .text_color(chrome::color(agent.color)),
            )
            .when_some(dot, |icon, dot| {
                icon.child(
                    div()
                        .absolute()
                        .top(px(-2.0))
                        .right(px(-2.0))
                        .size(px(DOT_SIZE))
                        .rounded_full()
                        .border_1()
                        .border_color(chrome::color(chrome::SIDEBAR_BG))
                        .bg(chrome::color(dot)),
                )
            })
            .tooltip(move |_, cx| cx.new(|_| TextTooltip(tooltip.clone())).into())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.on_agent_click(status, None, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.on_agent_click(status, Some(event.position), window, cx);
                }),
            )
            .into_any_element()
    }
}

/// Plain text tooltip.
pub(super) struct TextTooltip(pub(super) SharedString);

impl Render for TextTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(260.0))
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(chrome::color(chrome::BORDER))
            .bg(chrome::color(chrome::TITLEBAR_BG))
            .text_size(px(12.0))
            .text_color(chrome::color(chrome::TEXT))
            .child(self.0.clone())
    }
}
