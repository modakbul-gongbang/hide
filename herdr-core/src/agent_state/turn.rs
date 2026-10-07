//! What the operator needs to do, derived from the five axes.
use super::axes::*;
use super::tally::RowMark;

/// What one agent is doing as far as removing its checkout is concerned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentUse {
    /// Stopped: finished a turn or idle. Its pane may close.
    Quiet,
    Working,
    /// Waiting on the operator: a question, an approval or a blocked prompt.
    Waiting,
    Unknown,
}

impl AgentUse {
    /// From the status model's axes (`docs/status-model.md`): a demand or a
    /// blocked prompt is waiting whatever else is reported, and an activity
    /// other than `working` or `stopped` is not claimed to be idle.
    pub(crate) fn of(demand: &str, blocked: bool, activity: &str) -> Self {
        if demand != "none" || blocked {
            Self::Waiting
        } else {
            match activity {
                "working" => Self::Working,
                "stopped" => Self::Quiet,
                _ => Self::Unknown,
            }
        }
    }
}

/// Semantic tone; a shell chooses its existing color token and opacity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Tone {
    pub kind: &'static str,
    pub read: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TabState {
    pub mark_tone: Tone,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RowLine {
    pub text: String,
    pub mode: &'static str,
    pub tone: Tone,
}

/// The decisions formerly repeated by each row, search, graph and board.
/// Surface differences are intentional: a waiting root has a working mark
/// but its chip still follows its own axes, and every demand remains visible.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct RowState {
    pub attention: bool,
    pub needs_you: bool,
    pub root: bool,
    pub title_emphasized: bool,
    pub selection_emphasizes_title: bool,
    pub asking: bool,
    pub working: bool,
    pub waits_on_children: bool,
    pub chip_tone: Tone,
    pub mark_tone: Tone,
    pub line: Option<RowLine>,
    pub branch_badge: Option<String>,
    pub bucket: &'static str,
    pub attention_rank: u8,
    pub graph_rank: u8,
    pub graph_chip: &'static str,
    pub graph_resting: bool,
    pub edge: &'static str,
    pub search_tone: &'static str,
    pub subtree: &'static str,
    pub link: &'static str,
    pub link_rank: u8,
    pub verb: RequestVerb,
    pub request_todo: bool,
    pub descendant_asking: u32,
    pub request_since: Option<u64>,
}

