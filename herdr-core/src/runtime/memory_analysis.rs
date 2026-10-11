//! Project Memory's background work (PRD project-memory-v2 B1-B6, D-07,
//! D-08, D-10, D-16).
//!
//! One pass at a time walks every Memory-enabled Project of this machine,
//! whatever the screen shows. For each of its sessions it records the
//! receipts a live pane's label read cannot see (a session started outside
//! Herdr, or whose pane is gone), from a JSONL file or OpenCode's database,
//! and analyzes a session once its file has
//! been quiet for [`MEMORY_QUIESCENCE_MS`] or its pane closed. A pass runs on
//! a worker off the runtime lock; the lock is taken only to report a
//! Project's state and to finish.
//!
//! Receipts the label read of a live pane finds come in through
//! [`Runtime::take_memory_handoff`] and are recorded on one owned thread by
//! the same [`record_receipt`] the walk uses, so a session handed from one
//! reader to the other names each receipt by the same turn and never doubles.

use super::memory::{
    MemoryReceipt, SESSION_CALL_TIMEOUT, digest, now_ms, project_identity, project_sessions,
    session_chunk, session_stat, trusted_memory_receipt,
};
use super::*;
use crate::labels::worker::{MemoryHandoff, MemorySession, SightedMemoryReceipt};
use crate::model::{MemoryAnalysisSnapshot, MemoryNoticeSnapshot};
use crate::node::NodeId;
use crate::node_access::NodeLink;
use hide_ai::{AiError, AiRouter, CancelToken};
use hide_memory::{
    AnalysisBatch, ApplySummary, Candidate, CandidateRelation, CursorReader, HideNativeAnalyzer,
    MemoryStore, ReceiptRecord, SessionCursorRecord, SessionSourceRecord,
};
use hide_node_link::protocol::Call;
use hide_session::{EventKind, ProjectSession, SessionAvailability};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};

/// How long a session file stays unchanged before its new conversation is
/// analyzed (D-07): an agent quiet through a build or a test run is not done.
pub(super) const MEMORY_QUIESCENCE_MS: u64 = 30 * 60_000;
/// How often a pass walks the Projects; a closed pane starts one at once.
const MEMORY_PASS_INTERVAL_MS: u64 = 60_000;
/// The first wait after an analysis failure; each further one doubles it up
/// to [`RETRY_LAST_MS`].
const RETRY_FIRST_MS: u64 = 60_000;
const RETRY_LAST_MS: u64 = 60 * 60_000;
/// How long a Project whose provider needs a sign-in waits before it is tried
/// again on its own; the operator's Retry tries at once.
const SIGN_IN_RETRY_MS: u64 = 30 * 60_000;
/// A usage limit that names no resume time is tried again after this.
const USAGE_RETRY_MS: u64 = 60 * 60_000;
/// The analysis provider's weekly usage at which analysis waits (D-10 b).
const WEEKLY_USAGE_DEFER_PERCENT: f64 = 80.0;
/// The most receipts waiting for the recording thread; more means it fell
/// behind, and the oldest go to the log rather than to Memory.
const RECEIPTS_PENDING_LIMIT: usize = 256;
/// The most closed sessions waiting for a pass; a burst past it waits for
/// the quiet rule instead.
const CLOSED_SESSIONS_LIMIT: usize = 256;
/// Bytes of existing cards one analysis compares new text against: with an
/// event group's 36 KiB it keeps a request under the analysis input cap.
const COMPARISON_INPUT_LIMIT_BYTES: usize = 16 * 1024;
/// The text of the new events the comparison set is chosen by.
const COMPARISON_TEXT_LIMIT_BYTES: usize = 16 * 1024;

/// Memory's background state on the runtime, owned by this module.
#[derive(Default)]
pub(super) struct MemoryWork {
    pass_in_flight: bool,
    pub(super) next_pass_unix_ms: u64,
    /// Cancels the running pass; its AI settings, to tell whether a settings
    /// change moved the provider it asks.
    pass_cancel: Option<CancelToken>,
    pass_settings: Option<hide_ai::AiSettings>,
    /// The settings the last change check saw; the same settings read again
    /// change nothing.
    seen_settings: Option<hide_ai::AiSettings>,
    /// Sessions this Mac's live panes hold, whose receipts the label read
    /// records; the walk leaves them to it.
    live_sessions: BTreeSet<MemorySession>,
    /// Sessions whose pane let them go since the last pass started; each is
    /// analyzed without waiting for the quiet rule (B2).
    pub(super) closed_sessions: BTreeSet<MemorySession>,
    receipts_pending: Vec<SightedMemoryReceipt>,
    receipts_in_flight: bool,
    /// Each Project's analysis state and last change, by Memory's Project id.
    pub(super) projects: HashMap<String, ProjectMemoryWork>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct ProjectMemoryWork {
    pub(super) analysis: MemoryAnalysisSnapshot,
    pub(super) notice: Option<MemoryNoticeSnapshot>,
    /// No analysis before this time (a failure's backoff or a usage limit).
    retry_at_unix_ms: u64,
    failures: u32,
}

/// What a pass tells the runtime about one Project while it runs.
enum PassReport {
    Analyzing {
        project_id: String,
    },
    Analyzed {
        project_id: String,
        learned: usize,
        batch_ids: Vec<String>,
    },
    Failed {
        project_id: String,
        error: AiError,
    },
}

/// Why one analysis did not finish.
#[derive(Clone, Debug)]
pub(crate) enum AnalysisFailure {
    Provider(AiError),
    Local(String),
    /// The machine's daily cap or the provider's weekly usage holds it back;
    /// the conversation waits, unread past the cursor (B5).
    Deferred(&'static str),
    Cancelled,
}

impl Runtime {
    /// Takes what the label reads found for Memory: receipts to record off
    /// the lock, and the sessions live panes hold now. A session that left
    /// the set had its pane closed or replaced, and the next pass, started
    /// at once, analyzes it (B2).
    pub(crate) fn take_memory_handoff(&mut self, handoff: MemoryHandoff) {
        if let Some(live) = handoff.live_sessions {
            let work = &mut self.memory_work;
            for gone in work.live_sessions.difference(&live) {
                if work.closed_sessions.len() < CLOSED_SESSIONS_LIMIT {
                    work.closed_sessions.insert(gone.clone());
                }
            }
            if live
                .iter()
                .any(|session| !work.live_sessions.contains(session))
                || !work.closed_sessions.is_empty()
            {
                work.next_pass_unix_ms = 0;
            }
            work.live_sessions = live;
        }
        if !handoff.receipts.is_empty() {
            self.record_memory_receipts(handoff.receipts);
        }
    }

