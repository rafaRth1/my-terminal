//! Tabs and side-by-side panes, independent of the UI.
//!
//! Every tab holds at least one pane. Removing the last pane of a tab removes
//! the tab. Indexes stay valid after every operation. Each pane has a share
//! of the tab's width; the shares of a tab add up to 1.

/// Narrowest share a divider drag can leave a pane.
pub const MIN_PANE_SHARE: f32 = 0.1;

/// One tab: panes laid out left to right.
#[derive(Debug)]
pub struct Tab<P> {
    /// Stable id, saved with the tab's sessions to restore the split.
    id: u64,
    panes: Vec<P>,
    /// Share of the tab's width of each pane, in pane order.
    widths: Vec<f32>,
    active_pane: usize,
}

impl<P> Tab<P> {
    fn new(pane: P, id: u64) -> Self {
        Self {
            id,
            panes: vec![pane],
            widths: vec![1.0],
            active_pane: 0,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn panes(&self) -> &[P] {
        &self.panes
    }

    /// Share of the tab's width of each pane, in pane order.
    pub fn widths(&self) -> &[f32] {
        &self.widths
    }

    pub fn active_pane(&self) -> &P {
        &self.panes[self.active_pane]
    }
}

/// Ordered tabs with one active tab.
#[derive(Debug)]
pub struct Tabs<P> {
    list: Vec<Tab<P>>,
    active: usize,
    /// Id the next new tab gets.
    next_id: u64,
}

impl<P> Default for Tabs<P> {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            active: 0,
            next_id: 0,
        }
    }
}

impl<P: PartialEq> Tabs<P> {
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn tabs(&self) -> &[Tab<P>] {
        &self.list
    }

    #[cfg(test)]
    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active_tab(&self) -> Option<&Tab<P>> {
        self.list.get(self.active)
    }

    /// Every pane of every tab, in display order.
    pub fn all_panes(&self) -> impl Iterator<Item = &P> {
        self.list.iter().flat_map(|tab| tab.panes.iter())
    }

    /// The pane that should hold keyboard focus.
    pub fn active_pane(&self) -> Option<&P> {
        self.active_tab().map(Tab::active_pane)
    }

    /// Adds a tab after the active one and activates it.
    pub fn open_tab(&mut self, pane: P) {
        let id = self.take_id();
        self.open_tab_with_id(pane, id);
    }

    /// Like [`Self::open_tab`], with tab id `id` (a restored split); later
    /// tabs get higher ids.
    pub fn open_tab_with_id(&mut self, pane: P, id: u64) {
        self.reserve_ids_through(id);
        let index = if self.list.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.list.insert(index, Tab::new(pane, id));
        self.active = index;
    }

    /// New tabs get ids above `id` (saved ids stay free for their splits).
    pub fn reserve_ids_through(&mut self, id: u64) {
        self.next_id = self.next_id.max(id.saturating_add(1));
    }

    /// The next free id (skipping ids in use, wrapping around).
    fn take_id(&mut self) -> u64 {
        let mut id = self.next_id;
        while self.list.iter().any(|tab| tab.id == id) {
            id = id.wrapping_add(1);
        }
        self.next_id = id.wrapping_add(1);
        id
    }

    /// Id of the tab holding `pane`.
    #[cfg(test)]
    pub fn tab_id(&self, pane: &P) -> Option<u64> {
        self.find(pane).map(|(tab, _)| self.list[tab].id)
    }

    /// Puts `pane` at `position` (clamped) in the tab with id `id`, with
    /// `width` of it (the other panes shrink in proportion), and focuses it.
    /// Gives the pane back when there is no such tab.
    pub fn insert_into(&mut self, id: u64, pane: P, position: usize, width: f32) -> Result<(), P> {
        let Some(index) = self.list.iter().position(|tab| tab.id == id) else {
            return Err(pane);
        };
        let tab = &mut self.list[index];
        let others = tab.panes.len();
        #[allow(clippy::cast_precision_loss, reason = "a tab holds a handful of panes")]
        let max = (1.0 - MIN_PANE_SHARE * others as f32).max(MIN_PANE_SHARE);
        let width = if width.is_finite() { width } else { 0.5 };
        let width = width.clamp(MIN_PANE_SHARE, max);
        for share in &mut tab.widths {
            *share *= 1.0 - width;
        }
        let position = position.min(others);
        tab.panes.insert(position, pane);
        tab.widths.insert(position, width);
        tab.active_pane = position;
        self.active = index;
        Ok(())
    }

