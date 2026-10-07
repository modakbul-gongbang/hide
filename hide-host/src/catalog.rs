//! What paths are on this node, as the core's catalog reads them
//! (`hide_node_link::catalog`). Every fact is read from files: the catalog is
//! rebuilt on the session-sync coordinator, where a process would hold every
//! Herdr event behind it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hide_node_link::catalog::{PATH_FACTS_LIMIT, PathFact, PathFacts, RepositoryPlace};
use hide_platform::fs::identity;
use hide_platform::path;

use crate::error::{ErrorCode, HostError, HostResult};

/// Reads each of `paths`; a path that is not absolute is answered as one
/// that does not exist, spelled as it was asked.
pub fn path_facts(paths: &[String]) -> HostResult<PathFacts> {
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
    for asked in paths {
        if facts.paths.contains_key(asked) {
            continue;
        }
        let native = Path::new(asked);
        let fact = if native.is_absolute() {
            read(native, &mut facts.repositories)
        } else {
            PathFact {
                comparison: asked.clone(),
                exists: false,
                repository: None,
            }
        };
        facts.paths.insert(asked.clone(), fact);
    }
    Ok(facts)
}

fn read(
    native: &Path,
    repositories: &mut BTreeMap<
        String,
        Result<BTreeMap<String, hide_node_link::catalog::BranchNote>, String>,
    >,
) -> PathFact {
    let repository = hide_project::git::discover(native).map(|repository| {
        let main_root = comparison(&repository.main_root());
        repositories
            .entry(main_root.clone())
            .or_insert_with(|| repository.branch_notes());
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

    #[test]
    fn more_paths_than_the_limit_are_refused() {
        let paths = vec!["/a".to_owned(); PATH_FACTS_LIMIT + 1];
        assert!(path_facts(&paths).is_err());
    }
}