    fn record_memory_receipts(&mut self, receipts: Vec<SightedMemoryReceipt>) {
        let work = &mut self.memory_work;
        work.receipts_pending.extend(receipts);
        if work.receipts_pending.len() > RECEIPTS_PENDING_LIMIT {
            let dropped = work.receipts_pending.len() - RECEIPTS_PENDING_LIMIT;
            work.receipts_pending.drain(..dropped);
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "receipts.capped",
                "dropped": dropped,
            }));
        }
        if work.receipts_in_flight {
            return;
        }
        // Without a worker context the receipts wait for the next hand-over.
        let Some(context) = self.worker_context.clone() else {
            return;
        };
        self.memory_work.receipts_in_flight = true;
        let database = self.memory_database_path();
        let node = self.node.clone();
        let sessions_node = self.own_node();
        let spawn = thread::Builder::new()
            .name("hide-memory-receipts".to_owned())
            .spawn(move || {
                loop {
                    let Some(runtime) = context.runtime.upgrade() else {
                        return;
                    };
                    let batch = match runtime.lock() {
                        Ok(mut guard) => {
                            let work = &mut guard.memory_work;
                            let batch = std::mem::take(&mut work.receipts_pending);
                            if batch.is_empty() {
                                work.receipts_in_flight = false;
                            }
                            batch
                        }
                        Err(_) => return,
                    };
                    drop(runtime);
                    if batch.is_empty() {
                        return;
                    }
                    // A defect in one batch must not leave the flag set, which
                    // would stop every later batch from starting a worker.
                    let recorded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        record_sighted_receipts(&node, sessions_node.as_ref(), &database, batch)
                    }));
                    if recorded.is_err() {
                        crate::diagnostic!(json!({
                            "component": "memory",
                            "kind": "receipts.worker_panicked",
                        }));
                    }
                }
            });
        if let Err(error) = spawn {
            self.memory_work.receipts_in_flight = false;
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "receipts.worker_failed",
                "message": error.to_string(),
            }));
        }
    }

    /// Starts a pass when one is due and none runs. Called from the
    /// coordinator's tick; everything the pass reads or writes happens on its
    /// worker.
    pub(super) fn tick_project_memory(&mut self, now_unix_ms: u64) -> bool {
        if self.memory_work.pass_in_flight || now_unix_ms < self.memory_work.next_pass_unix_ms {
            return false;
        }
        let Some(context) = self.worker_context.clone() else {
            return false;
        };
        let database = self.memory_database_path();
        if !MemoryStore::exists(&database) {
            self.memory_work.next_pass_unix_ms =
                now_unix_ms.saturating_add(MEMORY_PASS_INTERVAL_MS);
            return false;
        }
        // Memory analysis is a model call: with Hide AI off or no agent chosen
        // the walk still records receipts and the analysis waits (B33).
        let settings = self.ai_active().then(|| self.ai_settings.clone()).flatten();
        let weekly_used = settings.as_ref().and_then(|settings| {
            weekly_used_percent(
                &self.snapshot.navigator.provider_usage,
                settings.provider.as_str(),
            )
        });
        let held = self
            .memory_work
            .projects
            .iter()
            .filter(|(_, project)| project.retry_at_unix_ms > now_unix_ms)
            .map(|(id, _)| id.clone())
            .collect::<HashSet<_>>();
        let cancel = CancelToken::new();
        let own_node = self.own_node();
        let router = settings
            .as_ref()
            .map(|settings| crate::ai::memory_router(&own_node, settings));
        let node = self.node.clone();
        let work = &mut self.memory_work;
        work.pass_in_flight = true;
        work.pass_cancel = Some(cancel.clone());
        work.pass_settings = settings.clone();
        let input = PassInput {
            node,
            sessions_node: own_node,
            database,
            router,
            weekly_used,
            live: work.live_sessions.clone(),
            closed: std::mem::take(&mut work.closed_sessions),
            held,
            cancel,
        };
        let report_context = context.clone();
        let spawn = thread::Builder::new()
            .name("hide-project-memory-pass".to_owned())
            .spawn(move || {
                let mut report = |report| apply_pass_report(&report_context, report);
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_pass(&input, &mut report)
                }));
                if outcome.is_err() {
                    crate::diagnostic!(json!({
                        "component": "memory",
                        "kind": "pass.panicked",
                    }));
                }
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.finish_memory_pass(),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawn {
            let work = &mut self.memory_work;
            work.pass_in_flight = false;
            work.pass_cancel = None;
            work.pass_settings = None;
            work.next_pass_unix_ms = now_unix_ms.saturating_add(MEMORY_PASS_INTERVAL_MS);
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "pass.worker_failed",
                "message": error.to_string(),
            }));
        }
        false
    }

    fn finish_memory_pass(&mut self) -> bool {
        let work = &mut self.memory_work;
        work.pass_in_flight = false;
        work.pass_cancel = None;
        work.pass_settings = None;
        // A pane closed while the pass ran asked for the next one at once.
        if work.closed_sessions.is_empty() {
            work.next_pass_unix_ms = now_ms().saturating_add(MEMORY_PASS_INTERVAL_MS);
        }
        let mut changed = false;
        for project in work.projects.values_mut() {
            if project.analysis.state == "analyzing" {
                project.analysis = MemoryAnalysisSnapshot::default();
                changed = true;
            }
        }
        if changed {
            self.sync_project_memory();
        }
        changed
    }

    /// Marks a pass as running on the current settings, without a worker,
    /// and hands back its cancel token.
    #[cfg(test)]
    pub(super) fn pretend_memory_pass(&mut self) -> CancelToken {
        let cancel = CancelToken::new();
        self.memory_work.pass_cancel = Some(cancel.clone());
        self.memory_work.pass_settings = self.ai_settings.clone();
        self.memory_work.seen_settings = self.ai_settings.clone();
        cancel
    }

    /// Stops the running pass; the next one starts on the usual interval.
    pub(super) fn cancel_memory_pass(&mut self) {
        if let Some(cancel) = &self.memory_work.pass_cancel {
            cancel.cancel();
        }
    }

    /// A pass builds its router once, so it would keep asking an agent the
    /// operator has since turned off, removed or replaced: it is stopped
    /// when Hide AI is no longer active or the agents and models it may ask
    /// changed (B33). What a provider failure decided is forgotten, since a
    /// new provider or a sign-in may answer now.
    pub(super) fn memory_ai_settings_changed(&mut self, settings: &hide_ai::AiSettings) {
        if self.memory_work.seen_settings.as_ref() == Some(settings) {
            return;
        }
        let first = self.memory_work.seen_settings.is_none();
        self.memory_work.seen_settings = Some(settings.clone());
        if first {
            return;
        }
        let active = settings.enabled && settings.chosen;
        let moved = self
            .memory_work
            .pass_settings
            .as_ref()
            .is_some_and(|started| !active || !started.routes_like(settings));
        if moved {
            self.cancel_memory_pass();
        }
        let mut changed = false;
        for project in self.memory_work.projects.values_mut() {
            if project.retry_at_unix_ms != 0 || project.analysis.state == "needs_action" {
                project.retry_at_unix_ms = 0;
                project.failures = 0;
                if project.analysis.state == "needs_action" {
                    project.analysis = MemoryAnalysisSnapshot::default();
                }
                changed = true;
            }
        }
        if changed {
            self.memory_work.next_pass_unix_ms = 0;
            self.sync_project_memory();
        }
    }

    /// Analyzes the Project again at once: the operator's Retry, Memory just
    /// turned on, or a card written.
    pub(super) fn wake_project_memory(&mut self, project_id: &str) {
        let project = self
            .memory_work
            .projects
            .entry(project_id.to_owned())
            .or_default();
        project.retry_at_unix_ms = 0;
        project.failures = 0;
        if project.analysis.state == "needs_action" {
            project.analysis = MemoryAnalysisSnapshot::default();
        }
        self.memory_work.next_pass_unix_ms = 0;
    }

    /// Forgets a Project's analysis state when its Memory is turned off or
    /// deleted, and stops a pass that may be analyzing it.
    pub(super) fn quiet_project_memory(&mut self, project_id: &str) {
        self.cancel_memory_pass();
        if let Some(project) = self.memory_work.projects.get_mut(project_id) {
            project.analysis = MemoryAnalysisSnapshot::default();
            project.retry_at_unix_ms = 0;
            project.failures = 0;
        }
    }

    fn apply_memory_report(&mut self, report: PassReport) {
        let now = now_ms();
        match report {
            PassReport::Analyzing { project_id } => {
                let project = self.memory_work.projects.entry(project_id).or_default();
                project.analysis = MemoryAnalysisSnapshot {
                    state: "analyzing".to_owned(),
                    message: None,
                    action: None,
                };
            }
            PassReport::Analyzed {
                project_id,
                learned,
                batch_ids,
            } => {
                let named = self.named_memory_project() == Some(project_id.as_str());
                let project = self.memory_work.projects.entry(project_id).or_default();
                project.analysis = MemoryAnalysisSnapshot::default();
                project.failures = 0;
                project.retry_at_unix_ms = 0;
                if !batch_ids.is_empty() {
                    project.notice = Some(MemoryNoticeSnapshot {
                        message: format!(
                            "Learned {learned} {}",
                            if learned == 1 { "memory" } else { "memories" }
                        ),
                        undo_batch_id: Some(batch_ids.join(",")),
                    });
                }
                if named {
                    self.refresh_project_memory();
                }
            }
            PassReport::Failed { project_id, error } => {
                crate::diagnostic!(json!({
                    "component": "memory",
                    "kind": "analysis.failed",
                    "project_id": project_id,
                    "error": format!("{error:?}"),
                }));
                let project = self.memory_work.projects.entry(project_id).or_default();
                project.failures = project.failures.saturating_add(1);
                let (analysis, retry_at) = analysis_after_failure(&error, project.failures, now);
                project.analysis = analysis;
                project.retry_at_unix_ms = retry_at;
            }
        }
        self.sync_project_memory();
    }
}

