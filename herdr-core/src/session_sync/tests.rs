use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::*;
use crate::model::{CoreOptions, SCHEMA_VERSION};

fn snapshot() -> Value {
    json!({
        "version": "0.8.2",
        "protocol": HERDR_PROTOCOL_REVISION,
        "host": {"host_id": "fixture-host", "session_id": "fixture"},
        "event_sequence": 40,
        "focused_pane_id": "w1:p1",
        "workspaces": [{
            "workspace_id": "w1",
            "label": "fixture",
            "agent_status": "idle", "focused": true, "number": 1, "pane_count": 1, "tab_count": 1,
            "active_tab_id": "w1:t1"
        }],
        "tabs": [{
            "workspace_id": "w1",
            "tab_id": "w1:t1",
            "agent_status": "idle", "focused": false, "number": 1, "pane_count": 1, "label": "1"
        }],
        "panes": [{
            "workspace_id": "w1",
            "tab_id": "w1:t1",
            "pane_id": "w1:p1", "focused": false, "revision": 0, "agent_status": "idle",
            "surface": {"kind": "terminal", "attach": {"terminal_id": "fixture-terminal", "protocol": HERDR_PROTOCOL_REVISION, "transport": "herdr_client", "host": {"host_id": "fixture-host", "session_id": "fixture"}}},
            "cwd": "/tmp/fixture"
        }],
        "layouts": [{
            "workspace_id": "w1",
            "tab_id": "w1:t1",
            "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 120, "height": 60},
            "focused_pane_id": "w1:p1",
            "panes": [{
                "pane_id": "w1:p1", "focused": false,
                "rect": {"x": 0, "y": 0, "width": 120, "height": 60}
            }],
            "splits": []
        }],
        "agents": [],
        "lineage": []
    })
}

#[test]
fn every_session_sync_snapshot_fixture_obeys_the_generated_contract() {
    for (fixture, panes) in [(snapshot(), 1), (two_tab_snapshot(), 2)] {
        let (_, cursor, state) = wire::snapshot(fixture).expect("generated snapshot contract");
        assert_eq!(cursor, 40);
        assert_eq!(state.panes.len(), panes);
    }
}

fn two_tab_snapshot() -> Value {
    let mut value = snapshot();
    value["tabs"]
        .as_array_mut()
        .expect("tabs array")
        .push(json!({
            "workspace_id": "w1",
            "tab_id": "w1:t2",
            "agent_status": "idle", "focused": false, "number": 2, "pane_count": 1, "label": "2"
        }));
    value["panes"]
            .as_array_mut()
            .expect("panes array")
            .push(json!({
                "workspace_id": "w1",
                "tab_id": "w1:t2",
                "pane_id": "w1:p2", "focused": false, "revision": 0, "agent_status": "idle",
                "surface": {"kind": "terminal", "attach": {"terminal_id": "fixture-terminal", "protocol": HERDR_PROTOCOL_REVISION, "transport": "herdr_client", "host": {"host_id": "fixture-host", "session_id": "fixture"}}},
                "cwd": "/tmp/fixture"
            }));
    value["layouts"]
        .as_array_mut()
        .expect("layouts array")
        .push(json!({
            "workspace_id": "w1",
            "tab_id": "w1:t2",
            "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 120, "height": 60},
            "focused_pane_id": "w1:p2",
            "panes": [{
                "pane_id": "w1:p2", "focused": false,
                "rect": {"x": 0, "y": 0, "width": 120, "height": 60}
            }],
            "splits": []
        }));
    value
}

fn event(sequence: u64, kind: &str, data: Value) -> ReplicaEnvelope {
    let raw = json!({
        "protocol": HERDR_PROTOCOL_REVISION,
        "host": {"host_id": "fixture-host", "session_id": "fixture"},
        "sequence": sequence,
        "event": kind,
        "data": data
    });
    match parse_subscription_line(&raw.to_string()).expect("event parses") {
        SubscriptionLine::Event(event) => event,
        SubscriptionLine::Error { .. } => unreachable!(),
    }
}

#[test]
fn worktree_events_invalidate_the_change_driven_reader_once() {
    let value = snapshot();
    let workspace = value["workspaces"][0].clone();
    let mut replica = SessionReplica::from_snapshot(&value).expect("snapshot");
    let opened = event(
        41,
        "worktree_opened",
        json!({
            "type": "worktree_opened",
            "workspace": workspace,
            "worktree": {"path": "/tmp/fixture", "is_bare": false, "is_detached": false, "is_prunable": false, "is_linked_worktree": true, "label": "fixture"},
            "already_open": true
        }),
    );
    let outcome = replica.apply(opened.clone()).expect("worktree event");
    assert!(outcome.refresh_worktrees);
    let duplicate = replica.apply(opened).expect("duplicate event");
    assert!(!duplicate.refresh_worktrees);
}

fn accept_request(listener: &UnixListener) -> (UnixStream, Value) {
    let (stream, _) = listener.accept().expect("accept request");
    let mut line = String::new();
    std::io::BufReader::new(stream.try_clone().expect("clone request stream"))
        .read_line(&mut line)
        .expect("read request");
    let request = serde_json::from_str(&line).expect("request JSON");
    (stream, request)
}

fn write_result(stream: &mut UnixStream, request: &Value, result: Value) {
    writeln!(stream, "{}", json!({"id": request["id"], "result": result})).expect("write response");
}

