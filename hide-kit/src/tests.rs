//! The kit against a HOME fixture and a stand-in Herdr socket. Nothing here
//! reads or writes the account running the tests.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;

/// A Herdr that keeps a plugin registry in memory and answers `plugin.list`
/// and `plugin.unlink` the way Herdr 0.9.1 does.
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
        executable(&root.join("launchctl"), "#!/bin/sh\nexit 113\n");
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
            codex: None,
            legacy_coordination_home: None,
            user_agents: hide_platform::user_agents::UserAgents::fixture(
                root.join("launchctl"),
                "fixture".into(),
            ),
            retirement_projects: Vec::new(),
            legacy: Vec::new(),
            stop: Arc::default(),
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

    /// The retired labels plugin as an older Hide left it: the kit's copy,
    /// the plugin's state folder, and Herdr's entry for it.
    fn legacy_plugin(&self, kind: &str) {
        let copy = labels_home(self.home());
        executable(&copy.join("hide-agent-context-labels"), "#!/bin/sh\n");
        std::fs::write(
            copy.join("herdr-plugin.toml"),
            "id = \"hide.agent-context-labels\"\n",
        )
        .unwrap();
        let state = plugin_state_dir(self.home());
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("display-state.json"), "{}").unwrap();
        let root = match kind {
            "github" => "/somewhere/herdr/plugins/github/agent-context-labels".to_owned(),
            _ => copy.display().to_string(),
        };
        self.herdr
            .plugins
            .lock()
            .unwrap()
            .push(plugin(LABELS_PLUGIN_ID, &root, kind));
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

    let report = apply(&fixture.target, &Scope::automatic());

    for id in [ComponentId::Cli, ComponentId::ClaudeCodeHook] {
        assert_eq!(
            state(&report, id),
            ComponentState::Installed,
            "{id:?}: {report:?}"
        );
    }
    assert_eq!(
        state(&report, ComponentId::CoordinationRetirement),
        ComponentState::Installed
    );
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
    // A machine that never had the labels plugin is not asked about it; the
    // standalone hcoord plugin is looked for once.
    assert_eq!(*fixture.herdr.calls.lock().unwrap(), ["plugin.list"]);
    assert!(report.labels_retirement.is_empty());
    assert!(!fixture.home().join(".hide/hcoord").exists());
    let record: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.home().join(".hide/kit/installed.json")).unwrap(),
    )
    .unwrap();
    let installed = vec!["claude_code_hook", "cli", "coordination_retirement"];
    assert_eq!(
        record["installed"],
        serde_json::json!(installed),
        "{record}"
    );
    // The standalone plugin is looked for on every platform, so its
    // retirement is recorded on every platform.
    assert_eq!(
        record["retired"],
        serde_json::json!(["hide.hcoord"]),
        "{record}"
    );
}

#[test]
fn a_second_apply_of_the_same_build_changes_nothing() {
    let fixture = Fixture::new();
    apply(&fixture.target, &Scope::automatic());
    let settings = fixture.settings();
    let record_path = fixture.home().join(".hide/kit/installed.json");
    let record_written = std::fs::metadata(&record_path).unwrap().modified().unwrap();
    let calls = fixture.herdr.calls.lock().unwrap().len();

    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(fixture.settings(), settings);
    assert_eq!(
        std::fs::metadata(&record_path).unwrap().modified().unwrap(),
        record_written
    );
    assert_eq!(fixture.herdr.calls.lock().unwrap().len(), calls);
    assert!(report.components.iter().all(|part| matches!(
        part.state,
        ComponentState::Installed | ComponentState::Absent
    )));
}

#[test]
fn a_part_the_operator_removed_stays_removed_until_reinstall_asks_for_it() {
    let fixture = Fixture::new();
    apply(&fixture.target, &Scope::automatic());
    hide_agent_hooks::remove(hide_agent_hooks::AgentRuntime::ClaudeCode, fixture.home()).unwrap();

    let report = apply(&fixture.target, &Scope::automatic());
    assert_eq!(
        state(&report, ComponentId::ClaudeCodeHook),
        ComponentState::Removed
    );
    assert!(!fixture.settings().contains("hide-subagents"));

    let report = apply(
        &fixture.target,
        &Scope::reinstall([ComponentId::ClaudeCodeHook]),
    );
    assert_eq!(
        state(&report, ComponentId::ClaudeCodeHook),
        ComponentState::Installed
    );
}

#[test]
fn the_old_first_run_marker_does_not_count_as_an_install() {
    let fixture = Fixture::new();
    let marker = fixture.home().join(".hide/agent-hooks/installed-once");
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(&marker, "").unwrap();

    let report = apply(&fixture.target, &Scope::automatic());

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

    let report = apply(&fixture.target, &Scope::automatic());

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

    let report = apply(&fixture.target, &Scope::automatic());

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

    let report = apply(&fixture.target, &Scope::automatic());
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
    let report = apply(&fixture.target, &Scope::automatic());
    assert_eq!(state(&report, ComponentId::Cli), ComponentState::Installed);

    std::fs::remove_file(&link).unwrap();
    let old_resources = fixture.home().join("Old Hide/resources");
    std::fs::create_dir_all(&old_resources).unwrap();
    std::fs::write(old_resources.join("app.asar"), "").unwrap();
    std::fs::write(old_resources.parent().unwrap().join("hide"), "").unwrap();
    std::os::unix::fs::symlink(old_resources.join("hide"), &link).unwrap();
    let report = apply(&fixture.target, &Scope::automatic());
    assert_eq!(state(&report, ComponentId::Cli), ComponentState::Installed);
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        fixture.target.kit_dir.join("hide")
    );

    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("/usr/local/bin/some-other-hide", &link).unwrap();
    let report = apply(&fixture.target, &Scope::automatic());
    assert_eq!(state(&report, ComponentId::Cli), ComponentState::Failed);
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        PathBuf::from("/usr/local/bin/some-other-hide")
    );

    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("/another-program/resources/hide", &link).unwrap();
    assert_eq!(
        state(
            &apply(&fixture.target, &Scope::automatic()),
            ComponentId::Cli
        ),
        ComponentState::Failed
    );
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        PathBuf::from("/another-program/resources/hide")
    );

    // A target that climbs out of a folder Hide owns is not Hide's.
    std::fs::remove_file(&link).unwrap();
    let climbing = fixture.target.owned_roots[0].join("../elsewhere/hide");
    std::os::unix::fs::symlink(&climbing, &link).unwrap();
    let report = apply(&fixture.target, &Scope::automatic());
    assert_eq!(state(&report, ComponentId::Cli), ComponentState::Failed);
    assert_eq!(std::fs::read_link(&link).unwrap(), climbing);
}

