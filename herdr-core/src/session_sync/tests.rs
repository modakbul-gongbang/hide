use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::*;
use crate::model::{CoreOptions, SCHEMA_VERSION};
use hide_herdr_client::HERDR_PROTOCOL_REVISION;

fn snapshot() -> Value {
    json!({
        "version": "0.8.2",
        "protocol": HERDR_PROTOCOL_REVISION,
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
            "pane_id": "w1:p1", "terminal_id": "fixture-terminal", "focused": false, "revision": 0, "agent_status": "idle",
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
        "agents": []
    })
}

#[test]
fn every_session_sync_snapshot_fixture_obeys_the_generated_contract() {
    for (fixture, panes) in [(snapshot(), 1), (two_tab_snapshot(), 2)] {
        let state = wire::snapshot(fixture).expect("generated snapshot contract");
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
            "pane_id": "w1:p2", "terminal_id": "fixture-terminal", "focused": false, "revision": 0, "agent_status": "idle",
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

fn event(kind: &str, data: Value) -> ReplicaEvent {
    let raw = json!({"event": kind, "data": data});
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
        "worktree_opened",
        json!({
            "type": "worktree_opened",
            "workspace": workspace,
            "worktree": {"path": "/tmp/fixture", "is_bare": false, "is_detached": false, "is_prunable": false, "is_linked_worktree": true, "label": "fixture"},
            "already_open": true
        }),
    );
    let outcome = replica
        .apply(opened, ApplyMode::Strict)
        .expect("worktree event");
    assert!(outcome.refresh_worktrees);
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
        .apply(
            event(
                "pane_created",
                json!({
                    "type": "pane_created",
                    "pane": {
                        "workspace_id": "w1",
                        "tab_id": "w1:t1",
                        "pane_id": "w1:p2", "terminal_id": "fixture-terminal", "focused": false, "revision": 0, "agent_status": "idle",
                        "cwd": "/tmp/fixture"
                    }
                }),
            ),
            ApplyMode::Strict,
        )
        .expect("pane event");
    assert!(!created.publish);

    let updated = replica
            .apply(event("layout_updated",
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
            ), ApplyMode::Strict)
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
        .apply(
            event(
                "pane_updated",
                json!({"type": "pane_updated", "pane": pane}),
            ),
            ApplyMode::Strict,
        )
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
        .apply(
            event(
                "pane_updated",
                json!({"type": "pane_updated", "pane": pane}),
            ),
            ApplyMode::Strict,
        )
        .expect("host release");
    assert!(
        crate::pane_content::PaneContent::from_tokens(&replica.project().panes[0].tokens, false)
            .is_terminal()
    );
}

#[test]
fn agent_projection_keeps_the_herdr_name_as_its_control_identifier() {
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
    value["panes"]
        .as_array_mut()
        .expect("panes array")
        .push(json!({
            "workspace_id": "w2",
            "tab_id": "w2:t1",
            "pane_id": "w2:p1", "terminal_id": "fixture-terminal", "focused": false, "revision": 0, "agent_status": "idle",
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
#[ignore = "requires HERDR_TEST_SSH_ALIAS"]
fn official_remote_session_coordinator_probe() {
    let alias_name = std::env::var("HERDR_TEST_SSH_ALIAS")
        .expect("HERDR_TEST_SSH_ALIAS names a configured SSH host");
    let home = std::env::var_os("HOME").expect("HOME is configured");
    let state_path = PathBuf::from("/tmp/herdr-core-remote-coordinator-probe-state.json");
    let _ = std::fs::remove_file(&state_path);
    let runtime = Arc::new(Mutex::new(Runtime::new(
        CoreOptions {
            schema_version: SCHEMA_VERSION,
            herdr_socket_path: None,
            herdr_bin_path: None,
            app_state_path: state_path.to_string_lossy().into_owned(),
        },
        crate::environment::EnvironmentReport {
            statuses: Vec::new(),
            home_path: Some(PathBuf::from(home)),
            remote_enabled: true,
            chromux_enabled: false,
            herdr_socket_path_override: None,
            codex_home: None,
        },
    )));
    // The same path the shell takes: register the device, and the runtime
    // resolves the alias, asks the host for its socket and starts the
    // coordinator itself.
    {
        let mut guard = runtime.lock().expect("runtime lock");
        guard.install_worker_context(Arc::downgrade(&runtime), crate::ffi::ChangeNotifier::noop());
        let register_device = serde_json::to_vec(&json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "register_device",
            "payload": { "id": "probe", "label": "Probe", "ssh_alias": alias_name }
        }))
        .expect("register device event");
        assert!(guard.dispatch_json(&register_device));
    }

    wait_until(Instant::now() + Duration::from_secs(15), || {
        runtime.lock().ok().is_some_and(|runtime| {
            let status = &runtime.snapshot().status.remote[0];
            status.state == "connected" && status.session.is_some()
        })
    });
    let mut guard = runtime.lock().expect("runtime lock");
    let status = &guard.snapshot().status.remote[0];
    assert_eq!(status.state, "connected", "{:?}", status.message);
    let session = status.session.as_ref().expect("remote session projected");
    assert!(!session.agents.is_empty());
    assert_eq!(
        guard.snapshot().navigator.devices[1].agent_count as usize,
        session.agents.len()
    );
    assert_eq!(guard.snapshot().navigator.devices[1].state, "ready");
    let handles = guard.take_remote_syncs();
    drop(guard);
    drop(handles);
}

#[test]
fn workspace_close_cascade_clears_pending_layout_and_nested_state() {
    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    let pane_closed = replica
        .apply(
            event(
                "pane_closed",
                json!({
                    "type": "pane_closed",
                    "pane_id": "w1:p1",
                    "workspace_id": "w1"
                }),
            ),
            ApplyMode::Strict,
        )
        .expect("pane close event");
    assert!(!pane_closed.publish);

    let workspace_closed = replica
        .apply(
            event(
                "workspace_closed",
                json!({
                    "type": "workspace_closed",
                    "workspace_id": "w1"
                }),
            ),
            ApplyMode::Strict,
        )
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
        .apply(
            event(
                "pane_closed",
                json!({
                    "type": "pane_closed",
                    "pane_id": "w1:p2",
                    "workspace_id": "w1"
                }),
            ),
            ApplyMode::Strict,
        )
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
        .apply(
            event(
                "pane_closed",
                json!({
                    "type": "pane_closed",
                    "pane_id": "w1:p1",
                    "workspace_id": "w1"
                }),
            ),
            ApplyMode::Strict,
        )
        .expect("last pane close event");
    assert!(!pane_closed.publish);
    assert!(!replica.ready_to_publish());

    let workspace_focused = replica
        .apply(
            event(
                "workspace_focused",
                json!({
                    "type": "workspace_focused",
                    "workspace_id": "w1"
                }),
            ),
            ApplyMode::Strict,
        )
        .expect("workspace focus event");
    assert!(!workspace_focused.publish);

    let tab_focused = replica
        .apply(
            event(
                "tab_focused",
                json!({
                    "type": "tab_focused",
                    "tab_id": "w1:t2",
                    "workspace_id": "w1"
                }),
            ),
            ApplyMode::Strict,
        )
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
            .apply(event("tab_moved",
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
            ), ApplyMode::Strict)
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
        .apply(
            event(
                "tab_closed",
                json!({
                    "type": "tab_closed",
                    "tab_id": "w1:t1",
                    "workspace_id": "w1"
                }),
            ),
            ApplyMode::Strict,
        )
        .expect("active tab close event");
    assert!(!tab_closed.publish);
    assert!(!replica.ready_to_publish());

    let tab_focused = replica
        .apply(
            event(
                "tab_focused",
                json!({
                    "type": "tab_focused",
                    "tab_id": "w1:t2",
                    "workspace_id": "w1"
                }),
            ),
            ApplyMode::Strict,
        )
        .expect("fallback tab focus event");
    assert!(tab_focused.publish);
    assert!(replica.ready_to_publish());
    assert_eq!(replica.state.workspaces[0].active_tab_id, "w1:t2");
}

/// Herdr moves a non-focused workspace's active tab without an event when a
/// close removes it, so the replica settles from a `workspace.get` answer.
/// The projection then drops the closed tab and names the replacement.
#[test]
fn active_tab_close_settles_from_a_workspace_read_when_no_focus_follows() {
    let mut replica = SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");
    replica
        .apply(
            event(
                "tab_closed",
                json!({"type": "tab_closed", "tab_id": "w1:t1", "workspace_id": "w1"}),
            ),
            ApplyMode::Strict,
        )
        .expect("active tab close event");
    assert_eq!(
        replica.workspaces_awaiting_active_tab(),
        vec!["w1".to_owned()]
    );

    // A read that ran ahead of the stream names a tab it has not delivered.
    assert!(!replica.settle_active_tab("w1", "w1:t9"));
    assert!(!replica.ready_to_publish());

    assert!(replica.settle_active_tab("w1", "w1:t2"));
    assert!(replica.ready_to_publish());
    assert!(replica.workspaces_awaiting_active_tab().is_empty());
    assert!(replica.refresh_published_state().expect("valid projection"));
    let projected = replica.project();
    assert!(projected.tabs.iter().all(|tab| tab.tab_id != "w1:t1"));
    assert_eq!(
        projected.workspaces[0].active_tab_id.as_deref(),
        Some("w1:t2")
    );
}

/// The focused workspace still gets its replacement from `tab_focused`; a
/// read that lands after it changes nothing.
#[test]
fn a_workspace_read_after_the_focus_event_changes_nothing() {
    let mut replica = SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");
    replica
        .apply(
            event(
                "tab_closed",
                json!({"type": "tab_closed", "tab_id": "w1:t1", "workspace_id": "w1"}),
            ),
            ApplyMode::Strict,
        )
        .expect("active tab close event");
    replica
        .apply(
            event(
                "tab_focused",
                json!({"type": "tab_focused", "tab_id": "w1:t2", "workspace_id": "w1"}),
            ),
            ApplyMode::Strict,
        )
        .expect("fallback tab focus event");
    assert!(replica.workspaces_awaiting_active_tab().is_empty());
    assert!(!replica.settle_active_tab("w1", "w1:t2"));
    assert_eq!(replica.state.workspaces[0].active_tab_id, "w1:t2");
}

#[test]
fn last_tab_close_waits_for_the_workspace_close_cascade() {
    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    let tab_closed = replica
        .apply(
            event(
                "tab_closed",
                json!({
                    "type": "tab_closed",
                    "tab_id": "w1:t1",
                    "workspace_id": "w1"
                }),
            ),
            ApplyMode::Strict,
        )
        .expect("tab close event");
    assert!(!tab_closed.publish);

    let workspace_closed = replica
        .apply(
            event(
                "workspace_closed",
                json!({
                    "type": "workspace_closed",
                    "workspace_id": "w1"
                }),
            ),
            ApplyMode::Strict,
        )
        .expect("workspace close event");
    assert!(workspace_closed.publish);
    assert!(replica.ready_to_publish());
    assert!(replica.project().workspaces.is_empty());
}

/// The subscription is opened before the snapshot, so an event emitted just
/// before the snapshot was taken arrives as well and describes a change the
/// snapshot already holds. In the reconcile window the snapshot wins and the
/// event is dropped; the same event afterwards is a real divergence.
#[test]
fn an_event_the_snapshot_already_holds_is_dropped_while_reconciling_and_rejected_afterwards() {
    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    let before = replica.project().panes.len();
    let already_listed = event(
        "pane_created",
        json!({
            "type": "pane_created",
            "workspace_id": "w1",
            "tab_id": "w1:t1",
            "pane": snapshot()["panes"][0]
        }),
    );
    let reconciled = replica
        .apply(already_listed.clone(), ApplyMode::Reconcile)
        .expect("the snapshot already holds this pane");
    assert!(!reconciled.publish);
    assert_eq!(replica.project().panes.len(), before);
    assert_eq!(replica.applied_events, 0);

    let error = replica
        .apply(already_listed, ApplyMode::Strict)
        .expect_err("after the window the replica has diverged");
    assert_eq!(error.state(), "malformed");
    assert_eq!(replica.project().panes.len(), before);

    // An event that does not contradict the snapshot applies in either mode.
    let focused = event(
        "pane_focused",
        json!({"type": "pane_focused", "workspace_id": "w1", "pane_id": "w1:p1"}),
    );
    assert!(
        replica
            .apply(focused, ApplyMode::Reconcile)
            .expect("a focus the snapshot agrees with")
            .publish
    );
    assert_eq!(replica.applied_events, 1);
}

#[test]
fn a_stream_error_is_an_explicit_stream_result() {
    let line = json!({
        "id": "herdr-core:events.subscribe",
        "error": {
            "code": "internal",
            "message": "event stream closed"
        }
    })
    .to_string();
    match parse_subscription_line(&line).expect("typed error") {
        SubscriptionLine::Error { code, message } => {
            assert_eq!(code, "internal");
            assert!(message.contains("closed"));
        }
        SubscriptionLine::Event(_) => panic!("expected subscription error"),
    }
}

#[test]
fn subscription_failure_is_stale_only_when_a_projection_already_exists() {
    let initial = connect_failure_from_api(ApiError::Transport("socket closed".to_owned()), false);
    assert_eq!(initial.state(), "unreachable");

    let reconnect = connect_failure_from_api(ApiError::Transport("socket closed".to_owned()), true);
    assert_eq!(reconnect.state(), "stale");
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
        .apply(
            event(
                "workspace_focused",
                json!({"type": "workspace_focused", "workspace_id": "w1"}),
            ),
            ApplyMode::Strict,
        )
        .expect("workspace focus applies");
    assert_eq!(
        replica.project().focused_workspace_id.as_deref(),
        Some("w1")
    );

    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    replica
        .apply(
            event(
                "tab_focused",
                json!({"type": "tab_focused", "workspace_id": "w1", "tab_id": "w1:t1"}),
            ),
            ApplyMode::Strict,
        )
        .expect("tab focus applies");
    assert_eq!(
        replica.project().focused_workspace_id.as_deref(),
        Some("w1"),
        "a tab focus names the workspace Herdr moved into"
    );

    let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
    replica
        .apply(
            event(
                "pane_focused",
                json!({"type": "pane_focused", "workspace_id": "w1", "pane_id": "w1:p1"}),
            ),
            ApplyMode::Strict,
        )
        .expect("pane focus applies");
    assert_eq!(
        replica.project().focused_workspace_id.as_deref(),
        Some("w1"),
        "a pane focus names the workspace the pane is in"
    );
}

/// Herdr's stream has no position to resume from, so a clean disconnect is
/// followed by a fresh subscription and a fresh snapshot, in that order:
/// listening first is what keeps an event between the two from being lost.
#[test]
fn coordinator_rebuilds_from_a_fresh_snapshot_after_a_clean_disconnect() {
    let root = Path::new("/tmp").join(format!(
        "herdr-core-session-rebuild-contract-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("create socket directory");
    let socket_path = root.join("herdr.sock");
    let state_path = root.join("state.json");
    let listener = UnixListener::bind(&socket_path).expect("bind fake Herdr socket");
    let rebuilt = Arc::new(AtomicBool::new(false));
    let rebuilt_from_server = Arc::clone(&rebuilt);
    let server = thread::spawn(move || {
        let (mut first_subscription, first_subscribe_request) =
            accept_request_for(&listener, "events.subscribe");
        assert_eq!(
            first_subscribe_request["params"].get("after_sequence"),
            None,
            "the stable contract has no resume cursor"
        );
        write_result(
            &mut first_subscription,
            &first_subscribe_request,
            json!({"type": "subscription_started"}),
        );
        let (mut snapshot_stream, snapshot_request) =
            accept_request_for(&listener, "session.snapshot");
        write_result(
            &mut snapshot_stream,
            &snapshot_request,
            json!({"type": "session_snapshot", "snapshot": snapshot()}),
        );
        writeln!(
            first_subscription,
            "{}",
            json!({
                "event": "workspace_focused",
                "data": {"type": "workspace_focused", "workspace_id": "w1"}
            })
        )
        .expect("write event");
        drop(first_subscription);

        let (mut second_subscription, second_subscribe_request) =
            accept_request_for(&listener, "events.subscribe");
        write_result(
            &mut second_subscription,
            &second_subscribe_request,
            json!({"type": "subscription_started"}),
        );
        let (mut second_snapshot_stream, second_snapshot_request) =
            accept_request_for(&listener, "session.snapshot");
        write_result(
            &mut second_snapshot_stream,
            &second_snapshot_request,
            json!({"type": "session_snapshot", "snapshot": snapshot()}),
        );
        rebuilt_from_server.store(true, Ordering::Release);
        let mut byte = [0_u8; 1];
        assert_eq!(
            second_subscription
                .read(&mut byte)
                .expect("wait for shutdown"),
            0
        );
    });

    let runtime = runtime_for_fixture(&socket_path, &state_path);
    let context = context_for_fixture(&runtime, &socket_path);
    let handle = spawn(context, None).expect("start session sync");
    wait_until(Instant::now() + Duration::from_secs(3), || {
        rebuilt.load(Ordering::Acquire)
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

/// End to end through the coordinator: Herdr reports the active tab closed
/// and nothing else, the coordinator asks `workspace.get`, and the runtime's
/// navigator stops listing the closed tab without waiting for a deadline.
#[test]
fn coordinator_reads_the_replacement_active_tab_when_herdr_names_none() {
    let root = Path::new("/tmp").join(format!(
        "herdr-core-session-active-tab-read-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("create socket directory");
    let socket_path = root.join("herdr.sock");
    let state_path = root.join("state.json");
    let listener = UnixListener::bind(&socket_path).expect("bind fake Herdr socket");
    let read_answered = Arc::new(AtomicBool::new(false));
    let read_answered_from_server = Arc::clone(&read_answered);
    let server = thread::spawn(move || {
        let (mut subscription, subscribe_request) =
            accept_request_for(&listener, "events.subscribe");
        write_result(
            &mut subscription,
            &subscribe_request,
            json!({"type": "subscription_started"}),
        );
        let (mut snapshot_stream, snapshot_request) =
            accept_request_for(&listener, "session.snapshot");
        write_result(
            &mut snapshot_stream,
            &snapshot_request,
            json!({"type": "session_snapshot", "snapshot": two_tab_snapshot()}),
        );
        writeln!(
            subscription,
            "{}",
            json!({
                "event": "tab_closed",
                "data": {"type": "tab_closed", "tab_id": "w1:t1", "workspace_id": "w1"}
            })
        )
        .expect("write tab_closed");

        let (mut read_stream, read_request) = accept_request_for(&listener, "workspace.get");
        assert_eq!(read_request["params"]["workspace_id"], "w1");
        write_result(
            &mut read_stream,
            &read_request,
            json!({
                "type": "workspace_info",
                "workspace": {
                    "workspace_id": "w1", "number": 1, "label": "fixture", "focused": false,
                    "pane_count": 1, "tab_count": 1, "active_tab_id": "w1:t2",
                    "agent_status": "idle"
                }
            }),
        );
        read_answered_from_server.store(true, Ordering::Release);
        let mut byte = [0_u8; 1];
        assert_eq!(subscription.read(&mut byte).expect("wait for shutdown"), 0);
    });

    let runtime = runtime_for_fixture(&socket_path, &state_path);
    let context = context_for_fixture(&runtime, &socket_path);
    let handle = spawn(context, None).expect("start session sync");
    let listed_tab_ids = || -> Vec<String> {
        runtime
            .lock()
            .expect("runtime lock")
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .flat_map(|checkout| checkout.tabs.iter())
            .filter_map(|tab| tab.id.clone())
            .collect()
    };
    wait_until(Instant::now() + Duration::from_secs(3), || {
        let tabs = listed_tab_ids();
        read_answered.load(Ordering::Acquire)
            && tabs.iter().any(|id| id == "w1:t2")
            && tabs.iter().all(|id| id != "w1:t1")
    });

    drop(handle);
    server.join().expect("fake server joins");
    remove_fixture(&root, &socket_path, &state_path);
}

#[test]
fn coordinator_recovers_a_stream_error_with_one_fresh_snapshot_and_stops_its_reader() {
    let root = Path::new("/tmp").join(format!(
        "herdr-core-session-stream-error-contract-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("create socket directory");
    let socket_path = root.join("herdr.sock");
    let state_path = root.join("state.json");
    let listener = UnixListener::bind(&socket_path).expect("bind fake Herdr socket");
    let server = thread::spawn(move || {
        let (mut first_subscription, first_subscribe_request) =
            accept_request_for(&listener, "events.subscribe");
        write_result(
            &mut first_subscription,
            &first_subscribe_request,
            json!({"type": "subscription_started"}),
        );
        let (mut first_snapshot_stream, first_snapshot_request) =
            accept_request_for(&listener, "session.snapshot");
        write_result(
            &mut first_snapshot_stream,
            &first_snapshot_request,
            json!({"type": "session_snapshot", "snapshot": snapshot()}),
        );
        writeln!(
            first_subscription,
            "{}",
            json!({
                "id": "herdr-core:events.subscribe",
                "error": {
                    "code": "internal",
                    "message": "event stream closed"
                }
            })
        )
        .expect("write stream error");
        drop(first_subscription);

        let (mut final_subscription, final_subscribe_request) =
            accept_request_for(&listener, "events.subscribe");
        write_result(
            &mut final_subscription,
            &final_subscribe_request,
            json!({"type": "subscription_started"}),
        );
        let (mut second_snapshot_stream, second_snapshot_request) =
            accept_request_for(&listener, "session.snapshot");
        let mut recovered = snapshot();
        // A project is named after its directory, not Herdr's workspace
        // label, so the recovery marker is a pane in a new directory.
        recovered["panes"][0]["cwd"] = json!("/tmp/fixture-recovered");
        write_result(
            &mut second_snapshot_stream,
            &second_snapshot_request,
            json!({"type": "session_snapshot", "snapshot": recovered}),
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