/// What a failure shows and when the Project is tried again (B4, B23): only
/// a sign-in or a provider choice reaches the screen, as an action; a usage
/// limit waits for the time the provider named, and anything else backs off
/// in the log alone.
fn analysis_after_failure(
    error: &AiError,
    failures: u32,
    now_unix_ms: u64,
) -> (MemoryAnalysisSnapshot, u64) {
    match error {
        AiError::NotAuthenticated => (
            MemoryAnalysisSnapshot {
                state: "needs_action".to_owned(),
                message: Some("Sign in to the agent Hide AI runs on".to_owned()),
                action: Some("sign_in".to_owned()),
            },
            now_unix_ms.saturating_add(SIGN_IN_RETRY_MS),
        ),
        AiError::NoProvider(_) | AiError::Unsupported(_) => (
            MemoryAnalysisSnapshot {
                state: "needs_action".to_owned(),
                message: Some("Choose an agent for Hide AI".to_owned()),
                action: Some("open_settings".to_owned()),
            },
            now_unix_ms.saturating_add(SIGN_IN_RETRY_MS),
        ),
        AiError::UsageLimited { retry_after } => (
            MemoryAnalysisSnapshot::default(),
            now_unix_ms
                .saturating_add(retry_after.map_or(USAGE_RETRY_MS, |wait| wait.as_millis() as u64)),
        ),
        _ => {
            let backoff = RETRY_FIRST_MS
                .saturating_mul(1_u64 << failures.saturating_sub(1).min(16))
                .min(RETRY_LAST_MS);
            (
                MemoryAnalysisSnapshot::default(),
                now_unix_ms.saturating_add(backoff),
            )
        }
    }
}

fn apply_pass_report(context: &RuntimeWorkerContext, report: PassReport) {
    let Some(runtime) = context.runtime.upgrade() else {
        return;
    };
    let applied = match runtime.lock() {
        Ok(mut guard) => {
            guard.apply_memory_report(report);
            true
        }
        Err(_) => false,
    };
    drop(runtime);
    if applied {
        context.notifier.notify();
    }
}

/// The weekly share of `provider`'s usage already spent, when the toolbar's
/// usage read knows it.
fn weekly_used_percent(
    rows: &[crate::model::ProviderUsageSnapshot],
    provider: &str,
) -> Option<f64> {
    rows.iter()
        .find(|row| row.provider == provider)
        .filter(|row| row.window_minutes == crate::usage::WEEKLY_WINDOW_MINUTES)
        .and_then(|row| row.used_percent)
}

/// What one pass needs, taken under the lock when it starts.
struct PassInput {
    node: NodeId,
    sessions_node: Arc<dyn NodeLink>,
    database: PathBuf,
    /// `None` while Hide AI is off: the pass only records receipts.
    router: Option<AiRouter>,
    weekly_used: Option<f64>,
    live: BTreeSet<MemorySession>,
    closed: BTreeSet<MemorySession>,
    /// Projects whose analysis waits for a retry time.
    held: HashSet<String>,
    cancel: CancelToken,
}

fn run_pass(input: &PassInput, report: &mut dyn FnMut(PassReport)) {
    let mut store = match MemoryStore::open(&input.database) {
        Ok(store) => store,
        Err(error) => {
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "pass.store_unavailable",
                "message": error.to_string(),
            }));
            return;
        }
    };
    let projects = match store.enabled_projects(input.node.as_str()) {
        Ok(projects) => projects,
        Err(error) => {
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "pass.projects_unreadable",
                "message": error.to_string(),
            }));
            return;
        }
    };
    // A deferral holds for the rest of the pass and is logged once.
    let mut deferred = None;
    for (project_id, root) in projects {
        if input.cancel.is_cancelled() {
            return;
        }
        let outcome = walk_project(input, &mut store, &project_id, &root, &mut deferred, report);
        if let Err(message) = outcome {
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "pass.project_failed",
                "project_id": project_id,
                "message": message,
            }));
        }
    }
    if let Some(reason) = deferred {
        crate::diagnostic!(json!({
            "component": "memory",
            "kind": "analysis.deferred",
            "reason": reason,
        }));
    }
}

fn walk_project(
    input: &PassInput,
    store: &mut MemoryStore,
    project_id: &str,
    root: &str,
    deferred: &mut Option<&'static str>,
    report: &mut dyn FnMut(PassReport),
) -> Result<(), String> {
    let sessions_node = input.sessions_node.as_ref();
    let identity = project_identity(sessions_node, &input.node, root)?;
    if identity.id != project_id {
        return Err(format!("project_moved:{}", identity.id));
    }
    store
        .prune_receipts(project_id, now_ms())
        .map_err(|error| error.to_string())?;
    // Memory analyzes the sessions of agents this build has a Memory reader
    // for (the JSONL ones: Claude Code, Codex, Grok, Pi, omp); an OpenCode
    // session, kept in OpenCode's database, is walked for its receipts alone
    // through the conversation reader (D-16).
    let readers = hide_node_link::sessions::ReaderFeatures::implemented();
    let mut sessions = Vec::new();
    for session in project_sessions(sessions_node, &identity)? {
        if readers.supports(
            session.agent.as_str(),
            hide_node_link::sessions::ReaderFeature::Memory,
        ) {
            sessions.push(session);
        } else if session.agent == hide_session::Agent::OpenCode
            && matches!(session.availability, SessionAvailability::Available)
            && !input
                .live
                .contains(&(session.agent.as_str(), session.id.clone()))
            && !input.cancel.is_cancelled()
            && let Err(message) = read_database_receipts(store, sessions_node, project_id, &session)
        {
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "receipts.walk_failed",
                "project_id": project_id,
                "provider": session.agent.as_str(),
                "message": message,
            }));
        }
    }
    let mut analyzing = false;
    let mut learned = 0;
    let mut batch_ids = Vec::new();
    for session in sessions {
        if input.cancel.is_cancelled() {
            break;
        }
        let availability = match &session.availability {
            SessionAvailability::Available => None,
            SessionAvailability::Unavailable { reason } => Some(reason.clone()),
        };
        store
            .upsert_session_source(&SessionSourceRecord {
                id: session.id.clone(),
                project_id: project_id.to_owned(),
                provider: session.agent.as_str().to_owned(),
                locator: session.locator.to_string_lossy().into_owned(),
                checkout_path: session.checkout_path.to_string_lossy().into_owned(),
                started_at_unix_ms: session.started_at_unix_ms,
                updated_at_unix_ms: session.updated_at_unix_ms,
                unavailable_reason: availability.clone(),
            })
            .map_err(|error| error.to_string())?;
        // A session that cannot be read is no failure of Memory's: it is
        // listed with its reason and read again when it comes back.
        if availability.is_some() {
            continue;
        }
        let key = (session.agent.as_str(), session.id.clone());
        let Ok(stat) = session_stat(sessions_node, &session) else {
            continue;
        };
        if !input.live.contains(&key)
            && let Err(message) = read_receipts(store, sessions_node, project_id, &session, &stat)
        {
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "receipts.walk_failed",
                "project_id": project_id,
                "provider": session.agent.as_str(),
                "message": message,
            }));
        }
        let Some(router) = input.router.as_ref() else {
            continue;
        };
        if deferred.is_some() || input.held.contains(project_id) {
            continue;
        }
        let closed = input.closed.contains(&key) && !input.live.contains(&key);
        let quiet = stat
            .modified_unix_ms
            .is_some_and(|modified| now_ms().saturating_sub(modified) >= MEMORY_QUIESCENCE_MS);
        if !(closed || quiet) || !analysis_pending(store, project_id, &session, &stat)? {
            continue;
        }
        // A held-back call is logged, never shown: the Project does not turn
        // "analyzing" for a pass that will ask nothing (B5).
        if let Some(reason) = deferral(store, input.weekly_used, true)? {
            *deferred = Some(reason);
            continue;
        }
        if !analyzing {
            analyzing = true;
            report(PassReport::Analyzing {
                project_id: project_id.to_owned(),
            });
        }
        let mut admit = |store: &MemoryStore| admit_call(store, input.weekly_used, true);
        match analyze_session(
            store,
            sessions_node,
            router,
            project_id,
            &session,
            &input.cancel,
            &mut admit,
        ) {
            Ok(result) => {
                learned += result.learned;
                batch_ids.extend(result.batch_ids);
            }
            Err(AnalysisFailure::Provider(error)) => {
                report(PassReport::Failed {
                    project_id: project_id.to_owned(),
                    error,
                });
                return Ok(());
            }
            Err(AnalysisFailure::Deferred(reason)) => *deferred = Some(reason),
            Err(AnalysisFailure::Cancelled) => break,
            Err(AnalysisFailure::Local(message)) => crate::diagnostic!(json!({
                "component": "memory",
                "kind": "analysis.session_failed",
                "project_id": project_id,
                "provider": session.agent.as_str(),
                "message": message,
            })),
        }
    }
    if analyzing {
        report(PassReport::Analyzed {
            project_id: project_id.to_owned(),
            learned,
            batch_ids,
        });
    }
    Ok(())
}

