use std::ffi::c_void;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::slice;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use herdr_core::{
    HerdrCore, herdr_core_create, herdr_core_destroy, herdr_core_dispatch, herdr_core_free_bytes,
    herdr_core_on_change, herdr_core_snapshot,
};
use serde_json::{Value, json};

static HERDR_SOCKET_ENV_LOCK: Mutex<()> = Mutex::new(());

fn options() -> Vec<u8> {
    let missing_state =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/missing-ui-state.json");
    options_with_state(&missing_state)
}

fn options_with_state(state_path: &std::path::Path) -> Vec<u8> {
    // A null socket keeps the core in loopback mode so byte-echo tests stay
    // deterministic; live semantics are covered by the dedicated live test.
    serde_json::to_vec(&json!({
        "schema_version": 1,
        "herdr_socket_path": null,
        "remote_targets": [
            {"id": "mini", "label": "Mac mini", "ssh_alias": "mini"}
        ],
        "app_state_path": state_path
    }))
    .expect("options serialize")
}

fn slow_live_options(state_path: &std::path::Path, herdr_bin_path: &std::path::Path) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema_version": 1,
        "herdr_socket_path": "/tmp/herdr-core-dispatch-latency.sock",
        "herdr_bin_path": herdr_bin_path,
        "remote_targets": [],
        "app_state_path": state_path
    }))
    .expect("slow live options serialize")
}

fn create_with_socket_override_hidden(options: &[u8]) -> *mut HerdrCore {
    let _guard = HERDR_SOCKET_ENV_LOCK
        .lock()
        .expect("socket environment lock");
    let previous = std::env::var_os("HERDR_SOCKET_PATH");
    // SAFETY: the test holds the dedicated environment lock, restores the
    // value before returning, and every potentially affected test option is
    // either loopback/null or points at its own disposable socket.
    unsafe { std::env::remove_var("HERDR_SOCKET_PATH") };
    let core = herdr_core_create(options.as_ptr(), options.len());
    if let Some(previous) = previous {
        // SAFETY: restoration happens under the same dedicated lock.
        unsafe { std::env::set_var("HERDR_SOCKET_PATH", previous) };
    }
    core
}

#[test]
fn live_key_without_attach_surfaces_an_explicit_error() {
    let missing_state =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/missing-ui-state.json");
    let options = serde_json::to_vec(&json!({
        "schema_version": 1,
        "herdr_socket_path": "/tmp/herdr-core-ffi-test-missing.sock",
        "remote_targets": [],
        "app_state_path": missing_state
    }))
    .expect("options serialize");
    let core = create_with_socket_override_hidden(&options);
    assert!(!core.is_null());

    dispatch(
        core,
        json!({"schema_version": 1, "kind": "key", "payload": {"pane_id": "p1", "bytes_base64": "fw=="}}),
    );
    let after = snapshot(core);
    assert_eq!(
        after["status"]["last_error"]["kind"],
        "terminal.not_attached"
    );
    assert_eq!(after["status"]["last_error"]["retryable"], true);
    // The live key path must not echo input back as terminal output.
    assert_eq!(
        after["terminal"]["chunks"].as_array().map(Vec::len),
        Some(0)
    );

    herdr_core_destroy(core);
}

fn create() -> *mut HerdrCore {
    let options = options();
    let core = herdr_core_create(options.as_ptr(), options.len());
    assert!(!core.is_null());
    core
}

fn dispatch(core: *mut HerdrCore, event: Value) {
    let bytes = serde_json::to_vec(&event).expect("event serialize");
    herdr_core_dispatch(core, bytes.as_ptr(), bytes.len());
}

fn snapshot(core: *mut HerdrCore) -> Value {
    let bytes = herdr_core_snapshot(core);
    assert!(!bytes.ptr.is_null());
    let parsed = unsafe { serde_json::from_slice(slice::from_raw_parts(bytes.ptr, bytes.len)) }
        .expect("snapshot JSON");
    herdr_core_free_bytes(bytes);
    parsed
}

