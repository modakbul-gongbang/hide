//! Event-driven projection of Herdr's authoritative session state.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::BufRead;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::herdr_api::{self, ApiError, HERDR_PROTOCOL_REVISION, HostScope};
use crate::live::{LiveContext, SessionFetchError};
use crate::model::{WorkspaceRegistration, WorkspaceSnapshot};
use crate::runtime::Runtime;
use crate::sidebar::{
    SessionAgentPayload, SessionLayoutPayload, SessionPanePayload, SessionSnapshotPayload,
    SessionTabPayload, SessionWorkspacePayload,
};
use crate::workspace;

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
}

struct CatalogCache {
    registrations: Vec<WorkspaceRegistration>,
    spaces: Vec<workspace::SessionSpace>,
    workspaces: Vec<WorkspaceSnapshot>,
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
            eprintln!(
                "{}",
                json!({
                    "component": "session_sync",
                    "kind": "coordinator.join_failed",
                })
            );
        }
    }
}

enum CoordinatorMessage {
    Stop,
    SubscriptionLine { generation: u64, line: String },
    SubscriptionEnded { generation: u64, message: String },
}

struct ActiveSubscription {
    generation: u64,
    shutdown: UnixStream,
    worker: Option<JoinHandle<()>>,
}

impl ActiveSubscription {
    fn stop(mut self) {
        let _ = self.shutdown.shutdown(Shutdown::Both);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) fn spawn(
    context: LiveContext,
    home_path: Option<std::path::PathBuf>,
) -> Result<SessionSyncHandle, String> {
    let (sender, receiver) = channel();
    let worker_sender = sender.clone();
    let worker = thread::Builder::new()
        .name("herdr-core-session-sync".to_owned())
        .spawn(move || run_coordinator(context, home_path, receiver, worker_sender))
        .map_err(|error| format!("session sync worker could not be started: {error}"))?;
    Ok(SessionSyncHandle {
        sender,
        worker: Some(worker),
    })
}

fn run_coordinator(
    context: LiveContext,
    home_path: Option<std::path::PathBuf>,
    receiver: Receiver<CoordinatorMessage>,
    sender: Sender<CoordinatorMessage>,
) {
    let mut replica: Option<SessionReplica> = None;
    let mut subscription: Option<ActiveSubscription> = None;
    let mut subscription_generation = 0_u64;
    let mut needs_bootstrap = true;
    let mut reconnect_at = Instant::now();
    let mut reconnect_delay = RECONNECT_INITIAL_DELAY;
    let mut next_agent_refresh = Instant::now() + AGENT_REFRESH_INTERVAL;
    let mut catalog_cache: Option<CatalogCache> = None;
    let mut usage_reader = crate::usage::ProviderUsageReader::new(home_path);

    loop {
        if context.runtime.upgrade().is_none() {
            stop_subscription(&mut subscription);
            return;
        }

        if subscription.is_none() && Instant::now() >= reconnect_at {
            let attempt = if needs_bootstrap || replica.is_none() {
                connect_from_snapshot(&context, &sender, &mut subscription_generation).map(
                    |(next_replica, next_subscription)| {
                        replica = Some(next_replica);
                        next_subscription
                    },
                )
            } else {
                connect_from_cursor(
                    &context,
                    replica.as_ref().expect("replica exists without bootstrap"),
                    &sender,
                    &mut subscription_generation,
                )
            };

            match attempt {
                Ok(next_subscription) => {
                    subscription = Some(next_subscription);
                    reconnect_delay = RECONNECT_INITIAL_DELAY;
                    next_agent_refresh = Instant::now() + AGENT_REFRESH_INTERVAL;
                    if replica
                        .as_ref()
                        .is_some_and(SessionReplica::ready_to_publish)
                        && !publish_replica(&context, replica.as_ref().unwrap(), &mut catalog_cache)
                    {
                        stop_subscription(&mut subscription);
                        return;
                    }
                    needs_bootstrap = false;
                }
                Err(failure) => {
                    needs_bootstrap |= failure.needs_bootstrap;
                    if !publish_failure(&context, failure.error) {
                        return;
                    }
                    reconnect_at = Instant::now() + reconnect_delay;
                    reconnect_delay = next_reconnect_delay(reconnect_delay);
                }
            }
        }

        if subscription.is_some() && Instant::now() >= next_agent_refresh {
            next_agent_refresh = Instant::now() + AGENT_REFRESH_INTERVAL;
            match fetch_agents(&context.socket_path) {
                Ok(agents) => {
                    let current = replica
                        .as_mut()
                        .expect("active subscription always has a replica");
                    current.replace_agents(agents);
                    if current.ready_to_publish()
                        && !publish_replica(&context, current, &mut catalog_cache)
                    {
                        stop_subscription(&mut subscription);
                        return;
                    }
                }
                Err(error) => {
                    log_sync_failure("agent_refresh.failed", replica.as_ref(), &error);
                    stop_subscription(&mut subscription);
                    if !publish_failure(&context, stale_if_projected(replica.as_ref(), error)) {
                        return;
                    }
                    reconnect_at = Instant::now() + reconnect_delay;
                    reconnect_delay = next_reconnect_delay(reconnect_delay);
                }
            }
        }

        if let Some(provider_usage) = usage_reader.read_if_due()
            && !publish_provider_usage(&context, provider_usage)
        {
            stop_subscription(&mut subscription);
            return;
        }

        let timeout = coordinator_wait(subscription.is_some(), reconnect_at, next_agent_refresh);
        match receiver.recv_timeout(timeout) {
            Ok(CoordinatorMessage::Stop) => {
                stop_subscription(&mut subscription);
                return;
            }
            Ok(CoordinatorMessage::SubscriptionLine { generation, line }) => {
                if subscription.as_ref().map(|active| active.generation) != Some(generation) {
                    continue;
                }
                match parse_subscription_line(&line) {
                    Ok(SubscriptionLine::Event(event)) => {
                        let current = replica
                            .as_mut()
                            .expect("active subscription always has a replica");
                        match current.apply(event) {
                            Ok(outcome) => {
                                if outcome.refresh_agents {
                                    next_agent_refresh = Instant::now();
                                }
                                if outcome.publish
                                    && !publish_replica(&context, current, &mut catalog_cache)
                                {
                                    stop_subscription(&mut subscription);
                                    return;
                                }
                            }
                            Err(error) => {
                                log_sync_failure("event.rejected", Some(current), &error);
                                stop_subscription(&mut subscription);
                                needs_bootstrap = true;
                                if !publish_failure(&context, error) {
                                    return;
                                }
                                reconnect_at = Instant::now() + reconnect_delay;
                                reconnect_delay = next_reconnect_delay(reconnect_delay);
                            }
                        }
                    }
                    Ok(SubscriptionLine::Error { code, message }) => {
                        let event_gap = code == "event_gap" || code == "event_journal_unavailable";
                        let cursor = replica.as_ref().map(|current| current.cursor);
                        eprintln!(
                            "{}",
                            json!({
                                "component": "session_sync",
                                "kind": "subscription.error",
                                "code": code,
                                "sequence": cursor,
                                "message": message,
                            })
                        );
                        stop_subscription(&mut subscription);
                        needs_bootstrap = event_gap;
                        let error = SessionFetchError::Stale(format!(
                            "Herdr event stream failed with {code}: {message}"
                        ));
                        if !publish_failure(&context, error) {
                            return;
                        }
                        reconnect_at = Instant::now() + reconnect_delay;
                        reconnect_delay = next_reconnect_delay(reconnect_delay);
                    }
                    Err(error) => {
                        log_sync_failure("subscription.malformed", replica.as_ref(), &error);
                        stop_subscription(&mut subscription);
                        needs_bootstrap = true;
                        if !publish_failure(&context, error) {
                            return;
                        }
                        reconnect_at = Instant::now() + reconnect_delay;
                        reconnect_delay = next_reconnect_delay(reconnect_delay);
                    }
                }
            }
            Ok(CoordinatorMessage::SubscriptionEnded {
                generation,
                message,
            }) => {
                if subscription.as_ref().map(|active| active.generation) != Some(generation) {
                    continue;
                }
                stop_subscription(&mut subscription);
                let cursor = replica.as_ref().map(|current| current.cursor).unwrap_or(0);
                let error = SessionFetchError::Stale(format!(
                    "Herdr event stream disconnected after sequence {cursor}: {message}"
                ));
                log_sync_failure("subscription.disconnected", replica.as_ref(), &error);
                if !publish_failure(&context, error) {
                    return;
                }
                reconnect_at = Instant::now() + reconnect_delay;
                reconnect_delay = next_reconnect_delay(reconnect_delay);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                stop_subscription(&mut subscription);
                return;
            }
        }
    }
}

fn connect_from_snapshot(
    context: &LiveContext,
    sender: &Sender<CoordinatorMessage>,
    generation: &mut u64,
) -> Result<(SessionReplica, ActiveSubscription), ConnectFailure> {
    let replica = fetch_replica(&context.socket_path).map_err(|error| ConnectFailure {
        error,
        needs_bootstrap: true,
    })?;
    let subscription = open_subscription(context, &replica, sender, generation)?;
    Ok((replica, subscription))
}

fn connect_from_cursor(
    context: &LiveContext,
    replica: &SessionReplica,
    sender: &Sender<CoordinatorMessage>,
    generation: &mut u64,
) -> Result<ActiveSubscription, ConnectFailure> {
    open_subscription(context, replica, sender, generation)
}

fn open_subscription(
    context: &LiveContext,
    replica: &SessionReplica,
    sender: &Sender<CoordinatorMessage>,
    generation: &mut u64,
) -> Result<ActiveSubscription, ConnectFailure> {
    let subscription = herdr_api::subscribe(
        &context.socket_path,
        replica.cursor,
        TOPOLOGY_SUBSCRIPTIONS,
        SYNC_REQUEST_TIMEOUT,
    )
    .map_err(|error| connect_failure_from_api(error, true, replica.cursor))?;
    if subscription.ack.host != replica.host {
        return Err(ConnectFailure {
            error: SessionFetchError::Stale(format!(
                "Herdr subscription host {:?} does not match snapshot host {:?}",
                subscription.ack.host, replica.host
            )),
            needs_bootstrap: true,
        });
    }
    if subscription.ack.sequence < replica.cursor {
        return Err(ConnectFailure {
            error: SessionFetchError::Stale(format!(
                "Herdr subscription sequence {} is behind snapshot sequence {}",
                subscription.ack.sequence, replica.cursor
            )),
            needs_bootstrap: true,
        });
    }
    *generation = generation.saturating_add(1);
    spawn_subscription_reader(subscription, *generation, sender.clone()).map_err(|message| {
        ConnectFailure {
            error: SessionFetchError::Unreachable(message),
            needs_bootstrap: false,
        }
    })
}

fn spawn_subscription_reader(
    subscription: herdr_api::Subscription,
    generation: u64,
    sender: Sender<CoordinatorMessage>,
) -> Result<ActiveSubscription, String> {
    let (mut reader, shutdown) = subscription.into_parts();
    let worker = thread::Builder::new()
        .name("herdr-core-event-reader".to_owned())
        .spawn(move || {
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _ = sender.send(CoordinatorMessage::SubscriptionEnded {
                            generation,
                            message: "socket reached EOF".to_owned(),
                        });
                        return;
                    }
                    Ok(_) => {
                        if sender
                            .send(CoordinatorMessage::SubscriptionLine { generation, line })
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(CoordinatorMessage::SubscriptionEnded {
                            generation,
                            message: format!("socket read failed: {error}"),
                        });
                        return;
                    }
                }
            }
        })
        .map_err(|error| format!("subscription reader could not be started: {error}"))?;
    Ok(ActiveSubscription {
        generation,
        shutdown,
        worker: Some(worker),
    })
}

