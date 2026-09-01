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
        "schema_version": 2,
        "herdr_socket_path": null,
        "remote_targets": [],
        "app_state_path": state_path
    }))
    .expect("options serialize")
}

fn slow_live_options(state_path: &std::path::Path, herdr_bin_path: &std::path::Path) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema_version": 2,
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
fn live_key_without_control_session_surfaces_an_explicit_error() {
    let missing_state =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/missing-ui-state.json");
    let options = serde_json::to_vec(&json!({
        "schema_version": 2,
        "herdr_socket_path": "/tmp/herdr-core-ffi-test-missing.sock",
        "remote_targets": [],
        "app_state_path": missing_state
    }))
    .expect("options serialize");
    let core = create_with_socket_override_hidden(&options);
    assert!(!core.is_null());

    dispatch(
        core,
        json!({"schema_version": 2, "kind": "key", "payload": {"pane_id": "p1", "bytes_base64": "fw=="}}),
    );
    let after = snapshot(core);
    assert_eq!(
        after["status"]["last_error"]["kind"],
        "terminal.unavailable"
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

fn create_file_test(name: &str) -> (*mut HerdrCore, PathBuf) {
    let state_path = std::env::temp_dir().join(format!(
        "herdr-core-{name}-{}-state.json",
        std::process::id()
    ));
    let _ = fs::remove_file(&state_path);
    let options = options_with_state(&state_path);
    let core = herdr_core_create(options.as_ptr(), options.len());
    assert!(!core.is_null());
    (core, state_path)
}

fn dispatch(core: *mut HerdrCore, event: Value) {
    let bytes = serde_json::to_vec(&event).expect("event serialize");
    herdr_core_dispatch(core, bytes.as_ptr(), bytes.len());
}

fn snapshot_delta(core: *mut HerdrCore, have_revision: u64, have_sequence: u64) -> Value {
    let bytes = herdr_core_snapshot(core, have_revision, have_sequence);
    assert!(!bytes.ptr.is_null());
    let parsed = unsafe { serde_json::from_slice(slice::from_raw_parts(bytes.ptr, bytes.len)) }
        .expect("snapshot JSON");
    herdr_core_free_bytes(bytes);
    parsed
}

/// Full read composed the way the shell composes it: a 0/0 cursor read must
/// carry `rest` and `editor`, and chunks join the terminal section.
fn snapshot(core: *mut HerdrCore) -> Value {
    let delta = snapshot_delta(core, 0, 0);
    let mut full = delta["rest"].clone();
    assert!(full.is_object(), "a 0/0 read must include rest: {delta}");
    assert!(
        delta["editor"].is_object(),
        "a 0/0 read must include editor: {delta}"
    );
    full["editor"] = delta["editor"].clone();
    full["schema_version"] = delta["schema_version"].clone();
    full["input_generation"] = delta["input_generation"].clone();
    full["terminal"]["chunks"] = delta["chunks"].clone();
    full["terminal"]["sequence"] = delta["terminal_sequence"].clone();
    full
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

fn install_file_context(core: *mut HerdrCore, root: &std::path::Path) -> (String, String) {
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "session_snapshot", "payload": {
            "agents": [],
            "workspaces": [{"workspace_id": "w-files", "label": "Files"}],
            "tabs": [{"workspace_id": "w-files", "tab_id": "w-files:t1", "label": "1"}],
            "panes": [{"pane_id": "files-pane", "cwd": root}],
            "layouts": [single_pane_layout("w-files", "files-pane")]
        }}),
    );
    let current = snapshot(core);
    let workspace = &current["navigator"]["workspaces"][0];
    (
        workspace["id"].as_str().expect("workspace id").to_owned(),
        workspace["checkouts"][0]["id"]
            .as_str()
            .expect("checkout id")
            .to_owned(),
    )
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
            "pet",
            "schema_version",
            "status",
            "tab",
            "terminal",
            "ui_state",
            "zoomed",
        ]
    );
    assert_eq!(snapshot["schema_version"], 2);
    // The default test options leave the herdr socket unconfigured.
    assert_eq!(snapshot["status"]["herdr"]["state"], "unconfigured");
    assert_eq!(snapshot["status"]["remote"], json!([]));
    assert_eq!(snapshot["status"]["chromux"]["profile"], "default");
    let environment = snapshot["status"]["environment"]
        .as_array()
        .expect("environment registry array");
    assert_eq!(
        environment
            .iter()
            .map(|entry| entry["key"].as_str().expect("environment key"))
            .collect::<Vec<_>>(),
        ["HOME", "SSH_AUTH_SOCK", "PATH", "HERDR_SOCKET_PATH"]
    );
    assert!(environment.iter().all(|entry| entry.get("value").is_none()));
    let provider_usage = snapshot["navigator"]["provider_usage"]
        .as_array()
        .expect("provider usage rows");
    assert_eq!(provider_usage.len(), 2);
    assert_eq!(provider_usage[0]["provider"], "claude");
    assert_eq!(provider_usage[0]["window_minutes"], 10_080);
    assert_eq!(provider_usage[0]["state"], "unavailable");
    assert_eq!(provider_usage[1]["provider"], "codex");
    assert_eq!(provider_usage[1]["window_minutes"], 10_080);
    assert_eq!(provider_usage[1]["state"], "unavailable");
    assert!(snapshot["status"]["last_error"].is_null());
    assert!(snapshot.get("spike").is_none());

    // The pet rides the existing snapshot rather than a seventh ABI function.
    let mut pet_keys = snapshot["pet"]
        .as_object()
        .expect("pet object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    pet_keys.sort_unstable();
    assert_eq!(
        pet_keys,
        [
            "attention_pane_ids",
            "badges",
            "connection",
            "connection_message",
            "origin",
            "pose",
            "roam_allowed",
            "shortcut",
            "shortcut_error",
            "sleep_phase",
            "theme_id",
            "visible",
        ]
    );
    // No socket configured means the pet says so instead of posing idle.
    assert_eq!(snapshot["pet"]["connection"], "unconfigured");
    assert_eq!(snapshot["pet"]["pose"], "disconnected");
    assert_eq!(snapshot["pet"]["visible"], true);
    assert_eq!(snapshot["pet"]["theme_id"], "default");
    assert!(
        snapshot["pet"]["attention_pane_ids"]
            .as_array()
            .expect("attention array")
            .is_empty()
    );

    herdr_core_destroy(core);
}

