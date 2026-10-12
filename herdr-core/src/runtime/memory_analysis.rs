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
    session_conversation, session_stat, trusted_memory_receipt,
};
use super::*;
use crate::labels::worker::{MemoryHandoff, MemorySession, SightedMemoryReceipt};
use crate::model::MemoryAnalysisState;
use crate::node::NodeId;
use crate::node_access::NodeLink;
use hide_ai::{AiError, AiRouter, CancelToken};
use hide_memory::{
    AnalysisBatch, ApplySummary, Candidate, CursorReader, HideNativeAnalyzer, MemoryStore,
    ReceiptRecord, SessionCursorRecord, SessionSourceRecord,
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
/// The most closed sessions waiting for analysis; a burst past it waits for
/// the quiet rule instead, and the log says so.
const CLOSED_SESSIONS_LIMIT: usize = 256;
/// How long background analysis waits for a first read of the provider's
/// weekly usage before it goes on under the daily cap alone (D-10).
const USAGE_READ_WAIT_MS: u64 = 10 * 60_000;
/// How long after a pass last wanted the weekly figure the usage reader keeps
/// reading with no window attached.
const USAGE_WANTED_MS: u64 = 30 * 60_000;
/// Bytes of existing cards one analysis compares new text against: with an
/// event group's 36 KiB it keeps a request under the analysis input cap.
const COMPARISON_INPUT_LIMIT_BYTES: usize = 16 * 1024;
/// The text of the new events the comparison set is chosen by.
const COMPARISON_TEXT_LIMIT_BYTES: usize = 16 * 1024;
/// The most session read failures remembered so each is logged once per
/// version of its file; past it a new failure is logged on every pass.
const FAILED_READS_LIMIT: usize = 4096;

/// The session read failures the log already has: each by its kind and
/// session, with the version of the file it failed on (its size and
/// modification time; an OpenCode session's row has only its update time).
type FailedReads = HashMap<(&'static str, MemorySession), (u64, Option<u64>)>;

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
    /// Sessions whose pane let them go; each is analyzed without waiting for
    /// the quiet rule (B2), and stays here until a pass finds nothing left
    /// to analyze in it, so a pass that was cancelled, held back or failed
    /// does not drop it into the 30 minute wait.
    pub(super) closed_sessions: BTreeSet<MemorySession>,
    /// Until when the usage reader reads with no window attached, because a
    /// pass wants the analysis provider's weekly usage (D-10 b).
    usage_wanted_until_unix_ms: u64,
    /// When a pass first found the weekly figure unread; past
    /// [`USAGE_READ_WAIT_MS`] analysis goes on without it.
    usage_unread_since_unix_ms: Option<u64>,
    receipts_pending: Vec<SightedMemoryReceipt>,
    receipts_in_flight: bool,
    /// What the last pass left of [`FailedReads`]; a pass that panicked
    /// leaves none, and its failures are logged again once.
    failed_reads: FailedReads,
    /// Each Project's analysis state and last change, by Memory's Project id.
    pub(super) projects: HashMap<String, ProjectMemoryWork>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ProjectMemoryWork {
    pub(super) analysis: MemoryAnalysisState,
    /// The card the last forget took away, which the Memory tab offers to
    /// bring back (B24).
    pub(super) forgotten: Option<u64>,
    /// No analysis before this time (a failure's backoff or a usage limit).
    retry_at_unix_ms: u64,
    failures: u32,
}

/// What a pass tells the runtime about one Project while it runs.
enum PassReport {
    Analyzing {
        project_id: String,
    },
    /// The Project's analysis in this pass ended: how many cards it wrote,
    /// and the provider error that stopped it, if one did.
    Analyzed {
        project_id: String,
        learned: usize,
        error: Option<AiError>,
    },
}

/// What a whole pass tells the runtime when it ends.
#[derive(Debug, Default, PartialEq)]
struct PassOutcome {
    /// Closed sessions with nothing left to analyze, or no longer in any
    /// Memory-enabled Project.
    settled: BTreeSet<MemorySession>,
    /// A session was due for background analysis, which needs the
    /// provider's weekly usage read fresh.
    wants_usage: bool,
    failed_reads: FailedReads,
}

impl PassOutcome {
    /// Logs a session that could not be read once per version of its file:
    /// the same file failing the same way on the next pass, a minute later,
    /// says nothing new.
    fn read_failed(
        &mut self,
        kind: &'static str,
        project_id: &str,
        session: &ProjectSession,
        version: (u64, Option<u64>),
        message: &str,
    ) {
        let key = (kind, (session.agent.as_str(), session.id.clone()));
        if self.failed_reads.get(&key) == Some(&version) {
            return;
        }
        crate::diagnostic!(json!({
            "component": "memory",
            "kind": kind,
            "project_id": project_id,
            "provider": session.agent.as_str(),
            "session_id": session.id,
            "message": message,
        }));
        if self.failed_reads.len() < FAILED_READS_LIMIT || self.failed_reads.contains_key(&key) {
            self.failed_reads.insert(key, version);
        }
    }

    /// The session was read: a later failure is news again.
    fn read_succeeded(&mut self, kind: &'static str, session: &ProjectSession) {
        if !self.failed_reads.is_empty() {
            self.failed_reads
                .remove(&(kind, (session.agent.as_str(), session.id.clone())));
        }
    }
}

/// The analysis provider's weekly usage as a pass sees it (D-10 b).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum WeeklyUsage {
    /// The share already spent.
    Used(f64),
    /// The provider reports weekly usage and it has not been read yet:
    /// background analysis waits for it.
    Unread,
    /// The provider reports no weekly usage, or reading it failed: only the
    /// daily cap applies.
    Unknown,
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
    /// The Project's Memory was turned off while the card was being made.
    Disabled,
}

impl Runtime {
    /// Takes what the label reads found for Memory: receipts to record off
    /// the lock, and the sessions live panes hold now. A session that left
    /// the set had its pane closed or replaced, and the next pass, started
    /// at once, analyzes it (B2).
    pub(crate) fn take_memory_handoff(&mut self, handoff: MemoryHandoff) {
        if let Some(live) = handoff.live_sessions {
            let work = &mut self.memory_work;
            let mut closed = false;
            for gone in work.live_sessions.difference(&live) {
                if work.closed_sessions.len() < CLOSED_SESSIONS_LIMIT {
                    closed |= work.closed_sessions.insert(gone.clone());
                } else {
                    crate::diagnostic!(json!({
                        "component": "memory",
                        "kind": "analysis.closed_sessions_capped",
                        "provider": gone.0,
                        "session_id": gone.1,
                    }));
                }
            }
            let opened = live
                .iter()
                .any(|session| !work.live_sessions.contains(session));
            if closed || opened {
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
                let mut recorded_any = false;
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
                                // The open Memory tab counts deliveries.
                                if recorded_any {
                                    guard.refresh_project_memory();
                                }
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
                    recorded_any = true;
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
        // Memory analysis is a model call: with Hide AI off or no agent chosen
        // the walk still records receipts and the analysis waits (B33).
        let settings = self.ai_active().then(|| self.ai_settings.clone()).flatten();
        let weekly = settings.as_ref().map_or(WeeklyUsage::Unknown, |settings| {
            self.weekly_usage(settings.provider.as_str(), now_unix_ms)
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
        // A pane closed or a wake while the pass runs sets this back to now.
        work.next_pass_unix_ms = now_unix_ms.saturating_add(MEMORY_PASS_INTERVAL_MS);
        let input = PassInput {
            node,
            sessions_node: own_node,
            database,
            router,
            weekly,
            live: work.live_sessions.clone(),
            closed: work.closed_sessions.clone(),
            held,
            cancel,
            failed_reads: work.failed_reads.clone(),
        };
        let report_context = context.clone();
        let spawn = thread::Builder::new()
            .name("hide-project-memory-pass".to_owned())
            .spawn(move || {
                let mut report = |report| apply_pass_report(&report_context, report);
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_pass(&input, &mut report)
                }))
                .unwrap_or_else(|_| {
                    crate::diagnostic!(json!({
                        "component": "memory",
                        "kind": "pass.panicked",
                    }));
                    PassOutcome::default()
                });
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.finish_memory_pass(outcome),
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
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "pass.worker_failed",
                "message": error.to_string(),
            }));
        }
        false
    }

    fn finish_memory_pass(&mut self, outcome: PassOutcome) -> bool {
        let work = &mut self.memory_work;
        work.pass_in_flight = false;
        work.pass_cancel = None;
        work.pass_settings = None;
        work.closed_sessions
            .retain(|session| !outcome.settled.contains(session));
        work.failed_reads = outcome.failed_reads;
        if outcome.wants_usage {
            let now = now_ms();
            work.usage_wanted_until_unix_ms = now.saturating_add(USAGE_WANTED_MS);
            work.usage_unread_since_unix_ms.get_or_insert(now);
        }
        let mut changed = false;
        for project in work.projects.values_mut() {
            if project.analysis == MemoryAnalysisState::Analyzing {
                project.analysis = MemoryAnalysisState::Idle;
                changed = true;
            }
        }
        if changed {
            self.sync_project_memory();
        }
        changed
    }

    /// The analysis provider's weekly usage as the toolbar's usage read last
    /// answered it. A provider the reader has not answered for yet is unread
    /// for at most [`USAGE_READ_WAIT_MS`] after a pass first wanted it.
    fn weekly_usage(&mut self, provider: &str, now_unix_ms: u64) -> WeeklyUsage {
        let row = self
            .snapshot
            .navigator
            .provider_usage
            .iter()
            .find(|row| row.provider == provider)
            .filter(|row| row.window_minutes == crate::usage::WEEKLY_WINDOW_MINUTES);
        let work = &mut self.memory_work;
        let usage = match row {
            Some(row) if row.used_percent.is_some() => {
                WeeklyUsage::Used(row.used_percent.unwrap_or_default())
            }
            Some(row) if row.last_checked_at_unix_ms.is_none() => {
                match work.usage_unread_since_unix_ms {
                    Some(since) if now_unix_ms.saturating_sub(since) >= USAGE_READ_WAIT_MS => {
                        crate::diagnostic!(json!({
                            "component": "memory",
                            "kind": "analysis.weekly_usage_unread",
                            "provider": provider,
                        }));
                        WeeklyUsage::Unknown
                    }
                    _ => return WeeklyUsage::Unread,
                }
            }
            _ => WeeklyUsage::Unknown,
        };
        work.usage_unread_since_unix_ms = None;
        usage
    }

    /// Whether the usage reader reads with no window attached: a pass wants
    /// the analysis provider's weekly usage (D-10 b).
    pub(crate) fn memory_reads_usage(&self) -> bool {
        now_ms() < self.memory_work.usage_wanted_until_unix_ms
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
            if project.retry_at_unix_ms != 0 || project.analysis == MemoryAnalysisState::AiNotReady
            {
                project.retry_at_unix_ms = 0;
                project.failures = 0;
                if project.analysis == MemoryAnalysisState::AiNotReady {
                    project.analysis = MemoryAnalysisState::Idle;
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
        if project.analysis == MemoryAnalysisState::AiNotReady {
            project.analysis = MemoryAnalysisState::Idle;
        }
        self.memory_work.next_pass_unix_ms = 0;
    }

    /// Forgets a Project's analysis state when its Memory is turned off or
    /// deleted, and stops a pass that may be analyzing it.
    pub(super) fn quiet_project_memory(&mut self, project_id: &str) {
        self.cancel_memory_pass();
        if let Some(project) = self.memory_work.projects.get_mut(project_id) {
            project.analysis = MemoryAnalysisState::Idle;
            project.retry_at_unix_ms = 0;
            project.failures = 0;
        }
    }

    pub(super) fn note_memory_change(&mut self, project_id: &str, change: MemoryChange) {
        let project = self
            .memory_work
            .projects
            .entry(project_id.to_owned())
            .or_default();
        match change {
            MemoryChange::Forgot(number) => project.forgotten = Some(number),
            MemoryChange::Undid(number) if project.forgotten == Some(number) => {
                project.forgotten = None;
            }
            MemoryChange::Undid(_) | MemoryChange::Other => {}
        }
        if self.named_memory_project() == Some(project_id) {
            self.sync_project_memory();
            self.refresh_project_memory();
        }
    }

    /// Applies one report; answers whether the screen changed.
    fn apply_memory_report(&mut self, report: PassReport) -> bool {
        let now = now_ms();
        let project_id = match &report {
            PassReport::Analyzing { project_id } | PassReport::Analyzed { project_id, .. } => {
                project_id.clone()
            }
        };
        let before = self.memory_work.projects.get(&project_id).cloned();
        match report {
            PassReport::Analyzing { project_id } => {
                let project = self.memory_work.projects.entry(project_id).or_default();
                project.analysis = MemoryAnalysisState::Analyzing;
            }
            PassReport::Analyzed {
                project_id,
                learned,
                error,
            } => {
                let named = self.named_memory_project() == Some(project_id.as_str());
                if let Some(error) = &error {
                    crate::diagnostic!(json!({
                        "component": "memory",
                        "kind": "analysis.failed",
                        "project_id": project_id,
                        "error": format!("{error:?}"),
                    }));
                }
                let project = self.memory_work.projects.entry(project_id).or_default();
                match &error {
                    Some(error) => {
                        project.failures = project.failures.saturating_add(1);
                        let (analysis, retry_at) =
                            analysis_after_failure(error, project.failures, now);
                        project.analysis = analysis;
                        project.retry_at_unix_ms = retry_at;
                    }
                    None => {
                        project.analysis = MemoryAnalysisState::Idle;
                        project.failures = 0;
                        project.retry_at_unix_ms = 0;
                    }
                }
                if named && learned > 0 {
                    self.refresh_project_memory();
                }
            }
        }
        if self.named_memory_project() != Some(project_id.as_str())
            || self.memory_work.projects.get(&project_id) == before.as_ref()
        {
            return false;
        }
        self.sync_project_memory();
        true
    }
}

/// What a failure shows and when the Project is tried again (B4, B23,
/// D-27): only Hide AI that cannot answer (signed out, no agent chosen)
/// reaches the screen, as "Hide AI not ready" with its settings; a usage
/// limit waits for the time the provider named, and anything else backs off
/// in the log alone.
fn analysis_after_failure(
    error: &AiError,
    failures: u32,
    now_unix_ms: u64,
) -> (MemoryAnalysisState, u64) {
    match error {
        AiError::NotAuthenticated | AiError::NoProvider(_) | AiError::Unsupported(_) => (
            MemoryAnalysisState::AiNotReady,
            now_unix_ms.saturating_add(SIGN_IN_RETRY_MS),
        ),
        AiError::UsageLimited { retry_after } => (
            MemoryAnalysisState::Idle,
            now_unix_ms
                .saturating_add(retry_after.map_or(USAGE_RETRY_MS, |wait| wait.as_millis() as u64)),
        ),
        _ => {
            let backoff = RETRY_FIRST_MS
                .saturating_mul(1_u64 << failures.saturating_sub(1).min(16))
                .min(RETRY_LAST_MS);
            (
                MemoryAnalysisState::Idle,
                now_unix_ms.saturating_add(backoff),
            )
        }
    }
}

fn apply_pass_report(context: &RuntimeWorkerContext, report: PassReport) {
    let Some(runtime) = context.runtime.upgrade() else {
        return;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) => guard.apply_memory_report(report),
        Err(_) => false,
    };
    drop(runtime);
    if changed {
        context.notifier.notify();
    }
}

/// What a `hide memory` command did, for the card the Memory tab offers to
/// bring back.
pub(crate) enum MemoryChange {
    Forgot(u64),
    Undid(u64),
    Other,
}

/// Tells the core a `hide memory` command changed a Project's Memory, so the
/// screen reads it again and a forgotten card can be brought back from it.
pub(crate) fn report_memory_change(
    context: &RuntimeWorkerContext,
    project_id: &str,
    change: MemoryChange,
) {
    let Some(runtime) = context.runtime.upgrade() else {
        return;
    };
    let applied = match runtime.lock() {
        Ok(mut guard) => {
            guard.note_memory_change(project_id, change);
            true
        }
        Err(_) => false,
    };
    drop(runtime);
    if applied {
        context.notifier.notify();
    }
}

/// What one pass needs, taken under the lock when it starts.
struct PassInput {
    node: NodeId,
    sessions_node: Arc<dyn NodeLink>,
    database: PathBuf,
    /// `None` while Hide AI is off: the pass only records receipts.
    router: Option<AiRouter>,
    weekly: WeeklyUsage,
    live: BTreeSet<MemorySession>,
    closed: BTreeSet<MemorySession>,
    /// Projects whose analysis waits for a retry time.
    held: HashSet<String>,
    cancel: CancelToken,
    failed_reads: FailedReads,
}

fn run_pass(input: &PassInput, report: &mut dyn FnMut(PassReport)) -> PassOutcome {
    let mut outcome = PassOutcome {
        failed_reads: input.failed_reads.clone(),
        ..PassOutcome::default()
    };
    // No store yet: Memory was never turned on on this machine.
    if !MemoryStore::exists(&input.database) {
        outcome.settled = input.closed.clone();
        return outcome;
    }
    let mut store = match MemoryStore::open(&input.database) {
        Ok(store) => store,
        Err(error) => {
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "pass.store_unavailable",
                "message": error.to_string(),
            }));
            return outcome;
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
            return outcome;
        }
    };
    // A deferral holds for the rest of the pass and is logged once.
    let mut deferred = None;
    // Closed sessions some enabled Project lists; the rest belong to no
    // Project Memory analyzes, once every Project was walked.
    let mut reached = BTreeSet::new();
    let mut walked_all = true;
    for (project_id, root) in projects {
        if input.cancel.is_cancelled() {
            walked_all = false;
            break;
        }
        let mut tally = ProjectTally::default();
        let walked = walk_project(
            input,
            &mut store,
            &project_id,
            &root,
            &mut deferred,
            &mut tally,
            &mut outcome,
            &mut reached,
            report,
        );
        if let Err(message) = walked {
            walked_all = false;
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "pass.project_failed",
                "project_id": project_id,
                "message": message,
            }));
        }
        // Reported whatever ended the walk, so a Project that turned
        // "analyzing" always ends it and keeps what it learned.
        if tally.analyzing {
            report(PassReport::Analyzed {
                project_id,
                learned: tally.learned,
                error: tally.error,
            });
        }
    }
    if walked_all {
        outcome
            .settled
            .extend(input.closed.difference(&reached).cloned());
    }
    if let Some(reason) = deferred {
        crate::diagnostic!(json!({
            "component": "memory",
            "kind": "analysis.deferred",
            "reason": reason,
        }));
    }
    outcome
}

