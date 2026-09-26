//! Derived project reading order and current checkout context. No I/O or clock.
use std::collections::{HashMap, HashSet};

use crate::model::{
    CheckoutPaneContext, CheckoutSnapshot, InactiveCheckoutGroupSnapshot,
    InactiveProjectGroupSnapshot, NavigatorSnapshot, SidebarAgentSnapshot, UiStateSnapshot,
    WorkspaceSnapshot,
};

const INACTIVE_AFTER_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

/// Wall time and server sequence are separate domains. A sequence is only a
/// tie-breaker, never interpreted as milliseconds. Missing facts remain absent.
#[derive(Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd)]
struct Activity {
    unix_ms: Option<u64>,
    sequence: Option<u64>,
}

fn checkout_activity(
    checkout: &CheckoutSnapshot,
    by_pane: &HashMap<&str, &SidebarAgentSnapshot>,
) -> Activity {
    let mut activity = Activity {
        unix_ms: checkout
            .worktree
            .as_ref()
            .and_then(|worktree| worktree.last_commit_unix_seconds)
            .and_then(|seconds| seconds.checked_mul(1_000)),
        sequence: None,
    };
    for pane in checkout.tabs.iter().flat_map(|tab| &tab.panes) {
        if let Some(agent) = by_pane.get(pane.id.as_str()) {
            // The label plugin's wall timestamp has 13 digits. Its padded
            // sequence is deliberately not interpreted as a date.
            let timestamp = (agent.last_activity.len() == 13)
                .then(|| agent.last_activity.parse::<u64>().ok())
                .flatten();
            activity.unix_ms = activity.unix_ms.max(timestamp);
            activity.sequence = activity.sequence.max(agent.state_change_seq);
        }
    }
    activity
}

