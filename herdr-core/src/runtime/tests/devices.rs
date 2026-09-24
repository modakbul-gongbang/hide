use super::*;

fn register_device(runtime: &mut Runtime, id: &str, alias: &str) -> bool {
    let event = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "register_device",
        "payload": { "id": id, "label": id, "ssh_alias": alias }
    }))
    .expect("register device event");
    runtime.dispatch_json(&event)
}

fn dispatch_device(runtime: &mut Runtime, kind: &str, id: &str) -> bool {
    let event = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": kind,
        "payload": { "device_id": id }
    }))
    .expect("device event");
    runtime.dispatch_json(&event)
}

fn device<'a>(runtime: &'a Runtime, id: &str) -> &'a DeviceSnapshot {
    runtime
        .snapshot()
        .navigator
        .devices
        .iter()
        .find(|device| device.id == id)
        .expect("device row")
}

/// A registered device has a remote status entry from the moment it is
/// registered, and its row says why it is not ready. Nothing is hardcoded
/// about which host it is: the row is the registration, the reason is the
/// connection's.
#[test]
fn registering_a_device_opens_its_remote_status_and_removing_it_closes_it() {
    let mut runtime = runtime();
    assert!(runtime.snapshot().status.remote.is_empty());

    assert!(register_device(&mut runtime, "studio", "studio-host"));

    let status = &runtime.snapshot().status.remote;
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].target_id, "studio");
    assert_eq!(status[0].state, "unreachable");
    let row = device(&runtime, "studio");
    assert_eq!(row.kind, "remote");
    assert_eq!(row.state, "unavailable");
    assert_eq!(row.ssh_alias.as_deref(), Some("studio-host"));
    assert_eq!(
        row.message.as_deref(),
        Some("HOME is unavailable, so the SSH config cannot be resolved")
    );

    // A catalog rebuild recreates the rows; the state has to come back with them.
    runtime.rebuild_catalog();
    assert_eq!(device(&runtime, "studio").state, "unavailable");
    assert!(device(&runtime, "studio").message.is_some());

    assert!(dispatch_device(&mut runtime, "remove_device", "studio"));
    assert!(runtime.snapshot().status.remote.is_empty());
    assert!(
        !runtime
            .snapshot()
            .navigator
            .devices
            .iter()
            .any(|device| device.id == "studio")
    );
}

/// An alias the SSH config does not know is the first thing that can go
/// wrong, and the row has to say so rather than spin on "connecting".
#[test]
fn an_unknown_ssh_alias_is_reported_on_the_device_row() {
    let mut runtime = runtime_with_home();
    assert!(register_device(
        &mut runtime,
        "nowhere",
        "hide-test-alias-that-does-not-exist"
    ));
    let row = device(&runtime, "nowhere");
    assert_eq!(row.state, "unavailable");
    let message = row.message.clone().expect("failure reason");
    assert!(
        message.starts_with("SSH alias hide-test-alias-that-does-not-exist could not be read"),
        "the row names the alias and the fix: {message}"
    );
    assert_eq!(runtime.snapshot().status.remote[0].state, "unreachable");
    assert_eq!(diagnostic_count(&runtime, "device.registered"), 1);
}

/// A test asked of a device that never connected has nothing to probe, and
/// the answer names the way out instead of leaving the button dead.
#[test]
fn testing_an_unconnected_device_reports_the_way_out() {
    let mut runtime = runtime();
    assert!(register_device(&mut runtime, "studio", "studio-host"));
    assert!(dispatch_device(&mut runtime, "test_device", "studio"));
    let error = runtime
        .snapshot()
        .status
        .last_error
        .clone()
        .expect("error reported");
    assert_eq!(error.kind, "device.not_connected");
    assert!(device(&runtime, "studio").test.is_none());

    assert!(dispatch_device(&mut runtime, "test_device", "local"));
    let error = runtime
        .snapshot()
        .status
        .last_error
        .clone()
        .expect("error reported");
    assert_eq!(error.kind, "device.unknown");
}