/// Accepts requests until the coordinator asks for the named method.
///
/// The coordinator refreshes agent telemetry once a second, on its own
/// clock, so an `agent.list` can land between any two requests a test is
/// interested in. Asserting that the very next request is the awaited one
/// therefore asserts the scheduler: these tests failed on it in one run
/// out of three with no change to the code under test. Each interleaved
/// refresh is answered with an empty list, which is what the coordinator
/// expects and what leaves its projection untouched.
fn accept_request_for(listener: &UnixListener, method: &str) -> (UnixStream, Value) {
    for _ in 0..32 {
        let (mut stream, request) = accept_request(listener);
        if request["method"] == method {
            return (stream, request);
        }
        assert_eq!(
            request["method"], "agent.list",
            "only an agent refresh may interleave while {method} is awaited"
        );
        write_result(
            &mut stream,
            &request,
            json!({"type": "agent_list", "agents": []}),
        );
    }
    panic!("the coordinator never asked for {method}");
}

fn wait_until(deadline: Instant, mut predicate: impl FnMut() -> bool) {
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(predicate(), "condition did not become true before deadline");
}

fn runtime_for_fixture(socket_path: &Path, state_path: &Path) -> Arc<Mutex<Runtime>> {
    Arc::new(Mutex::new(Runtime::new(
        CoreOptions {
            schema_version: SCHEMA_VERSION,
            herdr_socket_path: Some(socket_path.to_string_lossy().into_owned()),
            herdr_bin_path: None,
            remote_targets: Vec::new(),
            app_state_path: state_path.to_string_lossy().into_owned(),
        },
        crate::environment::EnvironmentReport {
            statuses: Vec::new(),
            home_path: None,
            remote_enabled: false,
            chromux_enabled: false,
            herdr_socket_path_override: None,
            codex_home: None,
        },
    )))
}

fn context_for_fixture(runtime: &Arc<Mutex<Runtime>>, socket_path: &Path) -> SessionSyncContext {
    let live = LiveContext {
        socket_path: socket_path.to_path_buf(),
        herdr_bin: None,
        runtime: Arc::downgrade(runtime),
        notifier: crate::ffi::ChangeNotifier::noop(),
        api_connector: Arc::new(hide_herdr_client::UnixSocketConnector::new(socket_path)),
    };
    SessionSyncContext::local(&live)
}

fn remove_fixture(root: &Path, socket_path: &Path, state_path: &Path) {
    if socket_path.exists() {
        std::fs::remove_file(socket_path).expect("remove socket");
    }
    if state_path.exists() {
        std::fs::remove_file(state_path).expect("remove state");
    }
    std::fs::remove_dir(root).expect("remove socket directory");
}

#[test]
fn split_is_published_only_after_the_authoritative_layout_arrives() {
    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    let created = replica
            .apply(event(
                41,
                "pane_created",
                json!({
                    "type": "pane_created",
                    "pane": {
                        "workspace_id": "w1",
                        "tab_id": "w1:t1",
                        "pane_id": "w1:p2", "focused": false, "revision": 0, "agent_status": "idle",
                        "surface": {"kind": "terminal", "attach": {"terminal_id": "fixture-terminal", "protocol": HERDR_PROTOCOL_REVISION, "transport": "herdr_client", "host": {"host_id": "fixture-host", "session_id": "fixture"}}},
                        "cwd": "/tmp/fixture"
                    }
                }),
            ))
            .expect("pane event");
    assert!(!created.publish);

    let updated = replica
            .apply(event(
                42,
                "layout_updated",
                json!({
                    "type": "layout_updated",
                    "layout": {
                        "workspace_id": "w1",
                        "tab_id": "w1:t1",
                        "zoomed": false,
                        "area": {"x": 0, "y": 0, "width": 120, "height": 60},
                        "focused_pane_id": "w1:p2",
                        "panes": [
                            {"pane_id": "w1:p1", "focused": false, "rect": {"x": 0, "y": 0, "width": 60, "height": 60}},
                            {"pane_id": "w1:p2", "focused": false, "rect": {"x": 60, "y": 0, "width": 60, "height": 60}}
                        ],
                        "splits": [{
                            "id": "split_0_root",
                            "direction": "right",
                            "ratio": 0.5,
                            "rect": {"x": 0, "y": 0, "width": 120, "height": 60}
                        }]
                    }
                }),
            ))
            .expect("layout event");
    assert!(updated.publish);
    assert_eq!(replica.project().panes.len(), 2);
    assert_eq!(replica.project().layouts[0].panes.len(), 2);
}

#[test]
fn browser_host_identity_follows_pane_updates_and_rejects_remote_attachment() {
    let value = snapshot();
    let mut replica = SessionReplica::from_snapshot(&value).expect("snapshot");
    let mut pane = value["panes"][0].clone();
    pane["tokens"] = json!({
        "hide_content": "browser-v1",
        "hide_browser_binding": "login-qa",
        "hide_browser_profile": "team",
        "hide_browser_target": "A12B",
        "hide_browser_session": "hide-login-qa",
        "hide_browser_cdp_port": "9300",
        "hide_browser_owns_target": "false"
    });
    replica
        .apply(event(
            41,
            "pane_updated",
            json!({"type": "pane_updated", "pane": pane}),
        ))
        .expect("host report");
    let projected = replica.project();
    let content = crate::pane_content::PaneContent::from_tokens(&projected.panes[0].tokens, false);
    assert!(matches!(content,
            crate::pane_content::PaneContent::Browser { target_id, .. } if target_id == "A12B"));
    let (remote, _) = replica.project_remote("mini").expect("remote projection");
    assert!(matches!(
        remote.workspaces[0].checkouts[0].tabs[0].panes[0].content,
        crate::pane_content::PaneContent::Unavailable { .. }
    ));
    // A host release must remove its content identity without leaving a
    // stale browser over a shell that now occupies the same layout leaf.
    pane["tokens"] = json!({});
    replica
        .apply(event(
            42,
            "pane_updated",
            json!({"type": "pane_updated", "pane": pane}),
        ))
        .expect("host release");
    assert!(
        crate::pane_content::PaneContent::from_tokens(&replica.project().panes[0].tokens, false)
            .is_terminal()
    );
}

