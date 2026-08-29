use std::ffi::c_void;
use std::path::PathBuf;
use std::slice;
use std::sync::atomic::{AtomicUsize, Ordering};

use herdr_core::{
    HerdrCore, herdr_core_create, herdr_core_destroy, herdr_core_dispatch, herdr_core_free_bytes,
    herdr_core_on_change, herdr_core_snapshot,
};
use serde_json::{Value, json};

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
    let core = herdr_core_create(options.as_ptr(), options.len());
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
