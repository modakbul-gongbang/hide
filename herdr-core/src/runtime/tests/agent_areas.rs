use super::workspace_view::{views_path, with_views};
use super::*;

fn setup() -> (Runtime, String) {
    let (runtime, checkout) = tab_order_runtime("/agent-groups");
    let mut runtime = with_views(runtime, &views_path("agent-groups"));
    ingest(&mut runtime, &["w-order:t1", "w-order:t2", "w-order:t3"]);
    (runtime, checkout)
}
fn ingest(runtime: &mut Runtime, tabs: &[&str]) {
    runtime.ingest_session(Ok(tab_order_payload(
        "/agent-groups",
        tabs,
        tabs,
        "w-order:t1",
    )));
    runtime.sync_workspace_view();
}
fn action(runtime: &mut Runtime, mut payload: serde_json::Value) {
    payload["workspace"] = serde_json::json!({"device_id":"local","path":"/agent-groups"});
    runtime.dispatch_json(&explorer_event("agent_layout", payload));
}
fn layout(runtime: &Runtime) -> crate::agent_layout::Layout {
    runtime
        .workspace_views
        .as_ref()
        .unwrap()
        .views
        .get("local", "/agent-groups")
        .unwrap()
        .agent_layout
        .clone()
}
#[test]
fn agent_split_shows_both_tabs_retains_their_attaches_and_preserves_herdr_order() {
    let (mut runtime, checkout) = setup();
    let order = ordered_tab_ids(&runtime, &checkout);
    action(
        &mut runtime,
        serde_json::json!({"action":"split","tab_id":"w-order:t2","area_id":"a1","edge":"right","request_id":"split-1"}),
    );
    assert_eq!(layout(&runtime).shown(), ["w-order:t1", "w-order:t2"]);
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout).as_deref(),
        Some("w-order:t2")
    );
    assert_eq!(ordered_tab_ids(&runtime, &checkout), order);
    assert!(runtime.recent_visible_tabs.contains(&"w-order:t1".into()));
    assert!(runtime.recent_visible_tabs.contains(&"w-order:t2".into()));
    let before = layout(&runtime);
    action(
        &mut runtime,
        serde_json::json!({"action":"split","tab_id":"w-order:t2","area_id":"a1","edge":"right","request_id":"split-1"}),
    );
    assert_eq!(layout(&runtime), before);
    ingest(
        &mut runtime,
        &["w-order:t3", "w-order:t2", "w-order:t1", "w-order:t4"],
    );
    assert_eq!(layout(&runtime).active(), Some("w-order:t2"));
    assert_eq!(
        layout(&runtime).tree.areas()[0]
            .displays
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        ["w-order:t1", "w-order:t3"]
    );
}
#[test]
fn agent_last_member_departure_collapses_its_area_and_stale_workspace_does_nothing() {
    let (mut runtime, _) = setup();
    action(
        &mut runtime,
        serde_json::json!({"action":"split","tab_id":"w-order:t2","area_id":"a1","edge":"down","request_id":"split-1"}),
    );
    action(
        &mut runtime,
        serde_json::json!({"action":"move","tab_id":"w-order:t2","area_id":"a1","index":0}),
    );
    assert_eq!(layout(&runtime).tree.area_count(), 1);
    let before = layout(&runtime);
    runtime.dispatch_json(&explorer_event("agent_layout", serde_json::json!({"workspace":{"device_id":"local","path":"/other"},"action":"split","tab_id":"w-order:t1","area_id":"a1","edge":"right","request_id":"stale"})));
    assert_eq!(layout(&runtime), before);
}

#[test]
fn requested_new_tab_area_survives_a_focus_change_before_topology_arrives() {
    let (mut runtime, _) = setup();
    action(
        &mut runtime,
        serde_json::json!({"action":"split","tab_id":"w-order:t2","area_id":"a1","edge":"right","request_id":"split-new"}),
    );
    runtime.ingest_local_control_result(
        RemoteControlAction::CreateTab {
            workspace_id: "w-order".into(),
            cwd: "/agent-groups".into(),
            label: "Tab".into(),
            area_id: Some("a1".into()),
        },
        Ok(RemoteControlOutcome::Acknowledged {
            created_tab_id: Some("w-order:t4".into()),
            created_pane_id: Some("w-order:p4".into()),
        }),
        1,
    );
    ingest(
        &mut runtime,
        &["w-order:t1", "w-order:t2", "w-order:t3", "w-order:t4"],
    );
    let arranged = layout(&runtime);
    assert_eq!(
        arranged
            .tree
            .area("a1")
            .unwrap()
            .displays
            .last()
            .unwrap()
            .id,
        "w-order:t4"
    );
    assert_eq!(arranged.tree.active_area, "a1");
    assert!(
        runtime
            .workspace_views
            .as_ref()
            .unwrap()
            .agent_placements
            .is_empty()
    );
}
