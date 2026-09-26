use super::*;

use crate::model::ThemePreference;

fn runtime_at(path: &std::path::Path) -> Runtime {
    Runtime::new(
        CoreOptions {
            schema_version: SCHEMA_VERSION,
            herdr_socket_path: Some("/tmp/herdr-core-appearance.sock".to_owned()),
            herdr_bin_path: None,
            app_state_path: path.to_string_lossy().into_owned(),
            host_helper_dir: None,
            host_helper_root: None,
            workspace_views_path: None,
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
