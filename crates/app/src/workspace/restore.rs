//! Agent sessions saved between runs. Open sessions are written whenever
//! they change; at start they come back as dormant sidebar rows that start
//! nothing until clicked. Plain shells are not saved: there is nothing in
//! them to bring back.

use std::path::PathBuf;

use gpui::{Context, SharedString, Window};

use super::{PaneAccount, Workspace};
use crate::saved::{self, SavedSession, SavedState};

/// A saved session that has not been resumed yet.
pub(super) struct Dormant {
    /// Sidebar item id, from the same counter as pane ids.
    pub(super) id: u64,
    pub(super) session: SavedSession,
}

impl Workspace {
    /// Reads the saved sessions: the dormant rows and the collapsed groups.
    pub(super) fn load_saved(&mut self) {
        let Some(path) = &self.saved_path else {
            return;
        };
        let (state, error) = saved::load(path);
        if let Some(error) = error {
            eprintln!("my-terminal: saved sessions: {error}");
        }
        self.collapsed = state
            .collapsed
            .into_iter()
            .map(SharedString::from)
            .collect();
        // Saved split ids stay free so restored splits keep them.
        if let Some(max) = state.sessions.iter().filter_map(|s| s.split).max() {
            self.tabs.reserve_ids_through(max);
        }
        for session in state.sessions {
            let id = self.next_pane_id;
            self.next_pane_id += 1;
            self.dormant.push(Dormant { id, session });
        }
    }

    /// What would be written now: open agent sessions in sidebar order (with
    /// their split, place and width when they share a tab), then the dormant
    /// ones, then the collapsed groups.
    fn saved_state(&self, cx: &Context<Self>) -> SavedState {
        let placed = self.tabs.tabs().iter().flat_map(|tab| {
            let shared = tab.panes().len() > 1;
            tab.panes().iter().zip(tab.widths()).enumerate().map(
                move |(position, (pane, &width))| {
                    let layout = shared.then_some((tab.id(), position, width));
                    (pane, layout)
                },
            )
        });
        let open = placed.filter_map(|(pane, layout)| {
            let meta = self.panes.get(&pane.entity_id())?;
            let hooks = self.agents.get(&meta.id);
            // Hooks only come from Claude Code.
            let agent = meta
                .running
                .map(|agent| agent.id)
                .or_else(|| hooks.map(|_| "claude"))?;
            Some(SavedSession {
                title: self.pane_title(pane, cx).to_string(),
                dir: hooks
                    .and_then(|state| state.cwd.clone())
                    .or_else(|| meta.dir.clone()),
                agent: Some(agent.to_owned()),
                claude_session: hooks.map(|state| state.session_id.clone()),
                account: meta.account.as_ref().map(ToString::to_string),
                name: meta.name.as_ref().map(ToString::to_string),
                split: layout.map(|(id, _, _)| id),
                position: layout.map(|(_, position, _)| position),
                width: layout.map(|(_, _, width)| width),
            })
        });
        let mut sessions: Vec<SavedSession> = open
            .chain(self.dormant.iter().map(|dormant| dormant.session.clone()))
            .collect();
        sessions.truncate(saved::MAX_SESSIONS);
        let mut collapsed: Vec<String> = self.collapsed.iter().map(ToString::to_string).collect();
        collapsed.sort();
        SavedState {
            sessions,
            collapsed,
        }
    }

    /// Writes the saved sessions when they changed since the last write.
    pub(super) fn save_sessions(&mut self, cx: &Context<Self>) {
        let Some(path) = self.saved_path.clone() else {
            return;
        };
        let state = self.saved_state(cx);
        let Ok(text) = serde_json::to_string(&state) else {
            return;
        };
        if self.last_saved.as_ref() == Some(&text) {
            return;
        }
        match saved::save(&path, &state) {
            Ok(()) => self.last_saved = Some(text),
            Err(err) => eprintln!(
                "my-terminal: cannot save sessions to {}: {err}",
                path.display()
            ),
        }
    }

    /// Starts dormant session `id` in a new tab, in its folder and with its
    /// account; Claude Code resumes its conversation.
    pub(super) fn resume_dormant(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.dormant.iter().position(|dormant| dormant.id == id) else {
            return;
        };
        let session = &self.dormant[index].session;
        let account = match session.account.as_deref() {
            None => Ok(PaneAccount::default()),
            Some(name) => self.accounts.pane_account_named(name),
        };
        let account = match account {
            Ok(account) => account,
            Err(err) => {
                // Keep the row so it can be resumed once the account is back.
                self.account_error = Some(format!("cannot resume {}: {err}", session.title).into());
                cx.notify();
                return;
            }
        };
        let dir: Option<PathBuf> = session.dir.clone().filter(|dir| dir.is_dir());
        let command = saved::resume_command(session);
        let Some(pane) = self.spawn_pane_at(account, dir, window, cx) else {
            return;
        };
        let saved = self.dormant.remove(index).session;
        if let Some(meta) = self.panes.get_mut(&pane.entity_id()) {
            meta.name = saved.name.clone().map(SharedString::from);
            meta.split_position = saved.position;
        }
        self.account_error = None;
        if let Some(command) = command {
            pane.read(cx).run_command(&command);
        }
        self.place_resumed(pane, &saved);
        self.focus_active(window, cx);
        self.save_sessions(cx);
        cx.notify();
    }

    /// Puts a resumed pane back in its split when another session of that
    /// split is open (at its place, with its width); otherwise opens the
    /// split's tab with just this pane.
    fn place_resumed(&mut self, pane: super::Pane, saved: &SavedSession) {
        let Some(split) = saved.split else {
            self.tabs.open_tab(pane);
            return;
        };
        let mine = saved.position.unwrap_or(usize::MAX);
        // Panes of that tab that were to its left (unknown places count as left).
        let before = self
            .tabs
            .tabs()
            .iter()
            .find(|tab| tab.id() == split)
            .map_or(0, |tab| {
                tab.panes()
                    .iter()
                    .filter(|view| {
                        self.panes
                            .get(&view.entity_id())
                            .and_then(|meta| meta.split_position)
                            .is_none_or(|theirs| theirs < mine)
                    })
                    .count()
            });
        let width = saved.width.unwrap_or(0.5);
        if let Err(pane) = self.tabs.insert_into(split, pane, before, width) {
            self.tabs.open_tab_with_id(pane, split);
        }
    }

    /// Drops dormant session `id` from the sidebar and the file.
    pub(super) fn forget_dormant(&mut self, id: u64, cx: &mut Context<Self>) {
        self.dormant.retain(|dormant| dormant.id != id);
        self.save_sessions(cx);
        cx.notify();
    }

    pub(super) fn is_dormant(&self, id: u64) -> bool {
        self.dormant.iter().any(|dormant| dormant.id == id)
    }

    /// Collapses or expands the sidebar group of `project`.
    pub(super) fn toggle_group(&mut self, project: &SharedString, cx: &mut Context<Self>) {
        if !self.collapsed.remove(project) {
            self.collapsed.insert(project.clone());
        }
        self.save_sessions(cx);
        cx.notify();
    }
}
