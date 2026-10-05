//! Where a pane's cwd is a birth value (the folder its shell is still in
//! before it enters the start folder, which Herdr never announces settling),
//! what each reader of it shows. #420 gave a tab Hide created its checkout by
//! creation intent; these are the other readers of the same value.

use super::*;

fn git(directory: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .current_dir(directory)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

/// The first workspace holds a tab in `checkout_path`; the second is a
/// created worktree's, whose one tab's pane reports `birth_cwd`.
fn payload_with_created_workspace(checkout_path: &str, birth_cwd: &str) -> SessionSnapshotPayload {
    let rows = [
        ("w-order", "w-order:t1", checkout_path),
        ("w-wt", "w-wt:t1", birth_cwd),
    ];
    let workspaces = rows.iter().map(|(workspace, tab, _)| {
        serde_json::json!({"workspace_id": workspace, "label": workspace, "active_tab_id": tab})
    });
    let tabs = rows.iter().map(|(workspace, tab, _)| {
        serde_json::json!({"workspace_id": workspace, "tab_id": tab, "label": "", "number": 1})
    });
    let panes = rows
        .iter()
        .map(|(_, tab, cwd)| serde_json::json!({"pane_id": format!("{tab}:p"), "cwd": cwd}));
    let layouts = rows.iter().map(|(workspace, tab, _)| {
        serde_json::json!({
            "workspace_id": workspace,
            "tab_id": tab,
            "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 80, "height": 24},
            "focused_pane_id": format!("{tab}:p"),
            "panes": [{
                "pane_id": format!("{tab}:p"),
                "rect": {"x": 0, "y": 0, "width": 80, "height": 24}
            }],
            "splits": []
        })
    });
    crate::sidebar::owned_label_fixture(serde_json::json!({
        "agents": [],
        "focused_workspace_id": "w-order",
        "focused_pane_id": "w-order:t1:p",
        "workspaces": workspaces.collect::<Vec<_>>(),
        "tabs": tabs.collect::<Vec<_>>(),
        "panes": panes.collect::<Vec<_>>(),
        "layouts": layouts.collect::<Vec<_>>()
    }))
    .expect("session payload")
}

fn checkout_path_of(runtime: &Runtime, tab_id: &str) -> Option<String> {
    runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .find(|checkout| {
            checkout
                .tabs
                .iter()
                .any(|tab| tab.id.as_deref() == Some(tab_id))
        })
        .map(|checkout| checkout.path.clone())
}

/// A worktree created from Hide opens with a first tab whose pane still
/// reports the birth cwd. The tab belongs to the new worktree's row, which the
/// catalog lists only after git has read it, whichever of the session and
/// the catalog arrives first.
#[test]
fn a_created_worktrees_first_tab_joins_its_row_while_its_pane_reports_the_birth_cwd() {
    let (mut runtime, _checkout_id, directory) = strip_checkout("birth-worktree");
    git(
        &directory,
        &["commit", "--allow-empty", "-q", "-m", "fixture"],
    );
    let linked = directory.parent().expect("a parent folder").join(format!(
        "{}-topic",
        directory.file_name().unwrap().to_string_lossy()
    ));
    git(
        &directory,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "topic",
            &linked.to_string_lossy(),
        ],
    );
    let linked = linked.canonicalize().expect("the linked worktree exists");
    let linked_path = linked.to_string_lossy().into_owned();
    let birth_cwd = directory
        .parent()
        .expect("a parent folder")
        .to_string_lossy()
        .into_owned();
    let payload = payload_with_created_workspace(&directory.to_string_lossy(), &birth_cwd);

    // Herdr answered the creation, and the session carries the first tab with the cwd its pane was born with.
    let id = runtime
        .begin_task_operation(
            "worktree_create",
            Some(directory.to_string_lossy().into_owned()),
            Some("topic".to_owned()),
            Some("main".to_owned()),
            None,
        )
        .expect("creation operation");
    assert!(runtime.ingest_task_operation_result(
        id,
        Ok(live::WorktreeTaskOutcome {
            path: linked_path.clone(),
            pane_id: "w-wt:t1:p".to_owned(),
            created_tab_id: Some("w-wt:t1".to_owned()),
            purpose_error: None,
            unconfirmed_purpose_token: None,
            issue_error: None,
        })
    ));
    runtime.ingest_session(Ok(payload));

    assert_eq!(
        checkout_path_of(&runtime, "w-wt:t1").as_deref(),
        Some(linked_path.as_str()),
        "the first tab is under the worktree it was created for"
    );
}

fn acknowledge_created_tab(runtime: &mut Runtime, checkout_path: &str, tab_id: &str) {
    runtime.ingest_local_control_result(
        RemoteControlAction::CreateTab {
            workspace_id: "w-order".to_owned(),
            cwd: checkout_path.to_owned(),
            label: "new".to_owned(),
            area_id: None,
            admission_id: None,
        },
        Ok(RemoteControlOutcome::Acknowledged {
            created_tab_id: Some(tab_id.to_owned()),
            created_pane_id: Some(format!("{tab_id}:p")),
        }),
        1,
    );
}

/// A session where `w-order:t2`'s pane reports `cwd` and the first tab sits in
/// the checkout.
fn two_tab_payload(checkout_path: &str, cwd: &str) -> SessionSnapshotPayload {
    let tabs = ["w-order:t1", "w-order:t2"];
    let mut payload = tab_order_payload(checkout_path, &tabs, &tabs, "w-order:t1");
    for pane in &mut payload.panes {
        if pane.pane_id == "w-order:t2:p" {
            pane.cwd = Some(cwd.to_owned());
        }
    }
    payload
}