pub(crate) fn row_state(agent: &SidebarAgentSnapshot) -> RowState {
    let demand = agent.demand.as_str();
    let activity = agent.activity.as_str();
    let group = agent.group.as_str();
    let asking = matches!(demand, "error" | "question" | "approval");
    let needs_you = group == "needs_you";
    let attention = needs_you || agent.unread;
    let chip_kind = match demand {
        "error" => "error",
        "question" | "approval" => "warning",
        _ if activity == "working" => "working",
        _ if activity == "stopped" && agent.emphasized => "success",
        _ => "subtle",
    };
    let chip_tone = Tone {
        kind: chip_kind,
        read: asking && !agent.emphasized,
    };
    let mark_tone = if agent.waiting_on_descendants {
        Tone {
            kind: "working",
            read: false,
        }
    } else {
        chip_tone
    };
    let line = agent
        .detail
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| {
            let mode = if asking {
                "request"
            } else if agent.unread {
                "news"
            } else {
                "quiet"
            };
            RowLine {
                text: text.to_owned(),
                mode,
                tone: match mode {
                    "request" => chip_tone,
                    "news" => Tone {
                        kind: "news",
                        read: false,
                    },
                    _ => Tone {
                        kind: "subtle",
                        read: false,
                    },
                },
            }
        });
    let bucket = if needs_you || group == "done" {
        "turn"
    } else if agent.waiting_on_descendants {
        "delegating"
    } else if group == "working" {
        "working"
    } else {
        "resting"
    };
    let counts = agent.descendant_counts;
    let waits_on_children = agent.waiting_on_descendants
        || counts.working + counts.question + counts.approval + counts.error > 0;
    let working = group == "working" || agent.waiting_on_descendants;
    let verb = agent
        .request
        .as_ref()
        .map(|request| request.verb)
        .unwrap_or(if group == "working" {
            RequestVerb::Working
        } else {
            RequestVerb::Idle
        });
    let request_todo = matches!(
        verb,
        RequestVerb::Answer
            | RequestVerb::Fix
            | RequestVerb::Review
            | RequestVerb::Stopped
            | RequestVerb::Result
    );
    RowState {
        attention,
        needs_you,
        root: !agent.delegated,
        title_emphasized: !(agent.delegated && !attention) && (attention || agent.emphasized),
        selection_emphasizes_title: !agent.delegated || attention,
        asking,
        working,
        waits_on_children,
        chip_tone,
        mark_tone,
        line,
        branch_badge: agent
            .lineage_worktree_badge
            .as_deref()
            .filter(|_| agent.delegated)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
        bucket,
        attention_rank: match group {
            "needs_you" if demand == "error" => 0,
            "needs_you" => 1,
            "done" => 2,
            "working" => 3,
            _ => 4,
        },
        graph_rank: if asking || bucket == "turn" {
            0
        } else if bucket == "working" {
            1
        } else if waits_on_children {
            2
        } else {
            3
        },
        graph_chip: match bucket {
            "turn" => "turn",
            "resting" => "resting",
            _ => "working",
        },
        graph_resting: !asking && bucket != "turn" && bucket != "working" && !waits_on_children,
        edge: if asking {
            "ask"
        } else if activity == "working" {
            "flow"
        } else if waits_on_children {
            "wait"
        } else {
            "rest"
        },
        search_tone: match chip_kind {
            "error" => "failed",
            "warning" => "attention",
            "working" => "working",
            "success" => "done",
            _ => "muted",
        },
        subtree: if agent.requires_close_status_check {
            "unknown"
        } else if demand != "none" && !demand.is_empty() {
            "waiting"
        } else if activity == "working" {
            "working"
        } else if agent.unread && agent.symbol == "✓" {
            "unread"
        } else {
            "quiet"
        },
        link: if asking {
            "question"
        } else if working {
            "working"
        } else {
            "idle"
        },
        link_rank: if asking {
            0
        } else if working {
            1
        } else {
            2
        },
        verb,
        request_todo,
        descendant_asking: agent.descendant_counts.question + agent.descendant_counts.approval,
        request_since: if request_todo {
            agent
                .request
                .as_ref()
                .map(|request| request.verb_since_unix_ms)
        } else {
            agent.changed_at_unix_ms
        },
    }
}

/// Runtime gates deliberately distinguish a running process from a root
/// waiting on work it delegated. Both prevent sleep and reopening.
pub(crate) fn is_running(agent: &SidebarAgentSnapshot) -> bool {
    agent.activity == "working"
}

pub(crate) fn has_work_in_progress(agent: &SidebarAgentSnapshot) -> bool {
    is_running(agent) || agent.group == "working"
}

pub(crate) fn is_waiting_for_operator(agent: &SidebarAgentSnapshot) -> bool {
    agent.demand != "none" || agent.blocked
}

/// A session plan approval Enter starts the person's next request, unlike
/// an Enter at Herdr's ordinary blocked prompt. Preserve the observed status
/// when present and the existing row fallback when it is not.
pub(crate) fn submit_answers_prompt(
    agent: &SidebarAgentSnapshot,
    observed_status: Option<&str>,
) -> bool {
    match observed_status {
        Some(status) => status == "blocked",
        None => agent.blocked && agent.activity != "stopped",
    }
}

pub(crate) fn is_seen(agent: &SidebarAgentSnapshot) -> bool {
    agent.group == "seen"
}

pub(crate) fn rest_refusal(agent: &SidebarAgentSnapshot) -> Option<&'static str> {
    if has_work_in_progress(agent) {
        Some("This agent is working")
    } else if is_waiting_for_operator(agent) {
        Some("This agent is waiting for you")
    } else if agent.activity != "stopped" {
        Some("Hide cannot tell what this agent is doing")
    } else {
        None
    }
}
use crate::labels::analysis::LabelEnd;
use crate::model::{AgentStatusCode, PullRequestChecks, SidebarAgentSnapshot};
use crate::request_view::AgentPullRequestSnapshot;
use serde::{Deserialize, Serialize};

