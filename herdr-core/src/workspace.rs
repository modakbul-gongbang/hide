//! Persistent workspace and checkout discovery for the hide navigator.
//!
//! The catalog is deliberately filesystem-first. A workspace registration is
//! metadata only, while checkout discovery is rebuilt from git on launch and
//! after a session update. Removing a registration therefore cannot remove a
//! checkout or terminate a remote process.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::{
    CheckoutSnapshot, DeviceRegistration, DeviceSnapshot, RemoteTarget, TabSnapshot,
    WorkspaceRegistration, WorkspaceSnapshot, WorktreeCatalogSnapshot,
};

pub const LOCAL_DEVICE_ID: &str = "local";

pub fn local_device() -> DeviceSnapshot {
    DeviceSnapshot {
        id: LOCAL_DEVICE_ID.to_owned(),
        label: "This Mac".to_owned(),
        kind: "local".to_owned(),
        state: "ready".to_owned(),
        ssh_alias: None,
        agent_count: 0,
    }
}

pub fn devices(
    remote_targets: &[RemoteTarget],
    registrations: &[DeviceRegistration],
) -> Vec<DeviceSnapshot> {
    let mut result = vec![local_device()];
    let mut seen = HashSet::from([LOCAL_DEVICE_ID.to_owned()]);

    for target in remote_targets {
        if seen.insert(target.id.clone()) {
            result.push(DeviceSnapshot {
                id: target.id.clone(),
                label: target.label.clone(),
                kind: "remote".to_owned(),
                state: "available".to_owned(),
                ssh_alias: Some(target.ssh_alias.clone()),
                agent_count: 0,
            });
        }
    }
    for registration in registrations {
        if seen.insert(registration.id.clone()) {
            result.push(DeviceSnapshot {
                id: registration.id.clone(),
                label: registration.label.clone(),
                kind: if registration.ssh_alias.is_some() {
                    "remote".to_owned()
                } else {
                    "local".to_owned()
                },
                state: "available".to_owned(),
                ssh_alias: registration.ssh_alias.clone(),
                agent_count: 0,
            });
        }
    }
    result
}

pub fn registration(
    path: &str,
    label: &str,
    device_id: &str,
) -> Result<WorkspaceRegistration, String> {
    let path = normalized_path(Path::new(path))?;
    if path.as_os_str().is_empty() {
        return Err("workspace path must not be empty".to_owned());
    }
    let label = if label.trim().is_empty() {
        path.file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("Workspace")
            .to_owned()
    } else {
        label.trim().to_owned()
    };
    Ok(WorkspaceRegistration {
        id: workspace_id_for_path(&path),
        label,
        path: path.to_string_lossy().into_owned(),
        device_id: if device_id.trim().is_empty() {
            LOCAL_DEVICE_ID.to_owned()
        } else {
            device_id.to_owned()
        },
    })
}

pub fn workspace_id_for_path(path: &Path) -> String {
    format!(
        "workspace:{:016x}",
        fnv1a(path.to_string_lossy().as_bytes())
    )
}

pub fn checkout_id_for_path(workspace_id: &str, path: &Path) -> String {
    format!(
        "{workspace_id}:checkout:{:016x}",
        fnv1a(path.to_string_lossy().as_bytes())
    )
}

pub fn inspect_registered(registration: &WorkspaceRegistration) -> WorkspaceSnapshot {
    inspect(
        &registration.id,
        &registration.label,
        Path::new(&registration.path),
        &registration.device_id,
        true,
        false,
    )
}

pub fn inspect_temporary(path: &Path, device_id: &str) -> WorkspaceSnapshot {
    let label = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Unregistered workspace");
    inspect(
        &workspace_id_for_path(path),
        label,
        path,
        device_id,
        false,
        true,
    )
}

/// One Herdr workspace and the working directories its panes occupy.
///
/// Herdr owns the workspace axis, so the navigator mirrors it rather than
/// inventing a second one. Herdr reports no path for a workspace, so the
/// checkouts under it come from where its panes actually are.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionSpace {
    pub id: String,
    pub label: String,
    pub cwds: Vec<String>,
}

/// Each pane directory's repository root in comparison form, keyed by the raw
/// directory Herdr reported. Resolved by the sync coordinator before it takes
/// the runtime lock, because the answer costs one `git rev-parse` per
/// directory and the reconcile that consumes it runs on every publish.
pub type RootIndex = BTreeMap<String, String>;