/// Admits one analysis call against the machine's daily cap and, for
/// background analysis, the provider's weekly usage (D-10), and counts it.
pub(crate) fn admit_call(
    store: &MemoryStore,
    weekly_used: Option<f64>,
    background: bool,
) -> Result<(), AnalysisFailure> {
    if let Some(reason) =
        deferral(store, weekly_used, background).map_err(AnalysisFailure::Local)?
    {
        return Err(AnalysisFailure::Deferred(reason));
    }
    store
        .count_analysis_call(now_ms())
        .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
    Ok(())
}

/// Why a call would be held back now, if it would: the provider's weekly
/// usage for background analysis, or the machine's calls today.
fn deferral(
    store: &MemoryStore,
    weekly_used: Option<f64>,
    background: bool,
) -> Result<Option<&'static str>, String> {
    if background && weekly_used.is_some_and(|used| used >= WEEKLY_USAGE_DEFER_PERCENT) {
        return Ok(Some("weekly_usage"));
    }
    let calls = store
        .analysis_calls(now_ms())
        .map_err(|error| error.to_string())?;
    Ok((calls >= hide_memory::DAILY_ANALYSIS_CALL_LIMIT).then_some("daily_cap"))
}

fn analysis_pending(
    store: &MemoryStore,
    project_id: &str,
    session: &ProjectSession,
    stat: &hide_node_link::sessions::SessionStat,
) -> Result<bool, String> {
    let cursor = store
        .load_cursor(
            CursorReader::Analysis,
            project_id,
            session.agent.as_str(),
            &session.id,
        )
        .map_err(|error| error.to_string())?;
    Ok(cursor_behind(cursor.as_ref(), stat))
}

fn cursor_behind(
    cursor: Option<&SessionCursorRecord>,
    stat: &hide_node_link::sessions::SessionStat,
) -> bool {
    match cursor {
        None => stat.size > 0,
        Some(cursor) => {
            stat.size != cursor.byte_offset
                || stat.modified_unix_ms.unwrap_or_default() > cursor.updated_at_unix_ms
        }
    }
}

/// Records the receipts the session stored past the receipt cursor: one
/// bounded read per pass, so a long file is caught up over several.
fn read_receipts(
    store: &mut MemoryStore,
    sessions_node: &dyn NodeLink,
    project_id: &str,
    session: &ProjectSession,
    stat: &hide_node_link::sessions::SessionStat,
) -> Result<(), String> {
    let provider = session.agent.as_str();
    let saved = store
        .load_cursor(CursorReader::Receipts, project_id, provider, &session.id)
        .map_err(|error| error.to_string())?;
    if !cursor_behind(saved.as_ref(), stat) {
        return Ok(());
    }
    let chunk = session_chunk(sessions_node, session, saved.as_ref())?;
    let checkpoint = serde_json::to_vec(&chunk.checkpoint).map_err(|error| error.to_string())?;
    // Joined as the label read joins a message's parts, so a receipt in a
    // later part is named by the same turn whichever reader finds it.
    let mut parsed =
        hide_session::parse_events_at(session.agent, &chunk.contents, chunk.start_offset);
    parsed.coalesce();
    for (event, offset) in parsed.events.iter().zip(&parsed.event_offsets) {
        if event.kind != EventKind::Injected {
            continue;
        }
        record_receipt(
            store,
            project_id,
            provider,
            &session.id,
            event.is_provider_injected(),
            &event.text,
            *offset,
        )?;
    }
    store
        .save_cursor(
            CursorReader::Receipts,
            &SessionCursorRecord {
                project_id: project_id.to_owned(),
                provider: provider.to_owned(),
                session_id: session.id.clone(),
                byte_offset: chunk.offset,
                checkpoint,
                last_content_hash: None,
                updated_at_unix_ms: now_ms(),
            },
        )
        .map_err(|error| error.to_string())
}

/// Records the receipts an OpenCode session stored past the receipt cursor,
/// read message by message through the conversation reader the label read
/// uses, so a receipt carries the same message index, and the same turn,
/// whichever of the two found it. One bounded read per pass; the cursor keeps
/// the session time it was read up to, or none while more remains.
fn read_database_receipts(
    store: &mut MemoryStore,
    sessions_node: &dyn NodeLink,
    project_id: &str,
    session: &ProjectSession,
) -> Result<(), String> {
    let provider = session.agent.as_str();
    let saved = store
        .load_cursor(CursorReader::Receipts, project_id, provider, &session.id)
        .map_err(|error| error.to_string())?;
    if saved
        .as_ref()
        .is_some_and(|cursor| cursor.updated_at_unix_ms >= session.updated_at_unix_ms)
    {
        return Ok(());
    }
    let checkpoint = saved
        .as_ref()
        .filter(|record| !record.checkpoint.is_empty())
        .map(|record| serde_json::from_slice(&record.checkpoint))
        .transpose()
        .map_err(|error| format!("session_checkpoint:{error}"))?;
    let transcript: hide_session::label_transcript::LabelTranscript =
        hide_node_link::link::call_as_reader(
            sessions_node,
            session.agent,
            hide_node_link::sessions::ReaderFeature::Conversation,
            Call::LabelTranscript {
                request: hide_session::label_transcript::LabelTranscriptRequest {
                    agent: session.agent,
                    reference_kind: "id".into(),
                    reference_value: session.id.clone(),
                    cwd: Some(session.checkout_path.to_string_lossy().into_owned()),
                    checkpoint,
                    subagents: Default::default(),
                    turns: None,
                },
            },
            SESSION_CALL_TIMEOUT,
        )
        .map_err(|error| error.to_string())?;
    for receipt in &transcript.memory_receipts {
        record_receipt(
            store,
            project_id,
            provider,
            &session.id,
            true,
            &receipt.text,
            receipt.offset,
        )?;
    }
    store
        .save_cursor(
            CursorReader::Receipts,
            &SessionCursorRecord {
                project_id: project_id.to_owned(),
                provider: provider.to_owned(),
                session_id: session.id.clone(),
                byte_offset: transcript.checkpoint.offset(),
                checkpoint: serde_json::to_vec(&transcript.checkpoint)
                    .map_err(|error| error.to_string())?,
                last_content_hash: None,
                updated_at_unix_ms: if transcript.has_more {
                    0
                } else {
                    session.updated_at_unix_ms
                },
            },
        )
        .map_err(|error| error.to_string())
}

