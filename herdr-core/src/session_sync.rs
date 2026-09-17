//! Event-driven projection of Herdr's authoritative session state.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::wire::{self, parse_subscription_line, protocol_mismatch};

use crate::ffi::ChangeNotifier;
use crate::live::{LiveContext, SessionFetchError};
use crate::model::{
    CheckoutSnapshot, PaneSnapshot, RemotePaneLayoutFrame, RemotePaneLayoutSnapshot,
    RemoteSessionSnapshot, StripTabSnapshot, TabSnapshot, WorkspaceRegistration, WorkspaceSnapshot,
};
use crate::runtime::Runtime;
use crate::sidebar::{
    SessionAgentPayload, SessionLayoutPayload, SessionPanePayload, SessionSnapshotPayload,
    SessionTabPayload, SessionWorkspacePayload,
};
use crate::workspace;
use hide_herdr_client::{self, ApiConnector, ApiError, HERDR_PROTOCOL_REVISION, HostScope};

const SYNC_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);
const AGENT_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const CATALOG_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const RECONNECT_INITIAL_DELAY: Duration = Duration::from_millis(100);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(5);

const TOPOLOGY_SUBSCRIPTIONS: &[&str] = &[
    "workspace.created",
    "workspace.updated",
    "workspace.metadata_updated",
    "workspace.renamed",
    "workspace.moved",
    "workspace.reordered",
    "workspace.closed",
    "workspace.focused",
    "worktree.created",
    "worktree.opened",
    "worktree.removed",
    "tab.created",
    "tab.closed",
    "tab.focused",
    "tab.renamed",
    "tab.moved",
    "pane.created",
    "pane.closed",
    "pane.updated",
    "pane.focused",
    "pane.moved",
    "pane.exited",
    "pane.agent_detected",
    "layout.updated",
];

/// A workspace catalog built outside the runtime mutex, together with the
/// registrations it came from so Runtime can reject a stale computation.
pub struct PrecomputedCatalog {
    pub registrations: Vec<WorkspaceRegistration>,
    pub workspaces: Vec<WorkspaceSnapshot>,
    /// Every pane directory's repository root, so the reconcile that places
    /// tabs into checkouts never asks git while it holds the runtime lock.
    pub roots: workspace::RootIndex,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SessionSyncTarget {
    Local { socket_path: PathBuf },
    Remote { target_id: String, label: String },
}

#[derive(Clone)]
pub(crate) struct SessionSyncContext {
    target: SessionSyncTarget,
    api_connector: Arc<dyn ApiConnector>,
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
}

impl SessionSyncContext {
    pub(crate) fn local(context: &LiveContext) -> Self {
        Self {
            target: SessionSyncTarget::Local {
                socket_path: context.socket_path.clone(),
            },
            api_connector: Arc::clone(&context.api_connector),
            runtime: context.runtime.clone(),
            notifier: context.notifier.clone(),
        }
    }

    pub(crate) fn remote(
        target_id: impl Into<String>,
        label: impl Into<String>,
        api_connector: Arc<dyn ApiConnector>,
        runtime: Weak<Mutex<Runtime>>,
        notifier: ChangeNotifier,
    ) -> Self {
        Self {
            target: SessionSyncTarget::Remote {
                target_id: target_id.into(),
                label: label.into(),
            },
            api_connector,
            runtime,
            notifier,
        }
    }

    fn log_target(&self) -> &str {
        match &self.target {
            SessionSyncTarget::Local { .. } => "local",
            SessionSyncTarget::Remote { target_id, .. } => target_id,
        }
    }

    fn is_local(&self) -> bool {
        matches!(self.target, SessionSyncTarget::Local { .. })
    }
}

pub(crate) struct CatalogCache {
    registrations: Vec<WorkspaceRegistration>,
    spaces: Vec<workspace::SessionSpace>,
    /// The worktrees the cached catalog was built from. Without this a new
    /// worktree, a merge, or a commit would never reach the sidebar: the
    /// registrations and spaces would compare equal and the stale rows would
    /// be republished unchanged.
    worktrees: crate::model::WorktreeCatalogSnapshot,
    workspaces: Vec<WorkspaceSnapshot>,
    roots: workspace::RootIndex,
    built_at: Instant,
}

pub(crate) struct SessionSyncHandle {
    sender: Sender<CoordinatorMessage>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for SessionSyncHandle {
    fn drop(&mut self) {
        let _ = self.sender.send(CoordinatorMessage::Stop);
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            crate::diagnostic!(json!({
                "component": "session_sync",
                "kind": "coordinator.join_failed",
            }));
        }
    }
}

pub(crate) enum CoordinatorMessage {
    Stop,
    SubscriptionLine { generation: u64, line: String },
    SubscriptionEnded { generation: u64, message: String },
}

pub(crate) struct ActiveSubscription {
    generation: u64,
    shutdown: Box<dyn hide_herdr_client::ConnectionShutdown>,
    worker: Option<JoinHandle<()>>,
}

impl ActiveSubscription {
    fn stop(mut self) {
        self.shutdown.shutdown();
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            crate::diagnostic!(json!({
                "component": "session_sync",
                "kind": "subscription_reader.join_failed",
                "generation": self.generation,
            }));
        }
    }
}

mod coordinator;
mod projection;
mod replica;
mod subscription;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use coordinator::agent_tick_needs_publish;
pub(crate) use coordinator::spawn;
pub(crate) use projection::{
    ProjectedAgent, ProjectedPane, ProjectedTab, ProjectedWorkspace, ProjectedWorktree,
    ProjectionState, non_blank, project_snapshot,
};
pub(crate) use replica::{
    PaneMove, ReplicaEnvelope, ReplicaEvent, SessionReplica, SubscriptionLine,
};
#[cfg(test)]
pub(crate) use replica::{SNAPSHOT_FIELDS_THE_REPLICA_READS, remote_tab_id};
#[cfg(test)]
pub(crate) use subscription::connect_failure_from_api;
pub(crate) use subscription::{
    connect_from_cursor, connect_from_snapshot, fetch_agents, log_sync_failure, stop_subscription,
};
