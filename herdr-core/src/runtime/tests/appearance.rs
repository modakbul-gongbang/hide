use super::*;

use crate::model::ThemePreference;

fn runtime_at(path: &std::path::Path) -> Runtime {
    Runtime::new(
        CoreOptions {
            schema_version: SCHEMA_VERSION,
            machine_id: None,
            herdr_socket_path: Some("/tmp/herdr-core-appearance.sock".to_owned()),
            herdr_bin_path: None,
            app_state_path: path.to_string_lossy().into_owned(),
            host_helper_dir: None,
            host_helper_root: None,
            workspace_views_path: None,
            shortcut_import_path: None,
        },
        environment::EnvironmentReport {
            statuses: Vec::new(),
            herdr_socket_path_override: None,
            home_path: None,
            codex_home: None,
        },
    )
}

fn state_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "herdr-core-appearance-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("state dir");
    dir.join("state.json")
}

fn theme_set(theme: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "theme_set",
        "payload": {"theme": theme}
    }))
    .expect("theme event")
}

/// B1, D-14: a first run and an update from a store without a theme open Dark.
#[test]
fn a_store_without_a_theme_opens_dark() {
    let runtime = runtime();
    assert_eq!(runtime.snapshot().ui_state.theme, ThemePreference::Dark);
}

/// B2, B4, D-15: one event changes the theme, and the choice survives a restart.
#[test]
fn theme_set_changes_the_theme_and_a_restart_keeps_it() {
    let path = state_path("roundtrip");
    let mut runtime = runtime_at(&path);
    assert!(runtime.dispatch_json(&theme_set("light")));
    assert_eq!(runtime.snapshot().ui_state.theme, ThemePreference::Light);
    assert!(
        !runtime.dispatch_json(&theme_set("light")),
        "the same theme again is no change"
    );
    assert_eq!(
        serde_json::to_value(runtime.snapshot()).unwrap()["ui_state"]["theme"],
        "light"
    );
    drop(runtime);

    let restarted = runtime_at(&path);
    assert_eq!(restarted.snapshot().ui_state.theme, ThemePreference::Light);

    let mut restarted = restarted;
    assert!(restarted.dispatch_json(&theme_set("system")));
    drop(restarted);
    assert_eq!(
        runtime_at(&path).snapshot().ui_state.theme,
        ThemePreference::System
    );
}

/// B4, D-15: a stored value this build does not know opens Dark and is
/// recorded only as a diagnostic, never as an error the screen shows.
#[test]
fn an_unknown_stored_theme_opens_dark_with_a_diagnostic() {
    let path = state_path("unknown");
    let mut runtime = runtime_at(&path);
    assert!(runtime.dispatch_json(&theme_set("light")));
    drop(runtime);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    stored["theme"] = serde_json::json!("sepia");
    std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();

    let runtime = runtime_at(&path);
    assert_eq!(runtime.snapshot().ui_state.theme, ThemePreference::Dark);
    assert!(
        runtime
            .snapshot()
            .status
            .diagnostics
            .iter()
            .any(|entry| entry.kind == "ui_state.theme_unknown"),
        "the fallback is logged"
    );
    assert!(
        runtime.snapshot().status.last_error.is_none(),
        "nothing reaches the screen"
    );
}

/// A theme the event does not name is refused and the theme stays.
#[test]
fn an_unknown_theme_event_is_refused_and_changes_nothing() {
    let mut runtime = runtime();
    runtime.dispatch_json(&theme_set("sepia"));
    assert_eq!(runtime.snapshot().ui_state.theme, ThemePreference::Dark);
}

/// A shared UI-state save from another control does not reset the theme.
#[test]
fn a_ui_state_update_carries_the_theme_through() {
    let mut runtime = runtime();
    assert!(runtime.dispatch_json(&theme_set("light")));
    let update = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "ui_state_update",
        "payload": {"accent_hex": "#7DD3FC", "font_size": 14}
    }))
    .unwrap();
    assert!(runtime.dispatch_json(&update));
    assert_eq!(runtime.snapshot().ui_state.theme, ThemePreference::Light);
}

