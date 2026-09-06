//! The only conversion boundary between the pinned Herdr schema and the replica.
//! Generated values are consumed into domain inputs; no JSON round trip converts
//! a generated payload back into a hand-written deserialization shape.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::herdr_api::{HERDR_PROTOCOL_REVISION, HostScope};
use crate::herdr_contract::wire::{
    error_response as err, event as ev, request as req, success_response as res,
};
use crate::live::SessionFetchError;
use crate::model::PaneLayoutDirection;
use crate::session_sync::{
    PaneMove, ProjectedAgent, ProjectedPane, ProjectedTab, ProjectedWorkspace, ProjectedWorktree,
    ProjectionState, ReplicaEnvelope, ReplicaEvent, SubscriptionLine,
};
use crate::sidebar::{
    SessionAgentSessionPayload, SessionLayoutPanePayload, SessionLayoutPayload, SessionLayoutRect,
    SessionLayoutSplitPayload,
};

// Observer decision: the pinned schema declares event/data but the server adds
// these three sequencing fields. Keep only that contract gap hand-written here.
// The schema-gap test below forces deletion when Herdr declares the metadata.
#[derive(Deserialize)]
struct EventMetadata {
    protocol: u64,
    host: res::HostScope,
    sequence: u64,
    #[serde(flatten)]
    payload: ev::EventEnvelope,
}

pub(crate) fn protocol_mismatch(received: u64) -> SessionFetchError {
    SessionFetchError::Protocol(format!(
        "The running Herdr speaks protocol {received}; this hide needs protocol {HERDR_PROTOCOL_REVISION}. \
         Stop it with `herdr server stop` and reopen hide so it starts its bundled Herdr, \
         or update hide to a release built against that Herdr."
    ))
}

fn malformed(message: impl Into<String>) -> SessionFetchError {
    SessionFetchError::Malformed(message.into())
}

// Preflight preserves the existing diagnostic priority and exact messages.
// It does not supply defaults: a successful preflight still has to deserialize
// the complete generated snapshot, including fields the projection discards.
fn validate_snapshot(value: &Value) -> Result<(), SessionFetchError> {
    let protocol = value
        .get("protocol")
        .and_then(Value::as_u64)
        .ok_or_else(|| malformed("snapshot is missing protocol"))?;
    if protocol != HERDR_PROTOCOL_REVISION {
        return Err(protocol_mismatch(protocol));
    }
    if !value
        .get("version")
        .and_then(Value::as_str)
        .is_some_and(|v| !v.trim().is_empty())
    {
        return Err(malformed("snapshot is missing version"));
    }
    for field in [
        "workspaces",
        "tabs",
        "panes",
        "layouts",
        "agents",
        "lineage",
    ] {
        if !value.get(field).is_some_and(Value::is_array) {
            return Err(malformed(format!("snapshot is missing {field}")));
        }
    }
    let host = value
        .get("host")
        .ok_or_else(|| malformed("snapshot is missing host"))?;
    let host: res::HostScope = serde_json::from_value(host.clone())
        .map_err(|e| malformed(format!("snapshot host is malformed: {e}")))?;
    if host.host_id.trim().is_empty() || host.session_id.trim().is_empty() {
        return Err(malformed("snapshot host contains an empty identifier"));
    }
    if value
        .get("event_sequence")
        .and_then(Value::as_u64)
        .is_none()
    {
        return Err(malformed("snapshot is missing event_sequence"));
    }
    Ok(())
}

pub(crate) fn snapshot(
    value: Value,
) -> Result<(HostScope, u64, ProjectionState), SessionFetchError> {
    validate_snapshot(&value)?;
    let snapshot: res::SessionSnapshot = serde_json::from_value(value)
        .map_err(|e| malformed(format!("snapshot projection is malformed: {e}")))?;
    Ok(convert_snapshot(snapshot))
}

pub(crate) fn snapshot_response(
    value: Value,
) -> Result<(HostScope, u64, ProjectionState), SessionFetchError> {
    validate_snapshot(
        value
            .get("snapshot")
            .ok_or_else(|| malformed("response is missing snapshot"))?,
    )?;
    let response: res::ResponseResult = serde_json::from_value(value)
        .map_err(|e| malformed(format!("snapshot projection is malformed: {e}")))?;
    match response {
        res::ResponseResult::SessionSnapshot { snapshot } => Ok(convert_snapshot(snapshot)),
        _ => Err(malformed("response is missing snapshot")),
    }
}

fn convert_snapshot(snapshot: res::SessionSnapshot) -> (HostScope, u64, ProjectionState) {
    (
        HostScope {
            host_id: snapshot.host.host_id,
            session_id: snapshot.host.session_id,
        },
        snapshot.event_sequence,
        ProjectionState {
            focused_pane_id: snapshot.focused_pane_id,
            focused_workspace_id: snapshot.focused_workspace_id,
            workspaces: snapshot.workspaces.into_iter().map(Into::into).collect(),
            tabs: snapshot.tabs.into_iter().map(Into::into).collect(),
            panes: snapshot.panes.into_iter().map(Into::into).collect(),
            layouts: snapshot.layouts.into_iter().map(Into::into).collect(),
            agents: snapshot.agents.into_iter().map(Into::into).collect(),
        },
    )
}

