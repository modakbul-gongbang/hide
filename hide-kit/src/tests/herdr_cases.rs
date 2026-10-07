//! Herdr's own integration for an agent (PRD settings-cleanup D-13), against
//! a fake `herdr` that keeps each integration's state in a file: installed
//! when the agent is switched on, taken out only when the record says Hide
//! installed it.

use super::*;

fn agent<'a>(report: &'a KitReport, id: &str) -> &'a AgentReport {
    report
        .agents
        .iter()
        .find(|agent| agent.id == id)
        .unwrap_or_else(|| panic!("no agent {id} in {report:?}"))
}

fn herdr_state(report: &KitReport, id: &str) -> ComponentState {
    agent(report, id)
        .herdr
        .as_ref()
        .unwrap_or_else(|| panic!("{id} has no Herdr piece"))
        .state
}

fn record(fixture: &Fixture) -> String {
    std::fs::read_to_string(fixture.home().join(".hide/kit/installed.json")).unwrap()
}

/// The `herdr integration install|uninstall` calls made for one target; the
/// automatic pass also settles Claude Code's, which these tests do not look at.
fn changes_for(fixture: &Fixture, target: &str) -> Vec<String> {
    fixture
        .integration_changes()
        .into_iter()
        .filter(|call| call.ends_with(&format!(" {target}")))
        .collect()
}

/// An agent installed the way its installer leaves it: its program and its
/// own folder.
fn install(fixture: &Fixture, program: &str, folder: &str) {
    executable(
        &fixture.home().join(".local/bin").join(program),
        "#!/bin/sh\n",
    );
    std::fs::create_dir_all(fixture.home().join(folder)).unwrap();
}

#[test]
fn switching_an_agent_on_installs_its_integration_and_off_takes_it_out() {
    let fixture = Fixture::new();
    install(&fixture, "pi", ".pi/agent");

    let report = apply(&fixture.target, &Scope::agents(["pi"], []));

    assert_eq!(herdr_state(&report, "pi"), ComponentState::Installed);
    assert_eq!(fixture.integration("pi"), "current");
    assert!(record(&fixture).contains("herdr:pi"));
    assert_eq!(changes_for(&fixture, "pi"), ["integration install pi"]);

    let report = apply(&fixture.target, &Scope::agents([], ["pi"]));

    assert_eq!(herdr_state(&report, "pi"), ComponentState::Off);
    assert_eq!(fixture.integration("pi"), "none");
    assert!(!record(&fixture).contains("herdr:pi"));
}

#[test]
fn a_pass_that_finds_the_integration_in_place_calls_nothing() {
    let fixture = Fixture::new();
    install(&fixture, "pi", ".pi/agent");
    apply(&fixture.target, &Scope::agents(["pi"], []));
    let calls = fixture.integration_changes();
    let tree = home_tree(fixture.home());

    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(fixture.integration_changes(), calls);
    assert_eq!(home_tree(fixture.home()), tree);
    assert_eq!(herdr_state(&report, "pi"), ComponentState::Installed);
}

#[test]
fn an_integration_the_operator_installed_is_kept_on_and_off() {
    let fixture = Fixture::new();
    install(&fixture, "pi", ".pi/agent");
    fixture.operator_installed("pi", "current");

    let report = apply(&fixture.target, &Scope::agents(["pi"], []));

    assert_eq!(herdr_state(&report, "pi"), ComponentState::Installed);
    assert!(!record(&fixture).contains("herdr:pi"));
    assert!(changes_for(&fixture, "pi").is_empty());

    apply(&fixture.target, &Scope::agents([], ["pi"]));

    assert_eq!(fixture.integration("pi"), "current");
    assert!(changes_for(&fixture, "pi").is_empty());
}

#[test]
fn an_older_integration_is_brought_up_to_date_only_when_hide_installed_it() {
    let fixture = Fixture::new();
    install(&fixture, "pi", ".pi/agent");
    install(&fixture, "opencode", ".config/opencode");
    apply(&fixture.target, &Scope::agents(["pi"], []));
    fixture.operator_installed("pi", "outdated");
    fixture.operator_installed("opencode", "outdated");

    let report = apply(&fixture.target, &Scope::agents(["opencode"], []));

    // Hide's own is replaced; the operator's older one is left as it is.
    assert_eq!(fixture.integration("pi"), "current");
    assert_eq!(fixture.integration("opencode"), "outdated");
    assert_eq!(herdr_state(&report, "opencode"), ComponentState::Installed);
}

#[test]
fn an_integration_taken_out_by_hand_stays_out_until_reinstall() {
    let fixture = Fixture::new();
    install(&fixture, "pi", ".pi/agent");
    apply(&fixture.target, &Scope::agents(["pi"], []));
    fixture.operator_installed("pi", "current");
    std::fs::remove_file(fixture.root.join("herdr-fake/pi")).unwrap();
    let before = fixture.integration_changes().len();

    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(herdr_state(&report, "pi"), ComponentState::Removed);
    assert_eq!(fixture.integration_changes().len(), before);

    let report = apply(&fixture.target, &Scope::agents(["pi"], []));

    assert_eq!(herdr_state(&report, "pi"), ComponentState::Installed);
    assert_eq!(fixture.integration("pi"), "current");
}

