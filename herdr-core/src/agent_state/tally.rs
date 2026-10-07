//! Scope totals, marks and representative ordering.
mod cleanup;
pub(crate) use cleanup::cleanup_facts;
mod close;
mod graph;
pub(crate) mod lineage;
mod relations;
pub mod scope;
use super::axes::*;
use super::turn::*;
use crate::model::SidebarAgentSnapshot;
use crate::pet::PetSummary;
use crate::sidebar::sync_checkout_purposes;
use std::collections::BTreeMap;

/// The mark a row draws, matching the retired label plugin's symbol table. It is
/// one decision for the row's own symbol and for every count of marks, so a
/// badge that stands for folded rows says what opening them would show.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RowMark {
    Error,
    Approval,
    Question,
    Working,
    Done,
    Idle,
    Unknown,
}

impl RowMark {
    pub(crate) fn of(agent: &SidebarAgentSnapshot) -> Self {
        // A root waiting on its children keeps the hollow ring and says so:
        // its own completion is not the news while a child is still busy, and
        // the badge beside it says what the children are doing (D-01, D-02).
        if agent.waiting_on_descendants {
            return Self::Idle;
        }
        let (demand, activity, unread) = axes_of(agent);
        match demand {
            AgentDemand::Error => Self::Error,
            AgentDemand::Question => Self::Question,
            AgentDemand::Approval => Self::Approval,
            AgentDemand::None => match activity {
                AgentActivity::Working => Self::Working,
                AgentActivity::Stopped if agent.completed && unread => Self::Done,
                AgentActivity::Stopped => Self::Idle,
                AgentActivity::Unknown => Self::Unknown,
            },
        }
    }

    pub(crate) fn symbol(self) -> &'static str {
        match self {
            Self::Error => "\u{d7}",
            Self::Approval => "!",
            Self::Question => "?",
            Self::Working => "\u{25cf}",
            Self::Done => "✓",
            Self::Idle => "\u{25cb}",
            Self::Unknown => "~",
        }
    }

    fn count_into(self, counts: &mut crate::model::MarkCountsSnapshot) {
        match self {
            Self::Error => counts.error += 1,
            Self::Approval => counts.approval += 1,
            Self::Question => counts.question += 1,
            Self::Working => counts.working += 1,
            Self::Done => counts.done += 1,
            Self::Idle => counts.idle += 1,
            Self::Unknown => {}
        }
    }
}

/// Where a row stands when one row has to speak for several.
///
/// Group order first, then the worst demand inside it, with an unknown
/// activity ahead of an ordinary idle so missing information is not hidden by
/// a quiet sibling. The Workspace summary chip and the pane header's parent
/// badge both read it, so the two cannot disagree about which child a badge
/// is describing (docs/status-model.md, Workspace aggregation).
pub fn representative_rank(agent: &SidebarAgentSnapshot) -> (u8, u8) {
    rank_within(agent, ownership_of(agent))
}

/// Where a child stands among its siblings, as the pane that spawned them
/// sees it.
///
/// Delegation moves a child's demand off the *operator's* attention groups,
/// not out of existence. Its parent is exactly who is supposed to answer it,
/// so the parent's badge ranks its children as if it owned them; ranking them
/// the way the sidebar does would flatten every child to Working or Seen and
/// leave the badge naming a quiet sibling over the one that failed (PRD B15,
/// B16, D-35, D-38).
pub(crate) fn child_representative_rank(agent: &SidebarAgentSnapshot) -> (u8, u8) {
    rank_within(agent, Ownership::Operator)
}

fn rank_within(agent: &SidebarAgentSnapshot, ownership: Ownership) -> (u8, u8) {
    let (demand, activity, unread) = axes_of(agent);
    let demand_rank = match demand {
        AgentDemand::Error => 0,
        AgentDemand::Approval => 1,
        AgentDemand::Question => 2,
        AgentDemand::None if agent.activity == "unknown" => 3,
        AgentDemand::None => 4,
    };
    let group = agent_group_for(
        demand,
        activity,
        agent.completed,
        unread,
        agent.blocked,
        ownership,
        agent.waiting_on_descendants,
    );
    (group.rank(), demand_rank)
}

