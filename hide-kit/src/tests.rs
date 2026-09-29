//! The kit against a HOME fixture and a stand-in Herdr socket. Nothing here
//! reads or writes the account running the tests.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;

/// A Herdr that keeps a plugin registry in memory and answers the three
/// plugin methods the way Herdr 0.9.1 does: `plugin.link` stores the
/// resolved path and replaces an entry with the same id.
struct FakeHerdr {
    socket: PathBuf,
    plugins: Arc<Mutex<Vec<Value>>>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl FakeHerdr {
    fn start(dir: &Path) -> Self {
        let socket = dir.join("herdr.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let plugins = Arc::new(Mutex::new(Vec::<Value>::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (registry, log) = (plugins.clone(), calls.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut line = String::new();
                if BufReader::new(&stream).read_line(&mut line).is_err() {
                    continue;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                let method = request["method"].as_str().unwrap().to_owned();
                log.lock().unwrap().push(method.clone());
                let mut plugins = registry.lock().unwrap();
                let result = match method.as_str() {
                    "plugin.list" => json!({ "type": "plugin_list", "plugins": *plugins }),
                    "plugin.link" => {
                        let root =
                            std::fs::canonicalize(request["params"]["path"].as_str().unwrap())
                                .unwrap();
                        let manifest =
                            std::fs::read_to_string(root.join("herdr-plugin.toml")).unwrap();
                        let id = manifest
                            .lines()
                            .find_map(|line| line.strip_prefix("id = "))
                            .unwrap()
                            .trim_matches('"')
                            .to_owned();
                        let entry = plugin(&id, &root.display().to_string(), "local");
                        let mut entry = entry;
                        entry["enabled"] = request["params"]["enabled"].clone();
                        plugins.retain(|existing| existing["plugin_id"] != id.as_str());
                        plugins.push(entry.clone());
                        json!({ "type": "plugin_linked", "plugin": entry })
                    }
                    "plugin.unlink" => {
                        let id = request["params"]["plugin_id"].as_str().unwrap().to_owned();
                        let before = plugins.len();
                        plugins.retain(|existing| existing["plugin_id"] != id.as_str());
                        json!({ "type": "plugin_unlinked", "plugin_id": id, "removed": plugins.len() < before })
                    }
                    other => panic!("unexpected method {other}"),
                };
                let response = json!({ "id": request["id"], "result": result });
                let _ = writeln!(stream, "{response}");
            }
        });
        Self {
            socket,
            plugins,
            calls,
        }
    }

    fn links(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|method| *method == "plugin.link")
            .count()
    }
}

fn plugin(id: &str, root: &str, kind: &str) -> Value {
    json!({
        "plugin_id": id,
        "name": "Agent Context Labels",
        "version": "0.2.0",
        "plugin_root": root,
        "manifest_path": format!("{root}/herdr-plugin.toml"),
        "enabled": true,
        "source": { "kind": kind },
    })
}

