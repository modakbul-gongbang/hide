use super::*;

use crate::model::{AgentLogoMode, ThemePreference};

pub(super) fn runtime_at(path: &std::path::Path) -> Runtime {
    Runtime::new(
        CoreOptions {
            schema_version: SCHEMA_VERSION,
            home: None,
            node_id: crate::node::test_node(),
            herdr_socket_path: Some("/tmp/herdr-core-appearance.sock".to_owned()),
            herdr_bin_path: None,
            app_state_path: path.to_string_lossy().into_owned(),
            host_helper_root: None,
            host_cli_dir: None,
            workspace_views_path: None,
            shortcut_import_path: None,
            local_issues_path: None,
            effects_held: false,
            machine_name: None,
        },
        environment::EnvironmentReport {
            statuses: Vec::new(),
            home_path: None,
            codex_home: None,
        },
        std::sync::Arc::new(hide_node::Local::of_process()),
        crate::node::test_devices(),
    )
}

/// A new folder for a state file, and that file's path in it; the test keeps
/// the folder across the restarts it makes.
fn state_path(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let folder = scratch_dir(&format!("herdr-core-appearance-{name}-"));
    let path = folder.path().join("state.json");
    (folder, path)
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
    let (_state, path) = state_path("roundtrip");
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
    let (_state, path) = state_path("unknown");
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

fn agent_logo_mode_set(mode: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "agent_logo_mode_set",
        "payload": {"mode": mode}
    }))
    .expect("agent logo mode event")
}

/// B4, D-03: a first run and a store written before the mode existed hide the logos.
#[test]
fn a_store_without_an_agent_logo_mode_hides_the_logos() {
    let (_state, path) = state_path("logo-default");
    let mut runtime = runtime_at(&path);
    assert_eq!(
        runtime.snapshot().ui_state.agent_logo_mode,
        AgentLogoMode::Hidden
    );
    assert!(runtime.dispatch_json(&theme_set("light")));
    drop(runtime);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    stored.as_object_mut().unwrap().remove("agent_logo_mode");
    std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
    let restarted = runtime_at(&path);
    assert_eq!(
        restarted.snapshot().ui_state.agent_logo_mode,
        AgentLogoMode::Hidden
    );
    assert!(restarted.snapshot().status.diagnostics.is_empty());
}

/// B9, B10, D-07: one event changes the mode, a repeat is no change, and a restart keeps it.
#[test]
fn agent_logo_mode_set_changes_the_mode_and_a_restart_keeps_it() {
    let (_state, path) = state_path("logo-roundtrip");
    let mut runtime = runtime_at(&path);
    assert!(runtime.dispatch_json(&agent_logo_mode_set("shown")));
    assert_eq!(
        runtime.snapshot().ui_state.agent_logo_mode,
        AgentLogoMode::Shown
    );
    assert!(
        !runtime.dispatch_json(&agent_logo_mode_set("shown")),
        "the same mode again is no change"
    );
    assert_eq!(
        serde_json::to_value(runtime.snapshot()).unwrap()["ui_state"]["agent_logo_mode"],
        "shown",
        "the snapshot the shell draws from carries the mode"
    );
    drop(runtime);
    let mut restarted = runtime_at(&path);
    assert_eq!(
        restarted.snapshot().ui_state.agent_logo_mode,
        AgentLogoMode::Shown
    );
    assert!(restarted.dispatch_json(&agent_logo_mode_set("hidden")));
    drop(restarted);
    assert_eq!(
        runtime_at(&path).snapshot().ui_state.agent_logo_mode,
        AgentLogoMode::Hidden
    );
}