fn pet_agent(pane_id: &str, token: &str, symbol: &str, rank: &str, activity: &str) -> Value {
    json!({
        "pane_id": pane_id,
        "workspace_label": "Fixture",
        "agent": "codex",
        "agent_status": "unknown",
        "tokens": {token: symbol, "sort_rank": rank, "activity": activity}
    })
}

#[test]
fn pet_state_rides_the_snapshot_and_reflects_agent_status() {
    let core = create();
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "session_snapshot", "payload": {
            "agents": [
                pet_agent("busy-a", "status_working", "\u{25cf}", "05", "0000000000002"),
                pet_agent("busy-b", "status_working", "\u{25cf}", "05", "0000000000003"),
                pet_agent("quiet", "status_idle", "\u{25cb}", "10", "0000000000001")
            ],
            "layouts": [single_pane_layout("w1", "busy-a")]
        }}),
    );
    let working = snapshot(core);
    assert_eq!(
        working["pet"]["pose"], "juggling",
        "two working panes juggle"
    );
    assert_eq!(working["pet"]["badges"]["working"], 2);
    assert_eq!(working["pet"]["badges"]["attention"], 0);
    assert_eq!(working["pet"]["badges"]["error"], 0);
    assert_eq!(working["pet"]["connection"], "connected");

    // An unseen question outranks the working panes and joins the queue; the
    // acknowledged one on the same snapshot does not.
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "session_snapshot", "payload": {
            "agents": [
                pet_agent("busy-a", "status_working", "\u{25cf}", "05", "0000000000002"),
                json!({"pane_id": "asked", "workspace_label": "Fixture", "agent": "claude",
                       "agent_status": "done",
                       "tokens": {"status_question_new": "?", "sort_rank": "01",
                                  "activity": "0000000000004"}}),
                json!({"pane_id": "acknowledged", "workspace_label": "Fixture", "agent": "claude",
                       "agent_status": "idle",
                       "tokens": {"status_question": "?", "sort_rank": "10",
                                  "activity": "0000000000005"}})
            ],
            "layouts": [single_pane_layout("w1", "busy-a")]
        }}),
    );
    let asked = snapshot(core);
    assert_eq!(asked["pet"]["pose"], "notification");
    assert_eq!(asked["pet"]["badges"]["attention"], 1);
    assert_eq!(
        asked["pet"]["attention_pane_ids"],
        json!(["asked"]),
        "only the unseen question is jumpable"
    );

    herdr_core_destroy(core);
}

