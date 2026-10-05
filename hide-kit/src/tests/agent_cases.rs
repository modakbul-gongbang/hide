//! The per-agent layer of the kit (issue #517): the adapter table, detection,
//! the skill stub and the guidance hooks, against a private HOME.

use super::*;
use crate::agents::{ADAPTERS, SKILL_NAME};

fn agent<'a>(report: &'a KitReport, id: &str) -> &'a AgentReport {
    report
        .agents
        .iter()
        .find(|agent| agent.id == id)
        .unwrap_or_else(|| panic!("no agent {id} in {report:?}"))
}

fn shared_skill(fixture: &Fixture) -> PathBuf {
    fixture
        .home()
        .join(".agents/skills")
        .join(SKILL_NAME)
        .join("SKILL.md")
}

fn gemini_settings(fixture: &Fixture) -> PathBuf {
    fixture.home().join(".gemini/settings.json")
}

fn set_up(fixture: &Fixture, folder: &str) {
    std::fs::create_dir_all(fixture.home().join(folder)).unwrap();
}

fn record(fixture: &Fixture) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(fixture.home().join(".hide/kit/installed.json")).unwrap(),
    )
    .unwrap()
}

#[test]
fn every_adapter_names_an_official_page_and_a_unique_id() {
    let mut seen = std::collections::BTreeSet::new();
    for adapter in ADAPTERS {
        assert!(seen.insert(adapter.id), "{} is listed twice", adapter.id);
        assert!(
            adapter.doc_url.starts_with("https://") && adapter.doc_url.len() > "https://".len(),
            "{} has no official page: {:?}",
            adapter.id,
            adapter.doc_url
        );
        assert!(!adapter.label.is_empty());
        assert!(
            !adapter.executables.is_empty() || !adapter.home_markers.is_empty(),
            "{} could never be detected",
            adapter.id
        );
        if let HookSupport::Guidance(guidance) = adapter.hook {
            assert_eq!(guidance.id(), adapter.id);
        }
    }
    let defaults: Vec<_> = ADAPTERS
        .iter()
        .filter(|adapter| adapter.default_on)
        .map(|adapter| adapter.id)
        .collect();
    assert_eq!(defaults, ["claude-code", "codex"]);
}

#[test]
fn an_agent_that_is_not_on_gets_nothing_until_the_operator_switches_it_on() {
    let fixture = Fixture::new();
    set_up(&fixture, ".gemini");

    let report = apply(&fixture.target, &Scope::automatic());

    let gemini = agent(&report, "gemini-cli");
    assert!(!gemini.enabled);
    assert_eq!(gemini.skill.state, ComponentState::Off);
    assert_eq!(gemini.hook.as_ref().unwrap().state, ComponentState::Off);
    assert!(!gemini_settings(&fixture).exists());
    assert!(!shared_skill(&fixture).exists());

    let report = apply(&fixture.target, &Scope::agents(["gemini-cli"], []));

    let gemini = agent(&report, "gemini-cli");
    assert!(gemini.enabled);
    assert_eq!(gemini.skill.state, ComponentState::Installed, "{gemini:?}");
    assert_eq!(
        gemini.hook.as_ref().unwrap().state,
        ComponentState::Installed,
        "{gemini:?}"
    );
    assert!(shared_skill(&fixture).is_file());
    assert!(
        std::fs::read_to_string(gemini_settings(&fixture))
            .unwrap()
            .contains("hide-guidance@1")
    );
    assert_eq!(record(&fixture)["agents"]["gemini-cli"], true);
}

#[test]
fn switching_on_an_agent_that_is_not_set_up_here_records_nothing() {
    let fixture = Fixture::new();

    let report = apply(&fixture.target, &Scope::agents(["gemini-cli"], []));

    let gemini = agent(&report, "gemini-cli");
    assert_eq!(gemini.availability, Availability::NotInstalled);
    assert!(
        record(&fixture).get("agents").is_none(),
        "{}",
        record(&fixture)
    );
    assert!(!gemini_settings(&fixture).exists());
}