fn runtime_with_home() -> Runtime {
    let state_id = NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed);
    let options = CoreOptions {
        schema_version: SCHEMA_VERSION,
        herdr_socket_path: None,
        herdr_bin_path: None,
        app_state_path: std::env::temp_dir()
            .join(format!(
                "herdr-core-devices-runtime-{}-{}.json",
                std::process::id(),
                state_id
            ))
            .to_string_lossy()
            .into_owned(),
        host_helper_dir: None,
        host_helper_root: None,
    };
    Runtime::new(
        options,
        environment::EnvironmentReport {
            statuses: Vec::new(),
            chromux_enabled: false,
            herdr_socket_path_override: None,
            home_path: Some(std::env::temp_dir().join(format!(
                "herdr-core-devices-home-{}-{}",
                std::process::id(),
                state_id
            ))),
            codex_home: None,
        },
    )
}

/// Retry is a new attempt, not a repaint of the last answer: fixing the SSH
/// config and retrying moves the row past the alias failure it showed.
#[test]
fn retrying_a_device_makes_a_new_connection_attempt() {
    let mut runtime = runtime_with_home();
    assert!(register_device(&mut runtime, "studio", "studio-host"));
    let first = device(&runtime, "studio").message.clone().expect("reason");
    assert!(
        first.starts_with("SSH alias studio-host could not be read"),
        "{first}"
    );

    let home = runtime.home_path.clone().expect("fixture home");
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::write(
        home.join(".ssh/config"),
        "Host studio-host\n  HostName 127.0.0.1\n  User fixture\n",
    )
    .unwrap();
    let retry = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "retry_connect",
        "payload": { "target_id": "studio" }
    }))
    .unwrap();
    assert!(runtime.dispatch_json(&retry));

    let second = device(&runtime, "studio").message.clone().expect("reason");
    assert_ne!(second, first, "the row reports the new attempt");
    assert_eq!(diagnostic_count(&runtime, "device.retry"), 1);
    assert_eq!(runtime.snapshot().status.remote.len(), 1);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn retrying_an_unregistered_device_is_refused() {
    let mut runtime = runtime();
    let retry = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "retry_connect",
        "payload": { "target_id": "nowhere" }
    }))
    .unwrap();
    assert!(runtime.dispatch_json(&retry));
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("remote.unknown_target")
    );
}

/// A split names no pane, so with an SSH device selected it would act on this
/// machine's current pane. It is refused instead of falling back to local.
#[test]
fn a_split_with_a_remote_device_selected_is_refused_not_run_locally() {
    let mut runtime = runtime();
    assert!(register_device(&mut runtime, "studio", "studio-host"));
    assert!(dispatch_device(&mut runtime, "focus_device", "studio"));
    runtime.snapshot.terminal.pane_id = Some("w1:p1".to_owned());
    let split = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "create_pane",
        "payload": { "tab_id": "w1:t1", "cwd": "/tmp", "direction": "right" }
    }))
    .unwrap();
    assert!(runtime.dispatch_json(&split));
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("pane.device_mismatch")
    );
    assert!(runtime.snapshot().status.async_operations.is_empty());
}

/// A connected device keeps its connection: a retry from a stale screen is
/// refused rather than tearing down live remote panes.
#[test]
fn retrying_a_connected_device_is_refused() {
    let mut runtime = runtime();
    assert!(register_device(&mut runtime, "studio", "studio-host"));
    runtime.snapshot.status.remote[0].state = "connected".to_owned();
    let retry = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "retry_connect",
        "payload": { "target_id": "studio" }
    }))
    .unwrap();
    assert!(runtime.dispatch_json(&retry));
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("remote.retry_connected")
    );
    assert_eq!(diagnostic_count(&runtime, "device.retry"), 0);
}

