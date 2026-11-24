//! Root view: agent sidebar, title bar, and the active tab's panes. Tabs are
//! listed in the sidebar (one row per pane), not in the title bar.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agent_status::ipc::{self, HookServer, PaneEvent};
use agent_status::{
    AgentState, AgentStatus, HookKind, PANE_ENV, SOCKET_ENV, format_elapsed, project_name,
};
use futures::StreamExt as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, Context, Entity, EntityId, FocusHandle, Focusable, Hsla,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, div, px, rgb, svg,
};
use term_core::{Session, SessionConfig, TerminalSize};

use crate::accounts::{self, Accounts, DEFAULT_ACCOUNT, PaneAccount, TOKEN_ENV};
use crate::actions::{
    ClosePane, CloseTab, FindInTerminal, FocusNextPane, FocusPrevPane, FocusSearch, NewTab,
    NextTab, PrevTab, SplitRight, SwitchAccount, ToggleGitPanel, TogglePalette, ToggleSidebar,
    WORKSPACE_CONTEXT, ZoomIn, ZoomOut, ZoomReset,
};
use crate::decorations::{Rounding, WindowFrame};
use crate::sidebar::{self, Filter, Item};
use crate::tabs::Tabs;
use crate::terminal_view::{TerminalView, TerminalViewEvent};
use crate::text_field::{TextField, TextFieldEvent};

mod account_modal;
mod agent_bar;
mod command_palette;
mod git_panel;
mod menu;
mod panes;
mod rename;
mod restore;
mod session_menu;
mod settings_modal;

use account_modal::AccountModal;

const TITLEBAR_HEIGHT: f32 = 38.0;
/// Opacity of saved sessions that are not started yet.
const DORMANT_OPACITY: f32 = 0.55;
const SIDEBAR_WIDTH: f32 = 270.0;
/// How often pane directories and elapsed times refresh.
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// UI colors for the window chrome (not the terminal palette).
pub(crate) mod chrome {
    use gpui::{Hsla, rgb};

    pub fn color(hex: u32) -> Hsla {
        rgb(hex).into()
    }

    pub const SIDEBAR_BG: u32 = 0x000e_0e0e;
    pub const TITLEBAR_BG: u32 = 0x0011_1111;
    pub const TAB_ACTIVE_BG: u32 = 0x000a_0a0a;
    pub const HOVER_BG: u32 = 0x001c_1c1c;
    pub const SELECTED_BG: u32 = 0x0022_2222;
    pub const TEXT: u32 = 0x00e6_e6e6;
    pub const TEXT_MUTED: u32 = 0x0080_8080;
    pub const TEXT_FAINT: u32 = 0x0058_5858;
    pub const BORDER: u32 = 0x0024_2424;
    pub const CLOSE_HOVER_BG: u32 = 0x00c4_2b1c;
    pub const WORKING: u32 = 0x0058_a6ff;
    pub const WAITING: u32 = 0x00e3_b341;
    pub const DONE: u32 = 0x003f_b950;
    pub const ACCENT: u32 = 0x00d9_7757;
}

type Pane = Entity<TerminalView>;

/// Bookkeeping for one pane.
struct PaneMeta {
    /// Stable id shared with the shell via `MY_TERMINAL_PANE`.
    id: u64,
    /// Last known working directory of the shell.
    dir: Option<PathBuf>,
    /// Account the shell was started with; `None` for the default login.
    account: Option<SharedString>,
    /// Agent CLI found running in the pane at the last refresh.
    running: Option<&'static crate::agents::Agent>,
    /// Name the user gave the session; shown instead of the automatic title.
    name: Option<SharedString>,
    /// Place in its split when it was restored from a saved one.
    split_position: Option<usize>,
    _subscription: Subscription,
}