/// The log kinds of a session whose receipts, or whose analysis, failed.
const RECEIPTS_FAILED: &str = "receipts.walk_failed";
const ANALYSIS_FAILED: &str = "analysis.session_failed";

/// What one Project's walk analyzed.
#[derive(Default)]
struct ProjectTally {
    analyzing: bool,
    learned: usize,
    error: Option<AiError>,
}

#[allow(clippy::too_many_arguments)]
fn walk_project(
    input: &PassInput,
    store: &mut MemoryStore,
    project_id: &str,
    root: &str,
    deferred: &mut Option<&'static str>,
    tally: &mut ProjectTally,
    outcome: &mut PassOutcome,
    reached: &mut BTreeSet<MemorySession>,
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
        {
            match read_database_receipts(store, sessions_node, project_id, &session) {
                Ok(()) => outcome.read_succeeded(RECEIPTS_FAILED, &session),
                Err(message) => outcome.read_failed(
                    RECEIPTS_FAILED,
                    project_id,
                    &session,
                    (0, Some(session.updated_at_unix_ms)),
                    &message,
                ),
            }
        }
    }
    for session in sessions {
        if input.cancel.is_cancelled() {
            break;
        }
        let key = (session.agent.as_str(), session.id.clone());
        let closed = input.closed.contains(&key);
        if closed {
            reached.insert(key.clone());
        }
        // A closed session open in a pane again is the label read's, and the
        // quiet rule analyzes it when that pane closes or goes quiet.
        if closed && input.live.contains(&key) {
            outcome.settled.insert(key.clone());
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
            if closed {
                outcome.settled.insert(key);
            }
            continue;
        }
        let Ok(stat) = session_stat(sessions_node, &session) else {
            continue;
        };
        let version = (stat.size, stat.modified_unix_ms);
        if !input.live.contains(&key) {
            match read_receipts(store, sessions_node, project_id, &session, &stat) {
                Ok(()) => outcome.read_succeeded(RECEIPTS_FAILED, &session),
                Err(message) => {
                    outcome.read_failed(RECEIPTS_FAILED, project_id, &session, version, &message);
                }
            }
        }
        let pending = analysis_pending(store, project_id, &session, &stat)?;
        if closed && !pending {
            outcome.settled.insert(key.clone());
        }
        let Some(router) = input.router.as_ref() else {
            continue;
        };
        if deferred.is_some() || input.held.contains(project_id) || tally.error.is_some() {
            continue;
        }
        let quiet = stat
            .modified_unix_ms
            .is_some_and(|modified| now_ms().saturating_sub(modified) >= MEMORY_QUIESCENCE_MS);
        if !pending || !((closed && !input.live.contains(&key)) || quiet) {
            continue;
        }
        outcome.wants_usage = true;
        // A held-back call is logged, never shown: the Project does not turn
        // "analyzing" for a pass that will ask nothing (B5).
        if let Some(reason) = deferral(store, input.weekly, true)? {
            *deferred = Some(reason);
            continue;
        }
        if !tally.analyzing {
            tally.analyzing = true;
            report(PassReport::Analyzing {
                project_id: project_id.to_owned(),
            });
        }
        let mut admit = |store: &MemoryStore| admit_call(store, input.weekly, true);
        match analyze_session(
            store,
            sessions_node,
            router,
            project_id,
            &session,
            &stat,
            &input.cancel,
            &mut admit,
        ) {
            Ok(learned) => {
                tally.learned += learned;
                outcome.read_succeeded(ANALYSIS_FAILED, &session);
            }
            // One provider failure stops this Project's analysis for the pass;
            // its receipts are still walked.
            Err(AnalysisFailure::Provider(error)) => tally.error = Some(error),
            Err(AnalysisFailure::Deferred(reason)) => *deferred = Some(reason),
            Err(AnalysisFailure::Cancelled | AnalysisFailure::Disabled) => break,
            Err(AnalysisFailure::Local(message)) => {
                outcome.read_failed(ANALYSIS_FAILED, project_id, &session, version, &message);
            }
        }
    }
    Ok(())
}

