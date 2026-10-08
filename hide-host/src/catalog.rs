//! What paths are on this node, as the core's catalog reads them
//! (`hide_node_link::catalog`). Every fact is read from files: the catalog is
//! rebuilt on the session-sync coordinator, where a process would hold every
//! Herdr event behind it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hide_node_link::catalog::{
    PATH_FACTS_ANSWER_LIMIT, PATH_FACTS_LIMIT, PathFact, PathFacts, RepositoryPlace,
};
use hide_platform::fs::identity;
use hide_platform::path;

use crate::error::{ErrorCode, HostError, HostResult};

/// Reads each of `paths`; a path that is not absolute is answered as one
/// that does not exist, spelled as it was asked.
pub fn path_facts(paths: &[String]) -> HostResult<PathFacts> {
    path_facts_within(paths, PATH_FACTS_ANSWER_LIMIT)
}

fn path_facts_within(paths: &[String], answer_limit: usize) -> HostResult<PathFacts> {
    if paths.len() > PATH_FACTS_LIMIT {
        return Err(HostError::new(
            ErrorCode::Unsupported,
            format!(
                "{} paths were asked at once; a node answers at most {PATH_FACTS_LIMIT}",
                paths.len()
            ),
        ));
    }
    let mut facts = PathFacts::default();
    let mut bytes = 0_usize;
    for asked in paths {
        if facts.paths.contains_key(asked) {
            continue;
        }
        let native = Path::new(asked);
        let fact = if native.is_absolute() {
            read(native, &mut facts.repositories, &mut bytes)
        } else {
            PathFact {
                comparison: asked.clone(),
                exists: false,
                repository: None,
            }
        };
        bytes = bytes.saturating_add(asked.len() + fact_bytes(&fact));
        if bytes > answer_limit {
            return Err(HostError::new(
                ErrorCode::TooLarge,
                format!(
                    "The facts of {} paths pass the {answer_limit}-byte answer limit",
                    paths.len()
                ),
            ));
        }
        facts.paths.insert(asked.clone(), fact);
    }
    Ok(facts)
}

fn fact_bytes(fact: &PathFact) -> usize {
    fact.comparison.len()
        + fact.repository.as_ref().map_or(0, |place| {
            place.root.len()
                + place.main_root.len()
                + place.branch.as_ref().map_or(0, String::len)
                + place.head_oid.as_ref().map_or(0, String::len)
        })
}

fn notes_bytes(
    notes: &Result<BTreeMap<String, hide_node_link::catalog::BranchNote>, String>,
) -> usize {
    match notes {
        Ok(notes) => notes
            .iter()
            .map(|(branch, note)| {
                branch.len()
                    + note.description.as_ref().map_or(0, String::len)
                    + note.issue.as_ref().map_or(0, String::len)
            })
            .sum(),
        Err(error) => error.len(),
    }
}

fn read(
    native: &Path,
    repositories: &mut BTreeMap<
        String,
        Result<BTreeMap<String, hide_node_link::catalog::BranchNote>, String>,
    >,
    bytes: &mut usize,
) -> PathFact {
    let repository = hide_project::git::discover(native).map(|repository| {
        let main_root = comparison(&repository.main_root());
        if !repositories.contains_key(&main_root) {
            let notes = repository.branch_notes();
            *bytes = bytes.saturating_add(main_root.len() + notes_bytes(&notes));
            repositories.insert(main_root.clone(), notes);
        }
        RepositoryPlace {
            root: comparison(&repository.root),
            main_root,
            branch: repository.branch(),
            head_oid: repository.head_oid(),
        }
    });
    PathFact {
        comparison: comparison(native),
        exists: native.exists(),
        repository,
    }
}

/// `path` with its links resolved as far as it exists: the whole path when
/// it is there, its parent and last name when only the parent is, and the
/// path as written otherwise.
pub fn comparison(native: &Path) -> String {
    path::to_wire_lossy(&resolved(native))
}

fn resolved(native: &Path) -> PathBuf {
    if let Ok(real) = identity::canonical(native) {
        return real;
    }
    match (native.parent(), native.file_name()) {
        (Some(parent), Some(name)) => identity::canonical(parent)
            .map(|parent| parent.join(name))
            .unwrap_or_else(|_| native.to_path_buf()),
        _ => native.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_path_carries_its_place_and_its_notes_once() {
        let temp = tempfile::tempdir().unwrap();
        let root = identity::canonical(temp.path()).unwrap();
        let git = root.join(".git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/topic\n").unwrap();
        std::fs::write(
            git.join("config"),
            "[branch \"topic\"]\n\tdescription = the purpose\n",
        )
        .unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        let src = path::to_wire_lossy(&root.join("src"));
        let gone = path::to_wire_lossy(&root.join("gone"));

        let facts = path_facts(&[src.clone(), gone.clone(), "relative".to_owned()]).unwrap();

        let fact = &facts.paths[&src];
        assert!(fact.exists);
        let place = fact.repository.as_ref().expect("in the repository");
        assert_eq!(place.root, path::to_wire_lossy(&root));
        assert_eq!(place.main_root, place.root);
        assert_eq!(place.branch.as_deref(), Some("topic"));
        let notes = facts.repositories[&place.main_root].as_ref().unwrap();
        assert_eq!(notes["topic"].description.as_deref(), Some("the purpose"));
        assert_eq!(facts.repositories.len(), 1);
        assert!(!facts.paths[&gone].exists);
        assert_eq!(facts.paths[&gone].comparison, gone);
        assert!(!facts.paths["relative"].exists);
        assert!(facts.paths["relative"].repository.is_none());
    }

    /// A repository whose config carries more notes than the answer may
    /// hold refuses the request whole, never a partial answer.
    #[test]
    fn an_answer_past_its_byte_limit_is_refused_as_too_large() {
        let temp = tempfile::tempdir().unwrap();
        let root = identity::canonical(temp.path()).unwrap();
        let git = root.join(".git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/topic\n").unwrap();
        let description = "d".repeat(4096);
        std::fs::write(
            git.join("config"),
            format!("[branch \"topic\"]\n\tdescription = {description}\n"),
        )
        .unwrap();
        let asked = vec![path::to_wire_lossy(&root)];

        assert!(path_facts_within(&asked, 8192).is_ok());
        let refused = path_facts_within(&asked, 4096).unwrap_err();
        assert_eq!(refused.code, ErrorCode::TooLarge);
    }

    #[test]
    fn more_paths_than_the_limit_are_refused() {
        let paths = vec!["/a".to_owned(); PATH_FACTS_LIMIT + 1];
        assert!(path_facts(&paths).is_err());
    }
}