#[test]
fn a_program_on_the_home_bin_folder_counts_as_the_agent_being_set_up() {
    let fixture = Fixture::new();
    executable(&fixture.home().join(".local/bin/gemini"), "#!/bin/sh\n");

    let report = status(&fixture.target);

    assert_eq!(
        agent(&report, "gemini-cli").availability,
        Availability::Available
    );
    assert_eq!(
        agent(&report, "qwen-code").availability,
        Availability::NotInstalled
    );
}

#[test]
fn a_second_apply_of_an_agent_that_is_on_changes_nothing() {
    let fixture = Fixture::new();
    set_up(&fixture, ".gemini");
    apply(&fixture.target, &Scope::agents(["gemini-cli"], []));
    let tree = home_tree(fixture.home());

    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(home_tree(fixture.home()), tree);
    assert!(agent(&report, "gemini-cli").enabled);
}

#[test]
fn removed_by_hand_stays_removed_until_the_operator_reinstalls() {
    let fixture = Fixture::new();
    set_up(&fixture, ".gemini");
    apply(&fixture.target, &Scope::agents(["gemini-cli"], []));
    std::fs::remove_file(gemini_settings(&fixture)).unwrap();
    std::fs::remove_dir_all(fixture.home().join(".agents")).unwrap();

    let report = apply(&fixture.target, &Scope::automatic());

    let gemini = agent(&report, "gemini-cli");
    assert_eq!(gemini.hook.as_ref().unwrap().state, ComponentState::Removed);
    assert_eq!(gemini.skill.state, ComponentState::Removed);
    assert!(!gemini_settings(&fixture).exists());
    assert!(!shared_skill(&fixture).exists());

    let report = apply(&fixture.target, &Scope::agents(["gemini-cli"], []));

    let gemini = agent(&report, "gemini-cli");
    assert_eq!(
        gemini.hook.as_ref().unwrap().state,
        ComponentState::Installed
    );
    assert_eq!(gemini.skill.state, ComponentState::Installed);
}

#[test]
fn an_older_stub_is_replaced_without_the_operator_asking() {
    let fixture = Fixture::new();
    set_up(&fixture, ".gemini");
    apply(&fixture.target, &Scope::agents(["gemini-cli"], []));
    let path = shared_skill(&fixture);
    let current = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, current.replace("hide-skill@1", "hide-skill@0")).unwrap();

    let before = status(&fixture.target);
    assert_eq!(
        agent(&before, "gemini-cli").skill.state,
        ComponentState::Outdated
    );
    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(
        agent(&report, "gemini-cli").skill.state,
        ComponentState::Installed
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), current);
}