fn executable(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

const OTHER_TOOL: &str = r#"{
  "model": "opus",
  "hooks": {
    "SubagentStart": [
      { "hooks": [ { "type": "command", "command": "/opt/other/notify.sh start" } ] }
    ]
  }
}
"#;

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    target: KitTarget,
    herdr: FakeHerdr,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        // Resolved, so paths compare equal to what the fake Herdr records.
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let home = root.join("home");
        let kit = root.join("kit");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(home.join(".claude/settings.json"), OTHER_TOOL).unwrap();
        executable(&kit.join("hide"), "#!/bin/sh\n");
        executable(&kit.join("hide-agent-hooks"), "#!/bin/sh\n");
        std::fs::create_dir_all(kit.join("agent-context-labels/scripts")).unwrap();
        std::fs::write(
            kit.join("agent-context-labels/herdr-plugin.toml"),
            "id = \"hide.agent-context-labels\"\n",
        )
        .unwrap();
        executable(
            &kit.join("agent-context-labels/hide-agent-context-labels"),
            "#!/bin/sh\n",
        );
        // hcoord's "Node" is sh, and its cli.js a script that answers the
        // ensure call the way hcoord does and remembers it was asked.
        executable(
            &kit.join("hcoord/dist/hcoord/cli.js"),
            "echo \"$@\" >> \"$HOME/ensure.log\"\nprintf '{\"ok\":true,\"value\":{}}\\n'\n",
        );
        executable(
            &root.join("bin/herdr"),
            "#!/bin/sh\necho \"$@\" >> \"$HOME/herdr.log\"\n",
        );
        let herdr = FakeHerdr::start(&root);
        let target = KitTarget {
            home: home.clone(),
            kit_dir: kit,
            cli_dir: home.join(".local/bin"),
            owned_roots: vec![root.join("helper-root")],
            herdr_socket: herdr.socket.clone(),
            herdr_bin: Some(root.join("bin/herdr")),
            hcoord: Ok(HcoordRuntime {
                program: PathBuf::from("/bin/sh"),
                env: vec![("HCOORD_TEST".to_owned(), "1".to_owned())],
            }),
        };
        Self {
            _dir: dir,
            root,
            target,
            herdr,
        }
    }

    fn home(&self) -> &Path {
        &self.target.home
    }

    fn settings(&self) -> String {
        std::fs::read_to_string(self.home().join(".claude/settings.json")).unwrap()
    }
}

fn state(report: &KitReport, id: ComponentId) -> ComponentState {
    report.component(id).unwrap().state
}

fn other_tool_entry(settings: &str) -> Value {
    let document: Value = serde_json::from_str(settings).unwrap();
    document["hooks"]["SubagentStart"][0].clone()
}

#[test]
fn a_first_apply_installs_every_part_and_keeps_other_tools_entries() {
    let fixture = Fixture::new();
    let before = other_tool_entry(OTHER_TOOL);

    let report = apply(&fixture.target, &Scope::Automatic);

    for id in [
        ComponentId::Cli,
        ComponentId::ClaudeCodeHook,
        ComponentId::Labels,
        ComponentId::Hcoord,
    ] {
        assert_eq!(
            state(&report, id),
            ComponentState::Installed,
            "{id:?}: {report:?}"
        );
    }
    assert_eq!(
        state(&report, ComponentId::CodexHook),
        ComponentState::Absent
    );
    assert!(!fixture.home().join(".codex").exists());

    let settings = fixture.settings();
    assert_eq!(other_tool_entry(&settings), before);
    let document: Value = serde_json::from_str(&settings).unwrap();
    assert_eq!(document["model"], "opus");
    assert!(settings.contains(&format!(
        "if [ -x '{}' ]",
        fixture.target.kit_dir.join("hide-agent-hooks").display()
    )));

    assert_eq!(
        std::fs::read_link(fixture.home().join(".local/bin/hide")).unwrap(),
        fixture.target.kit_dir.join("hide")
    );
    let plugins = fixture.herdr.plugins.lock().unwrap().clone();
    assert_eq!(plugins.len(), 1);
    assert_eq!(
        plugins[0]["plugin_root"],
        labels_home(fixture.home()).display().to_string()
    );
    assert!(
        labels_home(fixture.home())
            .join("hide-agent-context-labels")
            .is_file()
    );
    let shim = std::fs::read_to_string(fixture.home().join(".hcoord/bin/hcoord")).unwrap();
    assert!(shim.starts_with("#!/bin/sh\nHCOORD_TEST='1' exec '/bin/sh' '"));
    assert!(
        std::fs::read_to_string(fixture.home().join("ensure.log"))
            .unwrap()
            .contains("daemon ensure --json")
    );
    let record = std::fs::read_to_string(fixture.home().join(".hide/kit/installed.json")).unwrap();
    for code in ["cli", "claude_code_hook", "labels", "hcoord"] {
        assert!(record.contains(code), "{record}");
    }
    assert!(!record.contains("codex_hook"));
}