/// PRD labels-in-hided B2: an upgrade takes the plugin out of Herdr and off
/// the disk, and the record of having installed it means nothing any more.
#[test]
fn an_upgrade_takes_the_linked_labels_plugin_its_copy_and_its_state_out() {
    let fixture = Fixture::new();
    fixture.legacy_plugin("local");
    let record = fixture.home().join(".hide/kit/installed.json");
    std::fs::write(&record, r#"{"format":1,"installed":["labels"]}"#).unwrap();

    let report = apply(&fixture.target, &Scope::automatic());

    assert!(fixture.herdr.plugins.lock().unwrap().is_empty());
    assert!(!labels_home(fixture.home()).exists());
    assert!(!plugin_state_dir(fixture.home()).exists());
    assert_eq!(
        report.labels_retirement.removed,
        [
            "Herdr plugin link (linked folder)",
            "kit copy",
            "state folder"
        ]
    );
    assert!(report.labels_retirement.failures.is_empty());
    assert_eq!(state(&report, ComponentId::Cli), ComponentState::Installed);

    // Nothing is left to take out, and Herdr is not asked again.
    let calls = fixture.herdr.calls.lock().unwrap().len();
    let report = apply(&fixture.target, &Scope::automatic());
    assert!(report.labels_retirement.is_empty());
    assert_eq!(fixture.herdr.calls.lock().unwrap().len(), calls);
}

#[test]
fn a_github_install_of_the_labels_plugin_is_taken_out_through_the_herdr_command() {
    let fixture = Fixture::new();
    fixture.legacy_plugin("github");

    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(
        std::fs::read_to_string(fixture.home().join("herdr.log")).unwrap(),
        "plugin uninstall hide.agent-context-labels\n"
    );
    assert_eq!(
        report.labels_retirement.removed[0],
        "Herdr plugin link (GitHub)"
    );
}

#[test]
fn with_herdr_down_the_labels_plugin_files_stay_until_its_link_is_gone() {
    let mut fixture = Fixture::new();
    fixture.legacy_plugin("local");
    fixture.target.herdr_socket = fixture.root.join("no-herdr.sock");

    let report = apply(&fixture.target, &Scope::automatic());
    assert!(
        report.labels_retirement.failures[0].contains("plugin.list"),
        "{report:?}"
    );
    assert!(labels_home(fixture.home()).exists());
    assert!(plugin_state_dir(fixture.home()).exists());
    assert_eq!(state(&report, ComponentId::Cli), ComponentState::Installed);

    fixture.target.herdr_socket = fixture.herdr.socket.clone();
    let report = apply(&fixture.target, &Scope::automatic());
    assert!(report.labels_retirement.failures.is_empty());
    assert!(fixture.herdr.plugins.lock().unwrap().is_empty());
    assert!(!labels_home(fixture.home()).exists());
}

/// A process holding `lock` exclusively, as a running labels watcher does,
/// once it holds it.
fn lock_holder(lock: &Path, ready: &Path) -> std::process::Child {
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(lock, "").unwrap();
    let watcher = std::process::Command::new("perl")
        .args([
            "-e",
            r#"use Fcntl ":flock"; open(my $f, ">>", $ARGV[0]) or die; flock($f, LOCK_EX) or die; open(my $r, ">", $ARGV[1]); close($r); sleep 60;"#,
        ])
        .arg(lock)
        .arg(ready)
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    while !ready.exists() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    watcher
}

/// A watcher is found by the lock it holds under this home, whatever its
/// executable is called now, and it is stopped before its state goes.
#[test]
fn a_running_labels_watcher_is_found_by_its_lock_and_stopped() {
    let fixture = Fixture::new();
    fixture.legacy_plugin("local");
    let lock = plugin_state_dir(fixture.home()).join("watcher.lock");
    let mut watcher = lock_holder(&lock, &fixture.root.join("watcher-ready"));

    let report = apply(&fixture.target, &Scope::automatic());

    let status = watcher.wait().unwrap();
    assert!(!status.success(), "the watcher was ended by a signal");
    assert!(
        report
            .labels_retirement
            .removed
            .contains(&format!("watcher process {}", watcher.id())),
        "{report:?}"
    );
    assert!(!plugin_state_dir(fixture.home()).exists());
}

/// The boundary is the home: a watcher holding another home's lock (the
/// operator's, seen from a test home) is never signalled, while this home's
/// plugin is still taken out.
#[test]
fn a_watcher_holding_another_homes_lock_is_left_running() {
    let fixture = Fixture::new();
    fixture.legacy_plugin("local");
    let other_home = fixture.root.join("other-home");
    let lock = plugin_state_dir(&other_home).join("watcher.lock");
    let mut watcher = lock_holder(&lock, &fixture.root.join("other-ready"));

    let report = apply(&fixture.target, &Scope::automatic());

    let still_running = watcher.try_wait().unwrap().is_none();
    let _ = watcher.kill();
    let _ = watcher.wait();
    assert!(
        still_running,
        "another home's watcher was signalled: {report:?}"
    );
    assert!(
        !report
            .labels_retirement
            .removed
            .iter()
            .any(|part| part.starts_with("watcher process")),
        "{report:?}"
    );
    assert!(!plugin_state_dir(fixture.home()).exists());
    assert!(plugin_state_dir(&other_home).exists());
}

/// A watcher the pass cannot stop keeps its state folder, because the lock
/// inside it is the only way a later pass finds the watcher (D-12, B2).
/// The holder here is this test process, which the pass never signals, as
/// a watcher lsof cannot name or SIGKILL cannot end would be.
#[test]
fn a_watcher_that_cannot_be_stopped_keeps_its_state_folder() {
    use hide_platform::fs::lock::{Mode, Waited, lock_file};
    let fixture = Fixture::new();
    fixture.legacy_plugin("local");
    let lock = plugin_state_dir(fixture.home()).join("watcher.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    let held = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&lock)
        .unwrap();
    let held = match lock_file(held, Mode::Exclusive, Duration::ZERO, &|| false).unwrap() {
        Waited::Locked(lock) => lock,
        other => panic!("the test could not take the lock: {other:?}"),
    };

    let report = apply(&fixture.target, &Scope::automatic());

    assert!(!report.labels_retirement.failures.is_empty(), "{report:?}");
    assert!(lock.exists(), "the watcher's lock is gone: {report:?}");
    drop(held);
}

#[cfg(target_os = "macos")]
#[test]
fn the_kit_runs_nothing_from_a_folder_another_account_can_change() {
    let fixture = Fixture::new();
    apply(&fixture.target, &Scope::automatic());
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let state = kit_state_dir(&fixture.target.home);
    assert_eq!(mode(&state), 0o700);
    assert_eq!(mode(state.parent().unwrap()), 0o700);

    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o775)).unwrap();
    let report = status(&fixture.target);
    let part = report
        .component(ComponentId::CoordinationRetirement)
        .unwrap();
    assert_eq!(part.state, ComponentState::Failed);
    assert!(
        part.reason
            .as_deref()
            .is_some_and(|reason| reason.contains("can be changed by another account")),
        "{part:?}"
    );
}