    /// Adds a pane right of the active pane, which gives it half its width,
    /// and activates it. Opens a tab when there is none.
    pub fn split_active(&mut self, pane: P) {
        let Some(tab) = self.list.get_mut(self.active) else {
            self.open_tab(pane);
            return;
        };
        let half = tab.widths[tab.active_pane] / 2.0;
        tab.widths[tab.active_pane] = half;
        tab.active_pane += 1;
        tab.panes.insert(tab.active_pane, pane);
        tab.widths.insert(tab.active_pane, half);
    }

    /// Moves divider `divider` (between panes `divider` and `divider + 1` of
    /// the active tab) to `at`, a fraction of the tab's width. Only those two
    /// panes change, and neither gets narrower than [`MIN_PANE_SHARE`].
    pub fn move_divider(&mut self, divider: usize, at: f32) {
        let Some(tab) = self.list.get_mut(self.active) else {
            return;
        };
        let right = divider + 1;
        if right >= tab.widths.len() || !at.is_finite() {
            return;
        }
        let start: f32 = tab.widths[..divider].iter().sum();
        let pair = tab.widths[divider] + tab.widths[right];
        if pair < 2.0 * MIN_PANE_SHARE {
            return;
        }
        let boundary = at.clamp(start + MIN_PANE_SHARE, start + pair - MIN_PANE_SHARE);
        tab.widths[divider] = boundary - start;
        tab.widths[right] = pair - tab.widths[divider];
    }

    /// Moves `pane` from another tab into the active tab, right of the active
    /// pane (as [`Self::split_active`] does), and focuses it. Its old tab
    /// closes when it was its last pane. Returns `false`, changing nothing,
    /// when the pane is unknown or already in the active tab.
    pub fn move_into_active_split(&mut self, pane: &P) -> bool {
        match self.find(pane) {
            Some((tab_index, _)) if tab_index != self.active => {}
            _ => return false,
        }
        let Some(moved) = self.remove_pane(pane) else {
            return false;
        };
        self.split_active(moved);
        true
    }

    /// Moves `pane` out of its split into a new tab right after it, without
    /// switching tabs: the rest of the split stays where it was. Returns
    /// `false`, changing nothing, when the pane is unknown or alone.
    pub fn move_out_of_split(&mut self, pane: &P) -> bool {
        let Some((tab_index, _)) = self.find(pane) else {
            return false;
        };
        if self.list[tab_index].panes.len() < 2 {
            return false;
        }
        // The tab keeps other panes, so `remove_pane` leaves `active` alone.
        let Some(moved) = self.remove_pane(pane) else {
            return false;
        };
        let id = self.take_id();
        self.list.insert(tab_index + 1, Tab::new(moved, id));
        if self.active > tab_index {
            self.active += 1;
        }
        true
    }

    /// Activates tab `index`; out-of-range indexes are ignored.
    #[cfg(test)]
    pub fn activate(&mut self, index: usize) {
        if index < self.list.len() {
            self.active = index;
        }
    }

    /// Activates the next tab, wrapping around.
    pub fn next_tab(&mut self) {
        if !self.list.is_empty() {
            self.active = (self.active + 1) % self.list.len();
        }
    }

    /// Activates the previous tab, wrapping around.
    pub fn prev_tab(&mut self) {
        if !self.list.is_empty() {
            self.active = (self.active + self.list.len() - 1) % self.list.len();
        }
    }

    /// Moves focus to the next (`forward`) or previous pane of the active tab.
    pub fn cycle_pane(&mut self, forward: bool) {
        if let Some(tab) = self.list.get_mut(self.active) {
            let len = tab.panes.len();
            tab.active_pane = if forward {
                (tab.active_pane + 1) % len
            } else {
                (tab.active_pane + len - 1) % len
            };
        }
    }

    /// Makes `pane` the active pane and its tab the active tab.
    pub fn focus_pane(&mut self, pane: &P) {
        if let Some((tab_index, pane_index)) = self.find(pane) {
            self.active = tab_index;
            self.list[tab_index].active_pane = pane_index;
        }
    }

