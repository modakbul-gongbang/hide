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
    let mut lineage_writer = match crate::coordination::lineage::Writer::new(
        context.log_target().to_owned(),
        Arc::clone(&context.api_connector),
        context.runtime.clone(),
    ) {
        Ok(writer) => Some(writer),
        Err(message) => {
            crate::diagnostic!(
                json!({"component":"lineage","kind":"start_failed","message":message})
            );
            None
        }
    };
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
            let snapshot_started_at = Instant::now();
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
                    if let Some(writer) = lineage_writer.as_mut()
                        && let Some(runtime) = context.runtime.upgrade()
                    {
                        let state = runtime
                            .lock()
                            .ok()
                            .and_then(|guard| guard.delivery_state().ok());
                        if let Some(state) = state {
                            writer.observe(
                                &state,
                                &replica.as_ref().unwrap().state.agents,
                                true,
                                snapshot_started_at,
                            );
                        }
                    }
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
            let snapshot_started_at = Instant::now();
            match fetch_agents(&context) {
                Ok(agents) => {
                    let current = replica
                        .as_mut()
                        .expect("active subscription always has a replica");
                    let (native_changed, requested) =
                        agent_tick_changes(current, &agents, catalog_cache.as_ref());
                    if let Some(writer) = lineage_writer.as_mut()
                        && let Some(runtime) = context.runtime.upgrade()
                    {
                        let state = runtime
                            .lock()
                            .ok()
                            .and_then(|guard| guard.delivery_state().ok());
                        if let Some(state) = state {
                            writer.observe(&state, &agents, native_changed, snapshot_started_at);
                        }
                    }
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
            let mut republish_created_tab = false;
            if let Some(runtime) = context.runtime.upgrade() {
                let changed = match runtime.lock() {
                    Ok(mut guard) => {
                        let changed = guard.tick_async_operations(now_unix_ms);
                        // Hide creates tabs only on this machine's Herdr, so
                        // only its replica can place one again.
                        republish_created_tab =
                            context.is_local() && guard.take_created_tab_republish();
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
            if republish_created_tab
                && subscription.is_some()
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
                    worker.drain(Instant::now()) | exchange_pull_requests(&context, worker)
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
fn agent_tick_changes(
    replica: &SessionReplica,
    agents: &[ProjectedAgent],
    catalog_cache: Option<&CatalogCache>,
) -> (bool, bool) {
    let native_changed = replica.state.agents != agents;
    let publish = native_changed
        || catalog_cache.is_none_or(|cache| cache.built_at.elapsed() >= CATALOG_REFRESH_INTERVAL);
    (native_changed, publish)
}

#[cfg(test)]
pub(crate) fn agent_tick_needs_publish(
    replica: &SessionReplica,
    agents: &[ProjectedAgent],
    catalog_cache: Option<&CatalogCache>,
) -> bool {
    agent_tick_changes(replica, agents, catalog_cache).1
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
    // Observe native state before label overlays add UI timestamps. This is
    // bounded memory work; no delivery I/O or notifier is started here.
    if let Some(runtime) = context.runtime.upgrade()
        && let Ok(mut guard) = runtime.lock()
    {
        match &context.target {
            SessionSyncTarget::Local { socket_path } => {
                guard.observe_delivery("local", &payload, socket_path.to_str())
            }
            SessionSyncTarget::Remote { target_id, .. } => {
                guard.observe_delivery(target_id, &payload, None)
            }
        }
    }
    let overlay = labels.as_mut().map(|worker| {
        exchange_pull_requests(context, worker);
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
    let (registrations, worktrees, unconfirmed_created_purposes, created_tab_clamps) =
        match runtime.lock() {
            Ok(guard) => (
                guard.snapshot().ui_state.workspace_registrations.clone(),
                guard.worktree_catalog(),
                guard.unconfirmed_created_purpose_values(),
                guard.created_tab_clamps(),
            ),
            Err(_) => return false,
        };
    drop(runtime);

    // The catalog is built here, outside the lock, from the cwd the runtime
    // will read for a created tab, not the one it was born with.
    // Only a tab this session carries has a cwd to clamp; a record whose tab
    // has not arrived costs a publish nothing.
    let created_tab_clamps = created_tab_clamps
        .into_iter()
        .filter(|(tab_id, _)| {
            payload
                .layouts
                .iter()
                .any(|layout| &layout.tab_id == tab_id)
        })
        .collect::<Vec<_>>();
    let clamped;
    let birth_free = if created_tab_clamps.is_empty() {
        &payload
    } else {
        let mut copy = payload.clone();
        Runtime::clamp_created_tab_cwds(&mut copy, &created_tab_clamps);
        clamped = copy;
        &clamped
    };
    let spaces = Runtime::session_spaces(birth_free);
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
    // The unsequenced stream can still contain a layout from an older focus.
    // A focus that differs from Hide's is read back from Herdr, but the
    // update itself is not held for that answer: it is ingested now with
    // the differing focus left alone, and the read runs on its own worker
    // outside the lock.
    let (changed, readback) = match runtime.lock() {
        Ok(mut guard) => {
            if let Some(overlay) = overlay {
                guard.set_label_overlay(overlay);
            }
            guard.ingest_session_awaiting_focus_readback(Ok(payload), Some(precomputed))
        }
        Err(_) => return false,
    };
    drop(runtime);
    if changed {
        context.notifier.notify();
    }
    if let Some(identity) = readback
        && let Err(message) = spawn_focus_readback(context, identity.clone())
    {
        crate::diagnostic!(json!({
            "component": "pane_focus",
            "kind": "pane.focus.stream_readback_not_started",
            "pane_id": identity.target_id,
            "message": message,
        }));
        if let Some(runtime) = context.runtime.upgrade()
            && let Ok(mut guard) = runtime.lock()
        {
            guard.abandon_pane_focus_readback();
        }
    }
    true
}

/// The worker that reads Herdr's focus for a stream move, outside the runtime
/// lock. It owns the runtime's one readback slot from its start to its last
/// answer, and the guard frees the slot on any other way out.
fn spawn_focus_readback(
    context: &SessionSyncContext,
    first: crate::runtime::PendingPaneFocusControl,
) -> Result<(), String> {
    let connector = Arc::clone(&context.api_connector);
    let runtime = context.runtime.clone();
    let notifier = context.notifier.clone();
    thread::Builder::new()
        .name("herdr-core-focus-readback".to_owned())
        .spawn(move || {
            struct Slot(Weak<Mutex<Runtime>>, bool);
            impl Drop for Slot {
                fn drop(&mut self) {
                    if self.1
                        && let Some(runtime) = self.0.upgrade()
                        && let Ok(mut guard) = runtime.lock()
                    {
                        guard.abandon_pane_focus_readback();
                    }
                }
            }
            let mut slot = Slot(runtime.clone(), true);
            let mut next = Some(first);
            while let Some(identity) = next.take() {
                let result = live::confirm_pane_focus(connector.as_ref(), &identity.target_id);
                if let Err(error) = &result {
                    crate::diagnostic!(json!({
                        "component": "pane_focus",
                        "kind": "pane.focus.stream_readback_failed",
                        "pane_id": identity.target_id,
                        "serial": identity.serial,
                        "connection_generation": identity.live_generation,
                        "message": error.message(),
                    }));
                }
                let Some(runtime) = runtime.upgrade() else {
                    return;
                };
                let (changed, following) = match runtime.lock() {
                    Ok(mut guard) => guard.finish_pane_focus_readback(identity, result.is_ok()),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    notifier.notify();
                }
                slot.1 = following.is_some();
                next = following;
            }
        })
        .map(|_| ())
        .map_err(|error| format!("focus readback worker could not be started: {error}"))
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

/// Hands the runtime the pull requests this Mac's worker sighted that GitHub's
/// answer did not hold, and the worker the runtime's pull request creation
/// times, under one brief lock. Only this Mac's projects have their pull
/// requests read (a device's rows link none), so a device's worker is handed
/// no times and its sightings are dropped. Returns whether a session's pull
/// requests changed.
fn exchange_pull_requests(context: &SessionSyncContext, worker: &mut LabelWorker) -> bool {
    let sighted = worker.take_sighted();
    if !context.is_local() {
        return false;
    }
    let times = context.runtime.upgrade().and_then(|runtime| {
        runtime.lock().ok().map(|mut guard| {
            if !sighted.is_empty() {
                let now_unix_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
                    .unwrap_or(0);
                guard.read_sighted_pull_requests(&sighted, now_unix_ms);
            }
            guard.pull_request_times()
        })
    });
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
        let now = Instant::now();
        guard.reread_pending_checks(now);
        guard.reread_stale_github(now);
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
    if changed {
        context.notifier.notify();
    }
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

#[cfg(test)]
mod worktree_observer_tests {
    use super::*;
    use crate::handle::Core;
    use crate::model::{
        CoreOptions, ProjectWorktreesSnapshot, WorktreeCatalogSnapshot, WorktreeSnapshot,
    };
    use crate::worktrees::WorktreeAnswer;
    use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};

    struct ObserverFixture {
        core: Core,
        runtime: Arc<Mutex<Runtime>>,
        context: SessionSyncContext,
        observations: Receiver<bool>,
        _directory: tempfile::TempDir,
    }

    impl ObserverFixture {
        fn new() -> Self {
            let directory = tempfile::tempdir_in(workspace::temp_base_outside_any_repository())
                .expect("isolated observer directory");
            let root = directory.path().canonicalize().expect("fixture path");
            let path = hide_platform::path::to_wire_lossy(&root);
            let options: CoreOptions = serde_json::from_value(json!({
                "schema_version": crate::model::SCHEMA_VERSION,
                "app_state_path": ""
            }))
            .expect("workerless core options");
            let mut runtime = Runtime::new(
                options,
                crate::environment::EnvironmentReport {
                    statuses: Vec::new(),
                    home_path: None,
                    codex_home: None,
                },
            );
            let catalog = WorktreeCatalogSnapshot {
                projects: vec![ProjectWorktreesSnapshot {
                    root_path: path.clone(),
                    worktrees: vec![WorktreeSnapshot {
                        path: path.clone(),
                        branch: Some("feature/preflight".into()),
                        ignored_repositories: vec!["target/original".into()],
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
            };
            let payload: SessionSnapshotPayload = crate::sidebar::owned_label_fixture(json!({
                "agents": [],
                "focused_pane_id": "w1:p1",
                "focused_workspace_id": "w1",
                "workspaces": [{"workspace_id": "w1", "active_tab_id": "w1:t1"}],
                "tabs": [{"workspace_id": "w1", "tab_id": "w1:t1"}],
                "panes": [{"pane_id": "w1:p1", "cwd": path}],
                "layouts": [{
                    "workspace_id": "w1", "tab_id": "w1:t1", "zoomed": false,
                    "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                    "focused_pane_id": "w1:p1",
                    "panes": [{"pane_id": "w1:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                    "splits": []
                }]
            }))
            .expect("focused project input");
            let spaces = Runtime::session_spaces(&payload);
            let precomputed = PrecomputedCatalog {
                registrations: Vec::new(),
                workspaces: workspace::build_catalog(&[], &spaces, &catalog),
                roots: workspace::root_index(&spaces),
            };
            runtime.ingest_session_with_catalog(Ok(payload), Some(precomputed));
            // A populated baseline preserves the existing initial stale-answer policy.
            runtime.ingest_worktrees(catalog, 0);
            let runtime = Arc::new(Mutex::new(runtime));
            let (core, notifier) = Core::for_runtime_fixture(Arc::clone(&runtime));
            let socket_path = root.join("unused.sock");
            let context = SessionSyncContext::local(&LiveContext {
                socket_path: socket_path.clone(),
                herdr_bin: None,
                runtime: Arc::downgrade(&runtime),
                notifier,
                api_connector: Arc::new(hide_herdr_client::LocalSocketConnector::new(&socket_path)),
            });
            let baseline: Value = serde_json::from_slice(&core.snapshot_delta(0, 0))
                .expect("initial observer snapshot");
            assert_eq!(
                baseline["rest"]["git_worktrees"]["worktrees"][0]["branch"],
                "feature/preflight"
            );
            let (sender, observations) = sync_channel(8);
            let weak_runtime = Arc::downgrade(&runtime);
            core.on_change(move || {
                let unlocked = weak_runtime
                    .upgrade()
                    .is_some_and(|runtime| runtime.try_lock().is_ok());
                // notify catches callback panics, so the caller asserts this witness.
                let _ = sender.try_send(unlocked);
            });
            Self {
                core,
                runtime,
                context,
                observations,
                _directory: directory,
            }
        }

        fn catalog(&self) -> WorktreeCatalogSnapshot {
            self.runtime.lock().expect("runtime").worktree_catalog()
        }

        fn answer(&self, catalog: WorktreeCatalogSnapshot) -> WorktreeAnswer {
            let request = self.runtime.lock().expect("runtime").worktrees_request();
            WorktreeAnswer {
                removals: request.removals,
                request,
                catalog,
                observations_current: true,
            }
        }

        fn snapshot(&self) -> Value {
            serde_json::from_slice(&self.core.snapshot_delta(0, 0)).expect("observer snapshot")
        }

        fn expect_wake_after_unlock(&self) {
            assert!(
                self.observations.try_recv().expect("observer was notified"),
                "the observer must run after releasing the runtime lock"
            );
            self.expect_silence();
        }

        fn expect_silence(&self) {
            assert_eq!(self.observations.try_recv(), Err(TryRecvError::Empty));
        }
    }

    #[test]
    fn changed_worktree_facts_wake_the_observer_and_coalesce_until_its_snapshot() {
        let fixture = ObserverFixture::new();
        let mut catalog = fixture.catalog();
        catalog.projects[0].worktrees[0].lock_reason = Some("Review $(literal)".into());
        let _ = publish_worktrees(&fixture.context, fixture.answer(catalog));
        fixture.expect_wake_after_unlock();

        let mut catalog = fixture.catalog();
        catalog.projects[0].worktrees[0].ignored_repositories =
            vec!["target/review".into(), "vendor/next".into()];
        let _ = publish_worktrees(&fixture.context, fixture.answer(catalog));
        fixture.expect_silence();
        let snapshot = fixture.snapshot();
        let row = &snapshot["rest"]["git_worktrees"]["worktrees"][0];
        assert_eq!(row["lock_reason"], "Review $(literal)");
        assert_eq!(
            row["ignored_repositories"],
            json!(["target/review", "vendor/next"])
        );

        let mut catalog = fixture.catalog();
        catalog.projects[0].worktrees[0].lock_reason = None;
        let _ = publish_worktrees(&fixture.context, fixture.answer(catalog));
        fixture.expect_wake_after_unlock();
        let snapshot = fixture.snapshot();
        assert!(snapshot["rest"]["git_worktrees"]["worktrees"][0]["lock_reason"].is_null());
    }

    #[test]
    fn worktree_loading_completion_wakes_the_observer_with_unchanged_facts() {
        let fixture = ObserverFixture::new();
        let catalog = fixture.catalog();
        fixture.runtime.lock().expect("runtime").refresh_worktrees();
        let before = fixture.snapshot();
        assert_eq!(before["rest"]["git_worktrees_loading"], true);

        let _ = publish_worktrees(&fixture.context, fixture.answer(catalog));
        fixture.expect_wake_after_unlock();
        let after = fixture.snapshot();
        assert_eq!(after["rest"]["git_worktrees_loading"], false);
        assert_eq!(
            after["rest"]["git_worktrees"],
            before["rest"]["git_worktrees"]
        );
    }

    #[test]
    fn unchanged_and_rejected_worktree_answers_leave_the_observer_silent() {
        let fixture = ObserverFixture::new();
        let catalog = fixture.catalog();
        let original_request = fixture.answer(catalog.clone()).request;
        let before = fixture.snapshot();
        let _ = publish_worktrees(&fixture.context, fixture.answer(catalog));
        fixture.expect_silence();
        let after = fixture.snapshot();
        assert_eq!(
            after["rest"]["git_worktrees"],
            before["rest"]["git_worktrees"]
        );
        assert_eq!(after["rest"]["git_worktrees_loading"], false);

        fixture.runtime.lock().expect("runtime").refresh_worktrees();
        assert_eq!(fixture.snapshot()["rest"]["git_worktrees_loading"], true);
        let mut rejected = fixture.catalog();
        rejected.projects[0].worktrees[0].lock_reason = Some("Unaccepted facts".into());
        let mut stale_request = fixture.answer(rejected.clone());
        stale_request.request = original_request;
        let _ = publish_worktrees(&fixture.context, stale_request);
        fixture.expect_silence();
        let after = fixture.snapshot();
        assert_eq!(
            after["rest"]["git_worktrees"],
            before["rest"]["git_worktrees"]
        );
        assert_eq!(after["rest"]["git_worktrees_loading"], true);

        let mut stale_observation = fixture.answer(rejected);
        stale_observation.observations_current = false;
        let _ = publish_worktrees(&fixture.context, stale_observation);
        fixture.expect_silence();
        let after = fixture.snapshot();
        assert_eq!(
            after["rest"]["git_worktrees"],
            before["rest"]["git_worktrees"]
        );
        assert_eq!(after["rest"]["git_worktrees_loading"], true);
    }
}

#[cfg(test)]
mod focus_readback_order_tests {
    use super::*;
    use crate::fake_herdr::FakeHerdr;
    use crate::model::CoreOptions;
    use std::sync::mpsc::channel;

    /// One tab, panes `p1..` side by side. `focused` is Herdr's own focus.
    fn snapshot(panes: &[&str], focused: &str, zoomed: bool) -> Value {
        let width = 120 / panes.len() as u32;
        let pane_rows = |with_focus: bool| {
            panes
                .iter()
                .enumerate()
                .map(|(index, pane)| {
                    json!({
                        "pane_id": format!("w1:{pane}"),
                        "focused": with_focus && *pane == focused,
                        "rect": {"x": index as u32 * width, "y": 0, "width": width, "height": 60}
                    })
                })
                .collect::<Vec<_>>()
        };
        // Each split divides the area its predecessor left, first pane off.
        let splits = (1..panes.len())
            .map(|index| {
                let x = (index as u32 - 1) * width;
                let remaining = 120 - x;
                json!({
                    "id": format!("split_{index}"),
                    "direction": "right",
                    "ratio": width as f32 / remaining as f32,
                    "rect": {"x": x, "y": 0, "width": remaining, "height": 60}
                })
            })
            .collect::<Vec<_>>();
        json!({
            "version": "0.8.2",
            "protocol": hide_herdr_client::HERDR_PROTOCOL_REVISION,
            "focused_pane_id": format!("w1:{focused}"),
            "workspaces": [{
                "workspace_id": "w1", "label": "fixture", "agent_status": "idle",
                "focused": true, "number": 1, "pane_count": panes.len(), "tab_count": 1,
                "active_tab_id": "w1:t1"
            }],
            "tabs": [{
                "workspace_id": "w1", "tab_id": "w1:t1", "agent_status": "idle",
                "focused": false, "number": 1, "pane_count": panes.len(), "label": "1"
            }],
            "panes": panes.iter().map(|pane| json!({
                "workspace_id": "w1", "tab_id": "w1:t1", "pane_id": format!("w1:{pane}"),
                "terminal_id": "fixture-terminal", "focused": false, "revision": 0,
                "agent_status": "idle", "cwd": "/tmp/fixture"
            })).collect::<Vec<_>>(),
            "layouts": [{
                "workspace_id": "w1", "tab_id": "w1:t1", "zoomed": zoomed,
                "area": {"x": 0, "y": 0, "width": 120, "height": 60},
                "focused_pane_id": format!("w1:{focused}"),
                "panes": pane_rows(true),
                "splits": splits
            }],
            "agents": []
        })
    }

    fn runtime_with(first: &SessionReplica) -> Arc<Mutex<Runtime>> {
        let options: CoreOptions = serde_json::from_value(json!({
            "schema_version": crate::model::SCHEMA_VERSION,
            "app_state_path": ""
        }))
        .expect("workerless core options");
        let mut runtime = Runtime::new(
            options,
            crate::environment::EnvironmentReport {
                statuses: Vec::new(),
                home_path: None,
                codex_home: None,
            },
        );
        runtime.ingest_session(Ok(first.project()));
        Arc::new(Mutex::new(runtime))
    }

    fn context_for(runtime: &Arc<Mutex<Runtime>>, herdr: &FakeHerdr) -> SessionSyncContext {
        SessionSyncContext::local(&LiveContext {
            socket_path: herdr.socket_path().to_path_buf(),
            herdr_bin: None,
            runtime: Arc::downgrade(runtime),
            notifier: ChangeNotifier::noop(),
            api_connector: Arc::new(herdr.connector()),
        })
    }

    /// Herdr's answer to the focus readback: `pane.layout` waits for the test
    /// to say go, after telling it the question has arrived.
    fn holding_herdr(
        focused: &'static str,
    ) -> (
        FakeHerdr,
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::Sender<()>,
    ) {
        let (arrived, asked) = channel();
        let (release, held) = channel::<()>();
        let herdr = FakeHerdr::start("focus-readback-order", move |method, _| match method {
            "pane.layout" => {
                arrived.send(()).ok();
                // A dropped sender (the test failed) frees the answer too.
                held.recv().ok();
                json!({"type": "pane_layout", "layout": {
                    "workspace_id": "w1", "tab_id": "w1:t1", "zoomed": true,
                    "area": {"x": 0, "y": 0, "width": 120, "height": 60},
                    "focused_pane_id": format!("w1:{focused}"),
                    "panes": [
                        {"pane_id": "w1:p1", "focused": focused == "p1", "rect": {"x": 0, "y": 0, "width": 60, "height": 60}},
                        {"pane_id": "w1:p2", "focused": focused == "p2", "rect": {"x": 60, "y": 0, "width": 60, "height": 60}}
                    ],
                    "splits": [{"id": "split_1", "direction": "right", "ratio": 0.5, "rect": {"x": 0, "y": 0, "width": 120, "height": 60}}]
                }})
            }
            "workspace.get" => json!({
                "type": "workspace_info",
                "workspace": {"workspace_id": "w1", "number": 1, "label": "fixture", "focused": true, "pane_count": 2, "tab_count": 1, "active_tab_id": "w1:t1", "agent_status": "idle"}
            }),
            other => panic!("unexpected {other}"),
        });
        (herdr, asked, release)
    }

    fn view(runtime: &Arc<Mutex<Runtime>>) -> (Vec<String>, Option<String>, Option<String>) {
        let guard = runtime
            .lock()
            .expect("runtime lock is free during a readback");
        let snapshot = guard.snapshot();
        (
            snapshot
                .pane_layouts
                .iter()
                .flat_map(|layout| layout.pane_ids().into_iter().map(str::to_owned))
                .collect(),
            snapshot.focused.pane_id.clone(),
            snapshot.zoomed.clone(),
        )
    }

    #[test]
    fn a_pane_close_is_ingested_while_herdr_still_holds_the_focus_readback() {
        let first = SessionReplica::from_snapshot(&snapshot(&["p1", "p2", "p3"], "p1", false))
            .expect("first snapshot");
        let runtime = runtime_with(&first);
        // The operator's pane p3 closed, and Herdr's focus moved to p2 by
        // itself, which Hide may follow only after reading it back.
        let mut closed = SessionReplica::from_snapshot(&snapshot(&["p1", "p2"], "p2", true))
            .expect("closed snapshot");
        let (herdr, asked, release) = holding_herdr("p2");
        let context = context_for(&runtime, &herdr);

        let publish = thread::spawn(move || {
            publish_replica(&context, &mut closed, &mut None, &mut None, &mut None)
        });
        asked
            .recv_timeout(Duration::from_secs(30))
            .expect("the readback is asked");

        // Herdr has not answered. The close is what the screen needs now.
        let (panes, focused, zoomed) = view(&runtime);
        assert_eq!(
            panes,
            ["w1:p1", "w1:p2"],
            "the close is ingested before Herdr answers the readback"
        );
        assert_eq!(focused.as_deref(), Some("w1:p1"), "focus is not guessed");
        assert_eq!(zoomed, None);

        release.send(()).expect("the readback is answered");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let (_, focused, zoomed) = view(&runtime);
            if focused.as_deref() == Some("w1:p2") {
                assert_eq!(zoomed.as_deref(), Some("w1:p2"));
                break;
            }
            assert!(Instant::now() < deadline, "the confirmed focus is followed");
            thread::yield_now();
        }
        assert!(publish.join().expect("publish does not panic"));
    }

    #[test]
    fn an_unconfirmed_readback_keeps_hides_focus_and_frees_the_slot_for_the_next_update() {
        let first = SessionReplica::from_snapshot(&snapshot(&["p1", "p2"], "p1", false))
            .expect("first snapshot");
        let runtime = runtime_with(&first);
        let mut moved =
            SessionReplica::from_snapshot(&snapshot(&["p1", "p2"], "p2", false)).expect("moved");
        // Herdr's own answer still names p1: the stream's p2 was stale.
        let (herdr, asked, release) = holding_herdr("p1");
        let context = context_for(&runtime, &herdr);

        assert!(publish_replica(
            &context, &mut moved, &mut None, &mut None, &mut None
        ));
        asked
            .recv_timeout(Duration::from_secs(5))
            .expect("the readback is asked");
        release.send(()).expect("the readback is answered");
        // Both reads are answered once the worker has settled the slot.
        herdr.wait_for_requests(2, Duration::from_secs(30));
        let deadline = Instant::now() + Duration::from_secs(30);
        while runtime.lock().unwrap().pane_focus_readback_is_outstanding() {
            assert!(Instant::now() < deadline, "the slot is freed");
            thread::yield_now();
        }
        assert_eq!(view(&runtime).1.as_deref(), Some("w1:p1"));
    }
}
