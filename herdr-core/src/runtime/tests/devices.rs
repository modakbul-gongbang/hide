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
        Some("Remote features are disabled because the SSH agent socket is unavailable")
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
    let mut runtime = runtime_with_remote_enabled();
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

fn runtime_with_remote_enabled() -> Runtime {
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
    };
    Runtime::new(
        options,
        environment::EnvironmentReport {
            statuses: Vec::new(),
            remote_enabled: true,
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