/// Typing into a remote pane makes it the terminal pane. Selecting this
/// machine again hands the keyboard back to the local selection at once, so a
/// split or a zoom sent before the next local session event names a local
/// pane rather than the remote one.
#[test]
fn returning_to_this_machine_gives_the_keyboard_back_to_the_local_pane() {
    let mut runtime = runtime();
    assert!(register_device(&mut runtime, "studio", "studio-host"));
    runtime.snapshot.pane_layouts = vec![PaneLayoutSnapshot {
        workspace_id: "w1".to_owned(),
        tab_id: "w1:t1".to_owned(),
        focused_pane_id: "w1:p1".to_owned(),
        zoomed: false,
        root: PaneLayoutNodeSnapshot::Pane {
            pane_id: "w1:p1".to_owned(),
        },
    }];
    runtime.snapshot.ui_state.selected_pane_id = Some("w1:p1".to_owned());
    runtime.snapshot.terminal.pane_id = Some("w1:p1".to_owned());
    assert!(dispatch_device(&mut runtime, "focus_device", "studio"));
    let key = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "key",
        "payload": { "pane_id": "remote:studio:pane:w9:p1", "bytes_base64": "YQ==" }
    }))
    .unwrap();
    assert!(runtime.dispatch_json(&key));
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("remote:studio:pane:w9:p1")
    );

    assert!(dispatch_device(&mut runtime, "focus_device", "local"));

    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("w1:p1")
    );
    assert_eq!(runtime.snapshot().focused.pane_id.as_deref(), Some("w1:p1"));
    assert_eq!(
        runtime.snapshot().ui_state.selected_pane_id.as_deref(),
        Some("w1:p1")
    );
}

/// Removing the selected device returns this machine's projects exactly as
/// they were drawn, tabs included, and gives the keyboard back to the local
/// pane; the remote host itself is not touched.
#[test]
fn removing_the_selected_device_keeps_the_local_tabs_and_keyboard() {
    let mut runtime = runtime();
    let path = "/tmp/hide-device-removal";
    let mut local = checkout(
        "workspace:local",
        "checkout:local",
        path,
        Some(pane("w1:p1", path)),
    );
    local.tabs[0].id = Some("w1:t1".to_owned());
    local.active_tab_id = Some("w1:t1".to_owned());
    runtime.snapshot.navigator.workspaces =
        vec![workspace("workspace:local", "Local", path, vec![local])];
    runtime.snapshot.navigator.focused_checkout_id = Some("checkout:local".to_owned());
    runtime.snapshot.pane_layouts = vec![PaneLayoutSnapshot {
        workspace_id: "w1".to_owned(),
        tab_id: "w1:t1".to_owned(),
        focused_pane_id: "w1:p1".to_owned(),
        zoomed: false,
        root: PaneLayoutNodeSnapshot::Pane {
            pane_id: "w1:p1".to_owned(),
        },
    }];
    runtime.snapshot.ui_state.selected_pane_id = Some("w1:p1".to_owned());
    assert!(register_device(&mut runtime, "studio", "studio-host"));
    assert!(dispatch_device(&mut runtime, "focus_device", "studio"));
    runtime.snapshot.terminal.pane_id = Some("remote:studio:pane:w9:p1".to_owned());
    // The rows as a session publish draws them, strips included.
    runtime.rebuild_tab_strips();
    let before = runtime.snapshot().navigator.workspaces.clone();
    assert!(!before[0].checkouts[0].strip.is_empty());

    assert!(dispatch_device(&mut runtime, "remove_device", "studio"));

    assert_eq!(runtime.snapshot().navigator.workspaces, before);
    assert_eq!(
        runtime.snapshot().navigator.focused_device_id.as_deref(),
        Some("local")
    );
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("w1:p1")
    );
    assert!(
        runtime
            .snapshot()
            .navigator
            .devices
            .iter()
            .all(|device| device.id != "studio")
    );
}