    /// Removes `pane`. Drops its tab when it was the last pane.
    /// Returns the removed pane, or `None` when it was not found.
    pub fn remove_pane(&mut self, pane: &P) -> Option<P> {
        let (tab_index, pane_index) = self.find(pane)?;
        let tab = &mut self.list[tab_index];
        let removed = tab.panes.remove(pane_index);
        let width = tab.widths.remove(pane_index);
        // The left neighbor takes the width; the first pane's goes right.
        if let Some(neighbor) = tab.widths.get_mut(pane_index.saturating_sub(1)) {
            *neighbor += width;
        }

        if tab.panes.is_empty() {
            self.list.remove(tab_index);
            if self.active > tab_index || self.active >= self.list.len() {
                self.active = self.active.saturating_sub(1);
            }
        } else if tab.active_pane > pane_index || tab.active_pane >= tab.panes.len() {
            tab.active_pane = tab.active_pane.saturating_sub(1);
        }
        Some(removed)
    }

    /// Removes the active tab with all its panes and returns them.
    pub fn close_active_tab(&mut self) -> Vec<P> {
        if self.list.is_empty() {
            return Vec::new();
        }
        let tab = self.list.remove(self.active);
        if self.active >= self.list.len() {
            self.active = self.active.saturating_sub(1);
        }
        tab.panes
    }

    fn find(&self, pane: &P) -> Option<(usize, usize)> {
        self.list.iter().enumerate().find_map(|(tab_index, tab)| {
            tab.panes
                .iter()
                .position(|p| p == pane)
                .map(|pane_index| (tab_index, pane_index))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(tabs: &Tabs<u32>) -> Vec<Vec<u32>> {
        tabs.tabs().iter().map(|t| t.panes().to_vec()).collect()
    }

    #[test]
    fn open_tab_inserts_after_active_and_activates() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.open_tab(2);
        tabs.activate(0);
        tabs.open_tab(3);
        assert_eq!(layout(&tabs), vec![vec![1], vec![3], vec![2]]);
        assert_eq!(tabs.active_pane(), Some(&3));
    }

    #[test]
    fn split_adds_pane_right_of_active() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.split_active(2);
        tabs.cycle_pane(false);
        tabs.split_active(3);
        assert_eq!(layout(&tabs), vec![vec![1, 3, 2]]);
        assert_eq!(tabs.active_pane(), Some(&3));
    }

    #[test]
    fn split_without_tabs_opens_one() {
        let mut tabs = Tabs::default();
        tabs.split_active(7);
        assert_eq!(layout(&tabs), vec![vec![7]]);
    }

    #[test]
    fn tab_navigation_wraps() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.open_tab(2);
        tabs.next_tab();
        assert_eq!(tabs.active_index(), 0);
        tabs.prev_tab();
        assert_eq!(tabs.active_index(), 1);
        tabs.activate(9);
        assert_eq!(tabs.active_index(), 1);
    }

    #[test]
    fn removing_last_pane_removes_tab_and_keeps_active_valid() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.open_tab(2);
        tabs.open_tab(3);
        assert_eq!(tabs.remove_pane(&3), Some(3));
        assert_eq!(layout(&tabs), vec![vec![1], vec![2]]);
        assert_eq!(tabs.active_pane(), Some(&2));