fn stop_subscription(subscription: &mut Option<ActiveSubscription>) {
    if let Some(active) = subscription.take() {
        active.stop();
    }
}

fn fetch_replica(socket_path: &Path) -> Result<SessionReplica, SessionFetchError> {
    if !socket_path.exists() {
        return Err(SessionFetchError::SocketMissing(format!(
            "Herdr socket file does not exist at {}; the herdr server is not running",
            socket_path.display()
        )));
    }
    let result = herdr_api::request_with_timeout(
        socket_path,
        "session.snapshot",
        json!({}),
        SYNC_REQUEST_TIMEOUT,
    )
    .map_err(session_error_from_api)?;
    let snapshot = result
        .get("snapshot")
        .ok_or_else(|| SessionFetchError::Malformed("response is missing snapshot".to_owned()))?;
    SessionReplica::from_snapshot(snapshot)
}

fn fetch_agents(socket_path: &Path) -> Result<Vec<WireAgent>, SessionFetchError> {
    if !socket_path.exists() {
        return Err(SessionFetchError::SocketMissing(format!(
            "Herdr socket file does not exist at {}; the herdr server is not running",
            socket_path.display()
        )));
    }
    let result =
        herdr_api::request_with_timeout(socket_path, "agent.list", json!({}), SYNC_REQUEST_TIMEOUT)
            .map_err(session_error_from_api)?;
    let response: AgentListResult = serde_json::from_value(result).map_err(|error| {
        SessionFetchError::Malformed(format!("agent.list response is malformed: {error}"))
    })?;
    if response.kind != "agent_list" {
        return Err(SessionFetchError::Malformed(format!(
            "agent.list returned unexpected result type {:?}",
            response.kind
        )));
    }
    Ok(response.agents)
}