/// Admits one analysis call against the machine's daily cap and, for
/// background analysis, the provider's weekly usage (D-10), and counts it.
pub(crate) fn admit_call(
    store: &MemoryStore,
    weekly: WeeklyUsage,
    background: bool,
) -> Result<(), AnalysisFailure> {
    if let Some(reason) = deferral(store, weekly, background).map_err(AnalysisFailure::Local)? {
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
    weekly: WeeklyUsage,
    background: bool,
) -> Result<Option<&'static str>, String> {
    if background {
        match weekly {
            WeeklyUsage::Used(used) if used >= WEEKLY_USAGE_DEFER_PERCENT => {
                return Ok(Some("weekly_usage"));
            }
            WeeklyUsage::Unread => return Ok(Some("weekly_usage_unread")),
            WeeklyUsage::Used(_) | WeeklyUsage::Unknown => {}
        }
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
    // Joined as the label read joins a message's parts, so a receipt in a
    // later part is named by the same turn whichever reader finds it.
    let read = session_conversation(sessions_node, session, saved.as_ref())?;
    let checkpoint = serde_json::to_vec(&read.checkpoint).map_err(|error| error.to_string())?;
    for (event, offset) in read.events.iter().zip(&read.event_offsets) {
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
                byte_offset: read_through(&read, stat),
                checkpoint,
                last_content_hash: None,
                updated_at_unix_ms: now_ms(),
            },
        )
        .map_err(|error| error.to_string())?;
    log_skipped("receipts.records_skipped", project_id, session, &read);
    Ok(())
}