/// The four groups the sidebar reads top to bottom.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentGroup {
    NeedsYou,
    Done,
    Working,
    Seen,
}

impl AgentGroup {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::NeedsYou => "needs_you",
            Self::Done => "done",
            Self::Working => "working",
            Self::Seen => "seen",
        }
    }

    /// Where the group sits in the sidebar, read top to bottom: what is
    /// waiting on the operator, what finished while they were away, what is
    /// still running, then everything already dealt with.
    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::NeedsYou => 0,
            Self::Done => 1,
            Self::Working => 2,
            Self::Seen => 3,
        }
    }
}

/// The group a row belongs to, given who owns it.
///
/// `waiting_on_descendants` is the lineage pass's answer for a root that is
/// quiet itself while a descendant is still busy: such a row has not
/// finished, so it sits in Working, and it reaches Done only once it and
/// every descendant are quiet (sidebar-agent-status D-01). Its own unread
/// demand or a blocked prompt still wins, because the flag is only ever set
/// on a row with no demand of its own.
pub fn agent_group_for(
    demand: AgentDemand,
    activity: AgentActivity,
    completed: bool,
    unread: bool,
    blocked: bool,
    ownership: Ownership,
    waiting_on_descendants: bool,
) -> AgentGroup {
    if ownership == Ownership::Delegated {
        // The row keeps its own mark and status word; only its claim on the
        // operator's attention is withheld.
        return if activity == AgentActivity::Working {
            AgentGroup::Working
        } else {
            AgentGroup::Seen
        };
    }
    if blocked || (demand != AgentDemand::None && unread) {
        AgentGroup::NeedsYou
    } else if waiting_on_descendants {
        AgentGroup::Working
    } else if demand == AgentDemand::None
        && activity == AgentActivity::Stopped
        && completed
        && unread
    {
        AgentGroup::Done
    } else if activity == AgentActivity::Working {
        AgentGroup::Working
    } else {
        AgentGroup::Seen
    }
}

/// The one short word a row shows. A reported completion the operator has not
/// read is `Done`; an ordinary stopped pane and a read completion are `Idle`.
/// No view ever shows an axis value, so nothing underscored can reach the
/// screen.
/// The status word of a root waiting on its children.
pub(crate) fn agent_status_code(
    demand: AgentDemand,
    activity: AgentActivity,
    completed: bool,
    unread: bool,
) -> AgentStatusCode {
    match demand {
        AgentDemand::Error => AgentStatusCode::Error,
        AgentDemand::Question => AgentStatusCode::Question,
        AgentDemand::Approval => AgentStatusCode::Approval,
        AgentDemand::None => match activity {
            AgentActivity::Working => AgentStatusCode::Working,
            AgentActivity::Stopped if completed && unread => AgentStatusCode::Done,
            AgentActivity::Stopped => AgentStatusCode::Idle,
            AgentActivity::Unknown => AgentStatusCode::Unknown,
        },
    }
}

/// Closing this pane would interrupt running work or discard an unresolved
/// demand. Read state remains a presentation axis, so an unread completion or
/// an ordinary stopped pane cannot create a close prompt by itself.
pub(crate) fn agent_requires_close_confirmation(
    activity: AgentActivity,
    demand: AgentDemand,
    blocked: bool,
) -> bool {
    activity == AgentActivity::Working || demand != AgentDemand::None || blocked
}

/// An activity-less pane cannot safely be described as idle in a destructive
/// confirmation. The caller must obtain a fresh status before it can close.
pub(crate) fn agent_requires_close_status_check(
    activity: AgentActivity,
    demand: AgentDemand,
    blocked: bool,
) -> bool {
    activity == AgentActivity::Unknown && demand == AgentDemand::None && !blocked
}

