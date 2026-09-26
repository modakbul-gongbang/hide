use super::*;
use std::time::Duration;

use crate::fake_herdr::FakeHerdr;

#[path = "tests/agents_settings_remote.rs"]
mod agents_settings_remote;
#[path = "tests/appearance.rs"]
mod appearance;
#[path = "tests/device_catalog.rs"]
mod device_catalog;
#[path = "tests/device_worktrees.rs"]
mod device_worktrees;
#[path = "tests/devices.rs"]
mod devices;
#[path = "tests/documents.rs"]
mod documents;
#[path = "tests/editor_preview.rs"]
mod editor_preview;
#[path = "tests/editor_reopen.rs"]
mod editor_reopen;
#[path = "tests/issues.rs"]
mod issues;
#[path = "tests/lineage.rs"]
mod lineage;
#[path = "tests/memory.rs"]
mod memory;
#[path = "tests/project_sessions.rs"]
mod project_sessions;
#[path = "tests/projects.rs"]
mod projects;
#[path = "tests/session_navigation.rs"]
mod session_navigation;
#[path = "tests/shortcut_import.rs"]
mod shortcut_import;
#[path = "tests/snapshot_delta.rs"]
mod snapshot_delta;
#[path = "tests/terminal.rs"]
mod terminal;
#[path = "tests/view_areas.rs"]
mod view_areas;
#[path = "tests/workspace_view.rs"]
mod workspace_view;

/// The `tab_list` Herdr answers a `tab.move` with, carrying the tabs in
/// their new order. Only the order is read here; the other fields are
/// what the pinned schema requires of a tab.
fn tab_list(ids: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "type": "tab_list",
        "tabs": ids.iter().enumerate().map(|(index, tab_id)| serde_json::json!({
            "tab_id": tab_id,
            "workspace_id": tab_id.split(':').next().unwrap_or_default(),
            "number": index + 1,
            "label": "fixture",
            "focused": false,
            "pane_count": 1,
            "agent_status": "idle"
        })).collect::<Vec<_>>()
    })
}

fn no_worktrees() -> crate::model::WorktreeCatalogSnapshot {
    crate::model::WorktreeCatalogSnapshot::default()
}

/// A settled worktree with nothing in the way, ready for the Remove
/// button's rules to be applied to it.
fn settled_worktree(badge: crate::model::PullRequestBadge) -> CheckoutSnapshot {
    let pull_request = crate::model::PullRequestSnapshot {
        closing_issues: Default::default(),
        title: "Fixture pull request".into(),
        checks: crate::model::PullRequestChecks::Unknown,
        number: 7,
        head_branch: "feature".to_owned(),
        base_branch: "main".to_owned(),
        url: "https://example.invalid/pull/7".to_owned(),
        badge,
        review: None,
        is_draft: false,
        merged_at_unix_ms: None,
        updated_at_unix_ms: None,
    };
    CheckoutSnapshot {
        id: "checkout-feature".to_owned(),
        workspace_id: "workspace-1".to_owned(),
        label: "feature".to_owned(),
        path: "/tmp/hide/feature".to_owned(),
        branch: Some("feature".to_owned()),
        is_worktree: true,
        exists: true,
        pull_request: Some(pull_request.clone()),
        worktree: Some(crate::model::WorktreeSnapshot {
            path: "/tmp/hide/feature".to_owned(),
            branch: Some("feature".to_owned()),
            pull_request: Some(pull_request),
            deletion_gate: crate::model::WorktreeDeletionGateSnapshot {
                button_label: "Delete worktree…".to_owned(),
                can_delete_branch: true,
                ..crate::model::WorktreeDeletionGateSnapshot::default()
            },
            ..crate::model::WorktreeSnapshot::default()
        }),
        ..CheckoutSnapshot::default()
    }
}

fn card_for(
    runtime: &mut Runtime,
    checkout: CheckoutSnapshot,
) -> crate::model::CheckoutCardSnapshot {
    let checkout_id = checkout.id.clone();
    runtime.snapshot.navigator.workspaces = vec![workspace(
        "workspace-1",
        "hide",
        "/tmp/hide",
        vec![checkout],
    )];
    runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id);
    runtime.refresh_card();
    runtime.snapshot.card.clone()
}

use crate::live::SessionFetchError;
use crate::model::{
    CheckoutSnapshot, DeviceSnapshot, PaneLayoutNodeSnapshot, PaneLayoutSnapshot, PaneSnapshot,
    RemoteSessionSnapshot, RemoteStatusSnapshot, TabSnapshot, TerminalPaneSnapshot,
    WorkspaceRegistration, WorkspaceSnapshot,
};
use crate::model::{MAX_PANE_TEXT_SCALE, MIN_PANE_TEXT_SCALE};
use crate::sidebar::SessionSnapshotPayload;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_RUNTIME_STATE_ID: AtomicU64 = AtomicU64::new(0);