#[test]
fn agent_projection_keeps_the_herdr_name_for_lineage_hints() {
    let mut value = snapshot();
    value["agents"] = json!([{
        "pane_id": "w1:p1",
        "workspace_id": "w1",
        "tab_id": "w1:t1",
        "terminal_id": "fixture-terminal",
        "focused": false,
        "revision": 1,
        "name": "observer",
        "agent": "claude",
        "agent_status": "working",
        "state_change_seq": 1,
        "tokens": {}
    }]);
    let replica = SessionReplica::from_snapshot(&value).expect("snapshot");

    assert_eq!(replica.project().agents[0].id.as_deref(), Some("observer"));
}

/// B3, D-03. A remote device's project list follows the same activity
/// order the local list follows, not its own alphabetical one. The labels
/// here are deliberately in the opposite order to the activity, so a
/// return to sorting by label fails this.
#[test]
fn remote_projects_follow_activity_order_rather_than_label_order() {
    let mut value = snapshot();
    value["workspaces"][0]["label"] = json!("alpha");
    value["workspaces"]
        .as_array_mut()
        .expect("workspaces array")
        .push(json!({
            "workspace_id": "w2",
            "label": "zulu",
            "agent_status": "idle", "focused": false, "number": 2, "pane_count": 1, "tab_count": 1,
            "active_tab_id": "w2:t1"
        }));
    value["tabs"]
        .as_array_mut()
        .expect("tabs array")
        .push(json!({
            "workspace_id": "w2",
            "tab_id": "w2:t1",
            "agent_status": "idle", "focused": false, "number": 1, "pane_count": 1, "label": "1"
        }));
    value["panes"].as_array_mut().expect("panes array").push(json!({
            "workspace_id": "w2",
            "tab_id": "w2:t1",
            "pane_id": "w2:p1", "focused": false, "revision": 0, "agent_status": "idle",
            "surface": {"kind": "terminal", "attach": {"terminal_id": "fixture-terminal", "protocol": HERDR_PROTOCOL_REVISION, "transport": "herdr_client", "host": {"host_id": "fixture-host", "session_id": "fixture"}}},
            "cwd": "/tmp/fixture-zulu"
        }));
    value["layouts"]
        .as_array_mut()
        .expect("layouts array")
        .push(json!({
            "workspace_id": "w2",
            "tab_id": "w2:t1",
            "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 120, "height": 60},
            "focused_pane_id": "w2:p1",
            "panes": [{
                "pane_id": "w2:p1", "focused": false,
                "rect": {"x": 0, "y": 0, "width": 120, "height": 60}
            }],
            "splits": []
        }));
    value["agents"] = json!([
        {
            "pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1",
            "agent": "codex", "agent_status": "idle", "focused": false, "revision": 0,
            "terminal_id": "fixture-terminal", "state_change_seq": 1,
            "tokens": {"activity": "0000001000000"}
        },
        {
            "pane_id": "w2:p1", "workspace_id": "w2", "tab_id": "w2:t1",
            "agent": "codex", "agent_status": "working", "focused": false, "revision": 0,
            "terminal_id": "fixture-terminal", "state_change_seq": 2,
            "tokens": {"activity": "0000009000000"}
        },
    ]);
    let replica = SessionReplica::from_snapshot(&value).expect("snapshot");

    let (projected, _) = replica.project_remote("mini").expect("remote projection");

    assert_eq!(
        projected
            .workspaces
            .iter()
            .map(|workspace| workspace.label.as_str())
            .collect::<Vec<_>>(),
        ["zulu", "alpha"]
    );
    assert_eq!(
        projected.workspaces[0].last_activity_unix_ms,
        Some(9_000_000)
    );
    assert_eq!(
        projected.workspaces[1].last_activity_unix_ms,
        Some(1_000_000)
    );
}