#[test]
fn switching_an_agent_off_takes_only_hides_pieces_and_keeps_a_folder_another_agent_reads() {
    let fixture = Fixture::new();
    set_up(&fixture, ".gemini");
    set_up(&fixture, ".codex");
    let settings = r#"{"theme":"dark","hooks":{"SessionStart":[{"matcher":"*","hooks":[{"name":"other","type":"command","command":"/opt/other/start.sh"}]}]}}"#;
    std::fs::write(gemini_settings(&fixture), settings).unwrap();
    apply(&fixture.target, &Scope::agents(["gemini-cli"], []));
    assert!(shared_skill(&fixture).is_file());

    let report = apply(&fixture.target, &Scope::agents([], ["gemini-cli"]));

    let gemini = agent(&report, "gemini-cli");
    assert!(!gemini.enabled);
    assert_eq!(gemini.hook.as_ref().unwrap().state, ComponentState::Off);
    let left: Value =
        serde_json::from_str(&std::fs::read_to_string(gemini_settings(&fixture)).unwrap()).unwrap();
    assert_eq!(left["theme"], "dark");
    assert_eq!(left["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
    assert!(!left.to_string().contains("hide-guidance"));
    // Codex reads the shared folder and is still on.
    assert!(shared_skill(&fixture).is_file());
    assert_eq!(record(&fixture)["agents"]["gemini-cli"], false);

    // No later pass puts it back.
    apply(&fixture.target, &Scope::automatic());
    assert!(
        !std::fs::read_to_string(gemini_settings(&fixture))
            .unwrap()
            .contains("hide-guidance")
    );

    // Once no agent that reads the folder is on, the stub goes.
    apply(&fixture.target, &Scope::agents([], ["codex"]));
    assert!(!shared_skill(&fixture).exists());
}

#[test]
fn a_skill_that_hide_did_not_write_is_left_alone_and_reported() {
    let fixture = Fixture::new();
    set_up(&fixture, ".gemini");
    let own = shared_skill(&fixture);
    std::fs::create_dir_all(own.parent().unwrap()).unwrap();
    std::fs::write(&own, "---\nname: hide-browser\n---\nmine\n").unwrap();

    let report = apply(&fixture.target, &Scope::agents(["gemini-cli"], []));

    assert_eq!(
        agent(&report, "gemini-cli").skill.state,
        ComponentState::Absent
    );
    assert!(!agent(&report, "gemini-cli").needs_attention());
    assert_eq!(
        std::fs::read_to_string(&own).unwrap(),
        "---\nname: hide-browser\n---\nmine\n"
    );
    apply(&fixture.target, &Scope::agents([], ["gemini-cli", "codex"]));
    assert_eq!(
        std::fs::read_to_string(&own).unwrap(),
        "---\nname: hide-browser\n---\nmine\n"
    );
}

#[test]
fn an_agents_own_folder_is_not_created_for_its_skill() {
    let fixture = Fixture::new();
    std::fs::remove_dir_all(fixture.home().join(".claude")).unwrap();
    executable(&fixture.home().join(".local/bin/claude"), "#!/bin/sh\n");

    apply(&fixture.target, &Scope::automatic());

    assert!(!fixture.home().join(".claude").exists());
}

#[test]
fn claude_code_and_codex_are_on_without_a_choice_and_off_removes_the_hook_part() {
    let fixture = Fixture::new();

    let report = apply(&fixture.target, &Scope::automatic());

    let claude = agent(&report, "claude-code");
    assert!(claude.enabled);
    assert_eq!(
        claude.hook.as_ref().unwrap().state,
        ComponentState::Installed
    );
    assert_eq!(claude.skill.state, ComponentState::Installed);
    assert!(
        fixture
            .home()
            .join(".claude/skills")
            .join(SKILL_NAME)
            .join("SKILL.md")
            .is_file()
    );

    let report = apply(&fixture.target, &Scope::agents([], ["claude-code"]));

    let claude = agent(&report, "claude-code");
    assert!(!claude.enabled);
    assert_eq!(claude.hook.as_ref().unwrap().state, ComponentState::Off);
    assert_eq!(
        state(&report, ComponentId::ClaudeCodeHook),
        ComponentState::Off
    );
    assert!(!fixture.settings().contains("hide-subagents@"));
    assert!(fixture.settings().contains("/opt/other/notify.sh start"));
    assert!(
        !fixture
            .home()
            .join(".claude/skills")
            .join(SKILL_NAME)
            .exists()
    );

    apply(&fixture.target, &Scope::automatic());
    assert!(!fixture.settings().contains("hide-subagents@"));

    let report = apply(&fixture.target, &Scope::agents(["claude-code"], []));
    assert_eq!(
        state(&report, ComponentId::ClaudeCodeHook),
        ComponentState::Installed
    );
    assert!(fixture.settings().contains("hide-subagents@"));
}

#[test]
fn an_agent_with_a_documented_minimum_gets_the_hook_only_from_that_version() {
    let fixture = Fixture::new();
    set_up(&fixture, ".kiro");
    executable(
        &fixture.home().join(".local/bin/kiro-cli"),
        "#!/bin/sh\necho 'kiro-cli 2.9.0'\n",
    );

    let report = apply(&fixture.target, &Scope::agents(["kiro"], []));

    let kiro = agent(&report, "kiro");
    assert_eq!(kiro.skill.state, ComponentState::Installed);
    let hook = kiro.hook.as_ref().unwrap();
    // Nothing a Reinstall changes, so the row is not offered one.
    assert_eq!(hook.state, ComponentState::Absent);
    assert!(!kiro.needs_attention());
    assert!(
        hook.reason.as_deref().unwrap().contains("older than 3.0.0"),
        "{hook:?}"
    );
    assert!(
        !fixture
            .home()
            .join(".kiro/hooks/hide-guidance.json")
            .exists()
    );

    executable(
        &fixture.home().join(".local/bin/kiro-cli"),
        "#!/bin/sh\necho 'kiro-cli 3.0.1'\n",
    );
    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(
        agent(&report, "kiro").hook.as_ref().unwrap().state,
        ComponentState::Installed
    );
    assert!(
        fixture
            .home()
            .join(".kiro/hooks/hide-guidance.json")
            .is_file()
    );
}

#[test]
fn removing_a_machine_takes_every_marked_piece_and_nothing_else() {
    let fixture = Fixture::new();
    set_up(&fixture, ".gemini");
    apply(&fixture.target, &Scope::agents(["gemini-cli"], []));

    let removed = remove(&fixture.target);

    assert!(
        removed
            .agents
            .iter()
            .any(|(code, _)| code == "hook:gemini-cli")
    );
    assert!(!shared_skill(&fixture).exists());
    assert!(
        !std::fs::read_to_string(gemini_settings(&fixture))
            .map(|text| text.contains("hide-guidance"))
            .unwrap_or(false)
    );
}

#[test]
fn the_shared_stub_goes_when_the_last_agent_reading_it_is_switched_off_even_with_codex_not_set_up()
{
    let fixture = Fixture::new();
    set_up(&fixture, ".gemini");
    apply(&fixture.target, &Scope::agents(["gemini-cli"], []));
    assert!(shared_skill(&fixture).is_file());

    // Codex is on by default but not set up here, so it reads nothing.
    apply(&fixture.target, &Scope::agents([], ["gemini-cli"]));

    assert!(!shared_skill(&fixture).exists());
}

#[test]
fn a_switch_off_that_left_hides_stub_behind_does_not_read_as_off() {
    let fixture = Fixture::new();
    set_up(&fixture, ".gemini");
    apply(&fixture.target, &Scope::agents(["gemini-cli"], []));
    // The choice is off and Hide's stub is still there, as a removal that
    // failed would leave it.
    let mut text = record(&fixture);
    text["agents"]["gemini-cli"] = serde_json::json!(false);
    std::fs::write(
        fixture.home().join(".hide/kit/installed.json"),
        text.to_string(),
    )
    .unwrap();

    let report = status(&fixture.target);

    let gemini = agent(&report, "gemini-cli");
    assert_eq!(gemini.skill.state, ComponentState::Failed, "{gemini:?}");
    assert!(
        gemini
            .skill
            .reason
            .as_deref()
            .unwrap()
            .contains("switch the agent on and off again")
    );
}

#[test]
fn an_agent_found_only_by_its_program_reports_its_folder_as_not_made_yet() {
    let fixture = Fixture::new();
    std::fs::remove_dir_all(fixture.home().join(".claude")).unwrap();
    executable(&fixture.home().join(".local/bin/claude"), "#!/bin/sh\n");

    let report = apply(&fixture.target, &Scope::automatic());

    let claude = agent(&report, "claude-code");
    assert_eq!(claude.skill.state, ComponentState::Absent);
    assert!(!claude.needs_attention());
}

#[test]
fn switching_factory_droid_off_finds_its_entry_in_either_file() {
    let fixture = Fixture::new();
    set_up(&fixture, ".factory");
    std::fs::write(
        fixture.home().join(".factory/settings.json"),
        r#"{"hooks":{"SessionStart":[]}}"#,
    )
    .unwrap();
    apply(&fixture.target, &Scope::agents(["factory-droid"], []));
    let settings = fixture.home().join(".factory/settings.json");
    assert!(
        std::fs::read_to_string(&settings)
            .unwrap()
            .contains("hide-guidance")
    );
    // The operator later makes a hooks.json: Droid now reads that file.
    std::fs::write(fixture.home().join(".factory/hooks.json"), "{}").unwrap();

    apply(&fixture.target, &Scope::agents([], ["factory-droid"]));

    assert!(
        !std::fs::read_to_string(&settings)
            .unwrap()
            .contains("hide-guidance")
    );
}
