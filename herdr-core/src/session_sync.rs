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
use crate::herdr_api::{self, ApiConnector, ApiError, HERDR_PROTOCOL_REVISION, HostScope};
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

struct CatalogCache {
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
    let mut changes_reader = context.is_local().then(crate::changes::ChangesReader::new);
    // A listening port is this machine's, so only the local coordinator looks.
    let mut ports_reader = context.is_local().then(crate::ports::PortsReader::new);
    // The three project-panel readers describe this machine's repositories:
    // its worktrees, its `gh` login's view of their pull requests, and one
    // checkout's size on this disk. All three run their subprocess on a worker
    // thread, so a slow `gh` or `du` costs no coordinator latency.
    let mut worktree_reader = context
        .is_local()
        .then(crate::worktrees::WorktreeReader::new);
    let mut github_reader = context.is_local().then(crate::github::GithubReader::new);
    let mut disk_reader = context.is_local().then(crate::disk::DiskReader::new);

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
                    let publish =
                        agent_tick_needs_publish(current, &agents, catalog_cache.as_ref());
                    if publish {
                        let stopped_in = current.replace_agents(agents);
                        if !stopped_in.is_empty()
                            && !request_pull_request_refresh(&context, &stopped_in)
                        {
                            stop_subscription(&mut subscription);
                            return;
                        }
                        if current.ready_to_publish()
                            && !publish_replica(&context, current, &mut catalog_cache)
                        {
                            stop_subscription(&mut subscription);
                            return;
                        }
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

        if let Some(reader) = ports_reader.as_mut() {
            // `lsof` runs here, outside every lock; only the result is handed
            // in.
            if let Some(ports) = reader.read_if_due()
                && !publish_ports(&context, ports)
            {
                stop_subscription(&mut subscription);
                return;
            }
        }

        if let Some(reader) = worktree_reader.as_mut() {
            let Some(request) = read_worktrees_request(&context) else {
                stop_subscription(&mut subscription);
                return;
            };
            if let Some(catalog) = reader.read_if_due(request) {
                // New worktree facts change which rows exist and what they
                // say, so the catalog is rebuilt rather than only stored.
                match publish_worktrees(&context, catalog) {
                    None => {
                        stop_subscription(&mut subscription);
                        return;
                    }
                    Some(true) => {
                        if let Some(current) = replica.as_ref()
                            && current.ready_to_publish()
                            && !publish_replica(&context, current, &mut catalog_cache)
                        {
                            stop_subscription(&mut subscription);
                            return;
                        }
                    }
                    Some(false) => {}
                }
            }
        }

        if let Some(reader) = github_reader.as_mut() {
            let Some(request) = read_github_request(&context) else {
                stop_subscription(&mut subscription);
                return;
            };
            if let Some(github) = reader.read_if_due(request)
                && !publish_github(&context, github)
            {
                stop_subscription(&mut subscription);
                return;
            }
        }

        if let Some(reader) = disk_reader.as_mut() {
            let Some(request) = read_disk_request(&context) else {
                stop_subscription(&mut subscription);
                return;
            };
            if let Some(disk) = reader.read_if_due(request)
                && !publish_disk_usage(&context, disk)
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
    SessionReplica::from_decoded(wire::snapshot_response(result)?)
}

fn fetch_agents(context: &SessionSyncContext) -> Result<Vec<ProjectedAgent>, SessionFetchError> {
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
    wire::agents_response(result)
}

/// Whether an agent refresh has to republish the projection.
///
/// An `agent.list` identical to the one already held projects to the same
/// sidebar, so recomputing it would rebuild the whole projection and the
/// workspace catalog for a wire that did not move. `ProjectedAgent` is exactly the
/// projection's input - deserializing already drops the fields the projection
/// never reads - so equality here is equality of the projection.
///
/// The catalog is the other reason a tick must publish. It is rebuilt inside
/// `publish_replica` on its own refresh window, and on an idle session this
/// tick is the only thing that calls it, so a skip that ignored the window
/// would freeze every branch and dirty mark in the navigator.
fn agent_tick_needs_publish(
    replica: &SessionReplica,
    agents: &[ProjectedAgent],
    catalog_cache: Option<&CatalogCache>,
) -> bool {
    replica.state.agents != agents
        || catalog_cache.is_none_or(|cache| cache.built_at.elapsed() >= CATALOG_REFRESH_INTERVAL)
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
    let (registrations, worktrees, scratch_root) = match runtime.lock() {
        Ok(guard) => (
            guard.snapshot().ui_state.workspace_registrations.clone(),
            guard.worktree_catalog(),
            guard.scratch_root(),
        ),
        Err(_) => return false,
    };
    drop(runtime);

    let spaces = Runtime::session_spaces(&payload, &scratch_root);
    let cache_is_fresh = catalog_cache.as_ref().is_some_and(|cache| {
        cache.registrations == registrations
            && cache.spaces == spaces
            && cache.worktrees == worktrees
            && cache.built_at.elapsed() < CATALOG_REFRESH_INTERVAL
    });
    if !cache_is_fresh {
        let workspaces = workspace::build_catalog(&registrations, &spaces, &worktrees);
        let roots = workspace::root_index(&spaces);
        *catalog_cache = Some(CatalogCache {
            registrations: registrations.clone(),
            spaces,
            worktrees,
            workspaces,
            roots,
            built_at: Instant::now(),
        });
    }
    let cache = catalog_cache
        .as_ref()
        .expect("catalog cache is filled on a miss");
    let precomputed = PrecomputedCatalog {
        registrations,
        workspaces: cache.workspaces.clone(),
        roots: cache.roots.clone(),
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

/// Reads what the worktree reader needs, holding the runtime mutex only for
/// the read itself. `None` means the runtime is gone.
fn read_worktrees_request(
    context: &SessionSyncContext,
) -> Option<crate::worktrees::WorktreeRequest> {
    let runtime = context.runtime.upgrade()?;
    let request = runtime.lock().ok()?.worktrees_request();
    drop(runtime);
    Some(request)
}

/// Records that the pull requests must be read again now.
///
/// Several agents finishing at once are one refresh, because the runtime's
/// counter is the request and an unchanged request is not re-read (G7).
fn request_pull_request_refresh(context: &SessionSyncContext, directories: &[String]) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let recorded = match runtime.lock() {
        Ok(mut guard) => {
            guard.refresh_pull_requests_in(directories);
            true
        }
        Err(_) => false,
    };
    drop(runtime);
    recorded
}

fn read_github_request(context: &SessionSyncContext) -> Option<crate::github::GithubRequest> {
    let runtime = context.runtime.upgrade()?;
    let request = runtime.lock().ok()?.github_request();
    drop(runtime);
    Some(request)
}

fn read_disk_request(context: &SessionSyncContext) -> Option<crate::disk::DiskRequest> {
    let runtime = context.runtime.upgrade()?;
    let request = runtime.lock().ok()?.disk_request();
    drop(runtime);
    Some(request)
}

/// Stores a worktree catalog. `None` means the runtime is gone; `Some(true)`
/// means the rows changed and the navigator catalog must be rebuilt.
fn publish_worktrees(
    context: &SessionSyncContext,
    catalog: crate::model::WorktreeCatalogSnapshot,
) -> Option<bool> {
    let runtime = context.runtime.upgrade()?;
    let changed = runtime.lock().ok()?.ingest_worktrees(catalog);
    drop(runtime);
    Some(changed)
}

fn publish_github(context: &SessionSyncContext, github: crate::model::GithubSnapshot) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => guard.ingest_github(github),
        Err(_) => return false,
    };
    drop(runtime);
    if changed {
        context.notifier.notify();
    }
    true
}

fn publish_disk_usage(context: &SessionSyncContext, disk: crate::model::DiskUsageSnapshot) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => guard.ingest_disk_usage(disk),
        Err(_) => return false,
    };
    drop(runtime);
    if changed {
        context.notifier.notify();
    }
    true
}