fn single_pane_layout(workspace_id: &str, pane_id: &str) -> Value {
    json!({
        "workspace_id": workspace_id,
        "tab_id": format!("{workspace_id}:t1"),
        "zoomed": false,
        "area": {"x": 0, "y": 0, "width": 120, "height": 60},
        "focused_pane_id": pane_id,
        "panes": [{
            "pane_id": pane_id,
            "rect": {"x": 0, "y": 0, "width": 120, "height": 60}
        }],
        "splits": []
    })
}

fn wait_for_snapshot(
    core: *mut HerdrCore,
    timeout: Duration,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + timeout;
    loop {
        let current = snapshot(core);
        if predicate(&current) {
            return current;
        }
        assert!(
            Instant::now() < deadline,
            "snapshot condition timed out: {current}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn snapshot_exposes_the_production_schema_and_status() {
    let core = create();
    let snapshot = snapshot(core);

    let mut keys = snapshot
        .as_object()
        .expect("snapshot object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "connection",
            "editor",
            "focused",
            "ime",
            "input_generation",
            "navigator",
            "overlay",
            "pane_layout",
            "schema_version",
            "status",
            "tab",
            "terminal",
            "ui_state",
            "zoomed",
        ]
    );
    assert_eq!(snapshot["schema_version"], 1);
    // The default test options leave the herdr socket unconfigured.
    assert_eq!(snapshot["status"]["herdr"]["state"], "unconfigured");
    assert_eq!(snapshot["status"]["remote"][0]["target_id"], "mini");
    assert_eq!(snapshot["status"]["chromux"]["profile"], "default");
    let environment = snapshot["status"]["environment"]
        .as_array()
        .expect("environment registry array");
    assert_eq!(
        environment
            .iter()
            .map(|entry| entry["key"].as_str().expect("environment key"))
            .collect::<Vec<_>>(),
        ["SSH_AUTH_SOCK", "PATH", "HERDR_SOCKET_PATH"]
    );
    assert!(environment.iter().all(|entry| entry.get("value").is_none()));
    assert!(snapshot["status"]["last_error"].is_null());
    assert!(snapshot.get("spike").is_none());

    herdr_core_destroy(core);
}

#[test]
fn session_snapshot_exposes_authoritative_recursive_layout_and_per_pane_state() {
    let core = create();
    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "session_snapshot",
            "payload": {
                "focused_pane_id": "w-grid:p3",
                "agents": [],
                "layouts": [{
                    "workspace_id": "w-grid",
                    "tab_id": "w-grid:t1",
                    "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 120, "height": 60},
                    "focused_pane_id": "w-grid:p3",
                    "panes": [
                        {"pane_id": "w-grid:p1", "rect": {"x": 0, "y": 0, "width": 60, "height": 60}},
                        {"pane_id": "w-grid:p2", "rect": {"x": 60, "y": 0, "width": 60, "height": 30}},
                        {"pane_id": "w-grid:p3", "rect": {"x": 60, "y": 30, "width": 60, "height": 30}}
                    ],
                    "splits": [
                        {"direction": "right", "ratio": 0.5, "rect": {"x": 0, "y": 0, "width": 120, "height": 60}},
                        {"direction": "down", "ratio": 0.5, "rect": {"x": 60, "y": 0, "width": 60, "height": 60}}
                    ]
                }]
            }
        }),
    );
    let snapshot = snapshot(core);

    assert_eq!(snapshot["pane_layout"]["workspace_id"], "w-grid");
    assert_eq!(snapshot["pane_layout"]["tab_id"], "w-grid:t1");
    assert_eq!(snapshot["pane_layout"]["root"]["type"], "split");
    assert_eq!(snapshot["pane_layout"]["root"]["direction"], "right");
    assert_eq!(
        snapshot["pane_layout"]["root"]["second"]["direction"],
        "down"
    );
    assert_eq!(snapshot["terminal"]["pane_id"], "w-grid:p3");
    assert_eq!(
        snapshot["terminal"]["panes"]
            .as_array()
            .expect("pane state array")
            .iter()
            .map(|pane| pane["pane_id"].as_str().expect("pane id"))
            .collect::<Vec<_>>(),
        ["w-grid:p1", "w-grid:p2", "w-grid:p3"]
    );
    assert!(snapshot["zoomed"].is_null());

    herdr_core_destroy(core);
}