/// The web's `ui_state_update`: the whole current state with `patch` over it.
fn ui_state_update(runtime: &Runtime, patch: serde_json::Value) -> Vec<u8> {
    let mut payload = serde_json::to_value(&runtime.snapshot().ui_state).expect("ui state");
    for (key, value) in patch.as_object().expect("patch object") {
        payload[key] = value.clone();
    }
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "ui_state_update",
        "payload": payload
    }))
    .expect("ui state event")
}

/// PRD sidebar-typography B11: a dragged width survives a restart, and a store
/// from before the drag opens at the default.
#[test]
fn a_dragged_sidebar_width_survives_a_restart() {
    let path = state_path("sidebar-width");
    let mut runtime = runtime_at(&path);
    assert_eq!(runtime.snapshot().ui_state.sidebar_width, 292);
    let event = ui_state_update(&runtime, serde_json::json!({"sidebar_width": 360}));
    assert!(runtime.dispatch_json(&event));
    assert_eq!(runtime.snapshot().ui_state.sidebar_width, 360);
    let mut unrelated: serde_json::Value = serde_json::from_slice(&ui_state_update(
        &runtime,
        serde_json::json!({"left_sidebar_visible": false}),
    ))
    .unwrap();
    unrelated["payload"]
        .as_object_mut()
        .unwrap()
        .remove("sidebar_width");
    assert!(runtime.dispatch_json(&serde_json::to_vec(&unrelated).unwrap()));
    assert_eq!(
        runtime.snapshot().ui_state.sidebar_width,
        360,
        "an unrelated UI event without width preserves the completed drag"
    );
    drop(runtime);
    assert_eq!(runtime_at(&path).snapshot().ui_state.sidebar_width, 360);
}

/// PRD sidebar-typography B13, D-18 (3): a width outside 220..=440 is refused
/// into the diagnostic log, never presented, and the last width stays while
/// the rest of the event applies.
#[test]
fn an_out_of_range_sidebar_width_is_refused_into_the_log() {
    let path = state_path("sidebar-width-range");
    let mut runtime = runtime_at(&path);
    let event = ui_state_update(&runtime, serde_json::json!({"sidebar_width": 300}));
    assert!(runtime.dispatch_json(&event));
    let error_before = runtime.snapshot().status.last_error.clone();
    for width in [219, 441, 0] {
        let event = ui_state_update(
            &runtime,
            serde_json::json!({"sidebar_width": width, "font_size": 15.0}),
        );
        runtime.dispatch_json(&event);
        assert_eq!(
            runtime.snapshot().ui_state.sidebar_width,
            300,
            "{width} is refused"
        );
        assert!(
            runtime
                .snapshot()
                .status
                .diagnostics
                .iter()
                .any(|entry| entry.kind == "ui_state.sidebar_width_out_of_range"
                    && entry.message.contains(&width.to_string())),
            "{width} is recorded"
        );
    }
    assert_eq!(
        runtime.snapshot().ui_state.font_size,
        15.0,
        "the rest of the event applies"
    );
    assert_eq!(
        runtime.snapshot().status.last_error,
        error_before,
        "nothing is presented"
    );
    for width in [220, 440] {
        let event = ui_state_update(&runtime, serde_json::json!({"sidebar_width": width}));
        assert!(runtime.dispatch_json(&event));
        assert_eq!(
            runtime.snapshot().ui_state.sidebar_width,
            width,
            "{width} is a bound, not outside it"
        );
    }
}

/// A stored width the drag could not have produced opens at the default, with a diagnostic.
#[test]
fn an_out_of_range_stored_sidebar_width_opens_at_the_default() {
    let path = state_path("sidebar-width-stored");
    let mut runtime = runtime_at(&path);
    let event = ui_state_update(&runtime, serde_json::json!({"sidebar_width": 400}));
    assert!(runtime.dispatch_json(&event));
    drop(runtime);
    let mut stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("saved state")).expect("json");
    assert_eq!(stored["sidebar_width"], 400);
    stored["sidebar_width"] = serde_json::json!(900);
    std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).expect("edit store");
    let restarted = runtime_at(&path);
    assert_eq!(restarted.snapshot().ui_state.sidebar_width, 292);
    assert!(
        restarted
            .snapshot()
            .status
            .diagnostics
            .iter()
            .any(|entry| entry.kind == "ui_state.sidebar_width_out_of_range")
    );
}
