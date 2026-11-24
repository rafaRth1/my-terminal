//! Git panel on the right of the panes (Ctrl+Shift+G): branch and changed
//! files of the focused session's repository, and the diff of a file
//! against `HEAD`. Read-only; refreshed every couple of seconds while open.

use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, Context, ElementId, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use super::{Workspace, chrome};
use crate::git::{self, FileChange, LineKind, RepoStatus};
use crate::theme::TerminalSettings;

const PANEL_WIDTH: f32 = 380.0;
const ADDED: u32 = 0x003f_b950;
const REMOVED: u32 = 0x00f4_4747;
const RENAMED: u32 = 0x0058_a6ff;
const CONFLICT: u32 = 0x00d2_9922;

/// The open panel.
#[derive(Default)]
pub(super) struct GitPanel {
    root: Option<PathBuf>,
    status: Option<RepoStatus>,
    error: Option<SharedString>,
    /// File whose diff is shown instead of the list.
    selected: Option<FileChange>,
    diff: Vec<(LineKind, String)>,
    /// A refresh runs in the background.
    loading: bool,
}

/// What one background refresh found.
struct Snapshot {
    root: Option<PathBuf>,
    status: Result<RepoStatus, String>,
    diff: Option<Result<Vec<(LineKind, String)>, String>>,
}

impl Workspace {
    pub(super) fn toggle_git_panel(&mut self, cx: &mut Context<Self>) {
        if self.git_panel.take().is_none() {
            self.git_panel = Some(GitPanel::default());
            self.refresh_git(cx);
        }
        cx.notify();
    }

