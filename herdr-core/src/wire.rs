//! The only conversion boundary between the pinned Herdr schema and the replica.
//! Generated values are consumed into domain inputs; no JSON round trip converts
//! a generated payload back into a hand-written deserialization shape.

use serde_json::{Value, json};

use crate::live::SessionFetchError;
use crate::model::PaneLayoutDirection;
use crate::recent_closed::{ClosedLayout, ClosedLayoutNode, ClosedSplitDirection};
use crate::session_sync::{
    PaneMove, ProjectedAgent, ProjectedPane, ProjectedTab, ProjectedWorkspace, ProjectedWorktree,
    ProjectionState, ReplicaEvent, SubscriptionLine,
};
use crate::sidebar::{
    SessionAgentSessionPayload, SessionLayoutPanePayload, SessionLayoutPayload, SessionLayoutRect,
    SessionLayoutSplitPayload,
};
use hide_herdr_client::HERDR_PROTOCOL_REVISION;
use hide_herdr_client::wire::{
    error_response as err, event as ev, request as req, success_response as res,
};

pub(crate) fn protocol_mismatch(
    received_protocol: u64,
    received_version: Option<String>,
) -> SessionFetchError {
    let message = if received_protocol < HERDR_PROTOCOL_REVISION {
        format!(
            "The running Herdr uses protocol {received_protocol}, but Hide requires protocol {HERDR_PROTOCOL_REVISION}. When your current work is safe, stop the Herdr session and reopen Hide. Hide will start its compatible bundled Herdr. No workspace or agent was created."
        )
    } else {
        format!(
            "The running Herdr uses protocol {received_protocol}, but this Hide supports protocol {HERDR_PROTOCOL_REVISION}. Update Hide to a compatible release, then try again. No workspace or agent was created."
        )
    };
    SessionFetchError::Protocol {
        message,
        expected_protocol: HERDR_PROTOCOL_REVISION,
        received_protocol,
        received_version,
    }
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
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| !version.trim().is_empty());
    if protocol != HERDR_PROTOCOL_REVISION {
        return Err(protocol_mismatch(protocol, version.map(str::to_owned)));
    }
    if version.is_none() {
        return Err(malformed("snapshot is missing version"));
    }
    for field in ["workspaces", "tabs", "panes", "layouts", "agents"] {
        if !value.get(field).is_some_and(Value::is_array) {
            return Err(malformed(format!("snapshot is missing {field}")));
        }
    }
    Ok(())
}

pub(crate) fn snapshot(value: Value) -> Result<ProjectionState, SessionFetchError> {
    validate_snapshot(&value)?;
    let snapshot: res::SessionSnapshot = serde_json::from_value(value)
        .map_err(|e| malformed(format!("snapshot projection is malformed: {e}")))?;
    Ok(convert_snapshot(snapshot))
}

pub(crate) fn snapshot_response(value: Value) -> Result<ProjectionState, SessionFetchError> {
    decode_snapshot_response(value).map(convert_snapshot)
}

/// Cleanup protects both the launch directory and any current foreground
/// directory, without changing the navigation projection's ownership policy.
pub(crate) fn cleanup_usage_paths(value: Value) -> Result<Vec<Option<String>>, SessionFetchError> {
    let snapshot = decode_snapshot_response(value)?;
    let mut paths = Vec::new();
    for (cwd, foreground) in snapshot
        .panes
        .into_iter()
        .map(|p| (p.cwd, p.foreground_cwd))
        .chain(
            snapshot
                .agents
                .into_iter()
                .map(|a| (a.cwd, a.foreground_cwd)),
        )
    {
        if cwd.is_none() && foreground.is_none() {
            paths.push(None);
        }
        paths.extend(cwd.into_iter().chain(foreground).map(Some));
    }
    Ok(paths)
}

fn decode_snapshot_response(value: Value) -> Result<res::SessionSnapshot, SessionFetchError> {
    validate_snapshot(
        value
            .get("snapshot")
            .ok_or_else(|| malformed("response is missing snapshot"))?,
    )?;
    let response: res::ResponseResult = serde_json::from_value(value)
        .map_err(|e| malformed(format!("snapshot projection is malformed: {e}")))?;
    match response {
        res::ResponseResult::SessionSnapshot { snapshot } => Ok(snapshot),
        _ => Err(malformed("response is missing snapshot")),
    }
}

