use super::*;
use crate::fake_herdr::FakeHerdr;

fn context() -> ClosedContext {
    ClosedContext {
        workspace_id: "w1".into(),
        workspace_label: "primary".into(),
        workspace_ids_before_close: vec!["w1".into(), "w2".into()],
        tab_ids_before_close: vec!["w1:t1".into()],
        pane_ids_before_close: vec!["w1:p1".into()],
        checkout_id: "checkout".into(),
        checkout_path: "/tmp".into(),
        tab_id: "w1:t1".into(),
        tab_label: "old".into(),
        tab_index: 0,
        agent_area: Some(("a1".into(), 0)),
        replacement_shell: true,
    }
}
fn tab(id: &str) -> Value {
    json!({"tab_id":id,"workspace_id":"w1","number":1,"label":"shell","focused":false,"pane_count":1,"agent_status":"idle"})
}
fn snapshot(created: bool) -> Value {
    let mut tabs = vec![tab("w1:t1")];
    if created {
        tabs.push(tab("w1:t2"));
    }
    json!({"type":"session_snapshot","snapshot":{
        "version":"fixture","protocol":HERDR_PROTOCOL_REVISION,
        "workspaces":[{"workspace_id":"w1","number":1,"label":"primary","focused":false,"pane_count":tabs.len(),"tab_count":tabs.len(),"active_tab_id":"w1:t1","agent_status":"idle"}],
        "tabs":tabs,"panes":[],"layouts":[],"agents":[]
    }})
}

#[test]
fn replacement_creation_failure_sends_no_close() {
    let herdr = FakeHerdr::start_with_errors("replacement-failure", |method, _| match method {
        "session.snapshot" => Ok(snapshot(false)),
        "tab.create" => Err(("fixture_create_refused".into(), "private diagnostic".into())),
        other => panic!("unexpected effect {other}"),
    });
    let error =
        prepare_close_replacement(&herdr.connector(), "intent", &context(), true).unwrap_err();
    assert!(error.contains("fixture_create_refused"));
    assert_eq!(herdr.methods(), ["session.snapshot", "tab.create"]);
}

#[test]
fn replacement_retry_after_close_refusal_adopts_its_shell_without_another_create() {
    let mut created = false;
    let herdr = FakeHerdr::start_with_errors(
        "replacement-retry",
        move |method, params| match method {
            "session.snapshot" => Ok(snapshot(created)),
            "tab.create" => {
                assert!(!created);
                assert_eq!(params["cwd"], "/tmp");
                assert_eq!(params["focus"], false);
                assert_eq!(params["env"][REOPEN_INTENT_ENV], "intent:close-replacement");
                created = true;
                Ok(json!({"type":"tab_created","tab":tab("w1:t2"),"root_pane":{
                    "pane_id":"w1:p2","terminal_id":"terminal","workspace_id":"w1","tab_id":"w1:t2","focused":false,"agent_status":"idle","revision":0
                }}))
            }
            "layout.export" => Ok(json!({"type":"layout_export","layout":{
                "workspace_id":"w1","tab_id":"w1:t2","zoomed":false,"focused_pane_id":"w1:p2",
                "root":{"type":"pane","pane_id":"w1:p2","cwd":"/tmp","env":{(REOPEN_INTENT_ENV):"intent:close-replacement"}}
            }})),
            "tab.close" => Err(("fixture_close_refused".into(), "private diagnostic".into())),
            other => panic!("unexpected effect {other}"),
        },
    );
    let connector = herdr.connector();
    assert_eq!(
        prepare_close_replacement(&connector, "intent", &context(), true)
            .unwrap()
            .0,
        "w1:t2"
    );
    let close = CloseEffectRequest {
        allow_replacement_create: true,
        key: "intent".into(),
        connection_generation: 0,
        target: CloseCaptureTarget::Tab {
            tab_id: "w1:t1".into(),
        },
        replacement: Some(context()),
    };
    assert!(run_close_effect(&connector, &close).is_err());
    assert_eq!(
        prepare_close_replacement(&connector, "intent", &context(), false)
            .unwrap()
            .0,
        "w1:t2"
    );
    assert_eq!(
        herdr
            .methods()
            .iter()
            .filter(|method| method.as_str() == "tab.create")
            .count(),
        1
    );
    assert_eq!(
        herdr
            .methods()
            .iter()
            .filter(|method| method.as_str() == "tab.close")
            .count(),
        1
    );
}