fn assert_owner_conflict_observes_and_reconnects(owner_conflict: &str) {
    let mut runtime = runtime();
    runtime.suppress_terminal_session_workers = true;
    runtime.snapshot.navigator.workspaces = vec![workspace(
        "w1",
        "Fixture",
        "/tmp/hide-terminal-session-runtime",
        vec![checkout(
            "w1",
            "checkout-1",
            "/tmp/hide-terminal-session-runtime",
            Some(pane("w1:p1", "/tmp/hide-terminal-session-runtime")),
        )],
    )];
    runtime.snapshot.terminal.panes = vec![TerminalPaneSnapshot {
        pane_id: "w1:p1".to_owned(),
        closed: false,
        ..TerminalPaneSnapshot::default()
    }];
    runtime.next_terminal_session_generation = 40;
    // The pane is one the operator is looking at, so its view has already
    // reported a size; an attach is held back until one has.
    runtime.terminal_sizes.insert("w1:p1".to_owned(), (40, 120));
    runtime
        .terminal_session_generations
        .insert("w1:p1".to_owned(), 40);
    runtime.terminal_session_lifecycles.insert(
        "w1:p1".to_owned(),
        TerminalSessionLifecycle {
            state: "controlling",
            generation: 40,
            attempt: 1,
            mode: Some(TerminalSessionMode::Control),
            retry_decision: "none",
            ..TerminalSessionLifecycle::default()
        },
    );
    runtime.terminal_sessions.insert(
        "w1:p1".to_owned(),
        TerminalSession::test_stub("w1:p1", 40, TerminalSessionMode::Control),
    );

    assert!(runtime.ingest_terminal_session_closed(
        "w1:p1",
        40,
        TerminalSessionMode::Control,
        Some(owner_conflict.to_owned()),
    ));
    let observing = runtime
        .terminal_session_lifecycles
        .get("w1:p1")
        .expect("observer lifecycle");
    assert_eq!(observing.state, "observing");
    assert_eq!(observing.mode, Some(TerminalSessionMode::Observe));
    assert_eq!(observing.generation, 41);
    assert_eq!(observing.attempt, 1);
    assert_eq!(runtime.terminal_sessions.len(), 1);
    assert_eq!(
        runtime.terminal_sessions["w1:p1"].mode,
        TerminalSessionMode::Observe
    );
    assert!(
        !runtime.snapshot.terminal.panes[0].closed,
        "transport owner conflict must not close the authoritative pane"
    );

    let chunk_count = runtime.snapshot.terminal.chunks.len();
    assert_eq!(
        runtime.ingest_terminal_session_frame(
            "w1:p1",
            40,
            TerminalSessionMode::Control,
            b"stale-generation",
            crate::model::TerminalFrame {
                width: 80,
                height: 24,
                full: true
            },
        ),
        None
    );
    assert_eq!(
        runtime.ingest_terminal_session_frame(
            "w1:p1",
            41,
            TerminalSessionMode::Control,
            b"stale-mode",
            crate::model::TerminalFrame {
                width: 80,
                height: 24,
                full: true
            },
        ),
        None
    );
    assert!(!runtime.ingest_terminal_session_closed(
        "w1:p1",
        40,
        TerminalSessionMode::Control,
        Some(owner_conflict.to_owned()),
    ));
    assert_eq!(runtime.snapshot.terminal.chunks.len(), chunk_count);
    assert_eq!(runtime.next_terminal_session_generation, 41);

    let reconnect = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "reconnect_pane",
        "payload": {"pane_id": "w1:p1"}
    }))
    .expect("reconnect event");
    assert!(runtime.dispatch_json(&reconnect));
    let controlling = runtime
        .terminal_session_lifecycles
        .get("w1:p1")
        .expect("controller lifecycle");
    assert_eq!(controlling.state, "controlling");
    assert_eq!(controlling.mode, Some(TerminalSessionMode::Control));
    assert_eq!(controlling.generation, 42);
    assert_eq!(controlling.attempt, 2);
    assert_eq!(runtime.terminal_sessions.len(), 1);

    runtime.request_terminal_control("w1:p1");
    assert_eq!(runtime.next_terminal_session_generation, 42);
    assert_eq!(
        runtime
            .terminal_session_lifecycles
            .get("w1:p1")
            .expect("same controller")
            .attempt,
        2
    );
}

