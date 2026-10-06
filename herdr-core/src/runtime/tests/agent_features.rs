//! The Partial popover's feature table is the kit's one table
//! (`hide_kit::agents::ADAPTERS`); this holds each flag to the gate in the
//! core that really decides it, so the popover cannot promise what the build
//! does not do (PRD settings-cleanup B18, D-10).

use super::*;
use hide_kit::Feature;

/// The kind Herdr names an agent by, from the kit adapter's id.
fn herdr_kind(adapter: &hide_kit::AgentAdapter) -> &'static str {
    match adapter.id {
        "claude-code" => "claude",
        "gemini-cli" => "gemini",
        id => id,
    }
}

#[test]
fn every_flag_of_the_table_is_what_the_cores_own_gates_do() {
    for adapter in hide_kit::agents::ADAPTERS {
        let kind = herdr_kind(adapter);
        let hook = crate::agent_hooks::runtime_of(kind).is_some();
        for feature in [Feature::Letters, Feature::Memory, Feature::Subagents] {
            assert_eq!(adapter.supports(feature), hook, "{kind}: {feature:?}");
        }
        assert_eq!(
            adapter.supports(Feature::Sleep),
            crate::agent_sleep::sleeps_kind(kind),
            "{kind}: sleep"
        );
        assert_eq!(
            adapter.supports(Feature::Fork),
            crate::fork::ForkableAgent::parse(kind).is_some(),
            "{kind}: fork"
        );
        assert_eq!(
            adapter.supports(Feature::Start),
            crate::model::AGENT_KINDS.contains(&kind),
            "{kind}: start from Hide"
        );
        assert_eq!(
            adapter.supports(Feature::Titles),
            conversation_agent_kind(kind),
            "{kind}: conversation titles"
        );
        // Herdr's own integration is the pinned target list the kit installs
        // from, and a status judged from the screen has none (B15).
        assert_eq!(
            adapter.supports(Feature::HerdrIntegration),
            adapter.herdr.is_some(),
            "{kind}: herdr integration"
        );
    }
}

#[test]
fn the_snapshot_row_carries_the_chip_and_the_table_in_order() {
    let piece = hide_kit::PieceReport {
        state: hide_kit::ComponentState::Installed,
        reason: None,
        location: None,
    };
    let row = |id: &str| hide_kit::AgentReport {
        id: id.to_owned(),
        label: id.to_owned(),
        availability: hide_kit::Availability::Available,
        enabled: false,
        skill: piece.clone(),
        hook: None,
        herdr: None,
        doc_url: String::new(),
    };
    let report = hide_kit::KitReport {
        agents: vec![row("claude-code"), row("gemini-cli")],
        ..Default::default()
    };
    let kit = crate::model::KitSnapshot::from_report(&report);

    let claude = &kit.agents[0];
    assert!(!claude.partial);
    assert!(claude.features.iter().all(|feature| feature.supported));
    let gemini = &kit.agents[1];
    assert!(
        gemini.partial,
        "the chip shows whether or not the agent is on"
    );
    assert_eq!(
        gemini
            .features
            .iter()
            .filter(|feature| feature.supported)
            .map(|feature| feature.id)
            .collect::<Vec<_>>(),
        [Feature::Skill, Feature::Guidance],
        "Gemini CLI has no Herdr integration, so its status is judged from the screen (B15)"
    );
    assert_eq!(
        gemini
            .features
            .iter()
            .map(|feature| feature.id)
            .collect::<Vec<_>>(),
        Feature::ALL
    );
}