/// Owns every tab and pane of a window.
// The flags are independent UI facts, not one state machine.
#[expect(clippy::struct_excessive_bools)]
pub struct Workspace {
    tabs: Tabs<Pane>,
    panes: HashMap<EntityId, PaneMeta>,
    /// Claude Code session state per pane id.
    agents: HashMap<u64, AgentState>,
    /// Session titles from Claude Code transcripts, per pane id.
    titles: HashMap<u64, crate::transcript::TitleReader>,
    next_pane_id: u64,
    sidebar_visible: bool,
    filter: Filter,
    /// `None` when the socket could not be created: no agent statuses.
    hook_server: Option<HookServer>,
    focus: FocusHandle,
    frame: WindowFrame,
    /// Claude account that new panes start with.
    accounts: Accounts,
    /// Why the last pane could not start with the active account.
    account_error: Option<SharedString>,
    /// The app itself runs with `CLAUDE_CODE_OAUTH_TOKEN`, so `Default`
    /// panes inherit that token instead of the stored login.
    inherited_token: bool,
    /// Open accounts dialog.
    account_modal: Option<AccountModal>,
    /// Dialog hidden while the user gets a token in a tab; reopening the
    /// dialog restores it.
    parked_modal: Option<AccountModal>,
    /// Agent CLIs and their sign-in state, from the last probe.
    agent_bar: Vec<crate::agents::AgentStatus>,
    probing_agents: bool,
    /// Open right-click menu of the agent bar.
    agent_menu: Option<agent_bar::AgentMenu>,
    /// Right-click menu of a sidebar session.
    session_menu: Option<session_menu::SessionMenu>,
    /// Sidebar row whose title is being edited.
    renaming: Option<rename::Renaming>,
    /// Open settings dialog.
    settings_modal: Option<settings_modal::SettingsModal>,
    /// Open git panel.
    git_panel: Option<git_panel::GitPanel>,
    /// Open command palette.
    palette: Option<command_palette::CommandPalette>,
    /// Where sessions are saved; `None` without a home directory.
    saved_path: Option<PathBuf>,
    /// Last text written there, to skip writes when nothing changed.
    last_saved: Option<String>,
    /// Saved sessions not resumed yet.
    dormant: Vec<restore::Dormant>,
    /// Projects whose sidebar group is collapsed.
    collapsed: HashSet<SharedString>,
    /// Variables of a Claude Code session that started the app, hidden from
    /// shells (see `agents::parent_session_vars`).
    strip_env: Vec<String>,
    /// Sidebar search box; filters sessions by title, subtitle and project.
    search: Entity<TextField>,
    _search_subscription: Subscription,
    /// Whether the window has focus; notifications only go out when not.
    window_active: bool,
    _activation_subscription: Subscription,
    _tasks: Vec<Task<()>>,
}