#[test]
fn session_poll_cannot_retarget_an_explicit_pane_to_an_unrelated_workspace() {
    let core = create();
    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "session_snapshot",
            "payload": {
                "focused_pane_id": "fixture:p1",
                "agents": [],
                "layouts": [single_pane_layout("fixture", "fixture:p1")]
            }
        }),
    );
    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "session_snapshot",
            "payload": {
                "focused_pane_id": "user:p1",
                "agents": [],
                "layouts": [single_pane_layout("user", "user:p1")]
            }
        }),
    );

    let snapshot = snapshot(core);
    assert_eq!(snapshot["terminal"]["pane_id"], "fixture:p1");
    assert_eq!(snapshot["pane_layout"]["workspace_id"], "fixture");

    herdr_core_destroy(core);
}

#[test]
fn close_pane_requires_confirmation_only_for_working_or_attention_states() {
    let core = create();
    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "session_snapshot",
            "payload": {
                "agents": [
                    {"pane_id":"working","workspace_label":"Fixture","agent":"codex","agent_status":"working","tokens":{"status_working":"●","sort_rank":"05","activity":"0000000000003","summary":"Running task","elapsed":"1m"}},
                    {"pane_id":"attention","workspace_label":"Fixture","agent":"codex","agent_status":"idle","tokens":{"status_question_new":"?","sort_rank":"01","activity":"0000000000002","summary":"Needs answer","elapsed":"2m"}},
                    {"pane_id":"idle","workspace_label":"Fixture","agent":"codex","agent_status":"idle","tokens":{"status_idle":"○","sort_rank":"10","activity":"0000000000001","summary":"Idle","elapsed":"3m"}}
                ]
            }
        }),
    );

    for pane_id in ["working", "attention"] {
        dispatch(
            core,
            json!({"schema_version": 1, "kind": "close_pane", "payload": {"pane_id": pane_id, "confirmed": false}}),
        );
        let rejected = snapshot(core);
        assert_eq!(
            rejected["status"]["last_error"]["kind"],
            "pane.close_confirmation_required"
        );
    }

    dispatch(
        core,
        json!({"schema_version": 1, "kind": "close_pane", "payload": {"pane_id": "idle", "confirmed": false}}),
    );
    let idle = snapshot(core);
    assert_eq!(
        idle["status"]["last_error"]["kind"],
        "pane.control_unavailable"
    );

    dispatch(
        core,
        json!({"schema_version": 1, "kind": "close_pane", "payload": {"pane_id": "working", "confirmed": true}}),
    );
    let confirmed = snapshot(core);
    assert_eq!(
        confirmed["status"]["last_error"]["kind"],
        "pane.control_unavailable"
    );

    herdr_core_destroy(core);
}

#[test]
fn unknown_kind_and_version_mismatch_are_observable() {
    let core = create();

    dispatch(
        core,
        json!({"schema_version": 1, "kind": "invented", "payload": {}}),
    );
    let unknown = snapshot(core);
    assert_eq!(
        unknown["status"]["last_error"]["kind"],
        "event.unknown_kind"
    );
    assert_eq!(unknown["status"]["last_error"]["retryable"], false);
    assert!(unknown["status"]["last_error"]["occurred_at"].is_u64());

    dispatch(
        core,
        json!({"schema_version": 99, "kind": "focus_pane", "payload": {"pane_id": "p1"}}),
    );
    let mismatch = snapshot(core);
    assert_eq!(
        mismatch["status"]["last_error"]["kind"],
        "schema_version.mismatch"
    );

    herdr_core_destroy(core);
}

#[test]
fn malformed_payload_is_observable_and_valid_input_clears_it() {
    let core = create();

    dispatch(
        core,
        json!({"schema_version": 1, "kind": "focus_pane", "payload": {}}),
    );
    assert_eq!(
        snapshot(core)["status"]["last_error"]["kind"],
        "event.invalid_payload"
    );

    dispatch(
        core,
        json!({"schema_version": 1, "kind": "key", "payload": {"pane_id": "p1", "bytes_base64": "fw=="}}),
    );
    let valid = snapshot(core);
    assert!(valid["status"]["last_error"].is_null());
    assert_eq!(valid["input_generation"], 1);
    assert_eq!(valid["focused"]["pane_id"], "p1");

    herdr_core_destroy(core);
}