/// Resolves every directory the session's panes occupy to its repository root,
/// or to the directory itself when it is not inside a repository.
pub fn root_index(spaces: &[SessionSpace]) -> RootIndex {
    let mut index = RootIndex::new();
    for space in spaces {
        for cwd in &space.cwds {
            if index.contains_key(cwd) {
                continue;
            }
            let path = Path::new(cwd);
            let root = git_root(path)
                .map(|root| normalized_for_comparison(&root))
                .unwrap_or_else(|| normalized_for_comparison(path));
            index.insert(cwd.clone(), root);
        }
    }
    index
}

// How many times the current thread has run git. A test that asserts a code
// path never shells out reads it before and after; the runtime lock is held
// through some of those paths, and a fork there is a stall for every thread.
// Per thread, because the test runner runs other tests' git alongside.
#[cfg(test)]
thread_local! {
    static GIT_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn git_calls_on_this_thread() -> usize {
    GIT_CALLS.with(|calls| calls.get())
}

pub fn build_catalog(
    registrations: &[WorkspaceRegistration],
    spaces: &[SessionSpace],
    worktrees: &WorktreeCatalogSnapshot,
) -> Vec<WorkspaceSnapshot> {
    // A project is a repository: its main worktree is the identity and every
    // worktree with a pane in it is a checkout under it. A Herdr workspace
    // whose panes sit in two repositories contributes to two projects, and
    // two Herdr workspaces in one repository are one project with both Herdr
    // ids attached. Herdr closing a workspace with its last pane therefore
    // never changes which row the user is looking at.
    let mut result: Vec<WorkspaceSnapshot> = Vec::new();
    for space in spaces {
        for projected in inspect_space(space) {
            match result
                .iter_mut()
                .find(|existing| existing.id == projected.id)
            {
                Some(existing) => merge_space(existing, projected),
                None => result.push(projected),
            }
        }
    }

    // A registration Herdr has no workspace for is somewhere the user can
    // still start work, so it stays listed. One Herdr already occupies keeps
    // the registration's identity and label with Herdr's workspace attached,
    // so the row survives Herdr closing that workspace.
    for registration in registrations {
        let comparison = normalized_for_comparison(&project_root(Path::new(&registration.path)));
        let occupied = result.iter().position(|workspace| {
            normalized_for_comparison(Path::new(&workspace.path)) == comparison
        });
        match occupied {
            // A second registration for a repository that already carries
            // one is not a second row, and it must not rename the first.
            Some(index) if result[index].registered => continue,
            Some(index) => adopt_registration(&mut result[index], registration),
            None => result.push(inspect_registered(registration)),
        }
    }

    for project in &mut result {
        apply_worktrees(project, worktrees);
    }

    result.sort_by(|left, right| {
        left.temporary
            .cmp(&right.temporary)
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
            .then_with(|| left.path.cmp(&right.path))
    });
    result
}

/// The directory that identifies a project: the repository's main worktree
/// for a git checkout, the folder itself otherwise.
fn project_root(path: &Path) -> PathBuf {
    let root =
        git_root(path).unwrap_or_else(|| normalized_path(path).unwrap_or_else(|_| path.into()));
    if git_root(&root).is_some() {
        main_worktree_root(&root).unwrap_or(root)
    } else {
        root
    }
}

/// Folds a second Herdr workspace in the same repository into the project
/// that already represents it.
fn merge_space(existing: &mut WorkspaceSnapshot, incoming: WorkspaceSnapshot) {
    for id in incoming.session_workspace_ids {
        if !existing.session_workspace_ids.contains(&id) {
            existing.session_workspace_ids.push(id);
        }
    }
    for checkout in incoming.checkouts {
        let comparison = normalized_for_comparison(Path::new(&checkout.path));
        if !existing
            .checkouts
            .iter()
            .any(|known| normalized_for_comparison(Path::new(&known.path)) == comparison)
        {
            existing.checkouts.push(checkout);
        }
    }
}

/// Gives a Herdr-occupied project the identity and label of the registration
/// that covers it. Checkout ids embed the project id, so they are re-keyed
/// too; persisted focus on a registered checkout then resolves whether or not
/// Herdr currently has a workspace there.
fn adopt_registration(workspace: &mut WorkspaceSnapshot, registration: &WorkspaceRegistration) {
    workspace.id = registration.id.clone();
    workspace.label = registration.label.clone();
    workspace.registered = true;
    workspace.temporary = false;
    workspace.device_id = registration.device_id.clone();
    workspace.remote_target_id =
        (registration.device_id != LOCAL_DEVICE_ID).then(|| registration.device_id.clone());
    for checkout in &mut workspace.checkouts {
        checkout.id = checkout_id_for_path(&registration.id, Path::new(&checkout.path));
        checkout.workspace_id = registration.id.clone();
        checkout.temporary = false;
    }
}

/// Projects one Herdr workspace onto the repositories its panes sit in, one
/// project per repository with a checkout per directory a pane occupies.
///
/// This establishes the project rows and the checkouts Herdr can vouch for
/// without git. Every other worktree of the repository is added by
/// [`apply_worktrees`] from the worktree reader's answer, which is what makes
/// a worktree with no terminal a row the operator can select and start one in.
fn inspect_space(space: &SessionSpace) -> Vec<WorkspaceSnapshot> {
    let mut projects: Vec<WorkspaceSnapshot> = Vec::new();
    for cwd in &space.cwds {
        let path = Path::new(cwd);
        let root =
            git_root(path).unwrap_or_else(|| normalized_path(path).unwrap_or_else(|_| path.into()));
        let project_path = project_root(&root);
        let project_comparison = normalized_for_comparison(&project_path);
        let root_comparison = normalized_for_comparison(&root);
        let workspace_id = workspace_id_for_path(&project_path);
        let index = match projects.iter().position(|project| {
            normalized_for_comparison(Path::new(&project.path)) == project_comparison
        }) {
            Some(index) => index,
            None => {
                let name = project_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&space.label)
                    .to_owned();
                projects.push(WorkspaceSnapshot {
                    id: workspace_id.clone(),
                    label: name.clone(),
                    path: project_path.to_string_lossy().into_owned(),
                    remote_target_id: None,
                    expanded: true,
                    device_id: LOCAL_DEVICE_ID.to_owned(),
                    repo_name: name,
                    is_git: git_root(&root).is_some(),
                    default_branch: None,
                    branches: Vec::new(),
                    registered: false,
                    temporary: false,
                    session_workspace_ids: vec![space.id.clone()],
                    checkouts: Vec::new(),
                });
                projects.len() - 1
            }
        };
        if projects[index]
            .checkouts
            .iter()
            .any(|existing| normalized_for_comparison(Path::new(&existing.path)) == root_comparison)
        {
            continue;
        }
        let branch = current_branch(&root);
        let label = branch.clone().unwrap_or_else(|| {
            root.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Checkout")
                .to_owned()
        });
        let is_worktree = root_comparison != project_comparison;
        if !is_worktree {
            projects[index].default_branch = branch.clone();
        }
        projects[index].checkouts.push(checkout(
            &workspace_id,
            &root,
            &label,
            branch,
            is_worktree,
            false,
        ));
    }
    // The main worktree leads, so the primary badge and the project path
    // agree even when a worktree's pane was reported first.
    for project in &mut projects {
        let project_comparison = normalized_for_comparison(Path::new(&project.path));
        if let Some(index) = project.checkouts.iter().position(|checkout| {
            normalized_for_comparison(Path::new(&checkout.path)) == project_comparison
        }) && index != 0
        {
            let main = project.checkouts.remove(index);
            project.checkouts.insert(0, main);
        }
    }
    projects
}