        tabs.activate(1);
        tabs.remove_pane(&1);
        assert_eq!(tabs.active_pane(), Some(&2));
    }

    #[test]
    fn removing_pane_keeps_focus_on_same_pane_when_possible() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.split_active(2);
        tabs.split_active(3);
        tabs.remove_pane(&1);
        assert_eq!(layout(&tabs), vec![vec![2, 3]]);
        assert_eq!(tabs.active_pane(), Some(&3));
        tabs.remove_pane(&3);
        assert_eq!(tabs.active_pane(), Some(&2));
    }

    #[test]
    fn removing_everything_leaves_empty() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.split_active(2);
        assert_eq!(tabs.close_active_tab(), vec![1, 2]);
        assert!(tabs.is_empty());
        assert_eq!(tabs.active_pane(), None);
        assert_eq!(tabs.remove_pane(&1), None);
    }

    #[test]
    fn all_panes_lists_in_display_order() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.split_active(2);
        tabs.open_tab(3);
        assert_eq!(tabs.all_panes().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn focus_pane_activates_its_tab() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.split_active(2);
        tabs.open_tab(3);
        tabs.focus_pane(&1);
        assert_eq!(tabs.active_index(), 0);
        assert_eq!(tabs.active_pane(), Some(&1));
    }

    fn widths(tabs: &Tabs<u32>) -> Vec<f32> {
        tabs.active_tab().unwrap().widths().to_vec()
    }

    fn assert_widths(tabs: &Tabs<u32>, expected: &[f32]) {
        let got = widths(tabs);
        assert_eq!(got.len(), expected.len(), "{got:?}");
        for (g, e) in got.iter().zip(expected) {
            assert!((g - e).abs() < 1e-4, "{got:?} != {expected:?}");
        }
    }

    #[test]
    fn split_halves_the_active_pane() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        assert_widths(&tabs, &[1.0]);
        tabs.split_active(2);
        assert_widths(&tabs, &[0.5, 0.5]);
        tabs.focus_pane(&1);
        tabs.split_active(3);
        assert_eq!(layout(&tabs), vec![vec![1, 3, 2]]);
        assert_widths(&tabs, &[0.25, 0.25, 0.5]);
    }

    #[test]
    fn removed_width_goes_to_a_neighbor() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.split_active(2);
        tabs.focus_pane(&1);
        tabs.split_active(3);
        // [1: 0.25, 3: 0.25, 2: 0.5]; the left neighbor takes it.
        tabs.remove_pane(&3);
        assert_widths(&tabs, &[0.5, 0.5]);
        // The first pane has no left neighbor: the right one takes it.
        tabs.remove_pane(&1);
        assert_widths(&tabs, &[1.0]);
    }

    #[test]
    fn move_divider_sets_the_boundary_within_its_two_panes() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.split_active(2);
        tabs.move_divider(0, 0.3);
        assert_widths(&tabs, &[0.3, 0.7]);
        // Clamped so no pane gets narrower than MIN_PANE_SHARE.
        tabs.move_divider(0, 0.0);
        assert_widths(&tabs, &[MIN_PANE_SHARE, 1.0 - MIN_PANE_SHARE]);
        tabs.move_divider(0, 2.0);
        assert_widths(&tabs, &[1.0 - MIN_PANE_SHARE, MIN_PANE_SHARE]);

        tabs.move_divider(0, 0.5);
        tabs.split_active(3);
        // [1: 0.5, 2: 0.25, 3: 0.25]; divider 1 sits at 0.75.
        tabs.move_divider(1, 0.6);
        assert_widths(&tabs, &[0.5, 0.1, 0.4]);
        // Only the two panes next to the divider change.
        tabs.move_divider(1, 0.2);
        assert_widths(&tabs, &[0.5, MIN_PANE_SHARE, 0.4]);
    }

    #[test]
    fn move_divider_ignores_bad_input() {
        let mut tabs = Tabs::default();
        tabs.move_divider(0, 0.5);
        tabs.open_tab(1);
        tabs.move_divider(0, 0.5);
        assert_widths(&tabs, &[1.0]);
        tabs.split_active(2);
        tabs.move_divider(1, 0.2);
        tabs.move_divider(0, f32::NAN);
        assert_widths(&tabs, &[0.5, 0.5]);
    }

    #[test]
    fn move_into_active_split_takes_the_pane_from_its_tab() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.open_tab(2);
        tabs.open_tab(3);
        tabs.activate(1);
        // A later tab: it disappears, the active tab stays the same.
        assert!(tabs.move_into_active_split(&3));
        assert_eq!(layout(&tabs), vec![vec![1], vec![2, 3]]);
        assert_eq!(tabs.active_index(), 1);
        assert_eq!(tabs.active_pane(), Some(&3));
        assert_widths(&tabs, &[0.5, 0.5]);
        // An earlier tab: the active index shifts with it.
        assert!(tabs.move_into_active_split(&1));
        assert_eq!(layout(&tabs), vec![vec![2, 3, 1]]);
        assert_eq!(tabs.active_index(), 0);
        assert_eq!(tabs.active_pane(), Some(&1));
        assert_widths(&tabs, &[0.5, 0.25, 0.25]);
    }

    #[test]
    fn move_into_active_split_leaves_the_rest_of_a_split() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.split_active(2);
        tabs.open_tab(3);
        assert!(tabs.move_into_active_split(&2));
        assert_eq!(layout(&tabs), vec![vec![1], vec![3, 2]]);
        assert_widths(&tabs, &[0.5, 0.5]);
        tabs.activate(0);
        assert_widths(&tabs, &[1.0]);
    }

    #[test]
    fn move_into_active_split_ignores_visible_and_unknown_panes() {
        let mut tabs = Tabs::default();
        assert!(!tabs.move_into_active_split(&1));
        tabs.open_tab(1);
        tabs.split_active(2);
        tabs.open_tab(3);
        tabs.activate(0);
        assert!(!tabs.move_into_active_split(&1));
        assert!(!tabs.move_into_active_split(&9));
        assert_eq!(layout(&tabs), vec![vec![1, 2], vec![3]]);
        assert_eq!(tabs.active_pane(), Some(&2));
    }

    #[test]
    fn move_out_of_split_opens_the_pane_in_its_own_tab() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.split_active(2);
        tabs.split_active(3);
        tabs.open_tab(4);
        tabs.activate(0);
        tabs.focus_pane(&2);
        // [1: 0.5, 2: 0.25, 3: 0.25]; 2 leaves, 1 takes its width.
        assert!(tabs.move_out_of_split(&2));
        assert_eq!(layout(&tabs), vec![vec![1, 3], vec![2], vec![4]]);
        // The split stays on screen, focused on the neighbor (as closing).
        assert_eq!(tabs.active_index(), 0);
        assert_eq!(tabs.active_pane(), Some(&3));
        assert_widths(&tabs, &[0.75, 0.25]);
        tabs.activate(1);
        assert_widths(&tabs, &[1.0]);
    }

    #[test]
    fn move_out_of_split_from_a_tab_in_the_background() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.split_active(2);
        tabs.open_tab(3);
        assert!(tabs.move_out_of_split(&1));
        assert_eq!(layout(&tabs), vec![vec![2], vec![1], vec![3]]);
        assert_eq!(tabs.active_pane(), Some(&3));
    }

    #[test]
    fn move_out_of_split_ignores_single_and_unknown_panes() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.open_tab(2);
        assert!(!tabs.move_out_of_split(&1));
        assert!(!tabs.move_out_of_split(&9));
        assert_eq!(layout(&tabs), vec![vec![1], vec![2]]);
        assert_eq!(tabs.active_pane(), Some(&2));
    }

    #[test]
    fn tabs_get_unique_ids() {
        let mut tabs = Tabs::default();
        tabs.open_tab(1);
        tabs.open_tab(2);
        let (a, b) = (tabs.tab_id(&1).unwrap(), tabs.tab_id(&2).unwrap());
        assert_ne!(a, b);
        tabs.split_active(3);
        assert_eq!(tabs.tab_id(&3), Some(b));
        assert!(tabs.move_out_of_split(&3));
        let c = tabs.tab_id(&3).unwrap();
        assert!(c != a && c != b);
        assert_eq!(tabs.tab_id(&9), None);
    }

    #[test]
    fn open_tab_with_id_keeps_it_and_new_ids_skip_it() {
        let mut tabs = Tabs::default();
        tabs.open_tab_with_id(1, 7);
        assert_eq!(tabs.tab_id(&1), Some(7));
        tabs.open_tab(2);
        assert!(tabs.tab_id(&2).unwrap() > 7);
        tabs.reserve_ids_through(40);
        tabs.open_tab(3);
        assert!(tabs.tab_id(&3).unwrap() > 40);
    }

    #[test]
    fn insert_into_puts_the_pane_in_place_with_its_width() {
        let mut tabs = Tabs::default();
        tabs.open_tab_with_id(1, 3);
        tabs.open_tab(5);
        assert_eq!(tabs.insert_into(3, 2, 1, 0.3), Ok(()));
        assert_eq!(layout(&tabs), vec![vec![1, 2], vec![5]]);
        // The tab comes to the front, focused on the new pane.
        assert_eq!(tabs.active_pane(), Some(&2));
        assert_widths(&tabs, &[0.7, 0.3]);
        assert_eq!(tabs.insert_into(3, 4, 0, 0.5), Ok(()));
        assert_eq!(layout(&tabs), vec![vec![4, 1, 2], vec![5]]);
        assert_widths(&tabs, &[0.5, 0.35, 0.15]);
        // A position past the end appends; widths stay within limits.
        assert_eq!(tabs.insert_into(3, 6, 99, 0.99), Ok(()));
        assert_eq!(layout(&tabs)[0], vec![4, 1, 2, 6]);
        let widths = widths(&tabs);
        assert!(
            (widths.iter().sum::<f32>() - 1.0).abs() < 1e-4,
            "{widths:?}"
        );
        assert!(widths[3] <= 1.0 - 3.0 * MIN_PANE_SHARE + 1e-4, "{widths:?}");
        // Unknown tab: the pane comes back.
        assert_eq!(tabs.insert_into(77, 8, 0, 0.5), Err(8));
    }

    #[test]
    fn huge_reserved_ids_never_overflow() {
        let mut tabs = Tabs::default();
        tabs.open_tab_with_id(1, u64::MAX);
        tabs.open_tab(2);
        assert_ne!(tabs.tab_id(&2), Some(u64::MAX));
    }
}