pub(crate) fn agents_response(value: Value) -> Result<Vec<ProjectedAgent>, SessionFetchError> {
    if let Some(kind) = value.get("type").and_then(Value::as_str) {
        if kind != "agent_list" {
            return Err(malformed(format!(
                "agent.list returned unexpected result type {kind:?}"
            )));
        }
    }
    let response: res::ResponseResult = serde_json::from_value(value)
        .map_err(|e| malformed(format!("agent.list response is malformed: {e}")))?;
    match response {
        res::ResponseResult::AgentList { agents } => {
            Ok(agents.into_iter().map(Into::into).collect())
        }
        _ => unreachable!("the result discriminator was checked before deserialization"),
    }
}

pub(crate) fn subscription_params(
    after_sequence: u64,
    subscriptions: &[&str],
) -> Result<Value, String> {
    let subscriptions = subscriptions
        .iter()
        .map(|kind| serde_json::from_value::<req::Subscription>(json!({"type": kind})))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("invalid event subscription: {e}"))?;
    serde_json::to_value(req::EventsSubscribeParams {
        after_sequence,
        subscriptions,
    })
    .map_err(|e| format!("subscription parameters could not be encoded: {e}"))
}

pub(crate) fn agent_activity(agent: &ProjectedAgent) -> Option<&str> {
    agent.tokens.get("activity").and_then(Value::as_str)
}

pub(crate) fn parse_subscription_line(line: &str) -> Result<SubscriptionLine, SessionFetchError> {
    if line.trim().is_empty() {
        return Err(malformed("Herdr event stream emitted an empty line"));
    }
    let value: Value = serde_json::from_str(line)
        .map_err(|e| malformed(format!("Herdr event stream emitted invalid JSON: {e}")))?;
    if let Some(error) = value.get("error") {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| malformed("Herdr subscription error is missing id"))?;
        if id != "herdr-core:events.subscribe" {
            return Err(malformed(format!(
                "Herdr subscription error id {id:?} is unexpected"
            )));
        }
        for field in ["code", "message"] {
            if error.get(field).and_then(Value::as_str).is_none() {
                return Err(malformed(format!(
                    "Herdr subscription error is missing {field}"
                )));
            }
        }
        let response: err::ErrorResponse = serde_json::from_value(value)
            .map_err(|e| malformed(format!("Herdr subscription error is malformed: {e}")))?;
        return Ok(SubscriptionLine::Error {
            code: response.error.code,
            message: response.error.message,
        });
    }
    // Preserve semantic replay identity, independent of JSON key order or whitespace.
    let fingerprint = serde_json::to_string(&value)
        .map_err(|e| malformed(format!("event fingerprint could not be encoded: {e}")))?;
    let envelope: EventMetadata = serde_json::from_value(value)
        .map_err(|e| malformed(format!("Herdr sequenced event is malformed: {e}")))?;
    let kind = envelope.payload.event.to_string();
    let (actual, data) = convert_event(envelope.payload.data);
    if actual != kind {
        return Err(malformed(format!(
            "Herdr {kind} event event data type is {actual:?}"
        )));
    }
    Ok(SubscriptionLine::Event(ReplicaEnvelope {
        protocol: envelope.protocol,
        host: HostScope {
            host_id: envelope.host.host_id,
            session_id: envelope.host.session_id,
        },
        sequence: envelope.sequence,
        data,
        fingerprint,
    }))
}

// The two independently generated sub-schemas describe the same record shapes.
// Expand explicit field moves for each, keeping all generated type references here.
macro_rules! record_conversions {
    ($wire:ident) => {
        impl From<$wire::WorkspaceInfo> for ProjectedWorkspace {
            fn from(v: $wire::WorkspaceInfo) -> Self {
                Self {
                    workspace_id: v.workspace_id,
                    label: v.label,
                    active_tab_id: v.active_tab_id,
                    worktree: v.worktree.map(Into::into),
                }
            }
        }
        impl From<$wire::WorkspaceWorktreeInfo> for ProjectedWorktree {
            fn from(v: $wire::WorkspaceWorktreeInfo) -> Self {
                Self {
                    repo_key: v.repo_key,
                    repo_name: v.repo_name,
                    repo_root: v.repo_root,
                    checkout_path: v.checkout_path,
                    is_linked_worktree: v.is_linked_worktree,
                }
            }
        }
        impl From<$wire::TabInfo> for ProjectedTab {
            fn from(v: $wire::TabInfo) -> Self {
                Self {
                    tab_id: v.tab_id,
                    workspace_id: v.workspace_id,
                    label: v.label,
                }
            }
        }
        impl From<$wire::PaneInfo> for ProjectedPane {
            fn from(v: $wire::PaneInfo) -> Self {
                Self {
                    pane_id: v.pane_id,
                    workspace_id: v.workspace_id,
                    tab_id: v.tab_id,
                    cwd: v.cwd,
                    tokens: v.tokens.into_iter().map(|(k, v)| (k.into(), Value::String(v))).collect(),
                    label: v.label,
                    terminal_title: v.terminal_title,
                    terminal_title_stripped: v.terminal_title_stripped,
                }
            }
        }
        impl From<$wire::PaneLayoutRect> for SessionLayoutRect {
            fn from(v: $wire::PaneLayoutRect) -> Self {
                Self {
                    x: v.x,
                    y: v.y,
                    width: v.width,
                    height: v.height,
                }
            }
        }
        impl From<$wire::PaneLayoutSnapshot> for SessionLayoutPayload {
            fn from(v: $wire::PaneLayoutSnapshot) -> Self {
                Self {
                    workspace_id: v.workspace_id,
                    tab_id: v.tab_id,
                    zoomed: v.zoomed,
                    area: v.area.into(),
                    focused_pane_id: v.focused_pane_id,
                    panes: v
                        .panes
                        .into_iter()
                        .map(|p| SessionLayoutPanePayload {
                            pane_id: p.pane_id,
                            rect: p.rect.into(),
                        })
                        .collect(),
                    splits: v
                        .splits
                        .into_iter()
                        .map(|s| SessionLayoutSplitPayload {
                            direction: match s.direction {
                                $wire::SplitDirection::Right => PaneLayoutDirection::Right,
                                $wire::SplitDirection::Down => PaneLayoutDirection::Down,
                            },
                            ratio: s.ratio,
                            rect: s.rect.into(),
                        })
                        .collect(),
                }
            }
        }
    };
}
record_conversions!(res);
record_conversions!(ev);