fn publish_replica(
    context: &LiveContext,
    replica: &SessionReplica,
    catalog_cache: &mut Option<CatalogCache>,
) -> bool {
    let payload = replica.project();
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let registrations = match runtime.lock() {
        Ok(guard) => guard.snapshot().ui_state.workspace_registrations.clone(),
        Err(_) => return false,
    };
    drop(runtime);

    let spaces = Runtime::session_spaces(&payload);
    let cache_is_fresh = catalog_cache.as_ref().is_some_and(|cache| {
        cache.registrations == registrations
            && cache.spaces == spaces
            && cache.built_at.elapsed() < CATALOG_REFRESH_INTERVAL
    });
    if !cache_is_fresh {
        let workspaces = workspace::build_catalog(&registrations, &spaces);
        *catalog_cache = Some(CatalogCache {
            registrations: registrations.clone(),
            spaces,
            workspaces,
            built_at: Instant::now(),
        });
    }
    let cache = catalog_cache
        .as_ref()
        .expect("catalog cache is filled on a miss");
    let precomputed = PrecomputedCatalog {
        registrations,
        workspaces: cache.workspaces.clone(),
    };

    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => guard.ingest_session_with_catalog(Ok(payload), Some(precomputed)),
        Err(_) => return false,
    };
    drop(runtime);
    if changed {
        context.notifier.notify();
    }
    true
}

fn publish_failure(context: &LiveContext, error: SessionFetchError) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => guard.ingest_session_with_catalog(Err(error), None),
        Err(_) => return false,
    };
    drop(runtime);
    if changed {
        context.notifier.notify();
    }
    true
}

fn publish_provider_usage(
    context: &LiveContext,
    provider_usage: Vec<crate::model::ProviderUsageSnapshot>,
) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => guard.ingest_provider_usage(provider_usage),
        Err(_) => return false,
    };
    drop(runtime);
    if changed {
        context.notifier.notify();
    }
    true
}

fn coordinator_wait(
    subscribed: bool,
    reconnect_at: Instant,
    next_agent_refresh: Instant,
) -> Duration {
    let now = Instant::now();
    let deadline = if subscribed {
        next_agent_refresh
    } else {
        reconnect_at
    };
    deadline.saturating_duration_since(now)
}

fn next_reconnect_delay(current: Duration) -> Duration {
    current.saturating_mul(2).min(RECONNECT_MAX_DELAY)
}

fn stale_if_projected(
    replica: Option<&SessionReplica>,
    error: SessionFetchError,
) -> SessionFetchError {
    if replica.is_none() || matches!(error, SessionFetchError::Protocol(_)) {
        return error;
    }
    SessionFetchError::Stale(error.message().to_owned())
}

fn session_error_from_api(error: ApiError) -> SessionFetchError {
    match error {
        ApiError::Transport(message) | ApiError::Remote { message, .. } => {
            SessionFetchError::Unreachable(message)
        }
        ApiError::Malformed(message) => SessionFetchError::Malformed(message),
    }
}

fn connect_failure_from_api(error: ApiError, has_projection: bool, cursor: u64) -> ConnectFailure {
    let needs_bootstrap = matches!(
        error.code(),
        Some("event_gap" | "event_journal_unavailable")
    ) || matches!(error, ApiError::Malformed(_));
    let session_error = match error {
        ApiError::Malformed(message) => SessionFetchError::Malformed(message),
        ApiError::Remote { code, message }
            if code == "event_gap" || code == "event_journal_unavailable" =>
        {
            SessionFetchError::Stale(format!(
                "Herdr event stream cannot resume after sequence {cursor}: {message}"
            ))
        }
        ApiError::Transport(message) | ApiError::Remote { message, .. } if has_projection => {
            SessionFetchError::Stale(message)
        }
        ApiError::Transport(message) | ApiError::Remote { message, .. } => {
            SessionFetchError::Unreachable(message)
        }
    };
    ConnectFailure {
        error: session_error,
        needs_bootstrap,
    }
}

fn log_sync_failure(kind: &str, replica: Option<&SessionReplica>, error: &SessionFetchError) {
    eprintln!(
        "{}",
        json!({
            "component": "session_sync",
            "kind": kind,
            "state": error.state(),
            "sequence": replica.map(|current| current.cursor),
            "message": error.message(),
        })
    );
}