/// What a row's second line says, decided by the row's group (PRD D-06).
///
/// Rows that still concern the operator - the Needs You group and an unread
/// Done - keep their status word and add what the operator is being asked
/// for (`expected_reply`), or what happened (`progress`) when nothing is
/// asked. A working row shows only its progress: the mark already says it is
/// working. A row the operator has read, and a row whose activity is unknown,
/// say nothing more than their name, unless it still holds a request. With no sentence at all, a row that
/// would have shown one keeps the status word alone, so a pane with no label
/// plugin behind it still reads as it did before (PRD B6, B13).
///
/// Returns whether the status word is drawn and the sentence beside it.
pub(crate) fn agent_second_line(
    group: AgentGroup,
    demand: AgentDemand,
    expected_reply: Option<&str>,
    progress: Option<&str>,
) -> (bool, Option<String>) {
    // An unresolved demand keeps its request whatever group the row sits in:
    // a read question in Seen and a delegated child's approval still say what
    // they are asking, so a view can keep that line until the request is
    // answered rather than until it is looked at (sidebar-agent-status B7).
    // Only a group that wanted a sentence falls back to the status word.
    let request = (demand != AgentDemand::None)
        .then(|| expected_reply.or(progress))
        .flatten();
    let sentence = match group {
        AgentGroup::NeedsYou | AgentGroup::Done => expected_reply.or(progress),
        AgentGroup::Working => request.or(progress),
        AgentGroup::Seen => return (false, request.map(str::to_owned)),
    };
    // The word is the sentence's stand-in, never its prefix: the mark and the
    // group heading already say Question or Done, and the word beside a
    // sentence took the width the sentence needed (2026-09-18).
    match sentence {
        Some(sentence) => (false, Some(sentence.to_owned())),
        None => (true, None),
    }
}

/// The one ordering every agent surface reads: the two sidebar views, the pet
/// dashboard, and the agent switcher's candidate list.
///
/// Group order first, then most recent activity descending, then the order
/// Herdr sent the rows in. The sort is stable, so that third key costs
/// nothing, and re-running it on an already ordered list is the identity
/// (engineering rule 11).
///
/// It runs after the read axis, never inside `project_agents`: a row's group
/// depends on whether the operator has read it, and a projection that has not
/// met the read record ledger treats every completion as unread. Ordinary idle
/// rows still stay Seen. The order Hide shows is Hide's own.
pub(crate) fn sort_agents(agents: &mut [SidebarAgentSnapshot]) {
    agents.sort_by(|left, right| {
        group_of(left)
            .rank()
            .cmp(&group_of(right).rank())
            .then_with(|| right.last_activity.cmp(&left.last_activity))
    });
}

pub fn group_of(agent: &SidebarAgentSnapshot) -> AgentGroup {
    let (demand, activity, unread) = axes_of(agent);
    agent_group_for(
        demand,
        activity,
        agent.completed,
        unread,
        agent.blocked,
        ownership_of(agent),
        agent.waiting_on_descendants,
    )
}

/// Fills in every value the shell draws from the lifecycle axes.
///
/// Called again whenever the read axis moves, so the derived values can never
/// describe a different read state than the row they sit on.
pub(crate) fn derive_from_axes(agent: &mut SidebarAgentSnapshot) {
    let (demand, activity, unread) = axes_of(agent);
    let waiting = agent.waiting_on_descendants;
    let group = agent_group_for(
        demand,
        activity,
        agent.completed,
        unread,
        agent.blocked,
        ownership_of(agent),
        waiting,
    );
    agent.group = group.name().to_owned();
    agent.symbol = RowMark::of(agent).symbol().to_owned();
    // A row the operator still has to deal with is drawn bright; everything
    // already read or merely running is subdued.
    agent.emphasized = matches!(group, AgentGroup::NeedsYou | AgentGroup::Done);
    agent.status_code = if waiting {
        AgentStatusCode::Waiting
    } else {
        agent_status_code(demand, activity, agent.completed, unread)
    };
    let (status_word_visible, detail) = agent_second_line(
        group,
        demand,
        agent.expected_reply.as_deref(),
        agent.progress.as_deref(),
    );
    agent.status_word_visible = status_word_visible;
    agent.detail = detail;
    agent.requires_close_confirmation =
        agent_requires_close_confirmation(activity, demand, agent.blocked);
    agent.requires_close_status_check =
        agent_requires_close_status_check(activity, demand, agent.blocked);
    // A sleeping agent keeps its group, order and read axis; only its mark
    // and word say that its process is gone until it is opened (PRD B10).
    if let Some(sleep) = &agent.sleep {
        agent.symbol = crate::agent_sleep::SLEEPING_SYMBOL.to_owned();
        agent.status_code = crate::agent_sleep::status_code(sleep);
    }
    agent.state = row_state(agent);
}