#[test]
fn every_pet_toggle_surface_writes_one_shared_visibility_that_survives_relaunch() {
    let state_path = std::env::temp_dir().join(format!(
        "herdr-core-pet-visibility-{}.json",
        std::process::id()
    ));
    let _ = fs::remove_file(&state_path);
    let options = options_with_state(&state_path);
    let core = create_with_socket_override_hidden(&options);
    assert!(!core.is_null());
    assert_eq!(snapshot(core)["pet"]["visible"], true);

    // Explicit set (Settings toggle, URL scheme hide/show) and the shared
    // toggle (menu bar, shortcut, URL scheme toggle) reach the same state.
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "pet_set_visible", "payload": {"visible": false}}),
    );
    assert_eq!(snapshot(core)["pet"]["visible"], false);
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "pet_set_visible", "payload": {"visible": false}}),
    );
    assert_eq!(
        snapshot(core)["pet"]["visible"],
        false,
        "setting the same visibility twice converges"
    );
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "pet_toggle_visible", "payload": {}}),
    );
    assert_eq!(snapshot(core)["pet"]["visible"], true);
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "pet_toggle_visible", "payload": {}}),
    );
    assert_eq!(snapshot(core)["pet"]["visible"], false);

    dispatch(
        core,
        json!({"schema_version": 2, "kind": "pet_move", "payload": {"x": 320.0, "y": 96.0}}),
    );
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "pet_shortcut_update",
               "payload": {"accelerator": "command+option+p"}}),
    );
    herdr_core_destroy(core);

    // Relaunch: hidden stays hidden, and the position and shortcut come back.
    let relaunched = create_with_socket_override_hidden(&options);
    let restored = snapshot(relaunched);
    assert_eq!(restored["pet"]["visible"], false);
    assert_eq!(restored["pet"]["origin"]["x"], 320.0);
    assert_eq!(restored["pet"]["origin"]["y"], 96.0);
    assert_eq!(restored["pet"]["shortcut"], "command+option+p");

    // A blank accelerator clears the binding so nothing is registered.
    dispatch(
        relaunched,
        json!({"schema_version": 2, "kind": "pet_shortcut_update",
               "payload": {"accelerator": "  "}}),
    );
    assert!(snapshot(relaunched)["pet"]["shortcut"].is_null());

    // A keyboard save must not erase the pet's own placement.
    dispatch(
        relaunched,
        json!({"schema_version": 2, "kind": "ui_state_update", "payload": {
            "expanded_paths": [], "selected_path": null, "selected_pane_id": null,
            "shortcut_bindings": {"split_right": "command+option+r"}
        }}),
    );
    let after_keyboard_save = snapshot(relaunched);
    assert_eq!(after_keyboard_save["pet"]["origin"]["x"], 320.0);
    assert_eq!(after_keyboard_save["pet"]["visible"], false);

    herdr_core_destroy(relaunched);
    let _ = fs::remove_file(&state_path);
}