fn context_payload() -> SessionSnapshotPayload {
    serde_json::from_value(serde_json::json!({
        "agents": [
            {"pane_id":"w1:p1", "agent":"codex", "agent_status":"working", "state_change_seq":10,
             "cwd":"/tmp/hide-context-alpha", "tokens":{"activity":"1788871000000"}},
            {"pane_id":"w2:p1", "agent":"codex", "agent_status":"working", "state_change_seq":20,
             "cwd":"/tmp/hide-context-zeta", "tokens":{"activity":"1788872000000"},
             "agent_session":{"kind":"id", "value":"context-session"}, "spawned_from_pane_id":"w1:p1"}
        ],
        "workspaces":[{"workspace_id":"w1","label":"Alpha"},{"workspace_id":"w2","label":"Zeta"}],
        "tabs":[{"workspace_id":"w1","tab_id":"w1:t1","label":"1"},{"workspace_id":"w2","tab_id":"w2:t1","label":"1"}],
        "panes":[{"pane_id":"w1:p1","cwd":"/tmp/hide-context-alpha"},{"pane_id":"w2:p1","cwd":"/tmp/hide-context-zeta"}],
        "layouts":[
            {"workspace_id":"w1","tab_id":"w1:t1","zoomed":false,"area":{"x":0,"y":0,"width":80,"height":24},
             "focused_pane_id":"w1:p1","panes":[{"pane_id":"w1:p1","rect":{"x":0,"y":0,"width":80,"height":24}}],"splits":[]},
            {"workspace_id":"w2","tab_id":"w2:t1","zoomed":false,"area":{"x":0,"y":0,"width":80,"height":24},
             "focused_pane_id":"w2:p1","panes":[{"pane_id":"w2:p1","rect":{"x":0,"y":0,"width":80,"height":24}}],"splits":[]}
        ]
    })).unwrap()
}

fn runtime() -> Runtime {
    let state_id = NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed);
    let options = CoreOptions {
        schema_version: SCHEMA_VERSION,
        herdr_socket_path: Some("/tmp/herdr-core-pet-runtime.sock".to_owned()),
        herdr_bin_path: None,
        app_state_path: std::env::temp_dir()
            .join(format!(
                "herdr-core-pet-runtime-{}-{}.json",
                std::process::id(),
                state_id
            ))
            .to_string_lossy()
            .into_owned(),
        host_helper_dir: None,
        host_helper_root: None,
        workspace_views_path: None,
        shortcut_import_path: None,
    };
    Runtime::new(
        options,
        environment::EnvironmentReport {
            statuses: Vec::new(),
            herdr_socket_path_override: None,
            home_path: None,
            codex_home: None,
        },
    )
}

/// A runtime with a live context pointed at a socket that does not exist.
///
/// Pane focus needs a live connection to be dispatched at all, and the
/// worker it spawns fails on its own without touching this runtime, so a
/// test can drive the real focus event rather than a shortcut into the
/// read record.
fn live_runtime() -> Runtime {
    let mut runtime = runtime();
    let socket_path = std::env::temp_dir()
        .join(format!(
            "herdr-core-read-record-{}-{}.sock",
            std::process::id(),
            NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
        ))
        .to_string_lossy()
        .into_owned();
    runtime.live = Some(live::LiveContext {
        socket_path: socket_path.clone().into(),
        herdr_bin: None,
        runtime: std::sync::Weak::new(),
        notifier: crate::ffi::ChangeNotifier::noop(),
        api_connector: Arc::new(hide_herdr_client::UnixSocketConnector::new(&socket_path)),
    });
    runtime
}

/// The event the shell sends when the operator clicks an agent row, a
/// pane, or picks one from the switcher.
fn operator_focus_event(pane_id: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "focus_pane",
        "payload": {"pane_id": pane_id, "origin": "operator"}
    }))
    .expect("focus pane event")
}

/// The event the shell sends once on launch to put the terminal back on
/// the pane the last session ended on.
fn restore_focus_event(pane_id: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "focus_pane",
        "payload": {"pane_id": pane_id, "origin": "restore"}
    }))
    .expect("restore focus event")
}

/// The panes the sidebar still shows as unread, in snapshot order.
fn unread_panes(runtime: &Runtime) -> Vec<String> {
    let mut panes = runtime
        .snapshot
        .navigator
        .agents
        .iter()
        .filter(|agent| agent.unread)
        .map(|agent| agent.pane_id.clone())
        .collect::<Vec<_>>();
    panes.sort();
    panes
}

