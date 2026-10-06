//! The one-time retirement of the thirteen agents Hide stopped supporting
//! (PRD settings-cleanup D-06): only what the kit's record names and what
//! carries Hide's marker is taken, and a second pass changes nothing.

use super::*;
use crate::agents::{SKILL_NAME, skill_text};

const HOOK_COMMAND: &str = "[ -x /kit/hide-agent-hooks ] && /kit/hide-agent-hooks hook --runtime qwen-code --event SessionStart --source hide-guidance@1 || true";

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn qwen_settings(fixture: &Fixture) -> PathBuf {
    fixture.home().join(".qwen/settings.json")
}

fn qwen_skill(fixture: &Fixture) -> PathBuf {
    fixture
        .home()
        .join(".qwen/skills")
        .join(SKILL_NAME)
        .join("SKILL.md")
}

/// A machine an earlier build put Qwen Code on: its hook entry beside the
/// operator's own, its own skill stub, and the record that says so.
fn qwen_as_an_earlier_build_left_it(fixture: &Fixture) {
    write(
        &qwen_settings(fixture),
        &serde_json::json!({
            "theme": "dark",
            "hooks": { "SessionStart": [
                { "hooks": [{ "type": "command", "command": "/opt/mine.sh" }] },
                { "hooks": [{
                    "name": "hide-guidance",
                    "type": "command",
                    "command": HOOK_COMMAND,
                    "timeout": 8
                }] }
            ] }
        })
        .to_string(),
    );
    write(&qwen_skill(fixture), &skill_text());
    write(
        &fixture.home().join(".hide/kit/installed.json"),
        r#"{"format":1,"installed":["hook:qwen-code","skill:qwen"],"agents":{"qwen-code":true}}"#,
    );
}

fn record(fixture: &Fixture) -> String {
    std::fs::read_to_string(fixture.home().join(".hide/kit/installed.json")).unwrap()
}

#[test]
fn a_retired_agents_hide_marked_pieces_are_taken_and_the_operators_stay() {
    let fixture = Fixture::new();
    qwen_as_an_earlier_build_left_it(&fixture);

    let report = apply(&fixture.target, &Scope::automatic());

    let settings = std::fs::read_to_string(qwen_settings(&fixture)).unwrap();
    assert!(!settings.contains("hide-guidance"), "{settings}");
    assert!(settings.contains("/opt/mine.sh"), "{settings}");
    assert!(settings.contains("dark"), "{settings}");
    assert!(!qwen_skill(&fixture).exists());
    assert!(
        report
            .legacy_retirement
            .removed
            .iter()
            .any(|what| what.contains("Qwen Code")),
        "{report:?}"
    );
    assert!(report.legacy_retirement.failures.is_empty(), "{report:?}");
    let record = record(&fixture);
    assert!(!record.contains("qwen"), "{record}");
    // A retired agent is not one Settings lists.
    assert!(report.agents.iter().all(|agent| agent.id != "qwen-code"));
}

#[test]
fn a_second_pass_changes_nothing_and_reports_nothing() {
    let fixture = Fixture::new();
    qwen_as_an_earlier_build_left_it(&fixture);
    apply(&fixture.target, &Scope::automatic());
    let tree = home_tree(fixture.home());

    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(home_tree(fixture.home()), tree);
    assert!(report.legacy_retirement.is_empty(), "{report:?}");
}

#[test]
fn an_agent_the_record_never_named_is_not_touched() {
    let fixture = Fixture::new();
    // The operator's own Qwen Code files, and no record of Hide having put
    // anything there.
    qwen_as_an_earlier_build_left_it(&fixture);
    write(
        &fixture.home().join(".hide/kit/installed.json"),
        r#"{"format":1,"installed":[]}"#,
    );
    let own = |fixture: &Fixture| {
        home_tree(fixture.home())
            .into_iter()
            .filter(|(path, _)| path.starts_with(".qwen"))
            .collect::<Vec<_>>()
    };
    let tree = own(&fixture);
    assert!(!tree.is_empty());

    apply(&fixture.target, &Scope::automatic());

    assert_eq!(own(&fixture), tree);
}

