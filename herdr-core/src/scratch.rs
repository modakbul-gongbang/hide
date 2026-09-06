//! The one space that is not a project.
//!
//! Scratch is a single fixed folder the operator never registers and never
//! chooses. Panes whose working directory sits inside it are the navigator's
//! Scratch node and nothing else: not a project row, not a project count, and
//! never the unregistered-folder fallback that would otherwise draw them as an
//! orange temporary workspace in the middle of the project tree.
//!
//! One folder and one node, deliberately. A generalized "non-project space"
//! would need an identity, a registration and a lifecycle, and the product has
//! exactly one of these.

use std::path::{Path, PathBuf};

/// The navigator id of the Scratch node.
///
/// It is not a workspace id: no folder hashes to it, so a project can never
/// collide with it and an event naming it can only mean Scratch.
pub const NODE_ID: &str = "scratch";

/// What the sidebar section is called.
pub const LABEL: &str = "Scratch";

/// The Herdr pane metadata token that holds a chat's title.
///
/// Herdr is the only place a title lives: it survives a Hide restart because
/// the pane does, and Hide keeps no session file of its own to fall out of
/// step with it. The name matches Herdr's token grammar, `[A-Za-z0-9_-]{1,32}`.
pub const TITLE_TOKEN: &str = "hide_chat_title";

/// How much of the first message becomes the title.
///
/// Herdr truncates a token value at 80 characters - characters, not bytes,
/// measured against the pinned server: 90 Korean characters came back as 80
/// characters and 240 bytes. Forty characters is therefore always inside the
/// cap in any script, so the cut is a plain character count with no byte-aware
/// second rule.
pub const TITLE_MAX_CHARS: usize = 40;

/// Where Scratch lives, given a home directory.
///
/// Hide already keeps its own state under this root; Scratch is a sibling of
/// that state rather than a second location to explain.
pub fn root_for_home(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("hide")
        .join("scratch")
}

/// Where Scratch lives on this machine.
///
/// Resolved once when the core is created and carried on the snapshot from
/// there, so the shell, the projection and the launcher all read one answer.
pub fn root() -> PathBuf {
    let home = std::env::var("HOME")
        .ok()
        .filter(|home| !home.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    root_for_home(&home)
}

/// Whether a working directory belongs to Scratch.
///
/// This is the one place the question is asked. Every leak the Scratch node
/// exists to prevent - a scratch pane in the project tree, in the project
/// count, or in the temporary-workspace fallback - is the same judgment made
/// somewhere else, so there is only ever one.
///
/// It compares path components rather than characters, so a sibling folder
/// named `scratch-notes` is not inside `scratch`. It reads nothing from disk:
/// this runs once per pane inside the runtime lock, where a `stat` per pane is
/// latency every attach reader pays.
pub fn contains(root: &str, path: &str) -> bool {
    let root = root.trim_end_matches('/');
    if root.is_empty() {
        return false;
    }
    Path::new(path.trim_end_matches('/')).starts_with(Path::new(root))
}

/// The title a first message earns.
///
/// The first line, trimmed, cut to `TITLE_MAX_CHARS` characters. A message
/// with nothing but whitespace has no title; the composer refuses to send one,
/// so this only guards the type.
pub fn title_from_message(message: &str) -> Option<String> {
    let first_line = message.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        return None;
    }
    Some(first_line.chars().take(TITLE_MAX_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_sits_beside_the_state_hide_already_keeps() {
        assert_eq!(
            root_for_home(Path::new("/Users/example")),
            PathBuf::from("/Users/example/Library/Application Support/hide/scratch")
        );
    }

    #[test]
    fn a_pane_inside_the_folder_belongs_to_scratch() {
        let root = "/Users/example/Library/Application Support/hide/scratch";
        assert!(contains(root, root));
        assert!(contains(root, &format!("{root}/notes")));
        assert!(contains(root, &format!("{root}/")));
    }

    /// The bug a character prefix would have: a folder whose name merely
    /// starts with the scratch folder's name is a different folder.
    #[test]
    fn a_sibling_folder_with_the_same_prefix_is_not_scratch() {
        let root = "/Users/example/Library/Application Support/hide/scratch";
        assert!(!contains(root, &format!("{root}-notes")));
        assert!(!contains(root, "/Users/example/projects/hide"));
    }

    #[test]
    fn an_empty_root_claims_nothing() {
        assert!(!contains("", "/Users/example"));
        assert!(!contains("/", "/Users/example"));
    }

    #[test]
    fn the_title_is_the_first_line_cut_to_forty_characters() {
        assert_eq!(
            title_from_message("  build me a parser  \nsecond line"),
            Some("build me a parser".to_owned())
        );
        let long = "a".repeat(60);
        assert_eq!(title_from_message(&long), Some("a".repeat(40)));
    }

    /// Forty characters, not forty bytes: the cut is measured the same way
    /// Herdr measures its own 80-character cap, so a Korean title is forty
    /// readable characters rather than thirteen.
    #[test]
    fn a_korean_title_is_cut_by_characters() {
        let message = "가".repeat(60);
        let title = title_from_message(&message).expect("a title");
        assert_eq!(title.chars().count(), 40);
        assert!(title.chars().count() <= 80);
    }

    #[test]
    fn a_blank_message_has_no_title() {
        assert_eq!(title_from_message("   \n  "), None);
    }
}