/// Re-derives every drawn value of a row whose sleep mark was just set.
pub fn rederive(agent: &mut SidebarAgentSnapshot) {
    derive_from_axes(agent);
}

/// What a row asks of the operator now, in the order the view draws its
/// groups (D-06).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestVerb {
    /// A question or an approval waits on the operator.
    Answer,
    /// A pull request's checks failed.
    Fix,
    /// A pull request is ready to review or merge.
    Review,
    /// The agent stopped before its work was done (AI `unfinished`).
    Stopped,
    /// A finished turn, or a pull request settled since the last request,
    /// the operator has not looked at.
    Result,
    Working,
    /// Its pull request's checks, its descendants, or something it named.
    Waiting,
    #[default]
    Idle,
}

pub(crate) fn verb_of(
    row: &SidebarAgentSnapshot,
    pull_requests: &[AgentPullRequestSnapshot],
    result_opened: Option<u64>,
) -> RequestVerb {
    if row.demand != "none" {
        return RequestVerb::Answer;
    }
    if row.activity == "working" {
        return RequestVerb::Working;
    }
    let duty = || {
        pull_requests
            .iter()
            .filter(|pull_request| pull_request.live && pull_request.duty)
    };
    let open = |checks: &[PullRequestChecks]| {
        duty().any(|pull_request| {
            !pull_request.badge.is_settled() && checks.contains(&pull_request.checks)
        })
    };
    if open(&[PullRequestChecks::Failed]) {
        return RequestVerb::Fix;
    }
    if open(&[
        PullRequestChecks::Passing,
        PullRequestChecks::None,
        PullRequestChecks::Unknown,
    ]) {
        return RequestVerb::Review;
    }
    // Only the label analysis reads a turn as unfinished (D-33); a row
    // without one never stops here.
    let end = row.row_facts.as_ref().and_then(|facts| facts.end);
    if end == Some(LabelEnd::Unfinished) && !row.waiting_on_descendants {
        return RequestVerb::Stopped;
    }
    if open(&[PullRequestChecks::Pending]) {
        return RequestVerb::Waiting;
    }
    // A settled pull request is a result until the operator opens it.
    let unseen = |pull_request: &AgentPullRequestSnapshot| match result_opened {
        None => true,
        Some(opened) => pull_request
            .settled_at_unix_ms
            .is_some_and(|settled| settled > opened),
    };
    if (row.completed && row.unread)
        || duty().any(|pull_request| pull_request.badge.is_settled() && unseen(pull_request))
    {
        return RequestVerb::Result;
    }
    if row.waiting_on_descendants || end == Some(LabelEnd::Waiting) {
        return RequestVerb::Waiting;
    }
    RequestVerb::Idle
}