/// Three finished panes side by side in one tab, the shape the operator
/// reported. `focused` is the pane Herdr names as that tab's focus, which
/// is a tab-scoped verdict Hide's read axis must not follow.
fn finished_tab_payload(panes: &[(&str, u64)], focused: &str) -> SessionSnapshotPayload {
    let agents = panes
        .iter()
        .map(|(pane_id, seq)| {
            serde_json::json!({
                "pane_id": pane_id,
                "workspace_label": "Fixture",
                "agent": "codex",
                "agent_status": "done",
                "state_change_seq": seq,
                "tokens": {"status_done_new": "\u{25cf}", "activity": "0000000000001"}
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(panes.len(), 3, "the fixture is three panes side by side");
    let layout_panes = panes
        .iter()
        .enumerate()
        .map(|(index, (pane_id, _))| {
            serde_json::json!({
                "pane_id": pane_id,
                "rect": {"x": index * 30, "y": 0, "width": 30, "height": 24}
            })
        })
        .collect::<Vec<_>>();
    serde_json::from_value(serde_json::json!({
        "agents": agents,
        "tabs": [{"workspace_id": "w1", "tab_id": "t1", "label": ""}],
        "layouts": [{
            "workspace_id": "w1", "tab_id": "t1", "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 90, "height": 24},
            "focused_pane_id": focused,
            "panes": layout_panes,
            "splits": [
                {"direction": "right", "ratio": 0.333_333_34,
                 "rect": {"x": 0, "y": 0, "width": 90, "height": 24}},
                {"direction": "right", "ratio": 0.5,
                 "rect": {"x": 30, "y": 0, "width": 60, "height": 24}}
            ]
        }]
    }))
    .expect("session payload")
}

fn working_payload() -> SessionSnapshotPayload {
    serde_json::from_value(serde_json::json!({
        "agents": [{
            "pane_id": "w1:p1",
            "workspace_label": "Fixture",
            "agent": "codex",
            "agent_status": "working",
            "tokens": {"status_working": "\u{25cf}", "activity": "0000000000001"}
        }],
        "tabs": [{"workspace_id": "w1", "tab_id": "t1", "label": ""}],
        "layouts": [{
            "workspace_id": "w1", "tab_id": "t1", "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 80, "height": 24},
            "focused_pane_id": "w1:p1",
            "panes": [{"pane_id": "w1:p1",
                       "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
            "splits": []
        }]
    }))
    .expect("session payload")
}

fn pane(id: &str, cwd: &str) -> PaneSnapshot {
    PaneSnapshot {
        id: id.to_owned(),
        herdr_label: None,
        terminal_title: None,
        workspace_label: None,
        cwd: cwd.to_owned(),
        status_label: "Attached".to_owned(),
        requires_close_confirmation: false,
        requires_close_status_check: false,
        identity_label: None,
        activity_at_unix_ms: None,
        fork: PaneForkSnapshot::default(),
        ports: Vec::new(),
        children: None,
        lineage_path: Vec::new(),
    }
}

fn tab(workspace_id: &str, checkout_id: &str, pane: Option<PaneSnapshot>) -> TabSnapshot {
    TabSnapshot {
        id: Some(format!("{checkout_id}:tab")),
        workspace_id: Some(workspace_id.to_owned()),
        checkout_id: Some(checkout_id.to_owned()),
        label: Some("Session".to_owned()),
        empty: pane.is_none(),
        delegated: false,
        panes: pane.into_iter().collect(),
    }
}

fn checkout(
    workspace_id: &str,
    checkout_id: &str,
    path: &str,
    pane: Option<PaneSnapshot>,
) -> CheckoutSnapshot {
    CheckoutSnapshot {
        next_tab_label: crate::model::next_tab_label(std::iter::empty()),
        id: checkout_id.to_owned(),
        workspace_id: workspace_id.to_owned(),
        label: checkout_id.to_owned(),
        path: path.to_owned(),
        branch: None,
        is_worktree: false,
        exists: true,
        temporary: false,
        has_panes: pane.is_some(),
        tabs: pane
            .clone()
            .map(|pane| vec![tab(workspace_id, checkout_id, Some(pane))])
            .unwrap_or_default(),
        active_tab_id: pane.map(|_| format!("{checkout_id}:tab")),
        strip: Vec::new(),
        ..CheckoutSnapshot::default()
    }
}

fn workspace(
    id: &str,
    label: &str,
    path: &str,
    checkouts: Vec<CheckoutSnapshot>,
) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        home_issues: Default::default(),
        id: id.to_owned(),
        label: label.to_owned(),
        path: path.to_owned(),
        remote_target_id: None,
        expanded: true,
        device_id: "local".to_owned(),
        repo_name: label.to_owned(),
        is_git: false,
        default_branch: None,
        branches: Vec::new(),
        registered: true,
        temporary: false,
        session_workspace_ids: Vec::new(),
        last_activity_unix_ms: None,
        checkouts,
        pinned: false,
        inactive_checkouts: Default::default(),
        removal: Default::default(),
        disk: Default::default(),
    }
}

/// One Herdr workspace whose tabs each hold one pane inside
/// `checkout_path`. `tab_order` is the order Herdr reports its tabs in and
/// `layout_order` is the order the layouts arrive in, so a test can hand
/// the two orders apart and see which one the navigator follows.
fn tab_order_payload(
    checkout_path: &str,
    tab_order: &[&str],
    layout_order: &[&str],
    active_tab_id: &str,
) -> SessionSnapshotPayload {
    let tabs = tab_order
        .iter()
        .map(|tab_id| {
            serde_json::json!({
                "workspace_id": "w-order", "tab_id": tab_id, "label": ""
            })
        })
        .collect::<Vec<_>>();
    let panes = layout_order
        .iter()
        .map(|tab_id| serde_json::json!({"pane_id": format!("{tab_id}:p"), "cwd": checkout_path}))
        .collect::<Vec<_>>();
    let layouts = layout_order
        .iter()
        .map(|tab_id| {
            serde_json::json!({
                "workspace_id": "w-order",
                "tab_id": tab_id,
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": format!("{tab_id}:p"),
                "panes": [{
                    "pane_id": format!("{tab_id}:p"),
                    "rect": {"x": 0, "y": 0, "width": 80, "height": 24}
                }],
                "splits": []
            })
        })
        .collect::<Vec<_>>();
    // Herdr's keyboard is in the workspace, on the active tab's pane,
    // which is what a live `session.snapshot` reports.
    serde_json::from_value(serde_json::json!({
        "agents": [],
        "focused_workspace_id": "w-order",
        "focused_pane_id": format!("{active_tab_id}:p"),
        "workspaces": [{
            "workspace_id": "w-order",
            "label": "order",
            "active_tab_id": active_tab_id
        }],
        "tabs": tabs,
        "panes": panes,
        "layouts": layouts
    }))
    .expect("ordered session payload")
}

/// Two or more Herdr workspaces whose tabs all sit in `checkout_path`, so
/// the path-keyed navigator folds them into one checkout. Each entry is
/// `(workspace_id, tab_ids, active_tab_id)`; `focused_workspace_id` is
/// the one holding Herdr's keyboard, on its active tab's pane. Panes are
/// named `<tab>:p`.
fn split_checkout_payload(
    checkout_path: &str,
    workspaces: &[(&str, &[&str], &str)],
    focused_workspace_id: &str,
) -> SessionSnapshotPayload {
    let mut sessions = Vec::new();
    let mut tabs = Vec::new();
    let mut panes = Vec::new();
    let mut layouts = Vec::new();
    for (workspace_id, tab_ids, active_tab_id) in workspaces {
        sessions.push(serde_json::json!({
            "workspace_id": workspace_id,
            "label": workspace_id,
            "active_tab_id": active_tab_id
        }));
        for tab_id in tab_ids.iter() {
            tabs.push(serde_json::json!({
                "workspace_id": workspace_id, "tab_id": tab_id, "label": ""
            }));
            panes.push(serde_json::json!({
                "pane_id": format!("{tab_id}:p"), "cwd": checkout_path
            }));
            layouts.push(serde_json::json!({
                "workspace_id": workspace_id,
                "tab_id": tab_id,
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": format!("{tab_id}:p"),
                "panes": [{
                    "pane_id": format!("{tab_id}:p"),
                    "rect": {"x": 0, "y": 0, "width": 80, "height": 24}
                }],
                "splits": []
            }));
        }
    }
    let focused_pane_id = workspaces
        .iter()
        .find(|(workspace_id, _, _)| *workspace_id == focused_workspace_id)
        .map(|(_, _, active_tab_id)| format!("{active_tab_id}:p"))
        .expect("the focused workspace is one of the listed workspaces");
    serde_json::from_value(serde_json::json!({
        "agents": [],
        "focused_workspace_id": focused_workspace_id,
        "focused_pane_id": focused_pane_id,
        "workspaces": sessions,
        "tabs": tabs,
        "panes": panes,
        "layouts": layouts
    }))
    .expect("split checkout session payload")
}

/// A runtime with one registered checkout focused, ready to ingest
/// [`tab_order_payload`]. Returns the checkout id the navigator gave it.
fn tab_order_runtime(checkout_path: &str) -> (Runtime, String) {
    let mut runtime = runtime();
    runtime.snapshot.ui_state.workspace_registrations = vec![WorkspaceRegistration {
        id: "workspace:order".to_owned(),
        label: "order".to_owned(),
        path: checkout_path.to_owned(),
        device_id: "local".to_owned(),
        pinned: false,
    }];
    runtime.rebuild_catalog();
    let checkout_id = workspace::checkout_id_for_path("workspace:order", Path::new(checkout_path));
    runtime.snapshot.navigator.focused_workspace_id = Some("workspace:order".to_owned());
    runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
    // A selection the operator made, so the first live session keeps it
    // instead of retiring it with the restore hint.
    runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id.clone());
    runtime.snapshot.navigator.root_path = Some(checkout_path.to_owned());
    runtime.reset_terminal_projection(None);
    (runtime, checkout_id)
}

fn ordered_tab_ids(runtime: &Runtime, checkout_id: &str) -> Vec<String> {
    runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .find(|checkout| checkout.id == checkout_id)
        .expect("the registered checkout")
        .tabs
        .iter()
        .map(|tab| tab.id.clone().expect("a Herdr tab always has an id"))
        .collect()
}

/// The panes the terminal projection is holding open, in snapshot order.
fn projected_pane_ids(runtime: &Runtime) -> Vec<String> {
    runtime
        .snapshot()
        .terminal
        .panes
        .iter()
        .map(|pane| pane.pane_id.clone())
        .collect()
}

fn checkout_active_tab_id(runtime: &Runtime, checkout_id: &str) -> Option<String> {
    runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .find(|checkout| checkout.id == checkout_id)
        .expect("the registered checkout")
        .active_tab_id
        .clone()
}

/// A [`tab_order_runtime`] with a live Herdr context, so a view-state
/// notification actually leaves and a wait is armed.
fn live_tab_order_runtime(checkout_path: &str) -> (Runtime, String) {
    let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
    let socket_path = std::env::temp_dir()
        .join(format!(
            "herdr-core-view-authority-{}-{}.sock",
            std::process::id(),
            NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
        ))
        .to_string_lossy()
        .into_owned();
    runtime.live = Some(live::LiveContext {
        socket_path: socket_path.clone().into(),
        herdr_bin: None,
        runtime: std::sync::Weak::new(),
        notifier: crate::ffi::ChangeNotifier::noop(),
        api_connector: Arc::new(hide_herdr_client::UnixSocketConnector::new(&socket_path)),
    });
    (runtime, checkout_id)
}

fn focus_tab_event(checkout_id: &str, tab_id: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "focus_tab",
        "payload": {
            "workspace_id": "workspace:order",
            "checkout_id": checkout_id,
            "tab_id": tab_id
        }
    }))
    .expect("focus tab event")
}