    /// Reads the repository of the focused pane off the UI thread; one
    /// refresh at a time.
    pub(super) fn refresh_git(&mut self, cx: &mut Context<Self>) {
        let dir = self
            .tabs
            .active_pane()
            .and_then(|pane| pane.read(cx).working_directory());
        let Some(panel) = self.git_panel.as_mut() else {
            return;
        };
        if panel.loading {
            return;
        }
        panel.loading = true;
        let selected = panel.selected.clone();
        cx.spawn(async move |this, cx| {
            let snapshot = cx
                .background_spawn(async move {
                    let root = dir.as_deref().and_then(git::repo_root);
                    let status = root
                        .as_deref()
                        .map_or_else(|| Err("Not a git repository".to_owned()), git::status);
                    let diff = root
                        .as_deref()
                        .zip(selected.as_ref())
                        .map(|(root, file)| git::diff(root, file));
                    Snapshot { root, status, diff }
                })
                .await;
            let _ = this.update(cx, |workspace, cx| {
                if let Some(panel) = workspace.git_panel.as_mut() {
                    panel.apply(snapshot);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn select_git_file(&mut self, file: Option<FileChange>, cx: &mut Context<Self>) {
        if let Some(panel) = self.git_panel.as_mut() {
            panel.selected = file;
            panel.diff.clear();
        }
        self.refresh_git(cx);
        cx.notify();
    }

    pub(super) fn render_git_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panel = self.git_panel.as_ref()?;
        let family = cx.global::<TerminalSettings>().font_family.clone();
        let body: AnyElement = match (&panel.selected, &panel.status) {
            (Some(file), _) => Self::render_diff(panel, file, family, cx),
            (None, Some(status)) if !status.files.is_empty() => Self::render_files(status, cx),
            (None, Some(_)) => faint("No changes").into_any_element(),
            (None, None) => {
                faint(panel.error.clone().unwrap_or_else(|| "Loading…".into())).into_any_element()
            }
        };
        Some(
            div()
                .id("git-panel")
                .flex()
                .flex_col()
                .flex_none()
                .w(px(PANEL_WIDTH))
                .h_full()
                .border_l_1()
                .border_color(chrome::color(chrome::BORDER))
                .bg(chrome::color(chrome::SIDEBAR_BG))
                .text_size(px(12.5))
                .text_color(chrome::color(chrome::TEXT))
                .child(Self::render_git_header(panel, cx))
                .child(
                    div()
                        .id("git-panel-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(body),
                )
                .into_any_element(),
        )
    }

    fn render_git_header(panel: &GitPanel, cx: &mut Context<Self>) -> AnyElement {
        let repo = panel
            .root
            .as_deref()
            .and_then(|root| root.file_name())
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let branch = panel.status.as_ref().map(|status| {
            let ahead = (status.ahead > 0).then(|| format!("  ↑{}", status.ahead));
            let behind = (status.behind > 0).then(|| format!("  ↓{}", status.behind));
            format!(
                "{}{}{}",
                status.branch,
                ahead.unwrap_or_default(),
                behind.unwrap_or_default()
            )
        });
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(chrome::color(chrome::BORDER))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(if repo.is_empty() {
                                "Git".to_owned()
                            } else {
                                repo
                            }),
                    )
                    .when_some(branch, |col, branch| {
                        col.child(
                            div()
                                .text_size(px(11.5))
                                .text_color(chrome::color(chrome::TEXT_MUTED))
                                .child(branch),
                        )
                    }),
            )
            .child(
                icon_button("git-refresh", "⟳").on_click(cx.listener(|this, _, _, cx| {
                    this.refresh_git(cx);
                })),
            )
            .child(
                icon_button("git-close", "✕").on_click(cx.listener(|this, _, _, cx| {
                    this.git_panel = None;
                    cx.notify();
                })),
            )
            .into_any_element()
    }

    fn render_files(status: &RepoStatus, cx: &mut Context<Self>) -> AnyElement {
        let rows = status.files.iter().enumerate().map(|(index, file)| {
            let (dir, name) = match file.path.rsplit_once('/') {
                Some((dir, name)) => (format!("{dir}/"), name.to_owned()),
                None => (String::new(), file.path.clone()),
            };
            let picked = file.clone();
            div()
                .id(ElementId::NamedInteger("git-file".into(), index as u64))
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .hover(|row| row.bg(chrome::color(chrome::HOVER_BG)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_git_file(Some(picked.clone()), cx);
                }))
                .child(
                    div()
                        .flex_none()
                        .w(px(12.0))
                        .text_color(chrome::color(status_color(file.status)))
                        .child(file.status.to_string()),
                )
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(div().flex_none().child(name))
                        .child(
                            div()
                                .ml_1()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_color(chrome::color(chrome::TEXT_FAINT))
                                .child(dir),
                        ),
                )
                .when_some(file.added, |row, added| {
                    row.child(
                        div()
                            .flex_none()
                            .text_color(chrome::color(ADDED))
                            .child(format!("+{added}")),
                    )
                })
                .when_some(file.removed, |row, removed| {
                    row.child(
                        div()
                            .flex_none()
                            .text_color(chrome::color(REMOVED))
                            .child(format!("−{removed}")),
                    )
                })
        });
        div()
            .flex()
            .flex_col()
            .py_1()
            .children(rows)
            .into_any_element()
    }

    fn render_diff(
        panel: &GitPanel,
        file: &FileChange,
        family: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let lines = panel.diff.iter().map(|(kind, text)| {
            let (fg, bg) = match kind {
                LineKind::Added => (ADDED, Some(0x0012_2a18)),
                LineKind::Removed => (REMOVED, Some(0x002e_1414)),
                LineKind::Hunk => (RENAMED, None),
                LineKind::Header => (chrome::TEXT_FAINT, None),
                LineKind::Context => (chrome::TEXT_MUTED, None),
            };
            div()
                .px_3()
                .whitespace_nowrap()
                .text_color(chrome::color(fg))
                .when_some(bg, |line, bg| line.bg(chrome::color(bg)))
                .child(if text.is_empty() {
                    " ".to_owned()
                } else {
                    text.clone()
                })
        });
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("git-back")
                    .flex()
                    .gap_2()
                    .px_3()
                    .py_1p5()
                    .border_b_1()
                    .border_color(chrome::color(chrome::BORDER))
                    .hover(|row| row.bg(chrome::color(chrome::HOVER_BG)))
                    .on_click(cx.listener(|this, _, _, cx| this.select_git_file(None, cx)))
                    .child("← Files")
                    .child(
                        div()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_color(chrome::color(chrome::TEXT_MUTED))
                            .child(file.path.clone()),
                    ),
            )
            .when(panel.diff.is_empty(), |view| {
                view.child(faint(
                    panel
                        .error
                        .clone()
                        .unwrap_or_else(|| "No differences".into()),
                ))
            })
            .child(
                div()
                    .py_1()
                    .font_family(family)
                    .text_size(px(12.0))
                    .children(lines),
            )
            .into_any_element()
    }
}

impl GitPanel {
    fn apply(&mut self, snapshot: Snapshot) {
        self.loading = false;
        // A new repository (the focus moved) starts at its file list.
        if snapshot.root != self.root {
            self.selected = None;
            self.diff.clear();
        }
        self.root = snapshot.root;
        match snapshot.status {
            Ok(status) => {
                // The selected file may have been committed or reverted.
                if let Some(selected) = &self.selected
                    && !status.files.iter().any(|f| f.path == selected.path)
                {
                    self.selected = None;
                    self.diff.clear();
                }
                self.status = Some(status);
                self.error = None;
            }
            Err(error) => {
                self.status = None;
                self.selected = None;
                self.diff.clear();
                self.error = Some(error.into());
            }
        }
        if let (Some(_), Some(diff)) = (&self.selected, snapshot.diff) {
            match diff {
                Ok(lines) => self.diff = lines,
                Err(error) => self.error = Some(error.into()),
            }
        }
    }
}

fn status_color(status: char) -> u32 {
    match status {
        'A' | '?' => ADDED,
        'D' => REMOVED,
        'R' => RENAMED,
        'U' => CONFLICT,
        _ => chrome::WAITING,
    }
}

fn icon_button(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .px_1p5()
        .rounded_sm()
        .text_color(chrome::color(chrome::TEXT_MUTED))
        .hover(|b| b.bg(chrome::color(chrome::HOVER_BG)))
        .child(label)
}

fn faint(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .px_3()
        .py_2()
        .text_color(chrome::color(chrome::TEXT_FAINT))
        .child(text.into())
}
