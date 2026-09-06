//! Pet state: how the projected agent list becomes a pose, a badge row, and
//! an ordered attention queue.
//!
//! Ported from herdr-pet's `behavior.rs` (pose ladder, sleep sequence). The
//! unseen-vs-acknowledged token rule is *not* re-implemented here: it stays
//! owned by the sidebar projection (INV-herdr-unseen-token), and this module
//! counts the groups that projection already decided.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::model::SidebarAgentSnapshot;
use crate::sidebar::{AgentDemand, AgentGroup};

pub const SLEEP_IDLE_MS: u64 = 60_000;
pub const SLEEP_PHASE_MS: u64 = 4_000;
pub const FREE_ROAM_IDLE_MS: u64 = 8_000;

/// The pet counts the sidebar's four groups, plus the agents whose server
/// stopped answering, so the badge row and the sidebar can never disagree
/// about what is waiting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PetSummary {
    /// The pet's "act now" count: exactly the Needs You group.
    pub needs_you: usize,
    pub done: usize,
    pub working: usize,
    pub seen: usize,
    pub disconnected: usize,
    /// How many of `needs_you` are errors rather than questions or approvals.
    /// This is not a group of its own and nothing draws a count from it; the
    /// pose ladder puts `error` one rung above `notification`, so the ladder
    /// needs to know an error is in there.
    pub error: usize,
}

impl PetSummary {
    pub fn total(&self) -> usize {
        self.needs_you + self.working + self.done + self.seen + self.disconnected
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct AmbientTotals {
    pub subagents_active: u32,
    pub background_running: u32,
    pub background_failed: u32,
}

impl AmbientTotals {
    pub fn is_empty(&self) -> bool {
        self.subagents_active == 0 && self.background_running == 0 && self.background_failed == 0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SleepPhase {
    #[default]
    Awake,
    Yawning,
    Dozing,
    Collapsing,
    Sleeping,
}

impl SleepPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Awake => "awake",
            Self::Yawning => "yawning",
            Self::Dozing => "dozing",
            Self::Collapsing => "collapsing",
            Self::Sleeping => "sleeping",
        }
    }
}

/// The clawd-compatible sleep sequence starts after one minute of inactivity.
/// Each transitional pose lasts four seconds; once asleep it stays asleep
/// until activity wakes it.
pub fn sleep_phase_for_idle_ms(idle_ms: u64) -> SleepPhase {
    if idle_ms < SLEEP_IDLE_MS {
        return SleepPhase::Awake;
    }
    match (idle_ms - SLEEP_IDLE_MS) / SLEEP_PHASE_MS {
        0 => SleepPhase::Yawning,
        1 => SleepPhase::Dozing,
        2 => SleepPhase::Collapsing,
        _ => SleepPhase::Sleeping,
    }
}

/// The documented pose ladder:
/// `error > notification > sweeping > attention > carrying/juggling > working
/// > thinking > idle/roam > sleeping`.
///
/// Herdr has no separate sweeping or thinking token, so those rungs stay
/// reserved and fall through to the surrounding states.
pub fn expanded_state(summary: PetSummary, idle_ms: u64, waking: bool) -> &'static str {
    if summary.error > 0 {
        "error"
    } else if summary.needs_you > 0 {
        "notification"
    } else if summary.working >= 2 {
        "juggling"
    } else if summary.working == 1 {
        "carrying"
    } else if waking {
        "waking"
    } else if sleep_phase_for_idle_ms(idle_ms) == SleepPhase::Sleeping {
        "sleeping"
    } else if idle_ms >= FREE_ROAM_IDLE_MS {
        "roam"
    } else if summary.working > 0 {
        "working"
    } else {
        "idle"
    }
}

pub fn is_roam_allowed(summary: PetSummary, idle_ms: u64, dragging: bool) -> bool {
    !dragging
        && summary.needs_you == 0
        && summary.working == 0
        && summary.seen > 0
        && idle_ms >= FREE_ROAM_IDLE_MS
        && idle_ms < SLEEP_IDLE_MS
}