#[test]
fn a_second_apply_of_the_same_build_changes_nothing() {
    let fixture = Fixture::new();
    apply(&fixture.target, &Scope::Automatic);
    let settings = fixture.settings();
    let record_path = fixture.home().join(".hide/kit/installed.json");
    let record_written = std::fs::metadata(&record_path).unwrap().modified().unwrap();
    let links = fixture.herdr.links();

    let report = apply(&fixture.target, &Scope::Automatic);

    assert_eq!(fixture.settings(), settings);
    assert_eq!(
        std::fs::metadata(&record_path).unwrap().modified().unwrap(),
        record_written
    );
    assert_eq!(fixture.herdr.links(), links);
    assert!(report.components.iter().all(|part| matches!(
        part.state,
        ComponentState::Installed | ComponentState::Absent
    )));
}

#[test]
fn a_part_the_operator_removed_stays_removed_until_reinstall_asks_for_it() {
    let fixture = Fixture::new();
    apply(&fixture.target, &Scope::Automatic);
    hide_agent_hooks::remove(hide_agent_hooks::AgentRuntime::ClaudeCode, fixture.home()).unwrap();
    fixture.herdr.plugins.lock().unwrap().clear();

    let report = apply(&fixture.target, &Scope::Automatic);
    assert_eq!(
        state(&report, ComponentId::ClaudeCodeHook),
        ComponentState::Removed
    );
    assert_eq!(state(&report, ComponentId::Labels), ComponentState::Removed);
    assert!(!fixture.settings().contains("hide-subagents"));

    let report = apply(
        &fixture.target,
        &Scope::Reinstall(vec![ComponentId::ClaudeCodeHook]),
    );
    assert_eq!(
        state(&report, ComponentId::ClaudeCodeHook),
        ComponentState::Installed
    );
    assert_eq!(state(&report, ComponentId::Labels), ComponentState::Removed);
    assert!(fixture.herdr.plugins.lock().unwrap().is_empty());
}

#[test]
fn the_old_first_run_marker_does_not_count_as_an_install() {
    let fixture = Fixture::new();
    let marker = fixture.home().join(".hide/agent-hooks/installed-once");
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(&marker, "").unwrap();

    let report = apply(&fixture.target, &Scope::Automatic);

    assert_eq!(
        state(&report, ComponentId::ClaudeCodeHook),
        ComponentState::Installed
    );
}

#[test]
fn an_older_hook_is_replaced_on_the_next_apply() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.home().join(".claude/settings.json"),
        r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"'/Applications/hide.app/Contents/Resources/hide-agent-hooks' hook --runtime claude-code --event Stop --source hide-subagents@5"}]}]}}"#,
    )
    .unwrap();
    assert_eq!(
        state(&status(&fixture.target), ComponentId::ClaudeCodeHook),
        ComponentState::Outdated
    );

    let report = apply(&fixture.target, &Scope::Automatic);

    assert_eq!(
        state(&report, ComponentId::ClaudeCodeHook),
        ComponentState::Installed
    );
    let settings = fixture.settings();
    assert!(!settings.contains("hide-subagents@5"));
    assert!(settings.contains(&hide_agent_hooks::hook_source_id()));
}

#[test]
fn a_hook_file_that_does_not_parse_is_left_byte_for_byte() {
    let fixture = Fixture::new();
    let broken = "{ \"hooks\": [ not json\n";
    std::fs::write(fixture.home().join(".claude/settings.json"), broken).unwrap();

    let report = apply(&fixture.target, &Scope::Automatic);

    let hook = report.component(ComponentId::ClaudeCodeHook).unwrap();
    assert_eq!(hook.state, ComponentState::Failed);
    assert!(hook.reason.as_deref().unwrap().contains("not valid JSON"));
    assert_eq!(fixture.settings(), broken);
    assert_eq!(state(&report, ComponentId::Cli), ComponentState::Installed);
}

