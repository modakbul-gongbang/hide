//! Event-driven projection of Herdr's authoritative session state.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::ffi::ChangeNotifier;
use crate::herdr_api::{self, ApiConnector, ApiError, HERDR_PROTOCOL_REVISION, HostScope};
use crate::live::{LiveContext, SessionFetchError};
use crate::model::{
    CheckoutSnapshot, PaneSnapshot, RemotePaneLayoutFrame, RemotePaneLayoutSnapshot,
    RemoteSessionSnapshot, TabSnapshot, WorkspaceRegistration, WorkspaceSnapshot,
};
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
    shutdown: Box<dyn herdr_api::ConnectionShutdown>,
    worker: Option<JoinHandle<()>>,
}

impl ActiveSubscription {
    fn stop(mut self) {
        self.shutdown.shutdown();
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            eprintln!(
                "{}",
                json!({
                    "component": "session_sync",
                    "kind": "subscription_reader.join_failed",
                    "generation": self.generation,
                })
            );
        }
    }
}

pub(crate) fn spawn(
    context: SessionSyncContext,
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
    context: SessionSyncContext,
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
    let mut usage_reader = context
        .is_local()
        .then(|| crate::usage::ProviderUsageReader::new(home_path));
    // Git reads describe this machine's checkouts, so only the local
    // coordinator runs one.
    let mut changes_reader = context
        .is_local()
        .then(crate::changes::ChangesReader::new);

    loop {
        if context.runtime.upgrade().is_none() {
            stop_subscription(&mut subscription);
            return;
        }

        if subscription.is_none() && Instant::now() >= reconnect_at {
            let has_projection = replica.is_some();
            let attempt = match (needs_bootstrap, replica.as_ref()) {
                (false, Some(current)) => {
                    connect_from_cursor(&context, current, &sender, &mut subscription_generation)
                }
                _ => connect_from_snapshot(
                    &context,
                    &sender,
                    &mut subscription_generation,
                    has_projection,
                )
                .map(|(next_replica, next_subscription)| {
                    replica = Some(next_replica);
                    next_subscription
                }),
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
                    log_sync_failure(&context, "connect.failed", replica.as_ref(), &failure.error);
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
            match fetch_agents(&context) {
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
                    log_sync_failure(&context, "agent_refresh.failed", replica.as_ref(), &error);
                    stop_subscription(&mut subscription);
                    if !publish_failure(&context, stale_if_projected(replica.as_ref(), error)) {
                        return;
                    }
                    reconnect_at = Instant::now() + reconnect_delay;
                    reconnect_delay = next_reconnect_delay(reconnect_delay);
                }
            }
        }

        if let Some(provider_usage) = usage_reader
            .as_mut()
            .and_then(crate::usage::ProviderUsageReader::read_if_due)
            && !publish_provider_usage(&context, provider_usage)
        {
            stop_subscription(&mut subscription);
            return;
        }

        if let Some(reader) = changes_reader.as_mut() {
            // The request is read under a brief lock; the `git` calls that
            // answer it happen after the guard is dropped.
            let Some(request) = read_changes_request(&context) else {
                stop_subscription(&mut subscription);
                return;
            };
            if let Some(changes) = reader.read_if_due(request)
                && !publish_changes(&context, changes)
            {
                stop_subscription(&mut subscription);
                return;
            }
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
                                log_sync_failure(&context, "event.rejected", Some(current), &error);
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
                                "target": context.log_target(),
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
                        log_sync_failure(
                            &context,
                            "subscription.malformed",
                            replica.as_ref(),
                            &error,
                        );
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
                log_sync_failure(
                    &context,
                    "subscription.disconnected",
                    replica.as_ref(),
                    &error,
                );
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
    context: &SessionSyncContext,
    sender: &Sender<CoordinatorMessage>,
    generation: &mut u64,
    has_projection: bool,
) -> Result<(SessionReplica, ActiveSubscription), ConnectFailure> {
    let replica = fetch_replica(context).map_err(|error| ConnectFailure {
        error,
        needs_bootstrap: true,
    })?;
    let subscription = open_subscription(context, &replica, sender, generation, has_projection)?;
    Ok((replica, subscription))
}

fn connect_from_cursor(
    context: &SessionSyncContext,
    replica: &SessionReplica,
    sender: &Sender<CoordinatorMessage>,
    generation: &mut u64,
) -> Result<ActiveSubscription, ConnectFailure> {
    open_subscription(context, replica, sender, generation, true)
}

fn open_subscription(
    context: &SessionSyncContext,
    replica: &SessionReplica,
    sender: &Sender<CoordinatorMessage>,
    generation: &mut u64,
    has_projection: bool,
) -> Result<ActiveSubscription, ConnectFailure> {
    let subscription = herdr_api::subscribe_with_connector(
        context.api_connector.as_ref(),
        replica.cursor,
        TOPOLOGY_SUBSCRIPTIONS,
        SYNC_REQUEST_TIMEOUT,
    )
    .map_err(|error| connect_failure_from_api(error, has_projection, replica.cursor))?;
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

fn fetch_replica(context: &SessionSyncContext) -> Result<SessionReplica, SessionFetchError> {
    if let SessionSyncTarget::Local { socket_path } = &context.target
        && !socket_path.exists()
    {
        return Err(SessionFetchError::SocketMissing(format!(
            "Herdr socket file does not exist at {}; the herdr server is not running",
            socket_path.display()
        )));
    }
    let result = herdr_api::request_with_connector(
        context.api_connector.as_ref(),
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

fn fetch_agents(context: &SessionSyncContext) -> Result<Vec<WireAgent>, SessionFetchError> {
    if let SessionSyncTarget::Local { socket_path } = &context.target
        && !socket_path.exists()
    {
        return Err(SessionFetchError::SocketMissing(format!(
            "Herdr socket file does not exist at {}; the herdr server is not running",
            socket_path.display()
        )));
    }
    let result = herdr_api::request_with_connector(
        context.api_connector.as_ref(),
        "agent.list",
        json!({}),
        SYNC_REQUEST_TIMEOUT,
    )
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
    context: &SessionSyncContext,
    replica: &SessionReplica,
    catalog_cache: &mut Option<CatalogCache>,
) -> bool {
    if let SessionSyncTarget::Remote { target_id, .. } = &context.target {
        let projection = replica.project_remote(target_id);
        let (fetched, excluded) = match projection {
            Ok((session, excluded)) => (Ok(session), excluded),
            Err(error) => (Err(error), Vec::new()),
        };
        for exclusion in excluded {
            eprintln!(
                "{}",
                json!({
                    "component": "remote_session",
                    "kind": "agent.excluded",
                    "target": target_id,
                    "pane_id": exclusion.pane_id,
                    "source_index": exclusion.source_index,
                    "message": exclusion.reason,
                })
            );
        }
        let Some(runtime) = context.runtime.upgrade() else {
            return false;
        };
        let changed = match runtime.lock() {
            Ok(mut guard) => guard.ingest_remote_session(target_id, fetched),
            Err(_) => return false,
        };
        drop(runtime);
        if changed {
            context.notifier.notify();
        }
        return true;
    }

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

fn publish_failure(context: &SessionSyncContext, error: SessionFetchError) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => match &context.target {
            SessionSyncTarget::Local { .. } => guard.ingest_session_with_catalog(Err(error), None),
            SessionSyncTarget::Remote { target_id, .. } => {
                guard.ingest_remote_session(target_id, Err(error))
            }
        },
        Err(_) => return false,
    };
    drop(runtime);
    if changed {
        context.notifier.notify();
    }
    true
}

fn publish_provider_usage(
    context: &SessionSyncContext,
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

/// Reads what the changes view needs, holding the runtime mutex only for the
/// read itself. `None` means the runtime is gone.
fn read_changes_request(
    context: &SessionSyncContext,
) -> Option<Option<crate::changes::ChangesRequest>> {
    let runtime = context.runtime.upgrade()?;
    let request = runtime.lock().ok()?.changes_request();
    drop(runtime);
    Some(request)
}

fn publish_changes(context: &SessionSyncContext, changes: crate::model::ChangesSnapshot) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => guard.ingest_changes(changes),
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

fn log_sync_failure(
    context: &SessionSyncContext,
    kind: &str,
    replica: Option<&SessionReplica>,
    error: &SessionFetchError,
) {
    eprintln!(
        "{}",
        json!({
            "component": "session_sync",
            "kind": kind,
            "target": context.log_target(),
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
    active_tab_id: String,
    #[serde(default)]
    worktree: Option<WorkspaceWorktreeWire>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct WorkspaceWorktreeWire {
    repo_key: String,
    repo_name: String,
    repo_root: String,
    checkout_path: String,
    is_linked_worktree: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct TabWire {
    tab_id: String,
    workspace_id: String,
    label: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct PaneWire {
    pane_id: String,
    workspace_id: String,
    tab_id: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    terminal_title: Option<String>,
    #[serde(default)]
    terminal_title_stripped: Option<String>,
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
    agent_session: Option<crate::sidebar::SessionAgentSessionPayload>,
    #[serde(default)]
    spawned_from_pane_id: Option<String>,
    #[serde(default)]
    state_change_seq: u64,
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
                    agent_session: agent.agent_session.clone(),
                    spawned_from_pane_id: agent.spawned_from_pane_id.clone(),
                    state_change_seq: Some(agent.state_change_seq),
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
                label: non_blank(pane.label.as_deref()),
                terminal_title: non_blank(
                    pane.terminal_title_stripped
                        .as_deref()
                        .or(pane.terminal_title.as_deref()),
                ),
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
    pending_workspace_closures: BTreeSet<String>,
    pending_active_tab_focuses: BTreeSet<String>,
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
        if !snapshot
            .get("version")
            .and_then(Value::as_str)
            .is_some_and(|version| !version.trim().is_empty())
        {
            return Err(SessionFetchError::Malformed(
                "snapshot is missing version".to_owned(),
            ));
        }
        for field in [
            "workspaces",
            "tabs",
            "panes",
            "layouts",
            "agents",
            "lineage",
        ] {
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
        if host.host_id.trim().is_empty() || host.session_id.trim().is_empty() {
            return Err(SessionFetchError::Malformed(
                "snapshot host contains an empty identifier".to_owned(),
            ));
        }
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
            pending_workspace_closures: BTreeSet::new(),
            pending_active_tab_focuses: BTreeSet::new(),
            last_event: None,
        };
        replica.validate()?;
        replica.validate_active_tabs()?;
        Ok(replica)
    }

    fn project(&self) -> SessionSnapshotPayload {
        self.state.project()
    }

    fn project_remote(
        &self,
        target_id: &str,
    ) -> Result<(RemoteSessionSnapshot, Vec<crate::sidebar::AgentExclusion>), SessionFetchError>
    {
        let agent_projection = crate::sidebar::project_agents(self.project());
        let mut agents = agent_projection.agents;

        let mut pane_layouts = Vec::with_capacity(self.state.layouts.len());
        for layout in &self.state.layouts {
            let area_width = f64::from(layout.area.width);
            let area_height = f64::from(layout.area.height);
            if area_width <= 0.0 || area_height <= 0.0 {
                return Err(SessionFetchError::Malformed(format!(
                    "remote Herdr layout {} has an empty area",
                    layout.tab_id
                )));
            }
            let mut frames = Vec::with_capacity(layout.panes.len());
            for pane in &layout.panes {
                let frame = RemotePaneLayoutFrame {
                    pane_id: pane.pane_id.clone(),
                    x: (f64::from(pane.rect.x) - f64::from(layout.area.x)) / area_width,
                    y: (f64::from(pane.rect.y) - f64::from(layout.area.y)) / area_height,
                    width: f64::from(pane.rect.width) / area_width,
                    height: f64::from(pane.rect.height) / area_height,
                };
                if frame.x < 0.0
                    || frame.y < 0.0
                    || frame.width <= 0.0
                    || frame.height <= 0.0
                    || frame.x + frame.width > 1.0
                    || frame.y + frame.height > 1.0
                {
                    return Err(SessionFetchError::Malformed(format!(
                        "remote Herdr layout {} contains an out-of-bounds pane {}",
                        layout.tab_id, pane.pane_id
                    )));
                }
                frames.push(frame);
            }
            pane_layouts.push(RemotePaneLayoutSnapshot {
                workspace_id: remote_workspace_id(target_id, &layout.workspace_id),
                tab_id: remote_tab_id(target_id, &layout.tab_id),
                focused_pane_id: remote_pane_id(target_id, &layout.focused_pane_id),
                zoomed: layout.zoomed,
                frames: frames
                    .into_iter()
                    .map(|mut frame| {
                        frame.pane_id = remote_pane_id(target_id, &frame.pane_id);
                        frame
                    })
                    .collect(),
            });
        }

        let mut workspaces = self
            .state
            .workspaces
            .iter()
            .map(|workspace| {
                let workspace_id = remote_workspace_id(target_id, &workspace.workspace_id);
                let checkout_id = remote_checkout_id(target_id, &workspace.workspace_id);
                let workspace_panes = self
                    .state
                    .panes
                    .iter()
                    .filter(|pane| pane.workspace_id == workspace.workspace_id)
                    .collect::<Vec<_>>();
                let pane_path = workspace_panes
                    .iter()
                    .find_map(|pane| pane.cwd.as_deref().filter(|cwd| !cwd.trim().is_empty()))
                    .map(str::to_owned);
                let path = workspace
                    .worktree
                    .as_ref()
                    .map(|worktree| worktree.checkout_path.clone())
                    .or(pane_path)
                    .unwrap_or_default();
                let repo_name = workspace
                    .worktree
                    .as_ref()
                    .map(|worktree| worktree.repo_name.clone())
                    .unwrap_or_else(|| workspace.label.clone());
                let tabs = self
                    .state
                    .tabs
                    .iter()
                    .filter(|tab| tab.workspace_id == workspace.workspace_id)
                    .map(|tab| {
                        let panes = workspace_panes
                            .iter()
                            .filter(|pane| pane.tab_id == tab.tab_id)
                            .map(|pane| {
                                let agent =
                                    agents.iter().find(|agent| agent.pane_id == pane.pane_id);
                                PaneSnapshot {
                                    id: remote_pane_id(target_id, &pane.pane_id),
                                    herdr_label: non_blank(pane.label.as_deref()),
                                    terminal_title: non_blank(
                                        pane.terminal_title_stripped
                                            .as_deref()
                                            .or(pane.terminal_title.as_deref()),
                                    ),
                                    workspace_label: non_blank(Some(workspace.label.as_str())),
                                    cwd: pane.cwd.clone().unwrap_or_else(|| path.clone()),
                                    state: agent
                                        .map(|agent| agent.state.clone())
                                        .unwrap_or_else(|| "attached".to_owned()),
                                    summary: agent.map(|agent| agent.summary.clone()),
                                    activity_at_unix_ms: self
                                        .state
                                        .agents
                                        .iter()
                                        .find(|source| source.pane_id == pane.pane_id)
                                        .and_then(|source| source.tokens.get("activity"))
                                        .and_then(Value::as_str)
                                        .and_then(|activity| activity.parse().ok()),
                                    fork: crate::runtime::pane_fork_snapshot(agent),
                                }
                            })
                            .collect::<Vec<_>>();
                        TabSnapshot {
                            id: Some(remote_tab_id(target_id, &tab.tab_id)),
                            workspace_id: Some(workspace_id.clone()),
                            checkout_id: Some(checkout_id.clone()),
                            label: Some(tab.label.clone()),
                            empty: panes.is_empty(),
                            panes,
                        }
                    })
                    .collect::<Vec<_>>();
                WorkspaceSnapshot {
                    id: workspace_id.clone(),
                    label: workspace.label.clone(),
                    path: path.clone(),
                    remote_target_id: Some(target_id.to_owned()),
                    expanded: true,
                    device_id: target_id.to_owned(),
                    repo_name,
                    is_git: workspace.worktree.is_some(),
                    default_branch: None,
                    registered: true,
                    temporary: false,
                    checkouts: vec![CheckoutSnapshot {
                        id: checkout_id,
                        workspace_id,
                        label: workspace.label.clone(),
                        path: path.clone(),
                        branch: None,
                        is_worktree: workspace
                            .worktree
                            .as_ref()
                            .is_some_and(|worktree| worktree.is_linked_worktree),
                        exists: !path.is_empty(),
                        temporary: false,
                        tabs,
                    }],
                }
            })
            .collect::<Vec<_>>();
        workspaces.sort_by(|left, right| {
            left.label
                .to_lowercase()
                .cmp(&right.label.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });

        for agent in &mut agents {
            let source_pane_id = agent.pane_id.clone();
            agent.id = format!("remote:{target_id}:agent:{source_pane_id}");
            agent.pane_id = remote_pane_id(target_id, &source_pane_id);
        }

        let active_tab_ids = self
            .state
            .workspaces
            .iter()
            .map(|workspace| {
                (
                    remote_workspace_id(target_id, &workspace.workspace_id),
                    remote_tab_id(target_id, &workspace.active_tab_id),
                )
            })
            .collect();

        let focused = self
            .state
            .focused_pane_id
            .as_deref()
            .and_then(|pane_id| self.state.panes.iter().find(|pane| pane.pane_id == pane_id));
        let focused_workspace_id =
            focused.map(|pane| remote_workspace_id(target_id, &pane.workspace_id));
        let focused_checkout_id =
            focused.map(|pane| remote_checkout_id(target_id, &pane.workspace_id));
        let focused_tab_id = focused.map(|pane| remote_tab_id(target_id, &pane.tab_id));

        Ok((
            RemoteSessionSnapshot {
                workspaces,
                agents,
                active_tab_ids,
                focused_workspace_id,
                focused_checkout_id,
                focused_tab_id,
                focused_pane_id: self
                    .state
                    .focused_pane_id
                    .as_deref()
                    .map(|pane_id| remote_pane_id(target_id, pane_id)),
                pane_layouts,
            },
            agent_projection.excluded,
        ))
    }

    fn ready_to_publish(&self) -> bool {
        self.pending_layouts.is_empty()
            && self.pending_workspace_closures.is_empty()
            && self.pending_active_tab_focuses.is_empty()
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
            "workspace_created" => {
                let payload: WorkspaceEvent = decode_event_data(data, event)?;
                validate_workspace_wire(event, &payload.workspace)?;
                if self
                    .state
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.workspace_id == payload.workspace.workspace_id)
                {
                    return Err(malformed_event(event, "created workspace already exists"));
                }
                self.pending_layouts
                    .insert(payload.workspace.active_tab_id.clone());
                self.state.workspaces.push(payload.workspace);
            }
            "workspace_updated" | "workspace_metadata_updated" => {
                let payload: WorkspaceEvent = decode_event_data(data, event)?;
                validate_workspace_wire(event, &payload.workspace)?;
                let workspace = self
                    .state
                    .workspaces
                    .iter_mut()
                    .find(|workspace| workspace.workspace_id == payload.workspace.workspace_id)
                    .ok_or_else(|| malformed_event(event, "updated workspace does not exist"))?;
                *workspace = payload.workspace;
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
            "workspace_moved" => {
                let payload: WorkspaceMovedEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                for workspace in &payload.workspaces {
                    validate_workspace_wire(event, workspace)?;
                }
                if payload.insert_index > payload.workspaces.len()
                    || !payload
                        .workspaces
                        .iter()
                        .any(|workspace| workspace.workspace_id == payload.workspace_id)
                {
                    return Err(malformed_event(
                        event,
                        "resulting workspace order does not contain the moved workspace at a valid index",
                    ));
                }
                self.state.workspaces = payload.workspaces;
            }
            "workspace_reordered" => {
                let payload: WorkspaceReorderedEvent = decode_event_data(data, event)?;
                for workspace in &payload.workspaces {
                    validate_workspace_wire(event, workspace)?;
                }
                if payload.workspace_ids.is_empty()
                    || payload.workspace_ids.iter().any(|workspace_id| {
                        workspace_id.trim().is_empty()
                            || !payload
                                .workspaces
                                .iter()
                                .any(|workspace| &workspace.workspace_id == workspace_id)
                    })
                {
                    return Err(malformed_event(
                        event,
                        "resulting workspace order does not contain every reordered workspace",
                    ));
                }
                self.state.workspaces = payload.workspaces;
            }
            "workspace_closed" => {
                let payload: WorkspaceClosedEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                if !self
                    .state
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.workspace_id == payload.workspace_id)
                {
                    return Err(malformed_event(event, "closed workspace does not exist"));
                }
                self.remove_workspace(&payload.workspace_id);
            }
            "workspace_focused" => {
                let payload: WorkspaceIdEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                if !self
                    .state
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.workspace_id == payload.workspace_id)
                {
                    return Err(malformed_event(event, "focused workspace does not exist"));
                }
            }
            "worktree_created" => {
                let payload: WorktreeCreatedEvent = decode_event_data(data, event)?;
                validate_workspace_wire(event, &payload.workspace)?;
                upsert_workspace(&mut self.state.workspaces, payload.workspace);
            }
            "worktree_opened" => {
                let payload: WorktreeOpenedEvent = decode_event_data(data, event)?;
                validate_workspace_wire(event, &payload.workspace)?;
                upsert_workspace(&mut self.state.workspaces, payload.workspace);
            }
            "worktree_removed" => {
                let payload: WorktreeRemovedEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                if let Some(workspace) = payload.workspace {
                    validate_workspace_wire(event, &workspace)?;
                    if workspace.workspace_id != payload.workspace_id {
                        return Err(malformed_event(
                            event,
                            "workspace does not match workspace_id",
                        ));
                    }
                    upsert_workspace(&mut self.state.workspaces, workspace);
                }
            }
            "tab_created" => {
                let payload: TabEvent = decode_event_data(data, event)?;
                validate_tab_wire(event, &payload.tab)?;
                if !self
                    .state
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.workspace_id == payload.tab.workspace_id)
                {
                    return Err(malformed_event(
                        event,
                        "created tab references a missing workspace",
                    ));
                }
                if self
                    .state
                    .tabs
                    .iter()
                    .any(|tab| tab.tab_id == payload.tab.tab_id)
                {
                    return Err(malformed_event(event, "created tab already exists"));
                }
                self.pending_layouts.insert(payload.tab.tab_id.clone());
                self.state.tabs.push(payload.tab);
            }
            "tab_closed" => {
                let payload: TabIdEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                let workspace_tab_count = self
                    .state
                    .tabs
                    .iter()
                    .filter(|tab| tab.workspace_id == payload.workspace_id)
                    .count();
                let tab = self
                    .state
                    .tabs
                    .iter()
                    .find(|tab| tab.tab_id == payload.tab_id)
                    .ok_or_else(|| malformed_event(event, "closed tab does not exist"))?;
                if tab.workspace_id != payload.workspace_id {
                    return Err(malformed_event(
                        event,
                        "closed tab belongs to another workspace",
                    ));
                }
                let active_tab_closed = self
                    .state
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == payload.workspace_id)
                    .is_some_and(|workspace| workspace.active_tab_id == payload.tab_id);
                if workspace_tab_count == 1 {
                    self.pending_workspace_closures
                        .insert(payload.workspace_id.clone());
                } else if active_tab_closed {
                    self.pending_active_tab_focuses
                        .insert(payload.workspace_id.clone());
                }
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
                let payload: TabMovedEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                ensure_non_empty(event, "tab_id", &payload.tab_id)?;
                for tab in &payload.tabs {
                    validate_tab_wire(event, tab)?;
                }
                if payload.insert_index > payload.tabs.len()
                    || payload
                        .tabs
                        .iter()
                        .any(|tab| tab.workspace_id != payload.workspace_id)
                    || !payload.tabs.iter().any(|tab| tab.tab_id == payload.tab_id)
                {
                    return Err(malformed_event(
                        event,
                        "resulting tab order is inconsistent with the moved tab",
                    ));
                }
                self.state
                    .tabs
                    .retain(|tab| tab.workspace_id != payload.workspace_id);
                self.state.tabs.extend(payload.tabs);
            }
            "tab_focused" => {
                let payload: TabIdEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                ensure_non_empty(event, "tab_id", &payload.tab_id)?;
                let workspace = self
                    .state
                    .workspaces
                    .iter_mut()
                    .find(|workspace| workspace.workspace_id == payload.workspace_id)
                    .ok_or_else(|| malformed_event(event, "focused workspace does not exist"))?;
                let tab = self
                    .state
                    .tabs
                    .iter()
                    .find(|tab| tab.tab_id == payload.tab_id)
                    .ok_or_else(|| malformed_event(event, "focused tab does not exist"))?;
                if tab.workspace_id != payload.workspace_id {
                    return Err(malformed_event(
                        event,
                        "focused tab belongs to another workspace",
                    ));
                }
                workspace.active_tab_id = payload.tab_id;
                self.pending_active_tab_focuses
                    .remove(&payload.workspace_id);
            }
            "pane_created" => {
                let payload: PaneEvent = decode_event_data(data, event)?;
                validate_pane_wire(event, &payload.pane)?;
                if !self.state.tabs.iter().any(|tab| {
                    tab.tab_id == payload.pane.tab_id
                        && tab.workspace_id == payload.pane.workspace_id
                }) {
                    return Err(malformed_event(
                        event,
                        "created pane references a missing tab",
                    ));
                }
                if self
                    .state
                    .panes
                    .iter()
                    .any(|pane| pane.pane_id == payload.pane.pane_id)
                {
                    return Err(malformed_event(event, "created pane already exists"));
                }
                self.pending_layouts.insert(payload.pane.tab_id.clone());
                self.state.panes.push(payload.pane);
            }
            "pane_closed" => {
                let payload: PaneClosedEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                let pane = self
                    .state
                    .panes
                    .iter()
                    .find(|pane| pane.pane_id == payload.pane_id)
                    .ok_or_else(|| malformed_event(event, "closed pane does not exist"))?;
                if pane.workspace_id != payload.workspace_id {
                    return Err(malformed_event(
                        event,
                        "closed pane belongs to another workspace",
                    ));
                }
                let tab_id = pane.tab_id.clone();
                let last_pane_in_tab = self
                    .state
                    .panes
                    .iter()
                    .filter(|candidate| candidate.tab_id == tab_id)
                    .count()
                    == 1;
                let workspace_tab_count = self
                    .state
                    .tabs
                    .iter()
                    .filter(|tab| tab.workspace_id == payload.workspace_id)
                    .count();
                if last_pane_in_tab && workspace_tab_count > 1 {
                    let active_tab_closed = self
                        .state
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.workspace_id == payload.workspace_id)
                        .is_some_and(|workspace| workspace.active_tab_id == tab_id);
                    if active_tab_closed {
                        self.pending_active_tab_focuses
                            .insert(payload.workspace_id.clone());
                    }
                    // Herdr 0.8.2 removes an emptied tab as part of pane.close
                    // without emitting a separate tab.closed event. Mirror that
                    // authoritative cascade so the replica cannot wait forever
                    // for a layout.updated event for a tab that no longer exists.
                    self.remove_tab(&tab_id);
                } else {
                    self.pending_layouts.insert(tab_id);
                    self.remove_pane(&payload.pane_id);
                }
            }
            "pane_updated" => {
                let payload: PaneEvent = decode_event_data(data, event)?;
                validate_pane_wire(event, &payload.pane)?;
                let previous = self
                    .state
                    .panes
                    .iter()
                    .find(|pane| pane.pane_id == payload.pane.pane_id)
                    .cloned()
                    .ok_or_else(|| malformed_event(event, "updated pane does not exist"))?;
                if previous.tab_id != payload.pane.tab_id
                    || previous.workspace_id != payload.pane.workspace_id
                {
                    self.pending_layouts.insert(previous.tab_id);
                    self.pending_layouts.insert(payload.pane.tab_id.clone());
                }
                upsert_pane(&mut self.state.panes, payload.pane);
            }
            "pane_focused" => {
                let payload: PaneFocusedEvent = decode_event_data(data, event)?;
                ensure_non_empty(event, "workspace_id", &payload.workspace_id)?;
                if !self.state.panes.iter().any(|pane| {
                    pane.pane_id == payload.pane_id && pane.workspace_id == payload.workspace_id
                }) {
                    return Err(malformed_event(
                        event,
                        "focused pane does not exist in the stated workspace",
                    ));
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
                return Ok(true);
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
                ensure_non_empty(event, "workspace_id", &payload.layout.workspace_id)?;
                ensure_non_empty(event, "tab_id", &payload.layout.tab_id)?;
                ensure_non_empty(event, "focused_pane_id", &payload.layout.focused_pane_id)?;
                if !self.state.tabs.iter().any(|tab| {
                    tab.tab_id == payload.layout.tab_id
                        && tab.workspace_id == payload.layout.workspace_id
                }) {
                    return Err(malformed_event(event, "layout references a missing tab"));
                }
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
        ensure_non_empty("pane_moved", "previous_pane_id", &payload.previous_pane_id)?;
        ensure_non_empty(
            "pane_moved",
            "previous_workspace_id",
            &payload.previous_workspace_id,
        )?;
        ensure_non_empty("pane_moved", "previous_tab_id", &payload.previous_tab_id)?;
        validate_pane_wire("pane_moved", &payload.pane)?;
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
            validate_workspace_wire("pane_moved", &workspace)?;
            upsert_workspace(&mut self.state.workspaces, workspace);
        }
        if let Some(tab) = payload.created_tab {
            validate_tab_wire("pane_moved", &tab)?;
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
        self.pending_workspace_closures.remove(workspace_id);
        self.pending_active_tab_focuses.remove(workspace_id);
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

    fn validate_active_tabs(&self) -> Result<(), SessionFetchError> {
        let tabs_by_id = self
            .state
            .tabs
            .iter()
            .map(|tab| (tab.tab_id.as_str(), tab))
            .collect::<HashMap<_, _>>();
        for workspace in &self.state.workspaces {
            let Some(active_tab) = tabs_by_id.get(workspace.active_tab_id.as_str()) else {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr workspace {} references missing active tab {}",
                    workspace.workspace_id, workspace.active_tab_id
                )));
            };
            if active_tab.workspace_id != workspace.workspace_id {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr workspace {} references active tab {} from another workspace",
                    workspace.workspace_id, workspace.active_tab_id
                )));
            }
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), SessionFetchError> {
        for workspace in &self.state.workspaces {
            if let Some(worktree) = &workspace.worktree {
                for (field, value) in [
                    ("repo_key", worktree.repo_key.as_str()),
                    ("repo_name", worktree.repo_name.as_str()),
                    ("repo_root", worktree.repo_root.as_str()),
                    ("checkout_path", worktree.checkout_path.as_str()),
                ] {
                    if value.trim().is_empty() {
                        return Err(SessionFetchError::Malformed(format!(
                            "Herdr workspace {} has an empty worktree {field}",
                            workspace.workspace_id
                        )));
                    }
                }
            }
        }
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
        let tabs_by_id = self
            .state
            .tabs
            .iter()
            .map(|tab| (tab.tab_id.as_str(), tab))
            .collect::<HashMap<_, _>>();
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
            if tabs_by_id
                .get(pane.tab_id.as_str())
                .is_some_and(|tab| tab.workspace_id != pane.workspace_id)
            {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr pane {} references a tab from another workspace",
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
            if tabs_by_id
                .get(layout.tab_id.as_str())
                .is_some_and(|tab| tab.workspace_id != layout.workspace_id)
            {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr layout {} belongs to the wrong workspace",
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
        if let Some(missing_tab_id) = tab_ids
            .iter()
            .find(|tab_id| !layout_tabs.contains(**tab_id))
        {
            return Err(SessionFetchError::Malformed(format!(
                "Herdr tab {missing_tab_id} has no layout"
            )));
        }
        if let Some(missing_pane_id) = pane_ids
            .iter()
            .find(|pane_id| !laid_out_panes.contains(**pane_id))
        {
            return Err(SessionFetchError::Malformed(format!(
                "Herdr pane {missing_pane_id} is absent from every layout"
            )));
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

fn remote_workspace_id(target_id: &str, workspace_id: &str) -> String {
    format!("remote:{target_id}:workspace:{workspace_id}")
}

fn remote_checkout_id(target_id: &str, workspace_id: &str) -> String {
    format!("remote:{target_id}:checkout:{workspace_id}")
}

fn remote_tab_id(target_id: &str, tab_id: &str) -> String {
    format!("remote:{target_id}:tab:{tab_id}")
}

fn remote_pane_id(target_id: &str, pane_id: &str) -> String {
    format!("remote:{target_id}:pane:{pane_id}")
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

fn validate_workspace_wire(
    event: &str,
    workspace: &WorkspaceWire,
) -> Result<(), SessionFetchError> {
    ensure_non_empty(event, "workspace.workspace_id", &workspace.workspace_id)?;
    ensure_non_empty(event, "workspace.active_tab_id", &workspace.active_tab_id)?;
    if let Some(worktree) = &workspace.worktree {
        ensure_non_empty(event, "workspace.worktree.repo_key", &worktree.repo_key)?;
        ensure_non_empty(event, "workspace.worktree.repo_name", &worktree.repo_name)?;
        ensure_non_empty(event, "workspace.worktree.repo_root", &worktree.repo_root)?;
        ensure_non_empty(
            event,
            "workspace.worktree.checkout_path",
            &worktree.checkout_path,
        )?;
    }
    Ok(())
}

fn validate_tab_wire(event: &str, tab: &TabWire) -> Result<(), SessionFetchError> {
    ensure_non_empty(event, "tab.tab_id", &tab.tab_id)?;
    ensure_non_empty(event, "tab.workspace_id", &tab.workspace_id)
}

/// A Herdr label or terminal title that is present but blank carries no more
/// information than an absent one, and a header ladder that treated the two
/// differently would show an empty title instead of falling through.
fn non_blank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn validate_pane_wire(event: &str, pane: &PaneWire) -> Result<(), SessionFetchError> {
    ensure_non_empty(event, "pane.pane_id", &pane.pane_id)?;
    ensure_non_empty(event, "pane.workspace_id", &pane.workspace_id)?;
    ensure_non_empty(event, "pane.tab_id", &pane.tab_id)
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
struct WorkspaceMovedEvent {
    workspace_id: String,
    insert_index: usize,
    workspaces: Vec<WorkspaceWire>,
}

#[derive(Deserialize)]
struct WorkspaceReorderedEvent {
    workspace_ids: Vec<String>,
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
struct WorktreeCreatedEvent {
    workspace: WorkspaceWire,
    #[serde(rename = "worktree")]
    _worktree: Value,
}

#[derive(Deserialize)]
struct WorktreeOpenedEvent {
    workspace: WorkspaceWire,
    #[serde(rename = "worktree")]
    _worktree: Value,
    #[serde(rename = "already_open")]
    _already_open: bool,
}

#[derive(Deserialize)]
struct WorktreeRemovedEvent {
    workspace_id: String,
    #[serde(default)]
    workspace: Option<WorkspaceWire>,
    #[serde(rename = "worktree")]
    _worktree: Value,
    #[serde(rename = "forced")]
    _forced: bool,
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
struct TabMovedEvent {
    workspace_id: String,
    tab_id: String,
    insert_index: usize,
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
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::model::{CoreOptions, SCHEMA_VERSION};

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
            "agents": [],
            "lineage": []
        })
    }

    fn two_tab_snapshot() -> Value {
        let mut value = snapshot();
        value["tabs"]
            .as_array_mut()
            .expect("tabs array")
            .push(json!({
                "workspace_id": "w1",
                "tab_id": "w1:t2",
                "label": "2"
            }));
        value["panes"]
            .as_array_mut()
            .expect("panes array")
            .push(json!({
                "workspace_id": "w1",
                "tab_id": "w1:t2",
                "pane_id": "w1:p2",
                "cwd": "/tmp/fixture"
            }));
        value["layouts"]
            .as_array_mut()
            .expect("layouts array")
            .push(json!({
                "workspace_id": "w1",
                "tab_id": "w1:t2",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 120, "height": 60},
                "focused_pane_id": "w1:p2",
                "panes": [{
                    "pane_id": "w1:p2",
                    "rect": {"x": 0, "y": 0, "width": 120, "height": 60}
                }],
                "splits": []
            }));
        value
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

    fn accept_request(listener: &UnixListener) -> (UnixStream, Value) {
        let (stream, _) = listener.accept().expect("accept request");
        let mut line = String::new();
        std::io::BufReader::new(stream.try_clone().expect("clone request stream"))
            .read_line(&mut line)
            .expect("read request");
        let request = serde_json::from_str(&line).expect("request JSON");
        (stream, request)
    }

    fn write_result(stream: &mut UnixStream, request: &Value, result: Value) {
        writeln!(stream, "{}", json!({"id": request["id"], "result": result}))
            .expect("write response");
    }

    fn wait_until(deadline: Instant, mut predicate: impl FnMut() -> bool) {
        while Instant::now() < deadline {
            if predicate() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(predicate(), "condition did not become true before deadline");
    }

    fn runtime_for_fixture(socket_path: &Path, state_path: &Path) -> Arc<Mutex<Runtime>> {
        Arc::new(Mutex::new(Runtime::new(
            CoreOptions {
                schema_version: SCHEMA_VERSION,
                herdr_socket_path: Some(socket_path.to_string_lossy().into_owned()),
                herdr_bin_path: None,
                remote_targets: Vec::new(),
                app_state_path: state_path.to_string_lossy().into_owned(),
            },
            crate::environment::EnvironmentReport {
                statuses: Vec::new(),
                home_path: None,
                remote_enabled: false,
                chromux_enabled: false,
                herdr_socket_path_override: None,
            },
        )))
    }

    fn context_for_fixture(
        runtime: &Arc<Mutex<Runtime>>,
        socket_path: &Path,
    ) -> SessionSyncContext {
        let live = LiveContext {
            socket_path: socket_path.to_path_buf(),
            herdr_bin: None,
            runtime: Arc::downgrade(runtime),
            notifier: crate::ffi::ChangeNotifier::noop(),
            api_connector: Arc::new(herdr_api::UnixSocketConnector::new(socket_path)),
        };
        SessionSyncContext::local(&live)
    }

    fn remove_fixture(root: &Path, socket_path: &Path, state_path: &Path) {
        if socket_path.exists() {
            std::fs::remove_file(socket_path).expect("remove socket");
        }
        if state_path.exists() {
            std::fs::remove_file(state_path).expect("remove state");
        }
        std::fs::remove_dir(root).expect("remove socket directory");
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
    fn remote_projection_uses_target_scoped_ids_and_normalized_layout_frames() {
        let mut value = snapshot();
        value["agents"] = json!([{
            "pane_id": "w1:p1",
            "workspace_id": "w1",
            "tab_id": "w1:t1",
            "agent": "codex",
            "agent_status": "working",
            "state_change_seq": 1,
            "tokens": {}
        }]);
        let replica = SessionReplica::from_snapshot(&value).expect("snapshot");

        let (projected, excluded) = replica.project_remote("mini").expect("remote projection");

        assert!(excluded.is_empty());
        assert_eq!(projected.workspaces.len(), 1);
        assert_eq!(projected.workspaces[0].id, "remote:mini:workspace:w1");
        assert_eq!(
            projected.workspaces[0].checkouts[0].id,
            "remote:mini:checkout:w1"
        );
        assert_eq!(
            projected.focused_workspace_id.as_deref(),
            Some("remote:mini:workspace:w1")
        );
        assert_eq!(
            projected.focused_tab_id.as_deref(),
            Some("remote:mini:tab:w1:t1")
        );
        assert_eq!(
            projected.focused_pane_id.as_deref(),
            Some("remote:mini:pane:w1:p1")
        );
        assert_eq!(
            projected.workspaces[0].checkouts[0].tabs[0].id.as_deref(),
            Some("remote:mini:tab:w1:t1")
        );
        assert_eq!(
            projected.workspaces[0].checkouts[0].tabs[0].panes[0].id,
            "remote:mini:pane:w1:p1"
        );
        assert_eq!(projected.agents[0].pane_id, "remote:mini:pane:w1:p1");
        assert_eq!(projected.agents[0].state, "working");
        assert_eq!(projected.agents[0].sort_rank, "99");
        assert_eq!(projected.pane_layouts[0].frames[0].x, 0.0);
        assert_eq!(projected.pane_layouts[0].frames[0].width, 1.0);
        assert!(projected.workspaces[0].checkouts[0].exists);
        assert_eq!(
            projected
                .active_tab_ids
                .get("remote:mini:workspace:w1")
                .map(String::as_str),
            Some("remote:mini:tab:w1:t1")
        );

        let (other_target, _) = replica.project_remote("build-mini").expect("other target");
        assert_ne!(
            projected.focused_tab_id, other_target.focused_tab_id,
            "the same remote tab id must not collide across targets"
        );
        assert_ne!(
            projected.focused_pane_id, other_target.focused_pane_id,
            "the same remote pane id must not collide across targets"
        );
        assert_ne!(
            projected.agents[0].pane_id, other_target.agents[0].pane_id,
            "agent routing follows the target-scoped pane identity"
        );
    }

    #[test]
    fn remote_projection_preserves_official_worktree_metadata() {
        let mut value = snapshot();
        value["workspaces"][0]["worktree"] = json!({
            "repo_key": "/tmp/repo/.git",
            "repo_name": "repo",
            "repo_root": "/tmp/repo",
            "checkout_path": "/tmp/repo-linked",
            "is_linked_worktree": true
        });
        value["panes"][0]["cwd"] = json!("/tmp/repo-linked/subdirectory");
        let replica = SessionReplica::from_snapshot(&value).expect("snapshot");

        let (projected, _) = replica.project_remote("mini").expect("remote projection");
        let workspace = &projected.workspaces[0];
        let checkout = &workspace.checkouts[0];

        assert_eq!(workspace.path, "/tmp/repo-linked");
        assert_eq!(workspace.repo_name, "repo");
        assert!(workspace.is_git);
        assert_eq!(checkout.path, "/tmp/repo-linked");
        assert!(checkout.is_worktree);
    }

    #[test]
    fn remote_projection_rejects_an_empty_layout_area() {
        let mut value = snapshot();
        value["layouts"][0]["area"]["width"] = json!(0);
        let replica = SessionReplica::from_snapshot(&value).expect("snapshot shape");

        let error = replica
            .project_remote("mini")
            .expect_err("empty layout area must be visible");

        assert_eq!(error.state(), "malformed");
        assert!(error.message().contains("empty area"));
    }

    #[test]
    #[ignore = "requires HERDR_TEST_SSH_ALIAS and HERDR_TEST_SOCKET_PATH"]
    fn official_remote_session_coordinator_probe() {
        let alias_name = std::env::var("HERDR_TEST_SSH_ALIAS")
            .expect("HERDR_TEST_SSH_ALIAS names a configured SSH host");
        let socket_path = std::env::var("HERDR_TEST_SOCKET_PATH")
            .expect("HERDR_TEST_SOCKET_PATH is the absolute remote Unix socket path");
        let home = std::env::var_os("HOME").expect("HOME is configured");
        let alias = crate::remote::SshAlias::from_config_file(
            &PathBuf::from(home).join(".ssh/config"),
            &alias_name,
        )
        .expect("SSH alias resolves");
        let client =
            crate::remote::RusshRemoteClient::new(alias).expect("remote client initializes");
        let connector = client
            .herdr_api_connector(&socket_path)
            .expect("remote connector initializes");
        let state_path = PathBuf::from("/tmp/herdr-core-remote-coordinator-probe-state.json");
        let runtime = Arc::new(Mutex::new(Runtime::new(
            CoreOptions {
                schema_version: SCHEMA_VERSION,
                herdr_socket_path: None,
                herdr_bin_path: None,
                remote_targets: vec![crate::model::RemoteTarget {
                    id: "mini".to_owned(),
                    label: "Mac mini".to_owned(),
                    ssh_alias: alias_name,
                    herdr_socket_path: socket_path,
                }],
                app_state_path: state_path.to_string_lossy().into_owned(),
            },
            crate::environment::EnvironmentReport {
                statuses: Vec::new(),
                home_path: None,
                remote_enabled: true,
                chromux_enabled: false,
                herdr_socket_path_override: None,
            },
        )));
        let context = SessionSyncContext::remote(
            "mini",
            "Mac mini",
            Arc::new(connector),
            Arc::downgrade(&runtime),
            crate::ffi::ChangeNotifier::noop(),
        );
        let handle = spawn(context, None).expect("remote coordinator starts");

        wait_until(Instant::now() + Duration::from_secs(5), || {
            runtime.lock().ok().is_some_and(|runtime| {
                let status = &runtime.snapshot().status.remote[0];
                status.state == "connected" && status.session.is_some()
            })
        });
        let runtime = runtime.lock().expect("runtime lock");
        let status = &runtime.snapshot().status.remote[0];
        assert_eq!(status.state, "connected");
        let session = status.session.as_ref().expect("remote session projected");
        assert!(!session.agents.is_empty());
        assert_eq!(
            runtime.snapshot().navigator.devices[1].agent_count as usize,
            session.agents.len()
        );
        drop(runtime);
        drop(handle);
    }

    #[test]
    fn workspace_close_cascade_clears_pending_layout_and_nested_state() {
        let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
        let pane_closed = replica
            .apply(event(
                41,
                "pane_closed",
                json!({
                    "type": "pane_closed",
                    "pane_id": "w1:p1",
                    "workspace_id": "w1"
                }),
            ))
            .expect("pane close event");
        assert!(!pane_closed.publish);

        let workspace_closed = replica
            .apply(event(
                42,
                "workspace_closed",
                json!({
                    "type": "workspace_closed",
                    "workspace_id": "w1"
                }),
            ))
            .expect("workspace close event");
        assert!(workspace_closed.publish);
        assert!(replica.ready_to_publish());

        let projected = replica.project();
        assert!(projected.workspaces.is_empty());
        assert!(projected.tabs.is_empty());
        assert!(projected.panes.is_empty());
        assert!(projected.layouts.is_empty());
        assert_eq!(projected.focused_pane_id, None);
    }

    #[test]
    fn last_pane_close_removes_an_implicitly_closed_inactive_tab() {
        let mut replica =
            SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");

        let pane_closed = replica
            .apply(event(
                41,
                "pane_closed",
                json!({
                    "type": "pane_closed",
                    "pane_id": "w1:p2",
                    "workspace_id": "w1"
                }),
            ))
            .expect("last pane close event");

        assert!(pane_closed.publish);
        assert!(replica.ready_to_publish());
        let projected = replica.project();
        assert_eq!(
            projected
                .tabs
                .iter()
                .map(|tab| tab.tab_id.as_str())
                .collect::<Vec<_>>(),
            vec!["w1:t1"]
        );
        assert!(projected.panes.iter().all(|pane| pane.pane_id != "w1:p2"));
        assert!(
            projected
                .layouts
                .iter()
                .all(|layout| layout.tab_id != "w1:t2")
        );
    }

    #[test]
    fn last_pane_close_waits_for_the_authoritative_fallback_tab_focus() {
        let mut replica =
            SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");

        let pane_closed = replica
            .apply(event(
                41,
                "pane_closed",
                json!({
                    "type": "pane_closed",
                    "pane_id": "w1:p1",
                    "workspace_id": "w1"
                }),
            ))
            .expect("last pane close event");
        assert!(!pane_closed.publish);
        assert!(!replica.ready_to_publish());

        let workspace_focused = replica
            .apply(event(
                42,
                "workspace_focused",
                json!({
                    "type": "workspace_focused",
                    "workspace_id": "w1"
                }),
            ))
            .expect("workspace focus event");
        assert!(!workspace_focused.publish);

        let tab_focused = replica
            .apply(event(
                43,
                "tab_focused",
                json!({
                    "type": "tab_focused",
                    "tab_id": "w1:t2",
                    "workspace_id": "w1"
                }),
            ))
            .expect("fallback tab focus event");
        assert!(tab_focused.publish);
        assert!(replica.ready_to_publish());
        assert_eq!(replica.state.workspaces[0].active_tab_id, "w1:t2");
        assert!(
            replica
                .project()
                .tabs
                .iter()
                .all(|tab| tab.tab_id != "w1:t1")
        );
    }

    #[test]
    fn active_tab_close_waits_for_the_authoritative_fallback_tab_focus() {
        let mut replica =
            SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");

        let tab_closed = replica
            .apply(event(
                41,
                "tab_closed",
                json!({
                    "type": "tab_closed",
                    "tab_id": "w1:t1",
                    "workspace_id": "w1"
                }),
            ))
            .expect("active tab close event");
        assert!(!tab_closed.publish);
        assert!(!replica.ready_to_publish());

        let tab_focused = replica
            .apply(event(
                42,
                "tab_focused",
                json!({
                    "type": "tab_focused",
                    "tab_id": "w1:t2",
                    "workspace_id": "w1"
                }),
            ))
            .expect("fallback tab focus event");
        assert!(tab_focused.publish);
        assert!(replica.ready_to_publish());
        assert_eq!(replica.state.workspaces[0].active_tab_id, "w1:t2");
    }

    #[test]
    fn last_tab_close_waits_for_the_workspace_close_cascade() {
        let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
        let tab_closed = replica
            .apply(event(
                41,
                "tab_closed",
                json!({
                    "type": "tab_closed",
                    "tab_id": "w1:t1",
                    "workspace_id": "w1"
                }),
            ))
            .expect("tab close event");
        assert!(!tab_closed.publish);

        let workspace_closed = replica
            .apply(event(
                42,
                "workspace_closed",
                json!({
                    "type": "workspace_closed",
                    "workspace_id": "w1"
                }),
            ))
            .expect("workspace close event");
        assert!(workspace_closed.publish);
        assert!(replica.ready_to_publish());
        assert!(replica.project().workspaces.is_empty());
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
    fn subscription_failure_is_stale_only_when_a_projection_already_exists() {
        let initial =
            connect_failure_from_api(ApiError::Transport("socket closed".to_owned()), false, 40);
        assert_eq!(initial.error.state(), "unreachable");

        let reconnect =
            connect_failure_from_api(ApiError::Transport("socket closed".to_owned()), true, 40);
        assert_eq!(reconnect.error.state(), "stale");
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

    #[test]
    fn coordinator_resumes_from_the_last_event_without_fetching_another_snapshot() {
        let root = Path::new("/tmp").join(format!(
            "herdr-core-session-resume-contract-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create socket directory");
        let socket_path = root.join("herdr.sock");
        let state_path = root.join("state.json");
        let listener = UnixListener::bind(&socket_path).expect("bind fake Herdr socket");
        let resumed = Arc::new(AtomicBool::new(false));
        let resumed_from_server = Arc::clone(&resumed);
        let server = thread::spawn(move || {
            let (mut snapshot_stream, snapshot_request) = accept_request(&listener);
            assert_eq!(snapshot_request["method"], "session.snapshot");
            write_result(
                &mut snapshot_stream,
                &snapshot_request,
                json!({"type": "session_snapshot", "snapshot": snapshot()}),
            );

            let (mut first_subscription, first_subscribe_request) = accept_request(&listener);
            assert_eq!(first_subscribe_request["method"], "events.subscribe");
            assert_eq!(first_subscribe_request["params"]["after_sequence"], 40);
            write_result(
                &mut first_subscription,
                &first_subscribe_request,
                json!({
                    "type": "subscription_started",
                    "host": {"host_id": "fixture-host", "session_id": "fixture"},
                    "sequence": 40,
                    "oldest_available_sequence": 1
                }),
            );
            writeln!(
                first_subscription,
                "{}",
                json!({
                    "protocol": HERDR_PROTOCOL_REVISION,
                    "host": {"host_id": "fixture-host", "session_id": "fixture"},
                    "sequence": 41,
                    "event": "workspace_focused",
                    "data": {"type": "workspace_focused", "workspace_id": "w1"}
                })
            )
            .expect("write replayable event");
            drop(first_subscription);

            let (mut resumed_subscription, resumed_request) = accept_request(&listener);
            assert_eq!(
                resumed_request["method"], "events.subscribe",
                "a clean disconnect must resume the cursor instead of fetching a snapshot"
            );
            assert_eq!(resumed_request["params"]["after_sequence"], 41);
            write_result(
                &mut resumed_subscription,
                &resumed_request,
                json!({
                    "type": "subscription_started",
                    "host": {"host_id": "fixture-host", "session_id": "fixture"},
                    "sequence": 41,
                    "oldest_available_sequence": 1
                }),
            );
            resumed_from_server.store(true, Ordering::Release);
            let mut byte = [0_u8; 1];
            assert_eq!(
                resumed_subscription
                    .read(&mut byte)
                    .expect("wait for shutdown"),
                0
            );
        });

        let runtime = runtime_for_fixture(&socket_path, &state_path);
        let context = context_for_fixture(&runtime, &socket_path);
        let handle = spawn(context, None).expect("start session sync");
        wait_until(Instant::now() + Duration::from_secs(3), || {
            resumed.load(Ordering::Acquire)
                && runtime
                    .lock()
                    .expect("runtime lock")
                    .snapshot()
                    .status
                    .herdr
                    .state
                    == "connected"
        });

        drop(handle);
        server.join().expect("fake server joins");
        remove_fixture(&root, &socket_path, &state_path);
    }

    #[test]
    fn coordinator_recovers_event_gap_with_one_fresh_snapshot_and_stops_its_reader() {
        let root = Path::new("/tmp").join(format!(
            "herdr-core-session-gap-contract-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create socket directory");
        let socket_path = root.join("herdr.sock");
        let state_path = root.join("state.json");
        let listener = UnixListener::bind(&socket_path).expect("bind fake Herdr socket");
        let server = thread::spawn(move || {
            let (mut first_snapshot_stream, first_snapshot_request) = accept_request(&listener);
            assert_eq!(first_snapshot_request["method"], "session.snapshot");
            write_result(
                &mut first_snapshot_stream,
                &first_snapshot_request,
                json!({"type": "session_snapshot", "snapshot": snapshot()}),
            );

            let (mut first_subscription, first_subscribe_request) = accept_request(&listener);
            assert_eq!(first_subscribe_request["method"], "events.subscribe");
            assert_eq!(first_subscribe_request["params"]["after_sequence"], 40);
            write_result(
                &mut first_subscription,
                &first_subscribe_request,
                json!({
                    "type": "subscription_started",
                    "host": {"host_id": "fixture-host", "session_id": "fixture"},
                    "sequence": 40,
                    "oldest_available_sequence": 1
                }),
            );
            writeln!(
                first_subscription,
                "{}",
                json!({
                    "id": "herdr-core:events.subscribe",
                    "error": {
                        "code": "event_gap",
                        "message": "fetch session.snapshot and resubscribe"
                    }
                })
            )
            .expect("write event gap");
            drop(first_subscription);

            let (mut second_snapshot_stream, second_snapshot_request) = accept_request(&listener);
            assert_eq!(second_snapshot_request["method"], "session.snapshot");
            let mut recovered = snapshot();
            recovered["event_sequence"] = json!(50);
            recovered["workspaces"][0]["label"] = json!("recovered");
            write_result(
                &mut second_snapshot_stream,
                &second_snapshot_request,
                json!({"type": "session_snapshot", "snapshot": recovered}),
            );

            let (mut final_subscription, final_subscribe_request) = accept_request(&listener);
            assert_eq!(final_subscribe_request["method"], "events.subscribe");
            assert_eq!(final_subscribe_request["params"]["after_sequence"], 50);
            write_result(
                &mut final_subscription,
                &final_subscribe_request,
                json!({
                    "type": "subscription_started",
                    "host": {"host_id": "fixture-host", "session_id": "fixture"},
                    "sequence": 50,
                    "oldest_available_sequence": 1
                }),
            );
            let mut byte = [0_u8; 1];
            assert_eq!(
                final_subscription
                    .read(&mut byte)
                    .expect("wait for shutdown"),
                0
            );
        });

        let runtime = runtime_for_fixture(&socket_path, &state_path);
        let context = context_for_fixture(&runtime, &socket_path);
        let handle = spawn(context, None).expect("start session sync");
        wait_until(Instant::now() + Duration::from_secs(3), || {
            let snapshot = runtime.lock().expect("runtime lock").snapshot().clone();
            snapshot.status.herdr.state == "connected"
                && snapshot
                    .navigator
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.label == "recovered")
        });

        drop(handle);
        server.join().expect("fake server joins");
        remove_fixture(&root, &socket_path, &state_path);
    }
}