/// B20: a stored value this build does not know hides the logos and is only a diagnostic.
#[test]
fn an_unknown_stored_agent_logo_mode_hides_the_logos_with_a_diagnostic() {
    let (_state, path) = state_path("logo-unknown");
    let mut runtime = runtime_at(&path);
    assert!(runtime.dispatch_json(&agent_logo_mode_set("shown")));
    drop(runtime);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    stored["agent_logo_mode"] = serde_json::json!("beside");
    std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();

    let runtime = runtime_at(&path);
    assert_eq!(
        runtime.snapshot().ui_state.agent_logo_mode,
        AgentLogoMode::Hidden
    );
    assert!(
        runtime
            .snapshot()
            .status
            .diagnostics
            .iter()
            .any(|entry| entry.kind == "ui_state.agent_logo_mode_unknown"),
        "the fallback is logged"
    );
    assert!(runtime.snapshot().status.last_error.is_none());
}

/// B20: a mode the event does not name is refused and the mode stays.
#[test]
fn an_unknown_agent_logo_mode_event_is_refused_and_changes_nothing() {
    let mut runtime = runtime();
    assert!(runtime.dispatch_json(&agent_logo_mode_set("shown")));
    runtime.dispatch_json(&agent_logo_mode_set("beside"));
    assert!(
        runtime.snapshot().status.last_error.is_some(),
        "the refusal is recorded, not shown as a change"
    );
    assert_eq!(
        runtime.snapshot().ui_state.agent_logo_mode,
        AgentLogoMode::Shown
    );
}

/// A shared UI-state save, even one carrying a stale mode, does not change the mode.
#[test]
fn a_ui_state_update_never_changes_the_agent_logo_mode() {
    let mut runtime = runtime();
    assert!(runtime.dispatch_json(&agent_logo_mode_set("shown")));
    let update = ui_state_update(
        &runtime,
        serde_json::json!({"font_size": 14, "agent_logo_mode": "hidden"}),
    );
    assert!(runtime.dispatch_json(&update));
    assert_eq!(
        runtime.snapshot().ui_state.agent_logo_mode,
        AgentLogoMode::Shown
    );
}

/// The web's `ui_state_update`: the whole current state with `patch` over it.
pub(super) fn ui_state_update(runtime: &Runtime, patch: serde_json::Value) -> Vec<u8> {
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

/// A hidden device rail survives a restart, an unrelated UI save that leaves the
/// field out keeps it, and a store from before the toggle shows the rail.
#[test]
fn a_hidden_device_rail_survives_a_restart() {
    let (_state, path) = state_path("device-rail");
    let mut runtime = runtime_at(&path);
    assert!(runtime.snapshot().ui_state.device_rail_visible);
    let event = ui_state_update(&runtime, serde_json::json!({"device_rail_visible": false}));
    assert!(runtime.dispatch_json(&event));
    assert!(!runtime.snapshot().ui_state.device_rail_visible);
    let mut unrelated: serde_json::Value = serde_json::from_slice(&ui_state_update(
        &runtime,
        serde_json::json!({"left_sidebar_visible": false}),
    ))
    .unwrap();
    unrelated["payload"]
        .as_object_mut()
        .unwrap()
        .remove("device_rail_visible");
    assert!(runtime.dispatch_json(&serde_json::to_vec(&unrelated).unwrap()));
    assert!(
        !runtime.snapshot().ui_state.device_rail_visible,
        "an event that omits the field keeps the rail hidden"
    );
    drop(runtime);
    assert!(!runtime_at(&path).snapshot().ui_state.device_rail_visible);
}

/// PRD sidebar-typography B11: a dragged width survives a restart, and a store
/// from before the drag opens at the default.
#[test]
fn a_dragged_sidebar_width_survives_a_restart() {
    let (_state, path) = state_path("sidebar-width");
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
    let (_state, path) = state_path("sidebar-width-range");
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
    let (_state, path) = state_path("sidebar-width-stored");
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

fn language_set(language: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "interface_language_set",
        "payload": { "language": language }
    }))
    .unwrap()
}

