use std::cmp::Reverse;
use std::collections::BTreeMap;

use crate::domain::{AgentPhase, AgentProjection, ConnectionState, DomainProjection};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentLogo {
    Codex,
    Claude,
    Neutral,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentPresentation {
    pub stable_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub logo: AgentLogo,
    pub phase: AgentPhase,
    pub summary: String,
    pub elapsed_seconds: u64,
    pub host: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub pane_id: String,
}

#[derive(Clone, Debug, Default)]
pub struct AgentPresentationStore {
    agents: BTreeMap<String, AgentPresentation>,
}

impl AgentPresentationStore {
    pub fn rebuild(&mut self, projection: &DomainProjection) {
        self.agents = projection
            .agents()
            .map(|agent| (agent.agent_instance_id.clone(), present_agent(agent)))
            .collect();
    }

    pub fn get(&self, stable_id: &str) -> Option<&AgentPresentation> {
        self.agents.get(stable_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &AgentPresentation> {
        self.agents.values()
    }

    pub fn prioritized(&self) -> Vec<&AgentPresentation> {
        let mut agents = self.agents.values().collect::<Vec<_>>();
        agents.sort_by_key(|agent| {
            (
                phase_priority(agent.phase),
                Reverse(agent.elapsed_seconds),
                agent.stable_id.as_str(),
            )
        });
        agents
    }
}

impl AgentLogo {
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Neutral => "Agent",
        }
    }

    pub fn marker(self) -> char {
        match self {
            Self::Codex => '◈',
            Self::Claude => '◆',
            Self::Neutral => '◇',
        }
    }
}

pub fn phase_label(phase: AgentPhase) -> &'static str {
    match phase {
        AgentPhase::Error => "error",
        AgentPhase::Attention => "attention",
        AgentPhase::Working => "working",
        AgentPhase::Idle => "idle",
        AgentPhase::Ended => "ended",
    }
}

pub fn connection_label(connection: &ConnectionState) -> String {
    match connection {
        ConnectionState::Connected => "Connected".to_owned(),
        ConnectionState::Reconnecting { target } => format!("Reconnecting to {target}"),
        ConnectionState::Stale { expected, received } => {
            format!("Stale: expected event {expected}, received {received}; resync required")
        }
        ConnectionState::Failed { reason } => format!("Failed: {reason}"),
        ConnectionState::ActionRequired { reason } => format!("Action required: {reason}"),
    }
}

fn present_agent(agent: &AgentProjection) -> AgentPresentation {
    AgentPresentation {
        stable_id: agent.agent_instance_id.clone(),
        parent_id: agent.parent_agent_instance_id.clone(),
        name: agent.name.clone(),
        logo: match agent.kind.trim().to_ascii_lowercase().as_str() {
            "codex" | "openai-codex" => AgentLogo::Codex,
            "claude" | "claude-code" => AgentLogo::Claude,
            _ => AgentLogo::Neutral,
        },
        phase: agent.phase,
        summary: agent
            .summary
            .clone()
            .unwrap_or_else(|| "Summary unavailable".to_owned()),
        elapsed_seconds: agent.elapsed_seconds,
        host: agent.host.host_id.clone(),
        workspace_id: agent.workspace_id.clone(),
        tab_id: agent.tab_id.clone(),
        pane_id: agent.pane_id.clone(),
    }
}

fn phase_priority(phase: AgentPhase) -> u8 {
    match phase {
        AgentPhase::Error => 0,
        AgentPhase::Attention => 1,
        AgentPhase::Working => 2,
        AgentPhase::Idle => 3,
        AgentPhase::Ended => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgentProjection, DomainProjection, HostScope};

    fn agent(id: &str, kind: &str, phase: AgentPhase) -> AgentProjection {
        AgentProjection {
            agent_instance_id: id.to_owned(),
            parent_agent_instance_id: None,
            host: HostScope {
                host_id: "local".to_owned(),
                session_id: "fixture".to_owned(),
            },
            workspace_id: "workspace".to_owned(),
            tab_id: "tab".to_owned(),
            pane_id: format!("pane-{id}"),
            name: id.to_owned(),
            kind: kind.to_owned(),
            phase,
            summary: None,
            elapsed_seconds: 3,
        }
    }

    fn projection(agents: Vec<AgentProjection>) -> DomainProjection {
        let mut projection = DomainProjection::default();
        let mut snapshot = crate::domain::fixture_snapshot(1);
        snapshot.agents = agents;
        projection.apply_snapshot(snapshot).unwrap();
        projection
    }

    #[test]
    fn authoritative_kind_selects_one_logo_and_unknown_is_neutral() {
        let projection = projection(vec![
            agent("a", "codex", AgentPhase::Working),
            agent("b", "claude-code", AgentPhase::Attention),
            agent("c", "custom", AgentPhase::Error),
        ]);
        let mut store = AgentPresentationStore::default();
        store.rebuild(&projection);
        assert_eq!(store.get("a").unwrap().logo, AgentLogo::Codex);
        assert_eq!(store.get("b").unwrap().logo, AgentLogo::Claude);
        assert_eq!(store.get("c").unwrap().logo, AgentLogo::Neutral);
        assert_eq!(store.get("c").unwrap().summary, "Summary unavailable");
    }

    #[test]
    fn shared_priority_is_error_attention_working_idle_ended() {
        let projection = projection(vec![
            agent("idle", "codex", AgentPhase::Idle),
            agent("working", "codex", AgentPhase::Working),
            agent("error", "codex", AgentPhase::Error),
            agent("attention", "codex", AgentPhase::Attention),
            agent("ended", "codex", AgentPhase::Ended),
        ]);
        let mut store = AgentPresentationStore::default();
        store.rebuild(&projection);
        assert_eq!(
            store
                .prioritized()
                .into_iter()
                .map(|agent| agent.phase)
                .collect::<Vec<_>>(),
            vec![
                AgentPhase::Error,
                AgentPhase::Attention,
                AgentPhase::Working,
                AgentPhase::Idle,
                AgentPhase::Ended,
            ]
        );
    }
}