#[test]
fn a_broken_agent_record_excludes_only_itself_and_reports_the_exclusion() {
    let core = create();
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "session_snapshot", "payload": {
            "agents": [
                pet_agent("intact", "status_working", "\u{25cf}", "05", "0000000000002"),
                json!({"pane_id": "broken", "workspace_label": "Fixture", "agent": "codex",
                       "agent_status": "working",
                       "tokens": {"status_working": "\u{25cf}", "sort_rank": "oops",
                                  "activity": "0000000000001"}})
            ],
            "layouts": [single_pane_layout("w1", "intact")]
        }}),
    );
    let excluded = snapshot(core);
    assert_eq!(
        excluded["navigator"]["agents"]
            .as_array()
            .expect("agents array")
            .len(),
        1,
        "the readable agent survives its broken neighbour"
    );
    assert_eq!(excluded["pet"]["badges"]["working"], 1);
    assert_eq!(
        excluded["status"]["herdr"]["state"], "connected",
        "one broken record is not a broken snapshot"
    );
    let diagnostics = excluded["status"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    let exclusion = diagnostics
        .iter()
        .find(|entry| entry["kind"] == "agent.excluded")
        .expect("the exclusion is stated, not swallowed");
    assert!(
        exclusion["message"]
            .as_str()
            .expect("exclusion message")
            .contains("broken"),
        "the log names the excluded pane: {exclusion}"
    );

    herdr_core_destroy(core);
}

#[test]
fn official_agent_statuses_survive_without_optional_plugin_tokens() {
    let core = create();
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "session_snapshot", "payload": {
            "agents": [
                {"pane_id":"working","workspace_label":"Fixture","agent":"codex",
                 "agent_status":"working","state_change_seq":1},
                {"pane_id":"blocked","workspace_label":"Fixture","agent":"codex",
                 "agent_status":"blocked","state_change_seq":2},
                {"pane_id":"done","workspace_label":"Fixture","agent":"codex",
                 "agent_status":"done","state_change_seq":3}
            ],
            "layouts": [single_pane_layout("w1", "working")]
        }}),
    );

    let projected = snapshot(core);
    let agents = projected["navigator"]["agents"]
        .as_array()
        .expect("agents array");
    assert_eq!(agents.len(), 3);
    let state_for = |pane_id: &str| {
        agents
            .iter()
            .find(|agent| agent["pane_id"] == pane_id)
            .and_then(|agent| agent["state"].as_str())
            .expect("projected agent state")
    };
    assert_eq!(state_for("working"), "working");
    assert_eq!(state_for("blocked"), "blocked");
    assert_eq!(state_for("done"), "unseen_completion");
    assert_eq!(projected["pet"]["badges"]["working"], 1);
    assert_eq!(projected["pet"]["badges"]["attention"], 1);
    assert_eq!(projected["pet"]["badges"]["done"], 1);
    assert!(
        projected["status"]["diagnostics"]
            .as_array()
            .expect("diagnostics array")
            .iter()
            .all(|entry| entry["kind"] != "agent.excluded")
    );

    herdr_core_destroy(core);
}