#[test]
fn pane_split_direction_and_zoom_events_reach_the_live_control_boundary() {
    let core = create();
    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "session_snapshot",
            "payload": {
                "focused_pane_id": "w1:p1",
                "agents": [],
                "layouts": [single_pane_layout("w1", "w1:p1")]
            }
        }),
    );
    for direction in ["right", "down"] {
        dispatch(
            core,
            json!({
                "schema_version": 1,
                "kind": "create_pane",
                "payload": {
                    "tab_id": "t1",
                    "cwd": "/tmp/herdr-ide-verify-shortcuts",
                    "command": null,
                    "direction": direction
                }
            }),
        );
        assert_eq!(
            snapshot(core)["status"]["last_error"]["kind"],
            "pane.control_unavailable"
        );
    }
    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "toggle_zoom",
            "payload": {"pane_id": "w1:p1"}
        }),
    );
    assert_eq!(
        snapshot(core)["status"]["last_error"]["kind"],
        "pane.control_unavailable"
    );
    herdr_core_destroy(core);
}

#[test]
fn pane_control_dispatch_does_not_wait_for_the_child_process() {
    let suffix = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "herdr-core-pane-control-latency-{}-{suffix}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("latency fixture directory");
    let herdr_bin = root.join("herdr-latency-fixture");
    fs::write(
        &herdr_bin,
        concat!(
            "#!/bin/sh\n",
            "if [ \"$1\" = pane ] && [ \"$2\" = attach ]; then\n",
            "  exec /usr/bin/tail -f /dev/null\n",
            "fi\n",
            "/bin/sleep 1\n",
            "/usr/bin/printf '%s\\n' '{\"id\":\"test\",\"result\":{\"pane\":{\"pane_id\":\"w-test:p2\"}}}'\n",
        ),
    )
    .expect("latency fixture executable");
    fs::set_permissions(&herdr_bin, fs::Permissions::from_mode(0o755))
        .expect("latency fixture permissions");
    let state_path = root.join("state.json");
    let options = slow_live_options(&state_path, &herdr_bin);
    let core = create_with_socket_override_hidden(&options);
    assert!(!core.is_null());

    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "session_snapshot",
            "payload": {
                "focused_pane_id": "w-test:p1",
                "agents": [],
                "layouts": [single_pane_layout("w-test", "w-test:p1")]
            }
        }),
    );
    let _attach_ready = wait_for_snapshot(core, Duration::from_secs(2), |current| {
        current["status"]["diagnostics"]
            .as_array()
            .is_some_and(|diagnostics| {
                diagnostics.iter().any(|diagnostic| {
                    diagnostic["kind"] == "pane.attach.ready"
                        && diagnostic["message"]
                            .as_str()
                            .is_some_and(|message| message.contains("w-test:p1"))
                })
            })
    });
    let started = Instant::now();
    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "create_pane",
            "payload": {
                "tab_id": "w-test:t1",
                "cwd": root,
                "command": null,
                "direction": "right"
            }
        }),
    );
    let elapsed = started.elapsed();
    let returned_without_waiting = elapsed < Duration::from_millis(250);

    let split_result = wait_for_snapshot(core, Duration::from_secs(3), |current| {
        current["status"]["last_error"]["kind"] == "pane.layout_refresh_failed"
    });
    assert_eq!(split_result["terminal"]["pane_id"], "w-test:p1");

    let focus_started = Instant::now();
    dispatch(
        core,
        json!({"schema_version": 1, "kind": "focus_pane", "payload": {"pane_id": "w-test:p3"}}),
    );
    let focus_elapsed = focus_started.elapsed();
    let focus_returned_without_waiting = focus_elapsed < Duration::from_millis(250);
    let focus_result = wait_for_snapshot(core, Duration::from_secs(2), |current| {
        current["status"]["last_error"]["kind"] == "pane.focus_failed"
    });
    assert_eq!(focus_result["terminal"]["pane_id"], "w-test:p1");

    let zoom_started = Instant::now();
    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "toggle_zoom",
            "payload": {"pane_id": "w-test:p3"}
        }),
    );
    let zoom_elapsed = zoom_started.elapsed();
    let zoom_returned_without_waiting = zoom_elapsed < Duration::from_millis(250);
    let zoom_ready = wait_for_snapshot(core, Duration::from_secs(3), |current| {
        current["status"]["diagnostics"]
            .as_array()
            .is_some_and(|diagnostics| {
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic["kind"] == "pane.zoom_toggled")
            })
    });
    // The child-process seam has no Herdr layout server, so core refuses to
    // invent zoom state and waits for an authoritative session snapshot.
    assert!(zoom_ready["zoomed"].is_null());

    herdr_core_destroy(core);
    fs::remove_dir_all(root).expect("latency fixture cleanup");
    assert!(
        returned_without_waiting,
        "owner-thread dispatch waited {elapsed:?} for the pane-control child"
    );
    assert!(
        focus_returned_without_waiting,
        "owner-thread focus dispatch waited {focus_elapsed:?} for the socket worker"
    );
    assert!(
        zoom_returned_without_waiting,
        "owner-thread zoom dispatch waited {zoom_elapsed:?} for pane control"
    );
}