impl From<res::AgentInfo> for ProjectedAgent {
    fn from(v: res::AgentInfo) -> Self {
        Self {
            pane_id: v.pane_id, workspace_id: v.workspace_id, tab_id: v.tab_id, cwd: v.cwd,
            agent: v.agent, agent_status: Some(v.agent_status.to_string()),
            agent_session: v.agent_session.map(|s| SessionAgentSessionPayload { kind: s.kind.to_string(), value: s.value }),
            spawned_from_pane_id: v.spawned_from_pane_id, state_change_seq: v.state_change_seq,
            tokens: v.tokens.into_iter().map(|(k,v)| (k.into(), Value::String(v))).collect(),
            ambient: v.ambient.map(|v| json!({ "background_failed": v.background_failed, "background_running": v.background_running, "subagents_active": v.subagents_active })),
        }
    }
}

fn convert_event(data: ev::EventData) -> (&'static str, ReplicaEvent) {
    match data {
        ev::EventData::WorkspaceCreated { workspace, .. } => (
            "workspace_created",
            ReplicaEvent::WorkspaceCreated {
                workspace: workspace.into(),
            },
        ),
        ev::EventData::WorkspaceUpdated { workspace, .. } => (
            "workspace_updated",
            ReplicaEvent::WorkspaceUpdated {
                workspace: workspace.into(),
            },
        ),
        ev::EventData::WorkspaceMetadataUpdated { workspace, .. } => (
            "workspace_metadata_updated",
            ReplicaEvent::WorkspaceUpdated {
                workspace: workspace.into(),
            },
        ),
        ev::EventData::WorkspaceClosed { workspace_id, .. } => (
            "workspace_closed",
            ReplicaEvent::WorkspaceClosed {
                workspace_id: workspace_id,
            },
        ),
        ev::EventData::WorkspaceRenamed {
            label,
            workspace_id,
            ..
        } => (
            "workspace_renamed",
            ReplicaEvent::WorkspaceRenamed {
                label: label,
                workspace_id: workspace_id,
            },
        ),
        ev::EventData::WorkspaceMoved {
            insert_index,
            workspace_id,
            workspaces,
            ..
        } => (
            "workspace_moved",
            ReplicaEvent::WorkspaceMoved {
                insert_index: insert_index as usize,
                workspace_id: workspace_id,
                workspaces: workspaces.into_iter().map(Into::into).collect(),
            },
        ),
        ev::EventData::WorkspaceReordered {
            workspace_ids,
            workspaces,
            ..
        } => (
            "workspace_reordered",
            ReplicaEvent::WorkspaceReordered {
                workspace_ids: workspace_ids,
                workspaces: workspaces.into_iter().map(Into::into).collect(),
            },
        ),
        ev::EventData::WorkspaceFocused { workspace_id, .. } => (
            "workspace_focused",
            ReplicaEvent::WorkspaceFocused {
                workspace_id: workspace_id,
            },
        ),
        ev::EventData::WorktreeCreated { workspace, .. } => (
            "worktree_created",
            ReplicaEvent::WorktreeCreated {
                workspace: workspace.into(),
            },
        ),
        ev::EventData::WorktreeOpened { workspace, .. } => (
            "worktree_opened",
            ReplicaEvent::WorktreeOpened {
                workspace: workspace.into(),
            },
        ),
        ev::EventData::WorktreeRemoved {
            workspace,
            workspace_id,
            ..
        } => (
            "worktree_removed",
            ReplicaEvent::WorktreeRemoved {
                workspace: workspace.map(Into::into),
                workspace_id: workspace_id,
            },
        ),
        ev::EventData::TabCreated { tab, .. } => {
            ("tab_created", ReplicaEvent::TabCreated { tab: tab.into() })
        }
        ev::EventData::TabClosed {
            tab_id,
            workspace_id,
            ..
        } => (
            "tab_closed",
            ReplicaEvent::TabClosed {
                tab_id: tab_id,
                workspace_id: workspace_id,
            },
        ),
        ev::EventData::TabRenamed {
            label,
            tab_id,
            workspace_id,
            ..
        } => (
            "tab_renamed",
            ReplicaEvent::TabRenamed {
                label: label,
                tab_id: tab_id,
                workspace_id: workspace_id,
            },
        ),
        ev::EventData::TabMoved {
            insert_index,
            tab_id,
            tabs,
            workspace_id,
            ..
        } => (
            "tab_moved",
            ReplicaEvent::TabMoved {
                insert_index: insert_index as usize,
                tab_id: tab_id,
                tabs: tabs.into_iter().map(Into::into).collect(),
                workspace_id: workspace_id,
            },
        ),
        ev::EventData::TabFocused {
            tab_id,
            workspace_id,
            ..
        } => (
            "tab_focused",
            ReplicaEvent::TabFocused {
                tab_id: tab_id,
                workspace_id: workspace_id,
            },
        ),
        ev::EventData::PaneCreated { pane, .. } => (
            "pane_created",
            ReplicaEvent::PaneCreated { pane: pane.into() },
        ),
        ev::EventData::PaneClosed {
            pane_id,
            workspace_id,
            ..
        } => (
            "pane_closed",
            ReplicaEvent::PaneClosed {
                pane_id: pane_id,
                workspace_id: workspace_id,
            },
        ),
        ev::EventData::PaneUpdated { pane, .. } => (
            "pane_updated",
            ReplicaEvent::PaneUpdated { pane: pane.into() },
        ),
        ev::EventData::PaneFocused {
            pane_id,
            workspace_id,
            ..
        } => (
            "pane_focused",
            ReplicaEvent::PaneFocused {
                pane_id: pane_id,
                workspace_id: workspace_id,
            },
        ),
        ev::EventData::PaneExited {
            pane_id,
            workspace_id,
            ..
        } => (
            "pane_exited",
            ReplicaEvent::PaneExited {
                pane_id: pane_id,
                workspace_id: workspace_id,
            },
        ),
        ev::EventData::PaneAgentDetected {
            pane_id,
            workspace_id,
            ..
        } => (
            "pane_agent_detected",
            ReplicaEvent::PaneAgentDetected {
                pane_id: pane_id,
                workspace_id: workspace_id,
            },
        ),
        ev::EventData::LayoutUpdated { layout, .. } => (
            "layout_updated",
            ReplicaEvent::LayoutUpdated {
                layout: layout.into(),
            },
        ),
        ev::EventData::PaneMoved {
            previous_pane_id,
            previous_workspace_id,
            previous_tab_id,
            pane,
            created_workspace,
            created_tab,
            closed_workspace_id,
            closed_tab_id,
        } => (
            "pane_moved",
            ReplicaEvent::PaneMoved(PaneMove {
                previous_pane_id,
                previous_workspace_id,
                previous_tab_id,
                pane: pane.into(),
                created_workspace: created_workspace.map(Into::into),
                created_tab: created_tab.map(Into::into),
                closed_workspace_id,
                closed_tab_id,
            }),
        ),
        ev::EventData::AgentLineageChanged { .. } => (
            "agent_lineage_changed",
            ReplicaEvent::Unrequested("agent_lineage_changed".to_owned()),
        ),
        ev::EventData::PaneOutputChanged { .. } => (
            "pane_output_changed",
            ReplicaEvent::Unrequested("pane_output_changed".to_owned()),
        ),
        ev::EventData::PaneAgentStatusChanged { .. } => (
            "pane_agent_status_changed",
            ReplicaEvent::Unrequested("pane_agent_status_changed".to_owned()),
        ),
    }
}