/// Adds every worktree git reports to the project, and carries each
/// worktree's counts onto the row that represents it.
///
/// A worktree with no pane becomes a row here; one that already has a row from
/// a pane keeps its identity and only gains the counts. The reader has not
/// answered for a project until it appears in the catalog, and a project with
/// no answer keeps exactly the rows it already had rather than losing them.
pub(crate) fn apply_worktrees(
    project: &mut WorkspaceSnapshot,
    worktrees: &WorktreeCatalogSnapshot,
) {
    let project_comparison = normalized_for_comparison(Path::new(&project.path));
    let Some(listed) = worktrees.projects.iter().find(|listed| {
        normalized_for_comparison(Path::new(&listed.root_path)) == project_comparison
    }) else {
        return;
    };
    project.default_branch = listed.default_branch.clone();
    project.branches = listed.branches.clone();

    for worktree in &listed.worktrees {
        let comparison = normalized_for_comparison(Path::new(&worktree.path));
        let existing = project
            .checkouts
            .iter()
            .position(|known| normalized_for_comparison(Path::new(&known.path)) == comparison);
        let index = match existing {
            Some(index) => index,
            None => {
                let path = PathBuf::from(&worktree.path);
                let label = worktree.branch.clone().unwrap_or_else(|| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("Checkout")
                        .to_owned()
                });
                project.checkouts.push(checkout(
                    &project.id,
                    &path,
                    &label,
                    worktree.branch.clone(),
                    !worktree.is_main,
                    project.temporary,
                ));
                project.checkouts.len() - 1
            }
        };
        let row = &mut project.checkouts[index];
        row.is_worktree = !worktree.is_main;
        row.label = worktree_row_label(worktree.branch.as_deref(), worktree.head_sha.as_deref());
        row.exists = !worktree.missing;
        row.dirty = worktree.dirty;
        row.changed_file_count = worktree.changed_file_count;
        row.base_branch = worktree.base_branch.clone();
        row.ahead = worktree.ahead;
        row.behind = worktree.behind;
        row.added_lines = worktree.added_lines;
        row.removed_lines = worktree.removed_lines;
        row.unpushed = worktree.unpushed.clone();
        row.worktree = Some(worktree.clone());
        row.branch = worktree.branch.clone();
    }

    // The main worktree leads, so the primary badge and the project path
    // agree whichever order the rows were created in.
    if let Some(index) = project.checkouts.iter().position(|checkout| {
        normalized_for_comparison(Path::new(&checkout.path)) == project_comparison
    }) && index != 0
    {
        let main = project.checkouts.remove(index);
        project.checkouts.insert(0, main);
    }
}