/// The single pose the renderer draws.
///
/// A lost connection outranks everything: an unreachable server cannot say
/// anything true about agent state, so the pet says so rather than holding a
/// stale working pose. Otherwise this is [`expanded_state`] with the sleep
/// sequence's transitional poses made visible between 60s and 72s of idle,
/// which `expanded_state` itself reports as `roam`.
pub fn pose(summary: PetSummary, idle_ms: u64, waking: bool, connected: bool) -> &'static str {
    if !connected {
        return "disconnected";
    }
    let state = expanded_state(summary, idle_ms, waking);
    if state != "roam" {
        return state;
    }
    match sleep_phase_for_idle_ms(idle_ms) {
        SleepPhase::Awake => "roam",
        phase => phase.as_str(),
    }
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
        match crate::sidebar::group_of(agent) {
            AgentGroup::NeedsYou => {
                summary.needs_you += 1;
                if crate::sidebar::demand_of(agent) == AgentDemand::Error {
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

/// Ambient counts are only meaningful while the server is answering.
pub fn ambient_totals(agents: &[SidebarAgentSnapshot], connected: bool) -> AmbientTotals {
    let mut totals = AmbientTotals::default();
    if !connected {
        return totals;
    }
    for ambient in agents.iter().filter_map(|agent| agent.ambient.as_ref()) {
        totals.subagents_active = totals
            .subagents_active
            .saturating_add(ambient.subagents_active);
        totals.background_running = totals
            .background_running
            .saturating_add(ambient.background_running);
        totals.background_failed = totals
            .background_failed
            .saturating_add(ambient.background_failed);
    }
    totals
}

/// Whether this agent is one the operator still has to act on.
///
/// Read through the projection's own enum, not by matching the group name a
/// second time: a name compared here is a copy of a vocabulary that lives in
/// one place.
pub fn is_unseen(agent: &SidebarAgentSnapshot) -> bool {
    crate::sidebar::group_of(agent) == AgentGroup::NeedsYou
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AmbientSignal;

    /// A projected row named by the group it landed in. `needs_you` rows carry
    /// a question unless the name says error, which is the one demand the pet
    /// counts separately.
    fn agent(pane_id: &str, group: &str) -> SidebarAgentSnapshot {
        let (group, demand) = match group {
            "error" => ("needs_you", "error"),
            "needs_you" => ("needs_you", "question"),
            other => (other, "none"),
        };
        SidebarAgentSnapshot {
            id: pane_id.to_owned(),
            pane_id: pane_id.to_owned(),
            workspace_label: "Fixture".to_owned(),
            checkout_label: None,
            agent_kind: "codex".to_owned(),
            demand: demand.to_owned(),
            activity: if group == "working" {
                "working"
            } else {
                "stopped"
            }
            .to_owned(),
            unread: group != "seen",
            blocked: false,
            group: group.to_owned(),
            symbol: "\u{25cb}".to_owned(),
            emphasized: false,
            status_label: "Idle".to_owned(),
            requires_close_confirmation: false,
            summary: "summary".to_owned(),
            elapsed: "1s".to_owned(),
            last_activity: "0000000000001".to_owned(),
            state_change_seq: None,
            ambient: None,
            session_id: None,
            spawned_from_pane_id: None,
            lineage_depth: 0,
            lineage_child_pane_ids: Vec::new(),
            lineage_root_checkout_id: None,
            lineage_worktree_badge: None,
            lineage_orphan: false,
            lineage_hint: None,
            raised_hint: None,
            lineage_collapsed: false,
        }
    }

    fn idle_summary() -> PetSummary {
        PetSummary {
            seen: 1,
            ..Default::default()
        }
    }

    #[test]
    fn sleep_sequence_has_four_second_transitions() {
        assert_eq!(sleep_phase_for_idle_ms(59_999), SleepPhase::Awake);
        assert_eq!(sleep_phase_for_idle_ms(60_000), SleepPhase::Yawning);
        assert_eq!(sleep_phase_for_idle_ms(64_000), SleepPhase::Dozing);
        assert_eq!(sleep_phase_for_idle_ms(68_000), SleepPhase::Collapsing);
        assert_eq!(sleep_phase_for_idle_ms(72_000), SleepPhase::Sleeping);
    }

    #[test]
    fn expanded_priority_keeps_urgent_states_above_activity() {
        let mut summary = idle_summary();
        summary.working = 2;
        assert_eq!(expanded_state(summary, 0, false), "juggling");
        summary.working = 1;
        assert_eq!(expanded_state(summary, 0, false), "carrying");
        summary.working = 2;
        summary.needs_you = 1;
        assert_eq!(expanded_state(summary, 90_000, false), "notification");
        summary.error = 1;
        assert_eq!(expanded_state(summary, 90_000, false), "error");
    }

    #[test]
    fn roam_is_cancelled_by_work_or_sleep() {
        assert!(is_roam_allowed(idle_summary(), 8_000, false));
        assert!(!is_roam_allowed(idle_summary(), 7_999, false));
        assert!(!is_roam_allowed(idle_summary(), 60_000, false));
        assert!(!is_roam_allowed(idle_summary(), 8_000, true));
        let mut working = idle_summary();
        working.working = 1;
        assert!(!is_roam_allowed(working, 8_000, false));
    }

    #[test]
    fn pose_walks_idle_through_roam_and_the_full_sleep_sequence() {
        let summary = idle_summary();
        assert_eq!(pose(summary, 0, false, true), "idle");
        assert_eq!(pose(summary, 7_999, false, true), "idle");
        assert_eq!(pose(summary, 8_000, false, true), "roam");
        assert_eq!(pose(summary, 60_000, false, true), "yawning");
        assert_eq!(pose(summary, 64_000, false, true), "dozing");
        assert_eq!(pose(summary, 68_000, false, true), "collapsing");
        assert_eq!(pose(summary, 72_000, false, true), "sleeping");
        assert_eq!(pose(summary, 90_000, true, true), "waking");
    }

    #[test]
    fn a_lost_connection_outranks_every_other_pose() {
        let mut summary = idle_summary();
        summary.needs_you = 3;
        summary.error = 3;
        summary.working = 2;
        assert_eq!(pose(summary, 0, false, true), "error");
        assert_eq!(pose(summary, 0, false, false), "disconnected");
    }

    /// R6, AC9. The pet counts the sidebar's four groups and nothing else, so
    /// its act-now number is the Needs You count and its done number is the
    /// Done count. Before this the pet kept its own five buckets and the yellow
    /// badge held Needs You minus the errors.
    #[test]
    fn pet_groups_are_counted_as_the_sidebar_groups() {
        let agents = [
            agent("a", "error"),
            agent("b", "needs_you"),
            agent("c", "needs_you"),
            agent("d", "needs_you"),
            agent("e", "working"),
            agent("f", "done"),
            agent("g", "seen"),
            agent("h", "seen"),
        ];
        let summary = summarize(&agents, true);
        let groups = |group: AgentGroup| {
            agents
                .iter()
                .filter(|agent| crate::sidebar::group_of(agent) == group)
                .count()
        };
        assert_eq!(
            summary.needs_you,
            groups(AgentGroup::NeedsYou),
            "act now is the Needs You group, errors included"
        );
        assert_eq!(summary.done, groups(AgentGroup::Done));
        assert_eq!(summary.working, groups(AgentGroup::Working));
        assert_eq!(summary.seen, groups(AgentGroup::Seen));
        assert_eq!(
            summary.error, 1,
            "the pose ladder still knows an error is among them"
        );
        assert_eq!(summary.total(), agents.len());
    }

    /// AC9. A server that stopped answering cannot say anything true about what
    /// is waiting, so every count goes to zero and the retained rows are counted
    /// as disconnected instead.
    #[test]
    fn pet_groups_go_quiet_when_the_server_stops_answering() {
        let agents = [agent("a", "needs_you"), agent("b", "working")];
        let summary = summarize(&agents, false);
        assert_eq!(summary.disconnected, 2, "the last valid list is retained");
        assert_eq!(summary.needs_you, 0);
        assert_eq!(summary.done, 0);
        assert_eq!(summary.working, 0);
        assert_eq!(summary.seen, 0);
        assert_eq!(pose(summary, 0, false, false), "disconnected");
    }

    #[test]
    fn ambient_counts_sum_across_panes_and_go_quiet_while_disconnected() {
        let mut first = agent("a", "working");
        first.ambient = Some(AmbientSignal {
            subagents_active: 2,
            background_running: 1,
            background_failed: 0,
        });
        let mut second = agent("b", "working");
        second.ambient = Some(AmbientSignal {
            subagents_active: 1,
            background_running: 0,
            background_failed: 3,
        });
        let agents = [first, second, agent("c", "idle")];

        let totals = ambient_totals(&agents, true);
        assert_eq!(totals.subagents_active, 3);
        assert_eq!(totals.background_running, 1);
        assert_eq!(totals.background_failed, 3);
        assert!(!totals.is_empty());

        assert!(ambient_totals(&agents, false).is_empty());
        assert!(ambient_totals(&[agent("c", "seen")], true).is_empty());
    }

    #[test]
    fn the_oldest_observed_unseen_pane_is_selected_before_later_ones() {
        let agents = [
            agent("later", "needs_you"),
            agent("working", "working"),
            agent("earlier", "error"),
        ];
        let observed = BTreeMap::from([("later".to_owned(), 2_000), ("earlier".to_owned(), 1_000)]);
        assert_eq!(attention_order(&agents, &observed), ["earlier", "later"]);
    }

    #[test]
    fn without_observations_snapshot_order_decides_and_seen_panes_never_qualify() {
        let agents = [
            agent("first", "needs_you"),
            agent("second", "error"),
            agent("acknowledged", "seen"),
            agent("done", "done"),
        ];
        assert_eq!(
            attention_order(&agents, &BTreeMap::new()),
            ["first", "second"]
        );
    }

    #[test]
    fn observing_the_same_snapshot_twice_keeps_the_first_moment() {
        let agents = [agent("a", "needs_you"), agent("b", "seen")];
        let mut observed = BTreeMap::new();
        observe_unseen(&mut observed, &agents, 1_000);
        observe_unseen(&mut observed, &agents, 5_000);
        assert_eq!(observed.get("a"), Some(&1_000));
        assert!(!observed.contains_key("b"));

        // Once the user acknowledges it, the pane is forgotten and a later
        // unseen state starts a fresh observation.
        observe_unseen(&mut observed, &[agent("a", "seen")], 6_000);
        assert!(observed.is_empty());
        observe_unseen(&mut observed, &agents, 7_000);
        assert_eq!(observed.get("a"), Some(&7_000));
    }
}