fn params(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value)
        .map_err(|error| format!("Herdr parameters could not be encoded: {error}"))
}

// session.snapshot has no parameters in the pinned request schema.
pub(crate) fn empty_params() -> Value {
    Value::Object(Default::default())
}

pub(crate) fn workspace_create_params(cwd: &str, label: &str) -> Result<Value, String> {
    params(req::WorkspaceCreateParams {
        cwd: Some(cwd.into()),
        label: Some(label.into()),
        focus: true,
        env: Default::default(),
    })
}
pub(crate) fn workspace_target_params(id: &str) -> Result<Value, String> {
    params(req::WorkspaceTarget {
        workspace_id: id.into(),
    })
}
pub(crate) fn tab_target_params(id: &str) -> Result<Value, String> {
    params(req::TabTarget { tab_id: id.into() })
}
pub(crate) fn pane_target_params(id: &str) -> Result<Value, String> {
    params(req::PaneTarget { pane_id: id.into() })
}
pub(crate) fn tab_create_params(workspace: &str, cwd: &str, label: &str) -> Result<Value, String> {
    params(req::TabCreateParams {
        workspace_id: Some(workspace.into()),
        cwd: Some(cwd.into()),
        label: Some(label.into()),
        focus: true,
        env: Default::default(),
    })
}
pub(crate) fn tab_move_params(tab: &str, index: usize) -> Result<Value, String> {
    params(req::TabMoveParams {
        tab_id: tab.into(),
        insert_index: index
            .try_into()
            .map_err(|_| "tab.move insert_index exceeds u32")?,
    })
}
pub(crate) fn pane_split_params(
    pane: &str,
    direction: crate::live::PaneSplitDirection,
    cwd: Option<&str>,
) -> Result<Value, String> {
    params(req::PaneSplitParams {
        target_pane_id: Some(pane.into()),
        direction: match direction {
            crate::live::PaneSplitDirection::Right => req::SplitDirection::Right,
            crate::live::PaneSplitDirection::Down => req::SplitDirection::Down,
        },
        cwd: cwd.filter(|v| !v.trim().is_empty()).map(Into::into),
        focus: true,
        env: Default::default(),
        ratio: None,
        right_click: req::PaneRightClickTarget::Herdr,
        workspace_id: None,
    })
}
pub(crate) fn pane_resize_params(
    pane: &str,
    direction: crate::live::PaneResizeDirection,
    amount: f32,
) -> Result<Value, String> {
    params(req::PaneResizeParams {
        pane_id: Some(pane.into()),
        direction: match direction {
            crate::live::PaneResizeDirection::Left => req::PaneDirection::Left,
            crate::live::PaneResizeDirection::Right => req::PaneDirection::Right,
            crate::live::PaneResizeDirection::Up => req::PaneDirection::Up,
            crate::live::PaneResizeDirection::Down => req::PaneDirection::Down,
        },
        amount: Some(amount),
    })
}
pub(crate) fn pane_layout_params(pane: &str) -> Result<Value, String> {
    params(req::PaneLayoutParams {
        pane_id: Some(pane.into()),
    })
}
pub(crate) fn pane_zoom_params(pane: &str) -> Result<Value, String> {
    params(req::PaneZoomParams {
        pane_id: Some(pane.into()),
        mode: req::PaneZoomMode::Toggle,
    })
}
pub(crate) fn pane_read_params(pane: &str, source: &str, lines: u32) -> Result<Value, String> {
    params(req::PaneReadParams {
        pane_id: pane.into(),
        source: source
            .parse()
            .map_err(|e| format!("invalid read source: {e}"))?,
        lines: Some(lines),
        format: req::ReadFormat::Text,
        strip_ansi: true,
    })
}

