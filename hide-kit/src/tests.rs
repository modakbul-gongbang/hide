//! The kit against a HOME fixture and a stand-in Herdr socket. Nothing here
//! reads or writes the account running the tests.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;

/// Whether this system is one the kit installs hcoord on (see
/// `hcoord::unsupported_system`); the tests that need its shim, its modes or
/// its removal run only there.
const HCOORD_INSTALLS: bool = cfg!(target_os = "macos");

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
        // hcoord's "Node" is sh, and its cli.js a script that answers the
        // ensure call the way hcoord does and remembers it was asked.
        // `home adopt` moves the old home the way hcoord does, or refuses
        // when the test left an `adopt-fails` file.
        executable(
            &kit.join("hcoord/dist/hcoord/cli.js"),
            concat!(
                "echo \"$@\" >> \"$HOME/ensure.log\"\n",
                "if [ \"$1\" = home ]; then\n",
                "  if [ -e \"$HOME/adopt-fails\" ]; then printf '{\"ok\":false,\"error\":{\"code\":\"adopt_failed\",\"message\":\"pid 9 still runs\"}}\\n'; exit 1; fi\n",
                "  mv \"$4\" \"$HOME/.hide/hcoord\" && printf '{\"ok\":true,\"value\":{\"moved\":true}}\\n'; exit 0\n",
                "fi\n",
                "printf '{\"ok\":true,\"value\":{}}\\n'\n",
            ),
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
            hcoord_home: None,
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

    let report = apply(&fixture.target, &Scope::Automatic);

    for id in [ComponentId::Cli, ComponentId::ClaudeCodeHook] {
        assert_eq!(
            state(&report, id),
            ComponentState::Installed,
            "{id:?}: {report:?}"
        );
    }
    // hcoord keeps its daemon running through launchd, so the kit installs it
    // on a Mac and reports it absent anywhere else.
    assert_eq!(
        state(&report, ComponentId::Hcoord),
        if HCOORD_INSTALLS {
            ComponentState::Installed
        } else {
            ComponentState::Absent
        },
        "{report:?}"
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
    if HCOORD_INSTALLS {
        let shim = std::fs::read_to_string(fixture.home().join(".hide/hcoord/bin/hcoord")).unwrap();
        assert!(shim.starts_with("#!/bin/sh\nHCOORD_TEST='1' exec '/bin/sh' '"));
        assert!(
            std::fs::read_to_string(fixture.home().join("ensure.log"))
                .unwrap()
                .contains("daemon ensure --json")
        );
    } else {
        assert!(!fixture.home().join(".hide/hcoord").exists());
    }
    let record = std::fs::read_to_string(fixture.home().join(".hide/kit/installed.json")).unwrap();
    for code in ["cli", "claude_code_hook"] {
        assert!(record.contains(code), "{record}");
    }
    assert_eq!(record.contains("hcoord"), HCOORD_INSTALLS, "{record}");
    assert!(!record.contains("codex_hook"));
}

#[test]
fn a_second_apply_of_the_same_build_changes_nothing() {
    let fixture = Fixture::new();
    apply(&fixture.target, &Scope::Automatic);
    let settings = fixture.settings();
    let record_path = fixture.home().join(".hide/kit/installed.json");
    let record_written = std::fs::metadata(&record_path).unwrap().modified().unwrap();
    let calls = fixture.herdr.calls.lock().unwrap().len();

    let report = apply(&fixture.target, &Scope::Automatic);

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
    apply(&fixture.target, &Scope::Automatic);
    hide_agent_hooks::remove(hide_agent_hooks::AgentRuntime::ClaudeCode, fixture.home()).unwrap();

    let report = apply(&fixture.target, &Scope::Automatic);
    assert_eq!(
        state(&report, ComponentId::ClaudeCodeHook),
        ComponentState::Removed
    );
    assert!(!fixture.settings().contains("hide-subagents"));

    let report = apply(
        &fixture.target,
        &Scope::Reinstall(vec![ComponentId::ClaudeCodeHook]),
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

    // A target that climbs out of a folder Hide owns is not Hide's.
    std::fs::remove_file(&link).unwrap();
    let climbing = fixture.target.owned_roots[0].join("../elsewhere/hide");
    std::os::unix::fs::symlink(&climbing, &link).unwrap();
    let report = apply(&fixture.target, &Scope::Automatic);
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

    let report = apply(&fixture.target, &Scope::Automatic);

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
    let report = apply(&fixture.target, &Scope::Automatic);
    assert!(report.labels_retirement.is_empty());
    assert_eq!(fixture.herdr.calls.lock().unwrap().len(), calls);
}

#[test]
fn a_github_install_of_the_labels_plugin_is_taken_out_through_the_herdr_command() {
    let fixture = Fixture::new();
    fixture.legacy_plugin("github");

    let report = apply(&fixture.target, &Scope::Automatic);

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

    let report = apply(&fixture.target, &Scope::Automatic);
    assert!(
        report.labels_retirement.failures[0].contains("plugin.list"),
        "{report:?}"
    );
    assert!(labels_home(fixture.home()).exists());
    assert!(plugin_state_dir(fixture.home()).exists());
    assert_eq!(state(&report, ComponentId::Cli), ComponentState::Installed);

    fixture.target.herdr_socket = fixture.herdr.socket.clone();
    let report = apply(&fixture.target, &Scope::Automatic);
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

    let report = apply(&fixture.target, &Scope::Automatic);

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

    let report = apply(&fixture.target, &Scope::Automatic);

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
    use std::os::fd::AsRawFd;
    let fixture = Fixture::new();
    fixture.legacy_plugin("local");
    let lock = plugin_state_dir(fixture.home()).join("watcher.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    let held = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&lock)
        .unwrap();
    // SAFETY: flock on a descriptor `held` owns until the end of the test.
    assert_eq!(
        unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );

    let report = apply(&fixture.target, &Scope::Automatic);

    assert!(!report.labels_retirement.failures.is_empty(), "{report:?}");
    assert!(lock.exists(), "the watcher's lock is gone: {report:?}");
    drop(held);
}

#[cfg(target_os = "macos")]
#[test]
fn without_a_runtime_for_hcoord_it_fails_and_an_existing_shim_is_left() {
    let mut fixture = Fixture::new();
    let shim = fixture.home().join(".hide/hcoord/bin/hcoord");
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

/// The kit keeps code that launchd and Herdr run under `~/.hide/kit`: a
/// fresh folder is the account's alone, and one another account can write to
/// is refused rather than run from.
#[cfg(target_os = "macos")]
#[test]
fn the_kit_runs_nothing_from_a_folder_another_account_can_change() {
    let fixture = Fixture::new();
    apply(&fixture.target, &Scope::Automatic);
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let state = kit_state_dir(&fixture.target.home);
    assert_eq!(mode(&state), 0o700);
    assert_eq!(mode(state.parent().unwrap()), 0o700);
    assert_eq!(mode(&fixture.target.home.join(".hide/hcoord/bin")), 0o700);

    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o775)).unwrap();
    let report = status(&fixture.target);
    let part = report.component(ComponentId::Hcoord).unwrap();
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
    let waiting = std::thread::spawn(move || apply(&target, &Scope::Automatic));
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
    let report = apply(&fixture.target, &Scope::Automatic);
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

    let report = apply(&fixture.target, &Scope::Automatic);

    let cli = report.component(ComponentId::Cli).unwrap();
    assert_eq!(cli.state, ComponentState::Failed);
    assert!(cli.reason.as_deref().unwrap().contains("installed.json"));
    assert!(!fixture.home().join(".local/bin/hide").exists());
    assert_eq!(std::fs::read_to_string(&record).unwrap(), "not a record");
}

#[cfg(target_os = "macos")]
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
    assert!(matches!(
        outcome(ComponentId::Hcoord),
        RemoveOutcome::Kept { .. }
    ));
    let settings = fixture.settings();
    assert!(!settings.contains("hide-subagents"));
    assert_eq!(other_tool_entry(&settings), other_tool_entry(OTHER_TOOL));
    assert!(!fixture.home().join(".local/bin/hide").exists());
    assert!(fixture.home().join(".hide/hcoord/bin/hcoord").is_file());
    assert!(!fixture.home().join(".hide/kit/installed.json").exists());
}

/// The old `~/.hcoord` as an earlier Hide left it, and a record that says
/// hcoord was installed: the new shim is missing, which must not read as
/// taken away (PRD hide-home-layout D-09, B10, B11).
#[cfg(target_os = "macos")]
fn installed_under_the_old_home(fixture: &Fixture) -> PathBuf {
    let old = fixture.home().join(".hcoord");
    executable(
        &old.join("bin/hcoord"),
        "#!/bin/sh\nexec '/opt/node' '/x/cli.js' \"$@\"\n",
    );
    std::fs::write(old.join("ledger.json"), "{\"kept\":true}").unwrap();
    let record = fixture.home().join(".hide/kit/installed.json");
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::write(&record, "{\"format\":1,\"installed\":[\"cli\",\"hcoord\"]}").unwrap();
    old
}

#[cfg(target_os = "macos")]
#[test]
fn a_recorded_hcoord_in_the_old_home_is_moved_before_the_new_copy_and_linked_on_path() {
    let fixture = Fixture::new();
    let old = installed_under_the_old_home(&fixture);
    let before = status(&fixture.target);
    assert_eq!(
        state(&before, ComponentId::Hcoord),
        ComponentState::Outdated
    );

    let report = apply(&fixture.target, &Scope::Automatic);

    assert_eq!(
        state(&report, ComponentId::Hcoord),
        ComponentState::Installed,
        "{report:?}"
    );
    assert!(!old.exists(), "no copy of the old home is left");
    let home = fixture.home().join(".hide/hcoord");
    assert_eq!(
        std::fs::read_to_string(home.join("ledger.json")).unwrap(),
        "{\"kept\":true}"
    );
    let calls = std::fs::read_to_string(fixture.home().join("ensure.log")).unwrap();
    let calls: Vec<&str> = calls.lines().collect();
    assert_eq!(
        calls[0],
        format!("home adopt --from {} --json", old.display())
    );
    assert_eq!(calls[1], "daemon ensure --json", "{calls:?}");
    // The adopt ran from the build's packaged copy, before the installed one changed.
    assert!(
        std::fs::read_to_string(home.join("bin/hcoord"))
            .unwrap()
            .contains(".hide/kit/hcoord/dist/hcoord/cli.js")
    );
    assert_eq!(
        std::fs::read_link(fixture.home().join(".local/bin/hcoord")).unwrap(),
        home.join("bin/hcoord")
    );
    assert_eq!(
        report
            .component(ComponentId::Hcoord)
            .unwrap()
            .location
            .as_deref(),
        Some(home.join("bin/hcoord").display().to_string().as_str())
    );
    let again = apply(&fixture.target, &Scope::Automatic);
    assert_eq!(
        state(&again, ComponentId::Hcoord),
        ComponentState::Installed
    );
    let calls = std::fs::read_to_string(fixture.home().join("ensure.log")).unwrap();
    assert_eq!(
        calls.matches("home adopt").count(),
        1,
        "a second pass moves nothing"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_failed_move_leaves_the_old_home_and_says_how_it_is_retried() {
    let fixture = Fixture::new();
    let old = installed_under_the_old_home(&fixture);
    std::fs::write(fixture.home().join("adopt-fails"), "").unwrap();

    let report = apply(&fixture.target, &Scope::Automatic);

    let part = report.component(ComponentId::Hcoord).unwrap();
    assert_eq!(part.state, ComponentState::Failed);
    let reason = part.reason.as_deref().unwrap();
    assert!(reason.contains("pid 9 still runs"), "{reason}");
    assert!(
        reason.contains("next launch or Reinstall tries again"),
        "{reason}"
    );
    assert_eq!(
        std::fs::read_to_string(old.join("ledger.json")).unwrap(),
        "{\"kept\":true}"
    );
    assert!(
        !fixture.home().join(".hide/kit/hcoord").exists(),
        "the old daemon's code is not replaced"
    );
    let calls = std::fs::read_to_string(fixture.home().join("ensure.log")).unwrap();
    assert!(!calls.contains("daemon ensure"), "{calls}");
}

#[cfg(target_os = "macos")]
#[test]
fn a_relocated_hcoord_is_never_moved() {
    let mut fixture = Fixture::new();
    let old = installed_under_the_old_home(&fixture);
    std::fs::remove_file(fixture.home().join(".hide/kit/installed.json")).unwrap();
    let relocated = fixture.root.join("coordinator");
    fixture.target.hcoord_home = Some(relocated.clone());

    let report = apply(&fixture.target, &Scope::Automatic);

    assert_eq!(
        state(&report, ComponentId::Hcoord),
        ComponentState::Installed,
        "{report:?}"
    );
    assert!(old.join("ledger.json").is_file());
    assert!(relocated.join("bin/hcoord").is_file());
    let calls = std::fs::read_to_string(fixture.home().join("ensure.log")).unwrap();
    assert!(!calls.contains("home adopt"), "{calls}");
}

/// `hcoord` on PATH is linked only when the name is free or already Hide's;
/// another program's stays and the row says so (B14).
#[cfg(target_os = "macos")]
#[test]
fn another_programs_hcoord_on_path_is_left_and_named() {
    let fixture = Fixture::new();
    let theirs = fixture.home().join(".local/bin/hcoord");
    executable(&theirs, "#!/bin/sh\necho theirs\n");

    let report = apply(&fixture.target, &Scope::Automatic);

    let part = report.component(ComponentId::Hcoord).unwrap();
    assert_eq!(part.state, ComponentState::Installed);
    let reason = part.reason.as_deref().unwrap();
    assert!(reason.contains("another program's file"), "{reason}");
    assert_eq!(
        std::fs::read_to_string(&theirs).unwrap(),
        "#!/bin/sh\necho theirs\n"
    );

    // A link to the old home's shim is Hide's and is moved along.
    std::fs::remove_file(&theirs).unwrap();
    std::os::unix::fs::symlink(fixture.home().join(".hcoord/bin/hcoord"), &theirs).unwrap();
    let report = apply(&fixture.target, &Scope::Automatic);
    assert_eq!(report.component(ComponentId::Hcoord).unwrap().reason, None);
    assert_eq!(
        std::fs::read_link(&theirs).unwrap(),
        fixture.home().join(".hide/hcoord/bin/hcoord")
    );
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

    let report = apply(&fixture.target, &Scope::Automatic);

    assert_eq!(
        report.legacy_retirement.removed,
        ["hcoord Herdr plugin link (linked folder)"]
    );
    assert!(fixture.herdr.plugins.lock().unwrap().is_empty());
    let calls = fixture.herdr.calls.lock().unwrap().len();
    let again = apply(&fixture.target, &Scope::Automatic);
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

    let report = apply(&fixture.target, &Scope::Automatic);

    assert!(report.legacy_retirement.failures.is_empty(), "{report:?}");
    assert!(!home.join(".local/state/hide-plugin-upgrade").exists());
    assert!(!home.join(".local/share/hide").exists());
    assert!(home.join(".local/share").is_dir() && home.join(".local/state/claude").is_dir());

    let precious = fixture.root.join("precious");
    std::fs::create_dir_all(&precious).unwrap();
    std::fs::write(precious.join("keep"), "x").unwrap();
    std::os::unix::fs::symlink(&precious, home.join(".local/state/hide-plugin-upgrade")).unwrap();
    let report = apply(&fixture.target, &Scope::Automatic);
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

    let report = apply(&fixture.target, &Scope::Automatic);
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
    let report = apply(&fixture.target, &Scope::Automatic);
    assert!(!old.exists(), "{report:?}");
    assert!(!home.join(".local/share/hide").exists());
    assert!(home.join(".local/share").is_dir());

    // Under any other root nothing of the old layout is touched (D-12).
    assert!(crate::legacy::device(&home, &fixture.root.join("helper-root")).is_empty());
}
