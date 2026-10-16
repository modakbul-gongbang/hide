//! The View each Agent tab last put in front (PRD lane-files-checkout-views
//! D-03, D-16).
//!
//! A bookmark points at a display, never at a document, so a preview display
//! another tab retargeted shows the new document when it is restored. The map
//! is Hide's own presentation state: it lives on the Workspace's entry in
//! `workspace-views.json` beside `agent_layout`, keyed by the stable Herdr tab
//! id, and is never sent to the shell.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Herdr tab id -> display id. Bounded by the Workspace's tabs, because a tab
/// that left Herdr is pruned; a display that closed is ignored when read.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Bookmarks {
    tabs: BTreeMap<String, String>,
}

impl Bookmarks {
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// The display the tab last put in front.
    pub fn of(&self, tab: &str) -> Option<&str> {
        self.tabs.get(tab).map(String::as_str)
    }

    /// Remembers `display` as the tab's View. Returns whether that changed the
    /// bookmark.
    pub fn record(&mut self, tab: &str, display: &str) -> bool {
        match self.tabs.get_mut(tab) {
            Some(held) if held == display => false,
            Some(held) => {
                display.clone_into(held);
                true
            }
            None => {
                self.tabs.insert(tab.to_owned(), display.to_owned());
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_changes_the_bookmark_only_when_the_view_differs() {
        let mut bookmarks = Bookmarks::default();
        assert!(bookmarks.record("w:t1", "d1"));
        assert!(!bookmarks.record("w:t1", "d1"));
        assert!(bookmarks.record("w:t1", "d2"));
        assert_eq!(bookmarks.of("w:t1"), Some("d2"));
        assert!(bookmarks.of("w:t2").is_none());
    }

    #[test]
    fn pruning_a_tab_leaves_the_rest() {
        let mut bookmarks = Bookmarks::default();
        bookmarks.record("w:t1", "d1");
        bookmarks.record("w:t2", "d2");
        assert!(bookmarks.retain_tabs(|tab| tab == "w:t2"));
        assert!(bookmarks.of("w:t1").is_none());
        assert!(!bookmarks.retain_tabs(|tab| tab == "w:t2"));
        assert!(bookmarks.retain_tabs(|_| false));
        assert!(bookmarks.is_empty());
    }

    #[test]
    fn the_map_is_a_plain_object_in_the_file() {
        let mut bookmarks = Bookmarks::default();
        bookmarks.record("w:t1", "d2");
        let text = serde_json::to_string(&bookmarks).unwrap();
        assert_eq!(text, r#"{"w:t1":"d2"}"#);
        assert_eq!(serde_json::from_str::<Bookmarks>(&text).unwrap(), bookmarks);
    }
}