fn response(value: Value, missing: &str) -> Result<res::ResponseResult, String> {
    serde_json::from_value(value).map_err(|_| missing.to_owned())
}
fn nonempty_id(id: String, missing: &str) -> Result<String, String> {
    if id.trim().is_empty() {
        Err(missing.into())
    } else {
        Ok(id)
    }
}
pub(crate) fn created_workspace_pane(value: Value) -> Result<String, String> {
    let missing = "workspace.create response is missing root_pane.pane_id";
    match response(value, missing)? {
        res::ResponseResult::WorkspaceCreated { root_pane, .. } => {
            nonempty_id(root_pane.pane_id, missing)
        }
        _ => Err(missing.into()),
    }
}
pub(crate) fn created_tab(value: Value) -> Result<(String, String), String> {
    let tab_missing = "tab.create response is missing tab.tab_id";
    let pane_missing = "tab.create response is missing root_pane.pane_id";
    // Preserve missing-field priority before the generated record rejects it.
    for (pointer, message) in [
        ("/tab/tab_id", tab_missing),
        ("/root_pane/pane_id", pane_missing),
    ] {
        if !value
            .pointer(pointer)
            .and_then(Value::as_str)
            .is_some_and(|id| !id.trim().is_empty())
        {
            return Err(message.into());
        }
    }
    match response(value, tab_missing)? {
        res::ResponseResult::TabCreated { tab, root_pane } => Ok((tab.tab_id, root_pane.pane_id)),
        _ => Err(tab_missing.into()),
    }
}
pub(crate) fn moved_tabs(value: Value) -> Result<Vec<String>, String> {
    let missing = "tab.move response is missing tabs";
    let id_missing = "tab.move response has a tab without an id";
    let tabs = value.get("tabs").and_then(Value::as_array).ok_or(missing)?;
    if tabs.iter().any(|tab| {
        !tab.get("tab_id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.trim().is_empty())
    }) {
        return Err(id_missing.into());
    }
    match response(value, missing)? {
        res::ResponseResult::TabList { tabs } => {
            Ok(tabs.into_iter().map(|tab| tab.tab_id).collect())
        }
        _ => Err(missing.into()),
    }
}
pub(crate) fn split_pane(value: Value) -> Result<String, String> {
    let missing = "pane.split response is missing pane.pane_id";
    match response(value, missing)? {
        res::ResponseResult::PaneInfo { pane } => nonempty_id(pane.pane_id, missing),
        _ => Err(missing.into()),
    }
}
pub(crate) fn pane_layout(value: Value) -> Result<SessionLayoutPayload, String> {
    if value.get("layout").is_none() {
        return Err("pane.layout response is missing layout".into());
    }
    let result: res::ResponseResult = serde_json::from_value(value)
        .map_err(|error| format!("pane.layout response is malformed: {error}"))?;
    match result {
        res::ResponseResult::PaneLayout { layout } => Ok(layout.into()),
        _ => Err("pane.layout response is missing layout".into()),
    }
}
pub(crate) fn pane_text(value: Value) -> Result<crate::live::PaneText, String> {
    if value.get("read").is_none() {
        return Err("pane.read returned no read section".into());
    }
    let result: res::ResponseResult = serde_json::from_value(value)
        .map_err(|error| format!("pane.read response is malformed: {error}"))?;
    match result {
        res::ResponseResult::PaneRead { read } => Ok(crate::live::PaneText {
            text: read.text,
            truncated: read.truncated,
        }),
        _ => Err("pane.read returned no read section".into()),
    }
}
pub(crate) fn live_session_response(
    value: Value,
) -> Result<crate::sidebar::SessionSnapshotPayload, SessionFetchError> {
    let snapshot = value
        .get("snapshot")
        .ok_or_else(|| malformed("response is missing snapshot"))?;
    crate::session_sync::project_snapshot(snapshot)
}

fn remote_protocol_error(operation: &str, reason: impl Into<String>) -> crate::remote::RemoteError {
    crate::remote::RemoteError::new(
        operation,
        "herdr",
        crate::remote::RemoteStage::Protocol,
        reason,
        false,
        true,
    )
}

pub(crate) fn remote_snapshot(
    value: &Value,
    operation: &str,
) -> crate::remote::RemoteResult<crate::remote::RemoteSnapshotEnvelope> {
    use crate::remote::{
        REMOTE_PROTOCOL_REVISION, RemoteError, RemoteSnapshotEnvelope, RemoteStage,
    };
    let snapshot = value
        .get("result")
        .and_then(|v| v.get("snapshot"))
        .or_else(|| value.get("snapshot"))
        .ok_or_else(|| {
            RemoteError::new(
                operation,
                "herdr",
                RemoteStage::Herdr,
                "response does not contain result.snapshot",
                true,
                false,
            )
        })?;
    let protocol = snapshot
        .get("protocol")
        .and_then(Value::as_u64)
        .ok_or_else(|| remote_protocol_error(operation, "snapshot.protocol is missing"))?;
    let protocol = u32::try_from(protocol)
        .map_err(|_| remote_protocol_error(operation, "snapshot.protocol exceeds u32"))?;
    if protocol != REMOTE_PROTOCOL_REVISION {
        return Err(remote_protocol_error(
            operation,
            format!("protocol mismatch expected={REMOTE_PROTOCOL_REVISION} received={protocol}"),
        ));
    }
    let host = snapshot.get("host").ok_or_else(|| {
        RemoteError::new(
            operation,
            "herdr",
            RemoteStage::Herdr,
            "snapshot.host is missing",
            false,
            true,
        )
    })?;
    for key in ["host_id", "session_id"] {
        if !host
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|v| !v.is_empty())
        {
            return Err(remote_protocol_error(
                operation,
                format!("missing non-empty field {key}"),
            ));
        }
    }
    if snapshot
        .get("event_sequence")
        .and_then(Value::as_u64)
        .is_none()
    {
        return Err(remote_protocol_error(
            operation,
            "snapshot.event_sequence is missing",
        ));
    }
    let snapshot: res::SessionSnapshot = serde_json::from_value(snapshot.clone())
        .map_err(|error| remote_protocol_error(operation, error.to_string()))?;
    let workspace_ids = remote_ids(
        snapshot.workspaces.into_iter().map(|v| v.workspace_id),
        "workspaces",
        "workspace_id",
        operation,
    )?;
    let pane_ids = remote_ids(
        snapshot.panes.into_iter().map(|v| v.pane_id),
        "panes",
        "pane_id",
        operation,
    )?;
    let agent_ids = remote_ids(
        snapshot
            .agents
            .into_iter()
            .filter_map(|v| v.agent_instance_id),
        "agents",
        "agent_instance_id",
        operation,
    )?;
    Ok(RemoteSnapshotEnvelope {
        host: crate::domain::HostScope {
            host_id: snapshot.host.host_id,
            session_id: snapshot.host.session_id,
        },
        protocol,
        event_sequence: snapshot.event_sequence,
        workspace_ids,
        pane_ids,
        agent_ids,
    })
}
fn remote_ids(
    ids: impl Iterator<Item = String>,
    array: &str,
    key: &str,
    operation: &str,
) -> crate::remote::RemoteResult<Vec<String>> {
    let mut ids = ids.collect::<Vec<_>>();
    if ids.iter().any(String::is_empty) {
        let reason = if key == "agent_instance_id" {
            format!("{array}.{key} must be a non-empty string or null")
        } else {
            format!("missing non-empty field {key}")
        };
        return Err(remote_protocol_error(operation, reason));
    }
    ids.sort();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(remote_protocol_error(
            operation,
            format!("{array} contains duplicate {key}"),
        ));
    }
    Ok(ids)
}
pub(crate) fn terminal_input_line(bytes: &[u8]) -> Result<String, String> {
    let mut line = serde_json::to_string(&json!({
        "type": "terminal.input",
        "bytes": crate::live::encode_base64(bytes),
    }))
    .map_err(|error| format!("terminal input could not be encoded: {error}"))?;
    line.push('\n');
    Ok(line)
}