#[test]
fn language_choices_survive_restart_and_unrelated_stale_saves() {
    let (_state, path) = state_path("interface-language");
    let mut runtime = runtime_at(&path);
    assert_eq!(
        serde_json::to_value(runtime.snapshot()).unwrap()["ui_state"]["interface_language"],
        serde_json::Value::Null
    );
    let stale = ui_state_update(&runtime, serde_json::json!({ "font_size": 15 }));
    for language in ["en", "ko", "zh-CN", "ja"] {
        assert!(runtime.dispatch_json(&language_set(serde_json::json!(language))));
        assert!(!runtime.dispatch_json(&language_set(serde_json::json!(language))));
        runtime.dispatch_json(&stale);
        assert_eq!(
            serde_json::to_value(runtime.snapshot()).unwrap()["ui_state"]["interface_language"],
            language
        );
        drop(runtime);
        runtime = runtime_at(&path);
        assert_eq!(
            serde_json::to_value(runtime.snapshot()).unwrap()["ui_state"]["interface_language"],
            language
        );
    }
    for invalid in [
        serde_json::json!("fr"),
        serde_json::json!(true),
        serde_json::json!({"language":"en"}),
    ] {
        runtime.dispatch_json(&language_set(invalid));
        assert_eq!(
            serde_json::to_value(runtime.snapshot()).unwrap()["ui_state"]["interface_language"],
            "ja"
        );
    }
    let missing = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION, "kind": "interface_language_set", "payload": {}
    }))
    .unwrap();
    runtime.dispatch_json(&missing);
    assert_eq!(
        serde_json::to_value(runtime.snapshot()).unwrap()["ui_state"]["interface_language"],
        "ja"
    );
    assert!(runtime.dispatch_json(&language_set(serde_json::Value::Null)));
    drop(runtime);
    assert_eq!(
        runtime_at(&path).snapshot().ui_state.interface_language,
        None
    );
}

#[test]
fn invalid_stored_languages_publish_english_and_remain_stored() {
    let (_state, path) = state_path("interface-language-invalid");
    let mut runtime = runtime_at(&path);
    runtime.dispatch_json(&language_set(serde_json::json!("ko")));
    drop(runtime);
    for invalid in [
        serde_json::json!("fr"),
        serde_json::json!(42),
        serde_json::json!({"unexpected":true}),
    ] {
        let mut stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        stored["interface_language"] = invalid.clone();
        std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
        let mut runtime = runtime_at(&path);
        assert_eq!(
            serde_json::to_value(runtime.snapshot()).unwrap()["ui_state"]["interface_language"],
            "en"
        );
        assert!(
            runtime
                .snapshot()
                .status
                .diagnostics
                .iter()
                .any(|entry| entry.kind == "ui_state.interface_language_invalid")
        );
        assert!(runtime.snapshot().status.last_error.is_none());
        let update = ui_state_update(
            &runtime,
            serde_json::json!({ "left_sidebar_visible": false }),
        );
        runtime.dispatch_json(&update);
        // A changed appearance event also uses the existing coalesced save.
        runtime.dispatch_json(&theme_set("light"));
        drop(runtime);
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["interface_language"], invalid);
    }
}

#[test]
fn the_factory_reads_the_explicit_language_or_none_for_the_systems() {
    use crate::model::InterfaceLanguage;
    let (_state, path) = state_path("factory-language");
    let mut runtime = runtime_at(&path);
    assert_eq!(runtime.factory_interface_language(), None);
    runtime.dispatch_json(&language_set(serde_json::json!("ko")));
    assert_eq!(
        runtime.factory_interface_language(),
        Some(InterfaceLanguage::Korean)
    );
    runtime.dispatch_json(&language_set(serde_json::Value::Null));
    assert_eq!(runtime.factory_interface_language(), None);
}