struct ConnectFailure {
    error: SessionFetchError,
    needs_bootstrap: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct WorkspaceWire {
    workspace_id: String,
    label: String,
    #[serde(default)]
    active_tab_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct TabWire {
    tab_id: String,
    workspace_id: String,
    #[serde(default)]
    label: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct PaneWire {
    pane_id: String,
    #[serde(default)]
    workspace_id: String,
    #[serde(default)]
    tab_id: String,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct WireAgent {
    pane_id: String,
    #[serde(default)]
    workspace_id: String,
    #[serde(default)]
    tab_id: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    agent_status: Option<String>,
    #[serde(default)]
    tokens: BTreeMap<String, Value>,
    #[serde(default)]
    ambient: Option<Value>,
}

#[derive(Clone, Debug, Deserialize)]
struct ProjectionState {
    #[serde(default)]
    focused_pane_id: Option<String>,
    #[serde(default)]
    workspaces: Vec<WorkspaceWire>,
    #[serde(default)]
    tabs: Vec<TabWire>,
    #[serde(default)]
    panes: Vec<PaneWire>,
    layouts: Vec<SessionLayoutPayload>,
    agents: Vec<WireAgent>,
}

impl ProjectionState {
    fn project(&self) -> SessionSnapshotPayload {
        let workspace_labels = self
            .workspaces
            .iter()
            .map(|workspace| (workspace.workspace_id.as_str(), workspace.label.as_str()))
            .collect::<BTreeMap<_, _>>();
        let agents = self
            .agents
            .iter()
            .map(|agent| {
                let workspace_label = workspace_labels
                    .get(agent.workspace_id.as_str())
                    .copied()
                    .or_else(|| {
                        (!agent.workspace_id.trim().is_empty())
                            .then_some(agent.workspace_id.as_str())
                    })
                    .map(str::to_owned);
                SessionAgentPayload {
                    id: Some(agent.pane_id.clone()),
                    pane_id: Some(agent.pane_id.clone()),
                    workspace_label,
                    cwd: agent.cwd.clone(),
                    agent: agent.agent.clone(),
                    agent_status: agent.agent_status.clone(),
                    tokens: agent.tokens.clone(),
                    ambient: agent.ambient.clone(),
                }
            })
            .collect();
        let tabs = self
            .tabs
            .iter()
            .map(|tab| SessionTabPayload {
                tab_id: tab.tab_id.clone(),
                workspace_id: tab.workspace_id.clone(),
                label: tab.label.clone(),
            })
            .collect();
        let panes = self
            .panes
            .iter()
            .map(|pane| SessionPanePayload {
                pane_id: pane.pane_id.clone(),
                cwd: pane.cwd.clone(),
            })
            .collect();
        let workspaces = workspace_labels
            .into_iter()
            .map(|(workspace_id, label)| SessionWorkspacePayload {
                workspace_id: workspace_id.to_owned(),
                label: label.to_owned(),
            })
            .collect();
        SessionSnapshotPayload {
            focused_pane_id: self.focused_pane_id.clone(),
            tabs,
            layouts: self.layouts.clone(),
            agents,
            panes,
            workspaces,
        }
    }
}

pub(crate) fn project_snapshot(
    snapshot: &Value,
) -> Result<SessionSnapshotPayload, SessionFetchError> {
    validate_protocol(snapshot)?;
    let state: ProjectionState = serde_json::from_value(snapshot.clone()).map_err(|error| {
        SessionFetchError::Malformed(format!("snapshot projection is malformed: {error}"))
    })?;
    Ok(state.project())
}

#[derive(Clone)]
struct SessionReplica {
    host: HostScope,
    cursor: u64,
    state: ProjectionState,
    pending_layouts: BTreeSet<String>,
    last_event: Option<(u64, String)>,
}

#[derive(Debug)]
struct ApplyOutcome {
    publish: bool,
    refresh_agents: bool,
}

impl SessionReplica {
    fn from_snapshot(snapshot: &Value) -> Result<Self, SessionFetchError> {
        validate_protocol(snapshot)?;
        for field in ["workspaces", "tabs", "panes", "layouts", "agents"] {
            if !snapshot.get(field).is_some_and(Value::is_array) {
                return Err(SessionFetchError::Malformed(format!(
                    "snapshot is missing {field}"
                )));
            }
        }
        let host: HostScope =
            serde_json::from_value(snapshot.get("host").cloned().ok_or_else(|| {
                SessionFetchError::Malformed("snapshot is missing host".to_owned())
            })?)
            .map_err(|error| {
                SessionFetchError::Malformed(format!("snapshot host is malformed: {error}"))
            })?;
        let cursor = snapshot
            .get("event_sequence")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                SessionFetchError::Malformed("snapshot is missing event_sequence".to_owned())
            })?;
        let state: ProjectionState = serde_json::from_value(snapshot.clone()).map_err(|error| {
            SessionFetchError::Malformed(format!("snapshot projection is malformed: {error}"))
        })?;
        let replica = Self {
            host,
            cursor,
            state,
            pending_layouts: BTreeSet::new(),
            last_event: None,
        };
        replica.validate()?;
        Ok(replica)
    }

    fn project(&self) -> SessionSnapshotPayload {
        self.state.project()
    }

    fn ready_to_publish(&self) -> bool {
        self.pending_layouts.is_empty()
    }

    fn replace_agents(&mut self, agents: Vec<WireAgent>) {
        self.state.agents = agents;
    }

    fn apply(&mut self, event: SequencedEventEnvelope) -> Result<ApplyOutcome, SessionFetchError> {
        if event.protocol != HERDR_PROTOCOL_REVISION {
            return Err(SessionFetchError::Protocol(format!(
                "Herdr event protocol revision {} does not match required {}",
                event.protocol, HERDR_PROTOCOL_REVISION
            )));
        }
        if event.host != self.host {
            return Err(SessionFetchError::Stale(format!(
                "Herdr event host {:?} does not match snapshot host {:?}",
                event.host, self.host
            )));
        }
        let fingerprint = serde_json::to_string(&event.raw).map_err(|error| {
            SessionFetchError::Malformed(format!("event fingerprint could not be encoded: {error}"))
        })?;
        if event.sequence < self.cursor {
            return Ok(ApplyOutcome {
                publish: false,
                refresh_agents: false,
            });
        }
        if event.sequence == self.cursor {
            if self
                .last_event
                .as_ref()
                .is_some_and(|(sequence, previous)| {
                    *sequence == event.sequence && previous == &fingerprint
                })
            {
                return Ok(ApplyOutcome {
                    publish: false,
                    refresh_agents: false,
                });
            }
            return Err(SessionFetchError::Malformed(format!(
                "Herdr event sequence {} was reused with different content",
                event.sequence
            )));
        }

        let mut candidate = self.clone();
        let refresh_agents = candidate.apply_new_event(&event.event, &event.data)?;
        candidate.cursor = event.sequence;
        candidate.last_event = Some((event.sequence, fingerprint));
        let publish = candidate.ready_to_publish();
        if publish {
            candidate.validate()?;
        }
        *self = candidate;
        Ok(ApplyOutcome {
            publish,
            refresh_agents,
        })
    }

    fn apply_new_event(&mut self, event: &str, data: &Value) -> Result<bool, SessionFetchError> {
        match event {
            "workspace_created" | "workspace_updated" | "workspace_metadata_updated" => {
                let payload: WorkspaceEvent = decode_event_data(data, event)?;
                if event == "workspace_created" && !payload.workspace.active_tab_id.is_empty() {
                    self.pending_layouts
                        .insert(payload.workspace.active_tab_id.clone());
                }
                upsert_workspace(&mut self.state.workspaces, payload.workspace);
            }
            "workspace_renamed" => {
                let payload: WorkspaceRenamedEvent = decode_event_data(data, event)?;
                let workspace = self
                    .state
                    .workspaces
                    .iter_mut()
                    .find(|workspace| workspace.workspace_id == payload.workspace_id)
                    .ok_or_else(|| malformed_event(event, "renamed workspace does not exist"))?;
                workspace.label = payload.label;
            }
            "workspace_moved" | "workspace_reordered" => {
                let payload: WorkspaceListEvent = decode_event_data(data, event)?;
                self.state.workspaces = payload.workspaces;
            }
            "workspace_closed" => {
                let payload: WorkspaceClosedEvent = decode_event_data(data, event)?;
                self.remove_workspace(&payload.workspace_id);
            }
            "workspace_focused" => {
                let payload: WorkspaceIdEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
            }
            "worktree_created" | "worktree_opened" => {
                let payload: WorktreeWorkspaceEvent = decode_event_data(data, event)?;
                upsert_workspace(&mut self.state.workspaces, payload.workspace);
            }
            "worktree_removed" => {
                let payload: WorktreeRemovedEvent = decode_event_data(data, event)?;
                if let Some(workspace) = payload.workspace {
                    upsert_workspace(&mut self.state.workspaces, workspace);
                }
            }
            "tab_created" => {
                let payload: TabEvent = decode_event_data(data, event)?;
                self.pending_layouts.insert(payload.tab.tab_id.clone());
                upsert_tab(&mut self.state.tabs, payload.tab);
            }
            "tab_closed" => {
                let payload: TabIdEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                self.remove_tab(&payload.tab_id);
            }
            "tab_renamed" => {
                let payload: TabRenamedEvent = decode_event_data(data, event)?;
                let tab = self
                    .state
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.tab_id == payload.tab_id)
                    .ok_or_else(|| malformed_event(event, "renamed tab does not exist"))?;
                if tab.workspace_id != payload.workspace_id {
                    return Err(malformed_event(
                        event,
                        "renamed tab belongs to another workspace",
                    ));
                }
                tab.label = payload.label;
            }
            "tab_moved" => {
                let payload: TabListEvent = decode_event_data(data, event)?;
                self.state
                    .tabs
                    .retain(|tab| tab.workspace_id != payload.workspace_id);
                self.state.tabs.extend(payload.tabs);
            }
            "tab_focused" => {
                let payload: TabIdEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                ensure_non_empty(event, "tab_id", &payload.tab_id)?;
            }
            "pane_created" => {
                let payload: PaneEvent = decode_event_data(data, event)?;
                self.pending_layouts.insert(payload.pane.tab_id.clone());
                upsert_pane(&mut self.state.panes, payload.pane);
            }
            "pane_closed" => {
                let payload: PaneClosedEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                if let Some(tab_id) = self
                    .state
                    .panes
                    .iter()
                    .find(|pane| pane.pane_id == payload.pane_id)
                    .map(|pane| pane.tab_id.clone())
                {
                    self.pending_layouts.insert(tab_id);
                }
                self.remove_pane(&payload.pane_id);
            }
            "pane_updated" => {
                let payload: PaneEvent = decode_event_data(data, event)?;
                if let Some(previous) = self
                    .state
                    .panes
                    .iter()
                    .find(|pane| pane.pane_id == payload.pane.pane_id)
                    .cloned()
                    && (previous.tab_id != payload.pane.tab_id
                        || previous.workspace_id != payload.pane.workspace_id)
                {
                    self.pending_layouts.insert(previous.tab_id);
                    self.pending_layouts.insert(payload.pane.tab_id.clone());
                }
                upsert_pane(&mut self.state.panes, payload.pane);
            }
            "pane_focused" => {
                let payload: PaneFocusedEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                if !self
                    .state
                    .panes
                    .iter()
                    .any(|pane| pane.pane_id == payload.pane_id)
                {
                    return Err(malformed_event(event, "focused pane does not exist"));
                }
                self.state.focused_pane_id = Some(payload.pane_id.clone());
                if let Some(layout) = self.state.layouts.iter_mut().find(|layout| {
                    layout
                        .panes
                        .iter()
                        .any(|pane| pane.pane_id == payload.pane_id)
                }) {
                    layout.focused_pane_id = payload.pane_id;
                }
            }
            "pane_moved" => {
                let payload: PaneMovedEvent = decode_event_data(data, event)?;
                self.apply_pane_moved(payload)?;
            }
            "pane_exited" => {
                let payload: PaneClosedEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                ensure_non_empty(event, "pane_id", &payload.pane_id)?;
            }
            "pane_agent_detected" => {
                let payload: PaneClosedEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                ensure_non_empty(event, "pane_id", &payload.pane_id)?;
                return Ok(true);
            }
            "layout_updated" => {
                let payload: LayoutEvent = decode_event_data(data, event)?;
                let tab_id = payload.layout.tab_id.clone();
                let focused_was_missing =
                    self.state.focused_pane_id.as_ref().is_none_or(|focused| {
                        !self.state.panes.iter().any(|pane| &pane.pane_id == focused)
                    });
                upsert_layout(&mut self.state.layouts, payload.layout);
                self.pending_layouts.remove(&tab_id);
                if focused_was_missing
                    && let Some(layout) = self
                        .state
                        .layouts
                        .iter()
                        .find(|layout| layout.tab_id == tab_id)
                {
                    self.state.focused_pane_id = Some(layout.focused_pane_id.clone());
                }
            }
            unknown => {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr subscription emitted unrequested event {unknown:?}"
                )));
            }
        }
        Ok(false)
    }

    fn apply_pane_moved(&mut self, payload: PaneMovedEvent) -> Result<(), SessionFetchError> {
        ensure_non_empty(
            "pane_moved",
            "previous_workspace_id",
            &payload.previous_workspace_id,
        )?;
        if let Some(previous) = self
            .state
            .panes
            .iter()
            .find(|pane| pane.pane_id == payload.previous_pane_id)
            && (previous.workspace_id != payload.previous_workspace_id
                || previous.tab_id != payload.previous_tab_id)
        {
            return Err(malformed_event(
                "pane_moved",
                "previous pane scope does not match the replica",
            ));
        }
        let previous_agent = self
            .state
            .agents
            .iter()
            .find(|agent| agent.pane_id == payload.previous_pane_id)
            .cloned();
        let previous_tab_id = payload.previous_tab_id.clone();
        self.remove_pane(&payload.previous_pane_id);

        if let Some(workspace) = payload.created_workspace {
            upsert_workspace(&mut self.state.workspaces, workspace);
        }
        if let Some(tab) = payload.created_tab {
            self.pending_layouts.insert(tab.tab_id.clone());
            upsert_tab(&mut self.state.tabs, tab);
        }
        if let Some(tab_id) = payload.closed_tab_id.as_deref() {
            self.remove_tab(tab_id);
        } else {
            self.pending_layouts.insert(previous_tab_id);
        }
        if let Some(workspace_id) = payload.closed_workspace_id.as_deref() {
            self.remove_workspace(workspace_id);
        }

        self.pending_layouts.insert(payload.pane.tab_id.clone());
        let pane = payload.pane;
        if let Some(mut agent) = previous_agent {
            agent.pane_id = pane.pane_id.clone();
            agent.workspace_id = pane.workspace_id.clone();
            agent.tab_id = pane.tab_id.clone();
            agent.cwd = pane.cwd.clone();
            self.state
                .agents
                .retain(|candidate| candidate.pane_id != agent.pane_id);
            self.state.agents.push(agent);
        }
        upsert_pane(&mut self.state.panes, pane);
        Ok(())
    }

    fn remove_workspace(&mut self, workspace_id: &str) {
        let tab_ids = self
            .state
            .tabs
            .iter()
            .filter(|tab| tab.workspace_id == workspace_id)
            .map(|tab| tab.tab_id.clone())
            .collect::<HashSet<_>>();
        self.state
            .workspaces
            .retain(|workspace| workspace.workspace_id != workspace_id);
        self.state
            .tabs
            .retain(|tab| tab.workspace_id != workspace_id);
        self.state
            .panes
            .retain(|pane| pane.workspace_id != workspace_id);
        self.state
            .layouts
            .retain(|layout| layout.workspace_id != workspace_id);
        self.state
            .agents
            .retain(|agent| agent.workspace_id != workspace_id);
        self.pending_layouts
            .retain(|tab_id| !tab_ids.contains(tab_id));
        self.clear_missing_focus();
    }

    fn remove_tab(&mut self, tab_id: &str) {
        self.state.tabs.retain(|tab| tab.tab_id != tab_id);
        self.state.panes.retain(|pane| pane.tab_id != tab_id);
        self.state.layouts.retain(|layout| layout.tab_id != tab_id);
        self.state.agents.retain(|agent| agent.tab_id != tab_id);
        self.pending_layouts.remove(tab_id);
        self.clear_missing_focus();
    }

    fn remove_pane(&mut self, pane_id: &str) {
        self.state.panes.retain(|pane| pane.pane_id != pane_id);
        self.state.agents.retain(|agent| agent.pane_id != pane_id);
        self.clear_missing_focus();
    }

    fn clear_missing_focus(&mut self) {
        if self
            .state
            .focused_pane_id
            .as_ref()
            .is_some_and(|focused| !self.state.panes.iter().any(|pane| &pane.pane_id == focused))
        {
            self.state.focused_pane_id = None;
        }
    }

    fn validate(&self) -> Result<(), SessionFetchError> {
        let workspace_ids = unique_ids(
            "workspace",
            self.state
                .workspaces
                .iter()
                .map(|workspace| workspace.workspace_id.as_str()),
        )?;
        let tab_ids = unique_ids("tab", self.state.tabs.iter().map(|tab| tab.tab_id.as_str()))?;
        let pane_ids = unique_ids(
            "pane",
            self.state.panes.iter().map(|pane| pane.pane_id.as_str()),
        )?;
        for tab in &self.state.tabs {
            if !workspace_ids.contains(tab.workspace_id.as_str()) {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr tab {} references missing workspace {}",
                    tab.tab_id, tab.workspace_id
                )));
            }
        }
        let panes_by_id = self
            .state
            .panes
            .iter()
            .map(|pane| (pane.pane_id.as_str(), pane))
            .collect::<HashMap<_, _>>();
        for pane in &self.state.panes {
            if !workspace_ids.contains(pane.workspace_id.as_str())
                || !tab_ids.contains(pane.tab_id.as_str())
            {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr pane {} references missing workspace or tab",
                    pane.pane_id
                )));
            }
        }
        let mut layout_tabs = HashSet::new();
        let mut laid_out_panes = HashSet::new();
        for layout in &self.state.layouts {
            if !layout_tabs.insert(layout.tab_id.as_str()) {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr session contains duplicate layout for tab {}",
                    layout.tab_id
                )));
            }
            if !workspace_ids.contains(layout.workspace_id.as_str())
                || !tab_ids.contains(layout.tab_id.as_str())
            {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr layout {} references missing workspace or tab",
                    layout.tab_id
                )));
            }
            if !layout
                .panes
                .iter()
                .any(|pane| pane.pane_id == layout.focused_pane_id)
            {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr layout {} focuses a pane outside that layout",
                    layout.tab_id
                )));
            }
            for layout_pane in &layout.panes {
                let Some(pane) = panes_by_id.get(layout_pane.pane_id.as_str()) else {
                    return Err(SessionFetchError::Malformed(format!(
                        "Herdr layout {} references missing pane {}",
                        layout.tab_id, layout_pane.pane_id
                    )));
                };
                if pane.tab_id != layout.tab_id || pane.workspace_id != layout.workspace_id {
                    return Err(SessionFetchError::Malformed(format!(
                        "Herdr layout {} contains pane {} from another tab",
                        layout.tab_id, layout_pane.pane_id
                    )));
                }
                if !laid_out_panes.insert(layout_pane.pane_id.as_str()) {
                    return Err(SessionFetchError::Malformed(format!(
                        "Herdr pane {} appears in multiple layouts",
                        layout_pane.pane_id
                    )));
                }
            }
        }
        if let Some(focused) = self.state.focused_pane_id.as_deref()
            && !pane_ids.contains(focused)
        {
            return Err(SessionFetchError::Malformed(format!(
                "Herdr session focuses missing pane {focused}"
            )));
        }
        Ok(())
    }
}