pub(crate) fn worktree_row_label(branch: Option<&str>, head_sha: Option<&str>) -> String {
    branch.map(str::to_owned).unwrap_or_else(|| {
        format!(
            "Detached HEAD · {}",
            head_sha
                .unwrap_or("unknown")
                .chars()
                .take(8)
                .collect::<String>()
        )
    })
}

fn current_branch(root: &Path) -> Option<String> {
    let output = git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).ok()?;
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!branch.is_empty() && branch != "HEAD").then_some(branch)
}

/// The repository's main working tree, which tells a linked worktree apart
/// from the checkout that owns the git directory.
fn main_worktree_root(root: &Path) -> Option<PathBuf> {
    let output = git(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()?;
    let common = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!common.is_empty())
        .then(|| PathBuf::from(common))
        .and_then(|common| common.parent().map(Path::to_path_buf))
}

pub fn git_root(path: &Path) -> Option<PathBuf> {
    let output = git(path, &["rev-parse", "--show-toplevel"]).ok()?;
    let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!root.is_empty()).then(|| PathBuf::from(root))
}

/// A base directory for tests that assert what the catalog says about a
/// directory which is not a checkout.
///
/// `std::env::temp_dir()` is not always outside a repository. The verification
/// sandbox points `TMPDIR` inside this repository, and git discovery walks up
/// from a directory created there and finds it, so a test that means "a folder
/// with no repository" gets the enclosing repository instead. Picking the base
/// by asking git, rather than assuming, keeps those tests measuring the product
/// instead of the environment they happen to run in.
#[cfg(test)]
pub(crate) fn temp_base_outside_any_repository() -> &'static Path {
    use std::sync::OnceLock;

    static BASE: OnceLock<PathBuf> = OnceLock::new();
    BASE.get_or_init(|| {
        let preferred = std::env::temp_dir();
        for candidate in [preferred.clone(), PathBuf::from("/tmp")] {
            if candidate.is_dir() && git_root(&candidate).is_none() {
                return candidate;
            }
        }
        panic!(
            "these tests need a temporary directory outside every git repository; \
             both {} and /tmp are inside one",
            preferred.display()
        )
    })
    .as_path()
}

