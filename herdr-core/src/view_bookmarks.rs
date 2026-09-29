//! Each Agent tab's bookmark: the View every View area showed in front while
//! that tab was the Workspace's active Agent tab (PRD tab-view-bookmark).
//!
//! A bookmark points at a display, never at a document, so a preview display
//! another tab retargeted shows the new document when it is restored. The map
//! is Hide's own presentation state: it lives on the Workspace's entry in
//! `workspace-views.json` beside `agent_layout`, keyed by the stable Herdr tab
//! id, and is never sent to the shell.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Herdr tab id -> View area id -> display id. Bounded by the Workspace's
/// tabs times its View areas (at most six), because a tab that left Herdr and
/// an area that collapsed are pruned.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Bookmarks {
    tabs: BTreeMap<String, BTreeMap<String, String>>,
}

impl Bookmarks {
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// The tab's bookmark: each area it remembers with the display in front.
    pub fn of(&self, tab: &str) -> Option<&BTreeMap<String, String>> {
        self.tabs.get(tab)
    }

    /// Remembers `display` as what `area` showed for `tab`. Returns whether
    /// that changed the bookmark.
    pub fn record(&mut self, tab: &str, area: &str, display: &str) -> bool {
        let areas = self.tabs.entry(tab.to_owned()).or_default();
        match areas.get_mut(area) {
            Some(held) if held == display => false,
            Some(held) => {
                display.clone_into(held);
                true
            }
            None => {
                areas.insert(area.to_owned(), display.to_owned());
                true
            }
        }
    }

    /// Forgets the tabs `keep` refuses. Returns whether anything went.
    pub fn retain_tabs(&mut self, keep: impl Fn(&str) -> bool) -> bool {
        let before = self.tabs.len();
        self.tabs.retain(|tab, _| keep(tab));
        self.tabs.len() != before
    }

    /// Forgets the areas `keep` refuses, and a tab left with none. Returns
    /// whether anything went.
    pub fn retain_areas(&mut self, keep: impl Fn(&str) -> bool) -> bool {
        let mut changed = false;
        for areas in self.tabs.values_mut() {
            let before = areas.len();
            areas.retain(|area, _| keep(area));
            changed |= areas.len() != before;
        }
        let before = self.tabs.len();
        self.tabs.retain(|_, areas| !areas.is_empty());
        changed || self.tabs.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_changes_the_bookmark_only_when_the_front_differs() {
        let mut bookmarks = Bookmarks::default();
        assert!(bookmarks.record("w:t1", "a1", "d1"));
        assert!(!bookmarks.record("w:t1", "a1", "d1"));
        assert!(bookmarks.record("w:t1", "a1", "d2"));
        assert!(bookmarks.record("w:t1", "a2", "d3"));
        assert_eq!(bookmarks.of("w:t1").unwrap().len(), 2);
        assert!(bookmarks.of("w:t2").is_none());
    }

    #[test]
    fn pruning_a_tab_or_an_area_leaves_the_rest_and_drops_an_emptied_tab() {
        let mut bookmarks = Bookmarks::default();
        bookmarks.record("w:t1", "a1", "d1");
        bookmarks.record("w:t1", "a2", "d2");
        bookmarks.record("w:t2", "a2", "d3");
        assert!(bookmarks.retain_areas(|area| area == "a1"));
        assert_eq!(bookmarks.of("w:t1").unwrap().len(), 1);
        assert!(bookmarks.of("w:t2").is_none());
        assert!(!bookmarks.retain_areas(|area| area == "a1"));
        assert!(bookmarks.retain_tabs(|tab| tab == "w:t2"));
        assert!(bookmarks.is_empty());
    }

    #[test]
    fn the_map_is_a_plain_object_of_objects_in_the_file() {
        let mut bookmarks = Bookmarks::default();
        bookmarks.record("w:t1", "a1", "d2");
        let text = serde_json::to_string(&bookmarks).unwrap();
        assert_eq!(text, r#"{"w:t1":{"a1":"d2"}}"#);
        assert_eq!(serde_json::from_str::<Bookmarks>(&text).unwrap(), bookmarks);
    }
}