/// Two kits on one account take turns: while another holds the account,
/// an apply waits and then installs; a quitting owner stops waiting.
#[test]
fn an_apply_waits_for_another_kit_changing_the_same_account() {
    let fixture = Fixture::new();
    let settings = fixture.home().join(".claude/settings.json");
    let held = lock_account(&fixture.target).unwrap();
    let target = fixture.target.clone();
    let waiting = std::thread::spawn(move || apply(&target, &Scope::automatic()));
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!waiting.is_finished());
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), OTHER_TOOL);
    drop(held);
    let report = waiting.join().unwrap();
    assert_eq!(
        state(&report, ComponentId::ClaudeCodeHook),
        ComponentState::Installed
    );

    let _held = lock_account(&fixture.target).unwrap();
    fixture
        .target
        .stop
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let report = apply(&fixture.target, &Scope::automatic());
    assert!(
        report
            .components
            .iter()
            .all(|part| part.reason.as_deref() == Some("Hide is quitting")),
        "{report:?}"
    );
}

#[test]
fn an_unreadable_record_installs_nothing_missing_on_a_guess() {
    let fixture = Fixture::new();
    let record = fixture.home().join(".hide/kit/installed.json");
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::write(&record, "not a record").unwrap();

    let report = apply(&fixture.target, &Scope::automatic());

    let cli = report.component(ComponentId::Cli).unwrap();
    assert_eq!(cli.state, ComponentState::Failed);
    assert!(cli.reason.as_deref().unwrap().contains("installed.json"));
    assert!(!fixture.home().join(".local/bin/hide").exists());
    assert_eq!(std::fs::read_to_string(&record).unwrap(), "not a record");
}

#[cfg(target_os = "macos")]
#[test]
fn removing_the_kit_takes_only_hides_parts_and_keeps_preserved_data() {
    let fixture = Fixture::new();
    apply(&fixture.target, &Scope::automatic());

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
    assert!(matches!(
        outcome(ComponentId::CoordinationRetirement),
        RemoveOutcome::Kept { .. }
    ));
    let settings = fixture.settings();
    assert!(!settings.contains("hide-subagents"));
    assert_eq!(other_tool_entry(&settings), other_tool_entry(OTHER_TOOL));
    assert!(!fixture.home().join(".local/bin/hide").exists());
    assert!(!fixture.home().join(".hide/kit/installed.json").exists());
}

/// The standalone hcoord plugin is taken out of Herdr once, and the result
/// goes to the report the core logs (B16).
#[test]
fn the_standalone_hcoord_plugin_is_unlinked_once() {
    let fixture = Fixture::new();
    fixture.herdr.plugins.lock().unwrap().push(plugin(
        HCOORD_PLUGIN_ID,
        "/work/herdr-ide/plugins/hcoord",
        "local",
    ));

    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(
        report.legacy_retirement.removed,
        ["hcoord Herdr plugin link (linked folder)"]
    );
    assert!(fixture.herdr.plugins.lock().unwrap().is_empty());
    let calls = fixture.herdr.calls.lock().unwrap().len();
    let again = apply(&fixture.target, &Scope::automatic());
    assert!(again.legacy_retirement.is_empty());
    assert_eq!(
        fixture.herdr.calls.lock().unwrap().len(),
        calls,
        "asked once per machine"
    );
}