fn correlated_pane_focus_event(pane_id: &str, request_id: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "focus_pane",
        "payload": {
            "pane_id": pane_id,
            "origin": "operator",
            "request_id": request_id
        }
    }))
    .expect("correlated pane focus event")
}

fn diagnostic_count(runtime: &Runtime, kind: &str) -> usize {
    runtime
        .snapshot()
        .status
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == kind)
        .count()
}

fn strip_ids(runtime: &Runtime, checkout_id: &str) -> Vec<String> {
    runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .find(|checkout| checkout.id == checkout_id)
        .expect("the registered checkout")
        .strip
        .iter()
        .map(|entry| entry.id.clone())
        .collect()
}

fn strip_labels(runtime: &Runtime, checkout_id: &str) -> Vec<String> {
    runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .find(|checkout| checkout.id == checkout_id)
        .expect("the registered checkout")
        .strip
        .iter()
        .map(|entry| entry.label.clone())
        .collect()
}

/// A registered checkout the strip tests can drive.
///
/// The temp root is a symlink on macOS and the catalog keys checkouts by
/// the real path, so the fixture uses that. It is also made its own
/// repository, because a directory inside another repository is
/// catalogued under that repository's root instead.
fn strip_checkout(name: &str) -> (Runtime, String, PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "hide-strip-{name}-{}-{}",
        std::process::id(),
        NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).expect("checkout directory");
    let directory = directory.canonicalize().expect("a real checkout path");
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&directory)
            .status()
            .expect("git init runs")
            .success()
    );
    std::fs::write(directory.join("notes.md"), "notes\n").expect("fixture file");
    let (runtime, checkout_id) = tab_order_runtime(&directory.to_string_lossy());
    (runtime, checkout_id, directory)
}