/// Records one receipt a session carried, when its tag is the one this
/// machine's Memory signed for the Project, provider and session; anything
/// else is no delivery (`Ok(false)`). A receipt past the Project's cap, or
/// one naming a card this Project no longer has, is logged and passed over;
/// a store that could not write answers the failure, so the caller keeps its
/// cursor and reads the receipt again.
fn record_receipt(
    store: &mut MemoryStore,
    project_id: &str,
    provider: &str,
    session_id: &str,
    provider_injected: bool,
    text: &str,
    offset: u64,
) -> Result<bool, String> {
    let Some(receipt) = trusted_memory_receipt(
        store,
        project_id,
        provider,
        session_id,
        provider_injected,
        text,
    ) else {
        return Ok(false);
    };
    // A receipt naming no card records nothing.
    if receipt.item_key().is_empty() {
        return Ok(true);
    }
    match store.record_receipt(
        project_id,
        provider,
        session_id,
        &MemoryReceipt::turn_id(offset),
        receipt.item_key(),
        now_ms(),
    ) {
        Ok(ReceiptRecord::Recorded) => {}
        Ok(ReceiptRecord::Capped) => crate::diagnostic!(json!({
            "component": "memory",
            "kind": "receipts.project_capped",
            "project_id": project_id,
            "provider": provider,
        })),
        Err(
            error @ (hide_memory::MemoryError::CrossProjectTarget
            | hide_memory::MemoryError::InvalidState(_)),
        ) => crate::diagnostic!(json!({
            "component": "memory",
            "kind": "receipts.unrecordable",
            "project_id": project_id,
            "provider": provider,
            "message": error.to_string(),
        })),
        Err(error) => {
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "receipts.write_failed",
                "project_id": project_id,
                "provider": provider,
                "message": error.to_string(),
            }));
            return Err(error.to_string());
        }
    }
    Ok(true)
}

/// Records the receipts the label reads of live panes found, each against
/// the Project its pane's cwd names. One this store could not take is not
/// lost: the walk reads a session from its own receipt cursor once no live
/// pane holds it.
fn record_sighted_receipts(
    node: &NodeId,
    sessions_node: &dyn NodeLink,
    database: &Path,
    receipts: Vec<SightedMemoryReceipt>,
) {
    // No Memory store: nothing was signed, so nothing can be recorded.
    if !MemoryStore::exists(database) {
        return;
    }
    let mut store = match MemoryStore::open(database) {
        Ok(store) => store,
        Err(error) => {
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "receipts.store_unavailable",
                "message": error.to_string(),
            }));
            return;
        }
    };
    let mut projects = HashMap::new();
    let (mut recorded, mut refused) = (0_usize, 0_usize);
    for sighted in receipts {
        let identity = projects
            .entry(sighted.cwd.clone())
            .or_insert_with(|| project_identity(sessions_node, node, &sighted.cwd));
        let Ok(identity) = identity else {
            refused += 1;
            continue;
        };
        if matches!(
            record_receipt(
                &mut store,
                &identity.id,
                sighted.provider,
                &sighted.session_id,
                true,
                &sighted.text,
                sighted.offset,
            ),
            Ok(true)
        ) {
            recorded += 1;
        } else {
            refused += 1;
        }
    }
    if refused > 0 {
        crate::diagnostic!(json!({
            "component": "memory",
            "kind": "receipts.refused",
            "recorded": recorded,
            "refused": refused,
        }));
    }
}

pub(super) struct SessionAnalysis {
    pub(super) learned: usize,
    pub(super) batch_ids: Vec<String>,
}

/// Analyzes the session's conversation past the analysis cursor, one
/// provider call per bounded group, and moves the cursor only when every
/// group was written.
fn analyze_session(
    store: &mut MemoryStore,
    sessions_node: &dyn NodeLink,
    router: &AiRouter,
    project_id: &str,
    session: &ProjectSession,
    cancel: &CancelToken,
    admit: &mut dyn FnMut(&MemoryStore) -> Result<(), AnalysisFailure>,
) -> Result<SessionAnalysis, AnalysisFailure> {
    let local = |error: String| AnalysisFailure::Local(error);
    let provider = session.agent.as_str();
    let saved = store
        .load_cursor(CursorReader::Analysis, project_id, provider, &session.id)
        .map_err(|error| local(error.to_string()))?;
    let chunk = session_chunk(sessions_node, session, saved.as_ref()).map_err(local)?;
    let checkpoint =
        serde_json::to_vec(&chunk.checkpoint).map_err(|error| local(error.to_string()))?;
    let mut parsed =
        hide_session::parse_events_at(session.agent, &chunk.contents, chunk.start_offset);
    parsed.coalesce();
    let events = parsed
        .events
        .into_iter()
        .zip(parsed.event_offsets)
        .filter(|(event, _)| matches!(event.kind, EventKind::Human | EventKind::Assistant))
        .map(|(event, offset)| {
            json!({
                "offset": offset,
                "role": event.role,
                "kind": event.kind.as_str(),
                "at_unix_ms": event.at_unix_ms,
                "text": event.text,
            })
        })
        .collect::<Vec<_>>();
    let checkout = session.checkout_path.to_string_lossy().into_owned();
    let mut learned = 0;
    let mut batch_ids = Vec::new();
    let mut last_hash = saved.and_then(|record| record.last_content_hash);
    for group in event_groups(&events).map_err(local)? {
        if cancel.is_cancelled() {
            return Err(AnalysisFailure::Cancelled);
        }
        let written = write_cards(
            store,
            sessions_node,
            router,
            &BatchSource {
                project_id,
                provider,
                session_id: &session.id,
                checkout_path: &checkout,
            },
            &group,
            cancel,
            admit,
        )?;
        last_hash = Some(written.content_hash);
        if !written.summary.duplicate_batch {
            learned += written.summary.created + written.summary.superseded;
            if written.summary.created + written.summary.superseded + written.summary.conflicts > 0
            {
                batch_ids.push(written.batch_id);
            }
        }
    }
    store
        .save_cursor(
            CursorReader::Analysis,
            &SessionCursorRecord {
                project_id: project_id.to_owned(),
                provider: provider.to_owned(),
                session_id: session.id.clone(),
                byte_offset: chunk.offset,
                checkpoint,
                last_content_hash: last_hash,
                updated_at_unix_ms: now_ms(),
            },
        )
        .map_err(|error| local(error.to_string()))?;
    Ok(SessionAnalysis { learned, batch_ids })
}

/// Where a batch of new text came from.
pub(crate) struct BatchSource<'a> {
    pub(crate) project_id: &'a str,
    /// The session's agent, or `hide` for `hide memory add`.
    pub(crate) provider: &'a str,
    pub(crate) session_id: &'a str,
    /// The checkout cited paths are checked in.
    pub(crate) checkout_path: &'a str,
}

pub(crate) struct WrittenCards {
    pub(crate) summary: ApplySummary,
    pub(crate) batch_id: String,
    content_hash: String,
}

/// The one write path for new text, background analysis and `hide memory
/// add` alike (D-11): the cards it is compared against, one provider call,
/// the answer checked against what was sent, cited paths checked in the
/// checkout, and the cards written in one batch. A batch already written
/// asks no provider again.
#[allow(clippy::too_many_arguments)] // one batch: its store, nodes, source, text and limits
pub(crate) fn write_cards(
    store: &mut MemoryStore,
    sessions_node: &dyn NodeLink,
    router: &AiRouter,
    source: &BatchSource<'_>,
    group: &[Value],
    cancel: &CancelToken,
    admit: &mut dyn FnMut(&MemoryStore) -> Result<(), AnalysisFailure>,
) -> Result<WrittenCards, AnalysisFailure> {
    let local = |error: String| AnalysisFailure::Local(error);
    let serialized = serde_json::to_string(group).map_err(|error| local(error.to_string()))?;
    let content_hash = digest(&[
        source.project_id,
        source.provider,
        source.session_id,
        &serialized,
    ]);
    let batch_id = digest(&[
        "memory-batch",
        source.project_id,
        source.provider,
        source.session_id,
        &content_hash,
    ]);
    if store
        .has_batch(&batch_id)
        .map_err(|error| local(error.to_string()))?
    {
        return Ok(WrittenCards {
            summary: ApplySummary {
                duplicate_batch: true,
                ..ApplySummary::default()
            },
            batch_id,
            content_hash,
        });
    }
    let similar = store
        .similar(
            source.project_id,
            &comparison_text(group),
            hide_memory::COMPARISON_LIMIT,
            COMPARISON_INPUT_LIMIT_BYTES,
        )
        .map_err(|error| local(error.to_string()))?;
    let analyzer = HideNativeAnalyzer;
    let request_id = digest(&[
        "memory-request",
        source.project_id,
        source.provider,
        source.session_id,
        &content_hash,
    ]);
    let request = analyzer
        .request(
            request_id,
            source.project_id,
            source.session_id,
            &serialized,
            &similar,
        )
        .map_err(|error| local(error.to_string()))?;
    admit(store)?;
    let answer = router
        .execute(&request, cancel)
        .map_err(|error| match error {
            AiError::Cancelled => AnalysisFailure::Cancelled,
            error => AnalysisFailure::Provider(error),
        })?;
    let output = analyzer
        .parse(answer.value, &similar)
        .map_err(|error| AnalysisFailure::Provider(AiError::InvalidOutput(format!("{error:?}"))))?;
    let mut candidates = output.candidates;
    let unsourced = validate_candidates(&mut candidates, group);
    if unsourced + output.secret_rejected > 0 {
        crate::diagnostic!(json!({
            "component": "memory",
            "kind": "analysis.candidates_left_out",
            "project_id": source.project_id,
            "provider": source.provider,
            "unsourced": unsourced,
            "secret": output.secret_rejected,
        }));
    }
    keep_cited_paths(sessions_node, source.checkout_path, &mut candidates);
    let summary = store
        .apply_candidates(
            &AnalysisBatch {
                id: batch_id.clone(),
                project_id: source.project_id.to_owned(),
                provider: source.provider.to_owned(),
                analysis_provider: answer.provider.as_str().to_owned(),
                session_id: source.session_id.to_owned(),
                content_hash: content_hash.clone(),
                created_at_unix_ms: now_ms(),
            },
            &candidates,
        )
        .map_err(|error| local(error.to_string()))?;
    for backfill in output.backfill {
        if let Err(error) = store.fill_aliases(
            source.project_id,
            &backfill.target_id,
            &backfill.title,
            &backfill.aliases,
        ) {
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "analysis.alias_backfill_failed",
                "project_id": source.project_id,
                "message": error.to_string(),
            }));
        }
    }
    Ok(WrittenCards {
        summary,
        batch_id,
        content_hash,
    })
}