impl Workspace {
    /// Creates a workspace with one tab running the login shell.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut tasks = vec![
            Self::spawn_refresh_timer(cx),
            Self::spawn_agent_probe_timer(cx),
        ];
        let socket = ipc::default_socket_path(std::process::id());
        if let Some(dir) = socket.parent() {
            ipc::remove_stale_sockets(dir);
        }
        let hook_server = match HookServer::bind(&socket) {
            Ok((server, events)) => {
                tasks.push(Self::spawn_hook_listener(events, cx));
                Some(server)
            }
            Err(err) => {
                eprintln!("my-terminal: agent status disabled, cannot open socket: {err}");
                None
            }
        };

        let search = cx.new(|cx| TextField::new("Search agents", false, cx));
        let search_subscription = cx.subscribe_in(&search, window, |this, _, event, window, cx| {
            this.on_search_event(*event, window, cx);
        });
        let activation_subscription = cx.observe_window_activation(window, |this, window, _| {
            this.window_active = window.is_window_active();
        });
        let mut workspace = Self {
            tabs: Tabs::default(),
            panes: HashMap::new(),
            agents: HashMap::new(),
            titles: HashMap::new(),
            next_pane_id: 1,
            sidebar_visible: true,
            filter: Filter::default(),
            hook_server,
            focus: cx.focus_handle(),
            frame: WindowFrame::default(),
            accounts: load_accounts(),
            account_error: None,
            inherited_token: std::env::var_os(TOKEN_ENV).is_some(),
            account_modal: None,
            parked_modal: None,
            // Empty until the first probe, so the bar never flashes "not installed".
            agent_bar: Vec::new(),
            probing_agents: false,
            agent_menu: None,
            session_menu: None,
            renaming: None,
            settings_modal: None,
            git_panel: None,
            palette: None,
            saved_path: crate::saved::default_path(),
            last_saved: None,
            dormant: Vec::new(),
            collapsed: HashSet::new(),
            strip_env: crate::agents::parent_session_vars(
                std::env::vars_os().map(|(key, _)| key.to_string_lossy().into_owned()),
            ),
            search,
            _search_subscription: search_subscription,
            window_active: window.is_window_active(),
            _activation_subscription: activation_subscription,
            _tasks: tasks,
        };
        workspace.load_saved();
        workspace.probe_agents(cx);
        workspace.new_tab(&NewTab, window, cx);
        workspace
    }

    fn spawn_hook_listener(
        events: std::sync::mpsc::Receiver<PaneEvent>,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn(async move |this, cx| {
            let mut events = crate::bridge::to_stream("hook-events", events);
            while let Some(event) = events.next().await {
                if this
                    .update(cx, |workspace, cx| workspace.on_hook_event(&event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    /// Re-checks agent sign-in states now and then, so a login done in a
    /// tab or another terminal shows up.
    fn spawn_agent_probe_timer(cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(agent_bar::AGENT_PROBE_INTERVAL)
                    .await;
                if this.update(cx, Workspace::probe_agents).is_err() {
                    break;
                }
            }
        })
    }

    fn spawn_refresh_timer(cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH_INTERVAL).await;
                if this.update(cx, Workspace::refresh).is_err() {
                    break;
                }
            }
        })
    }

    /// Re-reads pane directories; redraws when they or elapsed times change.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let mut changed = !self.agents.is_empty();
        for pane in self.tabs.all_panes() {
            let view = pane.read(cx);
            let dir = view.working_directory();
            let running = crate::agents::agent_in_process_tree(view.child_pid());
            if let Some(meta) = self.panes.get_mut(&pane.entity_id()) {
                if meta.dir != dir {
                    meta.dir = dir;
                    changed = true;
                }
                if meta.running.map(|a| a.id) != running.map(|a| a.id) {
                    // Some agents (Antigravity) never report the end of a
                    // session: when the agent seen in the pane exits, its
                    // status goes too. Panes where no agent was ever seen
                    // (tmux, sudo, deep trees) keep their hook status.
                    if meta.running.is_some() && running.is_none() {
                        self.agents.remove(&meta.id);
                    }
                    meta.running = running;
                    changed = true;
                }
            }
        }
        changed |= self.poll_titles();
        self.refresh_git(cx);
        if changed && self.sidebar_visible {
            cx.notify();
        }
        self.save_sessions(cx);
    }

    /// Follows the transcript of every Claude Code session; `true` when a
    /// title changed.
    fn poll_titles(&mut self) -> bool {
        self.titles.retain(|id, _| self.agents.contains_key(id));
        let mut changed = false;
        for (id, state) in &self.agents {
            let Some(path) = state.transcript.as_deref() else {
                continue;
            };
            if self
                .titles
                .get(id)
                .is_none_or(|reader| reader.path() != path)
            {
                match crate::transcript::TitleReader::new(path) {
                    Some(reader) => self.titles.insert(*id, reader),
                    None => continue,
                };
            }
            if let Some(reader) = self.titles.get_mut(id) {
                changed |= reader.poll();
            }
        }
        changed
    }

    fn on_hook_event(&mut self, event: &PaneEvent, cx: &mut Context<Self>) {
        // Ignore events from panes that are already closed.
        if !self.panes.values().any(|meta| meta.id == event.pane) {
            return;
        }
        let now = Instant::now();
        let previous = self.agents.get(&event.pane).map(|state| state.status);
        match self.agents.get_mut(&event.pane) {
            Some(state) => {
                if !state.apply(&event.event, now) {
                    self.agents.remove(&event.pane);
                }
            }
            None if event.event.kind == HookKind::SessionEnd => {}
            None => {
                self.agents
                    .insert(event.pane, AgentState::new(&event.event, now));
            }
        }
        self.notify_status(event.pane, previous, cx);
        cx.notify();
    }

    /// Sends a desktop notification when a session in the background starts
    /// waiting or finishes.
    fn notify_status(&self, pane: u64, previous: Option<AgentStatus>, cx: &Context<Self>) {
        let Some(state) = self.agents.get(&pane) else {
            return;
        };
        let Some(body) = crate::notify::message(previous, state.status, self.window_active) else {
            return;
        };
        let body = match state.status {
            AgentStatus::Waiting => state.activity.as_deref().unwrap_or(body),
            _ => body,
        };
        let title = self
            .pane_by_id(pane)
            .map_or_else(|| "Claude Code".into(), |view| self.pane_title(&view, cx));
        crate::notify::send(&title, body);
    }

    /// Starts a shell with the active account in the directory of the
    /// active pane.
    fn spawn_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Pane> {
        // Never start a pane on another login than the footer shows: an
        // unreadable token blocks the pane and the footer shows why.
        let account = match self.accounts.pane_account() {
            Ok(account) => account,
            Err(err) => {
                eprintln!(
                    "my-terminal: account {}: {err}",
                    self.accounts.active_name()
                );
                self.account_error = Some(err.to_string().into());
                cx.notify();
                return None;
            }
        };
        self.account_error = None;
        self.spawn_pane_as(account, window, cx)
    }

    /// Starts a shell with `account` in the directory of the active pane.
    fn spawn_pane_as(
        &mut self,
        account: PaneAccount,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Pane> {
        let working_directory = self
            .tabs
            .active_pane()
            .and_then(|pane| pane.read(cx).working_directory());
        self.spawn_pane_at(account, working_directory, window, cx)
    }

    /// Starts a shell with `account` in `working_directory`.
    fn spawn_pane_at(
        &mut self,
        account: PaneAccount,
        working_directory: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Pane> {
        let id = self.next_pane_id;
        self.next_pane_id += 1;

        let mut config = SessionConfig {
            working_directory: working_directory.clone(),
            env_remove: self.strip_env.clone(),
            ..SessionConfig::default()
        };
        if let Some(server) = &self.hook_server {
            config.env.insert(PANE_ENV.into(), id.to_string());
            config
                .env
                .insert(SOCKET_ENV.into(), server.path().display().to_string());
        }
        if let Some((key, token)) = account.env {
            config.env.insert(key, token);
        }

        let (session, events) = match Session::spawn(config, TerminalSize::default()) {
            Ok(spawned) => spawned,
            Err(err) => {
                eprintln!("my-terminal: cannot start shell: {err}");
                return None;
            }
        };
        let view = cx.new(|cx| TerminalView::new(session, events, cx));
        let subscription = cx.subscribe_in(&view, window, Self::on_terminal_event);
        self.panes.insert(
            view.entity_id(),
            PaneMeta {
                id,
                dir: working_directory,
                account: account.badge.map(SharedString::from),
                running: None,
                name: None,
                split_position: None,
                _subscription: subscription,
            },
        );
        Some(view)
    }

    fn on_terminal_event(
        &mut self,
        view: &Pane,
        event: &TerminalViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TerminalViewEvent::TitleChanged => cx.notify(),
            TerminalViewEvent::Exited => {
                self.remove_pane(view, window, cx);
                // A login tab may have just finished.
                self.probe_agents(cx);
            }
            TerminalViewEvent::Focused => {
                self.tabs.focus_pane(view);
                cx.notify();
            }
            TerminalViewEvent::OpenLink(link) => self.open_link(view, link, window, cx),
        }
    }

    /// Opens a Ctrl+clicked link: URLs in the browser; existing folders in
    /// the file manager; existing files in the editor (a new tab, at the
    /// printed line) or, without one, in the default app.
    fn open_link(
        &mut self,
        view: &Pane,
        link: &term_core::links::Link,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use term_core::links::Link;
        match link {
            Link::Url(url) => cx.open_url(url),
            Link::File { path, line, .. } => {
                let cwd = view.read(cx).working_directory();
                let home = std::env::var_os("HOME").map(PathBuf::from);
                let file = crate::open_link::resolve(path, cwd.as_deref(), home.as_deref());
                if !file.exists() {
                    return;
                }
                let path_var = std::env::var_os("PATH").unwrap_or_default();
                let editor = crate::open_link::editor(
                    std::env::var("VISUAL").ok().as_deref(),
                    std::env::var("EDITOR").ok().as_deref(),
                    crate::agents::find_program("nvim", &path_var).is_some(),
                );
                match crate::open_link::target(&file, file.is_dir(), editor.as_deref(), *line) {
                    crate::open_link::Target::Folder(url)
                    | crate::open_link::Target::DefaultApp(url) => cx.open_url(&url),
                    crate::open_link::Target::Editor(command) => {
                        if let Some(pane) = self.spawn_pane(window, cx) {
                            pane.read(cx).run_command(&command);
                            self.tabs.open_tab(pane);
                            self.focus_active(window, cx);
                            cx.notify();
                        }
                    }
                }
            }
        }
    }

    fn forget_pane(&mut self, pane: &Pane) {
        if let Some(meta) = self.panes.remove(&pane.entity_id()) {
            self.agents.remove(&meta.id);
            self.titles.remove(&meta.id);
        }
    }

    fn remove_pane(&mut self, view: &Pane, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(removed) = self.tabs.remove_pane(view) {
            self.forget_pane(&removed);
        }
        self.save_sessions(cx);
        self.after_close(window, cx);
    }

    /// Focuses the new active pane, or closes the window when nothing is left.
    fn after_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            window.remove_window();
            return;
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.tabs.active_pane() {
            window.focus(&pane.focus_handle(cx));
        }
    }

    /// The pane whose sidebar item has id `id`.
    fn pane_by_id(&self, id: u64) -> Option<Pane> {
        self.tabs
            .all_panes()
            .find(|pane| {
                self.panes
                    .get(&pane.entity_id())
                    .is_some_and(|m| m.id == id)
            })
            .cloned()
    }

    fn focus_pane_by_id(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.pane_by_id(id) {
            self.tabs.focus_pane(&pane);
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.spawn_pane(window, cx) {
            self.tabs.open_tab(pane);
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    fn split_right(&mut self, _: &SplitRight, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.spawn_pane(window, cx) {
            self.tabs.split_active(pane);
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    fn close_pane(&mut self, _: &ClosePane, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.tabs.active_pane().cloned() {
            self.remove_pane(&view, window, cx);
        }
    }

    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        // Dropping the panes drops their sessions, which ends the shells.
        for pane in self.tabs.close_active_tab() {
            self.forget_pane(&pane);
        }
        self.save_sessions(cx);
        self.after_close(window, cx);
    }

    fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.next_tab();
        self.focus_active(window, cx);
        cx.notify();
    }

    fn prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.prev_tab();
        self.focus_active(window, cx);
        cx.notify();
    }

    fn focus_next_pane(&mut self, _: &FocusNextPane, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.cycle_pane(true);
        self.focus_active(window, cx);
        cx.notify();
    }

    fn focus_prev_pane(&mut self, _: &FocusPrevPane, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.cycle_pane(false);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Ctrl+= / Ctrl+- / Ctrl+0: font size of every pane, for this run.
    fn zoom(steps: Option<i32>, window: &mut Window, cx: &mut Context<Self>) {
        gpui::BorrowAppContext::update_global::<crate::theme::TerminalSettings, _>(
            cx,
            |settings, _| {
                settings.font_size = match steps {
                    Some(steps) => crate::config::zoom(settings.font_size, steps),
                    None => settings.base_font_size,
                };
            },
        );
        window.refresh();
    }

    /// Ctrl+Shift+F: search the output of the focused pane.
    fn find_in_terminal(
        &mut self,
        _: &FindInTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.tabs.active_pane().cloned() {
            pane.update(cx, |view, cx| view.open_search(window, cx));
        }
    }

    /// Ctrl+Shift+K: show the sidebar and type into its search box.
    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = true;
        self.search.read(cx).focus(window);
        cx.notify();
    }

    fn on_search_event(
        &mut self,
        event: TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Changed => cx.notify(),
            // Escape clears the search and goes back to the terminal.
            TextFieldEvent::Cancel => {
                self.search.update(cx, |field, cx| field.set_text("", cx));
                self.focus_active(window, cx);
            }
            // Enter jumps to the first session found.
            TextFieldEvent::Submit | TextFieldEvent::SubmitBack => {
                let query = self.search.read(cx).text().to_owned();
                let first = sidebar::search(self.sidebar_items(cx), &query)
                    .into_iter()
                    .find(|item| self.filter.matches(item.status))
                    .map(|item| item.pane);
                if let Some(pane) = first {
                    self.focus_pane_by_id(pane, window, cx);
                }
            }
            TextFieldEvent::Next => {}
        }
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        cx.notify();
    }

    /// Next account for new panes. Rescans first so token files added while
    /// the app runs show up. Running panes keep their account.
    fn switch_account(&mut self, _: &SwitchAccount, _: &mut Window, cx: &mut Context<Self>) {
        for skipped in self.accounts.rescan() {
            eprintln!("my-terminal: skipped account: {skipped}");
        }
        self.accounts.switch_next();
        self.account_error = None;
        cx.notify();
    }

    /// Label of a pane: agent topic, program title, or "shell".
    fn pane_title(&self, pane: &Pane, cx: &Context<Self>) -> SharedString {
        if let Some(name) = self
            .panes
            .get(&pane.entity_id())
            .and_then(|meta| meta.name.clone())
        {
            return name;
        }
        let agent = self
            .panes
            .get(&pane.entity_id())
            .and_then(|meta| self.agents.get(&meta.id));
        if let Some(agent) = agent {
            // Claude's own session title first, then the first prompt.
            let title = self
                .panes
                .get(&pane.entity_id())
                .and_then(|meta| self.titles.get(&meta.id))
                .and_then(|reader| reader.title())
                .map(str::to_owned);
            let running = self
                .panes
                .get(&pane.entity_id())
                .and_then(|meta| meta.running)
                .map_or("Claude Code", |agent| agent.name);
            return title
                .map(SharedString::from)
                .or_else(|| agent.topic.clone().map(Into::into))
                .unwrap_or_else(|| running.into());
        }
        if let Some(running) = self
            .panes
            .get(&pane.entity_id())
            .and_then(|meta| meta.running)
        {
            return running.name.into();
        }
        pane.read(cx)
            .title()
            .cloned()
            .unwrap_or_else(|| "shell".into())
    }

    fn sidebar_items(&self, cx: &Context<Self>) -> Vec<Item> {
        let now = Instant::now();
        let active = self.tabs.active_pane().map(Entity::entity_id);
        let in_view: Vec<EntityId> = self
            .tabs
            .active_tab()
            .map(|tab| tab.panes().iter().map(Entity::entity_id).collect())
            .unwrap_or_default();
        self.tabs
            .all_panes()
            .filter_map(|pane| {
                let meta = self.panes.get(&pane.entity_id())?;
                let agent = self.agents.get(&meta.id);
                let dir = agent
                    .and_then(|a| a.cwd.clone())
                    .or_else(|| meta.dir.clone());
                let project = dir.as_deref().map_or_else(|| "~".to_owned(), project_name);
                let subtitle = match agent {
                    Some(agent) => agent_subtitle(agent),
                    None => dir.as_deref().map_or_else(String::new, shorten_home),
                };
                Some(Item {
                    pane: meta.id,
                    project: project.into(),
                    title: self.pane_title(pane, cx),
                    subtitle: subtitle.into(),
                    status: agent.map(|a| a.status),
                    elapsed: agent.map(|a| format_elapsed(now - a.since).into()),
                    active: active == Some(pane.entity_id()),
                    in_view: in_view.contains(&pane.entity_id()),
                    dormant: false,
                    account: meta.account.clone(),
                    icon: meta
                        .running
                        .or_else(|| {
                            // Hooks only come from Claude Code.
                            agent.and_then(|_| crate::agents::AGENTS.first())
                        })
                        .map(|running| (running.icon, running.color)),
                })
            })
            .chain(self.dormant.iter().map(|dormant| {
                let session = &dormant.session;
                let agent = session
                    .agent
                    .as_deref()
                    .and_then(|id| crate::agents::AGENTS.iter().find(|a| a.id == id));
                Item {
                    pane: dormant.id,
                    project: session
                        .dir
                        .as_deref()
                        .map_or_else(|| "~".to_owned(), project_name)
                        .into(),
                    title: session
                        .name
                        .clone()
                        .unwrap_or_else(|| session.title.clone())
                        .into(),
                    subtitle: "Saved · click to resume".into(),
                    status: None,
                    elapsed: None,
                    active: false,
                    in_view: false,
                    dormant: true,
                    account: session.account.clone().map(SharedString::from),
                    icon: agent.map(|agent| (agent.icon, agent.color)),
                }
            }))
            .collect()
    }

    fn render_sidebar(&self, rounding: Rounding, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).text().to_owned();
        let searching = !query.trim().is_empty();
        let items = sidebar::search(self.sidebar_items(cx), &query);
        let counts = sidebar::counts(&items);
        let groups = sidebar::group(items, self.filter);

        let new_session = div()
            .id("new-session")
            .flex()
            .items_center()
            .justify_between()
            .mx_2()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(color(chrome::HOVER_BG))
            .hover(|s| s.bg(color(chrome::SELECTED_BG)))
            .on_click(cx.listener(|this, _, window, cx| this.new_tab(&NewTab, window, cx)))
            .child("New session")
            .child(
                div()
                    .text_color(color(chrome::TEXT_FAINT))
                    .text_size(px(11.0))
                    .child("Ctrl+Shift+T"),
            );

        let filters = self.render_filters(counts, cx);
        let search = div().flex_none().mx_2().child(self.search.clone()).child(
            div()
                .px_1()
                .pt_1()
                .text_size(px(10.5))
                .text_color(color(chrome::TEXT_FAINT))
                .child("Ctrl+Shift+K to search · Enter opens · Esc clears"),
        );

        let list = div()
            .id("sidebar-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb_2()
            .children(
                groups
                    .into_iter()
                    .enumerate()
                    .map(|(index, (project, items))| {
                        self.render_group(index, project, items, searching, cx)
                    }),
            );

        // Top strip lines up with the title bar and also drags the window.
        let top = div()
            .id("sidebar-top")
            .flex_none()
            .h(px(TITLEBAR_HEIGHT))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move());

        rounding
            .left_only()
            .apply(div())
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .gap_2()
            .bg(color(chrome::SIDEBAR_BG))
            .border_r_1()
            .border_color(color(chrome::BORDER))
            .text_size(px(13.0))
            .child(top)
            .child(new_session)
            .child(search)
            .child(filters)
            .child(list)
            .child(self.render_agent_bar(cx))
            .child(self.render_account_error())
    }

    /// Why the last pane could not start with the active account, under the
    /// agent bar; accounts themselves live in the Claude icon's dialog.
    fn render_account_error(&self) -> impl IntoElement {
        div().flex().flex_col().flex_none().when_some(
            self.account_error.clone(),
            |footer, error| {
                footer.child(
                    div()
                        .mx_2()
                        .mb_2()
                        .px_1()
                        .text_size(px(11.0))
                        .text_color(color(chrome::WAITING))
                        .child(error),
                )
            },
        )
    }

    fn render_filters(&self, counts: sidebar::Counts, cx: &mut Context<Self>) -> AnyElement {
        let filter_button = |filter: Filter, label: String, cx: &mut Context<Self>| {
            let selected = self.filter == filter;
            div()
                .id(SharedString::from(format!("filter-{label}")))
                .flex_1()
                .py_1()
                .rounded_md()
                .text_center()
                .text_color(color(if selected {
                    chrome::TEXT
                } else {
                    chrome::TEXT_MUTED
                }))
                .when(selected, |b| b.bg(color(chrome::SELECTED_BG)))
                .when(!selected, |b| b.hover(|s| s.bg(color(chrome::HOVER_BG))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.filter = filter;
                    cx.notify();
                }))
                .child(label)
        };

        div()
            .flex()
            .gap_1()
            .mx_2()
            .p_1()
            .rounded_md()
            .bg(color(chrome::TITLEBAR_BG))
            .text_size(px(12.0))
            .child(filter_button(
                Filter::All,
                format!("All {}", counts.all),
                cx,
            ))
            .child(filter_button(
                Filter::Waiting,
                format!("Waiting {}", counts.waiting),
                cx,
            ))
            .child(filter_button(
                Filter::Working,
                format!("Working {}", counts.working),
                cx,
            ))
            .into_any_element()
    }

    /// One project group: a header that collapses it, then its rows.
    fn render_group(
        &self,
        index: usize,
        project: SharedString,
        items: Vec<Item>,
        searching: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        // A search shows every match, collapsed or not.
        let collapsed = !searching && self.collapsed.contains(&project);
        let count = items.len();
        let toggle = project.clone();
        let header = div()
            .id(("group", index))
            .flex()
            .gap_1()
            .mx_2()
            .px_2()
            .pt_3()
            .pb_1()
            .rounded_md()
            .text_size(px(11.0))
            .text_color(color(chrome::TEXT_FAINT))
            .hover(|s| s.text_color(color(chrome::TEXT_MUTED)))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_group(&toggle, cx)))
            .child(if collapsed { "▸" } else { "▾" })
            .child(project)
            .when(collapsed, |header| header.child(format!("({count})")));
        div()
            .flex()
            .flex_col()
            .child(header)
            .when(!collapsed, |group| {
                group.child(
                    // A small gap keeps highlighted rows of one split apart.
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(items.into_iter().map(|item| self.render_item(item, cx))),
                )
            })
    }

    fn render_item(&self, item: Item, cx: &mut Context<Self>) -> AnyElement {
        let pane = item.pane;
        let menu_title = item.title.to_string();
        let title: AnyElement = self
            .rename_field(pane)
            .unwrap_or_else(|| item.title.clone().into_any_element());
        let (marker, marker_color) = match item.status {
            Some(AgentStatus::Working) => ("◌", chrome::WORKING),
            Some(AgentStatus::Waiting) => ("●", chrome::WAITING),
            Some(AgentStatus::Done) => ("●", chrome::DONE),
            Some(AgentStatus::Idle) => ("✻", chrome::ACCENT),
            None => (">_", chrome::TEXT_FAINT),
        };

        div()
            .id(("agent", usize::try_from(pane).unwrap_or(usize::MAX)))
            .flex()
            .gap_2()
            .mx_2()
            .px_2()
            .py_1p5()
            .rounded_md()
            // Every pane of the visible split is highlighted.
            .when(item.in_view, |row| row.bg(color(chrome::SELECTED_BG)))
            .when(!item.in_view, |row| {
                row.hover(|s| s.bg(color(chrome::HOVER_BG)))
            })
            .when(item.dormant, |row| row.opacity(DORMANT_OPACITY))
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.is_dormant(pane) {
                    this.resume_dormant(pane, window, cx);
                } else {
                    this.focus_pane_by_id(pane, window, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    this.open_session_menu(pane, menu_title.clone(), event.position, cx);
                }),
            )
            .child(item_marker(
                item.icon,
                item.status.is_some(),
                marker,
                marker_color,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(title),
                            )
                            .when_some(item.account, |row, account| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .px_1()
                                        .rounded_sm()
                                        .bg(color(chrome::HOVER_BG))
                                        .text_size(px(10.5))
                                        .text_color(color(chrome::TEXT_MUTED))
                                        .child(account),
                                )
                            }),
                    )
                    .child(
                        div()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(px(11.5))
                            .text_color(color(match item.status {
                                Some(AgentStatus::Done) => chrome::DONE,
                                Some(AgentStatus::Waiting) => chrome::WAITING,
                                _ => chrome::TEXT_MUTED,
                            }))
                            .child(item.subtitle),
                    ),
            )
            .when_some(item.elapsed, |row, elapsed| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(px(11.0))
                        .text_color(color(chrome::TEXT_FAINT))
                        .child(elapsed),
                )
            })
            .into_any_element()
    }

    fn render_titlebar(
        &self,
        rounding: Rounding,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let toggle = div()
            .id("toggle-sidebar")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(TITLEBAR_HEIGHT))
            .h_full()
            .text_color(color(chrome::TEXT_MUTED))
            .hover(|style| style.bg(color(chrome::HOVER_BG)))
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_sidebar(&ToggleSidebar, window, cx);
            }))
            .child("☰");

        // Empty title bar space drags the window; double click maximizes.
        let drag_area = div().id("drag-area").flex_1().h_full().on_mouse_down(
            MouseButton::Left,
            |event, window, _| {
                if event.click_count == 2 {
                    window.zoom_window();
                } else {
                    window.start_window_move();
                }
            },
        );

        rounding
            .apply(div())
            .flex()
            .flex_none()
            .items_center()
            .h(px(TITLEBAR_HEIGHT))
            .bg(color(chrome::TITLEBAR_BG))
            .border_b_1()
            .border_color(color(chrome::BORDER))
            .text_size(px(13.0))
            .child(toggle)
            .child(drag_area)
            .child(self.render_pane_buttons(cx))
            .child(window_controls(window))
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rounding = Rounding::from_decorations(window.window_decorations());
        // The title bar owns the top-left corner only when the sidebar is hidden.
        let titlebar_rounding = if self.sidebar_visible {
            rounding.right_only()
        } else {
            rounding
        };

        let content = rounding
            .apply(div())
            .id("workspace")
            .track_focus(&self.focus)
            .key_context(WORKSPACE_CONTEXT)
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::prev_tab))
            .on_action(cx.listener(Self::split_right))
            .on_action(cx.listener(Self::close_pane))
            .on_action(cx.listener(Self::focus_next_pane))
            .on_action(cx.listener(Self::focus_prev_pane))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::switch_account))
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::find_in_terminal))
            .on_action(cx.listener(|_, _: &ZoomIn, window, cx| {
                Self::zoom(Some(1), window, cx);
            }))
            .on_action(cx.listener(|_, _: &ZoomOut, window, cx| {
                Self::zoom(Some(-1), window, cx);
            }))
            .on_action(cx.listener(|this, _: &TogglePalette, window, cx| {
                this.toggle_palette(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleGitPanel, _, cx| {
                this.toggle_git_panel(cx);
            }))
            .on_action(cx.listener(|_, _: &ZoomReset, window, cx| {
                Self::zoom(None, window, cx);
            }))
            .flex()
            .size_full()
            .bg(rgb(chrome::TAB_ACTIVE_BG))
            .text_color(rgb(chrome::TEXT))
            .when(self.sidebar_visible, |root| {
                root.child(self.render_sidebar(rounding, cx))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.render_titlebar(titlebar_rounding, window, cx))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_h_0()
                            .child(self.render_panes(cx))
                            .when_some(self.render_git_panel(cx), ParentElement::child),
                    ),
            )
            .relative()
            .when_some(self.render_agent_menu(cx), ParentElement::child)
            .when_some(self.render_session_menu(cx), ParentElement::child)
            .when_some(self.render_accounts(cx), ParentElement::child)
            .when_some(self.render_settings(cx), ParentElement::child)
            .when_some(self.render_palette(cx), ParentElement::child);
        self.frame.render(window, content)
    }
}