/// This Mac's pass takes the labels-era folders and the share folder they
/// leave empty, and never follows a link out of them (B9).
#[test]
fn the_labels_era_folders_go_and_a_link_is_not_followed() {
    let mut fixture = Fixture::new();
    fixture.target.legacy = crate::legacy::local(fixture.home());
    let home = fixture.home().to_path_buf();
    std::fs::create_dir_all(home.join(".local/state/hide-plugin-upgrade/20260920T110346Z"))
        .unwrap();
    std::fs::create_dir_all(home.join(".local/share/hide/agent-context-labels")).unwrap();
    std::fs::write(
        home.join(".local/share/hide/agent-context-labels/watcher.out"),
        "x",
    )
    .unwrap();
    std::fs::create_dir_all(home.join(".local/state/claude")).unwrap();

    let report = apply(&fixture.target, &Scope::automatic());

    assert!(report.legacy_retirement.failures.is_empty(), "{report:?}");
    assert!(!home.join(".local/state/hide-plugin-upgrade").exists());
    assert!(!home.join(".local/share/hide").exists());
    assert!(home.join(".local/share").is_dir() && home.join(".local/state/claude").is_dir());

    let precious = fixture.root.join("precious");
    std::fs::create_dir_all(&precious).unwrap();
    std::fs::write(precious.join("keep"), "x").unwrap();
    std::os::unix::fs::symlink(&precious, home.join(".local/state/hide-plugin-upgrade")).unwrap();
    let report = apply(&fixture.target, &Scope::automatic());
    assert!(precious.join("keep").is_file());
    assert!(
        report
            .legacy_retirement
            .failures
            .iter()
            .any(|failure| failure.contains("not a folder of this account")),
        "{report:?}"
    );
}

/// A device's old helper root stays while a hook entry still names it and
/// goes on the pass after nothing does; folders it did not make stay (B19,
/// B22).
#[test]
fn the_old_helper_root_goes_only_once_nothing_names_it() {
    let mut fixture = Fixture::new();
    let home = fixture.home().to_path_buf();
    let root = crate::layout::helper_root(&home);
    std::fs::create_dir_all(&root).unwrap();
    fixture.target.legacy = crate::legacy::device(&home, &root);
    let old = crate::layout::legacy_helper_root(&home);
    std::fs::create_dir_all(old.join("0123456789abcdef")).unwrap();
    std::os::unix::fs::symlink("0123456789abcdef", old.join("current")).unwrap();
    let codex = home.join(".codex/hooks.json");
    std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
    std::fs::write(
        &codex,
        format!(
            "{{\"hooks\":{{\"Stop\":[{{\"command\":\"{}/current/hide-agent-hooks\"}}]}}}}",
            old.display()
        ),
    )
    .unwrap();

    let report = apply(&fixture.target, &Scope::automatic());
    assert!(
        old.join("current").exists(),
        "kept while the Codex hook names it"
    );
    assert!(
        report
            .legacy_retirement
            .failures
            .iter()
            .any(|failure| failure.contains("still names it")),
        "{report:?}"
    );

    std::fs::write(&codex, "{}").unwrap();
    let report = apply(&fixture.target, &Scope::automatic());
    assert!(!old.exists(), "{report:?}");
    assert!(!home.join(".local/share/hide").exists());
    assert!(home.join(".local/share").is_dir());

    // Under any other root nothing of the old layout is touched (D-12).
    assert!(crate::legacy::device(&home, &fixture.root.join("helper-root")).is_empty());
}

#[test]
fn a_recorded_command_is_upgraded_after_its_old_package_is_deleted() {
    let fixture = Fixture::new();
    let old_resources = fixture.home().join("Old Hide/resources");
    std::fs::create_dir_all(&old_resources).unwrap();
    executable(&old_resources.join("hide"), "#!/bin/sh\nexit 0\n");
    let mut old = fixture.target.clone();
    old.kit_dir = old_resources.clone();
    assert_eq!(
        state(&apply(&old, &Scope::automatic()), ComponentId::Cli),
        ComponentState::Installed
    );
    std::fs::remove_dir_all(old_resources.parent().unwrap()).unwrap();
    assert_eq!(
        state(
            &apply(&fixture.target, &Scope::automatic()),
            ComponentId::Cli
        ),
        ComponentState::Installed
    );
    assert_eq!(
        std::fs::read_link(fixture.home().join(".local/bin/hide")).unwrap(),
        fixture.target.kit_dir.join("hide")
    );
}
/// A `codex` that keeps `daemon_auto_start` in `$CODEX_HOME/daemon` the way
/// `codex features` reports it, logs every call, and is told by files in
/// HOME to be an old Codex (`codex-old`), to fail (`codex-fails`) or to have a
/// daemon answering (`daemon-running`).
fn fake_codex(fixture: &mut Fixture, daemon: &str) -> PathBuf {
    let codex = fixture.root.join("bin/codex");
    executable(
        &codex,
        concat!(
            "#!/bin/sh\n",
            "echo \"$@\" >> \"$HOME/codex.log\"\n",
            "if [ -e \"$HOME/codex-fails\" ]; then echo 'Error: config.toml is locked' >&2; exit 1; fi\n",
            "case \"$1 $2\" in\n",
            "  'features list')\n",
            "    echo 'apps                 stable  true'\n",
            "    [ -e \"$HOME/codex-old\" ] || echo \"daemon_auto_start    stable  $(cat \"$CODEX_HOME/daemon\" 2>/dev/null || echo true)\" ;;\n",
            "  'features disable') echo false > \"$CODEX_HOME/daemon\" ;;\n",
            "  'features enable') echo true > \"$CODEX_HOME/daemon\" ;;\n",
            "  'app-server daemon')\n",
            "    [ -e \"$HOME/daemon-running\" ] || { echo 'Error: failed to connect' >&2; exit 1; }\n",
            "    echo '{\"status\":\"running\"}' ;;\n",
            "  *) exit 2 ;;\n",
            "esac\n",
        ),
    );
    let folder = fixture.home().join(".codex");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("daemon"), format!("{daemon}\n")).unwrap();
    fixture.target.codex = Some(codex.clone());
    codex
}