/// Logs the records a read passed over, by reason, most often a tool result
/// past the line cap. Called once the cursor moved past them, so each is
/// logged once.
fn log_skipped(
    kind: &'static str,
    project_id: &str,
    session: &ProjectSession,
    read: &hide_session::ConversationRead,
) {
    if read.skipped_reasons.is_empty() {
        return;
    }
    crate::diagnostic!(json!({
        "component": "memory",
        "kind": kind,
        "project_id": project_id,
        "provider": session.agent.as_str(),
        "session_id": session.id,
        "skipped": read.skipped_reasons,
    }));
}

/// How much of the file `stat` saw a read accounted for: a read that reached
/// the end it found accounts for all of it, a torn last line too, so an
/// unchanged file is not read again on every pass.
fn read_through(
    read: &hide_session::ConversationRead,
    stat: &hide_node_link::sessions::SessionStat,
) -> u64 {
    let offset = read.checkpoint.offset();
    if read.has_more {
        offset
    } else {
        offset.max(stat.size)
    }
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

/// Analyzes the session's conversation past the analysis cursor, one
/// provider call per bounded group, and moves the cursor only when every
/// group was written. Answers how many cards it created or replaced.
#[allow(clippy::too_many_arguments)] // one session: its store, nodes, file and limits
fn analyze_session(
    store: &mut MemoryStore,
    sessions_node: &dyn NodeLink,
    router: &AiRouter,
    project_id: &str,
    session: &ProjectSession,
    stat: &hide_node_link::sessions::SessionStat,
    cancel: &CancelToken,
    admit: &mut dyn FnMut(&MemoryStore) -> Result<(), AnalysisFailure>,
) -> Result<usize, AnalysisFailure> {
    let local = |error: String| AnalysisFailure::Local(error);
    let provider = session.agent.as_str();
    let saved = store
        .load_cursor(CursorReader::Analysis, project_id, provider, &session.id)
        .map_err(|error| local(error.to_string()))?;
    let read = session_conversation(sessions_node, session, saved.as_ref()).map_err(local)?;
    let checkpoint =
        serde_json::to_vec(&read.checkpoint).map_err(|error| local(error.to_string()))?;
    let byte_offset = read_through(&read, stat);
    let events = read
        .events
        .iter()
        .zip(&read.event_offsets)
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
                deadline: None,
            },
            &group,
            cancel,
            admit,
        )?;
        last_hash = Some(written.content_hash);
        if !written.summary.duplicate_batch {
            learned += written.summary.created + written.summary.superseded;
        }
    }
    store
        .save_cursor(
            CursorReader::Analysis,
            &SessionCursorRecord {
                project_id: project_id.to_owned(),
                provider: provider.to_owned(),
                session_id: session.id.clone(),
                byte_offset,
                checkpoint,
                last_content_hash: last_hash,
                updated_at_unix_ms: now_ms(),
            },
        )
        .map_err(|error| local(error.to_string()))?;
    log_skipped("analysis.records_skipped", project_id, session, &read);
    Ok(learned)
}