pub fn initialize_git(path: &Path) -> Result<(), String> {
    if git_root(path).is_some() {
        return Ok(());
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["init"])
        .output()
        .map_err(|error| format!("git init could not start: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_failure("git init", &output))
    }
}

fn inspect(
    id: &str,
    label: &str,
    path: &Path,
    device_id: &str,
    registered: bool,
    temporary: bool,
) -> WorkspaceSnapshot {
    let normalized = normalized_path(path).unwrap_or_else(|_| path.to_path_buf());
    let git_root_path = git_root(&normalized);
    let is_git = git_root_path.is_some();
    // One row for where this registration points. The repository's other
    // worktrees are added from the worktree reader's answer, so this stands
    // alone only for a plain folder and for the ticks before the first read.
    let branch = git_root_path.as_deref().and_then(current_branch);
    let checkouts = match git_root_path.as_deref() {
        Some(root) => vec![checkout(
            id,
            root,
            branch.as_deref().unwrap_or("Repository"),
            branch.clone(),
            false,
            temporary,
        )],
        None => vec![checkout(id, &normalized, "Folder", None, false, temporary)],
    };

    WorkspaceSnapshot {
        id: id.to_owned(),
        label: label.to_owned(),
        path: normalized.to_string_lossy().into_owned(),
        remote_target_id: (device_id != LOCAL_DEVICE_ID).then(|| device_id.to_owned()),
        expanded: true,
        device_id: device_id.to_owned(),
        repo_name: git_root_path
            .as_deref()
            .and_then(|root| root.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or(label)
            .to_owned(),
        is_git,
        default_branch: branch,
        branches: Vec::new(),
        registered,
        temporary,
        session_workspace_ids: Vec::new(),
        checkouts,
    }
}

fn checkout(
    workspace_id: &str,
    path: &Path,
    label: &str,
    branch: Option<String>,
    is_worktree: bool,
    temporary: bool,
) -> CheckoutSnapshot {
    CheckoutSnapshot {
        // A checkout with no Herdr tabs yet: the first one the operator makes
        // here is Tab 1. Reconcile overwrites this the moment Herdr reports any.
        next_tab_label: crate::model::next_tab_label(std::iter::empty()),
        id: checkout_id_for_path(workspace_id, path),
        workspace_id: workspace_id.to_owned(),
        label: label.to_owned(),
        path: path.to_string_lossy().into_owned(),
        branch,
        is_worktree,
        exists: path.exists(),
        temporary,
        tabs: Vec::<TabSnapshot>::new(),
        active_tab_id: None,
        strip: Vec::new(),
        // The git facts arrive from the worktree reader; the catalog only
        // decides which rows exist.
        ..CheckoutSnapshot::default()
    }
}

fn git(path: &Path, arguments: &[&str]) -> Result<std::process::Output, String> {
    #[cfg(test)]
    GIT_CALLS.with(|calls| calls.set(calls.get() + 1));
    Command::new("git")
        .arg("-C")
        .arg(path)
        .args(arguments)
        .output()
        .map_err(|error| format!("git command could not start: {error}"))
        .and_then(|output| {
            if output.status.success() {
                Ok(output)
            } else {
                Err(command_failure("git", &output))
            }
        })
}

fn command_failure(command: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        format!("{command} exited with {}", output.status)
    } else {
        format!("{command}: {stderr}")
    }
}

fn normalized_path(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("workspace path must not be empty".to_owned());
    }
    if path.exists() {
        fs::canonicalize(path)
            .map_err(|error| format!("workspace path could not be resolved: {error}"))
    } else if let Some(parent) = path.parent().filter(|parent| parent.exists()) {
        let canonical_parent = fs::canonicalize(parent)
            .map_err(|error| format!("workspace parent could not be resolved: {error}"))?;
        Ok(canonical_parent.join(path.file_name().unwrap_or_default()))
    } else {
        Ok(path.to_path_buf())
    }
}

