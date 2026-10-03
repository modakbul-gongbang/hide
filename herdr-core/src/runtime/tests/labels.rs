//! The labels the local coordinator published reach every local projection
//! the runtime ingests (PRD labels-in-hided D-04), not only the
//! coordinator's own: a workspace creation or an agent close hands the
//! runtime a fresh Herdr projection of its own.

use super::*;

use crate::labels::overlay::LabelOverlay;
use crate::labels::store::PaneRecord;

const CHECKOUT: &str = "/private/tmp/hide-labels-overlay";
const TABS: [&str; 1] = ["w-order:t1"];
const PANE: &str = "w-order:t1:p";
const SESSION: &str = "11111111-2222-3333-4444-555555555555";

/// A fresh projection with one idle Claude agent on `session`, carrying no
/// label, as Herdr itself reports it.
fn projection(session: &str) -> SessionSnapshotPayload {
    let mut payload = tab_order_payload(CHECKOUT, &TABS, &TABS, "w-order:t1");
    let agent: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({"agents": [{
        "id": "worker", "pane_id": PANE, "agent": "claude",
        "agent_status": "idle", "state_change_seq": 4, "cwd": CHECKOUT,
        "agent_session": {"kind": "id", "value": session}
    }]}))
    .unwrap();
    payload.agents.extend(agent.agents);
    payload
}

fn row(runtime: &Runtime) -> serde_json::Value {
    serde_json::to_value(&runtime.snapshot().navigator.agents)
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["pane_id"] == PANE)
        .cloned()
        .expect("the agent row")
}

#[test]
fn a_projection_from_another_path_keeps_the_published_label_of_its_session() {
    let mut runtime = runtime();
    let record = PaneRecord {
        owner: hide_session::label_reference_token("claude", "id", SESSION),
        task: Some("파서 버그 수정 작업".to_owned()),
        changed_unix_ms: 1_000,
        ..PaneRecord::default()
    };
    let records = [(PANE.to_owned(), record)];
    runtime.set_label_overlay(LabelOverlay::of_records(
        records.iter().map(|(pane, record)| (pane, record)),
        true,
    ));

    // A workspace creation's own projection: no coordinator publish between.
    runtime.ingest_session(Ok(projection(SESSION)));
    assert_eq!(row(&runtime)["identity_label"], "파서 버그 수정 작업");

    // The pane now runs another session: the published label is not its own.
    runtime.ingest_session(Ok(projection("99999999-2222-3333-4444-555555555555")));
    assert_eq!(row(&runtime)["identity_label"], "Claude");
}

/// PRD overview-request-view D-19: a typed Enter and a phone reply to an
/// agent pane are kept as the operator's submits; an Enter at an approval
/// prompt, a newline and other keys are not.
#[test]
fn an_operator_submit_is_recorded_for_the_agent_pane_and_an_approval_enter_is_not() {
    let mut runtime = runtime();
    let services = std::sync::Arc::new(
        crate::labels::LabelServices::start(None, None, std::sync::Weak::new()).unwrap(),
    );
    runtime.install_label_services(std::sync::Arc::clone(&services));
    runtime.ingest_session(Ok(projection(SESSION)));
    let key = |bytes: &[u8]| {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION, "kind": "key",
            "payload": {"pane_id": PANE, "bytes_base64": crate::live::encode_base64(bytes)},
        }))
        .unwrap()
    };
    let submits = || {
        services
            .input
            .submits(crate::labels::store::LOCAL_TARGET, PANE)
    };
    runtime.dispatch_json(&key(b"hello"));
    runtime.dispatch_json(&key(b"\x1b\r"));
    assert!(submits().is_empty());
    runtime.dispatch_json(&key(b"\r"));
    assert_eq!(submits().len(), 1);
    assert!(!submits()[0].while_working);

    assert!(
        !runtime.dispatch_json(
            &serde_json::to_vec(&serde_json::json!({
                "schema_version": SCHEMA_VERSION, "kind": "pane_input_submitted",
                "payload": {"pane_id": PANE},
            }))
            .unwrap()
        )
    );
    assert_eq!(submits().len(), 2);

    let mut blocked = projection(SESSION);
    let agent = blocked
        .agents
        .iter_mut()
        .find(|agent| agent.pane_id.as_deref() == Some(PANE))
        .unwrap();
    agent.agent_status = Some("blocked".to_owned());
    agent.state_change_seq = Some(5);
    runtime.ingest_session(Ok(blocked));
    runtime.dispatch_json(&key(b"\r"));
    assert_eq!(submits().len(), 2, "an approval's Enter is not a request");
}
