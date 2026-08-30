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

pub fn build_catalog(
    registrations: &[WorkspaceRegistration],
    temporary_paths: &[String],
) -> Vec<WorkspaceSnapshot> {
    let mut result = registrations
        .iter()
        .map(inspect_registered)
        .collect::<Vec<_>>();
    let registered_roots = registrations
        .iter()
        .map(|registration| normalized_for_comparison(Path::new(&registration.path)))
        .collect::<Vec<_>>();

    let mut temporary_ids = HashSet::new();
    for raw_path in temporary_paths {
        let path = Path::new(raw_path);
        let root = git_root(path)
            .unwrap_or_else(|| normalized_path(path).unwrap_or_else(|_| path.to_path_buf()));
        let comparison = normalized_for_comparison(&root);
        if registered_roots.iter().any(|registered| {
            comparison == *registered || comparison.starts_with(&format!("{registered}/"))
        }) {
            continue;
        }
        let id = workspace_id_for_path(&root);
        if temporary_ids.insert(id) {
            result.push(inspect_temporary(&root, LOCAL_DEVICE_ID));
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
    let default_branch = git_root_path
        .as_deref()
        .and_then(default_branch)
        .or_else(|| is_git.then(|| "HEAD".to_owned()));
    let checkouts = if let Some(root) = git_root_path.as_deref() {
        discover_checkouts(id, root)
    } else {
        vec![checkout(id, &normalized, "Folder", None, false, temporary)]
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
        default_branch,
        registered,
        temporary,
        checkouts,
    }
}

fn discover_checkouts(workspace_id: &str, root: &Path) -> Vec<CheckoutSnapshot> {
    let mut checkouts = Vec::new();
    if let Ok(output) = git(root, &["worktree", "list", "--porcelain"]) {
        let mut current_path: Option<PathBuf> = None;
        let mut current_branch: Option<String> = None;
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if line.is_empty() {
                if let Some(path) = current_path.take() {
                    let is_worktree =
                        normalized_for_comparison(&path) != normalized_for_comparison(root);
                    let label = current_branch
                        .as_deref()
                        .unwrap_or_else(|| {
                            path.file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("Detached checkout")
                        })
                        .to_owned();
                    checkouts.push(checkout(
                        workspace_id,
                        &path,
                        &label,
                        current_branch.take(),
                        is_worktree,
                        false,
                    ));
                }
                continue;
            }
            if let Some(path) = line.strip_prefix("worktree ") {
                current_path = Some(PathBuf::from(path));
            } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                current_branch = Some(branch.to_owned());
            }
        }
        if let Some(path) = current_path.take() {
            let is_worktree = normalized_for_comparison(&path) != normalized_for_comparison(root);
            let label = current_branch
                .as_deref()
                .unwrap_or("Detached checkout")
                .to_owned();
            checkouts.push(checkout(
                workspace_id,
                &path,
                &label,
                current_branch,
                is_worktree,
                false,
            ));
        }
    }
    if checkouts.is_empty() {
        checkouts.push(checkout(
            workspace_id,
            root,
            "Repository",
            None,
            false,
            false,
        ));
    }
    checkouts
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

fn default_branch(root: &Path) -> Option<String> {
    if let Ok(output) = git(
        root,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) {
        let raw_branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let branch = raw_branch
            .strip_prefix("origin/")
            .unwrap_or(&raw_branch)
            .to_owned();
        if !branch.is_empty() {
            return Some(branch);
        }
    }
    if let Ok(output) = git(root, &["branch", "--format=%(refname:short)"]) {
        let branches = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|branch| !branch.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if let Some(branch) = branches
            .iter()
            .find(|branch| branch.as_str() == "main" || branch.as_str() == "master")
        {
            return Some(branch.clone());
        }
        if let Some(branch) = branches.first() {
            return Some(branch.clone());
        }
    }
    None
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
        assert!(snapshot.is_git);
        assert_eq!(snapshot.default_branch.as_deref(), Some("main"));
        assert_eq!(snapshot.checkouts.len(), 2);
        assert!(
            snapshot
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
    fn temporary_discovery_does_not_duplicate_a_registered_repository() {
        let root = temp_dir("temporary");
        let registration = registration(root.to_str().unwrap(), "Registered", LOCAL_DEVICE_ID)
            .expect("registration");
        let catalog = build_catalog(
            &[registration],
            &[root.join("nested").to_string_lossy().into_owned()],
        );
        assert_eq!(catalog.len(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