/// Left column of a session row: the agent's logo (with a Claude Code status
/// dot) or a text marker for plain shells.
fn item_marker(
    icon: Option<(&'static str, u32)>,
    has_status: bool,
    marker: &'static str,
    marker_color: u32,
) -> gpui::Div {
    match icon {
        Some((icon, agent_color)) => div()
            .flex_none()
            .relative()
            .w(px(18.0))
            .pt(px(2.0))
            .child(
                svg()
                    .path(icon)
                    .size(px(14.0))
                    .text_color(color(agent_color)),
            )
            // Claude Code status as a dot on the logo.
            .when(has_status, |logo| {
                logo.child(
                    div()
                        .absolute()
                        .top(px(0.0))
                        .left(px(-3.0))
                        .size(px(6.0))
                        .rounded_full()
                        .bg(color(marker_color)),
                )
            }),
        None => div()
            .flex_none()
            .w(px(18.0))
            .text_color(color(marker_color))
            .text_size(px(12.0))
            .child(marker),
    }
}

/// Loads the token accounts and reports files that were skipped.
fn load_accounts() -> Accounts {
    let (accounts, skipped) = Accounts::load(accounts::default_dir());
    for err in skipped {
        eprintln!("my-terminal: skipped account: {err}");
    }
    if std::env::var_os(TOKEN_ENV).is_some() {
        eprintln!(
            "my-terminal: {TOKEN_ENV} is set in the app environment; \
             {DEFAULT_ACCOUNT} panes inherit it instead of the stored login"
        );
    }
    accounts
}

/// Second line of an agent row: what it is doing now.
fn agent_subtitle(agent: &AgentState) -> String {
    match agent.status {
        AgentStatus::Working => agent
            .activity
            .clone()
            .unwrap_or_else(|| "Working".to_owned()),
        AgentStatus::Waiting => agent
            .activity
            .clone()
            .unwrap_or_else(|| "Waiting for you".to_owned()),
        status => status.label().to_owned(),
    }
}

/// Replaces the home directory prefix with `~`.
fn shorten_home(dir: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if dir.starts_with(&home) => {
            let rest = dir.strip_prefix(&home).unwrap_or(dir);
            if rest.as_os_str().is_empty() {
                "~".to_owned()
            } else {
                format!("~/{}", rest.display())
            }
        }
        _ => dir.display().to_string(),
    }
}