fn reorder_tab(runtime: &mut Runtime, checkout_id: &str, entry_id: &str, to_index: usize) -> bool {
    let event = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "reorder_tab",
        "payload": {
            "workspace_id": "workspace:order",
            "checkout_id": checkout_id,
            "tab_id": entry_id,
            "to_index": to_index
        }
    }))
    .expect("reorder tab event");
    runtime.dispatch_json(&event)
}

fn open_file(runtime: &mut Runtime, checkout_id: &str, path: &Path) {
    let event = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "file_open",
        "payload": {
            "path": path.to_string_lossy(),
            "workspace_id": "workspace:order",
            "checkout_id": checkout_id
        }
    }))
    .expect("file open event");
    assert!(runtime.dispatch_json(&event));
}

/// A repository with one linked worktree, both holding panes of the same
/// Herdr workspace, which is how a workspace comes to span two checkouts.
/// Returns the runtime, the repository's checkout id, and both directories.
fn split_workspace_checkouts(name: &str) -> (Runtime, String, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "hide-split-{name}-{}-{}",
        std::process::id(),
        NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).expect("fixture root");
    let root = root.canonicalize().expect("a real fixture path");
    let repository = root.join("repo");
    std::fs::create_dir_all(&repository).expect("repository directory");
    let git = |arguments: &[&str], directory: &Path| {
        assert!(
            std::process::Command::new("git")
                // The fixture owns its identity, and it must not reach for
                // the operator's signing key: a commit the fixture makes
                // failed whenever the signing agent was not answering,
                // which made this suite fail for a reason that had nothing
                // to do with the code under test.
                .args(["-c", "commit.gpgsign=false"])
                .args(arguments)
                .current_dir(directory)
                .env("GIT_AUTHOR_NAME", "fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .status()
                .expect("git runs")
                .success(),
            "git {arguments:?}"
        );
    };
    git(&["init", "-q", "-b", "main"], &repository);
    std::fs::write(repository.join("notes.md"), "notes\n").expect("fixture file");
    git(&["add", "notes.md"], &repository);
    git(&["commit", "-qm", "notes"], &repository);
    // A linked worktree is a second checkout of the same project, so the
    // catalog gives it its own row under one project.
    let worktree = root.join("feature");
    git(
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            &worktree.to_string_lossy(),
        ],
        &repository,
    );
    let (runtime, checkout_id) = tab_order_runtime(&repository.to_string_lossy());
    (runtime, checkout_id, repository, worktree)
}