#[test]
fn another_programs_hide_is_left_and_an_older_hide_link_is_replaced() {
    let fixture = Fixture::new();
    let link = fixture.home().join(".local/bin/hide");
    executable(&link, "#!/bin/sh\necho mine\n");

    let report = apply(&fixture.target, &Scope::Automatic);
    assert_eq!(state(&report, ComponentId::Cli), ComponentState::Failed);
    assert_eq!(
        std::fs::read_to_string(&link).unwrap(),
        "#!/bin/sh\necho mine\n"
    );

    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("/Applications/Old hide.app/Contents/Resources/hide", &link)
        .unwrap();
    assert_eq!(
        state(&status(&fixture.target), ComponentId::Cli),
        ComponentState::Outdated
    );
    let report = apply(&fixture.target, &Scope::Automatic);
    assert_eq!(state(&report, ComponentId::Cli), ComponentState::Installed);

    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("/usr/local/bin/some-other-hide", &link).unwrap();
    let report = apply(&fixture.target, &Scope::Automatic);
    assert_eq!(state(&report, ComponentId::Cli), ComponentState::Failed);
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        PathBuf::from("/usr/local/bin/some-other-hide")
    );
}

#[test]
fn a_github_install_of_the_plugin_is_replaced_through_the_herdr_command() {
    let fixture = Fixture::new();
    fixture.herdr.plugins.lock().unwrap().push(plugin(
        LABELS_PLUGIN_ID,
        "/somewhere/herdr/plugins/github/agent-context-labels",
        "github",
    ));
    assert_eq!(
        state(&status(&fixture.target), ComponentId::Labels),
        ComponentState::Outdated
    );

    let report = apply(&fixture.target, &Scope::Automatic);

    assert_eq!(
        state(&report, ComponentId::Labels),
        ComponentState::Installed
    );
    assert_eq!(
        std::fs::read_to_string(fixture.home().join("herdr.log")).unwrap(),
        "plugin uninstall hide.agent-context-labels\n"
    );
}

#[test]
fn a_plugin_turned_off_in_herdr_stays_off_when_it_is_updated() {
    let fixture = Fixture::new();
    apply(&fixture.target, &Scope::Automatic);
    fixture.herdr.plugins.lock().unwrap()[0]["enabled"] = json!(false);
    std::fs::write(
        fixture
            .target
            .kit_dir
            .join("agent-context-labels/hide-agent-context-labels"),
        "#!/bin/sh\necho newer\n",
    )
    .unwrap();
    assert_eq!(
        state(&status(&fixture.target), ComponentId::Labels),
        ComponentState::Outdated
    );

    let report = apply(&fixture.target, &Scope::Automatic);

    assert_eq!(
        state(&report, ComponentId::Labels),
        ComponentState::Installed
    );
    assert_eq!(fixture.herdr.plugins.lock().unwrap()[0]["enabled"], false);
    assert!(
        std::fs::read_to_string(labels_home(fixture.home()).join("hide-agent-context-labels"))
            .unwrap()
            .contains("newer")
    );
}

#[test]
fn with_herdr_down_the_plugin_fails_and_is_tried_again_later() {
    let mut fixture = Fixture::new();
    fixture.target.herdr_socket = fixture.root.join("no-herdr.sock");

    let report = apply(&fixture.target, &Scope::Automatic);
    let labels = report.component(ComponentId::Labels).unwrap();
    assert_eq!(labels.state, ComponentState::Failed);
    assert!(labels.reason.as_deref().unwrap().contains("plugin.list"));
    assert_eq!(state(&report, ComponentId::Cli), ComponentState::Installed);

    fixture.target.herdr_socket = fixture.herdr.socket.clone();
    let report = apply(&fixture.target, &Scope::Automatic);
    assert_eq!(
        state(&report, ComponentId::Labels),
        ComponentState::Installed
    );
}