/// The notification state machine. Transport and subscription policy stay
/// in hided; the status transition and descendant raise live with agent state.
pub mod push {
    use super::super::tally::phone::{AgentKey, Projection};
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::Duration;
    /// What a root agent's notification says, or would say.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Effective {
        NeedsYou,
        Done,
        Working,
        Seen,
        Other,
    }

    impl Effective {
        /// The state a notification announces for this one, if any.
        fn announced(self) -> Option<NoticeState> {
            match self {
                Self::NeedsYou => Some(NoticeState::NeedsYou),
                Self::Done => Some(NoticeState::Done),
                Self::Working | Self::Seen | Self::Other => None,
            }
        }

        fn from_group(group: &str) -> Self {
            match group {
                "needs_you" => Self::NeedsYou,
                "done" => Self::Done,
                "working" => Self::Working,
                "seen" => Self::Seen,
                _ => Self::Other,
            }
        }
    }

    /// The states a notification announces; the wire value is also the key of
    /// the phone's translated word for it. The last two are the human delivery
    /// causes (`herdr_core::delivery::worker::HumanNoticeKind`).
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum NoticeState {
        NeedsYou,
        Done,
        ObserverUnconfirmed,
        LetterUndelivered,
    }

    impl NoticeState {
        pub fn as_str(self) -> &'static str {
            match self {
                Self::NeedsYou => "needs_you",
                Self::Done => "done",
                Self::ObserverUnconfirmed => "observer_unconfirmed",
                Self::LetterUndelivered => "letter_undelivered",
            }
        }
    }

    /// One notification to send to every subscribed phone. It carries data only:
    /// the title and the place are the operator's own words, the state is a key
    /// the phone words in its own language.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct Notice {
        pub key: AgentKey,
        pub title: String,
        pub state: NoticeState,
        /// The project (`project`, or the part of `project · branch` before the
        /// branch separator); empty when the agent has no place.
        pub place: String,
    }

    /// Each root agent's effective state: its own group, raised to Needs You
    /// when a descendant asks for something (delegated rows stay Working or
    /// Seen themselves, docs/status-model.md), plus what its notification says.
    fn effective(projection: &Projection) -> BTreeMap<AgentKey, (Effective, String, String)> {
        let mut roots: BTreeMap<AgentKey, (Effective, String, String)> = BTreeMap::new();
        for agent in projection.agents() {
            if agent.root_pane_id == agent.pane_id {
                let place = agent
                    .place
                    .as_deref()
                    .map(|place| place.split(" · ").next().unwrap_or(place).to_owned())
                    .unwrap_or_default();
                let entry = roots.entry(agent.key()).or_insert((
                    Effective::Other,
                    String::new(),
                    String::new(),
                ));
                let raised = entry.0 == Effective::NeedsYou;
                entry.0 = if raised {
                    Effective::NeedsYou
                } else {
                    Effective::from_group(&agent.group)
                };
                entry.1 = agent.title.clone();
                entry.2 = place;
            }
        }
        for agent in projection.agents() {
            if agent.root_pane_id != agent.pane_id
                && matches!(agent.demand.as_str(), "question" | "approval" | "error")
            {
                let entry = roots.entry(agent.root_key()).or_insert((
                    Effective::Other,
                    String::new(),
                    String::new(),
                ));
                entry.0 = Effective::NeedsYou;
            }
        }
        roots
    }

    /// Follows root agents across projections and says which entered Needs You
    /// or Done, and which the desktop made Seen. The first projection it sees
    /// only seeds it: nothing that was already waiting is announced.
    #[derive(Default)]
    pub struct Transitions {
        last: Option<BTreeMap<AgentKey, Effective>>,
        /// Agents that left the list, with their state and when: a device that
        /// reconnects, or a list that was briefly empty, brings them back in the
        /// state they had, which is no transition (and no second notice).
        vanished: BTreeMap<AgentKey, (Effective, std::time::Instant)>,
    }

    /// How long a vanished agent's state is kept for its return.
    pub const VANISHED_TTL: Duration = Duration::from_secs(10 * 60);

    impl Transitions {
        pub fn reset(&mut self) {
            self.last = None;
            self.vanished.clear();
        }

        pub fn observe(&mut self, projection: &Projection) -> (Vec<Notice>, BTreeSet<AgentKey>) {
            self.observe_at(projection, std::time::Instant::now())
        }

        pub fn observe_at(
            &mut self,
            projection: &Projection,
            at: std::time::Instant,
        ) -> (Vec<Notice>, BTreeSet<AgentKey>) {
            let now = effective(projection);
            let states: BTreeMap<AgentKey, Effective> = now
                .iter()
                .map(|(key, (state, ..))| (key.clone(), *state))
                .collect();
            let Some(last) = self.last.replace(states) else {
                return (Vec::new(), BTreeSet::new());
            };
            let mut notices = Vec::new();
            let mut seen = BTreeSet::new();
            for (key, (state, title, place)) in &now {
                let before = last
                    .get(key)
                    .copied()
                    .or_else(|| self.vanished.remove(key).map(|(state, _)| state))
                    .unwrap_or(Effective::Other);
                if before == *state {
                    continue;
                }
                if let Some(announced) = state.announced() {
                    notices.push(Notice {
                        key: key.clone(),
                        title: title.clone(),
                        state: announced,
                        place: place.clone(),
                    });
                } else if *state == Effective::Seen
                    && matches!(before, Effective::NeedsYou | Effective::Done)
                {
                    seen.insert(key.clone());
                }
            }
            for (key, before) in &last {
                if !now.contains_key(key) {
                    self.vanished.insert(key.clone(), (*before, at));
                }
            }
            // An agent gone for good: its notification is closed on the next push.
            self.vanished.retain(|key, (before, since)| {
                let expired = at.duration_since(*since) >= VANISHED_TTL;
                if expired && matches!(before, Effective::NeedsYou | Effective::Done) {
                    seen.insert(key.clone());
                }
                !expired
            });
            (notices, seen)
        }
    }
}

