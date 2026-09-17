//! Owns Herdr subscription setup, reader lifetime, and snapshot fetches.

use super::*;

pub(crate) fn connect_from_snapshot(
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

pub(crate) fn connect_from_cursor(
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
    let subscription = hide_herdr_client::subscribe_with_connector(
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

pub(crate) fn connect_failure_from_api(
    error: ApiError,
    has_projection: bool,
    cursor: u64,
) -> ConnectFailure {
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
        "sequence": replica.map(|current| current.cursor),
        "message": error.message(),
    }));
}

pub(crate) struct ConnectFailure {
    pub(crate) error: SessionFetchError,
    pub(crate) needs_bootstrap: bool,
}