pub(crate) fn terminal_scroll_line(direction: &str, lines: u16) -> Result<String, String> {
    if !matches!(direction, "up" | "down") {
        return Err(format!(
            "terminal scroll direction is not up or down: {direction}"
        ));
    }
    if lines == 0 {
        return Err("terminal scroll needs at least one line".to_owned());
    }
    let mut line = serde_json::to_string(&json!({
        "type": "terminal.scroll",
        "direction": direction,
        "lines": lines,
        "source": "wheel",
    }))
    .map_err(|error| format!("terminal scroll could not be encoded: {error}"))?;
    line.push('\n');
    Ok(line)
}

pub(crate) fn terminal_resize_line(rows: u16, cols: u16) -> Result<String, String> {
    if rows == 0 || cols == 0 {
        return Err("terminal dimensions must be positive".to_owned());
    }
    let mut line = serde_json::to_string(&json!({
        "type": "terminal.resize",
        "cols": cols,
        "rows": rows,
        "cell_width_px": 0,
        "cell_height_px": 0,
    }))
    .map_err(|error| format!("terminal resize could not be encoded: {error}"))?;
    line.push('\n');
    Ok(line)
}

pub(crate) fn created_agent_pane(bytes: &[u8]) -> Result<String, String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("herdr agent new returned unreadable output: {error}"))?;
    let response: res::SuccessResponse = serde_json::from_value(value)
        .map_err(|_| "herdr agent new reported no created pane".to_owned())?;
    match response.result {
        res::ResponseResult::AgentCreated { agent, .. } => {
            nonempty_id(agent.pane_id, "herdr agent new reported no created pane")
        }
        _ => Err("herdr agent new reported no created pane".into()),
    }
}

