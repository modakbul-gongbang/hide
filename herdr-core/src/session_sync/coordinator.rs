//! Coordinates Herdr snapshot bootstrap, topology subscriptions, and capability readers.

use super::*;
use crate::labels::worker::{LabelWorker, ObservedAgent};
use crate::live;

pub(crate) fn spawn(
    context: SessionSyncContext,
    usage_paths: Option<crate::usage::UsagePaths>,
) -> Result<SessionSyncHandle, String> {
    let (sender, receiver) = channel();
    let worker_sender = sender.clone();
    let worker = thread::Builder::new()
        .name("herdr-core-session-sync".to_owned())
        .spawn(move || run_coordinator(context, usage_paths, receiver, worker_sender))
        .map_err(|error| format!("session sync worker could not be started: {error}"))?;
    Ok(SessionSyncHandle {
        sender,
        worker: Some(worker),
    })
}

fn run_coordinator(
    context: SessionSyncContext,
    usage_paths: Option<crate::usage::UsagePaths>,
    receiver: Receiver<CoordinatorMessage>,
    sender: Sender<CoordinatorMessage>,
) {
    let home_path = usage_paths.as_ref().and_then(|paths| paths.home.clone());
    let mut process_reader = super::process_info::ProcessReader::new(&context);
    let mut replica: Option<SessionReplica> = None;
    let mut active_tab_reads: BTreeMap<String, u32> = BTreeMap::new();
    let mut subscription: Option<ActiveSubscription> = None;
    let mut subscription_generation = 0_u64;
    // Until when a line off the stream is reconciled against the bootstrap
    // snapshot rather than applied strictly. Every connect is a bootstrap,
    // because Herdr's stream has no position to resume from.
    let mut reconcile_until = Instant::now();
    let mut reconnect_at = Instant::now();
    let mut reconnect_delay = RECONNECT_INITIAL_DELAY;
    let mut defer_background_reads = true;
    let mut next_agent_refresh = Instant::now() + AGENT_REFRESH_INTERVAL;
    let mut next_operation_tick = Instant::now() + ASYNC_OPERATION_TICK_INTERVAL;
    let mut next_hook_diagnosis_refresh = Instant::now();
    let mut catalog_cache: Option<CatalogCache> = None;
    // The runtime's settled worktree removals the catalog was last rebuilt for.
    let mut published_removals = 0;
    let mut purpose_mirror = if context.is_local() {
        match live::PurposeMirror::new(Arc::clone(&context.api_connector)) {
            Ok(mirror) => Some(mirror),
            Err(message) => {
                crate::diagnostic!(json!({
                    "component": "checkout_purpose",
                    "kind": "mirror.start_failed",
                    "message": message,
                }));
                None
            }
        }
    } else {
        None
    };
    // The hook-install state is two small file reads of this machine's own
    // configuration, so the local coordinator takes it once before the first
    // connect. It is not a poll: it changes only when the kit installs or the
    // operator removes, and the kit worker republishes it after each install
    // (PRD B36). The install itself is the kit's (`crate::kit`).
    if context.is_local()
        && let Some(home) = home_path.as_deref()
    {
        publish_hook_diagnosis(&context, hide_agent_hooks::Diagnosis::read(home));
    }
    // Kept for the counter sweep below, which runs on a fresh snapshot.
    let hook_home = context.is_local().then(|| home_path.clone()).flatten();
    let mut usage_reader = usage_paths.map(crate::usage::ProviderUsageReader::new);
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
    // The provider probe starts a `codex app-server` child and runs
    // `claude auth status`, so it is a reader like the three above and it
    // reads nothing at all while the Background AI group is off screen.
    let mut ai_reader = context.is_local().then(crate::ai::AiReader::new);
    if let Some(home) = hook_home.as_deref() {
        publish_ai_settings(&context, home);
    }
    let mut labels = start_label_worker(&context, &sender);

    loop {
        if context.runtime.upgrade().is_none() {
            stop_subscription(&mut subscription);
            return;
        }

        if subscription.is_none() && Instant::now() >= reconnect_at {
            let has_projection = replica.is_some();
            match connect(
                &context,
                &sender,
                &mut subscription_generation,
                has_projection,
            ) {
                Ok(Connected {
                    replica: next_replica,
                    subscription: next_subscription,
                    snapshot_at,
                }) => {
                    if context.is_local() && !begin_local_read_record_reconciliation(&context) {
                        stop_subscription(&mut subscription);
                        return;
                    }
                    replica = Some(next_replica);
                    subscription = Some(next_subscription);
                    reconcile_until = snapshot_at + RECONCILE_GRACE;
                    active_tab_reads.clear();
                    defer_background_reads = true;
                    reconnect_delay = RECONNECT_INITIAL_DELAY;
                    next_agent_refresh = Instant::now() + AGENT_REFRESH_INTERVAL;
                    next_operation_tick = Instant::now() + ASYNC_OPERATION_TICK_INTERVAL;
                    if replica
                        .as_ref()
                        .is_some_and(SessionReplica::ready_to_publish)
                        && !publish_replica(
                            &context,
                            replica.as_mut().unwrap(),
                            &mut catalog_cache,
                            &mut purpose_mirror,
                            &mut labels,
                        )
                    {
                        stop_subscription(&mut subscription);
                        return;
                    }
                    if let (Some(home), Some(current)) = (hook_home.as_deref(), replica.as_ref()) {
                        sweep_subagent_counters(home, current);
                    }
                }
                Err(error) => {
                    log_sync_failure(&context, "connect.failed", replica.as_ref(), &error);
                    if !publish_failure(&context, error) {
                        return;
                    }
                    reconnect_at = Instant::now() + reconnect_delay;
                    reconnect_delay = next_reconnect_delay(reconnect_delay);
                    defer_background_reads = true;
                }
            }
        }

        if subscription.is_some()
            && let Some(runtime) = context.runtime.upgrade()
        {
            let requested = match runtime.lock() {
                Ok(mut guard) => guard.take_status_refresh_request(),
                Err(_) => false,
            };
            drop(runtime);
            if requested {
                next_agent_refresh = Instant::now();
            }
        }

        if subscription.is_some() && Instant::now() >= next_agent_refresh {
            next_agent_refresh = Instant::now() + AGENT_REFRESH_INTERVAL;
            match fetch_agents(&context) {
                Ok(agents) => {
                    let current = replica
                        .as_mut()
                        .expect("active subscription always has a replica");
                    let requested =
                        agent_tick_needs_publish(current, &agents, catalog_cache.as_ref());
                    if requested {
                        current.replace_agents(agents);
                        match current.refresh_published_state() {
                            Ok(changed)
                                if (changed || requested)
                                    && !publish_replica(
                                        &context,
                                        current,
                                        &mut catalog_cache,
                                        &mut purpose_mirror,
                                        &mut labels,
                                    ) =>
                            {
                                stop_subscription(&mut subscription);
                                return;
                            }
                            Ok(_) => {}
                            Err(error) => {
                                log_sync_failure(
                                    &context,
                                    "agent_refresh.invalid_projection",
                                    Some(current),
                                    &error,
                                );
                                stop_subscription(&mut subscription);
                                if !publish_failure(&context, error) {
                                    return;
                                }
                                reconnect_at = Instant::now() + reconnect_delay;
                                reconnect_delay = next_reconnect_delay(reconnect_delay);
                            }
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

        if Instant::now() >= next_operation_tick {
            next_operation_tick = Instant::now() + ASYNC_OPERATION_TICK_INTERVAL;
            let now_unix_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
                .unwrap_or(0);
            if let Some(runtime) = context.runtime.upgrade() {
                let changed = match runtime.lock() {
                    Ok(mut guard) => {
                        let changed = guard.tick_async_operations(now_unix_ms);
                        // Agent sleep is this machine's alone (PRD agent-sleep).
                        changed | (context.is_local() && guard.tick_agent_sleep(now_unix_ms))
                    }
                    Err(_) => false,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            }
            if labels.as_mut().is_some_and(|worker| {
                take_label_switch(&context, worker) | worker.tick(Instant::now())
            }) && subscription.is_some()
                && let Some(current) = replica.as_mut()
                && !publish_replica(
                    &context,
                    current,
                    &mut catalog_cache,
                    &mut purpose_mirror,
                    &mut labels,
                )
            {
                stop_subscription(&mut subscription);
                return;
            }
            if subscription.is_some()
                && let Some(current) = replica.as_mut()
                && !current.workspaces_awaiting_active_tab().is_empty()
            {
                match settle_active_tab_reads(&context, current, &mut active_tab_reads) {
                    Ok(true) => {
                        if !publish_replica(
                            &context,
                            current,
                            &mut catalog_cache,
                            &mut purpose_mirror,
                            &mut labels,
                        ) {
                            stop_subscription(&mut subscription);
                            return;
                        }
                    }
                    Ok(false) => {}
                    Err(error) => {
                        log_sync_failure(&context, "active_tab_read.failed", Some(current), &error);
                        stop_subscription(&mut subscription);
                        if !publish_failure(&context, error) {
                            return;
                        }
                        reconnect_at = Instant::now() + reconnect_delay;
                        reconnect_delay = next_reconnect_delay(reconnect_delay);
                    }
                }
            }
        }

        // With no window attached only label work runs; these readers feed
        // nothing else (PRD labels-in-hided B29).
        let run_background_reads =
            subscription.is_some() && !defer_background_reads && read_ui_attached(&context);
        defer_background_reads = false;
        if run_background_reads {
            if let Some(current) = replica.as_mut()
                && process_reader.poll(&context, current, subscription_generation)
            {
                match current.refresh_published_state() {
                    Ok(true) => {
                        publish_replica(
                            &context,
                            current,
                            &mut catalog_cache,
                            &mut purpose_mirror,
                            &mut labels,
                        );
                    }
                    Ok(false) => {}
                    Err(error) => {
                        log_sync_failure(&context, "process.publish_failed", Some(current), &error)
                    }
                }
            }
            if let Some(reader) = usage_reader.as_mut() {
                let Some(activity) = read_usage_activity(&context) else {
                    stop_subscription(&mut subscription);
                    return;
                };
                if let Some(provider_usage) = reader.read_if_due(activity)
                    && !publish_provider_usage(&context, provider_usage)
                {
                    stop_subscription(&mut subscription);
                    return;
                }
            }

            if let Some(home) = hook_home.as_deref() {
                // While the Settings agents tab is on screen the diagnosis is
                // read back once a second, because the hook helper records a
                // report it could not deliver from its own process and that
                // record is the one thing that distinguishes "installed but
                // refused" from "this session started first". Off screen,
                // nothing is read.
                if Instant::now() >= next_hook_diagnosis_refresh {
                    next_hook_diagnosis_refresh = Instant::now() + HOOK_DIAGNOSIS_REFRESH_INTERVAL;
                    let Some(observed) = read_settings_observed(&context) else {
                        stop_subscription(&mut subscription);
                        return;
                    };
                    if observed
                        && !publish_hook_diagnosis(
                            &context,
                            hide_agent_hooks::Diagnosis::read(home),
                        )
                    {
                        stop_subscription(&mut subscription);
                        return;
                    }
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
                // A settled removal already took its worktree out of the
                // catalog, so the rows built on it are rebuilt on this wake
                // rather than when the next read lands.
                let mut rebuild = request.removals != published_removals;
                published_removals = request.removals;
                if let Some(answer) = reader.read_if_due(request) {
                    // New worktree facts change which rows exist and what they
                    // say, so the catalog is rebuilt rather than only stored.
                    match publish_worktrees(&context, answer) {
                        None => {
                            stop_subscription(&mut subscription);
                            return;
                        }
                        Some(changed) => rebuild |= changed,
                    }
                }
                if rebuild
                    && let Some(current) = replica.as_mut()
                    && !publish_replica(
                        &context,
                        current,
                        &mut catalog_cache,
                        &mut purpose_mirror,
                        &mut labels,
                    )
                {
                    stop_subscription(&mut subscription);
                    return;
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

            if let Some(reader) = ai_reader.as_mut() {
                let Some((request, queued_settings)) = read_ai_request(&context) else {
                    stop_subscription(&mut subscription);
                    return;
                };
                // The settings write is file I/O, so it happens here rather than
                // under the runtime mutex that took the operator's choice.
                if let Some(settings) = queued_settings {
                    // The choice has already been taken out of the runtime, so a
                    // home this process could not resolve must not swallow it in
                    // silence; it is the same failure as a refused write and it
                    // reaches the same line on the group.
                    let saved = match hook_home.as_deref() {
                        Some(home) => save_ai_settings(&context, home, &settings),
                        None => {
                            crate::diagnostic!(serde_json::json!({
                                "component": "ai_settings",
                                "kind": "settings.write_skipped",
                                "message": "no home directory on this session",
                            }));
                            report_ai_settings_failure(
                                &context,
                                "The choice could not be saved (no home directory); \
                             it applies to this session only"
                                    .to_string(),
                            )
                        }
                    };
                    if !saved {
                        stop_subscription(&mut subscription);
                        return;
                    }
                }
                if let Some(background_ai) = reader.read_if_due(request)
                    && !publish_background_ai(&context, background_ai)
                {
                    stop_subscription(&mut subscription);
                    return;
                }
            }
        }

        let timeout = coordinator_wait(
            subscription.is_some(),
            reconnect_at,
            next_agent_refresh,
            next_operation_tick,
        );
        match receiver.recv_timeout(timeout) {
            Ok(CoordinatorMessage::Stop) => {
                stop_subscription(&mut subscription);
                return;
            }
            Ok(CoordinatorMessage::SubscriptionLine {
                generation,
                line,
                received_at,
            }) => {
                if subscription.as_ref().map(|active| active.generation) != Some(generation) {
                    continue;
                }
                defer_background_reads = true;
                let mode = if received_at <= reconcile_until {
                    ApplyMode::Reconcile
                } else {
                    ApplyMode::Strict
                };
                match parse_subscription_line(&line) {
                    Ok(SubscriptionLine::Event(event)) => {
                        let current = replica
                            .as_mut()
                            .expect("active subscription always has a replica");
                        match current.apply(event, mode) {
                            Ok(outcome) => {
                                if outcome.refresh_agents {
                                    next_agent_refresh = Instant::now();
                                }
                                if outcome.refresh_worktrees && !request_worktree_refresh(&context)
                                {
                                    stop_subscription(&mut subscription);
                                    return;
                                }
                                if outcome.publish
                                    && !publish_replica(
                                        &context,
                                        current,
                                        &mut catalog_cache,
                                        &mut purpose_mirror,
                                        &mut labels,
                                    )
                                {
                                    stop_subscription(&mut subscription);
                                    return;
                                }
                                if !current.workspaces_awaiting_active_tab().is_empty() {
                                    match settle_active_tab_reads(
                                        &context,
                                        current,
                                        &mut active_tab_reads,
                                    ) {
                                        Ok(true) => {
                                            if !publish_replica(
                                                &context,
                                                current,
                                                &mut catalog_cache,
                                                &mut purpose_mirror,
                                                &mut labels,
                                            ) {
                                                stop_subscription(&mut subscription);
                                                return;
                                            }
                                        }
                                        Ok(false) => {}
                                        Err(error) => {
                                            log_sync_failure(
                                                &context,
                                                "active_tab_read.failed",
                                                Some(current),
                                                &error,
                                            );
                                            stop_subscription(&mut subscription);
                                            if !publish_failure(&context, error) {
                                                return;
                                            }
                                            reconnect_at = Instant::now() + reconnect_delay;
                                            reconnect_delay = next_reconnect_delay(reconnect_delay);
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                log_sync_failure(&context, "event.rejected", Some(current), &error);
                                stop_subscription(&mut subscription);
                                if !publish_failure(&context, error) {
                                    return;
                                }
                                reconnect_at = Instant::now() + reconnect_delay;
                                reconnect_delay = next_reconnect_delay(reconnect_delay);
                            }
                        }
                    }
                    Ok(SubscriptionLine::Error { code, message }) => {
                        crate::diagnostic!(json!({
                            "component": "session_sync",
                            "kind": "subscription.error",
                            "target": context.log_target(),
                            "code": code,
                            "applied_events": replica.as_ref().map(|current| current.applied_events),
                            "message": message,
                        }));
                        stop_subscription(&mut subscription);
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
                defer_background_reads = true;
                stop_subscription(&mut subscription);
                let applied = replica
                    .as_ref()
                    .map(|current| current.applied_events)
                    .unwrap_or(0);
                let error = SessionFetchError::Stale(format!(
                    "Herdr event stream disconnected after {applied} events: {message}"
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
            Ok(CoordinatorMessage::Labels) => {
                // A disconnected replica is stale; its labels wait for the
                // reconnect's first publish rather than clearing the error.
                if labels.as_mut().is_some_and(|worker| {
                    worker.drain(Instant::now()) | take_pull_request_times(&context, worker)
                }) && subscription.is_some()
                    && let Some(current) = replica.as_mut()
                    && !publish_replica(
                        &context,
                        current,
                        &mut catalog_cache,
                        &mut purpose_mirror,
                        &mut labels,
                    )
                {
                    stop_subscription(&mut subscription);
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                stop_subscription(&mut subscription);
                return;
            }
        }
    }
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
pub(crate) fn agent_tick_needs_publish(
    replica: &SessionReplica,
    agents: &[ProjectedAgent],
    catalog_cache: Option<&CatalogCache>,
) -> bool {
    replica.state.agents != agents
        || catalog_cache.is_none_or(|cache| cache.built_at.elapsed() >= CATALOG_REFRESH_INTERVAL)
}

/// Drops the subagent counts of panes Herdr no longer lists.
///
/// A fresh `session.snapshot` is the one moment the pane set is known to be
/// complete, so it is where the sweep belongs: a pane missing from an event
/// stream may only be one Hide has not heard about yet. Nothing on screen
/// depends on it - a pane with no agent projects no children at all - so this
/// is housekeeping, and a failure is recorded rather than escalated (PRD B31,
/// D-53).
fn sweep_subagent_counters(home: &std::path::Path, replica: &SessionReplica) {
    let live = replica.state.panes.iter().map(|pane| pane.pane_id.as_str());
    match hide_agent_hooks::counters::retain(home, live) {
        Ok(0) => {}
        Ok(dropped) => crate::diagnostic!(json!({
            "component": "agent_hooks",
            "kind": "counters.swept",
            "dropped": dropped,
        })),
        Err(error) => crate::diagnostic!(json!({
            "component": "agent_hooks",
            "kind": "counters.sweep_failed",
            "message": error.to_string(),
        })),
    }
}

/// Reads the replacement active tab for every workspace still waiting for
/// one and settles the replica with the answer. Returns whether the
/// published projection changed. A read that names a tab the event stream
/// has not delivered keeps the workspace waiting for the next tick; a
/// workspace still waiting after `ACTIVE_TAB_READ_ATTEMPT_LIMIT` reads is a
/// replica that cannot converge, which the caller rebuilds from a snapshot.
fn settle_active_tab_reads(
    context: &SessionSyncContext,
    replica: &mut SessionReplica,
    attempts: &mut BTreeMap<String, u32>,
) -> Result<bool, SessionFetchError> {
    let waiting = replica.workspaces_awaiting_active_tab();
    attempts.retain(|workspace_id, _| waiting.contains(workspace_id));
    let mut settled = false;
    for workspace_id in waiting {
        let attempt = attempts.entry(workspace_id.clone()).or_insert(0);
        *attempt += 1;
        let attempt = *attempt;
        if attempt > ACTIVE_TAB_READ_ATTEMPT_LIMIT {
            return Err(SessionFetchError::Stale(format!(
                "workspace {workspace_id} named no active tab the event stream knows in {ACTIVE_TAB_READ_ATTEMPT_LIMIT} reads"
            )));
        }
        let active_tab_id = fetch_workspace_active_tab(context, &workspace_id)?;
        let settled_now = replica.settle_active_tab(&workspace_id, &active_tab_id);
        crate::diagnostic!(json!({
            "component": "session_sync",
            "kind": "active_tab.read",
            "target": context.log_target(),
            "workspace_id": workspace_id,
            "tab_id": active_tab_id,
            "attempt": attempt,
            "settled": settled_now,
        }));
        if settled_now {
            attempts.remove(&workspace_id);
            settled = true;
        }
    }
    if !settled {
        return Ok(false);
    }
    replica.refresh_published_state()
}

fn publish_replica(
    context: &SessionSyncContext,
    replica: &mut SessionReplica,
    catalog_cache: &mut Option<CatalogCache>,
    purpose_mirror: &mut Option<live::PurposeMirror>,
    labels: &mut Option<LabelWorker>,
) -> bool {
    let mut payload = replica.project();
    let overlay = labels.as_mut().map(|worker| {
        take_pull_request_times(context, worker);
        take_label_switch(context, worker);
        observe_labels(worker, replica);
        worker.overlay()
    });
    if let SessionSyncTarget::Remote { target_id, .. } = &context.target {
        if let Some(overlay) = &overlay {
            overlay.apply(&mut payload);
        }
        let projection = replica.project_remote(target_id, payload);
        let (fetched, excluded) = match projection {
            Ok((session, excluded)) => (Ok(session), excluded),
            Err(error) => (Err(error), Vec::new()),
        };
        for exclusion in excluded {
            crate::diagnostic!(json!({
                "component": "remote_session",
                "kind": "agent.excluded",
                "target": target_id,
                "pane_id": exclusion.pane_id,
                "source_index": exclusion.source_index,
                "message": exclusion.reason,
            }));
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

    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let (registrations, worktrees, unconfirmed_created_purposes) = match runtime.lock() {
        Ok(guard) => (
            guard.snapshot().ui_state.workspace_registrations.clone(),
            guard.worktree_catalog(),
            guard.unconfirmed_created_purpose_values(),
        ),
        Err(_) => return false,
    };
    drop(runtime);

    let spaces = Runtime::session_spaces(&payload);
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
    if let Some(mirror) = purpose_mirror.as_mut() {
        mirror.sync(
            &cache.spaces,
            &cache.workspaces,
            &unconfirmed_created_purposes,
        );
    }
    let precomputed = PrecomputedCatalog {
        registrations,
        workspaces: cache.workspaces.clone(),
        roots: cache.roots.clone(),
    };

    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => {
            if let Some(overlay) = overlay {
                guard.set_label_overlay(overlay);
            }
            guard.ingest_session_with_catalog(Ok(payload), Some(precomputed))
        }
        Err(_) => return false,
    };
    drop(runtime);
    if changed {
        context.notifier.notify();
    }
    true
}

/// The label worker for this coordinator's Herdr server, built on the
/// core's shared label services. `None` when the core has none (tests) or
/// the worker could not start, which is logged: the rows then show
/// provider names.
fn start_label_worker(
    context: &SessionSyncContext,
    sender: &Sender<CoordinatorMessage>,
) -> Option<LabelWorker> {
    let services = {
        let runtime = context.runtime.upgrade()?;
        let guard = runtime.lock().ok()?;
        guard.label_services()?
    };
    let wake_sender = sender.clone();
    let wake: crate::labels::worker::Wake = Arc::new(move || {
        let _ = wake_sender.send(CoordinatorMessage::Labels);
    });
    let worker = match &context.target {
        SessionSyncTarget::Local { socket_path } => services.local_worker(socket_path, wake),
        SessionSyncTarget::Remote { target_id, .. } => services
            .device_worker(target_id, context.runtime.clone(), wake)
            .map(Some),
    };
    worker.unwrap_or_else(|message| {
        crate::diagnostic!(json!({
            "component": "labels",
            "kind": "worker.start_failed",
            "target": context.log_target(),
            "message": message,
        }));
        None
    })
}

/// Hands this Mac's worker the runtime's pull request creation times, under
/// a brief lock. Only this Mac's projects have their pull requests read (a
/// device's rows link none), so a device's worker is handed none. Returns
/// whether a session's pull requests changed.
fn take_pull_request_times(context: &SessionSyncContext, worker: &mut LabelWorker) -> bool {
    if !context.is_local() {
        return false;
    }
    let times = context
        .runtime
        .upgrade()
        .and_then(|runtime| runtime.lock().ok().map(|guard| guard.pull_request_times()));
    times.is_some_and(|times| worker.set_pull_request_times(times))
}

/// Hands the worker the operator's agent-summary switch, under a brief lock,
/// on every tick and publish: a turned switch reaches the screen and the
/// running request within one tick. Returns whether the shown labels
/// changed.
fn take_label_switch(context: &SessionSyncContext, worker: &mut LabelWorker) -> bool {
    let on = context
        .runtime
        .upgrade()
        .and_then(|runtime| runtime.lock().ok().map(|guard| guard.agent_summary()));
    on.is_some_and(|on| worker.set_summaries(on, Instant::now()))
}

/// Hands the worker the agents and the complete pane topology the replica
/// publishes. Per publish this is one pass over the agents and the panes.
fn observe_labels(worker: &mut LabelWorker, replica: &SessionReplica) {
    let state = &replica.published_state;
    let agents: Vec<ObservedAgent> = state
        .agents
        .iter()
        .map(|agent| ObservedAgent {
            pane_id: agent.pane_id.clone(),
            agent: agent.agent.clone(),
            status: agent.agent_status.clone(),
            reference: agent
                .agent_session
                .as_ref()
                .map(|session| (session.kind.clone(), session.value.clone())),
            cwd: agent.cwd.clone(),
            state_change_seq: agent.state_change_seq,
        })
        .collect();
    let live: HashSet<String> = state
        .panes
        .iter()
        .map(|pane| pane.pane_id.clone())
        .collect();
    let now_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0);
    worker.observe(&agents, Some(&live), Instant::now(), now_unix_ms);
}

/// Marks persisted read records before the first projection from a fresh
/// connection. Restored topology and agent detection can arrive on different
/// ticks, so the runtime keeps this bounded set until each pane is observed or
/// the authoritative topology drops it.
fn begin_local_read_record_reconciliation(context: &SessionSyncContext) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let Ok(mut guard) = runtime.lock() else {
        return false;
    };
    guard.begin_local_read_record_reconciliation();
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

fn read_usage_activity(context: &SessionSyncContext) -> Option<crate::usage::UsageActivity> {
    let runtime = context.runtime.upgrade()?;
    runtime.lock().ok().map(|guard| guard.usage_activity())
}

/// Whether a window draws the snapshot; a gone runtime reads as attached,
/// and the loop's own check ends the coordinator.
fn read_ui_attached(context: &SessionSyncContext) -> bool {
    context
        .runtime
        .upgrade()
        .and_then(|runtime| runtime.lock().ok().map(|guard| guard.ui_attached()))
        .unwrap_or(true)
}

/// Whether the Settings agents tab is on screen. `None` means the runtime is
/// gone.
fn read_settings_observed(context: &SessionSyncContext) -> Option<bool> {
    let runtime = context.runtime.upgrade()?;
    let observed = runtime.lock().ok()?.settings_observed();
    drop(runtime);
    Some(observed)
}

/// Hands the runtime the hook-install judgement, which was read on this
/// thread rather than under the mutex.
fn publish_hook_diagnosis(
    context: &SessionSyncContext,
    diagnosis: hide_agent_hooks::Diagnosis,
) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => guard.ingest_hook_diagnosis(diagnosis),
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

fn read_github_request(context: &SessionSyncContext) -> Option<crate::github::GithubRequest> {
    let runtime = context.runtime.upgrade()?;
    let request = {
        let mut guard = runtime.lock().ok()?;
        guard.reread_pending_checks(Instant::now());
        guard.github_request()
    };
    drop(runtime);
    Some(request)
}

fn read_disk_request(context: &SessionSyncContext) -> Option<crate::disk::DiskRequest> {
    let runtime = context.runtime.upgrade()?;
    let request = runtime.lock().ok()?.disk_request();
    drop(runtime);
    Some(request)
}

/// What the provider probe should ask, and any choice waiting to be written.
///
/// Both come out of one lock acquisition rather than two. The coordinator
/// takes this lock once per wake for each reader it drives, and a settings
/// write is rare enough that it does not deserve a wake of its own.
fn read_ai_request(
    context: &SessionSyncContext,
) -> Option<(crate::ai::AiRequest, Option<hide_ai::AiSettings>)> {
    let runtime = context.runtime.upgrade()?;
    let read = {
        let mut guard = runtime.lock().ok()?;
        (guard.ai_request(), guard.take_ai_settings_save())
    };
    drop(runtime);
    Some(read)
}

/// Reads the operator's saved background AI choice once at startup.
///
/// A file that is not there means nobody has chosen, so the defaults stand
/// and nothing is reported. A file that exists and cannot be read is stated
/// once, here, and the defaults are used with that reason attached rather
/// than in silence.
fn publish_ai_settings(context: &SessionSyncContext, home: &std::path::Path) {
    let path = hide_ai::settings::settings_path(home);
    let (settings, chosen, reason) = match hide_ai::settings::load(home) {
        Ok(settings) => (settings, path.exists(), None),
        Err(error) => {
            crate::diagnostic!(serde_json::json!({
                "component": "ai_settings",
                "kind": "settings.unreadable",
                "message": error.to_string(),
            }));
            (
                hide_ai::AiSettings::default(),
                false,
                Some(format!(
                    "The saved choice could not be read ({error}); the defaults are in use"
                )),
            )
        }
    };
    let Some(runtime) = context.runtime.upgrade() else {
        return;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => guard.ingest_ai_settings(settings, chosen, reason),
        Err(_) => return,
    };
    drop(runtime);
    if changed {
        context.notifier.notify();
    }
}

/// Writes a choice the runtime queued. `false` means the runtime is gone.
fn save_ai_settings(
    context: &SessionSyncContext,
    home: &std::path::Path,
    settings: &hide_ai::AiSettings,
) -> bool {
    let Err(error) = hide_ai::settings::save(home, settings) else {
        // The file now holds the operator's choice, so the group says
        // "chosen" from this write on, not only after the next launch reads it.
        let Some(runtime) = context.runtime.upgrade() else {
            return false;
        };
        let changed = match runtime.lock() {
            Ok(mut guard) => guard.ingest_ai_settings(settings.clone(), true, None),
            Err(_) => return false,
        };
        drop(runtime);
        if changed {
            context.notifier.notify();
        }
        return true;
    };
    crate::diagnostic!(serde_json::json!({
        "component": "ai_settings",
        "kind": "settings.write_failed",
        "message": error.to_string(),
    }));
    report_ai_settings_failure(
        context,
        format!("The choice could not be saved ({error}); it applies to this session only"),
    )
}

/// Puts the reason a choice was not written on the group that took it, and
/// reports whether the coordinator can carry on. A choice the runtime has
/// already handed over is gone either way; what this decides is whether the
/// operator is told.
fn report_ai_settings_failure(context: &SessionSyncContext, message: String) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => guard.report_ai_settings_failure(message),
        Err(_) => return false,
    };
    drop(runtime);
    if changed {
        context.notifier.notify();
    }
    true
}

/// Invalidates the local reader when Herdr reports a worktree topology event.
/// This only changes an in-memory generation while the mutex is held; the git
/// read itself starts later on `WorktreeReader`'s background worker.
fn request_worktree_refresh(context: &SessionSyncContext) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    match runtime.lock() {
        Ok(mut guard) => guard.refresh_worktrees(),
        Err(_) => return false,
    }
    context.notifier.notify();
    true
}

/// Stores a worktree catalog. `None` means the runtime is gone; `Some(true)`
/// means the rows changed and the navigator catalog must be rebuilt.
fn publish_worktrees(
    context: &SessionSyncContext,
    answer: crate::worktrees::WorktreeAnswer,
) -> Option<bool> {
    let runtime = context.runtime.upgrade()?;
    let mut guard = runtime.lock().ok()?;
    let current = answer.observations_current && guard.worktrees_request() == answer.request;
    let changed = guard.ingest_worktrees_answer(answer.catalog, answer.removals, current);
    drop(guard);
    drop(runtime);
    Some(changed)
}

fn publish_github(context: &SessionSyncContext, answer: crate::github::GithubAnswer) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => {
            let current = guard.github_request() == answer.request;
            guard.ingest_github_answer(answer.snapshot, current)
        }
        Err(_) => return false,
    };
    drop(runtime);
    if changed {
        context.notifier.notify();
    }
    true
}

fn publish_disk_usage(
    context: &SessionSyncContext,
    disk: Vec<crate::model::DiskUsageSnapshot>,
) -> bool {
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

fn publish_background_ai(
    context: &SessionSyncContext,
    background_ai: crate::model::BackgroundAiSnapshot,
) -> bool {
    let Some(runtime) = context.runtime.upgrade() else {
        return false;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => guard.ingest_background_ai(background_ai),
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
    next_operation_tick: Instant,
) -> Duration {
    let now = Instant::now();
    let deadline = if subscribed {
        next_agent_refresh.min(next_operation_tick)
    } else {
        reconnect_at.min(next_operation_tick)
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
    if replica.is_none() || matches!(error, SessionFetchError::Protocol { .. }) {
        return error;
    }
    SessionFetchError::Stale(error.message().to_owned())
}