/// A session payload for one Herdr workspace whose tabs are split across
/// two directories, in Herdr's own order.
fn split_workspace_payload(
    tab_order: &[(&str, &str)],
    active_tab_id: &str,
) -> SessionSnapshotPayload {
    let tabs = tab_order
        .iter()
        .map(|(tab_id, _)| {
            serde_json::json!({"workspace_id": "w-order", "tab_id": tab_id, "label": ""})
        })
        .collect::<Vec<_>>();
    let panes = tab_order
        .iter()
        .map(|(tab_id, cwd)| serde_json::json!({"pane_id": format!("{tab_id}:p"), "cwd": cwd}))
        .collect::<Vec<_>>();
    let layouts = tab_order
        .iter()
        .map(|(tab_id, _)| {
            serde_json::json!({
                "workspace_id": "w-order",
                "tab_id": tab_id,
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": format!("{tab_id}:p"),
                "panes": [{
                    "pane_id": format!("{tab_id}:p"),
                    "rect": {"x": 0, "y": 0, "width": 80, "height": 24}
                }],
                "splits": []
            })
        })
        .collect::<Vec<_>>();
    serde_json::from_value(serde_json::json!({
        "agents": [],
        "workspaces": [{
            "workspace_id": "w-order",
            "label": "order",
            "active_tab_id": active_tab_id
        }],
        "tabs": tabs,
        "panes": panes,
        "layouts": layouts
    }))
    .expect("split session payload")
}

/// Reads one view struct's body out of the shell's SwiftUI source.
///
/// The two surfaces that make up the window's first row live in one file,
/// so a whole-file scan would answer for views this rule does not reach.
fn shell_view_body(source: &str, declaration: &str) -> String {
    let start = source
        .find(declaration)
        .unwrap_or_else(|| panic!("the shell no longer declares {declaration}"));
    let rest = &source[start..];
    // Every view in this file closes at column zero, so the first such
    // brace after the declaration ends the struct.
    let end = rest
        .find("\n}\n")
        .unwrap_or_else(|| panic!("{declaration} has no closing brace"));
    rest[..end].to_owned()
}