#[test]
fn usage_pins_are_bounded_shared_and_survive_restart_and_stale_saves() {
    let (_state, path) = state_path("usage-pins");
    let mut runtime = runtime_at(&path);
    let stale = ui_state_update(&runtime, serde_json::json!({ "font_size": 15 }));
    let event = |kind: &str, payload: serde_json::Value| {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION, "kind": kind, "payload": payload,
        }))
        .unwrap()
    };
    assert_eq!(runtime.snapshot.ui_state.usage.pinned.len(), 2);
    let pin = event(
        "usage_pin_set",
        serde_json::json!({"agent":"claude","window":"five_hour","enabled":true}),
    );
    assert!(runtime.dispatch_json(&pin));
    assert!(
        !runtime.dispatch_json(&pin),
        "same desired value is idempotent"
    );
    let expected = runtime.snapshot.ui_state.usage.clone();
    runtime.dispatch_json(&event(
        "usage_pin_set",
        serde_json::json!({"agent":"codex","window":"five_hour","enabled":true}),
    ));
    assert_eq!(
        runtime.snapshot.ui_state.usage, expected,
        "fourth pin refused by core"
    );
    runtime.dispatch_json(&event(
        "usage_pin_set",
        serde_json::json!({"agent":"pi","window":"weekly","enabled":true}),
    ));
    assert_eq!(
        runtime.snapshot.ui_state.usage, expected,
        "unsupported agent refused"
    );
    runtime.dispatch_json(&event(
        "usage_hidden_set",
        serde_json::json!({"agent":"claude","hidden":true}),
    ));
    let expected = runtime.snapshot.ui_state.usage.clone();
    runtime.dispatch_json(&stale);
    assert_eq!(
        runtime.snapshot.ui_state.usage, expected,
        "stale page cannot overwrite usage choices"
    );
    drop(runtime);
    let mut restarted = runtime_at(&path);
    assert_eq!(restarted.snapshot.ui_state.usage, expected);
    assert_eq!(restarted.usage_activity().hidden, vec!["claude"]);
    restarted.dispatch_json(&event(
        "usage_hidden_set",
        serde_json::json!({"agent":"claude","hidden":false}),
    ));
    assert!(restarted.snapshot.ui_state.usage.hidden.is_empty());
    assert_eq!(
        restarted.snapshot.ui_state.usage.pinned, expected.pinned,
        "Show restores the chosen windows"
    );
}

#[test]
fn an_absent_pin_frees_a_shared_slot_and_stays_removed_after_restart() {
    use crate::usage::model::{
        AgentUsage, Cell, Freshness, LimitReading, Scope, WindowKind, WindowOutcome, WindowReading,
    };
    for outcome in [
        WindowOutcome::NotOffered,
        WindowOutcome::Unreadable {
            cause: crate::usage::model::Cause::FormatChanged,
        },
    ] {
        let (_state, path) = state_path("usage-absent-pin");
        let mut runtime = runtime_at(&path);
        let pin = |agent: &str| {
            serde_json::to_vec(&serde_json::json!({
                "schema_version": SCHEMA_VERSION, "kind":"usage_pin_set",
                "payload":{"agent":agent,"window":"five_hour","enabled":true},
            }))
            .unwrap()
        };
        assert!(runtime.dispatch_json(&pin("codex")));
        let mut rows = AgentUsage::initial_rows();
        let codex = rows.iter_mut().find(|row| row.agent == "codex").unwrap();
        codex.limits = Cell::Reading {
            value: LimitReading {
                windows: vec![WindowReading {
                    kind: WindowKind::FiveHour,
                    scope: Scope::All,
                    outcome,
                }],
            },
            freshness: Freshness::Stale,
            observed_at_ms: 1000,
        };
        runtime.ingest_usage(rows.clone());
        assert_eq!(
            runtime.snapshot.ui_state.usage.pinned.len(),
            3,
            "retained readings preserve the choice"
        );
        if let Cell::Reading { freshness, .. } = &mut rows
            .iter_mut()
            .find(|row| row.agent == "codex")
            .unwrap()
            .limits
        {
            *freshness = Freshness::Fresh;
        }
        runtime.ingest_usage(rows);
        assert_eq!(runtime.snapshot.ui_state.usage.pinned.len(), 2);
        runtime.dispatch_json(&pin("codex"));
        assert_eq!(
            runtime.snapshot.ui_state.usage.pinned.len(),
            2,
            "a late event from another window cannot restore an inaccessible pin"
        );
        assert!(
            runtime.dispatch_json(&pin("claude")),
            "the newly freed slot is selectable"
        );
        let expected = runtime.snapshot.ui_state.usage.clone();
        drop(runtime);
        assert_eq!(runtime_at(&path).snapshot.ui_state.usage, expected);
    }
}