impl Fixture {
    fn daemon_setting(&self) -> String {
        std::fs::read_to_string(self.home().join(".codex/daemon"))
            .unwrap()
            .trim()
            .to_owned()
    }

    /// The `codex features enable|disable` calls the kit made, in order.
    fn codex_writes(&self) -> Vec<String> {
        std::fs::read_to_string(self.home().join("codex.log"))
            .unwrap_or_default()
            .lines()
            .filter(|line| {
                line.starts_with("features enable") || line.starts_with("features disable")
            })
            .map(str::to_owned)
            .collect()
    }
}

#[test]
fn the_first_pass_turns_the_codex_daemon_off_once_and_then_converges() {
    let mut fixture = Fixture::new();
    fake_codex(&mut fixture, "true");

    let report = apply(&fixture.target, &Scope::automatic());
    assert_eq!(
        state(&report, ComponentId::CodexPerPane),
        ComponentState::Installed,
        "{report:?}"
    );
    assert_eq!(fixture.daemon_setting(), "false");
    let part = report.component(ComponentId::CodexPerPane).unwrap();
    assert_eq!(part.reason, None);
    assert_eq!(
        part.location.as_deref(),
        Some(fixture.home().join(".codex/config.toml").to_str().unwrap())
    );

    for _ in 0..2 {
        let report = apply(&fixture.target, &Scope::automatic());
        assert_eq!(
            state(&report, ComponentId::CodexPerPane),
            ComponentState::Installed
        );
    }
    assert_eq!(
        fixture.codex_writes(),
        ["features disable daemon_auto_start"]
    );
}

#[test]
fn a_codex_already_running_per_pane_is_not_written_to() {
    let mut fixture = Fixture::new();
    fake_codex(&mut fixture, "false");

    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(
        state(&report, ComponentId::CodexPerPane),
        ComponentState::Installed
    );
    assert!(fixture.codex_writes().is_empty());
}

#[test]
fn turning_the_part_off_gives_codex_its_daemon_back_and_no_pass_reverses_it() {
    let mut fixture = Fixture::new();
    fake_codex(&mut fixture, "true");
    apply(&fixture.target, &Scope::automatic());

    let report = apply(
        &fixture.target,
        &Scope::turn_off([ComponentId::CodexPerPane]),
    );
    assert_eq!(
        state(&report, ComponentId::CodexPerPane),
        ComponentState::Off,
        "{report:?}"
    );
    assert!(!ComponentState::Off.needs_attention());
    assert_eq!(fixture.daemon_setting(), "true");

    let report = apply(&fixture.target, &Scope::automatic());
    assert_eq!(
        state(&report, ComponentId::CodexPerPane),
        ComponentState::Off
    );
    assert_eq!(fixture.daemon_setting(), "true");

    // Turning it on is the operator asking for it back.
    let report = apply(
        &fixture.target,
        &Scope::reinstall([ComponentId::CodexPerPane]),
    );
    assert_eq!(
        state(&report, ComponentId::CodexPerPane),
        ComponentState::Installed
    );
    assert_eq!(
        fixture.codex_writes(),
        [
            "features disable daemon_auto_start",
            "features enable daemon_auto_start",
            "features disable daemon_auto_start",
        ]
    );
}

#[test]
fn a_daemon_setting_turned_back_on_outside_hide_reads_as_off() {
    let mut fixture = Fixture::new();
    fake_codex(&mut fixture, "true");
    apply(&fixture.target, &Scope::automatic());
    std::fs::write(fixture.home().join(".codex/daemon"), "true\n").unwrap();

    let report = apply(&fixture.target, &Scope::automatic());

    assert_eq!(
        state(&report, ComponentId::CodexPerPane),
        ComponentState::Off
    );
    assert_eq!(fixture.daemon_setting(), "true");
    assert_eq!(
        fixture.codex_writes(),
        ["features disable daemon_auto_start"]
    );
}

#[test]
fn a_machine_without_a_codex_that_has_the_daemon_is_not_applicable() {
    // No codex at all.
    let fixture = Fixture::new();
    let report = apply(&fixture.target, &Scope::automatic());
    assert_eq!(
        state(&report, ComponentId::CodexPerPane),
        ComponentState::Absent
    );

    // A Codex never set up here: capability is read, no configuration is made.
    let mut fixture = Fixture::new();
    fake_codex(&mut fixture, "true");
    std::fs::remove_dir_all(fixture.home().join(".codex")).unwrap();
    let report = apply(&fixture.target, &Scope::automatic());
    assert_eq!(
        state(&report, ComponentId::CodexPerPane),
        ComponentState::Absent
    );
    assert!(!fixture.home().join(".codex").exists());
    assert_eq!(
        report
            .component(ComponentId::CodexPerPane)
            .unwrap()
            .codex_daemon,
        Some(true)
    );
    assert!(fixture.codex_writes().is_empty());

    // An older Codex that has no such setting.
    let mut fixture = Fixture::new();
    fake_codex(&mut fixture, "true");
    std::fs::write(fixture.home().join("codex-old"), "").unwrap();
    let report = apply(&fixture.target, &Scope::automatic());
    assert_eq!(
        state(&report, ComponentId::CodexPerPane),
        ComponentState::Absent,
        "{report:?}"
    );
    assert!(fixture.codex_writes().is_empty());
    assert_eq!(
        report
            .component(ComponentId::CodexPerPane)
            .unwrap()
            .codex_daemon,
        Some(false)
    );
}

#[test]
fn a_codex_that_fails_marks_only_its_part_and_changes_nothing() {
    let mut fixture = Fixture::new();
    fake_codex(&mut fixture, "true");
    std::fs::write(fixture.home().join("codex-fails"), "").unwrap();

    let report = apply(&fixture.target, &Scope::automatic());

    let part = report.component(ComponentId::CodexPerPane).unwrap();
    assert_eq!(part.state, ComponentState::Failed);
    assert!(
        part.reason
            .as_deref()
            .is_some_and(|reason| reason.contains("config.toml is locked")),
        "{part:?}"
    );
    assert_eq!(
        state(&report, ComponentId::ClaudeCodeHook),
        ComponentState::Installed
    );
    assert_eq!(fixture.daemon_setting(), "true");
}