/// Where a batch of new text came from.
pub(crate) struct BatchSource<'a> {
    pub(crate) project_id: &'a str,
    /// The session's agent, or `hide` for `hide memory add`.
    pub(crate) provider: &'a str,
    pub(crate) session_id: &'a str,
    /// The checkout cited paths are checked in.
    pub(crate) checkout_path: &'a str,
    /// When the caller stops waiting: past it nothing is written (B8).
    pub(crate) deadline: Option<std::time::Instant>,
}

pub(crate) struct WrittenCards {
    pub(crate) summary: ApplySummary,
    pub(crate) batch_id: String,
    content_hash: String,
}

/// The content hash and batch id one group from one source is written
/// under, so the same text from the same source converges on one batch.
pub(crate) fn batch_identity(source: &BatchSource<'_>, serialized_group: &str) -> (String, String) {
    let content_hash = digest(&[
        source.project_id,
        source.provider,
        source.session_id,
        serialized_group,
    ]);
    let batch_id = digest(&[
        "memory-batch",
        source.project_id,
        source.provider,
        source.session_id,
        &content_hash,
    ]);
    (content_hash, batch_id)
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
    let (content_hash, batch_id) = batch_identity(source, &serialized);
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
    if source
        .deadline
        .is_some_and(|deadline| std::time::Instant::now() >= deadline)
    {
        return Err(AnalysisFailure::Cancelled);
    }
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
        .map_err(|error| match error {
            hide_memory::MemoryError::Disabled => AnalysisFailure::Disabled,
            error => local(error.to_string()),
        })?;
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
/// outside the group is left out, and the operator's own words are the
/// sources whose event is `human`, which the store needs to let a correction
/// replace a card (D-14). Answers how many were left out.
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
    let times = events
        .iter()
        .filter_map(|event| {
            Some((
                event.get("offset")?.as_u64()?,
                event.get("at_unix_ms")?.as_u64()?,
            ))
        })
        .collect::<HashMap<_, _>>();
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
        candidate.source_times = candidate
            .source_offsets
            .iter()
            .filter_map(|offset| Some((*offset, *times.get(offset)?)))
            .collect();
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
    let kept = paths_inside(sessions_node, checkout, &cited);
    for candidate in candidates {
        candidate.cited_paths.retain(|path| kept.contains(path));
    }
}

