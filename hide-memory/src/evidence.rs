//! What a card rests on (PRD memory-card-quality D-05, D-06).
//!
//! A card's evidence is the repository files it cites with a fingerprint of
//! their content, and the repository paths and script names its text writes,
//! each with whether it existed when the card was last verified. The core
//! asks the node that holds the checkout for the same facts later; a card
//! whose facts moved is not given to agents until it is verified again.

use serde::{Deserialize, Serialize};

use crate::store::repository_path;

/// Evidence rows one card revision keeps; past it the rest are not recorded.
pub const EVIDENCE_ROW_LIMIT: usize = 16;
/// Names read out of one card's text before the node says which exist.
pub const NAME_CANDIDATE_LIMIT: usize = 24;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    /// A file the card cites; its content fingerprint is kept.
    File,
    /// A repository path the card's text writes.
    Path,
    /// A script name the card's text writes, found by its file name.
    Command,
}

impl EvidenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Path => "path",
            Self::Command => "command",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "file" => Some(Self::File),
            "path" => Some(Self::Path),
            "command" => Some(Self::Command),
            _ => None,
        }
    }
}

/// One thing a card rests on, as it was when the card was last verified.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: EvidenceKind,
    /// A repository-relative path, or a file name for a command.
    pub value: String,
    /// The content revision of a cited file; `None` for a name.
    pub fingerprint: Option<String>,
    /// Whether the node found it.
    pub present: bool,
}

/// What the node answers now about one evidence row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvidenceFact {
    Found {
        fingerprint: Option<String>,
    },
    Missing,
    /// The node could not say; the card keeps its state.
    Unknown,
}

/// Whether `fact` still agrees with what was stored.
pub fn still_holds(stored: &Evidence, fact: &EvidenceFact) -> Option<bool> {
    match fact {
        EvidenceFact::Unknown => None,
        EvidenceFact::Missing => Some(!stored.present),
        EvidenceFact::Found { fingerprint } => Some(match stored.kind {
            // A name that existed must still exist; one that was not found
            // when stored was never evidence, so it cannot have moved.
            EvidenceKind::Path | EvidenceKind::Command => true,
            EvidenceKind::File => {
                stored.present
                    && (stored.fingerprint.is_none() || stored.fingerprint == *fingerprint)
            }
        }),
    }
}

/// The repository paths and script names a card's text writes, in order, at
/// most [`NAME_CANDIDATE_LIMIT`] together. Whether any exists is the node's
/// to say: a name found nowhere when the card is written was never evidence.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WrittenNames {
    pub paths: Vec<String>,
    pub commands: Vec<String>,
}

const SCRIPT_EXTENSIONS: [&str; 8] = ["sh", "py", "mjs", "cjs", "js", "ts", "bash", "zsh"];

pub fn written_names(body: &str) -> WrittenNames {
    let mut names = WrittenNames::default();
    let separators = |c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'));
    for raw in body.split(separators) {
        if names.paths.len() + names.commands.len() >= NAME_CANDIDATE_LIMIT {
            break;
        }
        let trimmed = raw
            .trim_start_matches("./")
            .trim_end_matches(['.', ',', ';', '-', '_']);
        // `macos/` names a folder, which is a path though no `/` is inside.
        let folder = trimmed.ends_with('/');
        let token = trimmed.trim_end_matches('/');
        if token.len() < 3 || !token.chars().any(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        if folder || token.contains('/') {
            if repository_path(token) && !names.paths.iter().any(|known| known == token) {
                names.paths.push(token.to_owned());
            }
        } else if let Some((stem, extension)) = token.rsplit_once('.')
            && !stem.is_empty()
            && SCRIPT_EXTENSIONS.contains(&extension)
            && !names.commands.iter().any(|known| known == token)
        {
            names.commands.push(token.to_owned());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_writes_paths_and_script_names_among_ordinary_words() {
        let names = written_names(
            "`scripts/verify-swift.sh`를 쓰지 않는다. macos/ 폴더와 docs/delivery.md:300, \
             https://example.com/a/b 그리고 build_dev_app.sh, 10/12, ~/.claude/CLAUDE.md, /Users/x/y.",
        );
        assert_eq!(
            names.paths,
            ["scripts/verify-swift.sh", "macos", "docs/delivery.md"]
        );
        assert_eq!(names.commands, ["build_dev_app.sh"]);
    }

    #[test]
    fn names_are_bounded_and_unrepeated() {
        let body = (0..60)
            .map(|index| format!("src/file{index}.rs src/file{index}.rs"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(written_names(&body).paths.len(), NAME_CANDIDATE_LIMIT);
    }

    #[test]
    fn a_fact_that_moved_breaks_the_evidence_and_an_unreadable_one_does_not() {
        let file = Evidence {
            kind: EvidenceKind::File,
            value: "docs/a.md".into(),
            fingerprint: Some("sha256:1".into()),
            present: true,
        };
        let found = |fingerprint: &str| EvidenceFact::Found {
            fingerprint: Some(fingerprint.into()),
        };
        assert_eq!(still_holds(&file, &found("sha256:1")), Some(true));
        assert_eq!(still_holds(&file, &found("sha256:2")), Some(false));
        assert_eq!(still_holds(&file, &EvidenceFact::Missing), Some(false));
        assert_eq!(still_holds(&file, &EvidenceFact::Unknown), None);
        let path = Evidence {
            kind: EvidenceKind::Path,
            value: "macos".into(),
            fingerprint: None,
            present: true,
        };
        assert_eq!(still_holds(&path, &EvidenceFact::Missing), Some(false));
        assert_eq!(still_holds(&path, &found("ignored")), Some(true));
    }
}