/// Aggregate physical pane ownership, never the visual lineage tree or raised rows.
/// Canonical order breaks ties after group and demand priority.
pub fn sync_checkout_agent_summaries(
    workspaces: &mut [crate::model::WorkspaceSnapshot],
    agents: &[SidebarAgentSnapshot],
) -> bool {
    let owners = workspaces
        .iter()
        .flat_map(|workspace| &workspace.checkouts)
        .enumerate()
        .flat_map(|(index, checkout)| {
            checkout
                .tabs
                .iter()
                .flat_map(|tab| &tab.panes)
                .map(move |pane| (pane.id.as_str(), index))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let count = workspaces
        .iter()
        .map(|workspace| workspace.checkouts.len())
        .sum();
    let mut summaries = vec![crate::model::CheckoutAgentSummary::default(); count];
    let mut ranks = vec![None; count];
    let mut counted = std::collections::HashSet::new();
    for agent in agents {
        let Some(&index) = owners.get(agent.pane_id.as_str()) else {
            continue;
        };
        if !counted.insert(agent.pane_id.as_str()) {
            continue;
        }
        let summary = &mut summaries[index];
        RowMark::of(agent).count_into(&mut summary.marks);
        let group = group_of(agent);
        match group {
            AgentGroup::NeedsYou => summary.needs_you += 1,
            AgentGroup::Done => summary.done += 1,
            AgentGroup::Working => summary.working += 1,
            AgentGroup::Seen => {
                summary.seen += 1;
                if agent.activity == "unknown" && agent.demand == "none" {
                    summary.unknown += 1;
                }
            }
        }
        let rank = representative_rank(agent);
        if ranks[index].is_none_or(|best| rank < best) {
            ranks[index] = Some(rank);
            summary.representative_pane_id = Some(agent.pane_id.clone());
        }
    }
    let mut changed = false;
    for (checkout, summary) in workspaces
        .iter_mut()
        .flat_map(|workspace| &mut workspace.checkouts)
        .zip(summaries)
    {
        if checkout.agent_summary != summary {
            checkout.agent_summary = summary;
            changed = true;
        }
    }
    // The removal confirmation's counts ride the same pass, so the dialog
    // names the panes the close will send and the agents still working in
    // them as of the same projection (D-10). "Running" is the deletion
    // gate's definition: the agent's activity axis says working. Every local
    // row offers `Remove project…`, a row Herdr shows without a registration
    // too (PRD sidebar-context-menus D-14), so every local row carries the
    // gate; a device's rows get theirs where its session is derived
    // (`device_catalog::apply_registrations`), because a remote tree arrives
    // freshly projected on every sync and a count written into it here
    // would read as a change on every tick.
    changed |= sync_workspace_removals(workspaces, agents, true);
    changed |= sync_checkout_purposes(workspaces, agents);
    changed
}

/// Buckets the projected agent list.
///
/// While the herdr connection is down the last valid agent list is retained
/// (so the pet does not blink to empty) but every retained agent counts as
/// disconnected: a stale yellow "act now" badge for a server that is no
/// longer answering is exactly the failure this prevents.
pub fn summarize(agents: &[SidebarAgentSnapshot], connected: bool) -> PetSummary {
    let mut summary = PetSummary::default();
    for agent in agents {
        if !connected {
            summary.disconnected += 1;
            continue;
        }
        // The groups are decided once, in the projection. The pet counts them
        // through the projection's own enum rather than reading tokens, axes,
        // or group names a second time.
        match crate::agent_state::group_of(agent) {
            AgentGroup::NeedsYou => {
                summary.needs_you += 1;
                if crate::agent_state::demand_of(agent) == AgentDemand::Error {
                    summary.error += 1;
                }
            }
            AgentGroup::Done => summary.done += 1,
            AgentGroup::Working => summary.working += 1,
            AgentGroup::Seen => summary.seen += 1,
        }
    }
    summary
}

/// The in-process subagents Hide's hook reports as working, summed over the
/// listed agents. A count is only meaningful while the server is answering,
/// and only a pane that still holds an agent is counted, so a token left on
/// a pane whose agent exited does not linger in the badge.
pub fn subagents_active(
    agents: &[SidebarAgentSnapshot],
    hook_tokens: &BTreeMap<String, crate::agent_hooks::PaneHookTokens>,
    connected: bool,
) -> u32 {
    if !connected {
        return 0;
    }
    agents
        .iter()
        .filter_map(|agent| hook_tokens.get(&agent.pane_id))
        .filter_map(|tokens| tokens.working)
        .fold(0, u32::saturating_add)
}

/// Whether this agent is one the operator still has to act on.
///
/// Read through the projection's own enum, not by matching the group name a
/// second time: a name compared here is a copy of a vocabulary that lives in
/// one place.
pub fn is_unseen(agent: &SidebarAgentSnapshot) -> bool {
    crate::agent_state::group_of(agent) == AgentGroup::NeedsYou
}

/// The unseen panes in click order: the pane whose unseen state was observed
/// first, then snapshot order.
///
/// `observed` holds the first time each pane was seen unseen. It lives in
/// memory only (D-21), so after a restart every pane carries the same first
/// observation and the snapshot's own order decides - which is the approved
/// fallback, not a defect.
pub fn attention_order(
    agents: &[SidebarAgentSnapshot],
    observed: &BTreeMap<String, u64>,
) -> Vec<String> {
    let mut unseen = agents
        .iter()
        .enumerate()
        .filter(|(_, agent)| is_unseen(agent))
        .map(|(index, agent)| {
            (
                observed.get(&agent.pane_id).copied().unwrap_or(u64::MAX),
                index,
                agent.pane_id.clone(),
            )
        })
        .collect::<Vec<_>>();
    unseen.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    unseen.into_iter().map(|(_, _, pane_id)| pane_id).collect()
}

/// Records the first moment each currently-unseen pane became unseen and
/// forgets panes that are no longer unseen. Running it twice with the same
/// agent list leaves the map unchanged.
pub fn observe_unseen(
    observed: &mut BTreeMap<String, u64>,
    agents: &[SidebarAgentSnapshot],
    now_unix_ms: u64,
) {
    let unseen = agents
        .iter()
        .filter(|agent| is_unseen(agent))
        .map(|agent| agent.pane_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    observed.retain(|pane_id, _| unseen.contains(pane_id.as_str()));
    for pane_id in unseen {
        observed.entry(pane_id.to_owned()).or_insert(now_unix_ms);
    }
}

/// The phone-safe projection, built only from accepted core snapshot values.
pub mod phone {
    use std::collections::HashMap;

    use serde::Serialize;
    use serde_json::Value;

    /// The groups in the order the phone draws them.
    pub const GROUP_ORDER: [&str; 4] = ["needs_you", "done", "working", "seen"];

    #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
    pub struct Line {
        pub text: String,
        /// `error`, `warning` (a question or approval), or `news`.
        pub tone: &'static str,
    }

    #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
    pub struct PhoneAgent {
        pub device_id: String,
        /// The core's pane id: a Herdr pane id on this Mac, `remote:<device>:pane:<id>` on a device.
        pub pane_id: String,
        /// The pane id of this agent's lineage root on the same device; itself for a root.
        pub root_pane_id: String,
        pub group: String,
        pub symbol: String,
        /// `error`, `warning`, `working`, `success` or `subtle`, as the desktop row colors its mark.
        pub tone: &'static str,
        pub agent_kind: String,
        pub title: String,
        /// `project · branch`, or the project alone for a plain folder.
        pub place: Option<String>,
        /// The SSH device's name; `None` on this Mac.
        pub device_label: Option<String>,
        /// When the core last saw the agent change state; the phone counts the
        /// elapsed time from it, so time passing sends nothing.
        pub changed_at_unix_ms: Option<u64>,
        pub line: Option<Line>,
        pub status_code: String,
        pub demand: String,
        /// Whether the compact title is emphasized, independent of the detail view.
        pub emphasized: bool,
        /// Whether opening the phone keeps this root notification. A read root
        /// question deliberately holds it even when the server clears its push.
        pub holds_notification: bool,
    }

    impl PhoneAgent {
        pub fn key(&self) -> AgentKey {
            AgentKey {
                device_id: self.device_id.clone(),
                pane_id: self.pane_id.clone(),
            }
        }

        pub fn root_key(&self) -> AgentKey {
            AgentKey {
                device_id: self.device_id.clone(),
                pane_id: self.root_pane_id.clone(),
            }
        }
    }

    #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
    pub struct AgentKey {
        pub device_id: String,
        pub pane_id: String,
    }

    impl AgentKey {
        /// The notification tag: one notification per root agent, replaced in place.
        pub fn tag(&self) -> String {
            format!("{}|{}", self.device_id, self.pane_id)
        }

        /// The pane id Herdr knows on the device that owns it.
        pub fn herdr_pane_id(&self) -> &str {
            let prefix = format!("remote:{}:pane:", self.device_id);
            self.pane_id
                .strip_prefix(prefix.as_str())
                .unwrap_or(&self.pane_id)
        }
    }

    #[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
    pub struct Group {
        pub group: String,
        pub agents: Vec<PhoneAgent>,
    }

    #[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
    pub struct Projection {
        pub groups: Vec<Group>,
        /// The core's explicit interface language (`ui_state.interface_language`)
        /// as stored, or `None` when the operator follows the system. The phone
        /// resolves the value; it is part of the projection so a change republishes
        /// the frame like any other change.
        pub interface_language: Option<String>,
    }

    impl Projection {
        pub fn agents(&self) -> impl Iterator<Item = &PhoneAgent> {
            self.groups.iter().flat_map(|group| group.agents.iter())
        }

        pub fn find(&self, key: &AgentKey) -> Option<&PhoneAgent> {
            self.agents()
                .find(|agent| agent.device_id == key.device_id && agent.pane_id == key.pane_id)
        }
    }

    fn str_of<'a>(value: &'a Value, field: &str) -> &'a str {
        value.get(field).and_then(Value::as_str).unwrap_or("")
    }

    fn tone(agent: &Value) -> &'static str {
        if agent
            .get("waiting_on_descendants")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return "working";
        }
        match str_of(agent, "demand") {
            "error" => "error",
            "question" | "approval" => "warning",
            _ => match str_of(agent, "activity") {
                "working" => "working",
                "stopped"
                    if agent
                        .get("emphasized")
                        .and_then(Value::as_bool)
                        .unwrap_or(false) =>
                {
                    "success"
                }
                _ => "subtle",
            },
        }
    }

    /// The desktop sidebar's second-line rule (`sidebarLine`): a request stays,
    /// news shows while unread, a quiet sentence is not drawn.
    fn line(agent: &Value) -> Option<Line> {
        let text = agent
            .get("detail")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())?;
        let tone = match str_of(agent, "demand") {
            "error" => "error",
            "question" | "approval" => "warning",
            _ if agent.get("unread").and_then(Value::as_bool) == Some(true) => "news",
            _ => return None,
        };
        Some(Line {
            text: text.to_owned(),
            tone,
        })
    }

    /// Each pane's `project · branch`, from one device's workspaces, the way
    /// the desktop's `agentPlaces` walks them.
    fn places(workspaces: Option<&Value>) -> HashMap<String, String> {
        let mut places = HashMap::new();
        for workspace in workspaces.and_then(Value::as_array).into_iter().flatten() {
            let label = str_of(workspace, "label");
            let checkouts = workspace
                .get("checkouts")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let folder = workspace.get("is_git").and_then(Value::as_bool) != Some(true)
                && checkouts.len() == 1;
            for checkout in checkouts {
                let place = if folder {
                    label.to_owned()
                } else {
                    let branch = checkout
                        .get("branch")
                        .and_then(Value::as_str)
                        .filter(|branch| !branch.is_empty())
                        .unwrap_or_else(|| str_of(checkout, "label"));
                    format!("{label} · {branch}")
                };
                for tab in checkout
                    .get("tabs")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    for pane in tab
                        .get("panes")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        if let Some(id) = pane.get("id").and_then(Value::as_str) {
                            places.entry(id.to_owned()).or_insert_with(|| place.clone());
                        }
                    }
                }
            }
        }
        places
    }

    fn roots(agents: &[Value]) -> HashMap<String, String> {
        let parents: HashMap<&str, &str> = agents
            .iter()
            .filter_map(|agent| {
                let pane = agent.get("pane_id").and_then(Value::as_str)?;
                let parent = agent
                    .get("lineage_parent_pane_id")
                    .and_then(Value::as_str)
                    .filter(|parent| !parent.is_empty())?;
                Some((pane, parent))
            })
            .collect();
        agents
            .iter()
            .filter_map(|agent| agent.get("pane_id").and_then(Value::as_str))
            .map(|pane| {
                let mut root = pane;
                // A lineage longer than the rows is a cycle; stop where it began.
                for _ in 0..agents.len() {
                    match parents.get(root) {
                        Some(parent) if *parent != pane => root = parent,
                        _ => break,
                    }
                }
                (pane.to_owned(), root.to_owned())
            })
            .collect()
    }

    fn rows(
        agents: &[Value],
        device_id: &str,
        device_label: Option<&str>,
        places: &HashMap<String, String>,
        out: &mut Vec<PhoneAgent>,
    ) {
        let roots = roots(agents);
        for agent in agents {
            let pane_id = str_of(agent, "pane_id");
            if pane_id.is_empty() {
                continue;
            }
            let title = agent
                .get("identity_label")
                .and_then(Value::as_str)
                .filter(|title| !title.trim().is_empty())
                .unwrap_or(str_of(agent, "agent_kind"));
            out.push(PhoneAgent {
                device_id: device_id.to_owned(),
                pane_id: pane_id.to_owned(),
                root_pane_id: roots
                    .get(pane_id)
                    .cloned()
                    .unwrap_or_else(|| pane_id.to_owned()),
                group: str_of(agent, "group").to_owned(),
                symbol: str_of(agent, "symbol").to_owned(),
                tone: tone(agent),
                emphasized: matches!(str_of(agent, "group"), "needs_you" | "done"),
                holds_notification: matches!(str_of(agent, "group"), "needs_you" | "done")
                    || matches!(str_of(agent, "demand"), "question" | "approval" | "error"),
                agent_kind: str_of(agent, "agent_kind").to_owned(),
                title: title.to_owned(),
                place: places.get(pane_id).cloned(),
                device_label: device_label.map(str::to_owned),
                changed_at_unix_ms: agent.get("changed_at_unix_ms").and_then(Value::as_u64),
                line: line(agent),
                status_code: str_of(agent, "status_code").to_owned(),
                demand: agent
                    .get("demand")
                    .and_then(Value::as_str)
                    .unwrap_or("none")
                    .to_owned(),
            });
        }
    }

    /// The phone projection of a merged `rest` section; `node` is the core's own
    /// machine, whose agents the navigator lists.
    pub fn project(rest: &Value, node: &str) -> Projection {
        let mut agents = Vec::new();
        let local = rest
            .pointer("/navigator/agents")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let local_places = places(rest.pointer("/navigator/workspaces"));
        rows(local, node, None, &local_places, &mut agents);
        let devices = rest
            .pointer("/navigator/devices")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for status in rest
            .pointer("/status/remote")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if str_of(status, "state") != "connected" {
                continue;
            }
            let target = str_of(status, "target_id");
            let label = devices
                .iter()
                .find(|device| str_of(device, "id") == target)
                .map(|device| str_of(device, "label"))
                .filter(|label| !label.is_empty())
                .unwrap_or(target);
            let session = status.get("session");
            let remote_agents = session
                .and_then(|session| session.get("agents"))
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let remote_places = places(session.and_then(|session| session.get("workspaces")));
            rows(
                remote_agents,
                target,
                Some(label),
                &remote_places,
                &mut agents,
            );
        }
        let mut groups: Vec<Group> = GROUP_ORDER
            .iter()
            .map(|group| Group {
                group: (*group).to_owned(),
                agents: Vec::new(),
            })
            .collect();
        for agent in agents {
            match groups.iter_mut().find(|group| group.group == agent.group) {
                Some(group) => group.agents.push(agent),
                None => groups.push(Group {
                    group: agent.group.clone(),
                    agents: vec![agent],
                }),
            }
        }
        groups.retain(|group| !group.agents.is_empty());
        let interface_language = rest
            .pointer("/ui_state/interface_language")
            .and_then(Value::as_str)
            .map(str::to_owned);
        Projection {
            groups,
            interface_language,
        }
    }
}

