//! Owns Herdr subscription setup, reader lifetime, and snapshot fetches.

use super::*;

/// What a successful connect hands the coordinator: the replica, the live
/// subscription, and when the snapshot the replica was built from arrived.
/// Lines read before that instant, plus a grace for scheduling, describe
/// changes the snapshot already holds (`ApplyMode::Reconcile`).
pub(crate) struct Connected {
    pub(crate) replica: SessionReplica,
    pub(crate) subscription: ActiveSubscription,
    pub(crate) snapshot_at: Instant,
}

/// Subscribes first and reads the snapshot second. Herdr's stream cannot be
/// resumed from a position, so the only way not to miss an event between the
/// two is to be listening before the snapshot is taken; the replica then
/// starts from the snapshot and applies what the stream delivers after it.
pub(crate) fn connect(
    context: &SessionSyncContext,
    sender: &Sender<CoordinatorMessage>,
    generation: &mut u64,
    has_projection: bool,
) -> Result<Connected, SessionFetchError> {
    require_local_socket(context)?;
    let subscription = hide_herdr_client::subscribe_with_connector(
        context.api_connector.as_ref(),
        TOPOLOGY_SUBSCRIPTIONS,
        SYNC_REQUEST_TIMEOUT,
    )
    .map_err(|error| connect_failure_from_api(error, has_projection))?;
    *generation = generation.saturating_add(1);
    let subscription = spawn_subscription_reader(subscription, *generation, sender.clone())
        .map_err(SessionFetchError::Unreachable)?;
    let replica = match fetch_replica(context) {
        Ok(replica) => replica,
        Err(error) => {
            subscription.stop();
            return Err(error);
        }
    };
    Ok(Connected {
        replica,
        subscription,
        snapshot_at: Instant::now(),
    })
}

fn spawn_subscription_reader(
    subscription: hide_herdr_client::Subscription,
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
                            .send(CoordinatorMessage::SubscriptionLine {
                                generation,
                                line,
                                received_at: Instant::now(),
                            })
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

pub(crate) fn stop_subscription(subscription: &mut Option<ActiveSubscription>) {
    if let Some(active) = subscription.take() {
        active.stop();
    }
}

/// A local read against a socket file that is gone is the server being
/// down, which is its own state, not a transport error.
fn require_local_socket(context: &SessionSyncContext) -> Result<(), SessionFetchError> {
    if let SessionSyncTarget::Local { socket_path } = &context.target
        && !socket_path.exists()
    {
        return Err(SessionFetchError::SocketMissing(format!(
            "Herdr socket file does not exist at {}; the herdr server is not running",
            socket_path.display()
        )));
    }
    Ok(())
}

fn fetch_replica(context: &SessionSyncContext) -> Result<SessionReplica, SessionFetchError> {
    require_local_socket(context)?;
    let result = hide_herdr_client::request_with_connector(
        context.api_connector.as_ref(),
        "session.snapshot",
        json!({}),
        SYNC_REQUEST_TIMEOUT,
    )
    .map_err(session_error_from_api)?;
    SessionReplica::from_decoded(wire::snapshot_response(result)?)
}

pub(crate) fn fetch_agents(
    context: &SessionSyncContext,
) -> Result<Vec<ProjectedAgent>, SessionFetchError> {
    require_local_socket(context)?;
    let result = hide_herdr_client::request_with_connector(
        context.api_connector.as_ref(),
        "agent.list",
        json!({}),
        SYNC_REQUEST_TIMEOUT,
    )
    .map_err(session_error_from_api)?;
    wire::agents_response(result)
}

/// Reads which tab a workspace now holds as active. It is the one read the
/// replica needs after a close removed a workspace's active tab, because
/// Herdr names the replacement in an event only for the workspace that holds
/// its keyboard focus (`SessionReplica::settle_active_tab`).
pub(crate) fn fetch_workspace_active_tab(
    context: &SessionSyncContext,
    workspace_id: &str,
) -> Result<String, SessionFetchError> {
    require_local_socket(context)?;
    let result = hide_herdr_client::request_with_connector(
        context.api_connector.as_ref(),
        "workspace.get",
        wire::workspace_target_params(workspace_id).map_err(SessionFetchError::Malformed)?,
        SYNC_REQUEST_TIMEOUT,
    )
    .map_err(session_error_from_api)?;
    wire::workspace_active_tab(result).map_err(SessionFetchError::Malformed)
}
fn session_error_from_api(error: ApiError) -> SessionFetchError {
    match error {
        ApiError::Transport(message) | ApiError::Remote { message, .. } => {
            SessionFetchError::Unreachable(message)
        }
        ApiError::Malformed(message) => SessionFetchError::Malformed(message),
    }
}

/// A subscription that failed to open. With a projection already on screen
/// the failure is what makes it stale; without one the server is unreachable.
pub(crate) fn connect_failure_from_api(error: ApiError, has_projection: bool) -> SessionFetchError {
    match error {
        ApiError::Malformed(message) => SessionFetchError::Malformed(message),
        ApiError::Transport(message) | ApiError::Remote { message, .. } if has_projection => {
            SessionFetchError::Stale(message)
        }
        ApiError::Transport(message) | ApiError::Remote { message, .. } => {
            SessionFetchError::Unreachable(message)
        }
    }
}

pub(crate) fn log_sync_failure(
    context: &SessionSyncContext,
    kind: &str,
    replica: Option<&SessionReplica>,
    error: &SessionFetchError,
) {
    crate::diagnostic!(json!({
        "component": "session_sync",
        "kind": kind,
        "target": context.log_target(),
        "state": error.state(),
        "applied_events": replica.map(|current| current.applied_events),
        "message": error.message(),
    }));
}