/// Minimize, maximize and close buttons (client-side decorations).
fn window_controls(window: &Window) -> impl IntoElement {
    let button = |id: &'static str, label: &'static str| {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .w(px(44.0))
            .h_full()
            .text_color(color(chrome::TEXT_MUTED))
            .child(label)
    };
    let maximize_label = if window.is_maximized() { "❐" } else { "□" };

    div()
        .flex()
        .flex_none()
        .h_full()
        .child(
            button("minimize", "–")
                .hover(|style| style.bg(color(chrome::HOVER_BG)))
                .on_click(|_, window, _| window.minimize_window()),
        )
        .child(
            button("maximize", maximize_label)
                .hover(|style| style.bg(color(chrome::HOVER_BG)))
                .on_click(|_, window, _| window.zoom_window()),
        )
        .child(
            button("close", "×")
                .hover(|style| {
                    style
                        .bg(color(chrome::CLOSE_HOVER_BG))
                        .text_color(color(chrome::TEXT))
                })
                .on_click(|_, window, _| window.remove_window()),
        )
}

fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_subtitle_prefers_activity_while_busy() {
        let event = agent_status::HookEvent::parse(
            r#"{"hook_event_name":"PreToolUse","session_id":"s","tool_name":"Edit",
                "tool_input":{"file_path":"src/a.rs"}}"#,
        )
        .unwrap();
        let mut state = AgentState::new(&event, Instant::now());
        assert_eq!(agent_subtitle(&state), "Edit src/a.rs");

        let stop = agent_status::HookEvent::parse(r#"{"hook_event_name":"Stop","session_id":"s"}"#)
            .unwrap();
        state.apply(&stop, Instant::now());
        assert_eq!(agent_subtitle(&state), "Done");
    }

    #[test]
    fn shorten_home_replaces_prefix() {
        let home = PathBuf::from(std::env::var_os("HOME").expect("HOME set in tests"));
        assert_eq!(shorten_home(&home), "~");
        assert_eq!(shorten_home(&home.join("dev/x")), "~/dev/x");
        assert_eq!(shorten_home(Path::new("/etc")), "/etc");
    }
}