#[test]
fn a_running_daemon_is_left_alone_and_said_on_the_row_until_it_goes_down() {
    let mut fixture = Fixture::new();
    fake_codex(&mut fixture, "true");
    std::fs::write(fixture.home().join("daemon-running"), "").unwrap();

    let report = apply(&fixture.target, &Scope::automatic());
    let part = report.component(ComponentId::CodexPerPane).unwrap();
    assert_eq!(part.state, ComponentState::Installed);
    assert_eq!(part.reason.as_deref(), Some("새로 여는 Codex부터 적용"));
    let log = std::fs::read_to_string(fixture.home().join("codex.log")).unwrap();
    assert!(!log.contains("daemon stop"), "{log}");

    std::fs::remove_file(fixture.home().join("daemon-running")).unwrap();
    let part = status(&fixture.target)
        .component(ComponentId::CodexPerPane)
        .cloned()
        .unwrap();
    assert_eq!(part.state, ComponentState::Installed);
    assert_eq!(part.reason, None);
}

#[test]
fn a_later_choice_for_one_part_wins_when_two_requests_merge() {
    let merged = Scope::turn_off([ComponentId::CodexPerPane]).merge(Scope::reinstall([
        ComponentId::CodexPerPane,
        ComponentId::Cli,
    ]));
    assert_eq!(
        merged,
        Scope::reinstall([ComponentId::CodexPerPane, ComponentId::Cli])
    );
    let merged = Scope::reinstall([ComponentId::CodexPerPane, ComponentId::Cli])
        .merge(Scope::turn_off([ComponentId::CodexPerPane]));
    assert_eq!(merged.restore, BTreeSet::from([ComponentId::Cli]));
    assert_eq!(merged.turn_off, BTreeSet::from([ComponentId::CodexPerPane]));
    assert!(Scope::automatic().merge(Scope::automatic()).is_automatic());
}

fn old_coordination(fixture: &Fixture, requests: Value, watches: Value) -> PathBuf {
    let home = fixture.home().join(".hide/hcoord");
    executable(&home.join("bin/hcoord"), "#!/bin/sh\n");
    std::fs::write(home.join("ledger.json"), json!({"schema":"hcoord.ledger.v1", "requests":requests, "watches":watches, "retained":"verbatim history"}).to_string()).unwrap();
    home
}