#[test]
fn a_skill_the_operator_wrote_over_is_kept_even_when_the_record_names_it() {
    let fixture = Fixture::new();
    qwen_as_an_earlier_build_left_it(&fixture);
    write(
        &qwen_skill(&fixture),
        "---\nname: hide-browser\n---\nmine\n",
    );

    apply(&fixture.target, &Scope::automatic());

    assert_eq!(
        std::fs::read_to_string(qwen_skill(&fixture)).unwrap(),
        "---\nname: hide-browser\n---\nmine\n"
    );
    assert!(!record(&fixture).contains("qwen"));
}

#[test]
fn a_hook_file_that_does_not_parse_stays_and_is_tried_again() {
    let fixture = Fixture::new();
    qwen_as_an_earlier_build_left_it(&fixture);
    write(&qwen_settings(&fixture), "{ not json");

    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(
        std::fs::read_to_string(qwen_settings(&fixture)).unwrap(),
        "{ not json"
    );
    assert!(!report.legacy_retirement.failures.is_empty(), "{report:?}");
    assert!(record(&fixture).contains("hook:qwen-code"));

    // Fixed by the operator, the next pass finishes.
    write(
        &qwen_settings(&fixture),
        &serde_json::json!({ "hooks": { "SessionStart": [{ "hooks": [{
            "name": "hide-guidance", "type": "command", "command": HOOK_COMMAND
        }] }] } })
        .to_string(),
    );
    let report = apply(&fixture.target, &Scope::automatic());
    assert!(report.legacy_retirement.failures.is_empty(), "{report:?}");
    assert!(!record(&fixture).contains("qwen"));
}

#[test]
fn the_shared_stub_stays_while_a_supported_agent_still_reads_it() {
    let fixture = Fixture::new();
    executable(&fixture.home().join(".local/bin/gemini"), "#!/bin/sh\n");
    std::fs::create_dir_all(fixture.home().join(".gemini")).unwrap();
    apply(&fixture.target, &Scope::agents(["gemini-cli"], []));
    let shared = fixture
        .home()
        .join(".agents/skills")
        .join(SKILL_NAME)
        .join("SKILL.md");
    assert!(shared.is_file());
    // An earlier build also had Amp on, which read the same folder.
    let mut text: serde_json::Value = serde_json::from_str(&record(&fixture)).unwrap();
    text["agents"]["amp"] = serde_json::json!(true);
    write(
        &fixture.home().join(".hide/kit/installed.json"),
        &text.to_string(),
    );

    apply(&fixture.target, &Scope::automatic());

    assert!(shared.is_file(), "Gemini CLI still reads it");
    assert!(!record(&fixture).contains("amp"));
}

#[test]
fn the_shared_stub_goes_with_the_last_retired_agent_that_read_it() {
    let fixture = Fixture::new();
    let shared = fixture
        .home()
        .join(".agents/skills")
        .join(SKILL_NAME)
        .join("SKILL.md");
    write(&shared, &skill_text());
    write(
        &fixture.home().join(".hide/kit/installed.json"),
        r#"{"format":1,"installed":["skill:agents"],"agents":{"amp":true,"goose":true}}"#,
    );

    apply(&fixture.target, &Scope::automatic());

    assert!(!shared.exists());
    let record = record(&fixture);
    assert!(
        !record.contains("amp") && !record.contains("goose"),
        "{record}"
    );
}

#[test]
fn removing_the_kit_takes_a_retired_agents_marked_hook_too() {
    let fixture = Fixture::new();
    qwen_as_an_earlier_build_left_it(&fixture);

    remove(&fixture.target);

    let settings = std::fs::read_to_string(qwen_settings(&fixture)).unwrap();
    assert!(!settings.contains("hide-guidance"), "{settings}");
    assert!(settings.contains("/opt/mine.sh"));
    assert!(!qwen_skill(&fixture).exists());
}