fn validate_protocol(snapshot: &Value) -> Result<(), SessionFetchError> {
    let protocol = snapshot
        .get("protocol")
        .and_then(Value::as_u64)
        .ok_or_else(|| SessionFetchError::Malformed("snapshot is missing protocol".to_owned()))?;
    if protocol != HERDR_PROTOCOL_REVISION {
        return Err(SessionFetchError::Protocol(format!(
            "Herdr protocol revision {protocol} does not match required {HERDR_PROTOCOL_REVISION}"
        )));
    }
    Ok(())
}

fn unique_ids<'a>(
    kind: &str,
    ids: impl Iterator<Item = &'a str>,
) -> Result<HashSet<&'a str>, SessionFetchError> {
    let mut unique = HashSet::new();
    for id in ids {
        if id.trim().is_empty() {
            return Err(SessionFetchError::Malformed(format!(
                "Herdr {kind} id is empty"
            )));
        }
        if !unique.insert(id) {
            return Err(SessionFetchError::Malformed(format!(
                "Herdr session contains duplicate {kind} id {id}"
            )));
        }
    }
    Ok(unique)
}

fn ensure_non_empty(event: &str, field: &str, value: &str) -> Result<(), SessionFetchError> {
    if value.trim().is_empty() {
        Err(malformed_event(event, &format!("has empty {field}")))
    } else {
        Ok(())
    }
}

