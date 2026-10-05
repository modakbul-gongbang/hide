//! Each agent's file is pinned to the example its own documentation gives
//! (`docs/agent-hooks.md`, the per-agent table). Every test builds its own
//! `HOME`; the operator's real files are never a target.

use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::*;

struct Fixture {
    home: tempfile::TempDir,
    helper: PathBuf,
}

impl Fixture {
    fn new(agent: GuidanceAgent) -> Self {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(agent.home_directory(home.path())).unwrap();
        let helper = home
            .path()
            .join("hide.app/Contents/Resources/hide-agent-hooks");
        fs::create_dir_all(helper.parent().unwrap()).unwrap();
        fs::write(&helper, b"binary").unwrap();
        Self { home, helper }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    fn read(&self, agent: GuidanceAgent) -> Value {
        let text = fs::read_to_string(agent.config_path(self.home())).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn write(&self, agent: GuidanceAgent, text: &str) {
        let path = agent.config_path(self.home());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

/// The command string of every hook anywhere in `value`.
fn commands(value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    collect(value, &mut found);
    found
}

fn collect(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                if matches!(key.as_str(), "command" | "bash") && item.is_string() {
                    found.push(item.as_str().unwrap().to_owned());
                } else {
                    collect(item, found);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect(item, found)),
        _ => {}
    }
}

#[test]
fn gemini_writes_the_documented_settings_shape_with_milliseconds() {
    let fixture = Fixture::new(GuidanceAgent::Gemini);
    install(GuidanceAgent::Gemini, fixture.home(), &fixture.helper).unwrap();
    let document = fixture.read(GuidanceAgent::Gemini);
    let group = &document["hooks"]["SessionStart"][0];
    assert_eq!(group["matcher"], "*");
    let hook = &group["hooks"][0];
    assert_eq!(hook["type"], "command");
    assert_eq!(hook["name"], GUIDANCE_SOURCE_NAME);
    assert_eq!(hook["timeout"], 8000, "Gemini CLI counts milliseconds");
    let command = hook["command"].as_str().unwrap();
    assert!(command.contains("--runtime gemini-cli --event SessionStart"));
    assert!(command.contains("--source hide-guidance@1"));
    assert!(command.starts_with("if [ -x '"), "guarded: {command}");
}

#[test]
fn qwen_writes_the_documented_settings_shape_with_seconds() {
    let fixture = Fixture::new(GuidanceAgent::Qwen);
    install(GuidanceAgent::Qwen, fixture.home(), &fixture.helper).unwrap();
    let hook = &fixture.read(GuidanceAgent::Qwen)["hooks"]["SessionStart"][0]["hooks"][0];
    assert_eq!(hook["type"], "command");
    assert_eq!(hook["timeout"], 8);
    assert!(
        hook["command"]
            .as_str()
            .unwrap()
            .contains("--runtime qwen-code")
    );
}

#[test]
fn droid_writes_events_at_the_top_of_its_own_hooks_file() {
    let fixture = Fixture::new(GuidanceAgent::Droid);
    install(GuidanceAgent::Droid, fixture.home(), &fixture.helper).unwrap();
    let path = GuidanceAgent::Droid.config_path(fixture.home());
    assert!(path.ends_with(".factory/hooks.json"));
    let document = fixture.read(GuidanceAgent::Droid);
    assert!(document.get("hooks").is_none(), "events sit at the top");
    let hook = &document["SessionStart"][0]["hooks"][0];
    assert_eq!(hook["type"], "command");
    assert_eq!(hook["timeout"], 8);
}

#[test]
fn droid_never_hides_hooks_kept_in_settings_json_behind_a_new_hooks_file() {
    let fixture = Fixture::new(GuidanceAgent::Droid);
    let settings = fixture.home().join(".factory/settings.json");
    let theirs =
        r#"{"model":"x","hooks":{"Stop":[{"hooks":[{"type":"command","command":"/theirs.sh"}]}]}}"#;
    fs::write(&settings, theirs).unwrap();
    install(GuidanceAgent::Droid, fixture.home(), &fixture.helper).unwrap();
    assert!(
        !fixture.home().join(".factory/hooks.json").exists(),
        "a hooks.json beside a settings file with hooks would make Droid ignore them"
    );
    let document: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(document["model"], "x");
    assert_eq!(
        document["hooks"]["Stop"][0]["hooks"][0]["command"],
        "/theirs.sh"
    );
    assert_eq!(
        document["hooks"]["SessionStart"][0]["hooks"][0]["type"],
        "command"
    );
    remove(GuidanceAgent::Droid, fixture.home()).unwrap();
    let after: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(
        after["hooks"]["Stop"][0]["hooks"][0]["command"],
        "/theirs.sh"
    );
    assert!(after["hooks"].get("SessionStart").is_none());
}

#[test]
fn copilot_writes_its_own_file_with_both_shell_keys_and_seconds() {
    let fixture = Fixture::new(GuidanceAgent::Copilot);
    install(GuidanceAgent::Copilot, fixture.home(), &fixture.helper).unwrap();
    let path = GuidanceAgent::Copilot.config_path(fixture.home());
    assert!(path.ends_with(".copilot/hooks/hide-guidance.json"));
    let document = fixture.read(GuidanceAgent::Copilot);
    assert_eq!(document["version"], 1);
    let entry = &document["hooks"]["sessionStart"][0];
    assert_eq!(entry["type"], "command");
    assert_eq!(entry["timeoutSec"], 8);
    assert!(
        entry["bash"]
            .as_str()
            .unwrap()
            .contains("--runtime copilot-cli")
    );
    assert!(
        entry["powershell"]
            .as_str()
            .unwrap()
            .starts_with("if (Test-Path -LiteralPath '")
    );
}

#[test]
fn kiro_writes_its_own_v1_file_with_an_action() {
    let fixture = Fixture::new(GuidanceAgent::Kiro);
    install(GuidanceAgent::Kiro, fixture.home(), &fixture.helper).unwrap();
    let path = GuidanceAgent::Kiro.config_path(fixture.home());
    assert!(path.ends_with(".kiro/hooks/hide-guidance.json"));
    let document = fixture.read(GuidanceAgent::Kiro);
    assert_eq!(document["version"], "v1");
    let hook = &document["hooks"][0];
    assert_eq!(hook["trigger"], "SessionStart");
    assert_eq!(hook["action"]["type"], "command");
    assert_eq!(hook["timeout"], 8);
    assert!(
        hook["action"]["command"]
            .as_str()
            .unwrap()
            .contains("--runtime kiro")
    );
}

#[test]
fn another_tools_entries_survive_install_and_remove_in_every_shared_file() {
    for (agent, theirs) in [
        (
            GuidanceAgent::Gemini,
            r#"{"theme":"dark","hooks":{"SessionStart":[{"matcher":"startup","hooks":[{"type":"command","command":"/theirs.sh"}]}]}}"#,
        ),
        (
            GuidanceAgent::Qwen,
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"/theirs.sh"}]}]},"ui":1}"#,
        ),
        (
            GuidanceAgent::Droid,
            r#"{"SessionStart":[{"hooks":[{"type":"command","command":"/theirs.sh"}]}]}"#,
        ),
        (
            GuidanceAgent::Copilot,
            r#"{"version":1,"hooks":{"sessionStart":[{"type":"command","bash":"/theirs.sh","powershell":"theirs"}]}}"#,
        ),
        (
            GuidanceAgent::Kiro,
            r#"{"version":"v1","hooks":[{"name":"theirs","trigger":"SessionStart","action":{"type":"command","command":"/theirs.sh"}}]}"#,
        ),
    ] {
        let fixture = Fixture::new(agent);
        fixture.write(agent, theirs);
        let outcome = install(agent, fixture.home(), &fixture.helper).unwrap();
        assert_eq!(outcome.preserved_entries, 1, "{agent:?}");
        let found = commands(&fixture.read(agent));
        assert!(
            found.iter().any(|command| command == "/theirs.sh"),
            "{agent:?}"
        );
        assert_eq!(found.len(), 2, "{agent:?}: theirs and Hide's");

        let removed = remove(agent, fixture.home()).unwrap();
        assert_eq!(removed.removed_entries, 1, "{agent:?}");
        let after = commands(&fixture.read(agent));
        assert_eq!(after, vec!["/theirs.sh".to_owned()], "{agent:?}");
    }
}

