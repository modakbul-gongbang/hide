//! Persistent workspace and checkout discovery for the hide navigator.
//!
//! The catalog is deliberately filesystem-first. A workspace registration is
//! metadata only, while checkout discovery is rebuilt from git on launch and
//! after a session update. Removing a registration therefore cannot remove a
//! checkout or terminate a remote process.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::{
    CheckoutSnapshot, DeviceRegistration, DeviceSnapshot, RemoteTarget, TabSnapshot,
    WorkspaceRegistration, WorkspaceSnapshot,
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

pub fn build_catalog(
    registrations: &[WorkspaceRegistration],
    spaces: &[SessionSpace],
) -> Vec<WorkspaceSnapshot> {
    // Project identity is the repository root. Two Herdr workspaces in one
    // repository are one project with both Herdr ids attached, and a Herdr
    // workspace that comes and goes (Herdr closes it with its last pane) never
    // changes which row the user is looking at.
    let mut result: Vec<WorkspaceSnapshot> = Vec::new();
    for space in spaces {
        let projected = inspect_space(space);
        if projected.checkouts.is_empty() {
            continue;
        }
        match result
            .iter_mut()
            .find(|existing| existing.id == projected.id)
        {
            Some(existing) => merge_space(existing, projected),
            None => result.push(projected),
        }
    }

    // A registration Herdr has no workspace for is somewhere the user can
    // still start work, so it stays listed. One Herdr already occupies keeps
    // the registration's identity and label with Herdr's workspace attached,
    // so the row survives Herdr closing that workspace.
    for registration in registrations {
        let root = git_root(Path::new(&registration.path)).unwrap_or_else(|| {
            normalized_path(Path::new(&registration.path))
                .unwrap_or_else(|_| PathBuf::from(&registration.path))
        });
        let comparison = normalized_for_comparison(&root);
        let occupied = result.iter().position(|workspace| {
            workspace.checkouts.iter().any(|checkout| {
                normalized_for_comparison(Path::new(&checkout.path)) == comparison
            })
        });
        match occupied {
            // A project already carrying a registration keeps it; a second
            // registration inside the same project (a repository a
            // multi-repository Herdr workspace also has a pane in) is not a
            // second row, and it must not rename the first.
            Some(index) if result[index].registered => continue,
            Some(index) => adopt_registration(&mut result[index], registration),
            None => result.push(inspect_registered(registration)),
        }
    }

    result.sort_by(|left, right| {
        left.temporary
            .cmp(&right.temporary)
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
            .then_with(|| left.path.cmp(&right.path))
    });
    result
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

/// Projects one Herdr workspace, with a checkout per distinct repository its
/// panes sit in.
///
/// Enumerating `git worktree list` here is what made the sidebar verbose: it
/// listed every branch the repository has ever had a worktree for, each with
/// no tabs. A checkout earns a row by having a pane in it.
fn inspect_space(space: &SessionSpace) -> WorkspaceSnapshot {
    // The project id comes from the first pane's repository root, so it is
    // the same id `inspect_temporary` and a registration of that root derive.
    let mut roots: Vec<(PathBuf, Option<String>, bool)> = Vec::new();
    for cwd in &space.cwds {
        let path = Path::new(cwd);
        let root =
            git_root(path).unwrap_or_else(|| normalized_path(path).unwrap_or_else(|_| path.into()));
        let comparison = normalized_for_comparison(&root);
        if roots
            .iter()
            .any(|(existing, _, _)| normalized_for_comparison(existing) == comparison)
        {
            continue;
        }
        let branch = current_branch(&root);
        let is_worktree = git_root(&root).is_some_and(|resolved| {
            main_worktree_root(&resolved)
                .is_some_and(|main| normalized_for_comparison(&main) != comparison)
        });
        roots.push((root, branch, is_worktree));
    }
    let workspace_id = roots
        .first()
        .map(|(root, _, _)| workspace_id_for_path(root))
        .unwrap_or_else(|| workspace_id_for_path(Path::new(&space.id)));
    let checkouts = roots
        .iter()
        .map(|(root, branch, is_worktree)| {
            let label = branch.clone().unwrap_or_else(|| {
                root.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Checkout")
                    .to_owned()
            });
            checkout(
                &workspace_id,
                root,
                &label,
                branch.clone(),
                *is_worktree,
                false,
            )
        })
        .collect::<Vec<CheckoutSnapshot>>();

    let primary = checkouts
        .first()
        .map(|checkout| checkout.path.clone())
        .unwrap_or_default();
    let repo_name = checkouts
        .first()
        .and_then(|checkout| Path::new(&checkout.path).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or(&space.label)
        .to_owned();

    WorkspaceSnapshot {
        id: workspace_id,
        label: space.label.clone(),
        path: primary,
        remote_target_id: None,
        expanded: true,
        device_id: LOCAL_DEVICE_ID.to_owned(),
        repo_name,
        is_git: checkouts.iter().any(|checkout| checkout.branch.is_some()),
        default_branch: checkouts
            .first()
            .and_then(|checkout| checkout.branch.clone()),
        registered: false,
        temporary: false,
        session_workspace_ids: vec![space.id.clone()],
        checkouts,
    }
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
    // One row for where this workspace actually is. Its other branches are
    // not checkouts until Herdr has a pane in them, and a branch with no pane
    // is reached through the branch picker rather than a permanent row.
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
        id: checkout_id_for_path(workspace_id, path),
        workspace_id: workspace_id.to_owned(),
        label: label.to_owned(),
        path: path.to_string_lossy().into_owned(),
        branch,
        is_worktree,
        exists: path.exists(),
        temporary,
        tabs: Vec::<TabSnapshot>::new(),
    }
}

fn git(path: &Path, arguments: &[&str]) -> Result<std::process::Output, String> {
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("hide-workspace-{name}-{stamp}"));
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

        let catalog = build_catalog(&[registration.clone()], &spaces);

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

        let occupied = build_catalog(&[registration.clone()], &[space]);
        let released = build_catalog(&[registration.clone()], &[]);

        assert_eq!(occupied[0].id, released[0].id);
        assert_eq!(occupied[0].label, released[0].label);
        assert_eq!(occupied[0].checkouts[0].id, released[0].checkouts[0].id);
        assert_eq!(occupied[0].session_workspace_ids, vec!["w7".to_owned()]);
        assert!(released[0].session_workspace_ids.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_first_registration_covering_a_space_keeps_its_identity() {
        let first = temp_dir("first-repo");
        let second = temp_dir("second-repo");
        let space = SessionSpace {
            id: "w9".to_owned(),
            label: "both".to_owned(),
            cwds: vec![
                first.to_string_lossy().into_owned(),
                second.to_string_lossy().into_owned(),
            ],
        };
        let registrations = [
            registration(first.to_str().unwrap(), "First", LOCAL_DEVICE_ID).expect("first"),
            registration(second.to_str().unwrap(), "Second", LOCAL_DEVICE_ID).expect("second"),
        ];

        let catalog = build_catalog(&registrations, &[space]);

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].id, registrations[0].id);
        assert_eq!(catalog[0].label, "First");
        assert_eq!(catalog[0].checkouts.len(), 2);
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

        let catalog = build_catalog(&[], &[space]);

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
            SessionSpace { id: "w1".to_owned(), label: "first".to_owned(), cwds: vec![cwd.clone()] },
            SessionSpace { id: "w2".to_owned(), label: "second".to_owned(), cwds: vec![cwd] },
        ];

        let catalog = build_catalog(&[], &spaces);

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].checkouts.len(), 1);
        assert_eq!(
            catalog[0].session_workspace_ids,
            vec!["w1".to_owned(), "w2".to_owned()]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_registration_no_space_occupies_stays_listed() {
        let root = temp_dir("unopened");
        let registration = registration(root.to_str().unwrap(), "Unopened", LOCAL_DEVICE_ID)
            .expect("registration");
        let registration_id = registration.id.clone();

        let catalog = build_catalog(&[registration], &[]);

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].id, registration_id);
        let _ = fs::remove_dir_all(root);
    }
}