/// Two registered checkouts with the first focused, so a reveal into the
/// second has a checkout switch to make. The second holds
/// `deep/nested/leaf/target.txt`, which is the shape SC1 and SC2 describe.
fn reveal_runtime() -> (Runtime, PathBuf, String, PathBuf, String) {
    let mut roots = Vec::new();
    for name in ["one", "two"] {
        let directory = std::env::temp_dir().join(format!(
            "hide-reveal-{name}-{}-{}",
            std::process::id(),
            NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(directory.join("deep/nested/leaf")).expect("fixture tree");
        let directory = directory.canonicalize().expect("a real checkout path");
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(&directory)
                .status()
                .expect("git init runs")
                .success()
        );
        std::fs::write(directory.join("deep/nested/leaf/target.txt"), "found\n")
            .expect("fixture file");
        roots.push(directory);
    }
    let mut runtime = runtime();
    runtime.snapshot.ui_state.workspace_registrations = roots
        .iter()
        .enumerate()
        .map(|(index, path)| WorkspaceRegistration {
            id: format!("workspace:{index}"),
            label: format!("workspace {index}"),
            path: path.to_string_lossy().into_owned(),
            device_id: "local".to_owned(),
            pinned: false,
        })
        .collect();
    runtime.rebuild_catalog();
    let ids = roots
        .iter()
        .enumerate()
        .map(|(index, path)| workspace::checkout_id_for_path(&format!("workspace:{index}"), path))
        .collect::<Vec<_>>();
    runtime.snapshot.navigator.focused_workspace_id = Some("workspace:0".to_owned());
    runtime.snapshot.navigator.focused_checkout_id = Some(ids[0].clone());
    runtime.snapshot.ui_state.focused_checkout_id = Some(ids[0].clone());
    runtime.snapshot.navigator.root_path = Some(roots[0].to_string_lossy().into_owned());
    // The panel starts hidden on a section that is not the tree, which is
    // the state SC1 names: the reveal has to open it and switch it.
    runtime.snapshot.ui_state.right_panel_visible = false;
    runtime.snapshot.ui_state.right_panel_section = RightPanelSection::Changes;
    let (first, second) = (roots[0].clone(), roots[1].clone());
    (runtime, first, ids[0].clone(), second, ids[1].clone())
}

fn reveal_event(workspace_id: &str, checkout_id: &str, path: &Path, is_directory: bool) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "reveal_path",
        "payload": {
            "path": path.to_string_lossy(),
            "workspace_id": workspace_id,
            "checkout_id": checkout_id,
            "is_directory": is_directory
        }
    }))
    .expect("reveal event")
}

fn explorer_runtime() -> (Runtime, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "hide-explorer-runtime-{}-{}",
        std::process::id(),
        NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(root.join("src/nested")).expect("fixture tree");
    std::fs::write(root.join("src/lib.rs"), "lib\n").expect("fixture file");
    let root = root.canonicalize().expect("a real root");
    let mut runtime = runtime();
    focus_local_checkout(&mut runtime, &root);
    (runtime, root)
}

/// Registers `root` as this machine's checkout and puts it in front, the
/// way a click on it in the sidebar leaves the navigator.
fn focus_local_checkout(runtime: &mut Runtime, root: &Path) {
    runtime.snapshot.ui_state.workspace_registrations = vec![WorkspaceRegistration {
        id: "workspace:0".to_owned(),
        label: "workspace 0".to_owned(),
        path: root.to_string_lossy().into_owned(),
        device_id: "local".to_owned(),
        pinned: false,
    }];
    runtime.rebuild_catalog();
    let checkout_id = workspace::checkout_id_for_path("workspace:0", root);
    runtime.snapshot.navigator.focused_workspace_id = Some("workspace:0".to_owned());
    runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
    runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id);
    runtime.snapshot.navigator.root_path = Some(root.to_string_lossy().into_owned());
}

fn explorer_event(kind: &str, payload: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": kind,
        "payload": payload
    }))
    .expect("explorer event")
}

fn closed_file(key: &str, path: &str) -> ClosedItem {
    ClosedItem::File {
        key: key.to_owned(),
        device_id: workspace::LOCAL_DEVICE_ID.to_owned(),
        workspace_id: "workspace:0".to_owned(),
        checkout_id: "checkout:0".to_owned(),
        checkout_path: "/repo".to_owned(),
        path: path.to_owned(),
        label: Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path)
            .to_owned(),
    }
}

fn close_capture_request(key: &str) -> live::CloseCaptureRequest {
    live::CloseCaptureRequest {
        key: key.to_owned(),
        connection_generation: 0,
        context: ClosedContext {
            workspace_id: "workspace:0".to_owned(),
            workspace_label: "Fixture".to_owned(),
            workspace_ids_before_close: vec!["workspace:0".to_owned()],
            tab_ids_before_close: vec![format!("tab:{key}")],
            pane_ids_before_close: vec![],
            checkout_id: "checkout:0".to_owned(),
            checkout_path: "/repo".to_owned(),
            tab_id: format!("tab:{key}"),
            tab_label: key.to_owned(),
            tab_index: 0,
        },
        panes: vec![],
        target: live::CloseCaptureTarget::Tab {
            tab_id: format!("tab:{key}"),
        },
    }
}
