use crate::model::{AgentSnapshot, AgentStatus};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AggregateSummary {
    pub attention: usize,
    pub error: usize,
    pub working: usize,
    pub done: usize,
    pub idle: usize,
    pub disconnected: usize,
}

impl AggregateSummary {
    pub fn total(&self) -> usize {
        self.attention + self.error + self.working + self.done + self.idle + self.disconnected
    }

    pub fn top_status(&self) -> TopStatus {
        // Questions and errors need a visible response even while another
        // agent keeps working. The forge look is only for calm, active work.
        if self.error > 0 {
            TopStatus::Error
        } else if self.attention > 0 {
            TopStatus::Attention
        } else if self.working > 0 {
            TopStatus::Working
        } else if self.done > 0 {
            TopStatus::Done
        } else if self.idle > 0 {
            TopStatus::Idle
        } else {
            TopStatus::Disconnected
        }
    }

    pub fn badge_count(&self, dnd: bool) -> usize {
        if dnd { 0 } else { self.attention + self.error }
    }
}

pub fn summarize(agents: &[AgentSnapshot]) -> AggregateSummary {
    let mut summary = AggregateSummary::default();
    for agent in agents {
        match agent.status {
            AgentStatus::Attention => summary.attention += 1,
            AgentStatus::Error => summary.error += 1,
            AgentStatus::Working => summary.working += 1,
            AgentStatus::Done => summary.done += 1,
            AgentStatus::Idle => summary.idle += 1,
            AgentStatus::Disconnected => summary.disconnected += 1,
        }
    }
    summary
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TopStatus {
    #[default]
    Disconnected,
    Idle,
    Working,
    Done,
    Error,
    Attention,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AttentionLevel {
    #[default]
    None,
    Fresh,
    Escalated1,
    Escalated2,
    Escalated3,
}

#[derive(Default)]
pub struct UrgentTransition {
    previous: HashMap<String, AgentStatus>,
    initialized: bool,
}

impl UrgentTransition {
    pub fn sound_cue(&mut self, agents: &[AgentSnapshot], dnd: bool) -> bool {
        let current = agents
            .iter()
            .filter(|agent| matches!(agent.status, AgentStatus::Attention | AgentStatus::Error))
            .map(|agent| (agent.id.clone(), agent.status.clone()))
            .collect::<HashMap<_, _>>();
        let added = self.initialized
            && current
                .iter()
                .any(|(id, status)| self.previous.get(id) != Some(status));
        self.previous = current;
        self.initialized = true;
        !dnd && added
    }
}

impl AttentionLevel {
    pub fn asset_name(self) -> &'static str {
        match self {
            Self::None => "idle",
            Self::Fresh => "attention-0",
            Self::Escalated1 => "attention-1",
            Self::Escalated2 => "attention-2",
            Self::Escalated3 => "attention-3",
        }
    }
}

pub trait Clock {
    fn now_ms(&self) -> i64;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }
}

pub struct EscalationEngine<C: Clock> {
    clock: C,
    attention_since: HashMap<String, i64>,
    dnd: bool,
}

impl<C: Clock> EscalationEngine<C> {
    pub fn new(clock: C) -> Self {
        Self {
            clock,
            attention_since: HashMap::new(),
            dnd: false,
        }
    }

    pub fn set_dnd(&mut self, enabled: bool) {
        self.dnd = enabled;
    }