pub(crate) fn sort_projects(
    projects: &mut [WorkspaceSnapshot],
    agents: &[SidebarAgentSnapshot],
) -> bool {
    let by_pane: HashMap<_, _> = agents
        .iter()
        .map(|agent| (agent.pane_id.as_str(), agent))
        .collect();
    let before: Vec<_> = projects
        .iter()
        .map(|project| {
            (
                project.id.clone(),
                project.last_activity_unix_ms,
                project
                    .checkouts
                    .iter()
                    .map(|c| c.id.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let mut project_activity = HashMap::new();
    for project in projects.iter_mut() {
        let mut keys = HashMap::new();
        let mut recent = Activity::default();
        for checkout in &project.checkouts {
            let key = checkout_activity(checkout, &by_pane);
            recent = recent.max(key);
            keys.insert(checkout.id.clone(), key);
        }
        project.checkouts.sort_by(|left, right| {
            keys[&right.id]
                .cmp(&keys[&left.id])
                .then_with(|| left.id.cmp(&right.id))
        });
        // The order's own reason, carried to the shell so the row can show it.
        // Only the wall time travels: a server sequence is a tie-breaker, not
        // a date, and rendering it as one would invent recency.
        project.last_activity_unix_ms = recent.unix_ms;
        project_activity.insert(project.id.clone(), recent);
    }
    // A pinned project stays ahead of its device's unpinned ones whatever
    // its activity, so the shell can split the list into the `Pinned`
    // section and the activity list without repeating either key (D-02,
    // D-03). The device is still the first key: pins never cross devices.
    projects.sort_by(|left, right| {
        left.device_id
            .cmp(&right.device_id)
            .then_with(|| right.pinned.cmp(&left.pinned))
            .then_with(|| project_activity[&right.id].cmp(&project_activity[&left.id]))
            .then_with(|| left.id.cmp(&right.id))
    });
    projects
        .iter()
        .zip(before)
        .any(|(project, (id, last_activity_unix_ms, checkouts))| {
            project.id != id
                || project.last_activity_unix_ms != last_activity_unix_ms
                || project.checkouts.iter().map(|c| &c.id).ne(checkouts.iter())
        })
}

fn checkout_has_live_exception(
    checkout: &CheckoutSnapshot,
    focused_checkout_id: Option<&str>,
) -> bool {
    checkout.agent_summary.working > 0
        || checkout.agent_summary.needs_you > 0
        || checkout.dirty
        || checkout
            .unpushed
            .as_ref()
            .is_some_and(|unpushed| unpushed.count > 0)
        || Some(checkout.id.as_str()) == focused_checkout_id
}

fn checkout_is_inactive(
    checkout: &CheckoutSnapshot,
    by_pane: &HashMap<&str, &SidebarAgentSnapshot>,
    focused_checkout_id: Option<&str>,
    now_unix_ms: u64,
) -> bool {
    if checkout_has_live_exception(checkout, focused_checkout_id) {
        return false;
    }
    let merged = checkout
        .worktree
        .as_ref()
        .and_then(|worktree| worktree.merged)
        == Some(true)
        || checkout
            .pull_request
            .as_ref()
            .is_some_and(|pull_request| pull_request.badge.is_settled());
    let stale = checkout_activity(checkout, by_pane)
        .unix_ms
        .is_some_and(|last| now_unix_ms.saturating_sub(last) > INACTIVE_AFTER_MS);
    merged || stale
}

/// Rebuilds core-owned inactive membership without moving the authoritative
/// project or checkout rows. The shell maps these ordered IDs back to the
/// existing rows, while search and focus continue to see the full collections.
pub(crate) fn refresh_inactive_groups(
    navigator: &mut NavigatorSnapshot,
    ui_state: &UiStateSnapshot,
    now_unix_ms: u64,
) -> bool {
    let before_projects = navigator.inactive_projects.clone();
    let before_checkouts = navigator
        .workspaces
        .iter()
        .map(|workspace| (workspace.id.clone(), workspace.inactive_checkouts.clone()))
        .collect::<Vec<_>>();
    let by_pane: HashMap<_, _> = navigator
        .agents
        .iter()
        .map(|agent| (agent.pane_id.as_str(), agent))
        .collect();
    let focused_checkout_id = navigator.focused_checkout_id.as_deref();
    let expanded_checkout_projects = ui_state
        .expanded_inactive_checkout_project_paths
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let expanded_project_devices = ui_state
        .expanded_inactive_project_device_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();

    let mut inactive_projects = Vec::<InactiveProjectGroupSnapshot>::new();
    for workspace in &mut navigator.workspaces {
        let inactive_checkout_ids = workspace
            .checkouts
            .iter()
            .filter(|checkout| {
                let is_primary = !checkout.is_worktree && checkout.path == workspace.path;
                !is_primary
                    && checkout_is_inactive(checkout, &by_pane, focused_checkout_id, now_unix_ms)
            })
            .map(|checkout| checkout.id.clone())
            .collect();
        workspace.inactive_checkouts = InactiveCheckoutGroupSnapshot {
            expanded: expanded_checkout_projects.contains(workspace.path.as_str()),
            checkout_ids: inactive_checkout_ids,
        };

        // A pinned project is exempt from the device fold: the operator asked
        // to see it whatever its activity (D-04). Its own checkouts still
        // fold above.
        let project_is_inactive = !workspace.pinned
            && !workspace.checkouts.is_empty()
            && workspace.checkouts.iter().all(|checkout| {
                checkout_is_inactive(checkout, &by_pane, focused_checkout_id, now_unix_ms)
            });
        if project_is_inactive {
            if let Some(group) = inactive_projects
                .iter_mut()
                .find(|group| group.device_id == workspace.device_id)
            {
                group.project_ids.push(workspace.id.clone());
            } else {
                inactive_projects.push(InactiveProjectGroupSnapshot {
                    device_id: workspace.device_id.clone(),
                    expanded: expanded_project_devices.contains(workspace.device_id.as_str()),
                    project_ids: vec![workspace.id.clone()],
                });
            }
        }
    }
    navigator.inactive_projects = inactive_projects;

    navigator.inactive_projects != before_projects
        || navigator
            .workspaces
            .iter()
            .zip(before_checkouts)
            .any(|(workspace, (id, group))| {
                workspace.id != id || workspace.inactive_checkouts != group
            })
}

pub(crate) fn checkout_panes(
    checkout: &CheckoutSnapshot,
    projects: &[WorkspaceSnapshot],
    agents: &[SidebarAgentSnapshot],
) -> Vec<CheckoutPaneContext> {
    let live: HashSet<_> = projects
        .iter()
        .flat_map(|w| &w.checkouts)
        .flat_map(|c| &c.tabs)
        .flat_map(|t| &t.panes)
        .map(|p| p.id.as_str())
        .collect();
    let agents: HashMap<_, _> = agents.iter().map(|a| (a.pane_id.as_str(), a)).collect();
    checkout
        .tabs
        .iter()
        .flat_map(|tab| {
            tab.panes.iter().map(|pane| {
                let agent = agents.get(pane.id.as_str());
                CheckoutPaneContext {
                    pane_id: pane.id.clone(),
                    title: pane
                        .herdr_label
                        .as_ref()
                        .or(pane.terminal_title.as_ref())
                        .or(pane.identity_label.as_ref())
                        .cloned()
                        .unwrap_or_else(|| pane.id.clone()),
                    status: pane.status_label.clone(),
                    session_id: agent.and_then(|a| a.session_id.clone()),
                    parent_pane_id: agent
                        .and_then(|a| a.spawned_from_pane_id.as_ref())
                        .filter(|id| live.contains(id.as_str()))
                        .cloned(),
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::model::{
        CoreOptions, NavigatorSnapshot, PaneSnapshot, PullRequestBadge, PullRequestSnapshot,
        SCHEMA_VERSION, Snapshot, TabSnapshot, UnpushedSnapshot, WorktreeSnapshot,
    };

    /// A project row is a project id, the commit time of its one checkout, and
    /// the panes an agent may be running in. Everything else the ordering does
    /// not read stays at its default.
    fn project(
        id: &str,
        device_id: &str,
        last_commit_unix_seconds: Option<u64>,
        pane_ids: &[&str],
    ) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            home_issues: Default::default(),
            id: id.to_owned(),
            label: id.to_owned(),
            path: format!("/fixture/{id}"),
            remote_target_id: None,
            expanded: true,
            device_id: device_id.to_owned(),
            repo_name: id.to_owned(),
            is_git: true,
            default_branch: None,
            branches: Vec::new(),
            registered: true,
            temporary: false,
            session_workspace_ids: Vec::new(),
            last_activity_unix_ms: None,
            checkouts: vec![checkout(id, last_commit_unix_seconds, pane_ids)],
            pinned: false,
            inactive_checkouts: InactiveCheckoutGroupSnapshot::default(),
            removal: Default::default(),
        }
    }

    fn checkout(
        id: &str,
        last_commit_unix_seconds: Option<u64>,
        pane_ids: &[&str],
    ) -> CheckoutSnapshot {
        CheckoutSnapshot {
            id: id.to_owned(),
            worktree: last_commit_unix_seconds.map(|seconds| WorktreeSnapshot {
                last_commit_unix_seconds: Some(seconds),
                ..Default::default()
            }),
            tabs: vec![TabSnapshot {
                id: None,
                workspace_id: None,
                checkout_id: None,
                label: None,
                empty: pane_ids.is_empty(),
                delegated: false,
                panes: pane_ids
                    .iter()
                    .map(|pane_id| PaneSnapshot {
                        id: (*pane_id).to_owned(),
                        content: Default::default(),
                        herdr_label: None,
                        terminal_title: None,
                        workspace_label: None,
                        cwd: "/fixture".to_owned(),
                        status_label: "Unknown".to_owned(),
                        requires_close_confirmation: false,
                        requires_close_status_check: false,
                        identity_label: None,
                        activity_at_unix_ms: None,
                        fork: Default::default(),
                        ports: vec![],
                        children: None,
                        lineage_path: Vec::new(),
                    })
                    .collect(),
            }],
            ..Default::default()
        }
    }

    /// Agents come through the real projection rather than a handwritten
    /// struct, so the tests read the same 13-digit activity token the sidebar
    /// validates.
    fn agents(rows: Value) -> Vec<SidebarAgentSnapshot> {
        crate::sidebar::project_agents(
            serde_json::from_value(json!({ "agents": rows })).expect("valid fixture"),
        )
        .agents
    }

    fn ids(projects: &[WorkspaceSnapshot]) -> Vec<&str> {
        projects.iter().map(|p| p.id.as_str()).collect()
    }

    fn navigator(workspaces: Vec<WorkspaceSnapshot>) -> NavigatorSnapshot {
        let mut navigator = Snapshot::initial(&CoreOptions {
            schema_version: SCHEMA_VERSION,
            herdr_socket_path: None,
            herdr_bin_path: None,
            app_state_path: "/tmp/hide-project-context-test-state.json".to_owned(),
            host_helper_dir: None,
            host_helper_root: None,
            workspace_views_path: None,
            shortcut_import_path: None,
        })
        .navigator;
        navigator.workspaces = workspaces;
        navigator
    }

    fn make_worktree_checkout(
        workspace_id: &str,
        id: &str,
        last_commit_unix_seconds: Option<u64>,
    ) -> CheckoutSnapshot {
        CheckoutSnapshot {
            id: id.to_owned(),
            workspace_id: workspace_id.to_owned(),
            label: id.to_owned(),
            path: format!("/fixture/{workspace_id}/{id}"),
            is_worktree: true,
            worktree: Some(WorktreeSnapshot {
                last_commit_unix_seconds,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn settled_pull_request(badge: PullRequestBadge) -> PullRequestSnapshot {
        PullRequestSnapshot {
            closing_issues: Default::default(),
            title: "Settled".to_owned(),
            checks: Default::default(),
            number: 1,
            head_branch: "topic".to_owned(),
            base_branch: "main".to_owned(),
            url: "https://example.invalid/pull/1".to_owned(),
            badge,
            review: None,
            is_draft: false,
            merged_at_unix_ms: None,
            updated_at_unix_ms: None,
        }
    }

    /// B1, B2. The newest activity in a project comes first inside its device
    /// group, whether that activity is a commit or an agent, and the carried
    /// timestamp is the same one the order was decided by.
    #[test]
    fn projects_order_by_newest_activity_within_a_device() {
        let mut projects = vec![
            project("stale", "local", Some(1_000), &[]),
            project("committed", "local", Some(3_000), &[]),
            project("agent", "local", Some(2_000), &["pane-agent"]),
        ];
        let agents = agents(json!([
            {"pane_id": "pane-agent", "state_change_seq": 1, "tokens": {"activity": "0000004000000"}},
        ]));

        assert!(sort_projects(&mut projects, &agents));

        assert_eq!(ids(&projects), ["agent", "committed", "stale"]);
        assert_eq!(projects[0].last_activity_unix_ms, Some(4_000_000));
        assert_eq!(projects[1].last_activity_unix_ms, Some(3_000_000));
        assert_eq!(projects[2].last_activity_unix_ms, Some(1_000_000));
    }

    /// B1. Equal activity keeps the existing order, which is the project id.
    /// A tie is not an invitation to shuffle the list on every projection.
    #[test]
    fn equal_activity_falls_back_to_the_project_id() {
        let mut projects = vec![
            project("beta", "local", Some(2_000), &[]),
            project("alpha", "local", Some(2_000), &[]),
        ];

        assert!(sort_projects(&mut projects, &[]));
        assert_eq!(ids(&projects), ["alpha", "beta"]);

        // Already ordered: the same input reports no change, so an idle tick
        // publishes nothing.
        assert!(!sort_projects(&mut projects, &[]));
    }

    /// B1, B4. A project with neither a commit nor an agent has no activity to
    /// claim. It sorts below every project that has one and carries no
    /// timestamp, which is what lets the row leave its time blank.
    #[test]
    fn projects_without_activity_sort_last_and_carry_no_time() {
        let mut projects = vec![
            project("quiet", "local", None, &[]),
            project("active", "local", Some(5_000), &[]),
        ];

        assert!(sort_projects(&mut projects, &[]));

        assert_eq!(ids(&projects), ["active", "quiet"]);
        assert_eq!(projects[0].last_activity_unix_ms, Some(5_000_000));
        assert_eq!(projects[1].last_activity_unix_ms, None);
    }

    /// B2, B6, B9, B16. Git merge, settled PR, and the seven-day boundary
    /// decide membership. The primary stays outside its project's checkout
    /// fold, and a checkout with no known activity is not guessed stale.
    #[test]
    fn inactive_checkouts_cover_settled_stale_boundary_primary_and_unknown_activity() {
        let now_ms = 20 * 24 * 60 * 60 * 1_000;
        let old_seconds = (now_ms - (7 * 24 + 1) * 60 * 60 * 1_000) / 1_000;
        let recent_seconds = (now_ms - (6 * 24 + 23) * 60 * 60 * 1_000) / 1_000;
        let mut project = project("alpha", "local", Some(old_seconds), &[]);
        project.checkouts[0].path = project.path.clone();
        project.checkouts[0].worktree.as_mut().unwrap().merged = Some(true);

        let mut merged = make_worktree_checkout("alpha", "merged", Some(recent_seconds));
        merged.worktree.as_mut().unwrap().merged = Some(true);
        let stale = make_worktree_checkout("alpha", "stale", Some(old_seconds));
        let recent = make_worktree_checkout("alpha", "recent", Some(recent_seconds));
        let unknown = make_worktree_checkout("alpha", "unknown", None);
        let mut closed = make_worktree_checkout("alpha", "closed", Some(recent_seconds));
        closed.pull_request = Some(settled_pull_request(PullRequestBadge::Closed));
        project
            .checkouts
            .extend([merged, stale, recent, unknown, closed]);
        let mut navigator = navigator(vec![project]);

        assert!(refresh_inactive_groups(
            &mut navigator,
            &UiStateSnapshot::default(),
            now_ms
        ));

        assert_eq!(
            navigator.workspaces[0].inactive_checkouts.checkout_ids,
            ["merged", "stale", "closed"]
        );
        assert!(
            !navigator.workspaces[0]
                .inactive_checkouts
                .checkout_ids
                .contains(&"alpha".to_owned()),
            "the primary never folds inside its project"
        );
        assert!(navigator.inactive_projects.is_empty());
    }

    /// D-06, B13. A recent agent timestamp keeps a checkout active even when
    /// its commit is old; the predicate uses the same newest-activity answer
    /// as project ordering.
    #[test]
    fn recent_agent_activity_overrides_an_old_commit() {
        let now_ms = 20 * 24 * 60 * 60 * 1_000;
        let old_seconds = (now_ms - 8 * 24 * 60 * 60 * 1_000) / 1_000;
        let recent_ms = now_ms - 2 * 24 * 60 * 60 * 1_000;
        let mut project = project("alpha", "local", Some(old_seconds), &[]);
        project.checkouts[0].path = project.path.clone();
        project.checkouts.push(make_worktree_checkout(
            "alpha",
            "recent-agent",
            Some(old_seconds),
        ));
        project.checkouts[1].tabs = checkout("ignored", None, &["pane-active"]).tabs;
        let mut navigator = navigator(vec![project]);
        navigator.agents = agents(json!([
            {"pane_id": "pane-active", "state_change_seq": 1, "tokens": {"activity": format!("{recent_ms:013}")}},
        ]));

        refresh_inactive_groups(&mut navigator, &UiStateSnapshot::default(), now_ms);

        assert!(
            navigator.workspaces[0]
                .inactive_checkouts
                .checkout_ids
                .is_empty()
        );
    }

    /// B3, B7. Every live-work exception wins over a settled branch, including
    /// the operator's current selection.
    #[test]
    fn live_work_and_focus_exceptions_stay_visible() {
        let now_ms = 20 * 24 * 60 * 60 * 1_000;
        let mut project = project("alpha", "local", None, &[]);
        let ids = [
            "working",
            "needs-you",
            "dirty",
            "unpushed",
            "focused",
            "control",
        ];
        for id in ids {
            let mut checkout = make_worktree_checkout("alpha", id, None);
            checkout.worktree.as_mut().unwrap().merged = Some(true);
            project.checkouts.push(checkout);
        }
        project.checkouts[1].agent_summary.working = 1;
        project.checkouts[2].agent_summary.needs_you = 1;
        project.checkouts[3].dirty = true;
        project.checkouts[4].unpushed = Some(UnpushedSnapshot {
            remote: "origin".to_owned(),
            count: 1,
        });
        let mut navigator = navigator(vec![project]);
        navigator.focused_checkout_id = Some("focused".to_owned());

        refresh_inactive_groups(&mut navigator, &UiStateSnapshot::default(), now_ms);

        assert_eq!(
            navigator.workspaces[0].inactive_checkouts.checkout_ids,
            ["control"]
        );
    }

    /// B11, B12, B16. A primary can make its whole project inactive even
    /// though it never enters the checkout fold. Device groups preserve the
    /// already sorted project order and read independent persisted expansion.
    #[test]
    fn fully_inactive_projects_group_by_device_in_existing_order() {
        let now_ms = 20 * 24 * 60 * 60 * 1_000;
        let old_seconds = (now_ms - 8 * 24 * 60 * 60 * 1_000) / 1_000;
        let active = project("active", "local", Some(now_ms / 1_000), &[]);
        let mut local_one = project("local-one", "local", Some(old_seconds), &[]);
        local_one.checkouts[0].path = local_one.path.clone();
        let mut local_two = project("local-two", "local", Some(old_seconds), &[]);
        local_two.checkouts[0].path = local_two.path.clone();
        let mut remote = project("remote", "mini", Some(old_seconds), &[]);
        remote.checkouts[0].path = remote.path.clone();
        let mut navigator = navigator(vec![active, local_one, local_two, remote]);
        let ui_state = UiStateSnapshot {
            expanded_inactive_checkout_project_paths: vec!["/fixture/local-one".to_owned()],
            expanded_inactive_project_device_ids: vec!["local".to_owned()],
            ..UiStateSnapshot::default()
        };

        refresh_inactive_groups(&mut navigator, &ui_state, now_ms);

        assert_eq!(
            ids(&navigator.workspaces),
            ["active", "local-one", "local-two", "remote"]
        );
        assert_eq!(navigator.inactive_projects.len(), 2);
        assert_eq!(
            navigator.inactive_projects[0].project_ids,
            ["local-one", "local-two"]
        );
        assert!(navigator.inactive_projects[0].expanded);
        assert_eq!(navigator.inactive_projects[1].project_ids, ["remote"]);
        assert!(!navigator.inactive_projects[1].expanded);
        assert!(navigator.workspaces[1].inactive_checkouts.expanded);
        assert!(
            navigator.workspaces[1]
                .inactive_checkouts
                .checkout_ids
                .is_empty(),
            "the primary contributes only to the project-level fold"
        );
    }

    /// B1. Device groups stay whole: activity orders projects inside a device,
    /// never across two of them.
    #[test]
    fn device_groups_are_ordered_before_activity() {
        let mut projects = vec![
            project("remote-old", "mini", Some(1_000), &[]),
            project("local-old", "local", Some(2_000), &[]),
            project("remote-new", "mini", Some(9_000), &[]),
            project("local-new", "local", Some(8_000), &[]),
        ];

        sort_projects(&mut projects, &[]);

        assert_eq!(
            ids(&projects),
            ["local-new", "local-old", "remote-new", "remote-old"]
        );
    }

    /// B2, B3. A pinned project leads its device whatever its activity, pins
    /// keep the activity order among themselves, and a remote pin still sits
    /// after every local row because the device is the first key.
    #[test]
    fn pinned_projects_lead_their_device_in_activity_order() {
        let mut pinned_stale = project("pinned-stale", "local", Some(1_000), &[]);
        pinned_stale.pinned = true;
        let mut pinned_busy = project("pinned-busy", "local", Some(2_000), &[]);
        pinned_busy.pinned = true;
        let mut remote_pinned = project("remote-pinned", "mini", Some(9_000), &[]);
        remote_pinned.pinned = true;
        let mut projects = vec![
            project("active", "local", Some(8_000), &[]),
            pinned_stale,
            remote_pinned,
            project("remote", "mini", Some(9_500), &[]),
            pinned_busy,
        ];

        assert!(sort_projects(&mut projects, &[]));

        assert_eq!(
            ids(&projects),
            [
                "pinned-busy",
                "pinned-stale",
                "active",
                "remote-pinned",
                "remote"
            ]
        );
    }

    /// B6. A pinned project whose every checkout is inactive stays out of the
    /// device fold; its own inactive worktrees still fold inside it.
    #[test]
    fn pinned_projects_stay_out_of_the_device_fold() {
        let now_ms = 20 * 24 * 60 * 60 * 1_000;
        let old_seconds = (now_ms - 8 * 24 * 60 * 60 * 1_000) / 1_000;
        let mut pinned = project("pinned", "local", Some(old_seconds), &[]);
        pinned.pinned = true;
        pinned.checkouts[0].path = pinned.path.clone();
        pinned.checkouts.push(make_worktree_checkout(
            "pinned",
            "old-topic",
            Some(old_seconds),
        ));
        let mut folded = project("folded", "local", Some(old_seconds), &[]);
        folded.checkouts[0].path = folded.path.clone();
        let mut navigator = navigator(vec![pinned, folded]);

        refresh_inactive_groups(&mut navigator, &UiStateSnapshot::default(), now_ms);

        assert_eq!(navigator.inactive_projects.len(), 1);
        assert_eq!(navigator.inactive_projects[0].project_ids, ["folded"]);
        assert_eq!(
            navigator.workspaces[0].inactive_checkouts.checkout_ids,
            ["old-topic"]
        );
    }

    /// B2. A pane's agent moving is enough to raise its project, and the move
    /// is reported so the snapshot publishes.
    #[test]
    fn new_agent_activity_raises_its_project_and_reports_the_change() {
        let mut projects = vec![
            project("busy", "local", Some(9_000), &[]),
            project("waking", "local", Some(1_000), &["pane-waking"]),
        ];
        sort_projects(&mut projects, &[]);
        assert_eq!(ids(&projects), ["busy", "waking"]);

        let awake = agents(json!([
            {"pane_id": "pane-waking", "state_change_seq": 2, "tokens": {"activity": "0000010000000"}},
        ]));

        assert!(sort_projects(&mut projects, &awake));
        assert_eq!(ids(&projects), ["waking", "busy"]);
        assert_eq!(projects[0].last_activity_unix_ms, Some(10_000_000));
    }
}