#[test]
fn a_second_install_changes_nothing_and_a_removal_leaves_no_trace_of_an_own_file() {
    for agent in GuidanceAgent::ALL {
        let fixture = Fixture::new(agent);
        assert!(
            install(agent, fixture.home(), &fixture.helper)
                .unwrap()
                .changed
        );
        let first = fs::read(agent.config_path(fixture.home())).unwrap();
        let again = install(agent, fixture.home(), &fixture.helper).unwrap();
        assert!(!again.changed, "{agent:?}");
        assert_eq!(first, fs::read(agent.config_path(fixture.home())).unwrap());
        assert!(matches!(
            status(agent, fixture.home()),
            HookStatus::Installed { version: 1 }
        ));
        let removed = remove(agent, fixture.home()).unwrap();
        assert_eq!(removed.removed_entries, 1, "{agent:?}");
        assert!(matches!(
            status(agent, fixture.home()),
            HookStatus::NotInstalled
        ));
        if matches!(
            agent,
            GuidanceAgent::Copilot | GuidanceAgent::Kiro | GuidanceAgent::Droid
        ) {
            // Hide's own file is gone; Droid's hooks.json held only Hide's
            // event, so it reads as empty rather than removed.
            let exists = agent.config_path(fixture.home()).exists();
            assert_eq!(exists, agent == GuidanceAgent::Droid, "{agent:?}");
        }
    }
}

