//! A device's projects (PRD S5.5 B1-B4, D-03).
//!
//! The session projection (`session_sync/replica.rs`) turns each Herdr
//! workspace on a device into one row with one checkout. This module groups
//! those rows the way this machine's catalog groups its own (`workspace.rs`):
//! a project is a repository, identified by its main worktree on that device,
//! and every checkout a tab sits in is a row under it. The facts come from the
//! device's helper (`Call::Project`), never from this machine's filesystem: a
//! path on another machine means nothing here.
//!
//! A directory the helper has not answered for keeps its Herdr workspace as a
//! row of its own and the catalog says why (`DeviceCatalogSnapshot`), so an
//! unconfirmed grouping is never presented as a confirmed one.
//!
//! Ids stay scoped to the device. A project is
//! `remote:<device>:project:<sha256(device, root)>`, so the same path or the
//! same repository on two devices is two projects. The checkout that holds a
//! Herdr workspace's own directory keeps that workspace's checkout id
//! (`remote:<device>:checkout:<workspace>`), so a file tab or a focus that
//! names it survives the grouping; a second checkout a tab of that workspace
//! sits in is `…:checkout:<workspace>#<hash of its root>`. Either way the
//! checkout id names exactly one Herdr workspace, which is what a command sent
//! to that host needs (`remote_checkout_source_id`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use hide_project::{ProjectFacts, ProjectKind};

use crate::model::{
    CheckoutSnapshot, DeviceCatalogRefusal, DeviceCatalogSnapshot, RemoteSessionSnapshot,
    StripTabKind, TabSnapshot, WorkspaceSnapshot,
};

/// What the device's helper said about one directory.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Fact {
    Known(ProjectFacts),
    /// The helper answered and refused: the folder is missing or unreadable.
    Refused(String),
}

/// The helper's answers for one device, kept for the life of its helper
/// connection; a new connection asks again.
#[derive(Debug, Default)]
pub(crate) struct DeviceFacts {
    pub(crate) facts: BTreeMap<String, Fact>,
    /// The helper generation a request is running against, if one is.
    pub(crate) in_flight: Option<u64>,
    /// Why the helper cannot be asked now, if it cannot.
    pub(crate) unavailable: Option<String>,
}

impl DeviceFacts {
    fn known(&self, path: &str) -> Option<&ProjectFacts> {
        match self.facts.get(path) {
            Some(Fact::Known(facts)) => Some(facts),
            _ => None,
        }
    }
}

/// The directory a tab stands for: its first pane's, else its checkout's.
fn tab_context<'a>(tab: &'a TabSnapshot, checkout: &'a CheckoutSnapshot) -> &'a str {
    tab.panes
        .first()
        .map(|pane| pane.cwd.as_str())
        .filter(|cwd| !cwd.trim().is_empty())
        .unwrap_or(checkout.path.as_str())
}

/// Every directory the grouping needs the helper's facts for.
pub(crate) fn needed_paths(raw: &RemoteSessionSnapshot) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for checkout in raw
        .workspaces
        .iter()
        .flat_map(|workspace| &workspace.checkouts)
    {
        if !checkout.path.trim().is_empty() {
            paths.insert(checkout.path.clone());
        }
        for tab in &checkout.tabs {
            let context = tab_context(tab, checkout);
            if !context.trim().is_empty() {
                paths.insert(context.to_owned());
            }
        }
    }
    paths
}

