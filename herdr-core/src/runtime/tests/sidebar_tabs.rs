use super::*;
use crate::model::SidebarTabKey;
use crate::persistence::PaneTerminalSizes;

fn disclosure(device: &str, checkout: &str, tab: &str, expanded: bool) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION, "kind": "sidebar_tab_expanded",
        "payload": {"device_id": device, "checkout_id": checkout, "tab_id": tab, "expanded": expanded}
    })).unwrap()
}

#[test]
fn sidebar_disclosure_is_admitted_idempotent_and_never_moves_focus() {
    let (mut runtime, checkout, directory) = strip_checkout("sidebar-disclosure");
    let path = directory.to_string_lossy();
    runtime.ingest_session(Ok(tab_order_payload(
        &path,
        &["w-order:t1", "w-order:t2"],
        &["w-order:t1", "w-order:t2"],
        "w-order:t1",
    )));
    let focus = runtime.snapshot.focused.clone();
    let fold = disclosure(crate::node::TEST_NODE, &checkout, "w-order:t1", false);
    assert!(runtime.dispatch_json(&fold));
    assert!(!runtime.dispatch_json(&fold));
    assert_eq!(runtime.snapshot.ui_state.sidebar_collapsed_tabs.len(), 1);
    assert_eq!(runtime.snapshot.focused, focus);
    for (device, owner, tab) in [
        ("missing", checkout.as_str(), "w-order:t1"),
        (crate::node::TEST_NODE, "wrong", "w-order:t1"),
        (crate::node::TEST_NODE, checkout.as_str(), "gone"),
    ] {
        assert!(!runtime.dispatch_json(&disclosure(device, owner, tab, true)));
    }
    assert_eq!(runtime.snapshot.ui_state.sidebar_collapsed_tabs.len(), 1);
    assert!(runtime.dispatch_json(&disclosure(
        crate::node::TEST_NODE,
        &checkout,
        "w-order:t1",
        true
    )));
    assert!(runtime.snapshot.ui_state.sidebar_collapsed_tabs.is_empty());
}

#[test]
fn sidebar_disclosure_round_trips_and_retires_only_on_authoritative_tab_removal() {
    let (mut runtime, checkout, directory) = strip_checkout("sidebar-restore");
    let path = directory.to_string_lossy();
    runtime.ingest_session(Ok(tab_order_payload(
        &path,
        &["w-order:t1", "w-order:t2"],
        &["w-order:t1", "w-order:t2"],
        "w-order:t1",
    )));
    runtime.dispatch_json(&disclosure(
        crate::node::TEST_NODE,
        &checkout,
        "w-order:t2",
        false,
    ));
    let remote = SidebarTabKey {
        device_id: "offline-device".into(),
        checkout_id: "remote-checkout".into(),
        tab_id: "w-order:t2".into(),
    };
    runtime
        .snapshot
        .ui_state
        .sidebar_collapsed_tabs
        .push(remote.clone());
    let stored = directory.join("ui.json");
    crate::persistence::save(
        &stored,
        &runtime.snapshot.ui_state,
        &PaneTerminalSizes::new(),
    )
    .unwrap();
    let (restored, _, _) = crate::persistence::load(&stored);
    assert_eq!(
        restored.sidebar_collapsed_tabs,
        runtime.snapshot.ui_state.sidebar_collapsed_tabs
    );
    runtime.ingest_session(Err(SessionFetchError::Stale(
        "temporarily unavailable".into(),
    )));
    assert_eq!(runtime.snapshot.ui_state.sidebar_collapsed_tabs.len(), 2);
    runtime.ingest_session(Ok(tab_order_payload(
        &path,
        &["w-order:t1"],
        &["w-order:t1"],
        "w-order:t1",
    )));
    assert_eq!(
        runtime.snapshot.ui_state.sidebar_collapsed_tabs,
        vec![remote]
    );
}

#[test]
fn sidebar_metadata_separates_manual_names_from_automatic_labels() {
    let (mut runtime, checkout, directory) = strip_checkout("sidebar-names");
    let path = directory.to_string_lossy();
    for (raw, expected) in [
        ("자동 작업 제목", Some("자동 작업 제목")),
        ("", None),
        ("2", None),
        ("Tab 2", None),
        ("Tabby", Some("Tabby")),
    ] {
        let mut payload = tab_order_payload(&path, &["w-order:t2"], &["w-order:t2"], "w-order:t2");
        payload.tabs[0].label = raw.into();
        runtime.ingest_session(Ok(payload));
        let tab = runtime
            .snapshot
            .navigator
            .workspaces
            .iter()
            .flat_map(|space| &space.checkouts)
            .find(|row| row.id == checkout)
            .unwrap()
            .tabs
            .first()
            .unwrap();
        let wire = serde_json::to_value(tab).unwrap();
        assert_eq!(wire["sidebar"]["number"], 2);
        assert_eq!(
            wire["sidebar"]["manual_name"],
            serde_json::to_value(expected).unwrap()
        );
    }
}