/// The text the comparison set is chosen by: the group's words, bounded.
fn comparison_text(group: &[Value]) -> String {
    let mut text = String::new();
    for event in group {
        let Some(part) = event.get("text").and_then(Value::as_str) else {
            continue;
        };
        if text.len() + part.len() > COMPARISON_TEXT_LIMIT_BYTES {
            let room = COMPARISON_TEXT_LIMIT_BYTES.saturating_sub(text.len());
            let mut end = room.min(part.len());
            while end > 0 && !part.is_char_boundary(end) {
                end -= 1;
            }
            text.push_str(&part[..end]);
            break;
        }
        text.push_str(part);
        text.push('\n');
    }
    text
}

/// Checks the answer against what was sent: a candidate citing an event
/// outside the group is left out, the operator's own words are the sources
/// whose event is `human`, and a correction without them waits for review
/// instead (D-14). Answers how many were left out.
pub(super) fn validate_candidates(candidates: &mut Vec<Candidate>, events: &[Value]) -> usize {
    let offsets = events
        .iter()
        .filter_map(|event| event.get("offset")?.as_u64())
        .collect::<HashSet<_>>();
    let human = events
        .iter()
        .filter_map(|event| {
            (event.get("kind")?.as_str()? == "human")
                .then(|| event.get("offset")?.as_u64())
                .flatten()
        })
        .collect::<HashSet<_>>();
    let before = candidates.len();
    candidates.retain(|candidate| {
        candidate
            .source_offsets
            .iter()
            .all(|offset| offsets.contains(offset))
    });
    for candidate in candidates.iter_mut() {
        candidate.human_offsets = candidate
            .source_offsets
            .iter()
            .copied()
            .filter(|offset| human.contains(offset))
            .collect();
        if !candidate.direct_human_source()
            && let CandidateRelation::Supersedes { target_id } = &candidate.relation
        {
            candidate.relation = CandidateRelation::Conflicts {
                target_id: target_id.clone(),
            };
        }
    }
    before - candidates.len()
}