/// omp is a row like Pi: Herdr's `omp` target installs into the agent's own
/// `~/.omp/agent`, and neither Hide nor Herdr makes that folder (B6, B7).
#[test]
fn omp_gets_herdrs_omp_integration_in_its_own_folder_and_loses_it_when_switched_off() {
    let fixture = Fixture::new();
    executable(&fixture.home().join(".local/bin/omp"), "#!/bin/sh\n");

    let report = apply(&fixture.target, &Scope::agents(["omp"], []));

    let omp = agent(&report, "omp");
    let piece = omp.herdr.as_ref().unwrap();
    assert_eq!(piece.state, ComponentState::Absent, "{omp:?}");
    assert!(
        piece
            .reason
            .as_deref()
            .unwrap()
            .contains("omp has not created its own folder"),
        "{piece:?}"
    );
    assert!(!omp.needs_attention(), "{omp:?}");
    assert!(changes_for(&fixture, "omp").is_empty());
    assert!(
        !fixture.home().join(".omp").exists(),
        "Hide makes no omp folder"
    );
    // The skill does not wait for omp's folder: it is the shared one.
    assert_eq!(omp.skill.state, ComponentState::Installed, "{omp:?}");

    std::fs::create_dir_all(fixture.home().join(".omp/agent")).unwrap();
    let report = apply(&fixture.target, &Scope::automatic());

    let piece = agent(&report, "omp").herdr.clone().unwrap();
    assert_eq!(piece.state, ComponentState::Installed);
    assert_eq!(
        piece.location.as_deref(),
        Some(
            fixture
                .home()
                .join(".omp/agent")
                .display()
                .to_string()
                .as_str()
        )
    );
    assert_eq!(fixture.integration("omp"), "current");
    assert!(record(&fixture).contains("herdr:omp"));
    assert_eq!(changes_for(&fixture, "omp"), ["integration install omp"]);

    let report = apply(&fixture.target, &Scope::agents([], ["omp"]));

    assert_eq!(herdr_state(&report, "omp"), ComponentState::Off);
    assert_eq!(fixture.integration("omp"), "none");
    assert!(!record(&fixture).contains("herdr:omp"));
}

#[test]
fn a_failed_install_fails_that_agents_row_only_and_the_next_pass_converges() {
    let fixture = Fixture::new();
    apply(&fixture.target, &Scope::automatic());
    install(&fixture, "pi", ".pi/agent");
    install(&fixture, "omp", ".omp/agent");
    std::fs::write(fixture.home().join("herdr-fails"), "").unwrap();

    let report = apply(&fixture.target, &Scope::agents(["pi"], []));

    let pi = agent(&report, "pi");
    let piece = pi.herdr.as_ref().unwrap();
    assert_eq!(piece.state, ComponentState::Failed);
    assert!(piece.reason.as_deref().unwrap().contains("disk full"));
    assert!(pi.needs_attention());
    // The other rows are whole, and the agent's skill went in regardless.
    assert_eq!(pi.skill.state, ComponentState::Installed);
    assert_eq!(
        herdr_state(&report, "claude-code"),
        ComponentState::Installed
    );
    // An agent that is off is not asked for, so the failure is not its.
    assert!(!agent(&report, "omp").needs_attention());
    assert!(changes_for(&fixture, "omp").is_empty());
    assert!(!record(&fixture).contains("herdr:pi"));

    std::fs::remove_file(fixture.home().join("herdr-fails")).unwrap();
    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(herdr_state(&report, "pi"), ComponentState::Installed);
    assert!(record(&fixture).contains("herdr:pi"));
}

#[test]
fn an_agent_that_has_not_made_its_folder_yet_gets_the_integration_once_it_has() {
    let fixture = Fixture::new();
    executable(&fixture.home().join(".local/bin/pi"), "#!/bin/sh\n");

    let report = apply(&fixture.target, &Scope::agents(["pi"], []));

    assert_eq!(herdr_state(&report, "pi"), ComponentState::Absent);
    assert!(!agent(&report, "pi").needs_attention());
    assert_eq!(fixture.integration("pi"), "none");

    std::fs::create_dir_all(fixture.home().join(".pi/agent")).unwrap();
    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(herdr_state(&report, "pi"), ComponentState::Installed);
}

#[test]
fn a_machine_with_no_herdr_cli_says_so_on_the_row_and_changes_nothing() {
    let mut fixture = Fixture::new();
    fixture.target.herdr_bin = None;
    install(&fixture, "pi", ".pi/agent");

    let report = apply(&fixture.target, &Scope::agents(["pi"], []));

    let piece = agent(&report, "pi").herdr.as_ref().unwrap();
    assert_eq!(piece.state, ComponentState::Absent);
    assert!(
        piece
            .reason
            .as_deref()
            .unwrap()
            .contains("Herdr is not found")
    );
    assert!(!record(&fixture).contains("herdr:pi"));
}

#[test]
fn removing_the_kit_takes_only_the_integrations_the_record_names() {
    let fixture = Fixture::new();
    install(&fixture, "pi", ".pi/agent");
    install(&fixture, "opencode", ".config/opencode");
    fixture.operator_installed("opencode", "current");
    apply(&fixture.target, &Scope::agents(["pi", "opencode"], []));
    assert_eq!(fixture.integration("pi"), "current");

    let removed = remove(&fixture.target);

    assert!(
        removed
            .agents
            .iter()
            .any(|(code, outcome)| code == "herdr:pi" && *outcome == RemoveOutcome::Removed),
        "{removed:?}"
    );
    assert_eq!(fixture.integration("pi"), "none");
    assert_eq!(fixture.integration("opencode"), "current");
}