fn home_tree(home: &Path) -> std::collections::BTreeMap<PathBuf, (u32, Option<Vec<u8>>)> {
    let mut result = std::collections::BTreeMap::new();
    let mut pending = vec![home.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let metadata = std::fs::symlink_metadata(entry.path()).unwrap();
            let contents = if metadata.is_file() {
                Some(std::fs::read(entry.path()).unwrap())
            } else {
                None
            };
            result.insert(
                entry.path().strip_prefix(home).unwrap().to_path_buf(),
                (metadata.permissions().mode(), contents),
            );
            if metadata.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    result
}

#[test]
fn open_legacy_requests_and_active_watches_leave_the_entire_home_unchanged() {
    for (requests, watches) in [
        (json!({"r":{"status":"open"}}), json!({})),
        (json!({}), json!({"w":{"status":"active"}})),
    ] {
        let fixture = Fixture::new();
        old_coordination(&fixture, requests, watches);
        let before = home_tree(fixture.home());
        let report = apply(&fixture.target, &Scope::automatic());
        assert_eq!(
            state(&report, ComponentId::CoordinationRetirement),
            ComponentState::Failed
        );
        assert_eq!(home_tree(fixture.home()), before);
        assert!(fixture.herdr.calls.lock().unwrap().is_empty());
    }
}

#[test]
fn supervisor_revision_head_and_unreadable_referenced_runs_block_before_mutation() {
    for (unreadable, owner) in [(false, "hcoord"), (true, "hcoord"), (false, "hide")] {
        let fixture = Fixture::new();
        old_coordination(&fixture, json!({}), json!({}));
        let registry = fixture.home().join(".sasu/supervisor");
        std::fs::create_dir_all(&registry).unwrap();
        let state_path = fixture.root.join("run-state.json");
        if !unreadable {
            std::fs::write(&state_path, json!({"status":"active","supervision":{"runInstanceId":"run-1","coordinationOwner":owner}}).to_string()).unwrap();
        }
        std::fs::write(registry.join("index.json"), json!({"schema":"sasu.supervisor.index.v1","entries":[],"coordinated":[],"tickExecutor":null}).to_string()).unwrap();
        std::fs::write(registry.join("index.json.revision-000000000002"), json!({"schema":"sasu.supervisor.index.v1","entries":[],"coordinated":[{"statePath":state_path,"runInstanceId":"run-1"}],"tickExecutor":null}).to_string()).unwrap();
        let before = home_tree(fixture.home());
        let report = apply(&fixture.target, &Scope::automatic());
        assert_eq!(
            state(&report, ComponentId::CoordinationRetirement),
            ComponentState::Failed
        );
        assert_eq!(home_tree(fixture.home()), before);
        assert!(fixture.herdr.calls.lock().unwrap().is_empty());
    }
}

#[test]
fn hide_waiting_requests_and_watches_block_retirement() {
    for (letters, watches) in [
        (
            json!([{"state":"delivered","waiting_answer":true}]),
            json!([]),
        ),
        (json!([]), json!([{"id":"watch-1"}])),
    ] {
        let fixture = Fixture::new();
        let state_directory = fixture.home().join(".hide/state");
        std::fs::create_dir_all(&state_directory).unwrap();
        std::fs::write(
            state_directory.join("delivery-ledger.json"),
            json!({"version":1,"letters":letters,"watches":watches}).to_string(),
        )
        .unwrap();
        let before = home_tree(fixture.home());
        assert_eq!(
            state(
                &apply(&fixture.target, &Scope::automatic()),
                ComponentId::CoordinationRetirement
            ),
            ComponentState::Failed
        );
        assert_eq!(home_tree(fixture.home()), before);
    }
}

fn loaded_agent(fixture: &Fixture) {
    std::fs::write(fixture.home().join("launchctl-loaded"), "loaded").unwrap();
    executable(
        &fixture.root.join("launchctl"),
        concat!(
            "#!/bin/sh\n",
            "printf '%s %s\\n' \"$1\" \"$2\" >> \"$HOME/launchctl.log\"\n",
            "if [ \"$1\" = print ]; then [ -f \"$HOME/launchctl-loaded\" ] && exit 0; exit 113; fi\n",
            "if [ -f \"$HOME/bootout-fails\" ]; then exit 5; fi\n",
            "/bin/rm \"$HOME/launchctl-loaded\"\n",
        ),
    );
}

#[test]
fn retirement_stops_the_fixture_daemon_removes_only_owned_links_and_preserves_bytes() {
    let mut fixture = Fixture::new();
    let old = old_coordination(
        &fixture,
        json!({"r":{"status":"answered"}}),
        json!({"w":{"status":"stopped"}}),
    );
    let bytes = std::fs::read(old.join("ledger.json")).unwrap();
    loaded_agent(&fixture);
    fixture.target.cli_dir = fixture.root.join("owned-bin");
    std::fs::create_dir_all(&fixture.target.cli_dir).unwrap();
    std::os::unix::fs::symlink(
        old.join("bin/hcoord"),
        fixture.target.cli_dir.join("hcoord"),
    )
    .unwrap();
    std::fs::create_dir_all(fixture.home().join(".local/bin")).unwrap();
    let foreign = fixture.home().join(".local/bin/hcoord");
    std::os::unix::fs::symlink("/another/tool/hcoord", &foreign).unwrap();
    executable(
        &fixture.home().join(".hide/kit/hcoord/dist/cli.js"),
        "old code",
    );
    let listener = UnixListener::bind(old.join("api.sock")).unwrap();
    let daemon = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = String::new();
        BufReader::new(&stream).read_line(&mut request).unwrap();
        let request: Value = serde_json::from_str(&request).unwrap();
        assert_eq!(request["operation"], "daemon.stop");
        drop(listener);
        writeln!(stream, "{{\"ok\":true,\"value\":{{\"stopped\":true}}}}").unwrap();
    });
    let report = apply(&fixture.target, &Scope::automatic());
    daemon.join().unwrap();
    assert_eq!(
        state(&report, ComponentId::CoordinationRetirement),
        ComponentState::Installed,
        "{report:?}"
    );
    assert!(!old.exists());
    assert!(!fixture.target.cli_dir.join("hcoord").exists());
    assert_eq!(
        std::fs::read_link(foreign).unwrap(),
        PathBuf::from("/another/tool/hcoord")
    );
    assert!(!fixture.home().join(".hide/kit/hcoord").exists());
    let preserved = std::fs::read_dir(fixture.home().join(".hide"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("hcoord.retired-")
        })
        .unwrap();
    assert_eq!(std::fs::read(preserved.join("ledger.json")).unwrap(), bytes);
    let calls = std::fs::read_to_string(fixture.home().join("launchctl.log")).unwrap();
    assert!(
        calls.contains("bootout fixture/com.hcoord.daemon."),
        "{calls}"
    );
    assert!(
        !calls
            .lines()
            .any(|line| line.ends_with("fixture/com.hcoord.daemon"))
    );
    let before = home_tree(fixture.home());
    apply(&fixture.target, &Scope::automatic());
    assert_eq!(home_tree(fixture.home()), before);
}

#[test]
fn failed_bootout_is_visible_and_a_second_pass_resumes_without_rollback() {
    let fixture = Fixture::new();
    let old = old_coordination(&fixture, json!({}), json!({}));
    let bytes = std::fs::read(old.join("ledger.json")).unwrap();
    loaded_agent(&fixture);
    std::fs::write(fixture.home().join("bootout-fails"), "fail").unwrap();
    let first = apply(&fixture.target, &Scope::automatic());
    assert_eq!(
        state(&first, ComponentId::CoordinationRetirement),
        ComponentState::Failed
    );
    let saved = status(&fixture.target);
    assert!(
        saved
            .component(ComponentId::CoordinationRetirement)
            .unwrap()
            .reason
            .as_ref()
            .unwrap()
            .contains("remove login agent failed")
    );
    assert_eq!(std::fs::read(old.join("ledger.json")).unwrap(), bytes);
    assert!(fixture.home().join("launchctl-loaded").exists());
    std::fs::remove_file(fixture.home().join("bootout-fails")).unwrap();
    let second = apply(&fixture.target, &Scope::automatic());
    assert_eq!(
        state(&second, ComponentId::CoordinationRetirement),
        ComponentState::Installed,
        "{second:?}"
    );
    assert!(!old.exists());
    assert!(!fixture.home().join("launchctl-loaded").exists());
}

#[test]
fn a_registered_checkout_with_an_active_run_blocks_before_mutation() {
    let mut fixture = Fixture::new();
    let project = fixture.root.join("project");
    let state_path = project.join("agents/runs/task/state.json");
    std::fs::create_dir_all(state_path.parent().unwrap()).unwrap();
    std::fs::write(&state_path, json!({"status":"active"}).to_string()).unwrap();
    fixture.target.retirement_projects.push(project);
    let before = home_tree(fixture.home());
    assert_eq!(
        state(
            &apply(&fixture.target, &Scope::automatic()),
            ComponentId::CoordinationRetirement
        ),
        ComponentState::Failed
    );
    assert_eq!(home_tree(fixture.home()), before);
}