#[test]
fn remote_projection_uses_target_scoped_ids_and_normalized_layout_frames() {
    let mut value = snapshot();
    value["agents"] = json!([{
        "pane_id": "w1:p1",
        "workspace_id": "w1",
        "tab_id": "w1:t1",
        "agent": "codex",
        "agent_status": "working", "focused": false, "revision": 0, "terminal_id": "fixture-terminal",
        "state_change_seq": 1,
        "tokens": {}
    }]);
    let replica = SessionReplica::from_snapshot(&value).expect("snapshot");

    let (projected, excluded) = replica.project_remote("mini").expect("remote projection");

    assert!(excluded.is_empty());
    assert_eq!(projected.workspaces.len(), 1);
    assert_eq!(projected.workspaces[0].id, "remote:mini:workspace:w1");
    assert_eq!(
        projected.workspaces[0].checkouts[0].id,
        "remote:mini:checkout:w1"
    );
    assert_eq!(
        projected.focused_workspace_id.as_deref(),
        Some("remote:mini:workspace:w1")
    );
    assert_eq!(
        projected.focused_tab_id.as_deref(),
        Some("remote:mini:tab:w1:t1")
    );
    assert_eq!(
        projected.focused_pane_id.as_deref(),
        Some("remote:mini:pane:w1:p1")
    );
    assert_eq!(
        projected.workspaces[0].checkouts[0].tabs[0].id.as_deref(),
        Some("remote:mini:tab:w1:t1")
    );
    assert_eq!(
        projected.workspaces[0].checkouts[0].tabs[0].panes[0].id,
        "remote:mini:pane:w1:p1"
    );
    assert_eq!(projected.agents[0].pane_id, "remote:mini:pane:w1:p1");
    assert_eq!(projected.agents[0].demand, "none");
    assert_eq!(projected.agents[0].activity, "working");
    assert_eq!(projected.pane_layouts[0].frames[0].x, 0.0);
    assert_eq!(projected.pane_layouts[0].frames[0].width, 1.0);
    assert!(projected.workspaces[0].checkouts[0].exists);
    assert_eq!(
        projected
            .active_tab_ids
            .get("remote:mini:workspace:w1")
            .map(String::as_str),
        Some("remote:mini:tab:w1:t1")
    );

    let (other_target, _) = replica.project_remote("build-mini").expect("other target");
    assert_ne!(
        projected.focused_tab_id, other_target.focused_tab_id,
        "the same remote tab id must not collide across targets"
    );
    assert_ne!(
        projected.focused_pane_id, other_target.focused_pane_id,
        "the same remote pane id must not collide across targets"
    );
    assert_ne!(
        projected.agents[0].pane_id, other_target.agents[0].pane_id,
        "agent routing follows the target-scoped pane identity"
    );
}

#[test]
fn remote_projection_preserves_official_worktree_metadata() {
    let mut value = snapshot();
    value["workspaces"][0]["worktree"] = json!({
        "repo_key": "/tmp/repo/.git",
        "repo_name": "repo",
        "repo_root": "/tmp/repo",
        "checkout_path": "/tmp/repo-linked",
        "is_linked_worktree": true
    });
    value["panes"][0]["cwd"] = json!("/tmp/repo-linked/subdirectory");
    let replica = SessionReplica::from_snapshot(&value).expect("snapshot");

    let (projected, _) = replica.project_remote("mini").expect("remote projection");
    let workspace = &projected.workspaces[0];
    let checkout = &workspace.checkouts[0];

    assert_eq!(workspace.path, "/tmp/repo-linked");
    assert_eq!(workspace.repo_name, "repo");
    assert!(workspace.is_git);
    assert_eq!(checkout.path, "/tmp/repo-linked");
    assert!(checkout.is_worktree);
}

#[test]
fn remote_projection_rejects_an_empty_layout_area() {
    let mut value = snapshot();
    value["layouts"][0]["area"]["width"] = json!(0);
    let replica = SessionReplica::from_snapshot(&value).expect("snapshot shape");

    let error = replica
        .project_remote("mini")
        .expect_err("empty layout area must be visible");

    assert_eq!(error.state(), "malformed");
    assert!(error.message().contains("empty area"));
}

#[test]
#[ignore = "requires HERDR_TEST_SSH_ALIAS and HERDR_TEST_SOCKET_PATH"]
fn official_remote_session_coordinator_probe() {
    let alias_name = std::env::var("HERDR_TEST_SSH_ALIAS")
        .expect("HERDR_TEST_SSH_ALIAS names a configured SSH host");
    let socket_path = std::env::var("HERDR_TEST_SOCKET_PATH")
        .expect("HERDR_TEST_SOCKET_PATH is the absolute remote Unix socket path");
    let home = std::env::var_os("HOME").expect("HOME is configured");
    let alias = crate::remote::SshAlias::from_config_file(
        &PathBuf::from(home).join(".ssh/config"),
        &alias_name,
    )
    .expect("SSH alias resolves");
    let client = crate::remote::RusshRemoteClient::new(alias).expect("remote client initializes");
    let connector = client
        .herdr_api_connector(&socket_path)
        .expect("remote connector initializes");
    let state_path = PathBuf::from("/tmp/herdr-core-remote-coordinator-probe-state.json");
    let runtime = Arc::new(Mutex::new(Runtime::new(
        CoreOptions {
            schema_version: SCHEMA_VERSION,
            herdr_socket_path: None,
            herdr_bin_path: None,
            remote_targets: vec![crate::model::RemoteTarget {
                id: "mini".to_owned(),
                label: "Mac mini".to_owned(),
                ssh_alias: alias_name,
                herdr_socket_path: socket_path,
            }],
            app_state_path: state_path.to_string_lossy().into_owned(),
        },
        crate::environment::EnvironmentReport {
            statuses: Vec::new(),
            home_path: None,
            remote_enabled: true,
            chromux_enabled: false,
            herdr_socket_path_override: None,
            codex_home: None,
        },
    )));
    let context = SessionSyncContext::remote(
        "mini",
        "Mac mini",
        Arc::new(connector),
        Arc::downgrade(&runtime),
        crate::ffi::ChangeNotifier::noop(),
    );
    let handle = spawn(context, None).expect("remote coordinator starts");

    wait_until(Instant::now() + Duration::from_secs(5), || {
        runtime.lock().ok().is_some_and(|runtime| {
            let status = &runtime.snapshot().status.remote[0];
            status.state == "connected" && status.session.is_some()
        })
    });
    let runtime = runtime.lock().expect("runtime lock");
    let status = &runtime.snapshot().status.remote[0];
    assert_eq!(status.state, "connected");
    let session = status.session.as_ref().expect("remote session projected");
    assert!(!session.agents.is_empty());
    assert_eq!(
        runtime.snapshot().navigator.devices[1].agent_count as usize,
        session.agents.len()
    );
    drop(runtime);
    drop(handle);
}