#[cfg(test)]
mod row_tests {
    use super::*;
    use crate::sidebar::{SessionSnapshotPayload, project_agents};
    use serde_json::json;

    fn row() -> SidebarAgentSnapshot {
        let payload: SessionSnapshotPayload = serde_json::from_value(json!({
            "agents": [{"pane_id": "root", "agent": "claude", "agent_status": "idle", "state_change_seq": 1}]
        })).unwrap();
        project_agents(payload).agents.remove(0)
    }

    #[test]
    fn read_question_keeps_its_request_line_and_hue_without_operator_attention() {
        let mut agent = row();
        agent.demand = "question".into();
        agent.activity = "stopped".into();
        agent.group = "seen".into();
        agent.emphasized = false;
        agent.unread = false;
        agent.detail = Some("  진행할까요?  ".into());
        let state = row_state(&agent);
        assert!(!state.attention);
        assert_eq!(state.bucket, "resting");
        assert_eq!(state.graph_rank, 0);
        assert_eq!(state.subtree, "waiting");
        assert_eq!(state.search_tone, "attention");
        assert_eq!(
            state.line,
            Some(RowLine {
                text: "진행할까요?".into(),
                mode: "request",
                tone: Tone {
                    kind: "warning",
                    read: true
                },
            })
        );
        assert_eq!(
            state.mark_tone,
            Tone {
                kind: "warning",
                read: true
            }
        );
        // Delegation changes title treatment, not the question's hue.
        agent.delegated = true;
        agent.lineage_worktree_badge = Some("  feature  ".into());
        let child = row_state(&agent);
        assert!(!child.root);
        assert!(!child.selection_emphasizes_title);
        assert_eq!(child.branch_badge.as_deref(), Some("feature"));
        assert_eq!(child.mark_tone, state.mark_tone);
    }

    #[test]
    fn waiting_root_keeps_distinct_row_chip_and_graph_decisions() {
        let mut agent = row();
        agent.group = "working".into();
        agent.activity = "stopped".into();
        agent.emphasized = false;
        agent.unread = false;
        agent.waiting_on_descendants = true;
        let state = row_state(&agent);
        assert_eq!(state.mark_tone.kind, "working");
        assert_eq!(state.chip_tone.kind, "subtle");
        assert_eq!(state.search_tone, "muted");
        assert_eq!(state.bucket, "delegating");
        assert_eq!(state.edge, "wait");
        assert_eq!(state.graph_rank, 2);
        assert_eq!(state.link, "working");
        assert!(state.working);
        assert!(!state.title_emphasized);
        assert!(state.selection_emphasizes_title);
    }
}