pub(crate) fn catalog_state(
    raw: &RemoteSessionSnapshot,
    facts: &DeviceFacts,
) -> DeviceCatalogSnapshot {
    let needed = needed_paths(raw);
    let refused = needed
        .iter()
        .filter_map(|path| match facts.facts.get(path) {
            Some(Fact::Refused(message)) => Some(DeviceCatalogRefusal {
                path: path.clone(),
                message: message.clone(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    let missing = needed.iter().any(|path| !facts.facts.contains_key(path));
    let (state, message) = match (missing, &facts.unavailable) {
        (false, _) => ("ready", None),
        (true, Some(reason)) => ("unavailable", Some(reason.clone())),
        (true, None) => ("resolving", None),
    };
    DeviceCatalogSnapshot {
        state: state.to_owned(),
        message,
        refused,
    }
}

pub(crate) fn project_id(target: &str, root: &Path) -> String {
    format!("remote:{target}:{}", hide_project::project_id(target, root))
}

/// The Herdr workspace a device checkout id names.
pub(crate) fn remote_checkout_source_id<'a>(target: &str, checkout_id: &'a str) -> Option<&'a str> {
    checkout_id
        .strip_prefix(&format!("remote:{target}:checkout:"))
        .map(|rest| {
            rest.split_once('#')
                .map_or(rest, |(workspace, _)| workspace)
        })
        .filter(|workspace| !workspace.trim().is_empty())
}

/// The session as the device's facts group it. See the module comment.
pub(crate) fn group(
    target: &str,
    raw: &RemoteSessionSnapshot,
    facts: &DeviceFacts,
) -> RemoteSessionSnapshot {
    let mut projects: Vec<WorkspaceSnapshot> = Vec::new();
    let mut active_tab_ids = BTreeMap::new();
    for raw_workspace in &raw.workspaces {
        let Some(raw_checkout) = raw_workspace.checkouts.first() else {
            projects.push(raw_workspace.clone());
            continue;
        };
        let Some(primary) = facts.known(&raw_checkout.path) else {
            // Unconfirmed: the Herdr workspace stays a row of its own.
            projects.push(raw_workspace.clone());
            if let Some(tab) = raw.active_tab_ids.get(&raw_checkout.id) {
                active_tab_ids.insert(raw_checkout.id.clone(), tab.clone());
            }
            continue;
        };
        // Tabs split by the checkout their directory is in; a tab whose
        // directory has no answer stays with the workspace's own checkout.
        let mut parts: Vec<(&ProjectFacts, Vec<&TabSnapshot>)> = vec![(primary, Vec::new())];
        for tab in &raw_checkout.tabs {
            let facts = facts
                .known(tab_context(tab, raw_checkout))
                .unwrap_or(primary);
            match parts
                .iter_mut()
                .find(|(known, _)| known.checkout_root == facts.checkout_root)
            {
                Some((_, tabs)) => tabs.push(tab),
                None => parts.push((facts, vec![tab])),
            }
        }
        for (index, (facts, tabs)) in parts.into_iter().enumerate() {
            let project_id = project_id(target, &facts.root);
            let checkout_root = facts.checkout_root.to_string_lossy().into_owned();
            let checkout_id = if index == 0 {
                raw_checkout.id.clone()
            } else {
                format!(
                    "{}#{:016x}",
                    raw_checkout.id,
                    crate::workspace::fnv1a(checkout_root.as_bytes())
                )
            };
            let tabs = tabs
                .into_iter()
                .map(|tab| TabSnapshot {
                    workspace_id: Some(project_id.clone()),
                    checkout_id: Some(checkout_id.clone()),
                    ..tab.clone()
                })
                .collect::<Vec<_>>();
            let active_tab_id = raw_checkout
                .active_tab_id
                .clone()
                .filter(|active| tabs.iter().any(|tab| tab.id.as_ref() == Some(active)));
            let herdr_active = raw
                .active_tab_ids
                .get(&raw_checkout.id)
                .filter(|active| tabs.iter().any(|tab| tab.id.as_ref() == Some(*active)))
                .cloned()
                .or_else(|| tabs.first().and_then(|tab| tab.id.clone()));
            if let Some(tab) = herdr_active {
                active_tab_ids.insert(checkout_id.clone(), tab);
            }
            let label = if index == 0 {
                raw_checkout.label.clone()
            } else {
                crate::workspace::checkout_row_label(facts.branch.as_deref(), &facts.checkout_root)
            };
            // The Herdr entries of this checkout's tabs; the runtime adds the
            // file tabs (`join_device_editor_tabs`).
            let strip = raw_checkout
                .strip
                .iter()
                .filter(|entry| {
                    entry.kind == StripTabKind::Herdr
                        && tabs
                            .iter()
                            .any(|tab| tab.id.as_ref() == Some(&entry.source_id))
                })
                .cloned()
                .collect::<Vec<_>>();
            let checkout = CheckoutSnapshot {
                id: checkout_id,
                workspace_id: project_id.clone(),
                label,
                path: checkout_root,
                branch: facts.branch.clone(),
                is_worktree: facts.linked_worktree,
                exists: true,
                has_panes: !tabs.is_empty(),
                purpose: if index == 0 {
                    raw_checkout.purpose.clone()
                } else {
                    None
                },
                active_tab_id,
                strip,
                tabs,
                ..raw_checkout.clone()
            };
            let root = facts.root.to_string_lossy().into_owned();
            let project = match projects.iter().position(|project| project.id == project_id) {
                Some(position) => &mut projects[position],
                None => {
                    let name = facts
                        .root
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| raw_workspace.label.clone());
                    projects.push(WorkspaceSnapshot {
                        id: project_id.clone(),
                        label: name.clone(),
                        path: root.clone(),
                        repo_name: name,
                        is_git: facts.kind == ProjectKind::Git,
                        default_branch: None,
                        session_workspace_ids: Vec::new(),
                        checkouts: Vec::new(),
                        last_activity_unix_ms: None,
                        ..raw_workspace.clone()
                    });
                    projects.last_mut().expect("just pushed")
                }
            };
            for id in &raw_workspace.session_workspace_ids {
                if !project.session_workspace_ids.contains(id) {
                    project.session_workspace_ids.push(id.clone());
                }
            }
            if facts.checkout_root == facts.root {
                project.default_branch = facts.branch.clone();
            }
            project.checkouts.push(checkout);
        }
    }
    // The main worktree leads, as on this machine.
    for project in &mut projects {
        if let Some(index) = project
            .checkouts
            .iter()
            .position(|checkout| checkout.path == project.path)
            && index != 0
        {
            let main = project.checkouts.remove(index);
            project.checkouts.insert(0, main);
        }
    }
    crate::sidebar::sync_checkout_agent_summaries(&mut projects, &raw.agents);
    crate::project_context::sort_projects(&mut projects, &raw.agents);

    let find_tab = |tab_id: &str| {
        projects.iter().find_map(|project| {
            project.checkouts.iter().find_map(|checkout| {
                checkout
                    .tabs
                    .iter()
                    .any(|tab| tab.id.as_deref() == Some(tab_id))
                    .then(|| (project.id.clone(), checkout.id.clone()))
            })
        })
    };
    let find_checkout = |checkout_id: &str| {
        projects.iter().find_map(|project| {
            project
                .checkouts
                .iter()
                .any(|checkout| checkout.id == checkout_id)
                .then(|| (project.id.clone(), checkout_id.to_owned()))
        })
    };
    let focused = raw
        .focused_tab_id
        .as_deref()
        .and_then(find_tab)
        .or_else(|| raw.focused_checkout_id.as_deref().and_then(find_checkout));
    RemoteSessionSnapshot {
        workspaces: projects,
        agents: raw.agents.clone(),
        active_tab_ids,
        focused_workspace_id: focused.as_ref().map(|(project, _)| project.clone()),
        focused_checkout_id: focused.map(|(_, checkout)| checkout),
        focused_tab_id: raw.focused_tab_id.clone(),
        focused_pane_id: raw.focused_pane_id.clone(),
        pane_layouts: raw.pane_layouts.clone(),
    }
}

/// A device's repositories' worktrees as its helper last answered, keyed by
/// main worktree path, and the read in flight.
#[derive(Clone, Debug, Default)]
pub(crate) struct DeviceWorktrees {
    pub projects: BTreeMap<String, crate::model::ProjectWorktreesSnapshot>,
    /// The helper connection the running read belongs to.
    pub in_flight: Option<u64>,
    /// Asked again while a read ran; one more read follows it.
    pub again: bool,
    pub unavailable: Option<String>,
}

/// The Git repositories a device's grouped session shows, by main worktree.
pub(crate) fn git_roots(session: &RemoteSessionSnapshot) -> BTreeSet<String> {
    session
        .workspaces
        .iter()
        .filter(|workspace| workspace.is_git)
        .map(|workspace| workspace.path.clone())
        .collect()
}

/// Carries a device repository's worktree facts onto the rows its session
/// already has. Paths are compared as the device's helper reported them;
/// nothing is resolved on this machine's filesystem, where the same path may
/// name another repository (B2, B7).
pub(crate) fn apply_worktrees(
    project: &mut WorkspaceSnapshot,
    listed: &crate::model::ProjectWorktreesSnapshot,
) {
    let same = |left: &str, right: &str| left.trim_end_matches('/') == right.trim_end_matches('/');
    project.default_branch = listed.default_branch.clone();
    project.branches = listed.branches.clone();
    for row in &mut project.checkouts {
        let Some(worktree) = listed
            .worktrees
            .iter()
            .find(|worktree| same(&worktree.path, &row.path))
        else {
            continue;
        };
        let mut worktree = worktree.clone();
        worktree.pane_count = row.tabs.iter().map(|tab| tab.panes.len()).sum();
        worktree.deletion_gate = crate::worktrees::deletion_gate(
            &worktree,
            worktree.branch == listed.base_branch && listed.base_branch.is_some(),
            worktree.pane_count,
            0,
        );
        row.is_worktree = !worktree.is_main;
        row.exists = !worktree.missing;
        row.dirty = worktree.dirty;
        row.changed_file_count = worktree.changed_file_count;
        row.base_branch = worktree.base_branch.clone();
        row.ahead = worktree.ahead;
        row.behind = worktree.behind;
        row.added_lines = worktree.added_lines;
        row.removed_lines = worktree.removed_lines;
        row.unpushed = worktree.unpushed.clone();
        row.branch = worktree.branch.clone();
        row.worktree = Some(worktree);
    }
}