/// Herdr names a new pane's folder from its process, which is the server's
/// own until the shell has started: the snapshot the close ingests must still
/// place the replacement at the checkout it was created for.
#[test]
fn replacement_shell_is_placed_at_its_checkout_while_herdr_still_names_the_servers_folder() {
    let mut created = false;
    let herdr = FakeHerdr::start("replacement-early-cwd", move |method, _| match method {
        "session.snapshot" => {
            let mut value = snapshot(created);
            if created {
                let body = &mut value["snapshot"];
                body["panes"] = json!([
                    {"workspace_id":"w1","tab_id":"w1:t1","pane_id":"w1:p1","terminal_id":"t1","focused":false,"revision":0,"agent_status":"idle","cwd":"/tmp"},
                    {"workspace_id":"w1","tab_id":"w1:t2","pane_id":"w1:p2","terminal_id":"t2","focused":false,"revision":0,"agent_status":"idle","cwd":"/srv/herdr-server"}
                ]);
                body["layouts"] = json!([{
                    "workspace_id":"w1","tab_id":"w1:t2","zoomed":false,
                    "area":{"x":0,"y":0,"width":120,"height":60},"focused_pane_id":"w1:p2",
                    "panes":[{"pane_id":"w1:p2","focused":false,"rect":{"x":0,"y":0,"width":120,"height":60}}],
                    "splits":[]
                }]);
            }
            value
        }
        "tab.create" => {
            created = true;
            json!({"type":"tab_created","tab":tab("w1:t2"),"root_pane":{
                "pane_id":"w1:p2","terminal_id":"t2","workspace_id":"w1","tab_id":"w1:t2","focused":false,"agent_status":"idle","revision":0
            }})
        }
        "layout.export" => json!({"type":"layout_export","layout":{
            "workspace_id":"w1","tab_id":"w1:t1","zoomed":false,"focused_pane_id":"w1:p1",
            "root":{"type":"pane","pane_id":"w1:p1","cwd":"/tmp","env":{}}
        }}),
        other => panic!("unexpected {other}"),
    });
    let (tab_id, payload) =
        prepare_close_replacement(&herdr.connector(), "intent", &context(), true).unwrap();
    assert_eq!(tab_id, "w1:t2");
    let cwd = |pane: &str| {
        payload
            .panes
            .iter()
            .find(|candidate| candidate.pane_id == pane)
            .and_then(|candidate| candidate.cwd.as_deref())
    };
    assert_eq!(cwd("w1:p2"), Some("/tmp"));
    assert_eq!(cwd("w1:p1"), Some("/tmp"));
}

#[test]
fn reuse_only_retry_cannot_create_when_its_shell_disappears_before_worker_reads() {
    let herdr = FakeHerdr::start("replacement-reuse-only", |method, _| match method {
        "session.snapshot" => snapshot(false),
        other => panic!("reuse-only retry must not send {other}"),
    });
    let error =
        prepare_close_replacement(&herdr.connector(), "intent", &context(), false).unwrap_err();
    assert!(error.contains("disappeared"));
    assert_eq!(herdr.methods(), ["session.snapshot"]);
}

#[test]
fn malformed_create_ack_is_recovered_by_its_marker_without_repeating_the_effect() {
    let mut marker = String::new();
    let herdr = FakeHerdr::start("create-recover", move |method, params| match method {
        "tab.create" => {
            assert!(marker.is_empty(), "creation must not be repeated");
            marker = params["env"][REOPEN_INTENT_ENV]
                .as_str()
                .unwrap()
                .to_owned();
            json!({"type":"tab_created","tab":tab(""),"root_pane":{
                "pane_id":"w1:p2","terminal_id":"terminal","workspace_id":"w1","tab_id":"w1:t2","focused":false,"agent_status":"idle","revision":0
            }})
        }
        "session.snapshot" => snapshot(true),
        "layout.export" => {
            let id = params["tab_id"].as_str().unwrap();
            json!({"type":"layout_export","layout":{
                "workspace_id":"w1","tab_id":id,"zoomed":false,"focused_pane_id":"w1:p2",
                "root":{"type":"pane","pane_id":"w1:p2","cwd":"/tmp","env":{
                    (REOPEN_INTENT_ENV): if id == "w1:t2" {marker.clone()} else {"other-intent".into()}
                }}
            }})
        }
        other => panic!("unexpected {other}"),
    });
    let outcome = execute_local_control(
        &herdr.connector(),
        &RemoteControlAction::CreateTab {
            workspace_id: "w1".into(),
            cwd: "/tmp".into(),
            label: "New".into(),
            area_id: Some("a1".into()),
            admission_id: Some(91),
        },
    )
    .unwrap();
    assert!(
        matches!(outcome, RemoteControlOutcome::Acknowledged { created_tab_id: Some(id), .. } if id == "w1:t2")
    );
    assert_eq!(
        herdr.methods(),
        [
            "tab.create",
            "session.snapshot",
            "layout.export",
            "layout.export"
        ]
    );
}