fn publish_ports(
    context: &SessionSyncContext,
    ports: crate::model::ListeningPortsSnapshot,
) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => guard.ingest_listening_ports(ports),
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

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectedWorkspace {
    pub(crate) workspace_id: String,
    pub(crate) label: String,
    pub(crate) active_tab_id: String,
    pub(crate) worktree: Option<ProjectedWorktree>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectedWorktree {
    pub(crate) repo_key: String,
    pub(crate) repo_name: String,
    pub(crate) repo_root: String,
    pub(crate) checkout_path: String,
    pub(crate) is_linked_worktree: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectedTab {
    pub(crate) tab_id: String,
    pub(crate) workspace_id: String,
    pub(crate) label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectedPane {
    pub(crate) pane_id: String,
    pub(crate) workspace_id: String,
    pub(crate) tab_id: String,
    pub(crate) cwd: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) terminal_title: Option<String>,
    pub(crate) terminal_title_stripped: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectedAgent {
    pub(crate) pane_id: String,
    pub(crate) workspace_id: String,
    pub(crate) tab_id: String,
    pub(crate) cwd: Option<String>,
    pub(crate) agent: Option<String>,
    pub(crate) agent_status: Option<String>,
    pub(crate) agent_session: Option<crate::sidebar::SessionAgentSessionPayload>,
    pub(crate) spawned_from_pane_id: Option<String>,
    pub(crate) state_change_seq: u64,
    pub(crate) tokens: BTreeMap<String, Value>,
    pub(crate) ambient: Option<Value>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProjectionState {
    pub(crate) focused_pane_id: Option<String>,
    /// Herdr's focused workspace, read from `session.snapshot` and kept
    /// current from the focus events. A `tab_focused` or `pane_focused` in
    /// another workspace moves it too, because Herdr focuses the workspace
    /// along with the tab and does not always send `workspace_focused` first.
    pub(crate) focused_workspace_id: Option<String>,
    pub(crate) workspaces: Vec<ProjectedWorkspace>,
    pub(crate) tabs: Vec<ProjectedTab>,
    pub(crate) panes: Vec<ProjectedPane>,
    pub(crate) layouts: Vec<SessionLayoutPayload>,
    pub(crate) agents: Vec<ProjectedAgent>,
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
        // Herdr order, not map order: the active tab id rides with the
        // workspace it belongs to, and the navigator reads it as the only
        // authority for which tab is active.
        let workspaces = self
            .workspaces
            .iter()
            .map(|workspace| SessionWorkspacePayload {
                workspace_id: workspace.workspace_id.clone(),
                label: workspace.label.clone(),
                active_tab_id: non_blank(Some(workspace.active_tab_id.as_str())),
            })
            .collect();
        SessionSnapshotPayload {
            focused_pane_id: self.focused_pane_id.clone(),
            focused_workspace_id: self.focused_workspace_id.clone(),
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
    Ok(wire::snapshot(snapshot.clone())?.2.project())
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

/// Every top-level `session.snapshot` field the replica reads. The contract
/// test asserts the pinned Herdr declares each one required, so a pin whose
/// snapshot cannot feed the replica fails in CI rather than at the user's
/// first launch with "snapshot is missing lineage".
pub(crate) const SNAPSHOT_FIELDS_THE_REPLICA_READS: [&str; 10] = [
    "version",
    "protocol",
    "workspaces",
    "tabs",
    "panes",
    "layouts",
    "agents",
    "lineage",
    "host",
    "event_sequence",
];

impl SessionReplica {
    #[cfg(test)]
    fn from_snapshot(snapshot: &Value) -> Result<Self, SessionFetchError> {
        Self::from_decoded(wire::snapshot(snapshot.clone())?)
    }

    fn from_decoded(
        (host, cursor, state): (HostScope, u64, ProjectionState),
    ) -> Result<Self, SessionFetchError> {
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
                let next_tab_label = crate::model::next_tab_label(
                    self.state
                        .tabs
                        .iter()
                        .filter(|tab| tab.workspace_id == workspace.workspace_id)
                        .map(|tab| tab.label.as_str()),
                );
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
                                    status_label: agent
                                        .map(|agent| agent.status_label.clone())
                                        .unwrap_or_else(|| "Attached".to_owned()),
                                    requires_close_confirmation: agent
                                        .is_some_and(|agent| agent.requires_close_confirmation),
                                    summary: agent.map(|agent| agent.summary.clone()),
                                    activity_at_unix_ms: self
                                        .state
                                        .agents
                                        .iter()
                                        .find(|source| source.pane_id == pane.pane_id)
                                        .and_then(wire::agent_activity)
                                        .and_then(|activity| activity.parse().ok()),
                                    fork: crate::runtime::pane_fork_snapshot(agent),
                                    // Ports describe this machine's listeners,
                                    // so a remote pane reports none rather than
                                    // claiming the local machine's.
                                    ports: Vec::new(),
                                }
                            })
                            .collect::<Vec<_>>();
                        TabSnapshot {
                            id: Some(remote_tab_id(target_id, &tab.tab_id)),
                            workspace_id: Some(workspace_id.clone()),
                            checkout_id: Some(checkout_id.clone()),
                            label: Some(crate::model::display_tab_label(&tab.label, &tab.tab_id)),
                            empty: panes.is_empty(),
                            panes,
                        }
                    })
                    .collect::<Vec<_>>();
                // A remote context has no file tabs, so its strip is the Herdr
                // tab list in Herdr's order and nothing else.
                let strip = StripTabSnapshot::from_herdr_tabs(&tabs);
                let active_tab_id = Some(remote_tab_id(target_id, &workspace.active_tab_id))
                    .filter(|active| tabs.iter().any(|tab| tab.id.as_ref() == Some(active)));
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
                    session_workspace_ids: vec![workspace.workspace_id.clone()],
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
                        // A remote checkout carries no worktree, pull-request,
                        // or disk facts: those readers describe this machine.
                        has_panes: !tabs.is_empty(),
                        tabs,
                        active_tab_id,
                        strip,
                        next_tab_label,
                        ..CheckoutSnapshot::default()
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

    /// Replaces the agent list and reports where agents stopped working.
    ///
    /// An agent leaving `working` is the moment its checkout's pull request is
    /// most likely to have just changed - it is what a run that ends in a push
    /// looks like from here - so it is the one event that re-reads `gh`
    /// without waiting out the five-minute window. Detecting it here rather
    /// than in the runtime keeps the comparison on the thread that already
    /// holds both the old and the new list.
    ///
    /// Returns the working directory of each agent that stopped, taken from
    /// the list it was working in: a pane that has since closed is gone from
    /// the new list and its old record is the one that still says where.
    fn replace_agents(&mut self, agents: Vec<ProjectedAgent>) -> Vec<String> {
        let stopped_in: Vec<String> = self
            .state
            .agents
            .iter()
            .filter(|agent| agent.agent_status.as_deref() == Some("working"))
            .filter(|agent| {
                !agents.iter().any(|next| {
                    next.pane_id == agent.pane_id && next.agent_status.as_deref() == Some("working")
                })
            })
            .filter_map(|agent| agent.cwd.clone())
            .collect();
        self.state.agents = agents;
        stopped_in
    }

    fn apply(&mut self, event: ReplicaEnvelope) -> Result<ApplyOutcome, SessionFetchError> {
        if event.protocol != HERDR_PROTOCOL_REVISION {
            return Err(protocol_mismatch(event.protocol));
        }
        if event.host != self.host {
            return Err(SessionFetchError::Stale(format!(
                "Herdr event host {:?} does not match snapshot host {:?}",
                event.host, self.host
            )));
        }
        let fingerprint = event.fingerprint;
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
        let refresh_agents = candidate.apply_new_event(event.data)?;
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

    fn apply_new_event(&mut self, data: ReplicaEvent) -> Result<bool, SessionFetchError> {
        match data {
            ReplicaEvent::WorkspaceCreated {
                workspace: input_workspace,
            } => {
                let event = "workspace_created";
                validate_workspace_wire(event, &input_workspace)?;
                if self
                    .state
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.workspace_id == input_workspace.workspace_id)
                {
                    return Err(malformed_event(event, "created workspace already exists"));
                }
                self.pending_layouts
                    .insert(input_workspace.active_tab_id.clone());
                self.state.workspaces.push(input_workspace);
            }
            ReplicaEvent::WorkspaceUpdated {
                workspace: input_workspace,
            } => {
                let event = "workspace_updated";
                validate_workspace_wire(event, &input_workspace)?;
                let workspace = self
                    .state
                    .workspaces
                    .iter_mut()
                    .find(|workspace| workspace.workspace_id == input_workspace.workspace_id)
                    .ok_or_else(|| malformed_event(event, "updated workspace does not exist"))?;
                *workspace = input_workspace;
            }
            ReplicaEvent::WorkspaceRenamed {
                workspace_id: input_workspace_id,
                label: input_label,
            } => {
                let event = "workspace_renamed";
                let workspace = self
                    .state
                    .workspaces
                    .iter_mut()
                    .find(|workspace| workspace.workspace_id == input_workspace_id)
                    .ok_or_else(|| malformed_event(event, "renamed workspace does not exist"))?;
                workspace.label = input_label;
            }
            ReplicaEvent::WorkspaceMoved {
                workspace_id: input_workspace_id,
                insert_index: input_insert_index,
                workspaces: input_workspaces,
            } => {
                let event = "workspace_moved";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                for workspace in &input_workspaces {
                    validate_workspace_wire(event, workspace)?;
                }
                if input_insert_index > input_workspaces.len()
                    || !input_workspaces
                        .iter()
                        .any(|workspace| workspace.workspace_id == input_workspace_id)
                {
                    return Err(malformed_event(
                        event,
                        "resulting workspace order does not contain the moved workspace at a valid index",
                    ));
                }
                self.state.workspaces = input_workspaces;
            }
            ReplicaEvent::WorkspaceReordered {
                workspace_ids: input_workspace_ids,
                workspaces: input_workspaces,
            } => {
                let event = "workspace_reordered";
                for workspace in &input_workspaces {
                    validate_workspace_wire(event, workspace)?;
                }
                if input_workspace_ids.is_empty()
                    || input_workspace_ids.iter().any(|workspace_id| {
                        workspace_id.trim().is_empty()
                            || !input_workspaces
                                .iter()
                                .any(|workspace| &workspace.workspace_id == workspace_id)
                    })
                {
                    return Err(malformed_event(
                        event,
                        "resulting workspace order does not contain every reordered workspace",
                    ));
                }
                self.state.workspaces = input_workspaces;
            }
            ReplicaEvent::WorkspaceClosed {
                workspace_id: input_workspace_id,
            } => {
                let event = "workspace_closed";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                if !self
                    .state
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.workspace_id == input_workspace_id)
                {
                    return Err(malformed_event(event, "closed workspace does not exist"));
                }
                self.remove_workspace(&input_workspace_id);
            }
            ReplicaEvent::WorkspaceFocused {
                workspace_id: input_workspace_id,
            } => {
                let event = "workspace_focused";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                if !self
                    .state
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.workspace_id == input_workspace_id)
                {
                    return Err(malformed_event(event, "focused workspace does not exist"));
                }
                self.state.focused_workspace_id = Some(input_workspace_id);
            }
            ReplicaEvent::WorktreeCreated {
                workspace: input_workspace,
            } => {
                let event = "worktree_created";
                validate_workspace_wire(event, &input_workspace)?;
                upsert_workspace(&mut self.state.workspaces, input_workspace);
            }
            ReplicaEvent::WorktreeOpened {
                workspace: input_workspace,
            } => {
                let event = "worktree_opened";
                validate_workspace_wire(event, &input_workspace)?;
                upsert_workspace(&mut self.state.workspaces, input_workspace);
            }
            ReplicaEvent::WorktreeRemoved {
                workspace_id: input_workspace_id,
                workspace: input_workspace,
            } => {
                let event = "worktree_removed";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                if let Some(workspace) = input_workspace {
                    validate_workspace_wire(event, &workspace)?;
                    if workspace.workspace_id != input_workspace_id {
                        return Err(malformed_event(
                            event,
                            "workspace does not match workspace_id",
                        ));
                    }
                    upsert_workspace(&mut self.state.workspaces, workspace);
                }
            }
            ReplicaEvent::TabCreated { tab: input_tab } => {
                let event = "tab_created";
                validate_tab_wire(event, &input_tab)?;
                if !self
                    .state
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.workspace_id == input_tab.workspace_id)
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
                    .any(|tab| tab.tab_id == input_tab.tab_id)
                {
                    return Err(malformed_event(event, "created tab already exists"));
                }
                self.pending_layouts.insert(input_tab.tab_id.clone());
                self.state.tabs.push(input_tab);
            }
            ReplicaEvent::TabClosed {
                workspace_id: input_workspace_id,
                tab_id: input_tab_id,
            } => {
                let event = "tab_closed";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                let workspace_tab_count = self
                    .state
                    .tabs
                    .iter()
                    .filter(|tab| tab.workspace_id == input_workspace_id)
                    .count();
                let tab = self
                    .state
                    .tabs
                    .iter()
                    .find(|tab| tab.tab_id == input_tab_id)
                    .ok_or_else(|| malformed_event(event, "closed tab does not exist"))?;
                if tab.workspace_id != input_workspace_id {
                    return Err(malformed_event(
                        event,
                        "closed tab belongs to another workspace",
                    ));
                }
                let active_tab_closed = self
                    .state
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == input_workspace_id)
                    .is_some_and(|workspace| workspace.active_tab_id == input_tab_id);
                if workspace_tab_count == 1 {
                    self.pending_workspace_closures
                        .insert(input_workspace_id.clone());
                } else if active_tab_closed {
                    self.pending_active_tab_focuses
                        .insert(input_workspace_id.clone());
                }
                self.remove_tab(&input_tab_id);
            }
            ReplicaEvent::TabRenamed {
                workspace_id: input_workspace_id,
                tab_id: input_tab_id,
                label: input_label,
            } => {
                let event = "tab_renamed";
                let tab = self
                    .state
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.tab_id == input_tab_id)
                    .ok_or_else(|| malformed_event(event, "renamed tab does not exist"))?;
                if tab.workspace_id != input_workspace_id {
                    return Err(malformed_event(
                        event,
                        "renamed tab belongs to another workspace",
                    ));
                }
                tab.label = input_label;
            }
            ReplicaEvent::TabMoved {
                workspace_id: input_workspace_id,
                tab_id: input_tab_id,
                insert_index: input_insert_index,
                tabs: input_tabs,
            } => {
                let event = "tab_moved";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                ensure_non_empty(event, "tab_id", &input_tab_id)?;
                for tab in &input_tabs {
                    validate_tab_wire(event, tab)?;
                }
                if input_insert_index > input_tabs.len()
                    || input_tabs
                        .iter()
                        .any(|tab| tab.workspace_id != input_workspace_id)
                    || !input_tabs.iter().any(|tab| tab.tab_id == input_tab_id)
                {
                    return Err(malformed_event(
                        event,
                        "resulting tab order is inconsistent with the moved tab",
                    ));
                }
                self.state
                    .tabs
                    .retain(|tab| tab.workspace_id != input_workspace_id);
                self.state.tabs.extend(input_tabs);
            }
            ReplicaEvent::TabFocused {
                workspace_id: input_workspace_id,
                tab_id: input_tab_id,
            } => {
                let event = "tab_focused";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                ensure_non_empty(event, "tab_id", &input_tab_id)?;
                let workspace = self
                    .state
                    .workspaces
                    .iter_mut()
                    .find(|workspace| workspace.workspace_id == input_workspace_id)
                    .ok_or_else(|| malformed_event(event, "focused workspace does not exist"))?;
                let tab = self
                    .state
                    .tabs
                    .iter()
                    .find(|tab| tab.tab_id == input_tab_id)
                    .ok_or_else(|| malformed_event(event, "focused tab does not exist"))?;
                if tab.workspace_id != input_workspace_id {
                    return Err(malformed_event(
                        event,
                        "focused tab belongs to another workspace",
                    ));
                }
                workspace.active_tab_id = input_tab_id;
                self.state.focused_workspace_id = Some(input_workspace_id.clone());
                self.pending_active_tab_focuses.remove(&input_workspace_id);
            }
            ReplicaEvent::PaneCreated { pane: input_pane } => {
                let event = "pane_created";
                validate_pane_wire(event, &input_pane)?;
                if !self.state.tabs.iter().any(|tab| {
                    tab.tab_id == input_pane.tab_id && tab.workspace_id == input_pane.workspace_id
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
                    .any(|pane| pane.pane_id == input_pane.pane_id)
                {
                    return Err(malformed_event(event, "created pane already exists"));
                }
                self.pending_layouts.insert(input_pane.tab_id.clone());
                self.state.panes.push(input_pane);
            }
            ReplicaEvent::PaneClosed {
                workspace_id: input_workspace_id,
                pane_id: input_pane_id,
            } => {
                let event = "pane_closed";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                let pane = self
                    .state
                    .panes
                    .iter()
                    .find(|pane| pane.pane_id == input_pane_id)
                    .ok_or_else(|| malformed_event(event, "closed pane does not exist"))?;
                if pane.workspace_id != input_workspace_id {
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
                    .filter(|tab| tab.workspace_id == input_workspace_id)
                    .count();
                if last_pane_in_tab && workspace_tab_count > 1 {
                    let active_tab_closed = self
                        .state
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.workspace_id == input_workspace_id)
                        .is_some_and(|workspace| workspace.active_tab_id == tab_id);
                    if active_tab_closed {
                        self.pending_active_tab_focuses
                            .insert(input_workspace_id.clone());
                    }
                    // Herdr 0.8.2 removes an emptied tab as part of pane.close
                    // without emitting a separate tab.closed event. Mirror that
                    // authoritative cascade so the replica cannot wait forever
                    // for a layout.updated event for a tab that no longer exists.
                    self.remove_tab(&tab_id);
                } else {
                    self.pending_layouts.insert(tab_id);
                    self.remove_pane(&input_pane_id);
                }
            }
            ReplicaEvent::PaneUpdated { pane: input_pane } => {
                let event = "pane_updated";
                validate_pane_wire(event, &input_pane)?;
                let previous = self
                    .state
                    .panes
                    .iter()
                    .find(|pane| pane.pane_id == input_pane.pane_id)
                    .cloned()
                    .ok_or_else(|| malformed_event(event, "updated pane does not exist"))?;
                if previous.tab_id != input_pane.tab_id
                    || previous.workspace_id != input_pane.workspace_id
                {
                    self.pending_layouts.insert(previous.tab_id);
                    self.pending_layouts.insert(input_pane.tab_id.clone());
                }
                upsert_pane(&mut self.state.panes, input_pane);
            }
            ReplicaEvent::PaneFocused {
                workspace_id: input_workspace_id,
                pane_id: input_pane_id,
            } => {
                let event = "pane_focused";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                if !self.state.panes.iter().any(|pane| {
                    pane.pane_id == input_pane_id && pane.workspace_id == input_workspace_id
                }) {
                    return Err(malformed_event(
                        event,
                        "focused pane does not exist in the stated workspace",
                    ));
                }
                self.state.focused_pane_id = Some(input_pane_id.clone());
                self.state.focused_workspace_id = Some(input_workspace_id.clone());
                if let Some(layout) = self.state.layouts.iter_mut().find(|layout| {
                    layout
                        .panes
                        .iter()
                        .any(|pane| pane.pane_id == input_pane_id)
                }) {
                    layout.focused_pane_id = input_pane_id;
                }
            }
            ReplicaEvent::PaneMoved(payload) => {
                self.apply_pane_moved(payload)?;
                return Ok(true);
            }
            ReplicaEvent::PaneExited {
                workspace_id: input_workspace_id,
                pane_id: input_pane_id,
            } => {
                let event = "pane_exited";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                ensure_non_empty(event, "pane_id", &input_pane_id)?;
            }
            ReplicaEvent::PaneAgentDetected {
                workspace_id: input_workspace_id,
                pane_id: input_pane_id,
            } => {
                let event = "pane_agent_detected";
                ensure_non_empty(event, "workspace_id", &input_workspace_id)?;
                ensure_non_empty(event, "pane_id", &input_pane_id)?;
                return Ok(true);
            }
            ReplicaEvent::LayoutUpdated {
                layout: input_layout,
            } => {
                let event = "layout_updated";
                ensure_non_empty(event, "workspace_id", &input_layout.workspace_id)?;
                ensure_non_empty(event, "tab_id", &input_layout.tab_id)?;
                ensure_non_empty(event, "focused_pane_id", &input_layout.focused_pane_id)?;
                if !self.state.tabs.iter().any(|tab| {
                    tab.tab_id == input_layout.tab_id
                        && tab.workspace_id == input_layout.workspace_id
                }) {
                    return Err(malformed_event(event, "layout references a missing tab"));
                }
                let tab_id = input_layout.tab_id.clone();
                let focused_was_missing =
                    self.state.focused_pane_id.as_ref().is_none_or(|focused| {
                        !self.state.panes.iter().any(|pane| &pane.pane_id == focused)
                    });
                upsert_layout(&mut self.state.layouts, input_layout);
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
            ReplicaEvent::Unrequested(unknown) => {
                return Err(SessionFetchError::Malformed(format!(
                    "Herdr subscription emitted unrequested event {unknown:?}"
                )));
            }
        }
        Ok(false)
    }

    fn apply_pane_moved(&mut self, payload: PaneMove) -> Result<(), SessionFetchError> {
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
        if self.state.focused_workspace_id.as_deref() == Some(workspace_id) {
            self.state.focused_workspace_id = None;
        }
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
    workspace: &ProjectedWorkspace,
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

fn validate_tab_wire(event: &str, tab: &ProjectedTab) -> Result<(), SessionFetchError> {
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

fn validate_pane_wire(event: &str, pane: &ProjectedPane) -> Result<(), SessionFetchError> {
    ensure_non_empty(event, "pane.pane_id", &pane.pane_id)?;
    ensure_non_empty(event, "pane.workspace_id", &pane.workspace_id)?;
    ensure_non_empty(event, "pane.tab_id", &pane.tab_id)
}

fn upsert_workspace(workspaces: &mut Vec<ProjectedWorkspace>, workspace: ProjectedWorkspace) {
    if let Some(existing) = workspaces
        .iter_mut()
        .find(|existing| existing.workspace_id == workspace.workspace_id)
    {
        *existing = workspace;
    } else {
        workspaces.push(workspace);
    }
}

fn upsert_tab(tabs: &mut Vec<ProjectedTab>, tab: ProjectedTab) {
    if let Some(existing) = tabs
        .iter_mut()
        .find(|existing| existing.tab_id == tab.tab_id)
    {
        *existing = tab;
    } else {
        tabs.push(tab);
    }
}

fn upsert_pane(panes: &mut Vec<ProjectedPane>, pane: ProjectedPane) {
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

/// Inputs to replica transitions, with transport details removed by the boundary.
#[derive(Clone, Debug)]
pub(crate) enum ReplicaEvent {
    WorkspaceCreated {
        workspace: ProjectedWorkspace,
    },
    WorkspaceUpdated {
        workspace: ProjectedWorkspace,
    },
    WorkspaceRenamed {
        workspace_id: String,
        label: String,
    },
    WorkspaceMoved {
        workspace_id: String,
        insert_index: usize,
        workspaces: Vec<ProjectedWorkspace>,
    },
    WorkspaceReordered {
        workspace_ids: Vec<String>,
        workspaces: Vec<ProjectedWorkspace>,
    },
    WorkspaceClosed {
        workspace_id: String,
    },
    WorkspaceFocused {
        workspace_id: String,
    },
    WorktreeCreated {
        workspace: ProjectedWorkspace,
    },
    WorktreeOpened {
        workspace: ProjectedWorkspace,
    },
    WorktreeRemoved {
        workspace_id: String,
        workspace: Option<ProjectedWorkspace>,
    },
    TabCreated {
        tab: ProjectedTab,
    },
    TabClosed {
        workspace_id: String,
        tab_id: String,
    },
    TabRenamed {
        workspace_id: String,
        tab_id: String,
        label: String,
    },
    TabMoved {
        workspace_id: String,
        tab_id: String,
        insert_index: usize,
        tabs: Vec<ProjectedTab>,
    },
    TabFocused {
        workspace_id: String,
        tab_id: String,
    },
    PaneCreated {
        pane: ProjectedPane,
    },
    PaneClosed {
        workspace_id: String,
        pane_id: String,
    },
    PaneUpdated {
        pane: ProjectedPane,
    },
    PaneFocused {
        workspace_id: String,
        pane_id: String,
    },
    PaneMoved(PaneMove),
    PaneExited {
        workspace_id: String,
        pane_id: String,
    },
    PaneAgentDetected {
        workspace_id: String,
        pane_id: String,
    },
    LayoutUpdated {
        layout: SessionLayoutPayload,
    },
    Unrequested(String),
}
#[derive(Clone, Debug)]
pub(crate) struct PaneMove {
    pub(crate) previous_pane_id: String,
    pub(crate) previous_workspace_id: String,
    pub(crate) previous_tab_id: String,
    pub(crate) pane: ProjectedPane,
    pub(crate) created_workspace: Option<ProjectedWorkspace>,
    pub(crate) created_tab: Option<ProjectedTab>,
    pub(crate) closed_workspace_id: Option<String>,
    pub(crate) closed_tab_id: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReplicaEnvelope {
    pub(crate) protocol: u64,
    pub(crate) host: HostScope,
    pub(crate) sequence: u64,
    pub(crate) data: ReplicaEvent,
    pub(crate) fingerprint: String,
}

pub(crate) enum SubscriptionLine {
    Event(ReplicaEnvelope),
    Error { code: String, message: String },
}

fn malformed_event(event: &str, detail: &str) -> SessionFetchError {
    SessionFetchError::Malformed(format!("Herdr {event} event {detail}"))
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
                "agent_status": "idle", "focused": true, "number": 1, "pane_count": 1, "tab_count": 1,
                "active_tab_id": "w1:t1"
            }],
            "tabs": [{
                "workspace_id": "w1",
                "tab_id": "w1:t1",
                "agent_status": "idle", "focused": false, "number": 1, "pane_count": 1, "label": "1"
            }],
            "panes": [{
                "workspace_id": "w1",
                "tab_id": "w1:t1",
                "pane_id": "w1:p1", "focused": false, "revision": 0, "agent_status": "idle",
                "surface": {"kind": "terminal", "attach": {"terminal_id": "fixture-terminal", "protocol": HERDR_PROTOCOL_REVISION, "transport": "herdr_client", "host": {"host_id": "fixture-host", "session_id": "fixture"}}},
                "cwd": "/tmp/fixture"
            }],
            "layouts": [{
                "workspace_id": "w1",
                "tab_id": "w1:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 120, "height": 60},
                "focused_pane_id": "w1:p1",
                "panes": [{
                    "pane_id": "w1:p1", "focused": false,
                    "rect": {"x": 0, "y": 0, "width": 120, "height": 60}
                }],
                "splits": []
            }],
            "agents": [],
            "lineage": []
        })
    }

    #[test]
    fn every_session_sync_snapshot_fixture_obeys_the_generated_contract() {
        for (fixture, panes) in [(snapshot(), 1), (two_tab_snapshot(), 2)] {
            let (_, cursor, state) = wire::snapshot(fixture).expect("generated snapshot contract");
            assert_eq!(cursor, 40);
            assert_eq!(state.panes.len(), panes);
        }
    }

    fn two_tab_snapshot() -> Value {
        let mut value = snapshot();
        value["tabs"]
            .as_array_mut()
            .expect("tabs array")
            .push(json!({
                "workspace_id": "w1",
                "tab_id": "w1:t2",
                "agent_status": "idle", "focused": false, "number": 2, "pane_count": 1, "label": "2"
            }));
        value["panes"]
            .as_array_mut()
            .expect("panes array")
            .push(json!({
                "workspace_id": "w1",
                "tab_id": "w1:t2",
                "pane_id": "w1:p2", "focused": false, "revision": 0, "agent_status": "idle",
                "surface": {"kind": "terminal", "attach": {"terminal_id": "fixture-terminal", "protocol": HERDR_PROTOCOL_REVISION, "transport": "herdr_client", "host": {"host_id": "fixture-host", "session_id": "fixture"}}},
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
                    "pane_id": "w1:p2", "focused": false,
                    "rect": {"x": 0, "y": 0, "width": 120, "height": 60}
                }],
                "splits": []
            }));
        value
    }

    fn event(sequence: u64, kind: &str, data: Value) -> ReplicaEnvelope {
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
                        "pane_id": "w1:p2", "focused": false, "revision": 0, "agent_status": "idle",
                        "surface": {"kind": "terminal", "attach": {"terminal_id": "fixture-terminal", "protocol": HERDR_PROTOCOL_REVISION, "transport": "herdr_client", "host": {"host_id": "fixture-host", "session_id": "fixture"}}},
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
                            {"pane_id": "w1:p1", "focused": false, "rect": {"x": 0, "y": 0, "width": 60, "height": 60}},
                            {"pane_id": "w1:p2", "focused": false, "rect": {"x": 60, "y": 0, "width": 60, "height": 60}}
                        ],
                        "splits": [{
                            "id": "split_0_root",
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
            "agent_status": "working", "focused": false, "revision": 0, "terminal_id": "fixture-terminal",
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
        assert_eq!(projected.agents[0].demand, "none");
        assert_eq!(projected.agents[0].activity, "working");
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

    /// The remote context browses Herdr's tabs and has no file tabs to mix in,
    /// so its strip is the Herdr tab list in Herdr's order and nothing else.
    #[test]
    fn remote_projection_tab_list_and_order_are_herdr_only() {
        let replica = SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");
        let (projected, _) = replica.project_remote("mini").expect("remote projection");
        let checkout = projected
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .next()
            .expect("the remote checkout");

        assert_eq!(
            checkout
                .tabs
                .iter()
                .map(|tab| tab.id.clone().expect("a remote tab id"))
                .collect::<Vec<_>>(),
            vec![
                remote_tab_id("mini", "w1:t1"),
                remote_tab_id("mini", "w1:t2")
            ]
        );
        assert_eq!(
            checkout
                .strip
                .iter()
                .map(|entry| (entry.kind, entry.source_id.clone()))
                .collect::<Vec<_>>(),
            vec![
                (
                    crate::model::StripTabKind::Herdr,
                    remote_tab_id("mini", "w1:t1")
                ),
                (
                    crate::model::StripTabKind::Herdr,
                    remote_tab_id("mini", "w1:t2")
                )
            ]
        );
        assert_eq!(checkout.active_tab_id, Some(remote_tab_id("mini", "w1:t1")));
    }

    #[test]
    fn tab_order_from_a_move_event_reaches_the_projection() {
        let mut replica =
            SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");
        assert_eq!(
            replica
                .project()
                .tabs
                .iter()
                .map(|tab| tab.tab_id.clone())
                .collect::<Vec<_>>(),
            vec!["w1:t1".to_owned(), "w1:t2".to_owned()]
        );

        // Herdr moved the second tab in front of the first and reported the
        // resulting order. The projection the navigator reads must carry that
        // order, not the order the tabs were created in.
        let moved = replica
            .apply(event(
                41,
                "tab_moved",
                json!({
                    "type": "tab_moved",
                    "workspace_id": "w1",
                    "tab_id": "w1:t2",
                    "insert_index": 0,
                    "tabs": [
                        {"workspace_id": "w1", "tab_id": "w1:t2", "agent_status": "idle", "focused": false, "number": 2, "pane_count": 1, "label": "2"},
                        {"workspace_id": "w1", "tab_id": "w1:t1", "agent_status": "idle", "focused": false, "number": 1, "pane_count": 1, "label": "1"}
                    ]
                }),
            ))
            .expect("tab move event");
        assert!(moved.publish);
        assert_eq!(
            replica
                .project()
                .tabs
                .iter()
                .map(|tab| tab.tab_id.clone())
                .collect::<Vec<_>>(),
            vec!["w1:t2".to_owned(), "w1:t1".to_owned()]
        );
    }

    #[test]
    fn herdr_active_tab_rides_the_projection_with_its_workspace() {
        let replica = SessionReplica::from_snapshot(&two_tab_snapshot()).expect("two-tab snapshot");
        let projected = replica.project();
        let workspace = projected
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == "w1")
            .expect("the fixture workspace");
        assert_eq!(workspace.active_tab_id.as_deref(), Some("w1:t1"));
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

    fn agent(pane_id: &str, elapsed: &str) -> ProjectedAgent {
        wire::agents_response(json!({"type": "agent_list", "agents": [{
            "pane_id": pane_id, "workspace_id": "w1", "tab_id": "w1:t1",
            "agent": "claude", "agent_status": "idle", "focused": false,
            "terminal_id": "fixture-terminal", "revision": 0,
            "tokens": {"status_idle": "\u{25cb}", "elapsed": elapsed, "activity": "1"}
        }]}))
        .expect("an agent list")
        .remove(0)
    }

    fn fresh_catalog() -> CatalogCache {
        CatalogCache {
            registrations: Vec::new(),
            spaces: Vec::new(),
            worktrees: crate::model::WorktreeCatalogSnapshot::default(),
            workspaces: Vec::new(),
            roots: workspace::RootIndex::new(),
            built_at: Instant::now(),
        }
    }

    /// R6, AC12, SC5. `agent.list` is polled once a second whether or not it
    /// moved, and every tick used to rebuild the whole projection and re-enter
    /// the runtime lock twice for a sidebar that had not changed.
    #[test]
    fn snapshot_delivery_skips_the_projection_when_the_agent_list_is_unchanged() {
        let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
        let held = vec![agent("w1:p1", "4m")];
        replica.replace_agents(held.clone());
        let catalog = fresh_catalog();

        assert!(
            !agent_tick_needs_publish(&replica, &held, Some(&catalog)),
            "an identical list projects the same sidebar, so the tick publishes nothing"
        );
        assert!(
            agent_tick_needs_publish(&replica, &[agent("w1:p1", "5m")], Some(&catalog)),
            "a token the sidebar renders moved, so the projection has to be rebuilt"
        );
        assert!(
            agent_tick_needs_publish(&replica, &[], Some(&catalog)),
            "an agent that went away has to leave the sidebar"
        );

        // The catalog is rebuilt inside the publish on its own refresh window,
        // and on an idle session this tick is the only thing that calls it, so
        // the skip must not be what freezes every branch mark in the navigator.
        assert!(
            agent_tick_needs_publish(&replica, &held, None),
            "a catalog that was never built has to be built"
        );
        let stale = CatalogCache {
            built_at: Instant::now() - CATALOG_REFRESH_INTERVAL,
            ..fresh_catalog()
        };
        assert!(
            agent_tick_needs_publish(&replica, &held, Some(&stale)),
            "a catalog past its refresh window has to be rebuilt"
        );
    }

    /// Herdr's focused workspace is carried from the snapshot and moved by
    /// every focus event, because a checkout can hold tabs from several
    /// workspaces and only the focused one's active tab is a focus.
    #[test]
    fn focus_events_move_the_projected_focused_workspace() {
        let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
        assert_eq!(
            replica.project().focused_workspace_id,
            None,
            "the fixture snapshot names no focused workspace"
        );

        replica
            .apply(event(
                41,
                "workspace_focused",
                json!({"type": "workspace_focused", "workspace_id": "w1"}),
            ))
            .expect("workspace focus applies");
        assert_eq!(
            replica.project().focused_workspace_id.as_deref(),
            Some("w1")
        );

        let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
        replica
            .apply(event(
                41,
                "tab_focused",
                json!({"type": "tab_focused", "workspace_id": "w1", "tab_id": "w1:t1"}),
            ))
            .expect("tab focus applies");
        assert_eq!(
            replica.project().focused_workspace_id.as_deref(),
            Some("w1"),
            "a tab focus names the workspace Herdr moved into"
        );

        let mut replica = SessionReplica::from_snapshot(&snapshot()).expect("snapshot");
        replica
            .apply(event(
                41,
                "pane_focused",
                json!({"type": "pane_focused", "workspace_id": "w1", "pane_id": "w1:p1"}),
            ))
            .expect("pane focus applies");
        assert_eq!(
            replica.project().focused_workspace_id.as_deref(),
            Some("w1"),
            "a pane focus names the workspace the pane is in"
        );
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
        let mismatch = replica.apply(event).expect_err("protocol mismatch");
        assert_eq!(mismatch.state(), "protocol_mismatch");
        let message = mismatch.message();
        assert!(
            message.contains(&format!("protocol {}", HERDR_PROTOCOL_REVISION + 1))
                && message.contains(&format!("needs protocol {HERDR_PROTOCOL_REVISION}"))
                && message.contains("herdr server stop"),
            "the mismatch names both revisions and the remedy: {message}"
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
            // A project is named after its directory, not Herdr's workspace
            // label, so the recovery marker is a pane in a new directory.
            recovered["panes"][0]["cwd"] = json!("/tmp/fixture-recovered");
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
                    .any(|workspace| workspace.label == "fixture-recovered")
        });

        drop(handle);
        server.join().expect("fake server joins");
        remove_fixture(&root, &socket_path, &state_path);
    }
}
