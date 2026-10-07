//! Per-surface scope membership and counts. Physical groups, root headings
//! and request rows deliberately have different membership.
use crate::agent_state::RequestVerb;
use crate::model::{MarkCountsSnapshot, SidebarAgentSnapshot, WorkspaceSnapshot};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct GroupCounts {
    pub needs_you: usize,
    pub done: usize,
    pub working: usize,
    pub seen: usize,
}
impl GroupCounts {
    fn add(&mut self, group: &str) {
        match group {
            "needs_you" => self.needs_you += 1,
            "done" => self.done += 1,
            "working" => self.working += 1,
            "seen" => self.seen += 1,
            _ => {} // An unfamiliar group is still a named section below.
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GroupRows {
    pub group: String,
    pub pane_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Member {
    pub pane_id: String,
    pub project_id: String,
    pub checkout_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TreeRow {
    pub pane_id: String,
    pub depth: usize,
    pub descendants: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Section {
    pub group: String,
    pub rows: Vec<TreeRow>,
    pub count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RequestRow {
    /// Index into this scope's members; a pane can belong to two projects.
    pub member: usize,
    pub children: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RequestGroup {
    pub verb: RequestVerb,
    /// Index into this scope's request rows, already in display order.
    pub rows: Vec<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Requests {
    pub rows: Vec<RequestRow>,
    pub groups: Vec<RequestGroup>,
    pub counts: BTreeMap<RequestVerb, usize>,
    pub todo: usize,
    pub answer: usize,
}

impl Default for Requests {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            groups: Vec::new(),
            counts: VERBS.into_iter().map(|verb| (verb, 0)).collect(),
            todo: 0,
            answer: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Buckets {
    pub turn: usize,
    pub working: usize,
    pub delegating: usize,
    pub resting: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct TurnCounts {
    pub question: usize,
    pub approval: usize,
    pub error: usize,
    pub done: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Raised {
    pub group: String,
    pub shown: Vec<String>,
    pub more: Vec<String>,
}

/// A scope carries both physical totals and Overview membership. The latter
/// uses the first checkout owner and deduplicates inside each project, as the
/// former web scope did; the checkout badge still uses its last-owner tally.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Scope {
    pub prs: crate::agent_state::work::board::Board,
    pub raised: Vec<Raised>,
    pub owners: BTreeMap<String, String>,
    pub badge_total: usize,
    pub pane_ids: Vec<String>,
    pub total: usize,
    pub overview_total: usize,
    pub roots: Vec<String>,
    pub groups: GroupCounts,
    pub group_rows: Vec<GroupRows>,
    pub descendants: BTreeMap<String, usize>,
    pub children: BTreeMap<String, Vec<String>>,
    pub marks: MarkCountsSnapshot,
    pub sections: Vec<Section>,
    pub members: Vec<Member>,
    pub buckets: Buckets,
    pub turns: TurnCounts,
    pub requests: Requests,
    pub folded: BTreeMap<String, super::lineage::Folded>,
    pub tree: super::lineage::Tree,
    pub global_tree: super::lineage::Tree,
}

const GROUPS: [&str; 4] = ["needs_you", "done", "working", "seen"];
const VERBS: [RequestVerb; 8] = [
    RequestVerb::Answer,
    RequestVerb::Fix,
    RequestVerb::Review,
    RequestVerb::Stopped,
    RequestVerb::Result,
    RequestVerb::Working,
    RequestVerb::Waiting,
    RequestVerb::Idle,
];

pub(super) fn members(
    projects: &[&WorkspaceSnapshot],
    agents: &[&SidebarAgentSnapshot],
) -> Vec<Member> {
    let mut result = Vec::new();
    for project in projects {
        let mut owners = HashMap::new();
        for checkout in &project.checkouts {
            for pane in checkout.tabs.iter().flat_map(|tab| &tab.panes) {
                owners
                    .entry(pane.id.as_str())
                    .or_insert(checkout.id.as_str());
            }
        }
        let mut seen = HashSet::new();
        for agent in agents {
            if let Some(checkout) = owners.get(agent.pane_id.as_str())
                && seen.insert(agent.pane_id.as_str())
            {
                result.push(Member {
                    pane_id: agent.pane_id.clone(),
                    project_id: project.id.clone(),
                    checkout_id: (*checkout).to_owned(),
                });
            }
        }
    }
    result
}

pub(super) fn project_marks(projects: &[&WorkspaceSnapshot]) -> MarkCountsSnapshot {
    let mut counts = MarkCountsSnapshot::default();
    for marks in projects
        .iter()
        .flat_map(|p| &p.checkouts)
        .map(|c| c.agent_summary.marks)
    {
        counts.error += marks.error;
        counts.approval += marks.approval;
        counts.question += marks.question;
        counts.working += marks.working;
        counts.done += marks.done;
        counts.idle += marks.idle;
    }
    counts
}

/// A BFS over rows this particular surface actually holds. A disconnected
/// device's retained row is not a descendant of its connected-device list.
pub(super) fn descendants<'a>(
    agent: &SidebarAgentSnapshot,
    rows: &HashMap<&str, &'a SidebarAgentSnapshot>,
) -> Vec<&'a SidebarAgentSnapshot> {
    let mut seen = HashSet::new();
    let mut queue = std::collections::VecDeque::from(agent.lineage_child_pane_ids.clone());
    let mut result = Vec::new();
    while let Some(id) = queue.pop_front() {
        if id == agent.pane_id || !seen.insert(id.clone()) {
            continue;
        }
        let Some(row) = rows.get(id.as_str()) else {
            continue;
        };
        result.push(*row);
        queue.extend(row.lineage_child_pane_ids.iter().cloned());
    }
    result
}

fn sections(agents: &[&SidebarAgentSnapshot]) -> Vec<Section> {
    let by_pane: HashMap<_, _> = agents
        .iter()
        .map(|row| (row.pane_id.as_str(), *row))
        .collect();
    let mut groups: Vec<&str> = GROUPS.into();
    for row in agents {
        if !groups.contains(&row.group.as_str()) {
            groups.push(&row.group);
        }
    }
    fn visit(
        agent: &SidebarAgentSnapshot,
        depth: usize,
        by_pane: &HashMap<&str, &SidebarAgentSnapshot>,
        seen: &mut HashSet<String>,
        rows: &mut Vec<TreeRow>,
    ) {
        if !seen.insert(agent.pane_id.clone()) {
            return;
        }
        rows.push(TreeRow {
            pane_id: agent.pane_id.clone(),
            depth,
            descendants: descendants(agent, by_pane).len(),
        });
        if agent.lineage_collapsed {
            return;
        }
        for id in &agent.lineage_child_pane_ids {
            if let Some(child) = by_pane.get(id.as_str()) {
                visit(child, depth + 1, by_pane, seen, rows);
            }
        }
    }
    groups
        .into_iter()
        .filter_map(|group| {
            let roots: Vec<_> = agents
                .iter()
                .filter(|row| row.state.root && row.group == group)
                .collect();
            if roots.is_empty() {
                return None;
            }
            let mut rows = Vec::new();
            let mut count = 0;
            for root in roots {
                count += 1 + descendants(root, &by_pane).len();
                visit(root, 0, &by_pane, &mut HashSet::new(), &mut rows);
            }
            Some(Section {
                group: group.to_owned(),
                rows,
                count,
            })
        })
        .collect()
}

pub(super) fn scope(
    physical: &[&SidebarAgentSnapshot],
    members: Vec<Member>,
    all: &[&SidebarAgentSnapshot],
    marks: MarkCountsSnapshot,
) -> Scope {
    let by_pane: HashMap<_, _> = all.iter().map(|row| (row.pane_id.as_str(), *row)).collect();
    let in_scope: HashSet<_> = members
        .iter()
        .map(|member| member.pane_id.as_str())
        .collect();
    let mut value = Scope {
        marks,
        sections: sections(physical),
        ..Scope::default()
    };
    for row in physical {
        value.pane_ids.push(row.pane_id.clone());
        value.groups.add(&row.group);
        if row.state.root {
            value.roots.push(row.pane_id.clone());
        }
    }
    let mut groups: Vec<&str> = GROUPS.into();
    for row in physical {
        if !groups.contains(&row.group.as_str()) {
            groups.push(&row.group);
        }
    }
    value.group_rows = groups
        .into_iter()
        .filter_map(|group| {
            let pane_ids: Vec<_> = physical
                .iter()
                .filter(|row| row.group == group)
                .map(|row| row.pane_id.clone())
                .collect();
            if pane_ids.is_empty() {
                None
            } else {
                Some(GroupRows {
                    group: group.to_owned(),
                    pane_ids,
                })
            }
        })
        .collect();
    let physical_index: HashMap<_, _> = physical
        .iter()
        .map(|row| (row.pane_id.as_str(), *row))
        .collect();
    value.descendants = physical
        .iter()
        .map(|row| (row.pane_id.clone(), descendants(row, &physical_index).len()))
        .collect();
    value.children = physical
        .iter()
        .map(|row| {
            (
                row.pane_id.clone(),
                row.lineage_child_pane_ids
                    .iter()
                    .filter(|id| physical_index.contains_key(id.as_str()))
                    .cloned()
                    .collect(),
            )
        })
        .collect();
    for (member, place) in members.iter().enumerate() {
        let row = by_pane[place.pane_id.as_str()];
        match row.state.bucket {
            "turn" => value.buckets.turn += 1,
            "working" => value.buckets.working += 1,
            "delegating" => value.buckets.delegating += 1,
            "resting" => value.buckets.resting += 1,
            other => unreachable!("core emitted unknown bucket {other}"),
        }
        if row.group == "done" {
            value.turns.done += 1;
        }
        if row.state.needs_you {
            match row.demand.as_str() {
                "question" => value.turns.question += 1,
                "approval" => value.turns.approval += 1,
                "error" => value.turns.error += 1,
                _ => {}
            }
        }
        if row.delegated
            && row
                .lineage_parent_pane_id
                .as_deref()
                .is_some_and(|parent| in_scope.contains(parent))
        {
            continue;
        }
        value.requests.rows.push(RequestRow {
            member,
            children: row
                .close_descendant_pane_ids
                .iter()
                .filter(|id| by_pane.contains_key(id.as_str()))
                .rev()
                .cloned()
                .collect(),
        });
    }
    for verb in VERBS {
        let row_of =
            |index: usize| by_pane[members[value.requests.rows[index].member].pane_id.as_str()];
        let mut indices: Vec<_> = (0..value.requests.rows.len())
            .filter(|&index| row_of(index).state.verb == verb)
            .collect();
        let todo = matches!(
            verb,
            RequestVerb::Answer
                | RequestVerb::Fix
                | RequestVerb::Review
                | RequestVerb::Stopped
                | RequestVerb::Result
        );
        indices.sort_by(|&a, &b| {
            let (a, b) = (row_of(a), row_of(b));
            if todo {
                // The old client used MAX_SAFE_INTEGER for a missing block.
                a.request
                    .as_ref()
                    .map_or(9_007_199_254_740_991, |r| r.verb_since_unix_ms)
                    .cmp(
                        &b.request
                            .as_ref()
                            .map_or(9_007_199_254_740_991, |r| r.verb_since_unix_ms),
                    )
            } else {
                b.last_activity.cmp(&a.last_activity)
            }
        });
        value.requests.counts.insert(verb, indices.len());
        if todo {
            value.requests.todo += indices.len();
        }
        if verb == RequestVerb::Answer {
            value.requests.answer = indices.len();
        }
        if !indices.is_empty() {
            value.requests.groups.push(RequestGroup {
                verb,
                rows: indices,
            });
        }
    }
    value.total = physical.len();
    value.overview_total = members.len();
    value.members = members;
    value
}

#[derive(Clone, Debug, PartialEq)]
struct PlaceInput {
    device: String,
    project: String,
    home: bool,
    pull_requests: Vec<crate::model::PullRequestSnapshot>,
    tasks: Vec<crate::tasks::TaskSnapshot>,
    repository: Option<String>,
    checkout_work: Vec<(Option<String>, bool, bool, Option<String>)>,
    checkout_labels: Vec<(String, Option<String>, Option<u32>)>,
    checkouts: Vec<(
        String,
        MarkCountsSnapshot,
        Vec<(Option<String>, Vec<String>)>,
    )>,
}

#[derive(Clone, Debug, PartialEq)]
struct DeviceInput {
    id: String,
    label: String,
    connected: bool,
    agents: Vec<SidebarAgentSnapshot>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Input {
    places: Vec<PlaceInput>,
    devices: Vec<DeviceInput>,
}

/// One owned comparison input, bounded by the snapshot's agents and catalog.
/// Terminal output, elapsed clocks and unrelated UI changes never enter it.
#[derive(Default)]
pub(crate) struct Cache {
    previous: Option<Input>,
    output: Output,
}

#[derive(Default)]
struct Output {
    projects: HashMap<(String, String), Scope>,
    checkouts: HashMap<(String, String), Scope>,
    devices: HashMap<String, Scope>,
    overall: Scope,
}

struct DeviceRows<'a> {
    id: &'a str,
    label: &'a str,
    connected: bool,
    projects: Vec<&'a WorkspaceSnapshot>,
    agents: Vec<&'a SidebarAgentSnapshot>,
}

fn device_rows(snapshot: &crate::model::Snapshot) -> Vec<DeviceRows<'_>> {
    let local = snapshot
        .navigator
        .devices
        .iter()
        .find(|d| d.kind != "remote");
    let mut devices = vec![DeviceRows {
        id: local.map_or("", |d| d.id.as_str()),
        label: local.map_or("", |d| d.label.as_str()),
        connected: true,
        projects: snapshot.navigator.workspaces.iter().collect(),
        agents: snapshot.navigator.agents.iter().collect(),
    }];
    for remote in &snapshot.status.remote {
        let device = snapshot
            .navigator
            .devices
            .iter()
            .find(|d| d.id == remote.target_id);
        devices.push(DeviceRows {
            id: &remote.target_id,
            label: device.map_or(remote.target_id.as_str(), |d| d.label.as_str()),
            connected: remote.state == "connected",
            projects: remote
                .session
                .as_ref()
                .map_or_else(Vec::new, |s| s.workspaces.iter().collect()),
            agents: remote
                .session
                .as_ref()
                .map_or_else(Vec::new, |s| s.agents.iter().collect()),
        });
    }
    devices
}

impl Cache {
    pub(crate) fn refresh(&mut self, snapshot: &mut crate::model::Snapshot) -> bool {
        let devices = device_rows(snapshot);
        let input = Input {
            devices: devices
                .iter()
                .map(|d| DeviceInput {
                    id: d.id.to_owned(),
                    label: d.label.to_owned(),
                    connected: d.connected,
                    agents: d.agents.iter().map(|r| (*r).clone()).collect(),
                })
                .collect(),
            places: devices
                .iter()
                .flat_map(|d| &d.projects)
                .map(|p| PlaceInput {
                    device: p.device_id.clone(),
                    project: p.id.clone(),
                    home: p.is_home,
                    pull_requests: p.pull_requests.clone(),
                    tasks: p.tasks.tasks.clone(),
                    repository: p.home_issues.repository.clone(),
                    checkout_work: p
                        .checkouts
                        .iter()
                        .map(|c| {
                            (
                                c.pull_request.as_ref().map(|p| p.url.clone()),
                                c.is_worktree,
                                c.exists,
                                c.task_key.clone(),
                            )
                        })
                        .collect(),
                    checkout_labels: p
                        .checkouts
                        .iter()
                        .map(|c| {
                            (
                                c.label.clone(),
                                c.branch.clone(),
                                c.pull_request.as_ref().map(|p| p.number),
                            )
                        })
                        .collect(),
                    checkouts: p
                        .checkouts
                        .iter()
                        .map(|c| {
                            (
                                c.id.clone(),
                                c.agent_summary.marks,
                                c.tabs
                                    .iter()
                                    .map(|t| {
                                        (
                                            t.id.clone(),
                                            t.panes.iter().map(|p| p.id.clone()).collect(),
                                        )
                                    })
                                    .collect(),
                            )
                        })
                        .collect(),
                })
                .collect(),
        };
        if self.previous.as_ref() == Some(&input) {
            return self.apply(snapshot);
        }
        let mut projects = HashMap::new();
        let mut checkouts = HashMap::new();
        let mut device_scopes = HashMap::new();
        let all: Vec<_> = devices
            .iter()
            .flat_map(|d| d.agents.iter().copied())
            .collect();
        let live: Vec<_> = devices
            .iter()
            .filter(|d| d.connected)
            .flat_map(|d| d.agents.iter().copied())
            .collect();
        let live_projects: Vec<_> = devices
            .iter()
            .filter(|d| d.connected)
            .flat_map(|d| d.projects.iter().copied())
            .collect();
        let places: HashMap<_, _> = devices
            .iter()
            .flat_map(|d| {
                d.agents
                    .iter()
                    .map(move |a| (a.pane_id.as_str(), (d.id, d.label)))
            })
            .collect();
        let mut overall_members = Vec::new();
        let mut overall_projects = Vec::new();
        for device in &devices {
            for project in &device.projects {
                let mut trees = super::lineage::checkout_trees(project, &device.agents);
                let mut global_trees = super::lineage::checkout_trees(project, &live);
                let pane_ids: HashSet<_> = project
                    .checkouts
                    .iter()
                    .flat_map(|c| &c.tabs)
                    .flat_map(|t| &t.panes)
                    .map(|p| p.id.as_str())
                    .collect();
                let physical: Vec<_> = device
                    .agents
                    .iter()
                    .copied()
                    .filter(|r| pane_ids.contains(r.pane_id.as_str()))
                    .collect();
                projects.insert(
                    (project.device_id.clone(), project.id.clone()),
                    scope(
                        &physical,
                        members(&[project], &device.agents),
                        &device.agents,
                        project_marks(&[project]),
                    ),
                );
                projects
                    .get_mut(&(project.device_id.clone(), project.id.clone()))
                    .expect("project scope inserted")
                    .prs =
                    crate::agent_state::work::board::project(project, &device.agents, &trees);
                for checkout in &project.checkouts {
                    let pane_ids: HashSet<_> = checkout
                        .tabs
                        .iter()
                        .flat_map(|t| &t.panes)
                        .map(|p| p.id.as_str())
                        .collect();
                    let physical: Vec<_> = device
                        .agents
                        .iter()
                        .copied()
                        .filter(|r| pane_ids.contains(r.pane_id.as_str()))
                        .collect();
                    let mut seen = HashSet::new();
                    let members = physical
                        .iter()
                        .filter(|r| seen.insert(r.pane_id.as_str()))
                        .map(|r| Member {
                            pane_id: r.pane_id.clone(),
                            project_id: project.id.clone(),
                            checkout_id: checkout.id.clone(),
                        })
                        .collect();
                    checkouts.insert(
                        (project.device_id.clone(), checkout.id.clone()),
                        scope(
                            &physical,
                            members,
                            &device.agents,
                            checkout.agent_summary.marks,
                        ),
                    );
                    let value = checkouts
                        .get_mut(&(project.device_id.clone(), checkout.id.clone()))
                        .expect("checkout scope just inserted");
                    value.badge_total = checkout.agent_summary.needs_you
                        + checkout.agent_summary.done
                        + checkout.agent_summary.working
                        + checkout.agent_summary.seen;
                    value.tree = trees.remove(&checkout.id).expect("checkout tree projected");
                    value.global_tree = global_trees
                        .remove(&checkout.id)
                        .expect("global checkout tree projected");
                }
            }
            let overview: Vec<_> = device
                .projects
                .iter()
                .copied()
                .filter(|p| !p.is_home)
                .collect();
            let scoped_members = members(&overview, &device.agents);
            overall_members.extend(scoped_members.iter().cloned());
            overall_projects.extend(overview.iter().copied());
            let physical = if device.connected {
                device.agents.as_slice()
            } else {
                &[]
            };
            device_scopes.insert(
                device.id.to_owned(),
                scope(
                    physical,
                    scoped_members,
                    &device.agents,
                    project_marks(&device.projects),
                ),
            );
            let mut owners = BTreeMap::new();
            let mut drawn = HashSet::new();
            for project in &device.projects {
                for checkout in &project.checkouts {
                    for pane in checkout.tabs.iter().flat_map(|t| &t.panes) {
                        drawn.insert(pane.id.as_str());
                        if !project.is_home {
                            owners
                                .entry(pane.id.clone())
                                .or_insert_with(|| checkout.id.clone());
                        }
                    }
                }
            }
            let raised = [("needs_you", 5), ("done", 3)]
                .into_iter()
                .filter_map(|(group, cap)| {
                    let ids: Vec<_> = live
                        .iter()
                        .filter(|a| a.group == group && drawn.contains(a.pane_id.as_str()))
                        .map(|a| a.pane_id.clone())
                        .collect();
                    if ids.is_empty() {
                        None
                    } else {
                        Some(Raised {
                            group: group.into(),
                            shown: ids.iter().take(cap).cloned().collect(),
                            more: ids.into_iter().skip(cap).collect(),
                        })
                    }
                })
                .collect();
            let device_scope = device_scopes
                .get_mut(device.id)
                .expect("device scope inserted");
            device_scope.raised = raised;
            device_scope.owners = owners;
            device_scopes
                .get_mut(device.id)
                .expect("device scope just inserted")
                .folded = super::lineage::folded(&device.agents, &live_projects, &places);
        }
        let mut overall = scope(
            &live,
            overall_members,
            &all,
            project_marks(&overall_projects),
        );
        overall.folded = super::lineage::folded(&live, &live_projects, &places);
        self.output = Output {
            projects,
            checkouts,
            devices: device_scopes,
            overall,
        };
        self.previous = Some(input);
        self.apply(snapshot)
    }

    /// Catalog constructors project only facts. Retain the previous derived
    /// values until the complete agent/catalog input has been reconciled, so
    /// comparing a fresh catalog does not report a spurious state transition.
    pub(crate) fn restore_projects(&self, workspaces: &mut [WorkspaceSnapshot]) {
        for workspace in workspaces {
            if let Some(scope) = self
                .output
                .projects
                .get(&(workspace.device_id.clone(), workspace.id.clone()))
            {
                workspace.agent_scope.clone_from(scope);
            }
            for checkout in &mut workspace.checkouts {
                if let Some(scope) = self
                    .output
                    .checkouts
                    .get(&(workspace.device_id.clone(), checkout.id.clone()))
                {
                    checkout.agent_scope.clone_from(scope);
                }
            }
        }
    }

    fn apply(&self, snapshot: &mut crate::model::Snapshot) -> bool {
        let mut changed = snapshot.navigator.agent_scope != self.output.overall;
        snapshot
            .navigator
            .agent_scope
            .clone_from(&self.output.overall);
        for device in &mut snapshot.navigator.devices {
            let next = self
                .output
                .devices
                .get(&device.id)
                .cloned()
                .unwrap_or_default();
            changed |= device.agent_scope != next;
            device.agent_scope = next;
        }
        let workspaces = snapshot.navigator.workspaces.iter_mut().chain(
            snapshot
                .status
                .remote
                .iter_mut()
                .filter_map(|r| r.session.as_mut())
                .flat_map(|s| s.workspaces.iter_mut()),
        );
        for workspace in workspaces {
            let key = (workspace.device_id.clone(), workspace.id.clone());
            let next = self
                .output
                .projects
                .get(&key)
                .expect("scope was projected for every physical project")
                .clone();
            changed |= workspace.agent_scope != next;
            workspace.agent_scope = next;
            for checkout in &mut workspace.checkouts {
                let next = self
                    .output
                    .checkouts
                    .get(&(workspace.device_id.clone(), checkout.id.clone()))
                    .expect("scope was projected for every checkout")
                    .clone();
                {
                    changed |= checkout.agent_scope != next;
                    checkout.agent_scope = next;
                }
            }
        }
        changed
    }
}