    pub fn level(&mut self, agents: &[AgentSnapshot]) -> AttentionLevel {
        let now = self.clock.now_ms();
        let waiting: Vec<&AgentSnapshot> = agents.iter().filter(|a| a.waiting()).collect();
        let waiting_ids: std::collections::HashSet<&str> =
            waiting.iter().map(|a| a.id.as_str()).collect();
        self.attention_since
            .retain(|id, _| waiting_ids.contains(id.as_str()));
        for agent in &waiting {
            self.attention_since
                .entry(agent.id.clone())
                .or_insert(now - agent.elapsed_seconds as i64 * 1000);
        }
        if waiting.is_empty() || self.dnd {
            return AttentionLevel::None;
        }
        let threshold = if waiting.len() >= 3 {
            [60_000, 300_000, 900_000]
        } else {
            [120_000, 600_000, 1_800_000]
        };
        let oldest_ms = waiting
            .iter()
            .filter_map(|a| self.attention_since.get(&a.id))
            .map(|since| now - since)
            .max()
            .unwrap_or(0);
        if oldest_ms >= threshold[2] {
            AttentionLevel::Escalated3
        } else if oldest_ms >= threshold[1] {
            AttentionLevel::Escalated2
        } else if oldest_ms >= threshold[0] {
            AttentionLevel::Escalated1
        } else {
            AttentionLevel::Fresh
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentSnapshot, QuestionPayload};

    #[derive(Clone)]
    struct FakeClock(i64);
    impl Clock for FakeClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    fn waiting(id: &str, elapsed_seconds: u64) -> AgentSnapshot {
        AgentSnapshot {
            id: id.into(),
            target_id: "local".into(),
            target_label: "Local".into(),
            project: "pet".into(),
            agent_kind: "codex".into(),
            cwd: "/tmp".into(),
            summary: "waiting".into(),
            status: AgentStatus::Attention,
            elapsed_seconds,
            pane_id: None,
            tab_id: None,
            workspace_id: None,
            focused: false,
            session_id: None,
            updated_at_ms: 0,
            question: Some(QuestionPayload {
                question: "Choose".into(),
                options: vec![],
                allow_free_text: true,
            }),
            disconnected_reason: None,
            ambient: None,
            ambient_compat_warning: false,
        }
    }

    #[test]
    fn urgent_states_take_priority_over_working() {
        let mut a = waiting("a", 0);
        a.status = AgentStatus::Attention;
        let mut e = a.clone();
        e.id = "e".into();
        e.status = AgentStatus::Error;
        e.question = None;
        let mut w = a.clone();
        w.id = "w".into();
        w.status = AgentStatus::Working;
        w.question = None;
        let summary = summarize(&[a, e, w]);
        assert_eq!(summary.top_status(), TopStatus::Error);
        assert_eq!(summary.error, 1);
        let mut finished = summary;
        finished.error = 0;
        assert_eq!(finished.top_status(), TopStatus::Attention);
        finished.attention = 0;
        finished.working = 0;
        assert_eq!(finished.top_status(), TopStatus::Disconnected);
    }

    #[test]
    fn disconnected_agents_are_retained_but_never_counted_as_waiting() {
        let mut offline = waiting("offline", 0);
        offline.status = AgentStatus::Disconnected;
        offline.question = None;
        let summary = summarize(&[offline.clone()]);
        assert_eq!(summary.disconnected, 1);
        assert_eq!(summary.attention, 0);
        assert!(!offline.waiting());
        assert!(!offline.status.is_countable());
    }

    #[test]
    fn badge_count_includes_attention_and_error_but_dnd_hides_it() {
        let mut waiting_agent = waiting("waiting", 0);
        let mut error = waiting("error", 0);
        error.status = AgentStatus::Error;
        error.question = None;
        let summary = summarize(&[waiting_agent.clone(), error]);
        assert_eq!(summary.badge_count(false), 2);
        assert_eq!(summary.badge_count(true), 0);
        waiting_agent.status = AgentStatus::Working;
        assert_eq!(summarize(&[waiting_agent]).badge_count(false), 0);
    }

    #[test]
    fn urgent_transition_only_cues_after_the_initial_snapshot() {
        let mut tracker = UrgentTransition::default();
        let idle = waiting("agent", 0);
        assert!(!tracker.sound_cue(&[idle.clone()], false));

        let mut active = idle.clone();
        active.status = AgentStatus::Error;
        active.question = None;
        assert!(tracker.sound_cue(&[active.clone()], false));
        assert!(!tracker.sound_cue(&[active.clone()], false));
        assert!(!tracker.sound_cue(&[active], true));
    }

    #[test]
    fn normal_escalation_reaches_each_two_ten_thirty_minute_stage() {
        for (elapsed, expected) in [
            (0, AttentionLevel::Fresh),
            (120, AttentionLevel::Escalated1),
            (600, AttentionLevel::Escalated2),
            (1800, AttentionLevel::Escalated3),
        ] {
            let now = elapsed * 1000;
            let mut engine = EscalationEngine::new(FakeClock(now));
            assert_eq!(engine.level(&[waiting("a", elapsed as u64)]), expected);
        }
    }

    #[test]
    fn three_waiting_agents_use_half_thresholds() {
        for (elapsed, expected) in [
            (60, AttentionLevel::Escalated1),
            (300, AttentionLevel::Escalated2),
            (900, AttentionLevel::Escalated3),
        ] {
            let now = elapsed * 1000;
            let mut engine = EscalationEngine::new(FakeClock(now));
            let agents = vec![
                waiting("a", elapsed as u64),
                waiting("b", elapsed as u64),
                waiting("c", elapsed as u64),
            ];
            assert_eq!(engine.level(&agents), expected);
        }
    }

    #[test]
    fn escalation_uses_half_threshold_for_three_waiting_agents_and_caps() {
        let mut engine = EscalationEngine::new(FakeClock(901_000));
        let agents = vec![waiting("a", 901), waiting("b", 901), waiting("c", 901)];
        assert_eq!(engine.level(&agents), AttentionLevel::Escalated3);
        engine.set_dnd(true);
        assert_eq!(engine.level(&agents), AttentionLevel::None);
    }
}
