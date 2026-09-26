use super::*;

// The macOS app's pane chords come across to the core once (user decision
// 2026-09-26: the desktop app honours the operator's Swift shortcut settings).

fn runtime_at(state: &std::path::Path, swift: &std::path::Path) -> Runtime {
    Runtime::new(
        CoreOptions {
            schema_version: SCHEMA_VERSION,
            herdr_socket_path: Some("/tmp/herdr-core-shortcut-import.sock".to_owned()),
            herdr_bin_path: None,
            app_state_path: state.to_string_lossy().into_owned(),
            host_helper_dir: None,
            host_helper_root: None,
            workspace_views_path: None,
            shortcut_import_path: Some(swift.to_string_lossy().into_owned()),
        },
        environment::EnvironmentReport {
            statuses: Vec::new(),
            chromux_enabled: false,
            herdr_socket_path_override: None,
            home_path: None,
            codex_home: None,
        },
    )
}

/// A private directory holding the core's store and the Swift app's.
fn paths(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "herdr-core-shortcut-import-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("state dir");
    (dir.join("core-state.json"), dir.join("swift-state.json"))
}

/// The Swift app's store: its own fields around the one the core reads.
fn write_swift(path: &std::path::Path, bindings: serde_json::Value) {
    let store = serde_json::json!({
        "schema_version": 1,
        "expanded_paths": ["/Users/operator/project"],
        "selected_path": null,
        "selected_pane_id": null,
        "workspace_registrations": [],
        "device_registrations": [],
        "shortcut_bindings": bindings,
    });
    std::fs::write(path, serde_json::to_vec(&store).unwrap()).expect("swift store");
}

fn bindings(runtime: &Runtime) -> BTreeMap<String, String> {
    runtime.snapshot().ui_state.shortcut_bindings.clone()
}

fn set_bindings(runtime: &mut Runtime, bindings: serde_json::Value) {
    let update = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "ui_state_update",
        "payload": {
            "expanded_paths": [],
            "selected_path": null,
            "selected_pane_id": null,
            "shortcut_bindings": bindings
        }
    }))
    .unwrap();
    assert!(runtime.dispatch_json(&update));
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(command, chord)| ((*command).to_owned(), (*chord).to_owned()))
        .collect()
}

#[test]
fn the_macos_apps_pane_chords_come_across_once_and_a_reset_stays_a_reset() {
    let (state, swift) = paths("once");
    write_swift(
        &swift,
        serde_json::json!({"toggle_zoom": "command+shift+return", "split_down": "command+shift+d"}),
    );
    let runtime = runtime_at(&state, &swift);
    let imported = map(&[
        ("split_down", "command+shift+d"),
        ("toggle_zoom", "command+shift+return"),
    ]);
    assert_eq!(bindings(&runtime), imported);
    drop(runtime);

    // A later change in the macOS app does not come across again.
    write_swift(
        &swift,
        serde_json::json!({"toggle_zoom": "command+option+z"}),
    );
    let mut runtime = runtime_at(&state, &swift);
    assert_eq!(bindings(&runtime), imported);

    // Restoring the defaults here empties the set, and a relaunch keeps it empty.
    set_bindings(&mut runtime, serde_json::json!({}));
    drop(runtime);
    let runtime = runtime_at(&state, &swift);
    assert!(bindings(&runtime).is_empty());
}

#[test]
fn a_set_edited_here_is_never_replaced_by_the_import() {
    let (state, swift) = paths("edited");
    write_swift(&swift, serde_json::json!({}));
    let mut runtime = runtime_at(&state, &swift);
    set_bindings(
        &mut runtime,
        serde_json::json!({"split_right": "command+option+r"}),
    );
    drop(runtime);

    write_swift(
        &swift,
        serde_json::json!({"split_right": "command+shift+r"}),
    );
    let runtime = runtime_at(&state, &swift);
    assert_eq!(
        bindings(&runtime),
        map(&[("split_right", "command+option+r")])
    );
}

#[test]
fn no_macos_store_imports_nothing_and_a_later_launch_still_can() {
    let (state, swift) = paths("missing");
    let runtime = runtime_at(&state, &swift);
    assert!(bindings(&runtime).is_empty());
    assert!(!runtime.snapshot().ui_state.shortcut_bindings_imported);
    drop(runtime);

    write_swift(&swift, serde_json::json!({"close_pane": "command+shift+w"}));
    let runtime = runtime_at(&state, &swift);
    assert_eq!(
        bindings(&runtime),
        map(&[("close_pane", "command+shift+w")])
    );
}

#[test]
fn an_unreadable_macos_store_is_recorded_and_imports_nothing() {
    let (state, swift) = paths("unreadable");
    std::fs::write(&swift, b"{not json").expect("swift store");
    let runtime = runtime_at(&state, &swift);
    assert!(bindings(&runtime).is_empty());
    assert!(
        runtime
            .snapshot()
            .status
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == "ui_state.shortcut_import_failed")
    );
}

#[test]
fn a_ui_state_update_keeps_the_import_marker_and_refuses_an_oversized_set() {
    let (state, swift) = paths("update");
    write_swift(
        &swift,
        serde_json::json!({"toggle_zoom": "command+shift+return"}),
    );
    let mut runtime = runtime_at(&state, &swift);
    set_bindings(
        &mut runtime,
        serde_json::json!({"toggle_zoom": "command+shift+z"}),
    );
    assert!(runtime.snapshot().ui_state.shortcut_bindings_imported);

    let oversized: serde_json::Map<String, serde_json::Value> = (0..40)
        .map(|index| (format!("command_{index}"), serde_json::json!("command+x")))
        .collect();
    set_bindings(&mut runtime, serde_json::Value::Object(oversized));
    assert_eq!(
        bindings(&runtime),
        map(&[("toggle_zoom", "command+shift+z")])
    );
}