fn convert_snapshot(snapshot: res::SessionSnapshot) -> ProjectionState {
    ProjectionState {
        focused_pane_id: snapshot.focused_pane_id,
        focused_workspace_id: snapshot.focused_workspace_id,
        workspaces: snapshot.workspaces.into_iter().map(Into::into).collect(),
        tabs: snapshot.tabs.into_iter().map(Into::into).collect(),
        panes: snapshot.panes.into_iter().map(Into::into).collect(),
        layouts: snapshot.layouts.into_iter().map(Into::into).collect(),
        agents: snapshot.agents.into_iter().map(Into::into).collect(),
    }
}

pub(crate) fn agents_response(value: Value) -> Result<Vec<ProjectedAgent>, SessionFetchError> {
    if let Some(kind) = value.get("type").and_then(Value::as_str)
        && kind != "agent_list"
    {
        return Err(malformed(format!(
            "agent.list returned unexpected result type {kind:?}"
        )));
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
    let envelope: ev::EventEnvelope = serde_json::from_value(value)
        .map_err(|e| malformed(format!("Herdr event is malformed: {e}")))?;
    let kind = envelope.event.to_string();
    let (actual, data) = convert_event(envelope.data);
    if actual != kind {
        return Err(malformed(format!(
            "Herdr {kind} event event data type is {actual:?}"
        )));
    }
    Ok(SubscriptionLine::Event(data))
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
                    tokens: v
                        .tokens
                        .into_iter()
                        .map(|(key, value)| (String::from(key), Value::String(value)))
                        .collect(),
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
                    tokens: v
                        .tokens
                        .into_iter()
                        .map(|(k, v)| (k.into(), Value::String(v)))
                        .collect(),
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

/// The pane metadata token by which whoever created a pane declares the pane
/// it was spawned from. It is the only lineage there is: Herdr records none,
/// so an agent is a child exactly when its spawner said so.
///
/// Hide's own fork writes it, and so does an orchestrator that starts a child
/// with `agent.start` (sasu's implementor); both declare it afterwards with
/// `pane.report_metadata`, the same display-only channel the label plugin and
/// the hook helper already use. The value is the parent's pane id, it dies
/// with the pane, and it is read here and nowhere else.
pub(crate) const PARENT_PANE_TOKEN: &str = "parent_pane";

/// The source under which Hide writes its own pane tokens. Herdr keeps one
/// token set per source, so a token Hide wrote can never overwrite one an
/// orchestrator or the hook helper wrote under theirs.
pub(crate) const HIDE_METADATA_SOURCE: &str = "hide";

/// The parent a spawner declared. An empty value is no declaration, because
/// Herdr clears a token by setting it empty.
fn lineage_parent(declared: Option<&str>) -> Option<String> {
    declared
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

impl From<res::AgentInfo> for ProjectedAgent {
    fn from(v: res::AgentInfo) -> Self {
        let declared_parent = v
            .tokens
            .iter()
            .find(|(key, _)| key.as_str() == PARENT_PANE_TOKEN)
            .map(|(_, id)| id.as_str());
        let spawned_from_pane_id = lineage_parent(declared_parent);
        Self {
            pane_id: v.pane_id,
            name: v.name,
            workspace_id: v.workspace_id,
            tab_id: v.tab_id,
            cwd: v.cwd,
            agent: v.agent,
            agent_status: Some(v.agent_status.to_string()),
            agent_session: v.agent_session.map(|s| SessionAgentSessionPayload {
                kind: s.kind.to_string(),
                value: s.value,
            }),
            spawned_from_pane_id,
            state_change_seq: v.state_change_seq,
            tokens: v
                .tokens
                .into_iter()
                .map(|(k, v)| (k.into(), Value::String(v)))
                .collect(),
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
            ReplicaEvent::WorkspaceClosed { workspace_id },
        ),
        ev::EventData::WorkspaceRenamed {
            label,
            workspace_id,
            ..
        } => (
            "workspace_renamed",
            ReplicaEvent::WorkspaceRenamed {
                label,
                workspace_id,
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
                workspace_id,
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
                workspace_ids,
                workspaces: workspaces.into_iter().map(Into::into).collect(),
            },
        ),
        ev::EventData::WorkspaceFocused { workspace_id, .. } => (
            "workspace_focused",
            ReplicaEvent::WorkspaceFocused { workspace_id },
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
                workspace_id,
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
                tab_id,
                workspace_id,
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
                label,
                tab_id,
                workspace_id,
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
                tab_id,
                tabs: tabs.into_iter().map(Into::into).collect(),
                workspace_id,
            },
        ),
        ev::EventData::TabFocused {
            tab_id,
            workspace_id,
            ..
        } => (
            "tab_focused",
            ReplicaEvent::TabFocused {
                tab_id,
                workspace_id,
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
                pane_id,
                workspace_id,
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
                pane_id,
                workspace_id,
            },
        ),
        ev::EventData::PaneExited {
            pane_id,
            workspace_id,
            ..
        } => (
            "pane_exited",
            ReplicaEvent::PaneExited {
                pane_id,
                workspace_id,
            },
        ),
        ev::EventData::PaneAgentDetected {
            pane_id,
            workspace_id,
            ..
        } => (
            "pane_agent_detected",
            ReplicaEvent::PaneAgentDetected {
                pane_id,
                workspace_id,
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
    workspace_create_with_env_params(cwd, label, Default::default())
}

pub(crate) fn workspace_create_with_env_params(
    cwd: &str,
    label: &str,
    env: std::collections::BTreeMap<String, String>,
) -> Result<Value, String> {
    params(req::WorkspaceCreateParams {
        cwd: Some(cwd.into()),
        label: Some(label.into()),
        focus: true,
        env: env.into_iter().collect(),
        source_workspace_id: None,
    })
}
pub(crate) fn workspace_target_params(id: &str) -> Result<Value, String> {
    params(req::WorkspaceTarget {
        workspace_id: id.into(),
    })
}

#[derive(Clone, Debug, Default)]
pub(crate) struct IssueTokens {
    pub panes: std::collections::BTreeMap<String, String>,
    pub workspaces: std::collections::BTreeMap<String, String>,
}

pub(crate) fn issue_tokens(payload: &crate::sidebar::SessionSnapshotPayload) -> IssueTokens {
    fn issue(tokens: &std::collections::BTreeMap<String, Value>) -> Option<String> {
        tokens
            .get("issue")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }
    let mut panes: std::collections::BTreeMap<_, _> = payload
        .agents
        .iter()
        .filter_map(|agent| {
            Some((
                agent.pane_id.as_ref().or(agent.id.as_ref())?.clone(),
                issue(&agent.tokens)?,
            ))
        })
        .collect();
    for pane in &payload.panes {
        if let Some(value) = issue(&pane.tokens) {
            panes.insert(pane.pane_id.clone(), value);
        }
    }
    IssueTokens {
        panes,
        workspaces: payload
            .workspaces
            .iter()
            .filter_map(|workspace| {
                Some((workspace.workspace_id.clone(), issue(&workspace.tokens)?))
            })
            .collect(),
    }
}

pub(crate) fn workspace_issue_params(
    workspace_id: &str,
    issue: Option<&str>,
) -> Result<Value, String> {
    workspace_metadata_params(workspace_id, "issue", issue)
}

pub(crate) fn workspace_purpose_params(
    workspace_id: &str,
    purpose: Option<&str>,
) -> Result<Value, String> {
    workspace_metadata_params(workspace_id, "purpose", purpose)
}

fn workspace_metadata_params(
    workspace_id: &str,
    name: &str,
    purpose: Option<&str>,
) -> Result<Value, String> {
    let key = name
        .try_into()
        .map_err(|error| format!("invalid workspace purpose token key: {error}"))?;
    params(req::WorkspaceReportMetadataParams {
        workspace_id: workspace_id.to_owned(),
        source: HIDE_METADATA_SOURCE.to_owned(),
        tokens: std::collections::HashMap::from([(key, purpose.map(str::to_owned))]),
        seq: None,
        ttl_ms: None,
    })
}
pub(crate) fn tab_target_params(id: &str) -> Result<Value, String> {
    params(req::TabTarget { tab_id: id.into() })
}
pub(crate) fn pane_target_params(id: &str) -> Result<Value, String> {
    params(req::PaneTarget { pane_id: id.into() })
}
pub(crate) fn pane_send_text_params(pane_id: &str, text: &str) -> Result<Value, String> {
    params(req::PaneSendTextParams {
        pane_id: pane_id.into(),
        text: text.into(),
    })
}
/// Moves a pane into a tab of its own inside the workspace it is already in.
///
/// The workspace matters: a delegated child runs in the same working
/// directory as its parent, so a new workspace would split one checkout into
/// two rows describing the same path (PRD D-43). `focus` stays false because
/// the point of the move is that the operator's canvas does not change.
pub(crate) fn pane_move_to_new_tab_params(
    pane_id: &str,
    workspace_id: &str,
    label: &str,
) -> Result<Value, String> {
    params(req::PaneMoveParams {
        pane_id: pane_id.into(),
        destination: req::PaneMoveDestination::NewTab {
            workspace_id: Some(workspace_id.into()),
            label: Some(label.into()),
        },
        focus: false,
    })
}

/// The tab `pane.move` created, when it created one.
///
/// Herdr answers a move it declined with `changed: false` and a reason
/// instead of an error, so a refusal has to be read off the result rather
/// than inferred from a missing field.
pub(crate) fn moved_pane_tab(value: Value) -> Result<String, String> {
    let missing = "pane.move response is missing created_tab.tab_id";
    match response(value, missing)? {
        res::ResponseResult::PaneMove { move_result } => move_outcome(
            move_result.changed,
            move_result.reason.map(|reason| format!("{reason:?}")),
            move_result.created_tab.map(|tab| tab.tab_id),
        ),
        _ => Err(missing.into()),
    }
}

/// The decision `pane.move`'s result carries, apart from decoding it.
///
/// Herdr answers a move it declined with `changed: false` and a reason rather
/// than an error, so a refusal has to be read rather than inferred from a
/// missing field.
pub(crate) fn move_outcome(
    changed: bool,
    reason: Option<String>,
    created_tab_id: Option<String>,
) -> Result<String, String> {
    let missing = "pane.move response is missing created_tab.tab_id";
    if !changed {
        return Err(match reason {
            Some(reason) => format!("Herdr declined the move: {reason}"),
            None => "Herdr declined the move".to_owned(),
        });
    }
    nonempty_id(created_tab_id.ok_or_else(|| missing.to_owned())?, missing)
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
pub(crate) fn worktree_create_params(
    cwd: &str,
    branch: &str,
    base: Option<&str>,
    focus: bool,
) -> Result<Value, String> {
    params(req::WorktreeCreateParams {
        base: base.map(str::to_owned),
        branch: Some(branch.to_owned()),
        cwd: Some(cwd.to_owned()),
        focus,
        label: None,
        // Herdr owns the default checkout location. Hide must never restate it.
        path: None,
        // Keep Herdr's repository trust checks in force.
        trust_repository: None,
        workspace_id: None,
    })
}
pub(crate) fn worktree_list_params(cwd: &str) -> Result<Value, String> {
    params(req::WorktreeListParams {
        cwd: Some(cwd.to_owned()),
        // Listing must not implicitly trust a repository on the user's behalf.
        trust_repository: None,
        workspace_id: None,
    })
}
pub(crate) fn worktree_remove_params(workspace_id: &str) -> Result<Value, String> {
    params(req::WorktreeRemoveParams {
        force: false,
        // Removal must not implicitly trust a repository on the user's behalf.
        trust_repository: None,
        workspace_id: workspace_id.to_owned(),
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

pub(crate) fn pane_split_with_ratio_params(
    pane: &str,
    direction: ClosedSplitDirection,
    cwd: &str,
    ratio: f32,
    env: std::collections::BTreeMap<String, String>,
) -> Result<Value, String> {
    params(req::PaneSplitParams {
        target_pane_id: Some(pane.into()),
        direction: match direction {
            ClosedSplitDirection::Right => req::SplitDirection::Right,
            ClosedSplitDirection::Down => req::SplitDirection::Down,
        },
        cwd: Some(cwd.into()),
        focus: true,
        env: env.into_iter().collect(),
        ratio: Some(ratio),
        right_click: req::PaneRightClickTarget::Herdr,
        workspace_id: None,
    })
}

pub(crate) fn layout_export_params(tab_id: &str) -> Result<Value, String> {
    params(req::LayoutExportParams {
        pane_id: None,
        tab_id: Some(tab_id.into()),
    })
}

pub(crate) fn layout_apply_params(
    workspace_id: &str,
    tab_id: Option<&str>,
    tab_label: &str,
    root: &ClosedLayoutNode,
) -> Result<Value, String> {
    params(req::LayoutApplyParams {
        focus: true,
        root: request_layout_node(root),
        tab_id: tab_id.map(str::to_owned),
        tab_label: Some(tab_label.into()),
        // Herdr accepts either a concrete tab to replace or a workspace in
        // which to create a tab. Sending both makes the pinned server reject
        // an otherwise valid restore of workspace.create's seed tab.
        workspace_id: tab_id.is_none().then(|| workspace_id.into()),
    })
}

pub(crate) fn agent_start_params(
    pane_id: &str,
    name: &str,
    kind: &str,
    args: Vec<String>,
) -> Result<Value, String> {
    params(req::AgentStartParams {
        args,
        kind: kind.into(),
        name: name.into(),
        pane_id: pane_id.into(),
        timeout_ms: Some(120_000),
    })
}

/// A split that creates the pane a fork will run in. Unlike the operator's
/// own split it does not take focus: the operator forked the pane they are
/// reading, and taking focus away from it would undo that.
pub(crate) fn fork_split_params(parent_pane_id: &str, cwd: Option<&str>) -> Result<Value, String> {
    params(req::PaneSplitParams {
        target_pane_id: Some(parent_pane_id.into()),
        direction: req::SplitDirection::Right,
        cwd: cwd.filter(|v| !v.trim().is_empty()).map(Into::into),
        focus: false,
        env: Default::default(),
        ratio: None,
        right_click: req::PaneRightClickTarget::Herdr,
        workspace_id: None,
    })
}

/// Declares `parent_pane_id` as the pane `child_pane_id` was spawned from,
/// under Hide's own metadata source.
pub(crate) fn declare_parent_pane_params(
    child_pane_id: &str,
    parent_pane_id: &str,
) -> Result<Value, String> {
    params(req::PaneReportMetadataParams {
        pane_id: child_pane_id.into(),
        source: HIDE_METADATA_SOURCE.into(),
        tokens: [(
            PARENT_PANE_TOKEN
                .parse()
                .map_err(|error| format!("parent pane token name is invalid: {error}"))?,
            Some(parent_pane_id.to_owned()),
        )]
        .into_iter()
        .collect(),
        agent: None,
        applies_to_source: None,
        clear_display_agent: false,
        clear_state_labels: false,
        clear_title: false,
        display_agent: None,
        seq: None,
        state_labels: Default::default(),
        title: None,
        ttl_ms: None,
    })
}

fn request_layout_node(node: &ClosedLayoutNode) -> req::LayoutNode {
    match node {
        ClosedLayoutNode::Pane {
            pane_id,
            label,
            cwd,
            command,
            env,
        } => req::LayoutNode::Pane {
            pane_id: pane_id.clone(),
            label: label.clone(),
            cwd: cwd.clone(),
            command: command.clone(),
            env: env.clone().into_iter().collect(),
        },
        ClosedLayoutNode::Split {
            direction,
            ratio,
            first,
            second,
        } => req::LayoutNode::Split {
            direction: match direction {
                ClosedSplitDirection::Right => req::SplitDirection::Right,
                ClosedSplitDirection::Down => req::SplitDirection::Down,
            },
            ratio: *ratio,
            first: Box::new(request_layout_node(first)),
            second: Box::new(request_layout_node(second)),
        },
    }
}

fn response_layout_node(node: res::LayoutNode) -> ClosedLayoutNode {
    match node {
        res::LayoutNode::Pane {
            pane_id,
            label,
            cwd,
            command,
            env,
        } => ClosedLayoutNode::Pane {
            pane_id,
            label,
            cwd,
            command,
            env: env.into_iter().collect(),
        },
        res::LayoutNode::Split {
            direction,
            ratio,
            first,
            second,
        } => ClosedLayoutNode::Split {
            direction: match direction {
                res::SplitDirection::Right => ClosedSplitDirection::Right,
                res::SplitDirection::Down => ClosedSplitDirection::Down,
            },
            ratio,
            first: Box::new(response_layout_node(*first)),
            second: Box::new(response_layout_node(*second)),
        },
    }
}

pub(crate) fn exported_layout(value: Value) -> Result<ClosedLayout, String> {
    let missing = "layout.export response is missing layout";
    match response(value, missing)? {
        res::ResponseResult::LayoutExport { layout } => Ok(ClosedLayout {
            workspace_id: layout.workspace_id,
            tab_id: layout.tab_id,
            zoomed: layout.zoomed,
            focused_pane_id: layout.focused_pane_id,
            root: response_layout_node(layout.root),
        }),
        _ => Err(missing.into()),
    }
}

pub(crate) fn applied_layout(value: Value) -> Result<ClosedLayout, String> {
    let missing = "layout.apply response is missing layout";
    match response(value, missing)? {
        res::ResponseResult::LayoutApply { layout } => Ok(ClosedLayout {
            workspace_id: layout.workspace_id,
            tab_id: layout.tab_id,
            zoomed: layout.zoomed,
            focused_pane_id: layout.focused_pane_id,
            root: response_layout_node(layout.root),
        }),
        _ => Err(missing.into()),
    }
}

pub(crate) fn created_workspace(value: Value) -> Result<(String, String, String), String> {
    let missing = "workspace.create response is missing workspace or root pane";
    match response(value, missing)? {
        res::ResponseResult::WorkspaceCreated {
            workspace,
            root_pane,
            tab,
        } => Ok((workspace.workspace_id, tab.tab_id, root_pane.pane_id)),
        _ => Err(missing.into()),
    }
}

pub(crate) fn pane_swap_params(first: &str, second: &str) -> Result<Value, String> {
    params(req::PaneSwapParams {
        direction: None,
        pane_id: None,
        source_pane_id: Some(first.into()),
        target_pane_id: Some(second.into()),
    })
}

pub(crate) fn started_agent(value: Value) -> Result<String, String> {
    let missing = "agent.start response is missing agent pane";
    match response(value, missing)? {
        res::ResponseResult::AgentStarted { agent, .. } => nonempty_id(agent.pane_id, missing),
        _ => Err(missing.into()),
    }
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
/// The active tab a `workspace.get` answer names. Herdr keeps a non-focused
/// workspace's active tab as that workspace's memory and emits no event when
/// a close moves it, so this read is how the replica learns the replacement.
pub(crate) fn workspace_active_tab(value: Value) -> Result<String, String> {
    let missing = "workspace.get response is missing workspace.active_tab_id";
    match response(value, missing)? {
        res::ResponseResult::WorkspaceInfo { workspace } => {
            nonempty_id(workspace.active_tab_id, missing)
        }
        _ => Err(missing.into()),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CreatedWorktree {
    pub workspace_id: String,
    pub pane_id: String,
    pub path: String,
    pub branch: Option<String>,
}

pub(crate) fn created_worktree(value: Value) -> Result<CreatedWorktree, String> {
    let missing = "worktree.create response is missing its created worktree identity";
    match response(value, missing)? {
        res::ResponseResult::WorktreeCreated {
            workspace,
            root_pane,
            worktree,
            ..
        } => Ok(CreatedWorktree {
            workspace_id: nonempty_id(workspace.workspace_id, missing)?,
            pane_id: nonempty_id(root_pane.pane_id, missing)?,
            path: nonempty_id(worktree.path, missing)?,
            branch: worktree.branch,
        }),
        _ => Err(missing.into()),
    }
}

pub(crate) fn listed_worktree_path(value: Value, branch: &str) -> Result<Option<String>, String> {
    let missing = "worktree.list response is malformed";
    match response(value, missing)? {
        res::ResponseResult::WorktreeList { worktrees, .. } => Ok(worktrees
            .into_iter()
            .find(|row| row.branch.as_deref() == Some(branch))
            .map(|row| row.path)),
        _ => Err(missing.into()),
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

/// Reads the identity envelope of a remote Herdr's snapshot. Herdr's stable
/// snapshot names no host and no sequence, so the caller stamps the host it
/// reached the socket through, and agents are identified by the pane they
/// run in.
pub(crate) fn remote_snapshot(
    value: &Value,
    operation: &str,
    host: &crate::domain::HostScope,
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
        snapshot.agents.into_iter().map(|v| v.pane_id),
        "agents",
        "pane_id",
        operation,
    )?;
    Ok(RemoteSnapshotEnvelope {
        host: host.clone(),
        protocol,
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
        return Err(remote_protocol_error(
            operation,
            format!("missing non-empty field {key}"),
        ));
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

/// A wheel carries the pointer's cell and modifiers because Herdr uses them
/// when the application tracks the mouse. Coordinates are zero-based.
pub(crate) fn terminal_scroll_line(
    direction: &str,
    lines: u16,
    column: Option<u16>,
    row: Option<u16>,
    modifiers: u8,
) -> Result<String, String> {
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
        "column": column,
        "row": row,
        "modifiers": modifiers,
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
    fn pane_interrupt_uses_the_generated_literal_text_contract() {
        assert_eq!(
            pane_send_text_params("w1:p1", "\u{3}").unwrap(),
            json!({"pane_id": "w1:p1", "text": "\u{3}"})
        );
    }

    #[test]
    fn a_fork_splits_without_focus_and_declares_its_parent_under_hides_source() {
        assert_eq!(
            fork_split_params("w1:p1", Some("/fixture")).unwrap(),
            json!({"target_pane_id": "w1:p1", "direction": "right", "cwd": "/fixture",
                "focus": false, "right_click": "herdr"})
        );
        assert_eq!(
            fork_split_params("w1:p1", Some("  ")).unwrap()["cwd"],
            Value::Null
        );
        assert_eq!(
            declare_parent_pane_params("w1:p4", "w1:p1").unwrap(),
            json!({"pane_id": "w1:p4", "source": "hide", "tokens": {"parent_pane": "w1:p1"},
                "clear_display_agent": false, "clear_state_labels": false, "clear_title": false})
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
        let remote = remote_snapshot(&snapshot, "probe", &probe_host()).unwrap();
        assert_eq!(remote.pane_ids, ["w1:p1", "w1:p2", "w1:p3"]);
    }

    fn probe_host() -> crate::domain::HostScope {
        crate::domain::HostScope {
            host_id: "probe".to_owned(),
            session_id: "probe".to_owned(),
        }
    }

    #[test]
    fn malformed_remote_snapshots_keep_protocol_priority_and_diagnostic_flags() {
        let value = json!({"snapshot": {"protocol": HERDR_PROTOCOL_REVISION + 1}});
        let error = remote_snapshot(&value, "fixture", &probe_host()).unwrap_err();
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
        let error =
            remote_snapshot(&json!({"snapshot":snapshot}), "fixture", &probe_host()).unwrap_err();
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
        json!({"protocol": HERDR_PROTOCOL_REVISION, "version": "fixture",
            "workspaces": [], "tabs": [], "panes": [], "layouts": [], "agents": []})
    }

    // Herdr's stable event stream has no sequence, so the replica has no cursor
    // and every reconnect reads a fresh snapshot. Should Herdr declare one, the
    // resume path is worth building back: this test names the day.
    #[test]
    fn a_sequenced_event_stream_would_earn_a_resume_cursor() {
        let schema: Value = serde_json::from_str(HERDR_API_SCHEMA_JSON).unwrap();
        let properties = schema["schemas"]["event"]["properties"]
            .as_object()
            .unwrap();
        assert!(
            !properties.contains_key("sequence"),
            "Herdr now sequences its events; resume the subscription from the replica's last sequence instead of re-reading a snapshot on every reconnect"
        );
        let params = schema["schemas"]["request"]["$defs"]["EventsSubscribeParams"]["properties"]
            .as_object()
            .unwrap();
        assert!(
            !params.contains_key("after_sequence"),
            "Herdr now takes a resume cursor; pass the replica's last sequence to events.subscribe"
        );
    }

    #[test]
    fn generated_snapshot_and_response_variants_feed_domain_inputs() {
        let value = empty_snapshot();
        let state = snapshot(value.clone()).unwrap();
        assert!(state.workspaces.is_empty());
        let state =
            snapshot_response(json!({"type": "session_snapshot", "snapshot": value})).unwrap();
        assert!(state.agents.is_empty());
        assert!(
            agents_response(json!({"type": "agent_list", "agents": []}))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn cleanup_preserves_launch_and_foreground_usage_and_unknown_paths() {
        let mut value = empty_snapshot();
        value["panes"] = json!([
            {"pane_id":"w1:p1", "terminal_id":"fixture-terminal", "workspace_id":"w1", "tab_id":"w1:t1", "focused":false, "agent_status":"idle", "revision":1,
             "cwd":"/fixture/main", "foreground_cwd":"/fixture/linked/subdir"},
            {"pane_id":"w1:p2", "terminal_id":"fixture-terminal", "workspace_id":"w1", "tab_id":"w1:t1", "focused":false, "agent_status":"idle", "revision":1}
        ]);
        let paths =
            cleanup_usage_paths(json!({"type":"session_snapshot", "snapshot": value})).unwrap();
        assert_eq!(
            paths,
            vec![
                Some("/fixture/main".into()),
                Some("/fixture/linked/subdir".into()),
                None
            ]
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
    }

    #[test]
    fn incompatible_protocol_retains_typed_revisions_version_and_safe_remedy() {
        let mut value = empty_snapshot();
        let received = HERDR_PROTOCOL_REVISION + 1;
        value["protocol"] = json!(received);
        let error = snapshot(value.clone()).unwrap_err();
        assert_eq!(error.state(), "protocol_mismatch");
        assert_eq!(
            error.message(),
            format!(
                "The running Herdr uses protocol {received}, but this Hide supports protocol {HERDR_PROTOCOL_REVISION}. Update Hide to a compatible release, then try again. No workspace or agent was created."
            )
        );
        assert_eq!(
            error.protocol_details(),
            Some((HERDR_PROTOCOL_REVISION, received, Some("fixture")))
        );

        let received = HERDR_PROTOCOL_REVISION - 1;
        value["protocol"] = json!(received);
        let error = snapshot(value).unwrap_err();
        assert_eq!(
            error.message(),
            format!(
                "The running Herdr uses protocol {received}, but Hide requires protocol {HERDR_PROTOCOL_REVISION}. When your current work is safe, stop the Herdr session and reopen Hide. Hide will start its compatible bundled Herdr. No workspace or agent was created."
            )
        );
    }

    #[test]
    fn generated_subscriptions_keep_the_filter_shape() {
        assert_eq!(
            hide_herdr_client::subscription_params(&["workspace.created", "pane.focused"]).unwrap(),
            json!({"subscriptions": [{"type": "workspace.created"}, {"type": "pane.focused"}]})
        );
        assert!(hide_herdr_client::subscription_params(&["not.a.subscription"]).is_err());
    }

    #[test]
    fn generated_event_payloads_feed_replica_events() {
        let value = json!({"event": "workspace_focused", "data": {"type": "workspace_focused", "workspace_id": "w1"}});
        let SubscriptionLine::Event(event) = parse_subscription_line(&value.to_string()).unwrap()
        else {
            panic!("event expected")
        };
        assert!(
            matches!(event, ReplicaEvent::WorkspaceFocused { workspace_id } if workspace_id == "w1")
        );
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

    fn listed_agent(extra: Value) -> Value {
        let mut agent = json!({"pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t2",
            "terminal_id": "fixture-terminal", "revision": 1, "focused": false, "agent_status": "working"});
        agent
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        json!({"type": "agent_list", "agents": [agent]})
    }

    // sasu starts its implementor with `agent.start` (the only start that can
    // carry the role marker) and declares the Observer's pane as the parent
    // afterwards; without this the row was a root in another checkout and the
    // Observer had no child (2026-09-18).
    #[test]
    fn a_parent_declared_as_a_pane_token_is_the_lineage() {
        let declared =
            agents_response(listed_agent(json!({"tokens": {"parent_pane": "w1:p1"}}))).unwrap();
        assert_eq!(declared[0].spawned_from_pane_id.as_deref(), Some("w1:p1"));
        // The token is still carried verbatim; the sidebar's own token readers are unaffected.
        assert_eq!(declared[0].tokens.get("parent_pane"), Some(&json!("w1:p1")));

        let cleared =
            agents_response(listed_agent(json!({"tokens": {"parent_pane": "  "}}))).unwrap();
        assert_eq!(
            cleared[0].spawned_from_pane_id, None,
            "an empty token is a cleared declaration, not a parent named \"\""
        );

        let silent = agents_response(listed_agent(json!({}))).unwrap();
        assert_eq!(silent[0].spawned_from_pane_id, None);
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