#[test]
fn multi_pane_attach_destroy_returns_without_waiting_and_reaps_children() {
    let suffix = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "herdr-core-attach-lifecycle-{}-{suffix}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("lifecycle fixture directory");
    let herdr_bin = root.join("herdr-attach-fixture");
    fs::write(
        &herdr_bin,
        format!(
            concat!(
                "#!/bin/sh\n",
                "if [ \"$1\" = pane ] && [ \"$2\" = attach ]; then\n",
                "  /usr/bin/printf '%s' \"$$\" > '{}/'$3.pid\n",
                "  exec /usr/bin/tail -f /dev/null\n",
                "fi\n",
                "exit 1\n",
            ),
            root.display()
        ),
    )
    .expect("attach fixture executable");
    fs::set_permissions(&herdr_bin, fs::Permissions::from_mode(0o755))
        .expect("attach fixture permissions");
    let state_path = root.join("state.json");
    let options = slow_live_options(&state_path, &herdr_bin);
    let core = create_with_socket_override_hidden(&options);
    assert!(!core.is_null());

    let panes = ["w-lifecycle:p1", "w-lifecycle:p2", "w-lifecycle:p3"];
    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "session_snapshot",
            "payload": {
                "focused_pane_id": "w-lifecycle:p1",
                "agents": [],
                "layouts": [{
                    "workspace_id": "w-lifecycle",
                    "tab_id": "w-lifecycle:t1",
                    "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 120, "height": 60},
                    "focused_pane_id": "w-lifecycle:p1",
                    "panes": [
                        {"pane_id": "w-lifecycle:p1", "rect": {"x": 0, "y": 0, "width": 60, "height": 60}},
                        {"pane_id": "w-lifecycle:p2", "rect": {"x": 60, "y": 0, "width": 60, "height": 30}},
                        {"pane_id": "w-lifecycle:p3", "rect": {"x": 60, "y": 30, "width": 60, "height": 30}}
                    ],
                    "splits": [
                        {"direction": "right", "ratio": 0.5, "rect": {"x": 0, "y": 0, "width": 120, "height": 60}},
                        {"direction": "down", "ratio": 0.5, "rect": {"x": 60, "y": 0, "width": 60, "height": 60}}
                    ]
                }]
            }
        }),
    );
    for pane_id in panes {
        let pid_path = root.join(format!("{pane_id}.pid"));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !pid_path.exists() {
            assert!(
                Instant::now() < deadline,
                "attach pid was not recorded for {pane_id}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    let child_pids = panes
        .iter()
        .map(|pane_id| {
            fs::read_to_string(root.join(format!("{pane_id}.pid")))
                .expect("attach pid file")
                .trim()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let destroy_started = Instant::now();
    herdr_core_destroy(core);
    let destroy_elapsed = destroy_started.elapsed();
    assert!(
        destroy_elapsed < Duration::from_millis(250),
        "destroy waited {destroy_elapsed:?} for attach children"
    );

    for pid in child_pids {
        let deadline = Instant::now() + Duration::from_secs(2);
        while std::process::Command::new("/bin/kill")
            .args(["-0", &pid])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            assert!(
                Instant::now() < deadline,
                "attach child {pid} was not reaped"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fs::remove_dir_all(root).expect("lifecycle fixture cleanup");
}

extern "C" fn count_change(context: *mut c_void) {
    let counter = unsafe { &*(context.cast::<AtomicUsize>()) };
    counter.fetch_add(1, Ordering::SeqCst);
}

#[test]
fn callback_fires_for_snapshot_changes_and_unregisters_cleanly() {
    let core = create();
    let counter = AtomicUsize::new(0);
    herdr_core_on_change(
        core,
        Some(count_change),
        (&counter as *const AtomicUsize).cast_mut().cast::<c_void>(),
    );

    dispatch(
        core,
        json!({"schema_version": 1, "kind": "focus_pane", "payload": {"pane_id": "p2"}}),
    );
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    herdr_core_on_change(core, None, std::ptr::null_mut());
    dispatch(
        core,
        json!({"schema_version": 1, "kind": "focus_pane", "payload": {"pane_id": "p3"}}),
    );
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    herdr_core_destroy(core);
}

#[test]
fn off_owner_dispatch_surfaces_a_thread_contract_failure() {
    let core = create();
    let core_address = core as usize;
    let event = serde_json::to_vec(
        &json!({"schema_version": 1, "kind": "focus_pane", "payload": {"pane_id": "p4"}}),
    )
    .expect("event serialize");

    std::thread::spawn(move || {
        herdr_core_dispatch(core_address as *mut HerdrCore, event.as_ptr(), event.len());
    })
    .join()
    .expect("worker exits");

    let snapshot = snapshot(core);
    assert_eq!(snapshot["status"]["last_error"]["kind"], "ffi.wrong_thread");
    herdr_core_destroy(core);
}

#[test]
fn off_owner_snapshot_returns_no_bytes_and_preserves_the_violation() {
    let core = create();
    let core_address = core as usize;

    std::thread::spawn(move || {
        let bytes = herdr_core_snapshot(core_address as *mut HerdrCore);
        assert!(bytes.ptr.is_null());
        assert_eq!(bytes.len, 0);
        assert_eq!(bytes.cap, 0);
    })
    .join()
    .expect("worker exits");

    let owner_snapshot = snapshot(core);
    assert_eq!(
        owner_snapshot["status"]["last_error"]["kind"],
        "ffi.wrong_thread"
    );
    herdr_core_destroy(core);
}

#[test]
fn off_owner_destroy_is_rejected_until_the_owner_destroys() {
    let core = create();
    let core_address = core as usize;

    std::thread::spawn(move || {
        herdr_core_destroy(core_address as *mut HerdrCore);
    })
    .join()
    .expect("worker exits");

    let owner_snapshot = snapshot(core);
    assert_eq!(
        owner_snapshot["status"]["last_error"]["kind"],
        "ffi.wrong_thread"
    );
    herdr_core_destroy(core);
}

#[test]
fn create_rejects_invalid_options_and_snapshot_buffers_can_repeat() {
    let invalid = serde_json::to_vec(&json!({
        "schema_version": 99,
        "herdr_socket_path": null,
        "remote_targets": [],
        "app_state_path": "/tmp/herdr-state.json"
    }))
    .expect("invalid options serialize");
    assert!(herdr_core_create(invalid.as_ptr(), invalid.len()).is_null());

    let core = create();
    for _ in 0..1_000 {
        let bytes = herdr_core_snapshot(core);
        assert!(!bytes.ptr.is_null());
        herdr_core_free_bytes(bytes);
    }
    herdr_core_destroy(core);
}

#[test]
fn terminal_bytes_round_trip_through_the_json_boundary_without_transcoding() {
    let core = create();
    let arbitrary_bytes = "AP+Afg0K";

    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "key",
            "payload": {"pane_id": "pane-terminal", "bytes_base64": arbitrary_bytes}
        }),
    );
    let after_input = snapshot(core);
    assert_eq!(after_input["input_generation"], 1);
    assert_eq!(after_input["terminal"]["pane_id"], "pane-terminal");
    assert_eq!(
        after_input["terminal"]["chunks"][0]["pane_id"],
        "pane-terminal"
    );
    assert_eq!(
        after_input["terminal"]["chunks"][0]["bytes_base64"],
        arbitrary_bytes
    );

    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "terminal_output",
            "payload": {"pane_id": "pane-terminal", "bytes_base64": "4piD7ZWcCg=="}
        }),
    );
    let after_output = snapshot(core);
    assert_eq!(after_output["terminal"]["sequence"], 2);
    assert_eq!(
        after_output["terminal"]["chunks"][1]["pane_id"],
        "pane-terminal"
    );
    assert_eq!(
        after_output["terminal"]["chunks"][1]["bytes_base64"],
        "4piD7ZWcCg=="
    );

    herdr_core_destroy(core);
}