pub fn normalized_for_comparison(path: &Path) -> String {
    normalized_path(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .trim_end_matches('/')
        .to_owned()
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProjectWorktreesSnapshot, WorktreeSnapshot};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// The catalog before the worktree reader has answered. Every case that
    /// is not about worktree rows uses this, so those tests still assert what
    /// the pane-derived catalog alone produces.
    fn no_worktrees() -> WorktreeCatalogSnapshot {
        WorktreeCatalogSnapshot::default()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            temp_base_outside_any_repository().join(format!("hide-workspace-{name}-{stamp}"));
        fs::create_dir_all(&path).expect("temp directory");
        path
    }

    #[test]
    fn discovers_git_default_branch_and_worktrees_without_mutating_the_repository() {
        let root = temp_dir("git");
        let status = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["init", "-b", "main"])
            .status()
            .expect("git init");
        assert!(status.success());
        fs::write(root.join("README.md"), "fixture\n").expect("fixture file");
        let add = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["add", "."])
            .status()
            .expect("git add");
        assert!(add.success());
        let commit = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.email=hide@example.invalid",
                "-c",
                "user.name=hide-test",
                "commit",
                "-m",
                "fixture",
            ])
            .status()
            .expect("git commit");
        assert!(commit.success());
        let checkout_path = root.with_file_name(format!(
            "{}-worktree",
            root.file_name().unwrap().to_string_lossy()
        ));
        let worktree = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args([
                "worktree",
                "add",
                "-b",
                "feature",
                checkout_path.to_str().unwrap(),
            ])
            .status()
            .expect("git worktree add");
        assert!(worktree.success());

        let registration =
            registration(root.to_str().unwrap(), "Demo", LOCAL_DEVICE_ID).expect("registration");
        let snapshot = inspect_registered(&registration);
        // A registration is one row for where it is. The `feature` worktree
        // exists on disk but has no pane, so it is not a checkout row.
        assert!(snapshot.is_git);
        assert_eq!(snapshot.default_branch.as_deref(), Some("main"));
        assert_eq!(snapshot.checkouts.len(), 1);
        assert_eq!(snapshot.checkouts[0].branch.as_deref(), Some("main"));

        // It becomes one once a Herdr workspace has a pane in it.
        let occupied = build_catalog(
            &[],
            &[SessionSpace {
                id: "w1".to_owned(),
                label: "Demo".to_owned(),
                cwds: vec![
                    root.to_string_lossy().into_owned(),
                    checkout_path.to_string_lossy().into_owned(),
                ],
            }],
            &no_worktrees(),
        );
        assert_eq!(occupied.len(), 1);
        assert_eq!(occupied[0].checkouts.len(), 2);
        assert!(
            occupied[0]
                .checkouts
                .iter()
                .any(|checkout| checkout.is_worktree
                    && checkout.branch.as_deref() == Some("feature"))
        );
        assert!(root.join("README.md").exists());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&checkout_path);
    }

    #[test]
    fn flat_folder_is_visible_without_implicit_git_init() {
        let root = temp_dir("flat");
        let registration =
            registration(root.to_str().unwrap(), "Flat", LOCAL_DEVICE_ID).expect("registration");
        let snapshot = inspect_registered(&registration);
        assert!(!snapshot.is_git);
        assert_eq!(snapshot.checkouts.len(), 1);
        assert_eq!(snapshot.checkouts[0].label, "Folder");
        assert!(!root.join(".git").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_space_occupying_a_registered_directory_does_not_duplicate_it() {
        let root = temp_dir("temporary");
        let registration = registration(root.to_str().unwrap(), "Registered", LOCAL_DEVICE_ID)
            .expect("registration");
        let spaces = [SessionSpace {
            id: "w1".to_owned(),
            label: "Registered".to_owned(),
            cwds: vec![root.to_string_lossy().into_owned()],
        }];

        let catalog = build_catalog(&[registration.clone()], &spaces, &no_worktrees());

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].id, registration.id);
        assert_eq!(catalog[0].label, "Registered");
        assert!(catalog[0].registered);
        assert_eq!(catalog[0].session_workspace_ids, vec!["w1".to_owned()]);
        let _ = fs::remove_dir_all(root);
    }

    /// Herdr closes a workspace together with its last pane. The project and
    /// checkout the user had selected must be the same rows afterwards, so a
    /// persisted focus keeps resolving and the checkout offers to start a new
    /// terminal instead of the app falling back to "no workspace".
    #[test]
    fn a_project_keeps_its_identity_when_herdr_closes_its_workspace() {
        let root = temp_dir("identity");
        let registration = registration(root.to_str().unwrap(), "Identity", LOCAL_DEVICE_ID)
            .expect("registration");
        let space = SessionSpace {
            id: "w7".to_owned(),
            label: "hide main".to_owned(),
            cwds: vec![root.to_string_lossy().into_owned()],
        };

        let occupied = build_catalog(&[registration.clone()], &[space], &no_worktrees());
        let released = build_catalog(&[registration.clone()], &[], &no_worktrees());

        assert_eq!(occupied[0].id, released[0].id);
        assert_eq!(occupied[0].label, released[0].label);
        assert_eq!(occupied[0].checkouts[0].id, released[0].checkouts[0].id);
        assert_eq!(occupied[0].session_workspace_ids, vec!["w7".to_owned()]);
        assert!(released[0].session_workspace_ids.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    /// A Herdr workspace with panes in two repositories is two projects, and
    /// each registration lands on its own repository.
    #[test]
    fn a_space_spanning_two_repositories_is_two_projects() {
        let first = temp_dir("first-repo");
        let second = temp_dir("second-repo");
        let space = SessionSpace {
            id: "w9".to_owned(),
            label: "hide main".to_owned(),
            cwds: vec![
                first.to_string_lossy().into_owned(),
                second.to_string_lossy().into_owned(),
            ],
        };
        let registrations = [
            registration(first.to_str().unwrap(), "First", LOCAL_DEVICE_ID).expect("first"),
            registration(second.to_str().unwrap(), "Second", LOCAL_DEVICE_ID).expect("second"),
        ];

        let unregistered = build_catalog(&[], &[space.clone()], &no_worktrees());
        let catalog = build_catalog(&registrations, &[space], &no_worktrees());

        assert_eq!(unregistered.len(), 2);
        assert!(
            unregistered
                .iter()
                .all(|project| project.label != "hide main")
        );
        assert_eq!(catalog.len(), 2);
        let labels = catalog.iter().map(|p| p.label.as_str()).collect::<Vec<_>>();
        assert_eq!(labels, vec!["First", "Second"]);
        assert!(catalog.iter().all(|project| {
            project.registered
                && project.checkouts.len() == 1
                && project.session_workspace_ids == vec!["w9".to_owned()]
        }));
        let _ = fs::remove_dir_all(first);
        let _ = fs::remove_dir_all(second);
    }

    #[test]
    fn an_unregistered_space_is_keyed_by_its_repository_path() {
        let root = temp_dir("unregistered-space");
        let space = SessionSpace {
            id: "w8".to_owned(),
            label: "scratch".to_owned(),
            cwds: vec![root.to_string_lossy().into_owned()],
        };

        let catalog = build_catalog(&[], &[space], &no_worktrees());

        assert_eq!(catalog.len(), 1);
        let canonical = fs::canonicalize(&root).expect("canonical root");
        assert_eq!(catalog[0].id, workspace_id_for_path(&canonical));
        assert_eq!(catalog[0].session_workspace_ids, vec!["w8".to_owned()]);
        assert!(!catalog[0].registered);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn two_spaces_in_one_repository_are_one_project() {
        let root = temp_dir("shared-root");
        let cwd = root.to_string_lossy().into_owned();
        let spaces = [
            SessionSpace {
                id: "w1".to_owned(),
                label: "first".to_owned(),
                cwds: vec![cwd.clone()],
            },
            SessionSpace {
                id: "w2".to_owned(),
                label: "second".to_owned(),
                cwds: vec![cwd],
            },
        ];

        let catalog = build_catalog(&[], &spaces, &no_worktrees());

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].checkouts.len(), 1);
        assert_eq!(
            catalog[0].session_workspace_ids,
            vec!["w1".to_owned(), "w2".to_owned()]
        );
        let _ = fs::remove_dir_all(root);
    }

    fn listed_worktree(path: &str, branch: &str, is_main: bool) -> WorktreeSnapshot {
        WorktreeSnapshot {
            path: path.to_owned(),
            branch: Some(branch.to_owned()),
            is_main,
            ..WorktreeSnapshot::default()
        }
    }

    /// The rule R1 replaces: a worktree is a row because git lists it, not
    /// because Herdr has a pane in it. The worktree that has a pane keeps its
    /// identity and gains the counts; the ones without become rows that can
    /// be selected and started in.
    #[test]
    fn every_worktree_is_a_row_whether_or_not_a_pane_sits_in_it() {
        let root = temp_dir("all-worktrees");
        let idle = root.with_file_name(format!(
            "{}-idle",
            root.file_name().unwrap().to_string_lossy()
        ));
        let second = root.with_file_name(format!(
            "{}-second",
            root.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir_all(&idle).expect("idle worktree");
        fs::create_dir_all(&second).expect("second worktree");
        let registration =
            registration(root.to_str().unwrap(), "Project", LOCAL_DEVICE_ID).expect("registration");
        let worktrees = WorktreeCatalogSnapshot {
            projects: vec![ProjectWorktreesSnapshot {
                root_path: root.to_string_lossy().into_owned(),
                default_branch: Some("main".to_owned()),
                worktrees: vec![
                    WorktreeSnapshot {
                        dirty: true,
                        changed_file_count: 3,
                        ahead: 2,
                        behind: 1,
                        added_lines: 42,
                        removed_lines: 7,
                        base_branch: Some("release".to_owned()),
                        unpushed: Some(crate::model::UnpushedSnapshot {
                            remote: "origin".to_owned(),
                            count: 1,
                        }),
                        ..listed_worktree(root.to_str().unwrap(), "main", true)
                    },
                    listed_worktree(idle.to_str().unwrap(), "idle", false),
                    listed_worktree(second.to_str().unwrap(), "second", false),
                ],
                unavailable_reason: None,
                ..ProjectWorktreesSnapshot::default()
            }],
        };

        let catalog = build_catalog(
            &[registration],
            &[SessionSpace {
                id: "w1".to_owned(),
                label: "Project".to_owned(),
                cwds: vec![root.to_string_lossy().into_owned()],
            }],
            &worktrees,
        );

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].default_branch.as_deref(), Some("main"));
        let rows = &catalog[0].checkouts;
        assert_eq!(rows.len(), 3, "one row per worktree, none duplicated");
        // The worktree Herdr already had a row for keeps that one row and
        // gains the counts, rather than becoming a second row beside it.
        let occupied = &rows[0];
        assert_eq!(
            normalized_for_comparison(Path::new(&occupied.path)),
            normalized_for_comparison(&root),
            "the main worktree leads"
        );
        assert!(!occupied.is_worktree);
        assert!(occupied.dirty);
        assert_eq!(occupied.changed_file_count, 3);
        assert_eq!((occupied.ahead, occupied.behind), (2, 1));
        assert_eq!((occupied.added_lines, occupied.removed_lines), (42, 7));
        assert_eq!(occupied.base_branch.as_deref(), Some("release"));
        assert_eq!(
            occupied.unpushed.as_ref().map(|unpushed| unpushed.count),
            Some(1)
        );

        let idle_row = rows.iter().find(|row| row.label == "idle").expect("idle");
        assert!(idle_row.is_worktree, "a linked worktree is marked as one");
        assert!(idle_row.exists);
        assert!(
            idle_row.tabs.is_empty(),
            "it has no pane, and that is the point"
        );
        assert!(rows.iter().any(|row| row.label == "second"));
        // Every row is keyed under the project, so a persisted selection on a
        // worktree with no pane still resolves.
        assert!(rows.iter().all(|row| row.workspace_id == catalog[0].id));

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&idle);
        let _ = fs::remove_dir_all(&second);
    }

    /// A worktree git lists but disk does not have keeps its row and reports
    /// that the path is gone, so the operator can see what to clean up.
    #[test]
    fn a_missing_worktree_is_a_row_that_says_it_is_missing() {
        let root = temp_dir("missing-worktree");
        let registration =
            registration(root.to_str().unwrap(), "Project", LOCAL_DEVICE_ID).expect("registration");
        let worktrees = WorktreeCatalogSnapshot {
            projects: vec![ProjectWorktreesSnapshot {
                root_path: root.to_string_lossy().into_owned(),
                default_branch: Some("main".to_owned()),
                worktrees: vec![
                    listed_worktree(root.to_str().unwrap(), "main", true),
                    WorktreeSnapshot {
                        missing: true,
                        ..listed_worktree("/definitely/not/here/hide-test", "gone", false)
                    },
                ],
                unavailable_reason: None,
                ..ProjectWorktreesSnapshot::default()
            }],
        };

        let catalog = build_catalog(&[registration], &[], &worktrees);

        let gone = catalog[0]
            .checkouts
            .iter()
            .find(|row| row.label == "gone")
            .expect("the missing worktree is still a row");
        assert!(!gone.exists);
        let _ = fs::remove_dir_all(root);
    }

    /// Before the reader answers, and for a project it has no answer for, the
    /// rows the pane-derived catalog produced stay exactly as they were.
    #[test]
    fn a_project_with_no_worktree_answer_keeps_the_rows_it_had() {
        let root = temp_dir("no-answer");
        let registration =
            registration(root.to_str().unwrap(), "Project", LOCAL_DEVICE_ID).expect("registration");

        let before = build_catalog(&[registration.clone()], &[], &no_worktrees());
        let unrelated = build_catalog(
            &[registration],
            &[],
            &WorktreeCatalogSnapshot {
                projects: vec![ProjectWorktreesSnapshot {
                    root_path: "/some/other/repository".to_owned(),
                    ..ProjectWorktreesSnapshot::default()
                }],
            },
        );

        assert_eq!(before, unrelated);
        assert_eq!(before[0].checkouts.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_registration_no_space_occupies_stays_listed() {
        let root = temp_dir("unopened");
        let registration = registration(root.to_str().unwrap(), "Unopened", LOCAL_DEVICE_ID)
            .expect("registration");
        let registration_id = registration.id.clone();

        let catalog = build_catalog(&[registration], &[], &no_worktrees());

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].id, registration_id);
        let _ = fs::remove_dir_all(root);
    }
}