pub(crate) fn terminal_release_line() -> String {
    "{\"type\":\"terminal.release\"}\n".to_owned()
}

#[cfg(test)]
pub(crate) fn checked_response_fixture(id: &Value, result: Value) -> Value {
    let value = json!({"id": id, "result": result});
    let _: res::SuccessResponse = serde_json::from_value(value.clone())
        .expect("fixture must match the pinned generated response");
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr_contract::HERDR_API_SCHEMA_JSON;

    #[test]
    fn captured_agent_new_decodes_only_the_observed_created_variant() {
        let bytes = include_bytes!("../tests/fixtures/agent-created.json");
        let response: res::SuccessResponse = serde_json::from_slice(bytes).unwrap();
        assert!(matches!(
            response.result,
            res::ResponseResult::AgentCreated { .. }
        ));
        assert_eq!(created_agent_pane(bytes).unwrap(), "w1:p4");
        assert_eq!(
            created_agent_pane(br#"{"id":"fixture","result":{"type":"ok"}}"#).unwrap_err(),
            "herdr agent new reported no created pane"
        );
        assert!(
            created_agent_pane(b"not json")
                .unwrap_err()
                .starts_with("herdr agent new returned unreadable output: ")
        );
    }

    #[test]
    #[ignore = "requires the owned pinned-server probe output"]
    fn isolated_live_remote_responses_decode_through_generated_types() {
        let directory = std::env::var("HERDR_TEST_TYPED_RESPONSES")
            .expect("check script supplies the owned probe directory");
        let load = |name: &str| -> Value {
            let bytes = std::fs::read(std::path::Path::new(&directory).join(name)).unwrap();
            let _: res::SuccessResponse = serde_json::from_slice(&bytes).unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()["result"].clone()
        };
        assert_eq!(
            created_workspace_pane(load("workspace.create.json")).unwrap(),
            "w1:p1"
        );
        assert_eq!(
            created_tab(load("tab.create.json")).unwrap(),
            ("w1:t2".into(), "w1:p2".into())
        );
        assert_eq!(
            moved_tabs(load("tab.move.json")).unwrap(),
            ["w1:t2", "w1:t1"]
        );
        assert_eq!(split_pane(load("pane.split.json")).unwrap(), "w1:p3");
        let layout = pane_layout(load("pane.layout.json")).unwrap();
        assert_eq!(layout.panes.len(), 2);
        pane_text(load("pane.read.json")).unwrap();
        let snapshot = load("session.snapshot.json");
        live_session_response(snapshot.clone()).unwrap();
        let remote = remote_snapshot(&snapshot, "probe").unwrap();
        assert_eq!(remote.pane_ids, ["w1:p1", "w1:p2", "w1:p3"]);
        let bytes = std::fs::read(std::path::Path::new(&directory).join("agent-new.json")).unwrap();
        assert_eq!(created_agent_pane(&bytes).unwrap(), "w1:p4");
    }

    #[test]
    fn malformed_remote_snapshots_keep_protocol_priority_and_diagnostic_flags() {
        let value = json!({"snapshot": {"protocol": HERDR_PROTOCOL_REVISION + 1}});
        let error = remote_snapshot(&value, "fixture").unwrap_err();
        assert_eq!(error.stage(), crate::remote::RemoteStage::Protocol);
        assert!(!error.diagnostic().retryable);
        assert!(error.diagnostic().action_required);
        assert_eq!(
            error.diagnostic().reason,
            format!(
                "protocol mismatch expected={HERDR_PROTOCOL_REVISION} received={}",
                HERDR_PROTOCOL_REVISION + 1
            )
        );
        let mut snapshot = empty_snapshot();
        snapshot.as_object_mut().unwrap().remove("version");
        let error = remote_snapshot(&json!({"snapshot":snapshot}), "fixture").unwrap_err();
        assert_eq!(error.stage(), crate::remote::RemoteStage::Protocol);
        assert!(!error.diagnostic().retryable);
        assert!(error.diagnostic().action_required);
        assert_eq!(error.diagnostic().reason, "missing field `version`");
    }

    #[test]
    fn delete_manual_parameterless_and_terminal_requests_when_schema_declares_them() {
        let schema: Value = serde_json::from_str(HERDR_API_SCHEMA_JSON).unwrap();
        let definitions = schema["schemas"]["request"]["$defs"].as_object().unwrap();
        for name in [
            "SessionSnapshotParams",
            "TerminalInputParams",
            "TerminalScrollParams",
            "TerminalResizeParams",
            "TerminalReleaseParams",
        ] {
            assert!(
                !definitions.contains_key(name),
                "{name} is now generated; delete its manual request builder"
            );
        }
    }

    fn empty_snapshot() -> Value {
        json!({"protocol": HERDR_PROTOCOL_REVISION, "version": "fixture", "host": {"host_id": "fixture-host", "session_id": "fixture"},
            "event_sequence": 4, "workspaces": [], "tabs": [], "panes": [], "layouts": [], "agents": [], "lineage": []})
    }

    #[test]
    fn delete_manual_event_metadata_when_the_contract_declares_it() {
        let schema: Value = serde_json::from_str(HERDR_API_SCHEMA_JSON).unwrap();
        let properties = schema["schemas"]["event"]["properties"]
            .as_object()
            .unwrap();
        for field in ["sequence", "host", "protocol"] {
            assert!(
                !properties.contains_key(field),
                "Herdr now declares {field}; delete EventMetadata and consume the generated envelope"
            );
        }
    }

    #[test]
    fn generated_snapshot_and_response_variants_feed_domain_inputs() {
        let value = empty_snapshot();
        let (host, cursor, state) = snapshot(value.clone()).unwrap();
        assert_eq!(host.host_id, "fixture-host");
        assert_eq!(cursor, 4);
        assert!(state.workspaces.is_empty());
        let (_, cursor, _) =
            snapshot_response(json!({"type": "session_snapshot", "snapshot": value})).unwrap();
        assert_eq!(cursor, 4);
        assert!(
            agents_response(json!({"type": "agent_list", "agents": []}))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn missing_snapshot_fields_keep_their_diagnostics() {
        for field in crate::session_sync::SNAPSHOT_FIELDS_THE_REPLICA_READS {
            let mut value = empty_snapshot();
            value.as_object_mut().unwrap().remove(field);
            let error = snapshot(value).unwrap_err();
            assert_eq!(error.state(), "malformed");
            assert_eq!(error.message(), format!("snapshot is missing {field}"));
        }
        let mut value = empty_snapshot();
        value["host"]["host_id"] = json!(" ");
        let error = snapshot(value).unwrap_err();
        assert_eq!(error.state(), "malformed");
        assert_eq!(
            error.message(),
            "snapshot host contains an empty identifier"
        );
    }

    #[test]
    fn incompatible_protocol_retains_both_revisions_and_the_remedy() {
        let mut value = empty_snapshot();
        let received = HERDR_PROTOCOL_REVISION + 1;
        value["protocol"] = json!(received);
        let error = snapshot(value).unwrap_err();
        assert_eq!(error.state(), "protocol_mismatch");
        assert_eq!(
            error.message(),
            format!(
                "The running Herdr speaks protocol {received}; this hide needs protocol {HERDR_PROTOCOL_REVISION}. Stop it with `herdr server stop` and reopen hide so it starts its bundled Herdr, or update hide to a release built against that Herdr."
            )
        );
    }

    #[test]
    fn generated_subscriptions_preserve_the_resume_cursor_and_filter_shape() {
        assert_eq!(
            subscription_params(42, &["workspace.created", "pane.focused"]).unwrap(),
            json!({"after_sequence": 42, "subscriptions": [{"type": "workspace.created"}, {"type": "pane.focused"}]})
        );
        assert!(subscription_params(42, &["not.a.subscription"]).is_err());
    }

    #[test]
    fn generated_event_payload_and_metadata_keep_replay_identity() {
        let value = json!({"protocol": HERDR_PROTOCOL_REVISION, "host": {"host_id": "fixture-host", "session_id": "fixture"},
            "sequence": 42, "event": "workspace_focused", "data": {"type": "workspace_focused", "workspace_id": "w1"}});
        let SubscriptionLine::Event(event) = parse_subscription_line(&value.to_string()).unwrap()
        else {
            panic!("event expected")
        };
        assert_eq!(event.sequence, 42);
        assert_eq!(event.host.host_id, "fixture-host");
        assert!(
            matches!(event.data, ReplicaEvent::WorkspaceFocused { workspace_id } if workspace_id == "w1")
        );
        let SubscriptionLine::Event(pretty) =
            parse_subscription_line(&serde_json::to_string_pretty(&value).unwrap()).unwrap()
        else {
            panic!("event expected")
        };
        assert_eq!(event.fingerprint, pretty.fingerprint);
        let mut wrong = value;
        wrong["data"]["type"] = json!("workspace_closed");
        let error = parse_subscription_line(&wrong.to_string()).err().unwrap();
        assert_eq!(
            error.message(),
            "Herdr workspace_focused event event data type is \"workspace_closed\""
        );
    }

    #[test]
    fn subscription_error_diagnostics_survive_generated_deserialization() {
        let value = json!({"id": "herdr-core:events.subscribe", "error": {"code": "event_gap", "message": "cursor retired"}});
        let SubscriptionLine::Error { code, message } =
            parse_subscription_line(&value.to_string()).unwrap()
        else {
            panic!("error expected")
        };
        assert_eq!(
            (code.as_str(), message.as_str()),
            ("event_gap", "cursor retired")
        );
        for field in ["id", "code", "message"] {
            let mut value = value.clone();
            if field == "id" {
                value.as_object_mut().unwrap().remove(field);
            } else {
                value["error"].as_object_mut().unwrap().remove(field);
            }
            let error = parse_subscription_line(&value.to_string()).err().unwrap();
            assert_eq!(
                error.message(),
                format!("Herdr subscription error is missing {field}")
            );
        }
        assert_eq!(
            parse_subscription_line(" ").err().unwrap().message(),
            "Herdr event stream emitted an empty line"
        );
    }

    #[test]
    fn generated_types_reject_incomplete_records_and_invalid_token_names() {
        let value = json!({"type": "agent_list", "agents": [{"pane_id": "w1:p1"}]});
        assert!(agents_response(value).is_err());
        let value = json!({"type": "agent_list", "agents": [{"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1",
            "terminal_id": "fixture-terminal", "revision": 1, "focused": false, "agent_status": "idle", "tokens": {"invalid key": "value"}}]});
        assert!(agents_response(value).is_err());
    }
}
