//! Sidebar model: one item per pane, grouped by project, filtered by status.

use agent_status::AgentStatus;
use gpui::SharedString;

/// Which items the sidebar shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Waiting,
    Working,
}

impl Filter {
    pub fn matches(self, status: Option<AgentStatus>) -> bool {
        match self {
            Self::All => true,
            Self::Waiting => status == Some(AgentStatus::Waiting),
            Self::Working => status == Some(AgentStatus::Working),
        }
    }
}

/// One row in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub pane: u64,
    pub project: SharedString,
    pub title: SharedString,
    pub subtitle: SharedString,
    /// `None` for plain shells without a Claude session.
    pub status: Option<AgentStatus>,
    /// Time since the status changed, already formatted (`3m`).
    pub elapsed: Option<SharedString>,
    pub active: bool,
    /// Shown in the active tab's split (the focused pane included).
    pub in_view: bool,
    /// Saved from an earlier run and not started yet.
    pub dormant: bool,
    /// Account the pane was started with; `None` for the default login.
    pub account: Option<SharedString>,
    /// Logo of the agent running in the pane (asset path, `0xRRGGBB`).
    pub icon: Option<(&'static str, u32)>,
}

/// Item counts for the filter buttons.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub all: usize,
    pub waiting: usize,
    pub working: usize,
}

pub fn counts(items: &[Item]) -> Counts {
    Counts {
        all: items.len(),
        waiting: items
            .iter()
            .filter(|i| i.status == Some(AgentStatus::Waiting))
            .count(),
        working: items
            .iter()
            .filter(|i| i.status == Some(AgentStatus::Working))
            .count(),
    }
}

/// Whether every word of `query` appears in the item's title, subtitle or
/// project, ignoring case. An empty query matches everything.
pub fn matches_search(item: &Item, query: &str) -> bool {
    let haystack = format!("{} {} {}", item.title, item.subtitle, item.project).to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| haystack.contains(word))
}

/// The items matching `query`, in their original order.
pub fn search(items: Vec<Item>, query: &str) -> Vec<Item> {
    items
        .into_iter()
        .filter(|item| matches_search(item, query))
        .collect()
}

/// Groups the items matching `filter` by project, keeping first-seen order.
pub fn group(items: Vec<Item>, filter: Filter) -> Vec<(SharedString, Vec<Item>)> {
    let mut groups: Vec<(SharedString, Vec<Item>)> = Vec::new();
    for item in items.into_iter().filter(|i| filter.matches(i.status)) {
        match groups
            .iter_mut()
            .find(|(project, _)| *project == item.project)
        {
            Some((_, members)) => members.push(item),
            None => groups.push((item.project.clone(), vec![item])),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(pane: u64, project: &str, status: Option<AgentStatus>) -> Item {
        Item {
            pane,
            project: project.to_owned().into(),
            title: "t".into(),
            subtitle: "s".into(),
            status,
            elapsed: None,
            active: false,
            in_view: false,
            dormant: false,
            account: None,
            icon: None,
        }
    }

    fn sample() -> Vec<Item> {
        vec![
            item(1, "midterm", Some(AgentStatus::Working)),
            item(2, "midulive", Some(AgentStatus::Waiting)),
            item(3, "midterm", None),
            item(4, "midulive", Some(AgentStatus::Working)),
        ]
    }

    fn panes(groups: &[(SharedString, Vec<Item>)]) -> Vec<(String, Vec<u64>)> {
        groups
            .iter()
            .map(|(p, items)| (p.to_string(), items.iter().map(|i| i.pane).collect()))
            .collect()
    }

    #[test]
    fn groups_by_project_in_first_seen_order() {
        assert_eq!(
            panes(&group(sample(), Filter::All)),
            vec![
                ("midterm".into(), vec![1, 3]),
                ("midulive".into(), vec![2, 4])
            ]
        );
    }

    #[test]
    fn filters_by_status() {
        assert_eq!(
            panes(&group(sample(), Filter::Working)),
            vec![("midterm".into(), vec![1]), ("midulive".into(), vec![4])]
        );
        assert_eq!(
            panes(&group(sample(), Filter::Waiting)),
            vec![("midulive".into(), vec![2])]
        );
    }

    #[test]
    fn search_matches_title_subtitle_or_project_ignoring_case() {
        let mut first = item(1, "midterm", None);
        first.title = "Claude Code".into();
        first.subtitle = "Writing crates/platform/src/vision.rs".into();
        let second = item(2, "midulive", None);

        assert!(matches_search(&first, ""));
        assert!(matches_search(&first, "  "));
        assert!(matches_search(&first, "claude"));
        assert!(matches_search(&first, "VISION"));
        assert!(matches_search(&first, "midterm"));
        assert!(
            matches_search(&first, "claude vision"),
            "every word must match"
        );
        assert!(!matches_search(&first, "claude midulive"));
        assert!(!matches_search(&second, "claude"));
    }

    #[test]
    fn search_filters_before_grouping() {
        let items = sample();
        let found: Vec<u64> = search(items, "midulive").iter().map(|i| i.pane).collect();
        assert_eq!(found, [2, 4]);
        assert_eq!(search(sample(), "").len(), 4);
    }

    #[test]
    fn counts_statuses() {
        assert_eq!(
            counts(&sample()),
            Counts {
                all: 4,
                waiting: 1,
                working: 2
            }
        );
    }
}