#[cfg(test)]
mod scope_tests;

impl crate::model::DescendantCountsSnapshot {
    /// Whether any drawn count is above zero, so a row with descendants that
    /// are all merely ready wears no badge rather than an empty one.
    pub fn any_drawn(&self) -> bool {
        self.error + self.approval + self.question + self.working + self.done > 0
    }
}

/// Existing inactive-checkout exception, independent of Git and focus policy.
pub(crate) fn checkout_has_active_agents(summary: &crate::model::CheckoutAgentSummary) -> bool {
    summary.working > 0 || summary.needs_you > 0
}

/// Removal confirmation counts have the same activity rule on both devices.
pub(crate) fn sync_workspace_removals(
    workspaces: &mut [crate::model::WorkspaceSnapshot],
    agents: &[SidebarAgentSnapshot],
    local_only: bool,
) -> bool {
    let mut changed = false;
    let running = agents
        .iter()
        .filter(|agent| agent.activity == AgentActivity::Working.name())
        .map(|agent| agent.pane_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    for workspace in workspaces
        .iter_mut()
        .filter(|workspace| !local_only || workspace.remote_target_id.is_none())
    {
        let panes = workspace
            .checkouts
            .iter()
            .flat_map(|checkout| &checkout.tabs)
            .flat_map(|tab| &tab.panes);
        let mut removal = crate::model::WorkspaceRemovalGateSnapshot::default();
        for pane in panes {
            removal.pane_count += 1;
            if running.contains(pane.id.as_str()) {
                removal.running_agent_count += 1;
            }
        }
        if workspace.removal != removal {
            workspace.removal = removal;
            changed = true;
        }
    }
    changed
}

pub(crate) fn device_catalog_count(
    workspaces: &[crate::model::WorkspaceSnapshot],
    agents: &[SidebarAgentSnapshot],
    device: &str,
) -> u32 {
    agents
        .iter()
        .filter(|agent| {
            workspaces
                .iter()
                .filter(|w| w.device_id == device)
                .flat_map(|w| &w.checkouts)
                .flat_map(|c| &c.tabs)
                .flat_map(|t| &t.panes)
                .any(|p| p.id == agent.pane_id)
        })
        .count() as u32
}

/// The rail retains the remote session's last row count while disconnected.
pub(crate) fn remote_session_count(session: Option<&crate::model::RemoteSessionSnapshot>) -> u32 {
    session.map_or(0, |s| s.agents.len()).min(u32::MAX as usize) as u32
}
