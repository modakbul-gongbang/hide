//! What the guardrails read about a path: the checkout that holds it, read
//! from the repository's own files (`hide_project::git`), and the policy
//! project it belongs to. No Git process runs here.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{Policy, ProjectPolicy};

/// A checkout of a policy project: a Git worktree, or a plain folder
/// project's root (`git` false), whose only protection is the files one.
#[derive(Clone, Debug)]
pub(super) struct Checkout {
    /// The checkout's own root.
    pub root: PathBuf,
    /// The repository's main worktree root; the folder itself for a folder.
    pub main_root: PathBuf,
    pub branch: Option<String>,
    common_dir: Option<PathBuf>,
}

impl Checkout {
    pub fn git(&self) -> bool {
        self.common_dir.is_some()
    }

    /// The original checkout: the repository's main worktree.
    pub fn original(&self) -> bool {
        self.git() && self.root == self.main_root
    }
}

#[derive(Clone, Debug)]
pub(super) enum Located {
    /// Under no project of the policy.
    Outside,
    In(Checkout),
    /// Under a policy project's root, but its repository could not be read.
    Unreadable,
}

#[derive(Clone, Debug)]
pub(super) struct Place {
    pub place: Located,
    project: Option<usize>,
}

pub(super) struct Project<'a> {
    pub policy: &'a ProjectPolicy,
}

impl Place {
    pub fn project<'a>(&self, facts: &Facts<'a>) -> Option<Project<'a>> {
        self.project.map(|index| Project {
            policy: &facts.policy.projects[index],
        })
    }
}

pub(super) struct Facts<'a> {
    policy: &'a Policy,
    pub home: PathBuf,
    /// Each project's root as the filesystem spells it; `None` when it is
    /// gone, so nothing is matched against it.
    roots: Vec<Option<PathBuf>>,
    places: RefCell<HashMap<PathBuf, Place>>,
}

impl<'a> Facts<'a> {
    pub fn new(policy: &'a Policy, home: &Path) -> Self {
        Self {
            policy,
            home: canonical_prefix(home),
            roots: policy
                .projects
                .iter()
                .map(|project| {
                    project
                        .rules
                        .any()
                        .then(|| {
                            hide_platform::fs::identity::canonical(Path::new(&project.root)).ok()
                        })
                        .flatten()
                })
                .collect(),
            places: RefCell::new(HashMap::new()),
        }
    }

    /// The checkout and project that hold `path`, which need not exist yet.
    pub fn locate(&self, path: &Path) -> Place {
        let (existing, _) = existing_ancestor(path);
        if let Some(place) = self.places.borrow().get(&existing) {
            return place.clone();
        }
        let place = self.read(path, &existing);
        self.places.borrow_mut().insert(existing, place.clone());
        place
    }

    fn read(&self, path: &Path, existing: &Path) -> Place {
        let spelled = canonical_prefix(path);
        let under = self
            .roots
            .iter()
            .position(|root| root.as_ref().is_some_and(|root| spelled.starts_with(root)));
        match hide_project::git::discover_checked(existing) {
            Ok(Some(repository)) => {
                let (Ok(root), Ok(main_root)) = (
                    hide_platform::fs::identity::canonical(&repository.root),
                    hide_platform::fs::identity::canonical(&repository.main_root()),
                ) else {
                    return self.unreadable(under);
                };
                let Some(project) = self.root_index(&main_root) else {
                    // A repository nested in a folder project is the folder's.
                    return self.folder(under);
                };
                Place {
                    place: Located::In(Checkout {
                        branch: repository.branch(),
                        common_dir: Some(repository.common_dir.clone()),
                        root,
                        main_root,
                    }),
                    project: Some(project),
                }
            }
            Ok(None) => self.folder(under),
            Err(_) => self.unreadable(under),
        }
    }

    fn root_index(&self, main_root: &Path) -> Option<usize> {
        self.roots
            .iter()
            .position(|root| root.as_deref() == Some(main_root))
    }

    fn folder(&self, under: Option<usize>) -> Place {
        let Some(index) = under else {
            return Place {
                place: Located::Outside,
                project: None,
            };
        };
        let root = self.roots[index].clone().unwrap_or_default();
        Place {
            place: Located::In(Checkout {
                main_root: root.clone(),
                root,
                branch: None,
                common_dir: None,
            }),
            project: Some(index),
        }
    }

    fn unreadable(&self, under: Option<usize>) -> Place {
        match under {
            Some(index) => Place {
                place: Located::Unreadable,
                project: Some(index),
            },
            None => Place {
                place: Located::Outside,
                project: None,
            },
        }
    }

    /// The branch the base branch protection guards in `checkout`'s
    /// repository: the operator's choice, else what `origin/HEAD` names.
    pub fn base(&self, project: &ProjectPolicy, checkout: &Checkout) -> Option<String> {
        if let Some(base) = &project.base {
            return Some(base.clone());
        }
        let common = checkout.common_dir.as_ref()?;
        let text = std::fs::read_to_string(common.join("refs/remotes/origin/HEAD")).ok()?;
        let branch = text
            .trim()
            .strip_prefix("ref:")?
            .trim()
            .strip_prefix("refs/remotes/origin/")?;
        (!branch.is_empty()).then(|| branch.to_owned())
    }
}

/// The deepest ancestor of `path` that exists (itself when it does), and
/// what follows it.
fn existing_ancestor(path: &Path) -> (PathBuf, PathBuf) {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while std::fs::symlink_metadata(&existing).is_err() {
        let Some(name) = existing.file_name().map(|name| name.to_os_string()) else {
            break;
        };
        rest.push(name);
        if !existing.pop() {
            break;
        }
    }
    let rest = rest.into_iter().rev().collect();
    (existing, rest)
}

/// `path` with its existing part spelled the way the filesystem spells it
/// (links followed, case as stored), so it compares by name with a project
/// root on a filesystem that ignores case too (B24).
pub(super) fn canonical_prefix(path: &Path) -> PathBuf {
    let (existing, rest) = existing_ancestor(path);
    match hide_platform::fs::identity::canonical(&existing) {
        Ok(canonical) if rest.as_os_str().is_empty() => canonical,
        Ok(canonical) => canonical.join(rest),
        Err(_) => path.to_path_buf(),
    }
}