fn upsert_workspace(workspaces: &mut Vec<WorkspaceWire>, workspace: WorkspaceWire) {
    if let Some(existing) = workspaces
        .iter_mut()
        .find(|existing| existing.workspace_id == workspace.workspace_id)
    {
        *existing = workspace;
    } else {
        workspaces.push(workspace);
    }
}

fn upsert_tab(tabs: &mut Vec<TabWire>, tab: TabWire) {
    if let Some(existing) = tabs
        .iter_mut()
        .find(|existing| existing.tab_id == tab.tab_id)
    {
        *existing = tab;
    } else {
        tabs.push(tab);
    }
}

fn upsert_pane(panes: &mut Vec<PaneWire>, pane: PaneWire) {
    if let Some(existing) = panes
        .iter_mut()
        .find(|existing| existing.pane_id == pane.pane_id)
    {
        *existing = pane;
    } else {
        panes.push(pane);
    }
}

fn upsert_layout(layouts: &mut Vec<SessionLayoutPayload>, layout: SessionLayoutPayload) {
    if let Some(existing) = layouts
        .iter_mut()
        .find(|existing| existing.tab_id == layout.tab_id)
    {
        *existing = layout;
    } else {
        layouts.push(layout);
    }
}

#[derive(Debug, Deserialize)]
struct AgentListResult {
    #[serde(rename = "type")]
    kind: String,
    agents: Vec<WireAgent>,
}