#[test]
fn without_a_runtime_for_hcoord_it_fails_and_an_existing_shim_is_left() {
    let mut fixture = Fixture::new();
    let shim = fixture.home().join(".hcoord/bin/hcoord");
    executable(
        &shim,
        "#!/bin/sh\nexec \"/opt/node\" \"/x/cli.js\" \"$@\"\n",
    );
    fixture.target.hcoord = Err("hcoord needs Node 22.12.0 or later; none was found".to_owned());

    let report = apply(&fixture.target, &Scope::Automatic);

    let hcoord = report.component(ComponentId::Hcoord).unwrap();
    assert_eq!(hcoord.state, ComponentState::Failed);
    assert!(hcoord.reason.as_deref().unwrap().contains("Node 22.12.0"));
    assert!(
        std::fs::read_to_string(&shim)
            .unwrap()
            .contains("/opt/node")
    );
}

#[test]
fn an_unreadable_record_installs_nothing_missing_on_a_guess() {
    let fixture = Fixture::new();
    let record = fixture.home().join(".hide/kit/installed.json");
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::write(&record, "not a record").unwrap();

    let report = apply(&fixture.target, &Scope::Automatic);

    let cli = report.component(ComponentId::Cli).unwrap();
    assert_eq!(cli.state, ComponentState::Failed);
    assert!(cli.reason.as_deref().unwrap().contains("installed.json"));
    assert!(!fixture.home().join(".local/bin/hide").exists());
    assert_eq!(std::fs::read_to_string(&record).unwrap(), "not a record");
}

#[test]
fn removing_the_kit_takes_only_hides_parts_and_leaves_hcoord() {
    let fixture = Fixture::new();
    apply(&fixture.target, &Scope::Automatic);

    let report = remove(&fixture.target);

    let outcome = |id| {
        report
            .components
            .iter()
            .find(|(part, _)| *part == id)
            .map(|(_, outcome)| outcome.clone())
            .unwrap()
    };
    assert_eq!(outcome(ComponentId::Cli), RemoveOutcome::Removed);
    assert_eq!(outcome(ComponentId::ClaudeCodeHook), RemoveOutcome::Removed);
    assert_eq!(outcome(ComponentId::Labels), RemoveOutcome::Removed);
    assert!(matches!(
        outcome(ComponentId::Hcoord),
        RemoveOutcome::Kept { .. }
    ));
    let settings = fixture.settings();
    assert!(!settings.contains("hide-subagents"));
    assert_eq!(other_tool_entry(&settings), other_tool_entry(OTHER_TOOL));
    assert!(!fixture.home().join(".local/bin/hide").exists());
    assert!(fixture.herdr.plugins.lock().unwrap().is_empty());
    assert!(fixture.home().join(".hcoord/bin/hcoord").is_file());
    assert!(!fixture.home().join(".hide/kit/installed.json").exists());
}

/// The kit links the packaged manifest; the checkout's manifest is what
/// `herdr plugin install` builds from. They must say the same thing apart
/// from the build step.
#[test]
fn the_packaged_plugin_manifest_is_the_source_manifest_without_its_build_step() {
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins/agent-context-labels");
    let meaningful = |text: &str| {
        let mut lines = Vec::new();
        let mut in_build = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("[[") || trimmed.starts_with('[') {
                in_build = trimmed == "[[build]]";
            }
            if in_build || trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            lines.push(trimmed.to_owned());
        }
        lines
    };
    let source = std::fs::read_to_string(plugin.join("herdr-plugin.toml")).unwrap();
    let packaged = std::fs::read_to_string(plugin.join("package/herdr-plugin.toml")).unwrap();
    let has_build = |text: &str| text.lines().any(|line| line.trim() == "[[build]]");
    assert!(has_build(&source));
    assert!(!has_build(&packaged));
    assert_eq!(meaningful(&packaged), meaningful(&source));
    for line in meaningful(&packaged) {
        if let Some(script) = line
            .strip_prefix("command = [\"/bin/sh\", \"")
            .and_then(|rest| rest.strip_suffix("\"]"))
        {
            assert!(plugin.join("package").join(script).is_file(), "{script}");
        }
    }
}