#[test]
fn workspace_close_cascade_clears_pending_layout_and_nested_state() {
    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    let pane_closed = replica
        .apply(event(
            41,
            "pane_closed",
            json!({
                "type": "pane_closed",
                "pane_id": "w1:p1",
                "workspace_id": "w1"
            }),
        ))
        .expect("pane close event");
    assert!(!pane_closed.publish);

    let workspace_closed = replica
        .apply(event(
            42,
            "workspace_closed",
            json!({
                "type": "workspace_closed",
                "workspace_id": "w1"
            }),
        ))
        .expect("workspace close event");
    assert!(workspace_closed.publish);
    assert!(replica.ready_to_publish());

    let projected = replica.project();
    assert!(projected.workspaces.is_empty());
    assert!(projected.tabs.is_empty());
    assert!(projected.panes.is_empty());
    assert!(projected.layouts.is_empty());
    assert_eq!(projected.focused_pane_id, None);
}

#[test]
fn last_pane_close_removes_an_implicitly_closed_inactive_tab() {
    let mut replica = SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");

    let pane_closed = replica
        .apply(event(
            41,
            "pane_closed",
            json!({
                "type": "pane_closed",
                "pane_id": "w1:p2",
                "workspace_id": "w1"
            }),
        ))
        .expect("last pane close event");

    assert!(pane_closed.publish);
    assert!(replica.ready_to_publish());
    let projected = replica.project();
    assert_eq!(
        projected
            .tabs
            .iter()
            .map(|tab| tab.tab_id.as_str())
            .collect::<Vec<_>>(),
        vec!["w1:t1"]
    );
    assert!(projected.panes.iter().all(|pane| pane.pane_id != "w1:p2"));
    assert!(
        projected
            .layouts
            .iter()
            .all(|layout| layout.tab_id != "w1:t2")
    );
}

#[test]
fn last_pane_close_waits_for_the_authoritative_fallback_tab_focus() {
    let mut replica = SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");

    let pane_closed = replica
        .apply(event(
            41,
            "pane_closed",
            json!({
                "type": "pane_closed",
                "pane_id": "w1:p1",
                "workspace_id": "w1"
            }),
        ))
        .expect("last pane close event");
    assert!(!pane_closed.publish);
    assert!(!replica.ready_to_publish());

    let workspace_focused = replica
        .apply(event(
            42,
            "workspace_focused",
            json!({
                "type": "workspace_focused",
                "workspace_id": "w1"
            }),
        ))
        .expect("workspace focus event");
    assert!(!workspace_focused.publish);

    let tab_focused = replica
        .apply(event(
            43,
            "tab_focused",
            json!({
                "type": "tab_focused",
                "tab_id": "w1:t2",
                "workspace_id": "w1"
            }),
        ))
        .expect("fallback tab focus event");
    assert!(tab_focused.publish);
    assert!(replica.ready_to_publish());
    assert_eq!(replica.state.workspaces[0].active_tab_id, "w1:t2");
    assert!(
        replica
            .project()
            .tabs
            .iter()
            .all(|tab| tab.tab_id != "w1:t1")
    );
}

/// The remote context browses Herdr's tabs and has no file tabs to mix in,
/// so its strip is the Herdr tab list in Herdr's order and nothing else.
#[test]
fn remote_projection_tab_list_and_order_are_herdr_only() {
    let replica = SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");
    let (projected, _) = replica.project_remote("mini").expect("remote projection");
    let checkout = projected
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .next()
        .expect("the remote checkout");

    assert_eq!(
        checkout
            .tabs
            .iter()
            .map(|tab| tab.id.clone().expect("a remote tab id"))
            .collect::<Vec<_>>(),
        vec![
            remote_tab_id("mini", "w1:t1"),
            remote_tab_id("mini", "w1:t2")
        ]
    );
    assert_eq!(
        checkout
            .strip
            .iter()
            .map(|entry| (entry.kind, entry.source_id.clone()))
            .collect::<Vec<_>>(),
        vec![
            (
                crate::model::StripTabKind::Herdr,
                remote_tab_id("mini", "w1:t1")
            ),
            (
                crate::model::StripTabKind::Herdr,
                remote_tab_id("mini", "w1:t2")
            )
        ]
    );
    assert_eq!(checkout.active_tab_id, Some(remote_tab_id("mini", "w1:t1")));
}

#[test]
fn tab_order_from_a_move_event_reaches_the_projection() {
    let mut replica = SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");
    assert_eq!(
        replica
            .project()
            .tabs
            .iter()
            .map(|tab| tab.tab_id.clone())
            .collect::<Vec<_>>(),
        vec!["w1:t1".to_owned(), "w1:t2".to_owned()]
    );

    // Herdr moved the second tab in front of the first and reported the
    // resulting order. The projection the navigator reads must carry that
    // order, not the order the tabs were created in.
    let moved = replica
            .apply(event(
                41,
                "tab_moved",
                json!({
                    "type": "tab_moved",
                    "workspace_id": "w1",
                    "tab_id": "w1:t2",
                    "insert_index": 0,
                    "tabs": [
                        {"workspace_id": "w1", "tab_id": "w1:t2", "agent_status": "idle", "focused": false, "number": 2, "pane_count": 1, "label": "2"},
                        {"workspace_id": "w1", "tab_id": "w1:t1", "agent_status": "idle", "focused": false, "number": 1, "pane_count": 1, "label": "1"}
                    ]
                }),
            ))
            .expect("tab move event");
    assert!(moved.publish);
    assert_eq!(
        replica
            .project()
            .tabs
            .iter()
            .map(|tab| tab.tab_id.clone())
            .collect::<Vec<_>>(),
        vec!["w1:t2".to_owned(), "w1:t1".to_owned()]
    );
}