#[test]
fn session_snapshot_exposes_authoritative_recursive_layout_and_per_pane_state() {
    let core = create();
    dispatch(
        core,
        json!({
            "schema_version": 2,
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
fn session_sync_cannot_retarget_an_explicit_pane_to_an_unrelated_workspace() {
    let core = create();
    dispatch(
        core,
        json!({
            "schema_version": 2,
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
            "schema_version": 2,
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
    assert!(snapshot["pane_layout"].is_null());
    assert!(
        snapshot["terminal"]["panes"]
            .as_array()
            .expect("terminal pane state")
            .is_empty()
    );
    assert_eq!(
        snapshot["status"]["last_error"]["kind"],
        "pane.projection_unavailable"
    );
    assert_eq!(snapshot["status"]["last_error"]["retryable"], true);

    herdr_core_destroy(core);
}

#[test]
fn close_pane_requires_confirmation_only_for_working_or_attention_states() {
    let core = create();
    dispatch(
        core,
        json!({
            "schema_version": 2,
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
            json!({"schema_version": 2, "kind": "close_pane", "payload": {"pane_id": pane_id, "confirmed": false}}),
        );
        let rejected = snapshot(core);
        assert_eq!(
            rejected["status"]["last_error"]["kind"],
            "pane.close_confirmation_required"
        );
    }

    dispatch(
        core,
        json!({"schema_version": 2, "kind": "close_pane", "payload": {"pane_id": "idle", "confirmed": false}}),
    );
    let idle = snapshot(core);
    assert_eq!(
        idle["status"]["last_error"]["kind"],
        "pane.control_unavailable"
    );

    dispatch(
        core,
        json!({"schema_version": 2, "kind": "close_pane", "payload": {"pane_id": "working", "confirmed": true}}),
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
        json!({"schema_version": 2, "kind": "invented", "payload": {}}),
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
        json!({"schema_version": 2, "kind": "pet_click", "payload": {}}),
    );
    let retired_pet_click = snapshot(core);
    assert_eq!(
        retired_pet_click["status"]["last_error"]["kind"],
        "event.unknown_kind"
    );
    assert!(
        retired_pet_click["ui_state"]["selected_pane_id"].is_null(),
        "the retired direct-jump event cannot focus a pane"
    );

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
        json!({"schema_version": 2, "kind": "focus_pane", "payload": {}}),
    );
    assert_eq!(
        snapshot(core)["status"]["last_error"]["kind"],
        "event.invalid_payload"
    );

    dispatch(
        core,
        json!({"schema_version": 2, "kind": "key", "payload": {"pane_id": "p1", "bytes_base64": "fw=="}}),
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
            "schema_version": 2,
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
                "schema_version": 2,
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
            "schema_version": 2,
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
fn multi_pane_terminal_session_destroy_releases_and_reaps_children() {
    let suffix = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "herdr-core-terminal-lifecycle-{}-{suffix}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("lifecycle fixture directory");
    let herdr_bin = root.join("herdr-terminal-fixture");
    fs::write(
        &herdr_bin,
        format!(
            concat!(
                "#!/bin/sh\n",
                "if [ \"$1\" = terminal ] && [ \"$2\" = session ] && [ \"$3\" = control ]; then\n",
                "  /usr/bin/printf '%s' \"$$\" > '{}/'$4.pid\n",
                "  /usr/bin/printf '%s\\n' '{{\"type\":\"terminal.frame\",\"seq\":1,\"encoding\":\"ansi\",\"width\":80,\"height\":24,\"full\":true,\"bytes\":\"G2M=\"}}'\n",
                "  while IFS= read -r line; do\n",
                "    /usr/bin/printf '%s\\n' \"$line\" >> '{}/'$4.stdin\n",
                "    case \"$line\" in *'\"type\":\"terminal.release\"'*) exit 0 ;; esac\n",
                "  done\n",
                "  exit 0\n",
                "fi\n",
                "exit 1\n",
            ),
            root.display(),
            root.display()
        ),
    )
    .expect("terminal fixture executable");
    fs::set_permissions(&herdr_bin, fs::Permissions::from_mode(0o755))
        .expect("terminal fixture permissions");
    let state_path = root.join("state.json");
    let options = slow_live_options(&state_path, &herdr_bin);
    let core = create_with_socket_override_hidden(&options);
    assert!(!core.is_null());

    let panes = ["w-lifecycle:p1", "w-lifecycle:p2", "w-lifecycle:p3"];
    dispatch(
        core,
        json!({
            "schema_version": 2,
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
        while fs::read_to_string(&pid_path)
            .ok()
            .is_none_or(|pid| pid.trim().parse::<u32>().is_err())
        {
            assert!(
                Instant::now() < deadline,
                "terminal session pid was not fully recorded for {pane_id}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    let child_pids = panes
        .iter()
        .map(|pane_id| {
            fs::read_to_string(root.join(format!("{pane_id}.pid")))
                .expect("terminal session pid file")
                .trim()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let destroy_started = Instant::now();
    herdr_core_destroy(core);
    let destroy_elapsed = destroy_started.elapsed();
    assert!(
        destroy_elapsed < Duration::from_millis(250),
        "destroy waited {destroy_elapsed:?} for terminal session children"
    );

    for (pane_id, pid) in panes.iter().zip(child_pids) {
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
                "terminal session child {pid} was not reaped"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let input = fs::read_to_string(root.join(format!("{pane_id}.stdin")))
            .expect("terminal control stdin capture");
        assert_eq!(
            input.lines().last(),
            Some(r#"{"type":"terminal.release"}"#),
            "drop sends the official release frame for {pane_id}"
        );
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
        json!({"schema_version": 2, "kind": "focus_pane", "payload": {"pane_id": "p2"}}),
    );
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    herdr_core_on_change(core, None, std::ptr::null_mut());
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "focus_pane", "payload": {"pane_id": "p3"}}),
    );
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    herdr_core_destroy(core);
}

#[test]
fn off_owner_dispatch_surfaces_a_thread_contract_failure() {
    let core = create();
    let core_address = core as usize;
    let event = serde_json::to_vec(
        &json!({"schema_version": 2, "kind": "focus_pane", "payload": {"pane_id": "p4"}}),
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
        let bytes = herdr_core_snapshot(core_address as *mut HerdrCore, 0, 0);
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
        let bytes = herdr_core_snapshot(core, 0, 0);
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
            "schema_version": 2,
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
            "schema_version": 2,
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
    let (core, state_path) = create_file_test("file-save");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../macos/VerificationFixtures/Sample.swift");
    let before = std::fs::read(&path).expect("read committed fixture");
    let (workspace_id, checkout_id) =
        install_file_context(core, path.parent().expect("fixture parent"));
    dispatch(
        core,
        json!({
            "schema_version": 2,
            "kind": "file_open",
            "payload": {"path": path, "workspace_id": workspace_id, "checkout_id": checkout_id}
        }),
    );
    let opened = snapshot(core);
    assert_eq!(opened["editor"]["document"]["language"], "swift");
    assert_eq!(opened["editor"]["document"]["dirty"], false);
    let contents = opened["editor"]["document"]["contents_utf8"]
        .as_str()
        .expect("UTF-8 fixture");
    let tab_id = opened["editor"]["active_tab_id"]
        .as_str()
        .expect("active file tab");
    let modified = opened["editor"]["document"]["opened_modified_at_unix_ms"].clone();
    dispatch(
        core,
        json!({
            "schema_version": 2,
            "kind": "file_save",
            "payload": {
                "tab_id": tab_id,
                "path": path,
                "contents_utf8": contents,
                "expected_modified_at_unix_ms": modified
            }
        }),
    );
    let saved = wait_for_snapshot(core, Duration::from_secs(2), |current| {
        current["editor"]["document"]["dirty"] == false
    });
    assert_eq!(saved["editor"]["document"]["dirty"], false);
    assert!(saved["status"]["last_error"].is_null());
    assert_eq!(std::fs::read(&path).expect("reread fixture"), before);
    herdr_core_destroy(core);
    let _ = fs::remove_file(state_path);
}

#[test]
fn file_tabs_deduplicate_and_closing_active_restores_the_previous_file() {
    let (core, state_path) = create_file_test("file-tabs");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../macos/VerificationFixtures/Sample.swift");
    let second_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let (workspace_id, checkout_id) =
        install_file_context(core, path.parent().expect("fixture parent"));
    let open = |path: &PathBuf| {
        dispatch(
            core,
            json!({
                "schema_version": 2,
                "kind": "file_open",
                "payload": {
                    "path": path,
                    "workspace_id": workspace_id.clone(),
                    "checkout_id": checkout_id.clone()
                }
            }),
        );
    };
    open(&path);
    open(&path);
    assert_eq!(
        snapshot(core)["editor"]["tabs"].as_array().map(Vec::len),
        Some(1)
    );

    open(&second_path);
    let second = snapshot(core);
    assert_eq!(second["editor"]["tabs"].as_array().map(Vec::len), Some(2));
    let second_tab_id = second["editor"]["active_tab_id"]
        .as_str()
        .expect("second tab active")
        .to_owned();
    dispatch(
        core,
        json!({
            "schema_version": 2,
            "kind": "file_close",
            "payload": {"tab_id": second_tab_id}
        }),
    );
    let restored = snapshot(core);
    assert_eq!(restored["editor"]["tabs"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        restored["editor"]["document"]["path"],
        path.to_string_lossy().as_ref()
    );
    herdr_core_destroy(core);
    let _ = fs::remove_file(state_path);
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
            "schema_version": 2,
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

#[test]
fn delta_reads_send_only_what_the_cursors_have_not_seen() {
    let core = create();
    // First contact with a pane changes the rest sections (pane registry),
    // so settle that before measuring the chunk-only path.
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "terminal_output", "payload": {"pane_id": "local-loopback", "bytes_base64": "aGk="}}),
    );
    let full = snapshot_delta(core, 0, 0);
    let revision = full["revision"].as_u64().expect("revision");
    let sequence = full["terminal_sequence"].as_u64().expect("sequence");
    assert!(full["rest"].is_object());
    assert!(full["editor"].is_object());
    assert_eq!(full["chunks_dropped"], false);

    // Caught-up cursors: nothing rides.
    let idle = snapshot_delta(core, revision, sequence);
    assert_eq!(idle["revision"], revision);
    assert!(idle["rest"].is_null(), "unchanged rest must be omitted");
    assert!(idle["editor"].is_null(), "unchanged editor must be omitted");
    assert_eq!(idle["chunks"].as_array().map(Vec::len), Some(0));

    // Output to an already-known pane moves only the chunk channel.
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "terminal_output", "payload": {"pane_id": "local-loopback", "bytes_base64": "bW8="}}),
    );
    let chunk_only = snapshot_delta(core, revision, sequence);
    assert!(chunk_only["rest"].is_null());
    assert!(chunk_only["editor"].is_null());
    assert_eq!(chunk_only["chunks"].as_array().map(Vec::len), Some(1));
    assert_eq!(chunk_only["chunks"][0]["bytes_base64"], "bW8=");
    assert_eq!(chunk_only["terminal_sequence"].as_u64(), Some(sequence + 1));

    // A read is not an ack: the same cursors return the same delta again.
    let retried = snapshot_delta(core, revision, sequence);
    assert_eq!(retried["chunks"], chunk_only["chunks"]);

    // Advancing the sequence cursor drains the chunk channel.
    let caught_up = snapshot_delta(core, revision, sequence + 1);
    assert_eq!(caught_up["chunks"].as_array().map(Vec::len), Some(0));

    // A section mutation rides as a new rest revision, not as chunks.
    dispatch(
        core,
        json!({"schema_version": 2, "kind": "focus_pane", "payload": {"pane_id": "local-loopback"}}),
    );
    let rest_changed = snapshot_delta(core, revision, sequence + 1);
    assert!(rest_changed["rest"].is_object());
    assert!(rest_changed["revision"].as_u64() > Some(revision));
    assert_eq!(rest_changed["chunks"].as_array().map(Vec::len), Some(0));

    herdr_core_destroy(core);
}

#[test]
fn chunk_ring_overflow_is_reported_to_a_lagging_cursor() {
    let core = create();
    // The ring retains 512 chunks; push past it while the cursor stays at 0.
    for _ in 0..600 {
        dispatch(
            core,
            json!({"schema_version": 2, "kind": "terminal_output", "payload": {"pane_id": "local-loopback", "bytes_base64": "eA=="}}),
        );
    }
    let lagging = snapshot_delta(core, 0, 0);
    assert_eq!(lagging["chunks_dropped"], true);
    assert_eq!(lagging["chunks"].as_array().map(Vec::len), Some(512));

    let sequence = lagging["terminal_sequence"].as_u64().expect("sequence");
    let current = snapshot_delta(core, 0, sequence);
    assert_eq!(current["chunks_dropped"], false);
    assert_eq!(current["chunks"].as_array().map(Vec::len), Some(0));

    herdr_core_destroy(core);
}