#[derive(Clone, Debug, Deserialize)]
struct SequencedEventEnvelope {
    protocol: u64,
    host: HostScope,
    sequence: u64,
    event: String,
    data: Value,
    #[serde(skip)]
    raw: Value,
}

enum SubscriptionLine {
    Event(SequencedEventEnvelope),
    Error { code: String, message: String },
}

fn parse_subscription_line(line: &str) -> Result<SubscriptionLine, SessionFetchError> {
    if line.trim().is_empty() {
        return Err(SessionFetchError::Malformed(
            "Herdr event stream emitted an empty line".to_owned(),
        ));
    }
    let value: Value = serde_json::from_str(line).map_err(|error| {
        SessionFetchError::Malformed(format!("Herdr event stream emitted invalid JSON: {error}"))
    })?;
    if let Some(error) = value.get("error") {
        let id = value.get("id").and_then(Value::as_str).ok_or_else(|| {
            SessionFetchError::Malformed("Herdr subscription error is missing id".to_owned())
        })?;
        if id != "herdr-core:events.subscribe" {
            return Err(SessionFetchError::Malformed(format!(
                "Herdr subscription error id {id:?} is unexpected"
            )));
        }
        let code = error
            .get("code")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                SessionFetchError::Malformed("Herdr subscription error is missing code".to_owned())
            })?
            .to_owned();
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                SessionFetchError::Malformed(
                    "Herdr subscription error is missing message".to_owned(),
                )
            })?
            .to_owned();
        return Ok(SubscriptionLine::Error { code, message });
    }
    let mut event: SequencedEventEnvelope =
        serde_json::from_value(value.clone()).map_err(|error| {
            SessionFetchError::Malformed(format!("Herdr sequenced event is malformed: {error}"))
        })?;
    event.raw = value;
    Ok(SubscriptionLine::Event(event))
}

fn decode_event_data<T: DeserializeOwned>(
    data: &Value,
    expected_type: &str,
) -> Result<T, SessionFetchError> {
    let actual_type = data.get("type").and_then(Value::as_str).ok_or_else(|| {
        malformed_event(
            expected_type,
            "event data is missing its type discriminator",
        )
    })?;
    if actual_type != expected_type {
        return Err(malformed_event(
            expected_type,
            &format!("event data type is {actual_type:?}"),
        ));
    }
    serde_json::from_value(data.clone()).map_err(|error| {
        malformed_event(expected_type, &format!("event data is malformed: {error}"))
    })
}

fn malformed_event(event: &str, detail: &str) -> SessionFetchError {
    SessionFetchError::Malformed(format!("Herdr {event} event {detail}"))
}

#[derive(Deserialize)]
struct WorkspaceEvent {
    workspace: WorkspaceWire,
}

#[derive(Deserialize)]
struct WorkspaceRenamedEvent {
    workspace_id: String,
    label: String,
}

#[derive(Deserialize)]
struct WorkspaceListEvent {
    workspaces: Vec<WorkspaceWire>,
}

#[derive(Deserialize)]
struct WorkspaceClosedEvent {
    workspace_id: String,
}

#[derive(Deserialize)]
struct WorkspaceIdEvent {
    workspace_id: String,
}

#[derive(Deserialize)]
struct WorktreeWorkspaceEvent {
    workspace: WorkspaceWire,
}

#[derive(Deserialize)]
struct WorktreeRemovedEvent {
    #[serde(default)]
    workspace: Option<WorkspaceWire>,
}

#[derive(Deserialize)]
struct TabEvent {
    tab: TabWire,
}

#[derive(Deserialize)]
struct TabIdEvent {
    workspace_id: String,
    tab_id: String,
}

#[derive(Deserialize)]
struct TabRenamedEvent {
    workspace_id: String,
    tab_id: String,
    label: String,
}

#[derive(Deserialize)]
struct TabListEvent {
    workspace_id: String,
    tabs: Vec<TabWire>,
}

#[derive(Deserialize)]
struct PaneEvent {
    pane: PaneWire,
}

#[derive(Deserialize)]
struct PaneClosedEvent {
    pane_id: String,
    workspace_id: String,
}

#[derive(Deserialize)]
struct PaneFocusedEvent {
    pane_id: String,
    workspace_id: String,
}

#[derive(Deserialize)]
struct PaneMovedEvent {
    previous_pane_id: String,
    previous_workspace_id: String,
    previous_tab_id: String,
    pane: PaneWire,
    #[serde(default)]
    created_workspace: Option<WorkspaceWire>,
    #[serde(default)]
    created_tab: Option<TabWire>,
    #[serde(default)]
    closed_workspace_id: Option<String>,
    #[serde(default)]
    closed_tab_id: Option<String>,
}