#[test]
fn herdr_active_tab_rides_the_projection_with_its_workspace() {
    let replica = SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");
    let projected = replica.project();
    let workspace = projected
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == "w1")
        .expect("the fixture workspace");
    assert_eq!(workspace.active_tab_id.as_deref(), Some("w1:t1"));
}

#[test]
fn active_tab_close_waits_for_the_authoritative_fallback_tab_focus() {
    let mut replica = SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");

    let tab_closed = replica
        .apply(event(
            41,
            "tab_closed",
            json!({
                "type": "tab_closed",
                "tab_id": "w1:t1",
                "workspace_id": "w1"
            }),
        ))
        .expect("active tab close event");
    assert!(!tab_closed.publish);
    assert!(!replica.ready_to_publish());

    let tab_focused = replica
        .apply(event(
            42,
            "tab_focused",
            json!({
                "type": "tab_focused",
                "tab_id": "w1:t2",
                "workspace_id": "w1"
            }),
        ))
        .expect("fallback tab focus event");
    assert!(tab_focused.publish);
    assert!(replica.ready_to_publish());
    assert_eq!(replica.state.workspaces[0].active_tab_id, "w1:t2");
}

#[test]
fn last_tab_close_waits_for_the_workspace_close_cascade() {
    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    let tab_closed = replica
        .apply(event(
            41,
            "tab_closed",
            json!({
                "type": "tab_closed",
                "tab_id": "w1:t1",
                "workspace_id": "w1"
            }),
        ))
        .expect("tab close event");
    assert!(!tab_closed.publish);

    let workspace_closed = replica
        .apply(event(
            42,
            "workspace_closed",
            json!({
                "type": "workspace_closed",
                "workspace_id": "w1"
            }),
        ))
        .expect("workspace close event");
    assert!(workspace_closed.publish);
    assert!(replica.ready_to_publish());
    assert!(replica.project().workspaces.is_empty());
}

#[test]
fn exact_duplicate_replay_is_idempotent_but_reused_sequence_is_rejected() {
    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    let focused = event(
        41,
        "pane_focused",
        json!({
            "type": "pane_focused",
            "workspace_id": "w1",
            "pane_id": "w1:p1"
        }),
    );
    assert!(replica.apply(focused.clone()).expect("first event").publish);
    assert!(!replica.apply(focused).expect("duplicate").publish);

    let changed = event(
        41,
        "workspace_focused",
        json!({
            "type": "workspace_focused",
            "workspace_id": "w1"
        }),
    );
    let error = replica
        .apply(changed)
        .expect_err("sequence reuse must fail");
    assert_eq!(error.state(), "malformed");
}

#[test]
fn event_gap_is_an_explicit_stream_result() {
    let line = json!({
        "id": "herdr-core:events.subscribe",
        "error": {
            "code": "event_gap",
            "message": "fetch session.snapshot and resubscribe"
        }
    })
    .to_string();
    match parse_subscription_line(&line).expect("typed error") {
        SubscriptionLine::Error { code, message } => {
            assert_eq!(code, "event_gap");
            assert!(message.contains("session.snapshot"));
        }
        SubscriptionLine::Event(_) => panic!("expected subscription error"),
    }
}

#[test]
fn subscription_failure_is_stale_only_when_a_projection_already_exists() {
    let initial =
        connect_failure_from_api(ApiError::Transport("socket closed".to_owned()), false, 40);
    assert_eq!(initial.error.state(), "unreachable");

    let reconnect =
        connect_failure_from_api(ApiError::Transport("socket closed".to_owned()), true, 40);
    assert_eq!(reconnect.error.state(), "stale");
}

#[test]
fn filtered_global_sequence_gaps_are_accepted() {
    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    let outcome = replica
        .apply(event(
            57,
            "workspace_focused",
            json!({
                "type": "workspace_focused",
                "workspace_id": "w1"
            }),
        ))
        .expect("sequence jump is legal");
    assert!(outcome.publish);
    assert_eq!(replica.cursor, 57);
}

fn agent(pane_id: &str, elapsed: &str) -> ProjectedAgent {
    wire::agents_response(json!({"type": "agent_list", "agents": [{
        "pane_id": pane_id, "workspace_id": "w1", "tab_id": "w1:t1",
        "agent": "claude", "agent_status": "idle", "focused": false,
        "terminal_id": "fixture-terminal", "revision": 0,
        "tokens": {"status_idle": "\u{25cb}", "elapsed": elapsed, "activity": "1"}
    }]}))
    .expect("an agent list")
    .remove(0)
}

fn fresh_catalog() -> CatalogCache {
    CatalogCache {
        registrations: Vec::new(),
        spaces: Vec::new(),
        worktrees: crate::model::WorktreeCatalogSnapshot::default(),
        workspaces: Vec::new(),
        roots: workspace::RootIndex::new(),
        built_at: Instant::now(),
    }
}