#[test]
fn existing_local_file_opens_and_idempotent_save_preserves_its_contents() {
    let core = create();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../macos/VerificationFixtures/Sample.swift");
    let before = std::fs::read(&path).expect("read committed fixture");
    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "file_open",
            "payload": {"path": path}
        }),
    );
    let opened = snapshot(core);
    assert_eq!(opened["editor"]["language"], "swift");
    assert_eq!(opened["editor"]["dirty"], false);
    let contents = opened["editor"]["contents_utf8"]
        .as_str()
        .expect("UTF-8 fixture");
    let modified = opened["editor"]["opened_modified_at_unix_ms"].clone();
    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "file_save",
            "payload": {
                "path": path,
                "contents_utf8": contents,
                "expected_modified_at_unix_ms": modified
            }
        }),
    );
    let saved = snapshot(core);
    assert_eq!(saved["editor"]["dirty"], false);
    assert!(saved["status"]["last_error"].is_null());
    assert_eq!(std::fs::read(&path).expect("reread fixture"), before);
    herdr_core_destroy(core);
}

#[test]
fn corrupt_ui_state_falls_back_to_defaults_with_structured_status() {
    let corrupt_state =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corrupt-ui-state.json");
    let options = options_with_state(&corrupt_state);
    let core = herdr_core_create(options.as_ptr(), options.len());
    assert!(!core.is_null());
    let current = snapshot(core);
    assert!(
        current["ui_state"]["expanded_paths"]
            .as_array()
            .expect("expanded paths")
            .is_empty()
    );
    assert_eq!(
        current["status"]["diagnostics"][0]["kind"],
        "ui_state.corrupt"
    );
    assert_eq!(
        current["status"]["diagnostics"][0]["message"],
        "UI state could not be decoded; safe defaults were loaded"
    );
    herdr_core_destroy(core);
}