#[derive(Deserialize)]
struct LayoutEvent {
    layout: SessionLayoutPayload,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Value {
        json!({
            "version": "0.8.2",
            "protocol": HERDR_PROTOCOL_REVISION,
            "host": {"host_id": "fixture-host", "session_id": "fixture"},
            "event_sequence": 40,
            "focused_pane_id": "w1:p1",
            "workspaces": [{
                "workspace_id": "w1",
                "label": "fixture",
                "active_tab_id": "w1:t1"
            }],
            "tabs": [{
                "workspace_id": "w1",
                "tab_id": "w1:t1",
                "label": "1"
            }],
            "panes": [{
                "workspace_id": "w1",
                "tab_id": "w1:t1",
                "pane_id": "w1:p1",
                "cwd": "/tmp/fixture"
            }],
            "layouts": [{
                "workspace_id": "w1",
                "tab_id": "w1:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 120, "height": 60},
                "focused_pane_id": "w1:p1",
                "panes": [{
                    "pane_id": "w1:p1",
                    "rect": {"x": 0, "y": 0, "width": 120, "height": 60}
                }],
                "splits": []
            }],
            "agents": []
        })
    }

    fn event(sequence: u64, kind: &str, data: Value) -> SequencedEventEnvelope {
        let raw = json!({
            "protocol": HERDR_PROTOCOL_REVISION,
            "host": {"host_id": "fixture-host", "session_id": "fixture"},
            "sequence": sequence,
            "event": kind,
            "data": data
        });
        match parse_subscription_line(&raw.to_string()).expect("event parses") {
            SubscriptionLine::Event(event) => event,
            SubscriptionLine::Error { .. } => unreachable!(),
        }
    }

    #[test]
    fn split_is_published_only_after_the_authoritative_layout_arrives() {
        let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
        let created = replica
            .apply(event(
                41,
                "pane_created",
                json!({
                    "type": "pane_created",
                    "pane": {
                        "workspace_id": "w1",
                        "tab_id": "w1:t1",
                        "pane_id": "w1:p2",
                        "cwd": "/tmp/fixture"
                    }
                }),
            ))
            .expect("pane event");
        assert!(!created.publish);

        let updated = replica
            .apply(event(
                42,
                "layout_updated",
                json!({
                    "type": "layout_updated",
                    "layout": {
                        "workspace_id": "w1",
                        "tab_id": "w1:t1",
                        "zoomed": false,
                        "area": {"x": 0, "y": 0, "width": 120, "height": 60},
                        "focused_pane_id": "w1:p2",
                        "panes": [
                            {"pane_id": "w1:p1", "rect": {"x": 0, "y": 0, "width": 60, "height": 60}},
                            {"pane_id": "w1:p2", "rect": {"x": 60, "y": 0, "width": 60, "height": 60}}
                        ],
                        "splits": [{
                            "direction": "right",
                            "ratio": 0.5,
                            "rect": {"x": 0, "y": 0, "width": 120, "height": 60}
                        }]
                    }
                }),
            ))
            .expect("layout event");
        assert!(updated.publish);
        assert_eq!(replica.project().panes.len(), 2);
        assert_eq!(replica.project().layouts[0].panes.len(), 2);
    }

    #[test]
    fn exact_duplicate_replay_is_idempotent_but_reused_sequence_is_rejected() {
        let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
        let focused = event(
            41,
            "pane_focused",
            json!({
                "type": "pane_focused",
                "workspace_id": "w1",
                "pane_id": "w1:p1"
            }),
        );
        assert!(replica.apply(focused.clone()).expect("first event").publish);
        assert!(!replica.apply(focused).expect("duplicate").publish);

        let changed = event(
            41,
            "workspace_focused",
            json!({
                "type": "workspace_focused",
                "workspace_id": "w1"
            }),
        );
        let error = replica
            .apply(changed)
            .expect_err("sequence reuse must fail");
        assert_eq!(error.state(), "malformed");
    }

    #[test]
    fn event_gap_is_an_explicit_stream_result() {
        let line = json!({
            "id": "herdr-core:events.subscribe",
            "error": {
                "code": "event_gap",
                "message": "fetch session.snapshot and resubscribe"
            }
        })
        .to_string();
        match parse_subscription_line(&line).expect("typed error") {
            SubscriptionLine::Error { code, message } => {
                assert_eq!(code, "event_gap");
                assert!(message.contains("session.snapshot"));
            }
            SubscriptionLine::Event(_) => panic!("expected subscription error"),
        }
    }

    #[test]
    fn filtered_global_sequence_gaps_are_accepted() {
        let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
        let outcome = replica
            .apply(event(
                57,
                "workspace_focused",
                json!({
                    "type": "workspace_focused",
                    "workspace_id": "w1"
                }),
            ))
            .expect("sequence jump is legal");
        assert!(outcome.publish);
        assert_eq!(replica.cursor, 57);
    }

    #[test]
    fn host_and_protocol_mismatch_leave_the_last_projection_unchanged() {
        let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
        let before = replica.project();
        let wrong_host = json!({
            "protocol": HERDR_PROTOCOL_REVISION,
            "host": {"host_id": "another-host", "session_id": "fixture"},
            "sequence": 41,
            "event": "workspace_focused",
            "data": {"type": "workspace_focused", "workspace_id": "w1"}
        });
        let event = match parse_subscription_line(&wrong_host.to_string()).expect("event parses") {
            SubscriptionLine::Event(event) => event,
            SubscriptionLine::Error { .. } => unreachable!(),
        };
        assert_eq!(
            replica.apply(event).expect_err("host mismatch").state(),
            "stale"
        );
        assert_eq!(replica.project().focused_pane_id, before.focused_pane_id);

        let wrong_protocol = json!({
            "protocol": HERDR_PROTOCOL_REVISION + 1,
            "host": {"host_id": "fixture-host", "session_id": "fixture"},
            "sequence": 41,
            "event": "workspace_focused",
            "data": {"type": "workspace_focused", "workspace_id": "w1"}
        });
        let event =
            match parse_subscription_line(&wrong_protocol.to_string()).expect("event parses") {
                SubscriptionLine::Event(event) => event,
                SubscriptionLine::Error { .. } => unreachable!(),
            };
        assert_eq!(
            replica.apply(event).expect_err("protocol mismatch").state(),
            "protocol_mismatch"
        );
        assert_eq!(replica.project().focused_pane_id, before.focused_pane_id);
    }
}