/// R6, AC12, SC5. `agent.list` is polled once a second whether or not it
/// moved, and every tick used to rebuild the whole projection and re-enter
/// the runtime lock twice for a sidebar that had not changed.
#[test]
fn snapshot_delivery_skips_the_projection_when_the_agent_list_is_unchanged() {
    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    let held = vec![agent("w1:p1", "4m")];
    replica.replace_agents(held.clone());
    let catalog = fresh_catalog();

    assert!(
        !agent_tick_needs_publish(&replica, &held, Some(&catalog)),
        "an identical list projects the same sidebar, so the tick publishes nothing"
    );
    assert!(
        agent_tick_needs_publish(&replica, &[agent("w1:p1", "5m")], Some(&catalog)),
        "a token the sidebar renders moved, so the projection has to be rebuilt"
    );
    assert!(
        agent_tick_needs_publish(&replica, &[], Some(&catalog)),
        "an agent that went away has to leave the sidebar"
    );

    // The catalog is rebuilt inside the publish on its own refresh window,
    // and on an idle session this tick is the only thing that calls it, so
    // the skip must not be what freezes every branch mark in the navigator.
    assert!(
        agent_tick_needs_publish(&replica, &held, None),
        "a catalog that was never built has to be built"
    );
    let stale = CatalogCache {
        built_at: Instant::now() - CATALOG_REFRESH_INTERVAL,
        ..fresh_catalog()
    };
    assert!(
        agent_tick_needs_publish(&replica, &held, Some(&stale)),
        "a catalog past its refresh window has to be rebuilt"
    );
}

/// Herdr's focused workspace is carried from the snapshot and moved by
/// every focus event, because a checkout can hold tabs from several
/// workspaces and only the focused one's active tab is a focus.
#[test]
fn focus_events_move_the_projected_focused_workspace() {
    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    assert_eq!(
        replica.project().focused_workspace_id,
        None,
        "the fixture snapshot names no focused workspace"
    );

    replica
        .apply(event(
            41,
            "workspace_focused",
            json!({"type": "workspace_focused", "workspace_id": "w1"}),
        ))
        .expect("workspace focus applies");
    assert_eq!(
        replica.project().focused_workspace_id.as_deref(),
        Some("w1")
    );

    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    replica
        .apply(event(
            41,
            "tab_focused",
            json!({"type": "tab_focused", "workspace_id": "w1", "tab_id": "w1:t1"}),
        ))
        .expect("tab focus applies");
    assert_eq!(
        replica.project().focused_workspace_id.as_deref(),
        Some("w1"),
        "a tab focus names the workspace Herdr moved into"
    );

    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    replica
        .apply(event(
            41,
            "pane_focused",
            json!({"type": "pane_focused", "workspace_id": "w1", "pane_id": "w1:p1"}),
        ))
        .expect("pane focus applies");
    assert_eq!(
        replica.project().focused_workspace_id.as_deref(),
        Some("w1"),
        "a pane focus names the workspace the pane is in"
    );
}

#[test]
fn host_and_protocol_mismatch_leave_the_last_projection_unchanged() {
    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    let before = replica.project();
    let wrong_host = json!({
        "protocol": HERDR_PROTOCOL_REVISION,
        "host": {"host_id": "another-host", "session_id": "fixture"},
        "sequence": 41,
        "event": "workspace_focused",
        "data": {"type": "workspace_focused", "workspace_id": "w1"}
    });
    let event = match parse_subscription_line(&wrong_host.to_string()).expect("event parses") {
        SubscriptionLine::Event(event) => event,
        SubscriptionLine::Error { .. } => unreachable!(),
    };
    assert_eq!(
        replica.apply(event).expect_err("host mismatch").state(),
        "stale"
    );
    assert_eq!(replica.project().focused_pane_id, before.focused_pane_id);

    let wrong_protocol = json!({
        "protocol": HERDR_PROTOCOL_REVISION + 1,
        "host": {"host_id": "fixture-host", "session_id": "fixture"},
        "sequence": 41,
        "event": "workspace_focused",
        "data": {"type": "workspace_focused", "workspace_id": "w1"}
    });
    let event = match parse_subscription_line(&wrong_protocol.to_string()).expect("event parses") {
        SubscriptionLine::Event(event) => event,
        SubscriptionLine::Error { .. } => unreachable!(),
    };
    let mismatch = replica.apply(event).expect_err("protocol mismatch");
    assert_eq!(mismatch.state(), "protocol_mismatch");
    let message = mismatch.message();
    assert!(
        message.contains(&format!("protocol {}", HERDR_PROTOCOL_REVISION + 1))
            && message.contains(&format!("supports protocol {HERDR_PROTOCOL_REVISION}"))
            && message.contains("Update Hide")
            && message.contains("No workspace or agent was created")
            && !message.contains("server stop"),
        "the mismatch names both revisions and the remedy: {message}"
    );
    assert_eq!(replica.project().focused_pane_id, before.focused_pane_id);
}