/// The catalog is built from the cwds the session reports. A created tab's
/// birth cwd in the parent folder made a project row for that folder, with no
/// tabs under it, until the next publish after the shell had settled.
#[test]
fn a_created_tabs_birth_cwd_makes_no_row_for_the_parent_folder() {
    let (mut runtime, _checkout_id, directory) = strip_checkout("birth-rows");
    let checkout_path = directory.to_string_lossy().into_owned();
    let birth_cwd = directory.parent().unwrap().to_string_lossy().into_owned();
    acknowledge_created_tab(&mut runtime, &checkout_path, "w-order:t2");

    runtime.ingest_session(Ok(two_tab_payload(&checkout_path, &birth_cwd)));

    let paths = runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| {
            std::iter::once(workspace.path.clone()).chain(
                workspace
                    .checkouts
                    .iter()
                    .map(|checkout| checkout.path.clone()),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        !paths.iter().any(|path| path == &birth_cwd),
        "the parent folder got a row: {paths:?}"
    );
    assert_eq!(
        runtime.last_session_spaces[0].cwds,
        std::slice::from_ref(&checkout_path),
        "the purpose mirror reads the folder the tab was created for"
    );
}

/// A listener under the parent folder belongs to a sibling project. The
/// birth cwd reaches the pane's own cwd, which attributes ports and servers by
/// prefix, so a created tab claimed every sibling's listener.
#[test]
fn a_created_tab_with_a_birth_cwd_claims_no_listener_of_a_sibling_folder() {
    let (mut runtime, _checkout_id, directory) = strip_checkout("birth-ports");
    let checkout_path = directory.to_string_lossy().into_owned();
    let parent = directory.parent().unwrap().to_string_lossy().into_owned();
    runtime.ingest_listening_ports(crate::model::ListeningPortsSnapshot {
        entries: vec![crate::model::ListeningPortSnapshot {
            host: "127.0.0.1".to_owned(),
            port: 4321,
            cwd: format!("{parent}/a-sibling-project"),
        }],
        unavailable_reason: None,
    });
    acknowledge_created_tab(&mut runtime, &checkout_path, "w-order:t2");

    runtime.ingest_session(Ok(two_tab_payload(&checkout_path, &parent)));

    let pane = runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .flat_map(|checkout| checkout.tabs.iter())
        .flat_map(|tab| tab.panes.iter())
        .find(|pane| pane.id == "w-order:t2:p")
        .expect("the created tab's pane")
        .clone();
    assert_eq!(pane.cwd, checkout_path);
    assert!(pane.ports.is_empty(), "claimed: {:?}", pane.ports);
    assert!(pane.servers.is_empty());
}

/// Once a pane has reported a cwd inside its checkout the value is where the
/// shell is: a later `cd` out of the checkout is read as it is.
#[test]
fn a_settled_created_tab_reads_its_cwd_as_reported() {
    let (mut runtime, _checkout_id, directory) = strip_checkout("birth-settled");
    let checkout_path = directory.to_string_lossy().into_owned();
    let parent = directory.parent().unwrap().to_string_lossy().into_owned();
    acknowledge_created_tab(&mut runtime, &checkout_path, "w-order:t2");

    runtime.ingest_session(Ok(two_tab_payload(&checkout_path, &parent)));
    runtime.ingest_session(Ok(two_tab_payload(&checkout_path, &checkout_path)));
    runtime.ingest_session(Ok(two_tab_payload(&checkout_path, &parent)));

    let cwd = runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .flat_map(|checkout| checkout.tabs.iter())
        .flat_map(|tab| tab.panes.iter())
        .find(|pane| pane.id == "w-order:t2:p")
        .map(|pane| pane.cwd.clone());
    assert_eq!(cwd.as_deref(), Some(parent.as_str()));
}

/// A pane that never reports its checkout within the birth window is believed:
/// the clamp ends instead of hiding the shell's real folder for the life of
/// the tab, and a record whose tab never arrives costs nothing.
#[test]
fn a_created_tab_whose_pane_never_reports_its_checkout_is_believed_after_the_window() {
    let (mut runtime, _checkout_id, directory) = strip_checkout("birth-window");
    let checkout_path = directory.to_string_lossy().into_owned();
    let parent = directory.parent().unwrap().to_string_lossy().into_owned();
    acknowledge_created_tab(&mut runtime, &checkout_path, "w-order:t2");
    let cwd_of_t2 = |runtime: &Runtime| {
        runtime
            .snapshot()
            .navigator
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .flat_map(|checkout| checkout.tabs.iter())
            .flat_map(|tab| tab.panes.iter())
            .find(|pane| pane.id == "w-order:t2:p")
            .map(|pane| pane.cwd.clone())
    };

    runtime.ingest_session(Ok(two_tab_payload(&checkout_path, &parent)));
    assert_eq!(cwd_of_t2(&runtime).as_deref(), Some(checkout_path.as_str()));

    // The clock is the record's own stamp, which the test moves back.
    runtime
        .created_tab_checkouts
        .get_mut("w-order:t2")
        .expect("the record")
        .recorded_at_unix_ms = 0;
    runtime.ingest_session(Ok(two_tab_payload(&checkout_path, &parent)));
    assert_eq!(cwd_of_t2(&runtime).as_deref(), Some(parent.as_str()));
}