#[test]
fn a_file_that_does_not_parse_is_never_written() {
    for agent in GuidanceAgent::ALL {
        let fixture = Fixture::new(agent);
        fixture.write(agent, "{ not json");
        let before = fs::read(agent.config_path(fixture.home())).unwrap();
        let failure = install(agent, fixture.home(), &fixture.helper).unwrap_err();
        assert!(
            matches!(failure, InstallFailure::Unparsable { .. }),
            "{agent:?}"
        );
        assert!(matches!(
            remove(agent, fixture.home()),
            Err(InstallFailure::Unparsable { .. })
        ));
        assert_eq!(before, fs::read(agent.config_path(fixture.home())).unwrap());
        assert!(matches!(
            status(agent, fixture.home()),
            HookStatus::Failed { .. }
        ));
    }
}

#[test]
fn an_agent_that_is_not_set_up_has_nothing_to_attach_to() {
    for agent in GuidanceAgent::ALL {
        let home = tempfile::tempdir().unwrap();
        assert!(matches!(
            status(agent, home.path()),
            HookStatus::RuntimeAbsent
        ));
    }
}

#[test]
fn an_older_marker_reads_outdated_and_a_gone_helper_reads_failed() {
    let fixture = Fixture::new(GuidanceAgent::Qwen);
    install(GuidanceAgent::Qwen, fixture.home(), &fixture.helper).unwrap();
    let path = GuidanceAgent::Qwen.config_path(fixture.home());
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("hide-guidance@1", "hide-guidance@0");
    fs::write(&path, text).unwrap();
    assert!(matches!(
        status(GuidanceAgent::Qwen, fixture.home()),
        HookStatus::Outdated { version: 0 }
    ));
    install(GuidanceAgent::Qwen, fixture.home(), &fixture.helper).unwrap();
    fs::remove_file(&fixture.helper).unwrap();
    assert!(matches!(
        status(GuidanceAgent::Qwen, fixture.home()),
        HookStatus::Failed {
            reason: InstallFailure::HelperMissing { .. }
        }
    ));
}

#[test]
fn the_installed_helper_is_read_back_from_the_entry() {
    let fixture = Fixture::new(GuidanceAgent::Kiro);
    install(GuidanceAgent::Kiro, fixture.home(), &fixture.helper).unwrap();
    assert_eq!(
        installed_helper_path(GuidanceAgent::Kiro, fixture.home()).as_deref(),
        Some(fixture.helper.to_str().unwrap())
    );
}

#[test]
fn each_agent_reads_context_in_the_field_its_documentation_names() {
    let context = session_context(Some("workspace guidance"));
    assert!(context.contains(PURPOSE_CONTEXT));
    assert!(context.contains("`hide browser help`"));
    assert!(context.ends_with("workspace guidance"));
    assert!(session_context(None).contains("`hide browser help`"));

    let gemini: Value = serde_json::from_str(&stdout(GuidanceAgent::Gemini, &context)).unwrap();
    assert_eq!(gemini["hookSpecificOutput"]["additionalContext"], context);
    for agent in [GuidanceAgent::Qwen, GuidanceAgent::Droid] {
        let value: Value = serde_json::from_str(&stdout(agent, &context)).unwrap();
        assert_eq!(value["hookSpecificOutput"]["hookEventName"], "SessionStart");
        assert_eq!(value["hookSpecificOutput"]["additionalContext"], context);
    }
    let copilot: Value = serde_json::from_str(&stdout(GuidanceAgent::Copilot, &context)).unwrap();
    assert_eq!(copilot, json!({ "additionalContext": context }));
    assert_eq!(stdout(GuidanceAgent::Kiro, &context), context);
}

#[test]
fn the_same_session_start_twice_prints_the_same_bytes() {
    // A session that reaches the hook by two routes (a bridge that runs the
    // agent's hooks inside another agent, or two config layers) is told the
    // same thing twice: the output depends on nothing but the context.
    for agent in GuidanceAgent::ALL {
        let context = session_context(None);
        assert_eq!(
            stdout(agent, &context),
            stdout(agent, &context),
            "{agent:?}"
        );
    }
}