#[test]
fn coordinator_resumes_from_the_last_event_without_fetching_another_snapshot() {
    let root = Path::new("/tmp").join(format!(
        "herdr-core-session-resume-contract-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("create socket directory");
    let socket_path = root.join("herdr.sock");
    let state_path = root.join("state.json");
    let listener = UnixListener::bind(&socket_path).expect("bind fake Herdr socket");
    let resumed = Arc::new(AtomicBool::new(false));
    let resumed_from_server = Arc::clone(&resumed);
    let server = thread::spawn(move || {
        let (mut snapshot_stream, snapshot_request) =
            accept_request_for(&listener, "session.snapshot");
        assert_eq!(snapshot_request["method"], "session.snapshot");
        write_result(
            &mut snapshot_stream,
            &snapshot_request,
            json!({"type": "session_snapshot", "snapshot": snapshot()}),
        );

        let (mut first_subscription, first_subscribe_request) =
            accept_request_for(&listener, "events.subscribe");
        assert_eq!(first_subscribe_request["method"], "events.subscribe");
        assert_eq!(first_subscribe_request["params"]["after_sequence"], 40);
        write_result(
            &mut first_subscription,
            &first_subscribe_request,
            json!({
                "type": "subscription_started",
                "host": {"host_id": "fixture-host", "session_id": "fixture"},
                "sequence": 40,
                "oldest_available_sequence": 1
            }),
        );
        writeln!(
            first_subscription,
            "{}",
            json!({
                "protocol": HERDR_PROTOCOL_REVISION,
                "host": {"host_id": "fixture-host", "session_id": "fixture"},
                "sequence": 41,
                "event": "workspace_focused",
                "data": {"type": "workspace_focused", "workspace_id": "w1"}
            })
        )
        .expect("write replayable event");
        drop(first_subscription);

        let (mut resumed_subscription, resumed_request) =
            accept_request_for(&listener, "events.subscribe");
        assert_eq!(
            resumed_request["method"], "events.subscribe",
            "a clean disconnect must resume the cursor instead of fetching a snapshot"
        );
        assert_eq!(resumed_request["params"]["after_sequence"], 41);
        write_result(
            &mut resumed_subscription,
            &resumed_request,
            json!({
                "type": "subscription_started",
                "host": {"host_id": "fixture-host", "session_id": "fixture"},
                "sequence": 41,
                "oldest_available_sequence": 1
            }),
        );
        resumed_from_server.store(true, Ordering::Release);
        let mut byte = [0_u8; 1];
        assert_eq!(
            resumed_subscription
                .read(&mut byte)
                .expect("wait for shutdown"),
            0
        );
    });

    let runtime = runtime_for_fixture(&socket_path, &state_path);
    let context = context_for_fixture(&runtime, &socket_path);
    let handle = spawn(context, None).expect("start session sync");
    wait_until(Instant::now() + Duration::from_secs(3), || {
        resumed.load(Ordering::Acquire)
            && runtime
                .lock()
                .expect("runtime lock")
                .snapshot()
                .status
                .herdr
                .state
                == "connected"
    });

    drop(handle);
    server.join().expect("fake server joins");
    remove_fixture(&root, &socket_path, &state_path);
}

#[test]
fn coordinator_recovers_event_gap_with_one_fresh_snapshot_and_stops_its_reader() {
    let root = Path::new("/tmp").join(format!(
        "herdr-core-session-gap-contract-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("create socket directory");
    let socket_path = root.join("herdr.sock");
    let state_path = root.join("state.json");
    let listener = UnixListener::bind(&socket_path).expect("bind fake Herdr socket");
    let server = thread::spawn(move || {
        let (mut first_snapshot_stream, first_snapshot_request) =
            accept_request_for(&listener, "session.snapshot");
        assert_eq!(first_snapshot_request["method"], "session.snapshot");
        write_result(
            &mut first_snapshot_stream,
            &first_snapshot_request,
            json!({"type": "session_snapshot", "snapshot": snapshot()}),
        );

        let (mut first_subscription, first_subscribe_request) =
            accept_request_for(&listener, "events.subscribe");
        assert_eq!(first_subscribe_request["method"], "events.subscribe");
        assert_eq!(first_subscribe_request["params"]["after_sequence"], 40);
        write_result(
            &mut first_subscription,
            &first_subscribe_request,
            json!({
                "type": "subscription_started",
                "host": {"host_id": "fixture-host", "session_id": "fixture"},
                "sequence": 40,
                "oldest_available_sequence": 1
            }),
        );
        writeln!(
            first_subscription,
            "{}",
            json!({
                "id": "herdr-core:events.subscribe",
                "error": {
                    "code": "event_gap",
                    "message": "fetch session.snapshot and resubscribe"
                }
            })
        )
        .expect("write event gap");
        drop(first_subscription);

        let (mut second_snapshot_stream, second_snapshot_request) =
            accept_request_for(&listener, "session.snapshot");
        assert_eq!(second_snapshot_request["method"], "session.snapshot");
        let mut recovered = snapshot();
        recovered["event_sequence"] = json!(50);
        // A project is named after its directory, not Herdr's workspace
        // label, so the recovery marker is a pane in a new directory.
        recovered["panes"][0]["cwd"] = json!("/tmp/fixture-recovered");
        write_result(
            &mut second_snapshot_stream,
            &second_snapshot_request,
            json!({"type": "session_snapshot", "snapshot": recovered}),
        );

        let (mut final_subscription, final_subscribe_request) =
            accept_request_for(&listener, "events.subscribe");
        assert_eq!(final_subscribe_request["method"], "events.subscribe");
        assert_eq!(final_subscribe_request["params"]["after_sequence"], 50);
        write_result(
            &mut final_subscription,
            &final_subscribe_request,
            json!({
                "type": "subscription_started",
                "host": {"host_id": "fixture-host", "session_id": "fixture"},
                "sequence": 50,
                "oldest_available_sequence": 1
            }),
        );
        let mut byte = [0_u8; 1];
        assert_eq!(
            final_subscription
                .read(&mut byte)
                .expect("wait for shutdown"),
            0
        );
    });

    let runtime = runtime_for_fixture(&socket_path, &state_path);
    let context = context_for_fixture(&runtime, &socket_path);
    let handle = spawn(context, None).expect("start session sync");
    wait_until(Instant::now() + Duration::from_secs(3), || {
        let snapshot = runtime.lock().expect("runtime lock").snapshot().clone();
        snapshot.status.herdr.state == "connected"
            && snapshot
                .navigator
                .workspaces
                .iter()
                .any(|workspace| workspace.label == "fixture-recovered")
    });

    drop(handle);
    server.join().expect("fake server joins");
    remove_fixture(&root, &socket_path, &state_path);
}