/// Which of the repository-relative `relative` paths name a file or folder
/// inside `checkout`, as the node that holds it resolves them: one outside
/// it, one that does not exist, or one the node cannot read is left out.
pub(crate) fn paths_inside(
    sessions_node: &dyn NodeLink,
    checkout: &str,
    relative: &[String],
) -> HashSet<String> {
    let mut paths = vec![checkout.to_owned()];
    paths.extend(
        relative
            .iter()
            .map(|path| format!("{}/{path}", checkout.trim_end_matches('/'))),
    );
    use hide_node_link::cleanup::PathState;
    let states = crate::node_access::call_as::<Vec<PathState>>(
        sessions_node,
        hide_node_link::protocol::Call::RealPaths { paths },
        SESSION_CALL_TIMEOUT,
    );
    match states {
        Ok(states) if states.len() == relative.len() + 1 => {
            let root = match &states[0] {
                PathState::Real { path } => format!("{}/", path.trim_end_matches('/')),
                _ => String::new(),
            };
            relative
                .iter()
                .zip(&states[1..])
                .filter(|(_, state)| {
                    !root.is_empty()
                        && matches!(state, PathState::Real { path } if path.starts_with(&root))
                })
                .map(|(path, _)| path.clone())
                .collect()
        }
        _ => HashSet::new(),
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
    use hide_memory::{CandidateKind, CandidateRelation, MemoryLifecycle};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Answers each request with the next scripted value, counts calls and
    /// keeps what each was asked.
    struct Scripted {
        answers: Mutex<Vec<Value>>,
        calls: AtomicUsize,
        inputs: Mutex<Vec<String>>,
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
        fn execute(&self, request: &AiRequest, _: &CancelToken) -> Result<AiResponse, AiError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inputs.lock().unwrap().push(request.input.clone());
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
        /// The session's records: the operator's rule, then the receipt.
        lines: Vec<Value>,
        database: PathBuf,
        project_id: String,
        backend: Arc<Scripted>,
        node: Arc<dyn NodeLink>,
        /// What the last pass remembered, as the runtime keeps it.
        failed_reads: std::cell::RefCell<FailedReads>,
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
                        source_times: Default::default(),
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
            let fixture = Self {
                _temp: temp,
                root,
                locator,
                lines: lines.to_vec(),
                database,
                project_id: identity.id,
                backend: Arc::new(Scripted {
                    answers: Mutex::new(Vec::new()),
                    calls: AtomicUsize::new(0),
                    inputs: Mutex::new(Vec::new()),
                }),
                node,
                failed_reads: Default::default(),
            };
            fixture.write(&fixture.lines);
            fixture
        }

        /// Replaces the session file with `lines`, one record each.
        fn write(&self, lines: &[Value]) {
            std::fs::write(
                &self.locator,
                lines
                    .iter()
                    .map(|line| format!("{line}\n"))
                    .collect::<String>(),
            )
            .unwrap();
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

        fn pass(&self, live: bool, closed: bool, weekly: WeeklyUsage) -> Vec<&'static str> {
            self.pass_for(("claude", SESSION.to_owned()), live, closed, weekly)
                .0
        }

        fn pass_for(
            &self,
            key: MemorySession,
            live: bool,
            closed: bool,
            weekly: WeeklyUsage,
        ) -> (Vec<&'static str>, PassOutcome) {
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
                weekly,
                live: live.then(|| key.clone()).into_iter().collect(),
                closed: closed.then_some(key).into_iter().collect(),
                held: HashSet::new(),
                cancel: CancelToken::new(),
                failed_reads: self.failed_reads.borrow().clone(),
            };
            let mut reports = Vec::new();
            let outcome = run_pass(&input, &mut |report| {
                reports.push(match report {
                    PassReport::Analyzing { .. } => "analyzing",
                    PassReport::Analyzed { error: Some(_), .. } => "failed",
                    PassReport::Analyzed { learned: 1, .. } => "learned one",
                    PassReport::Analyzed { .. } => "analyzed",
                })
            });
            *self.failed_reads.borrow_mut() = outcome.failed_reads.clone();
            (reports, outcome)
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
        assert!(fixture.pass(true, false, WeeklyUsage::Unknown).is_empty());
        assert_eq!(fixture.calls(), 0);
        let seeded = |store: &MemoryStore| store.item_by_number(&fixture.project_id, 1).unwrap();
        assert_eq!(seeded(&fixture.store()).unwrap().provided_session_count, 0);

        // Its pane closed: the receipt is recorded and the session analyzed.
        fixture.answer(&["src/lib.rs", "src/missing.rs", "../outside.rs"]);
        assert_eq!(
            fixture.pass(false, true, WeeklyUsage::Unknown),
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
        assert!(fixture.pass(false, true, WeeklyUsage::Unknown).is_empty());
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
        assert!(fixture.pass(false, false, WeeklyUsage::Unknown).is_empty());
        assert_eq!(fixture.calls(), 1);
    }

    #[test]
    fn a_conversation_names_the_cards_a_prompt_received_and_the_ones_learned_from_it() {
        // P1-2: the Sessions conversation's quiet lines, by card number.
        let fixture = Fixture::new();
        fixture.answer(&[]);
        assert_eq!(
            fixture.pass(false, true, WeeklyUsage::Unknown),
            ["analyzing", "learned one"]
        );
        let row = crate::model::SessionRowSnapshot {
            id: SESSION.to_owned(),
            provider: "claude".to_owned(),
            provider_label: "Claude Code".to_owned(),
            locator: fixture.locator.to_string_lossy().into_owned(),
            checkout_path: fixture.root.to_string_lossy().into_owned(),
            first_human_request: None,
            started_at_unix_ms: None,
            updated_at_unix_ms: 1,
            title: None,
            unavailable_reason: None,
        };

        let detail = super::super::memory::load_session_detail(
            fixture.node.as_ref(),
            &fixture.database,
            &fixture.project_id,
            row,
        )
        .unwrap();

        let marks = detail
            .events
            .iter()
            .map(|event| (event.memory_learned.clone(), event.memory_received.clone()))
            .collect::<Vec<_>>();
        assert_eq!(marks, [(vec![2], vec![]), (vec![], vec![1])]);
        assert!(!detail.events[1].text.contains("hide-memory-receipt"));
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
        assert!(fixture.pass(false, false, WeeklyUsage::Unknown).is_empty());
        assert_eq!(fixture.calls(), 0);

        age(31);
        assert_eq!(
            fixture.pass(false, false, WeeklyUsage::Unknown),
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
        fixture.pass_for(key.clone(), true, false, WeeklyUsage::Unknown);
        assert_eq!(delivered(), 1);

        fixture.pass_for(key.clone(), false, false, WeeklyUsage::Unknown);
        assert_eq!(delivered(), 2);
        fixture.pass_for(key, false, false, WeeklyUsage::Unknown);
        assert_eq!(delivered(), 2);
        assert_eq!(fixture.calls(), 0, "an OpenCode session is not analyzed");
    }

    #[test]
    fn the_daily_cap_and_weekly_usage_defer_analysis_without_a_provider_call() {
        // B5, D-10.
        let fixture = Fixture::new();
        fixture.answer(&[]);

        assert!(
            fixture
                .pass(false, true, WeeklyUsage::Used(80.0))
                .is_empty(),
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
        assert!(
            fixture
                .pass(false, true, WeeklyUsage::Used(10.0))
                .is_empty()
        );
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
    fn unread_weekly_usage_holds_background_analysis_and_asks_for_a_read() {
        // D-10 b: with no window attached the figure is read for Memory.
        let fixture = Fixture::new();
        fixture.answer(&[]);
        let key = ("claude", SESSION.to_owned());

        let (reports, outcome) = fixture.pass_for(key.clone(), false, true, WeeklyUsage::Unread);
        assert!(reports.is_empty());
        assert_eq!(fixture.calls(), 0);
        assert!(outcome.wants_usage);
        assert!(
            !outcome.settled.contains(&key),
            "the closed session waits for the read, not for the quiet rule"
        );

        let (reports, _) = fixture.pass_for(key, false, true, WeeklyUsage::Used(10.0));
        assert_eq!(reports, ["analyzing", "learned one"]);
    }

    #[test]
    fn weekly_usage_is_read_headless_for_a_pass_and_waited_for_ten_minutes_at_most() {
        // D-10: a provider whose usage cannot be read gets the daily cap alone.
        let mut runtime = crate::runtime::tests::runtime();
        let now = now_ms();
        runtime.snapshot.navigator.provider_usage =
            crate::model::ProviderUsageSnapshot::initial_rows();
        assert!(!runtime.memory_reads_usage());
        assert_eq!(runtime.weekly_usage("claude", now), WeeklyUsage::Unread);
        assert_eq!(runtime.weekly_usage("grok", now), WeeklyUsage::Unknown);

        runtime.finish_memory_pass(PassOutcome {
            wants_usage: true,
            ..PassOutcome::default()
        });
        assert!(runtime.memory_reads_usage());
        assert!(runtime.usage_activity().window_visible);
        assert_eq!(runtime.weekly_usage("claude", now), WeeklyUsage::Unread);
        assert_eq!(
            runtime.weekly_usage("claude", now + USAGE_READ_WAIT_MS),
            WeeklyUsage::Unknown
        );

        let row = &mut runtime.snapshot.navigator.provider_usage[0];
        row.used_percent = Some(85.0);
        let provider = row.provider.clone();
        assert_eq!(
            runtime.weekly_usage(&provider, now),
            WeeklyUsage::Used(85.0)
        );
    }

    #[test]
    fn a_closed_session_stays_closed_until_a_pass_analyzes_it() {
        // B2: a failed pass does not drop the session into the 30 minute wait.
        let fixture = Fixture::new();
        let key = ("claude", SESSION.to_owned());

        // No scripted answer: the provider call fails.
        let (reports, outcome) = fixture.pass_for(key.clone(), false, true, WeeklyUsage::Unknown);
        assert_eq!(reports, ["analyzing", "failed"]);
        assert!(!outcome.settled.contains(&key));

        fixture.answer(&[]);
        let (reports, outcome) = fixture.pass_for(key.clone(), false, true, WeeklyUsage::Unknown);
        assert_eq!(reports, ["analyzing", "learned one"]);
        assert!(
            !outcome.settled.contains(&key),
            "settled by the pass that finds nothing left"
        );

        let (reports, outcome) = fixture.pass_for(key.clone(), false, true, WeeklyUsage::Unknown);
        assert!(reports.is_empty());
        assert!(outcome.settled.contains(&key));

        // A session no Memory-enabled Project lists is let go after a pass.
        let stray = ("codex", "not-in-any-project".to_owned());
        let (_, outcome) = fixture.pass_for(stray.clone(), false, true, WeeklyUsage::Unknown);
        assert!(outcome.settled.contains(&stray));
    }

    /// The Memory records a pass logged, by kind.
    fn memory_kinds(records: &[Value]) -> Vec<String> {
        records
            .iter()
            .filter(|record| record["component"] == "memory")
            .map(|record| record["kind"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    #[test]
    fn a_session_holding_an_oversized_tool_result_is_analyzed_and_its_receipts_read_past_it() {
        // A screenshot's tool result past the 256 KiB line cap, between the
        // operator's rule and the agent's reply, before the receipt.
        let fixture = Fixture::new();
        let screenshot = json!({"type": "user", "sessionId": SESSION, "cwd": fixture.root,
            "timestamp": "2026-10-11T00:00:02Z",
            "message": {"role": "user", "content": [{"type": "tool_result",
                "tool_use_id": "toolu_1", "content": "x".repeat(300 * 1024)}]}});
        let reply = json!({"type": "assistant", "sessionId": SESSION, "cwd": fixture.root,
            "timestamp": "2026-10-11T00:00:03Z",
            "message": {"role": "assistant",
                "content": [{"type": "text", "text": "Nextest runs before every merge"}]}});
        fixture.write(&[
            fixture.lines[0].clone(),
            screenshot,
            reply,
            fixture.lines[1].clone(),
        ]);
        fixture.answer(&[]);

        let (reports, records) =
            crate::diagnostics::capture(|| fixture.pass(false, true, WeeklyUsage::Unknown));
        assert_eq!(reports, ["analyzing", "learned one"]);
        assert_eq!(
            memory_kinds(&records),
            ["receipts.records_skipped", "analysis.records_skipped"]
        );
        for record in &records {
            assert_eq!(record["skipped"], json!({"non_conversation_capacity": 1}));
        }
        let inputs = fixture.backend.inputs.lock().unwrap().clone();
        assert_eq!(inputs.len(), 1);
        assert!(inputs[0].contains("Always run tests with nextest before a merge"));
        assert!(inputs[0].contains("Nextest runs before every merge"));
        assert!(
            !inputs[0].contains("xxxxxxxx"),
            "the tool result is not sent"
        );
        let seeded = fixture
            .store()
            .item_by_number(&fixture.project_id, 1)
            .unwrap()
            .unwrap();
        assert_eq!(seeded.provided_session_count, 1, "the receipt was read");

        let (reports, records) =
            crate::diagnostics::capture(|| fixture.pass(false, true, WeeklyUsage::Unknown));
        assert!(reports.is_empty());
        assert!(memory_kinds(&records).is_empty(), "{records:?}");
        assert_eq!(fixture.calls(), 1);
    }

    #[test]
    fn a_session_that_cannot_be_read_is_logged_once_per_change_of_its_file() {
        let fixture = Fixture::new();
        let store = fixture.store();
        for reader in [CursorReader::Receipts, CursorReader::Analysis] {
            store
                .save_cursor(
                    reader,
                    &SessionCursorRecord {
                        project_id: fixture.project_id.clone(),
                        provider: "claude".to_owned(),
                        session_id: SESSION.to_owned(),
                        byte_offset: 1,
                        checkpoint: b"no checkpoint".to_vec(),
                        last_content_hash: None,
                        updated_at_unix_ms: 1,
                    },
                )
                .unwrap();
        }
        drop(store);
        let logged = || {
            let (_, records) =
                crate::diagnostics::capture(|| fixture.pass(false, true, WeeklyUsage::Unknown));
            memory_kinds(&records)
        };
        let failed = ["receipts.walk_failed", "analysis.session_failed"];

        assert_eq!(logged(), failed);
        assert!(logged().is_empty(), "the same file fails the same way");

        let mut lines = fixture.lines.clone();
        lines.push(
            json!({"type": "user", "sessionId": SESSION, "cwd": fixture.root,
            "timestamp": "2026-10-11T00:01:00Z", "origin": {"kind": "human"},
            "message": {"role": "user", "content": "And lint before tests"}}),
        );
        fixture.write(&lines);
        assert_eq!(logged(), failed, "a changed file is read and logged again");
        assert_eq!(fixture.calls(), 0);
    }

    #[test]
    fn only_hide_ai_that_cannot_answer_reaches_the_screen_and_the_rest_backs_off() {
        // B4, B23, D-27.
        let now = 1_000_000;
        for error in [AiError::NotAuthenticated, AiError::NoProvider(Vec::new())] {
            let (shown, retry) = analysis_after_failure(&error, 1, now);
            assert_eq!(shown, MemoryAnalysisState::AiNotReady, "{error:?}");
            assert_eq!(retry, now + SIGN_IN_RETRY_MS);
        }

        let (shown, retry) = analysis_after_failure(
            &AiError::UsageLimited {
                retry_after: Some(std::time::Duration::from_secs(90)),
            },
            1,
            now,
        );
        assert_eq!(shown, MemoryAnalysisState::Idle);
        assert_eq!(retry, now + 90_000, "the provider's resume time is kept");

        let backoff = |failures| analysis_after_failure(&AiError::Timeout, failures, now).1 - now;
        assert_eq!(backoff(1), RETRY_FIRST_MS);
        assert_eq!(backoff(2), 2 * RETRY_FIRST_MS);
        assert_eq!(backoff(40), RETRY_LAST_MS);
        assert_eq!(
            analysis_after_failure(&AiError::Timeout, 3, now).0,
            MemoryAnalysisState::Idle
        );
    }

    #[test]
    fn the_operators_words_and_their_times_come_from_the_sent_events_and_an_unsent_offset_is_left_out()
     {
        // D-14: the conflict rule ranks the operator's words first; B21: a
        // source shows when it was said.
        let candidate = |offset| Candidate {
            text: "Use yarn".to_owned(),
            title: String::new(),
            aliases: Vec::new(),
            kind: CandidateKind::Rule,
            confidence: 0.9,
            salience: 0.9,
            source_offsets: vec![offset],
            human_offsets: Vec::new(),
            source_times: Default::default(),
            cited_paths: Vec::new(),
            relation: CandidateRelation::Supersedes {
                target_id: "card".to_owned(),
            },
        };
        let events = [
            json!({"offset": 1, "kind": "human", "at_unix_ms": 10, "text": "Use yarn now"}),
            json!({"offset": 2, "kind": "assistant", "at_unix_ms": 20, "text": "Switching to yarn"}),
        ];

        let mut checked = vec![candidate(1), candidate(2), candidate(9)];
        assert_eq!(
            validate_candidates(&mut checked, &events),
            1,
            "offset 9 was never sent"
        );

        assert_eq!(checked[0].human_offsets, [1]);
        assert!(checked[1].human_offsets.is_empty());
        assert_eq!(checked[0].source_times, [(1, 10)].into());
        assert_eq!(checked[1].source_times, [(2, 20)].into());
        assert_eq!(checked.len(), 2);
    }
}