#[test]
fn session_snapshot_keeps_authoritative_agent_order_and_tokens() {
    let core = create();
    dispatch(
        core,
        json!({
            "schema_version": 1,
            "kind": "session_snapshot",
            "payload": {"agents": [
                {"pane_id":"seen-old","workspace_label":"Core","agent":"codex","agent_status":"idle","tokens":{"status_idle":"○","sort_rank":"10","activity":"0000000000010","summary":"Seen older","elapsed":"2h"}},
                {"pane_id":"blocked","workspace_label":"UI","agent":"claude","agent_status":"done","tokens":{"status_question_new":"?","sort_rank":"01","activity":"0000000000001","summary":"Need a decision","elapsed":"12s"}},
                {"pane_id":"seen-new","workspace_label":"Core","agent":"codex","agent_status":"idle","tokens":{"status_idle":"○","sort_rank":"10","activity":"0000000000020","summary":"Seen newer","elapsed":"4m"}}
            ]}
        }),
    );
    let snapshot = snapshot(core);
    let agents = snapshot["navigator"]["agents"]
        .as_array()
        .expect("agent array");
    assert_eq!(agents[0]["pane_id"], "blocked");
    assert_eq!(agents[1]["pane_id"], "seen-new");
    assert_eq!(agents[2]["pane_id"], "seen-old");
    assert_eq!(agents[0]["state"], "question");
    assert_eq!(agents[0]["symbol"], "?");

    herdr_core_destroy(core);
}