/// Keeps only the cited paths that name a file or folder inside the
/// checkout, as the node that holds it resolves them (B27): a path outside
/// it, one that does not exist, or one the node cannot read is dropped.
fn keep_cited_paths(sessions_node: &dyn NodeLink, checkout: &str, candidates: &mut [Candidate]) {
    let cited = candidates
        .iter()
        .flat_map(|candidate| candidate.cited_paths.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if cited.is_empty() {
        return;
    }
    let mut paths = vec![checkout.to_owned()];
    paths.extend(
        cited
            .iter()
            .map(|relative| format!("{}/{relative}", checkout.trim_end_matches('/'))),
    );
    use hide_node_link::cleanup::PathState;
    let states = crate::node_access::call_as::<Vec<PathState>>(
        sessions_node,
        hide_node_link::protocol::Call::RealPaths { paths },
        SESSION_CALL_TIMEOUT,
    );
    let kept = match states {
        Ok(states) if states.len() == cited.len() + 1 => {
            let root = match &states[0] {
                PathState::Real { path } => format!("{}/", path.trim_end_matches('/')),
                _ => String::new(),
            };
            cited
                .iter()
                .zip(&states[1..])
                .filter(|(_, state)| {
                    !root.is_empty()
                        && matches!(state, PathState::Real { path } if path.starts_with(&root))
                })
                .map(|(relative, _)| relative.clone())
                .collect::<HashSet<_>>()
        }
        _ => HashSet::new(),
    };
    for candidate in candidates {
        candidate.cited_paths.retain(|path| kept.contains(path));
    }
}

/// The new events cut into groups, each sent in one provider call.
/// The most text of one event analysis reads.
const EVENT_TEXT_LIMIT_BYTES: usize = 48 * 1024;

pub(super) fn event_groups(events: &[Value]) -> Result<Vec<Vec<Value>>, String> {
    const EVENT_BUDGET: usize = 36 * 1024;
    let mut groups = Vec::new();
    let mut current = Vec::new();
    for event in events {
        for event in bounded_event_chunks(event, EVENT_BUDGET)? {
            current.push(event);
            let measured = serde_json::to_vec(&current)
                .map_err(|error| error.to_string())?
                .len();
            if measured > EVENT_BUDGET {
                let event = current.pop().expect("the just-pushed event exists");
                if !current.is_empty() {
                    groups.push(std::mem::take(&mut current));
                }
                current.push(event);
            }
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    Ok(groups)
}

fn bounded_event_chunks(event: &Value, budget: usize) -> Result<Vec<Value>, String> {
    let serialized = serde_json::to_vec(event).map_err(|error| error.to_string())?;
    if serialized.len() <= budget {
        return Ok(vec![event.clone()]);
    }
    let object = event
        .as_object()
        .ok_or_else(|| "Memory event must be a JSON object".to_owned())?;
    let text = object
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| "Oversized Memory event has no text to split".to_owned())?;
    // A message far past one request, a paste or a dump, keeps its start:
    // a few requests' worth, never one call per slice of it.
    let text = split_utf8(text, EVENT_TEXT_LIMIT_BYTES)
        .into_iter()
        .next()
        .unwrap_or_default();
    let mut base = object.clone();
    base.remove("text");
    let base_size = serde_json::to_vec(&base)
        .map_err(|error| error.to_string())?
        .len();
    let maximum_text_bytes = budget
        .checked_sub(base_size.saturating_add(256))
        .map(|remaining| remaining / 6)
        .filter(|remaining| *remaining > 0)
        .ok_or_else(|| "Memory event metadata exceeds the analysis request limit".to_owned())?;
    let pieces = split_utf8(text, maximum_text_bytes);
    let count = pieces.len();
    let mut chunks = Vec::with_capacity(count);
    for (index, piece) in pieces.into_iter().enumerate() {
        let mut chunk = base.clone();
        chunk.insert("text".to_owned(), Value::String(piece.to_owned()));
        chunk.insert("chunk_index".to_owned(), json!(index));
        chunk.insert("chunk_count".to_owned(), json!(count));
        let chunk = Value::Object(chunk);
        if serde_json::to_vec(&chunk)
            .map_err(|error| error.to_string())?
            .len()
            > budget
        {
            return Err("Memory event chunk exceeds the analysis request limit".to_owned());
        }
        chunks.push(chunk);
    }
    Ok(chunks)
}

fn split_utf8(text: &str, maximum_bytes: usize) -> Vec<&str> {
    if text.is_empty() {
        return vec![text];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + maximum_bytes).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            end = text[start..]
                .char_indices()
                .nth(1)
                .map(|(offset, _)| start + offset)
                .unwrap_or(text.len());
        }
        chunks.push(&text[start..end]);
        start = end;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use hide_ai::{
        AiBackend, AiRequest, AiResponse, Availability, ModelCatalog, NoopLogSink, ProviderId,
        RouterConfig,
    };
    use hide_memory::{CandidateKind, MemoryLifecycle};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Answers each request with the next scripted value and counts calls.
    struct Scripted {
        answers: Mutex<Vec<Value>>,
        calls: AtomicUsize,
    }

    impl AiBackend for Scripted {
        fn id(&self) -> ProviderId {
            ProviderId::CLAUDE
        }
        fn availability(&self) -> Availability {
            Availability::Ready
        }
        fn models(&self) -> ModelCatalog {
            ModelCatalog::Offered(vec!["fixture".to_owned()])
        }
        fn execute(&self, _: &AiRequest, _: &CancelToken) -> Result<AiResponse, AiError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let value = self.answers.lock().unwrap().pop();
            value
                .map(|value| AiResponse {
                    value,
                    usage: Default::default(),
                })
                .ok_or_else(|| AiError::Transient("no scripted answer".to_owned()))
        }
    }

    struct Fixture {
        _temp: tempfile::TempDir,
        root: PathBuf,
        locator: PathBuf,
        database: PathBuf,
        project_id: String,
        backend: Arc<Scripted>,
        node: Arc<dyn NodeLink>,
    }

    const SESSION: &str = "claude-memory-pass";

    impl Fixture {
        /// A Project with Memory on and one card, and a Claude Code session in
        /// it whose first line is the operator's rule and whose second carries
        /// a prompt hook's signed receipt for that card.
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let home = temp.path().join("home");
            let root = temp.path().join("project");
            std::fs::create_dir_all(root.join("src")).unwrap();
            std::fs::write(root.join("src/lib.rs"), "").unwrap();
            let root = std::fs::canonicalize(root).unwrap();
            let node: Arc<dyn NodeLink> = Arc::new(hide_node::Local::new(Some(home.clone())));
            let identity = project_identity(
                node.as_ref(),
                &crate::node::test_node(),
                &root.to_string_lossy(),
            )
            .unwrap();
            let database = temp.path().join("memory.sqlite3");
            let mut store = MemoryStore::open(&database).unwrap();
            store
                .ensure_project(&identity.id, &identity.root, crate::node::TEST_NODE)
                .unwrap();
            store.set_enabled(&identity.id, true, true).unwrap();
            store
                .apply_candidates(
                    &AnalysisBatch {
                        id: "seed".to_owned(),
                        project_id: identity.id.clone(),
                        provider: "codex".to_owned(),
                        analysis_provider: "codex".to_owned(),
                        session_id: "seed".to_owned(),
                        content_hash: "seed".to_owned(),
                        created_at_unix_ms: 1,
                    },
                    &[Candidate {
                        text: "Keep the hook under 100 ms".to_owned(),
                        title: String::new(),
                        aliases: Vec::new(),
                        kind: CandidateKind::Rule,
                        confidence: 0.9,
                        salience: 0.9,
                        source_offsets: vec![1],
                        human_offsets: vec![1],
                        cited_paths: Vec::new(),
                        relation: CandidateRelation::New,
                    }],
                )
                .unwrap();
            let auth = store
                .receipt_auth_tag(&identity.id, "claude", SESSION, "UserPromptSubmit", "m1@1")
                .unwrap();
            let folder = home.join(hide_session::CLAUDE_SESSIONS).join("-project");
            std::fs::create_dir_all(&folder).unwrap();
            let locator = folder.join(format!("{SESSION}.jsonl"));
            let checkout = root.to_string_lossy().into_owned();
            let receipt = format!(
                "<hide-memory-receipt event=\"UserPromptSubmit\" count=\"1\" items=\"m1@1\" auth=\"{auth}\" />"
            );
            let lines = [
                json!({"type": "user", "sessionId": SESSION, "cwd": checkout,
                    "timestamp": "2026-10-11T00:00:00Z", "origin": {"kind": "human"},
                    "message": {"role": "user", "content": "Always run tests with nextest before a merge"}}),
                json!({"type": "attachment", "sessionId": SESSION, "cwd": checkout,
                    "timestamp": "2026-10-11T00:00:01Z",
                    "attachment": {"type": "hook_additional_context", "hookEvent": "UserPromptSubmit",
                        "hookName": "UserPromptSubmit", "content": [receipt]}}),
            ];
            std::fs::write(
                &locator,
                lines
                    .iter()
                    .map(|line| format!("{line}\n"))
                    .collect::<String>(),
            )
            .unwrap();
            Self {
                _temp: temp,
                root,
                locator,
                database,
                project_id: identity.id,
                backend: Arc::new(Scripted {
                    answers: Mutex::new(Vec::new()),
                    calls: AtomicUsize::new(0),
                }),
                node,
            }
        }

        fn answer(&self, cited: &[&str]) {
            self.backend.answers.lock().unwrap().push(json!({
                "candidates": [{
                    "text": "Run tests with nextest before a merge.",
                    "title": "Tests run with nextest",
                    "aliases": ["테스트", "nextest"],
                    "kind": "rule",
                    "confidence": 0.9,
                    "salience": 0.8,
                    "source_offsets": [0],
                    "cited_paths": cited,
                    "relation": "new",
                    "target_id": null,
                }],
                "alias_backfill": [],
            }));
        }

        fn pass(&self, live: bool, closed: bool, weekly_used: Option<f64>) -> Vec<&'static str> {
            self.pass_for(("claude", SESSION.to_owned()), live, closed, weekly_used)
        }

        fn pass_for(
            &self,
            key: MemorySession,
            live: bool,
            closed: bool,
            weekly_used: Option<f64>,
        ) -> Vec<&'static str> {
            let backend: Arc<dyn AiBackend> = self.backend.clone();
            let input = PassInput {
                node: crate::node::test_node(),
                sessions_node: Arc::clone(&self.node),
                database: self.database.clone(),
                router: Some(AiRouter::new(
                    vec![backend],
                    RouterConfig {
                        priority: vec![ProviderId::CLAUDE],
                        max_transient_attempts: 1,
                        ..RouterConfig::default()
                    },
                    Arc::new(NoopLogSink),
                )),
                weekly_used,
                live: live.then(|| key.clone()).into_iter().collect(),
                closed: closed.then_some(key).into_iter().collect(),
                held: HashSet::new(),
                cancel: CancelToken::new(),
            };
            let mut reports = Vec::new();
            run_pass(&input, &mut |report| {
                reports.push(match report {
                    PassReport::Analyzing { .. } => "analyzing",
                    PassReport::Analyzed { learned: 1, .. } => "learned one",
                    PassReport::Analyzed { .. } => "analyzed",
                    PassReport::Failed { .. } => "failed",
                })
            });
            reports
        }

        fn store(&self) -> MemoryStore {
            MemoryStore::open(&self.database).unwrap()
        }

        fn calls(&self) -> usize {
            self.backend.calls.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn a_pass_records_receipts_a_live_pane_cannot_and_analyzes_a_closed_session_once() {
        // B1, B2, B3, B27, D-16.
        let fixture = Fixture::new();

        // A live pane's receipts are its label read's, and a session written
        // a moment ago is not quiet: nothing is read, nothing is analyzed.
        assert!(fixture.pass(true, false, None).is_empty());
        assert_eq!(fixture.calls(), 0);
        let seeded = |store: &MemoryStore| store.item_by_number(&fixture.project_id, 1).unwrap();
        assert_eq!(seeded(&fixture.store()).unwrap().provided_session_count, 0);

        // Its pane closed: the receipt is recorded and the session analyzed.
        fixture.answer(&["src/lib.rs", "src/missing.rs", "../outside.rs"]);
        assert_eq!(
            fixture.pass(false, true, None),
            ["analyzing", "learned one"]
        );
        assert_eq!(fixture.calls(), 1);
        let store = fixture.store();
        assert_eq!(seeded(&store).unwrap().provided_session_count, 1);
        let learned = store
            .item_by_number(&fixture.project_id, 2)
            .unwrap()
            .expect("the analysis wrote a card");
        assert_eq!(learned.lifecycle, MemoryLifecycle::Active);
        assert_eq!(learned.title, "Tests run with nextest");
        assert_eq!(learned.cited_paths, ["src/lib.rs"]);
        assert_eq!(learned.human_source_count, 1);
        assert_eq!(store.analysis_calls(now_ms()).unwrap(), 1);
        drop(store);

        // Nothing new: the same signal analyzes nothing again.
        assert!(fixture.pass(false, true, None).is_empty());
        assert_eq!(fixture.calls(), 1);

        // A new line keeps the session busy for 30 minutes (B1).
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&fixture.locator)
            .unwrap();
        std::io::Write::write_all(
            &mut file,
            format!(
                "{}\n",
                json!({"type": "user", "sessionId": SESSION, "cwd": fixture.root,
                    "timestamp": "2026-10-11T00:01:00Z", "origin": {"kind": "human"},
                    "message": {"role": "user", "content": "And lint before tests"}})
            )
            .as_bytes(),
        )
        .unwrap();
        drop(file);
        assert!(fixture.pass(false, false, None).is_empty());
        assert_eq!(fixture.calls(), 1);
    }

    #[test]
    fn a_session_quiet_for_thirty_minutes_is_analyzed_and_one_quiet_a_moment_less_waits() {
        // B1, D-07.
        let fixture = Fixture::new();
        fixture.answer(&[]);
        let age = |minutes: u64| {
            std::fs::File::options()
                .append(true)
                .open(&fixture.locator)
                .unwrap()
                .set_modified(
                    std::time::SystemTime::now() - std::time::Duration::from_secs(minutes * 60),
                )
                .unwrap();
        };

        age(29);
        assert!(fixture.pass(false, false, None).is_empty());
        assert_eq!(fixture.calls(), 0);

        age(31);
        assert_eq!(
            fixture.pass(false, false, None),
            ["analyzing", "learned one"]
        );
        assert_eq!(fixture.calls(), 1);
    }

    #[test]
    fn an_opencode_session_outside_a_pane_has_its_receipt_recorded_once_by_the_walk() {
        // B13, D-16: OpenCode keeps its sessions in a database, read through
        // the conversation reader the label read uses.
        const OPENCODE: &str = "ses_0a1b2c3d4e5f60718293a4b5c6";
        let fixture = Fixture::new();
        let directory = fixture._temp.path().join("home/.local/share/opencode");
        std::fs::create_dir_all(&directory).unwrap();
        let connection = rusqlite::Connection::open(directory.join("opencode.db")).unwrap();
        connection
            .execute_batch(include_str!(
                "../../../hide-session/tests/fixtures/adapters/opencode-1.18.30/opencode.sql"
            ))
            .unwrap();
        let checkout = fixture.root.to_string_lossy().into_owned();
        connection
            .execute(
                "UPDATE session SET directory = ?1, time_updated = ?2",
                rusqlite::params![checkout, now_ms() as i64],
            )
            .unwrap();
        let auth = fixture
            .store()
            .receipt_auth_tag(
                &fixture.project_id,
                "opencode",
                OPENCODE,
                "UserPromptSubmit",
                "m1@1",
            )
            .unwrap();
        let receipt = format!(
            "<hide-memory-receipt event=\"UserPromptSubmit\" count=\"1\" items=\"m1@1\" auth=\"{auth}\" />"
        );
        connection
            .execute(
                "INSERT INTO part VALUES ('prt_hide', 'msg_01', ?1, 1790989200001, 1790989200001, ?2)",
                rusqlite::params![
                    OPENCODE,
                    json!({"type": "text", "synthetic": true,
                        "text": format!("<system-reminder>\n{receipt}\n</system-reminder>")})
                    .to_string()
                ],
            )
            .unwrap();
        drop(connection);
        let key = ("opencode", OPENCODE.to_owned());
        let delivered = || {
            fixture
                .store()
                .item_by_number(&fixture.project_id, 1)
                .unwrap()
                .unwrap()
                .provided_session_count
        };

        // A live pane's label read owns the session: the walk records only
        // the fixture's Claude Code session.
        fixture.pass_for(key.clone(), true, false, None);
        assert_eq!(delivered(), 1);

        fixture.pass_for(key.clone(), false, false, None);
        assert_eq!(delivered(), 2);
        fixture.pass_for(key, false, false, None);
        assert_eq!(delivered(), 2);
        assert_eq!(fixture.calls(), 0, "an OpenCode session is not analyzed");
    }

    #[test]
    fn the_daily_cap_and_weekly_usage_defer_analysis_without_a_provider_call() {
        // B5, D-10.
        let fixture = Fixture::new();
        fixture.answer(&[]);

        assert!(
            fixture.pass(false, true, Some(80.0)).is_empty(),
            "a held-back Project never shows analyzing"
        );
        assert_eq!(
            fixture.calls(),
            0,
            "weekly usage at 80% holds analysis back"
        );

        let store = fixture.store();
        for _ in 0..hide_memory::DAILY_ANALYSIS_CALL_LIMIT {
            store.count_analysis_call(now_ms()).unwrap();
        }
        drop(store);
        assert!(fixture.pass(false, true, Some(10.0)).is_empty());
        assert_eq!(fixture.calls(), 0, "the day's calls are spent");
        assert!(
            fixture
                .store()
                .item_by_number(&fixture.project_id, 2)
                .unwrap()
                .is_none(),
            "the conversation waits, unread"
        );
    }

    #[test]
    fn only_a_sign_in_or_an_agent_choice_reaches_the_screen_and_the_rest_backs_off() {
        // B4, B23.
        let now = 1_000_000;
        let (shown, retry) = analysis_after_failure(&AiError::NotAuthenticated, 1, now);
        assert_eq!(
            (shown.state.as_str(), shown.action.as_deref()),
            ("needs_action", Some("sign_in"))
        );
        assert_eq!(retry, now + SIGN_IN_RETRY_MS);

        let (shown, retry) = analysis_after_failure(
            &AiError::UsageLimited {
                retry_after: Some(std::time::Duration::from_secs(90)),
            },
            1,
            now,
        );
        assert_eq!(shown, MemoryAnalysisSnapshot::default());
        assert_eq!(retry, now + 90_000, "the provider's resume time is kept");

        let backoff = |failures| analysis_after_failure(&AiError::Timeout, failures, now).1 - now;
        assert_eq!(backoff(1), RETRY_FIRST_MS);
        assert_eq!(backoff(2), 2 * RETRY_FIRST_MS);
        assert_eq!(backoff(40), RETRY_LAST_MS);
        assert_eq!(
            analysis_after_failure(&AiError::Timeout, 3, now).0,
            MemoryAnalysisSnapshot::default()
        );
    }

    #[test]
    fn a_correction_without_the_operators_words_waits_for_review() {
        // D-14: only a `human` event can make a candidate replace a card.
        let candidate = |offset| Candidate {
            text: "Use yarn".to_owned(),
            title: String::new(),
            aliases: Vec::new(),
            kind: CandidateKind::Rule,
            confidence: 0.9,
            salience: 0.9,
            source_offsets: vec![offset],
            human_offsets: Vec::new(),
            cited_paths: Vec::new(),
            relation: CandidateRelation::Supersedes {
                target_id: "card".to_owned(),
            },
        };
        let events = [
            json!({"offset": 1, "kind": "human", "text": "Use yarn now"}),
            json!({"offset": 2, "kind": "assistant", "text": "Switching to yarn"}),
        ];

        let mut checked = vec![candidate(1), candidate(2), candidate(9)];
        assert_eq!(
            validate_candidates(&mut checked, &events),
            1,
            "offset 9 was never sent"
        );

        assert_eq!(checked[0].human_offsets, [1]);
        assert!(matches!(
            checked[0].relation,
            CandidateRelation::Supersedes { .. }
        ));
        assert!(checked[1].human_offsets.is_empty());
        assert!(matches!(
            checked[1].relation,
            CandidateRelation::Conflicts { .. }
        ));
        assert_eq!(checked.len(), 2);
    }
}