#[test]
fn identified_sasu_run_with_unknown_status_blocks_and_other_artifacts_do_not() {
    for (state, blocked) in [
        (
            json!({"schema":"sasu.implement.state.v10", "status":"unknown"}),
            true,
        ),
        (
            json!({"schema":"sasu.implement.state.v11.stateless-verification"}),
            true,
        ),
        (json!({"schema":"another-tool.v1"}), false),
        (
            json!({"schema":"sasu.implement.state.v10", "status":"retired"}),
            false,
        ),
    ] {
        let mut fixture = Fixture::new();
        let project = fixture.root.join("project");
        let path = project.join("agents/runs/task/state.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, state.to_string()).unwrap();
        fixture.target.retirement_projects.push(project);
        let before = home_tree(fixture.home());
        assert_eq!(retirement_preflight(&fixture.target).is_err(), blocked);
        assert_eq!(home_tree(fixture.home()), before);
    }
}

#[test]
fn intermediate_kit_links_refuse_before_services_status_or_any_mutation() {
    for linked_component in [".hide", ".hide/kit"] {
        let mut fixture = Fixture::new();
        let outside = fixture.root.join("outside-owned-folder");
        let copy = if linked_component == ".hide" {
            outside.join("kit/hcoord")
        } else {
            outside.join("hcoord")
        };
        executable(&copy.join("dist/cli.js"), "unrelated preserved bytes");
        std::fs::write(outside.join("notes.txt"), b"another folder's files").unwrap();
        let link = fixture.home().join(linked_component);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        executable(
            &fixture.root.join("launchctl"),
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/service-called\"\nexit 113\n",
        );
        let codex = fixture.root.join("bin/codex");
        executable(
            &codex,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/status-called\"\nexit 1\n",
        );
        fixture.target.codex = Some(codex);
        let daemon_home = fixture.home().join(".hcoord");
        std::fs::create_dir_all(&daemon_home).unwrap();
        let daemon = UnixListener::bind(daemon_home.join("api.sock")).unwrap();
        daemon.set_nonblocking(true).unwrap();
        let before_home = home_tree(fixture.home());
        let before_outside = home_tree(&outside);
        let observed = status(&fixture.target);
        assert_eq!(
            state(&observed, ComponentId::CoordinationRetirement),
            ComponentState::Failed,
            "{observed:?}"
        );
        let report = apply(&fixture.target, &Scope::automatic());
        assert_eq!(
            state(&report, ComponentId::CoordinationRetirement),
            ComponentState::Failed,
            "{report:?}"
        );
        assert!(
            matches!(daemon.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "a daemon RPC was initiated"
        );
        assert_eq!(
            home_tree(&outside),
            before_outside,
            "the intermediate link target changed"
        );
        assert_eq!(
            home_tree(fixture.home()),
            before_home,
            "preflight wrote to HOME"
        );
        assert!(fixture.herdr.calls.lock().unwrap().is_empty());
        assert!(!fixture.home().join("service-called").exists());
        assert!(!fixture.home().join("status-called").exists());
        assert!(!outside.join(".lock").exists());
        assert!(!outside.join("installed.json").exists());
        assert_eq!(std::fs::read_link(link).unwrap(), outside);
    }
}

#[test]
fn writable_retirement_records_refuse_before_services_status_or_any_mutation() {
    for record in ["ledger", "progress", "run"] {
        for mode in [0o660, 0o666] {
            let mut fixture = Fixture::new();
            let path = match record {
                "ledger" => old_coordination(&fixture, json!({}), json!({})).join("ledger.json"),
                "progress" => {
                    let path = kit_state_dir(fixture.home()).join("coordination-retirement.json");
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(
                        &path,
                        json!({"homes":[],"step":"complete","failure":null,"complete":true})
                            .to_string(),
                    )
                    .unwrap();
                    path
                }
                _ => {
                    let project = fixture.root.join("project");
                    let path = project.join("agents/runs/task/state.json");
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, json!({"schema":"sasu.implement.state.v11.stateless-verification", "status":"retired"}).to_string()).unwrap();
                    fixture.target.retirement_projects.push(project);
                    path
                }
            };
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            executable(
                &fixture.root.join("launchctl"),
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/service-called\"\nexit 113\n",
            );
            let codex = fixture.root.join("bin/codex");
            executable(
                &codex,
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/status-called\"\nexit 1\n",
            );
            fixture.target.codex = Some(codex);
            let daemon_home = fixture.home().join(".hcoord");
            std::fs::create_dir_all(&daemon_home).unwrap();
            let daemon = UnixListener::bind(daemon_home.join("api.sock")).unwrap();
            daemon.set_nonblocking(true).unwrap();
            let before_home = home_tree(fixture.home());
            let before_record = std::fs::read(&path).unwrap();
            let parent = path.parent().unwrap();
            let before_parent = home_tree(parent);
            let report = apply(&fixture.target, &Scope::automatic());
            assert_eq!(
                state(&report, ComponentId::CoordinationRetirement),
                ComponentState::Failed,
                "{record} {mode:o}: {report:?}"
            );
            assert!(
                matches!(daemon.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
                "a daemon RPC was initiated"
            );
            assert_eq!(std::fs::read(&path).unwrap(), before_record);
            assert_eq!(
                home_tree(parent),
                before_parent,
                "{record} {mode:o}: record folder changed"
            );
            assert_eq!(
                home_tree(fixture.home()),
                before_home,
                "{record} {mode:o}: HOME changed"
            );
            assert!(fixture.herdr.calls.lock().unwrap().is_empty());
            assert!(!fixture.home().join("service-called").exists());
            assert!(!fixture.home().join("status-called").exists());
        }
    }
}
