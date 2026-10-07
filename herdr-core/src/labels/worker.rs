//! One Herdr server's labels: which session each agent pane runs, when its
//! conversation is read, when it is analyzed, and what the projection shows.
//!
//! The session-sync coordinator owns one worker and drives it from the agent
//! state it already follows (`observe`), from its tick (`tick`) and from the
//! worker's own wake (`drain`). Nothing here runs under the runtime mutex:
//! conversation reads run on the worker's reader thread, analyses on the
//! core's one analyzer, and the coordinator hands the runtime an overlay of
//! the result with each publish (`overlay`), which the runtime lays on every
//! projection it ingests.
//!
//! A read happens only when a pane's state, status or session reference
//! moved, when a read left a backlog, when a provider wait ran out, or once
//! three seconds after a turn started with its prompt not yet written
//! (B12). A label is shown only while the pane's current reference proves
//! the session it was made for (D-04): a new session, a reused pane, a
//! provider change or an A -> B -> A switch shows nothing of the old session
//! until the new one is proven, and every read and analysis result carries
//! the generation it started under so a late one is dropped.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hide_session::label_transcript::{
    LabelEvent, LabelEventKind, LabelTranscript, LabelTranscriptRequest,
};
use hide_session::{Agent, PrSighting, label_reference_token};
use serde_json::json;

use super::analysis::{AnalysisFailure, LabelEnd};
use super::analysis::{
    AnalysisPhase, analysis_context, analysis_phase, context_fingerprint, interrupted,
    new_human_turns, newest_user_is_last, retain_bounded, rolling_analysis_context,
    task_input_cursor, turn_key,
};
use super::analyzer::{AnalysisJob, AnalysisResult, LabelAnalyzer};
use super::context_label;
use super::facts::{InputView, LogTarget, PullRequestTimes, ReadFacts};
use super::generator::GeneratorLock;
use super::input::{OperatorInput, Submit};
use super::overlay::LabelOverlay;
use super::store::{LabelStore, PaneRecord};

/// How long after a turn starts a pane whose prompt was not in the
/// transcript yet is read once more.
const FOLLOW_UP_READ: Duration = Duration::from_secs(3);
/// How many times a resting agent's session is read again, per Herdr state,
/// while what its last turn waits for is not settled.
const UNSETTLED_TURN_READS: u8 = 3;
/// How long a read the machine could not answer (a device whose helper is
/// not connected) waits before it is tried again, so the pane catches up
/// after a reconnect without waiting for its next state change (B14).
const UNAVAILABLE_RETRY: Duration = Duration::from_secs(15);
/// The most sighted pull requests waiting between two takes; a read that
/// finds more keeps the newest.
const SIGHTED_LIMIT: usize = 32;
/// How long after a session printed a pull request's address its absence from
/// GitHub's answer still sets off a read: a session prints the address of a
/// pull request it has just made, while an older sighting is a session read
/// again from its start, or a pull request past the newest 200 the list
/// holds, which a read would only answer the same way.
const SIGHTING_FRESH_MS: u64 = 15 * 60 * 1_000;

/// How much longer a sighting printed at `at_unix_ms` is recent enough to read
/// for, or `None` once it is not. Never more than the whole window: a
/// transcript written while the clock ran ahead would otherwise hold its
/// address for as long as the clock was wrong.
pub(crate) fn sighting_fresh_for(at_unix_ms: u64, now_unix_ms: u64) -> Option<Duration> {
    at_unix_ms
        .saturating_add(SIGHTING_FRESH_MS)
        .checked_sub(now_unix_ms)
        .map(|left| Duration::from_millis(left.min(SIGHTING_FRESH_MS)))
}

/// Why a read produced nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReadFailure {
    /// The machine cannot be reached now (a device without a helper
    /// connection). The pane keeps its label and waits for its next change.
    Unavailable(String),
    /// The read ran and refused, with a stable reason code.
    Refused(String),
}

/// Where a server's conversations are read: this machine's files, or a
/// device's through its helper. Blocks; called only on the reader thread.
pub(crate) trait TranscriptSource: Send + Sync {
    fn read(&self, request: &LabelTranscriptRequest) -> Result<LabelTranscript, ReadFailure>;
}

/// A pull request address a read found in a session's tool output that the
/// core's GitHub answer did not hold, with the pane whose session printed it;
/// the coordinator hands it to `Runtime::read_sighted_pull_requests`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SightedPullRequest {
    pub(crate) pane_id: String,
    /// `owner/name`, lowercase, as `PullRequestTimes` keys it.
    pub(crate) repository: String,
    pub(crate) number: u64,
    pub(crate) at_unix_ms: u64,
}

/// One agent as the coordinator's replica holds it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ObservedAgent {
    pub(crate) pane_id: String,
    pub(crate) agent: Option<String>,
    pub(crate) status: Option<String>,
    /// Herdr's `agent_session` as `(kind, value)`.
    pub(crate) reference: Option<(String, String)>,
    pub(crate) cwd: Option<String>,
    pub(crate) state_change_seq: u64,
}

pub(crate) struct WorkerConfig {
    /// The store key: the core's node id, or `device:<id>`.
    pub(crate) target: String,
    /// The Herdr server's generator lock (D-10); see `generator`.
    pub(crate) lock_path: Option<PathBuf>,
    /// Where the runtime records the operator's submits (D-19).
    pub(crate) input: Arc<OperatorInput>,
}

pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;

enum WorkerResult {
    Read {
        pane: String,
        generation: u64,
        reference: String,
        from_start: bool,
        /// The read began at the conversation's start.
        from_beginning: bool,
        /// Herdr's `state_change_seq` the read was asked under.
        state_change_seq: Option<u64>,
        result: Box<Result<LabelTranscript, ReadFailure>>,
    },
    Analysis(AnalysisOutcome),
}

/// What an analysis was asked under, carried back with its answer.
struct AnalysisMeta {
    pane: String,
    generation: u64,
    owner: String,
    turn: u64,
    phase: AnalysisPhase,
    task_input_cursor: Option<u64>,
    initial_context: bool,
    /// The turn began with the operator's request, so it may move the goal
    /// (D-09).
    operator_turn: bool,
    context_chars: usize,
}

struct AnalysisOutcome {
    meta: AnalysisMeta,
    result: AnalysisResult,
}

struct ReadJob {
    pane: String,
    generation: u64,
    reference: String,
    from_start: bool,
    state_change_seq: Option<u64>,
    request: LabelTranscriptRequest,
}

struct PaneState {
    agent: Agent,
    status: String,
    reference: Option<(String, String)>,
    reference_token: Option<String>,
    cwd: Option<String>,
    generation: u64,
    /// The conversation since the anchor, bounded; empty until the first
    /// read of this process.
    events: VecDeque<LabelEvent>,
    events_loaded: bool,
    needs_read: bool,
    next_analysis_at: Option<Instant>,
    follow_up_at: Option<Instant>,
    follow_up_armed: bool,
    /// Reads asked in this Herdr state because the turn's wait was not
    /// settled (`UNSETTLED_TURN_READS`).
    unsettled_reads: u8,
    /// The last failure logged, so a repeating one is logged once.
    last_failure: Option<String>,
    /// When this worker began seeing the pane, and so its input (D-19).
    input_observed_since_unix_ms: u64,
    /// The newest operator submit matched to a message.
    claimed_submit: Option<Submit>,
    /// The turn whose end already had its one retry after a timeout (D-33).
    end_retried: Option<u64>,
    /// The skip reasons already logged, so a cap that holds on every read
    /// (more subagent files than one read takes) is logged once.
    skip_reasons_logged: std::collections::BTreeSet<String>,
}

pub(crate) struct LabelWorker {
    target: String,
    store: Arc<LabelStore>,
    analyzer: Arc<LabelAnalyzer>,
    input: Arc<OperatorInput>,
    records: BTreeMap<String, PaneRecord>,
    panes: BTreeMap<String, PaneState>,
    generator: GeneratorLock,
    reader: Reader,
    results: Receiver<WorkerResult>,
    sender: Sender<WorkerResult>,
    wake: Wake,
    read_in_flight: Option<String>,
    analysis_in_flight: Option<String>,
    /// Panes owing an analysis while another runs, in arrival order.
    waiting: VecDeque<String>,
    next_generation: u64,
    dirty: bool,
    /// GitHub's creation time of each pull request the core has read.
    pull_request_times: Arc<PullRequestTimes>,
    /// What the reads since the last `take_sighted` found that the core has
    /// not read; see `note_sighted`.
    sighted: Vec<SightedPullRequest>,
    /// The operator's agent-summary switch (D-11). Off, no analysis is
    /// asked for and the overlay lays no AI field; reads go on, because the
    /// rows stand on what they find.
    summaries: bool,
}

impl LabelWorker {
    pub(crate) fn spawn(
        config: WorkerConfig,
        store: Arc<LabelStore>,
        analyzer: Arc<LabelAnalyzer>,
        source: Arc<dyn TranscriptSource>,
        wake: Wake,
    ) -> Result<Self, String> {
        let (sender, results) = channel();
        let reader = Reader::spawn(source, sender.clone(), Arc::clone(&wake))?;
        Ok(Self {
            records: store.target(&config.target),
            generator: GeneratorLock::new(config.lock_path, &config.target),
            target: config.target,
            input: config.input,
            store,
            analyzer,
            panes: BTreeMap::new(),
            reader,
            results,
            sender,
            wake,
            read_in_flight: None,
            analysis_in_flight: None,
            waiting: VecDeque::new(),
            next_generation: 0,
            dirty: false,
            pull_request_times: Arc::default(),
            sighted: Vec::new(),
            summaries: true,
        })
    }

    /// Follows the server's agents. `live_panes` is the server's complete
    /// pane topology when the caller has one; records of panes outside it
    /// are dropped (D-08), while an agent list alone never drops a record,
    /// because a restored server lists its agents late. Returns whether
    /// anything the projection shows changed.
    pub(crate) fn observe(
        &mut self,
        agents: &[ObservedAgent],
        live_panes: Option<&HashSet<String>>,
        now: Instant,
        now_unix_ms: u64,
    ) -> bool {
        // Settled before anything is laid on a payload, so the first
        // projection already shows what the store proves.
        let mut changed = self.ensure_generator(now);
        let mut seen = HashSet::new();
        for observed in agents {
            seen.insert(observed.pane_id.as_str());
            let mut fresh = false;
            let record = self
                .records
                .entry(observed.pane_id.clone())
                .or_insert_with(|| {
                    fresh = true;
                    PaneRecord::first_seen(observed.state_change_seq, now_unix_ms)
                });
            if fresh {
                self.dirty = true;
                changed = true;
            }
            let seq_moved = record.state_change_seq != observed.state_change_seq;
            let was_stopped = matches!(record.agent_status.as_deref(), Some("idle" | "done"));
            if seq_moved {
                record.state_change_seq = observed.state_change_seq;
                record.changed_unix_ms = now_unix_ms;
                self.dirty = true;
                changed = true;
            }
            let status = observed
                .status
                .clone()
                .unwrap_or_else(|| "unknown".to_owned());
            let status_moved = record.agent_status.as_deref() != Some(status.as_str());
            if status_moved {
                record.agent_status = Some(status.clone());
                self.dirty = true;
            }
            // A running agent is not waiting on anyone: how the last turn
            // ended (and a question's reply) is over, and the turn's end
            // writes anew. A
            // stopped agent whose state moved and is stopped again (or at a
            // permission prompt) ran in between, even when its working state
            // came and went inside one burst of Herdr events and was never
            // seen here.
            let ran = status == "working"
                || (seq_moved
                    && was_stopped
                    && matches!(status.as_str(), "idle" | "done" | "blocked"));
            if ran && let Some(end) = record.end.filter(|end| *end != LabelEnd::Working) {
                if end == LabelEnd::Question {
                    record.line.clear();
                }
                record.end = None;
                self.dirty = true;
                changed = true;
            }
            let Some(agent) = provider(observed.agent.as_deref()) else {
                self.forget_pane(&observed.pane_id);
                continue;
            };
            let reference_token = observed
                .reference
                .as_ref()
                .and_then(|(kind, value)| label_reference_token(agent.as_str(), kind, value));
            match self.panes.get_mut(&observed.pane_id) {
                None => {
                    // A restart resumes where it stopped: an unchanged pane
                    // whose position belongs to its current reference is
                    // not read until something moves (B10).
                    // An agent whose read reports its turns is read again
                    // unless its wait was read for this very state, so a
                    // record from before turns were read is not trusted.
                    let resumable = !fresh
                        && !seq_moved
                        && !status_moved
                        && reference_token.is_some()
                        && record.read_reference == reference_token
                        && record.checkpoint.is_some()
                        && (!agent.reports_turns()
                            || record.turns_seq == Some(record.state_change_seq));
                    self.next_generation += 1;
                    self.panes.insert(
                        observed.pane_id.clone(),
                        PaneState {
                            agent,
                            status,
                            reference: observed.reference.clone(),
                            reference_token: reference_token.clone(),
                            cwd: observed.cwd.clone(),
                            generation: self.next_generation,
                            events: VecDeque::new(),
                            events_loaded: false,
                            needs_read: reference_token.is_some() && !resumable,
                            next_analysis_at: None,
                            follow_up_at: None,
                            follow_up_armed: status_moved,
                            unsettled_reads: 0,
                            last_failure: None,
                            input_observed_since_unix_ms: now_unix_ms,
                            claimed_submit: None,
                            end_retried: None,
                            skip_reasons_logged: Default::default(),
                        },
                    );
                }
                Some(pane) => {
                    if pane.reference_token != reference_token || pane.agent != agent {
                        // Another session, or none: everything in flight
                        // for the old one is dropped when it lands.
                        self.next_generation += 1;
                        pane.generation = self.next_generation;
                        pane.events.clear();
                        pane.events_loaded = false;
                        pane.next_analysis_at = None;
                        pane.follow_up_at = None;
                        pane.needs_read = reference_token.is_some();
                        self.waiting.retain(|id| id != &observed.pane_id);
                        changed = true;
                        crate::diagnostic!(json!({
                            "component": "labels",
                            "kind": "session.reference_changed",
                            "target": self.target,
                            "pane_id": observed.pane_id,
                            "reference": reference_token,
                            "generation": pane.generation,
                        }));
                    }
                    if seq_moved || status_moved {
                        pane.needs_read |= reference_token.is_some();
                        pane.follow_up_armed = status == "working";
                        pane.unsettled_reads = 0;
                    }
                    pane.agent = agent;
                    pane.status = status;
                    pane.reference = observed.reference.clone();
                    pane.reference_token = reference_token;
                    pane.cwd = observed.cwd.clone();
                }
            }
        }
        let gone: Vec<String> = self
            .panes
            .keys()
            .filter(|id| !seen.contains(id.as_str()))
            .cloned()
            .collect();
        for id in gone {
            self.forget_pane(&id);
        }
        if let Some(live) = live_panes {
            // An agent Herdr lists is live even when its pane is not in
            // the pane list yet (a pane created between the two reads).
            let before = self.records.len();
            self.records
                .retain(|id, _| live.contains(id) || seen.contains(id.as_str()));
            if self.records.len() != before {
                self.dirty = true;
            }
        }
        self.schedule(now);
        self.persist();
        changed
    }

    /// Takes the core's latest pull request creation times and judges every
    /// session's sightings against them (D-31). Returns whether a row's pull
    /// requests changed.
    pub(crate) fn set_pull_request_times(&mut self, times: Arc<PullRequestTimes>) -> bool {
        if Arc::ptr_eq(&self.pull_request_times, &times) {
            return false;
        }
        self.pull_request_times = times;
        let mut changed = false;
        for record in self.records.values_mut() {
            changed |= record.facts.judge_created(&self.pull_request_times);
        }
        if changed {
            self.dirty = true;
            self.persist();
        }
        changed
    }

    /// Keeps each recent address this read sighted that the core's pull
    /// requests do not hold, once, so the core can read its project before
    /// the next re-read would (a session prints the address of a pull request
    /// it has just made). Only this Mac's projects have their pull requests
    /// read, so a device's worker keeps none.
    fn note_sighted(&mut self, pane_id: &str, sightings: &[PrSighting], now_unix_ms: u64) {
        if self.target != self.store.node() {
            return;
        }
        for sighting in sightings {
            if sighting_fresh_for(sighting.at_unix_ms, now_unix_ms).is_none() {
                continue;
            }
            let repository = sighting.repository.to_ascii_lowercase();
            let known =
                self.pull_request_times
                    .contains_key(&(repository.clone(), sighting.number))
                    || self.sighted.iter().any(|kept| {
                        kept.number == sighting.number && kept.repository == repository
                    });
            if !known {
                self.sighted.push(SightedPullRequest {
                    pane_id: pane_id.to_owned(),
                    repository,
                    number: sighting.number,
                    at_unix_ms: sighting.at_unix_ms,
                });
            }
        }
        if self.sighted.len() > SIGHTED_LIMIT {
            let dropped = self.sighted.len() - SIGHTED_LIMIT;
            self.sighted.sort_by_key(|sighting| sighting.at_unix_ms);
            self.sighted.drain(..dropped);
            crate::diagnostic!(json!({
                "component": "labels",
                "kind": "read.sighted_capped",
                "target": self.target,
                "pane_id": pane_id,
                "dropped": dropped,
            }));
        }
    }

    /// Takes what the reads since the last take sighted (`note_sighted`).
    pub(crate) fn take_sighted(&mut self) -> Vec<SightedPullRequest> {
        std::mem::take(&mut self.sighted)
    }

    /// Runs what came due: the generator role, provider waits, follow-up
    /// reads. Returns whether anything shown changed.
    pub(crate) fn tick(&mut self, now: Instant) -> bool {
        let mut changed = self.ensure_generator(now);
        if !self.generator.held() {
            return false;
        }
        let ids: Vec<String> = self.panes.keys().cloned().collect();
        for id in ids {
            let pane = self.panes.get_mut(&id).expect("listed above");
            if pane.follow_up_at.is_some_and(|at| now >= at) {
                pane.follow_up_at = None;
                pane.needs_read |= pane.reference_token.is_some();
            }
            if pane.next_analysis_at.is_some_and(|at| now >= at) {
                pane.next_analysis_at = None;
                if pane.events_loaded {
                    changed |= self.decide(&id, now);
                } else {
                    pane.needs_read |= pane.reference_token.is_some();
                }
            }
        }
        self.schedule(now);
        self.persist();
        changed
    }

    /// Takes the reader's and the analyzer's results. Returns whether
    /// anything shown changed.
    pub(crate) fn drain(&mut self, now: Instant, now_unix_ms: u64) -> bool {
        let mut changed = false;
        while let Ok(result) = self.results.try_recv() {
            changed |= match result {
                WorkerResult::Read {
                    pane,
                    generation,
                    reference,
                    from_start,
                    from_beginning,
                    state_change_seq,
                    result,
                } => self.handle_read(
                    &pane,
                    generation,
                    &reference,
                    (from_start, from_beginning, state_change_seq),
                    *result,
                    (now, now_unix_ms),
                ),
                WorkerResult::Analysis(outcome) => self.handle_analysis(outcome, now),
            };
        }
        self.schedule(now);
        self.persist();
        changed
    }

    /// What these labels lay onto a projection; see [`LabelOverlay`].
    pub(crate) fn overlay(&self) -> LabelOverlay {
        LabelOverlay::of_records(&self.records, self.generator.held(), self.summaries)
    }

    /// Takes the operator's agent-summary switch. Off cancels the request
    /// running and asks for nothing more; on asks each pane for its current
    /// turn only, since the turns that ended while it was off are not owed.
    /// Returns whether what the projection shows changed.
    pub(crate) fn set_summaries(&mut self, on: bool, now: Instant) -> bool {
        if self.summaries == on {
            return false;
        }
        self.summaries = on;
        if on {
            for (id, pane) in &self.panes {
                if pane.events_loaded && !self.waiting.contains(id) {
                    self.waiting.push_back(id.clone());
                }
            }
            self.schedule(now);
        } else {
            self.waiting.clear();
            if self.analysis_in_flight.is_some() {
                self.analyzer.cancel_running();
            }
        }
        true
    }

    /// Nothing is being read or analyzed and nothing waits to be.
    #[cfg(test)]
    pub(crate) fn settled(&self) -> bool {
        self.read_in_flight.is_none()
            && self.analysis_in_flight.is_none()
            && self.waiting.is_empty()
            && !self
                .panes
                .values()
                .any(|pane| pane.needs_read && pane.reference_token.is_some())
    }

    /// Takes the generator role when it is free. Returns whether this worker
    /// just took it over from a standby, in which case whatever moved while
    /// another daemon generated is read again.
    fn ensure_generator(&mut self, now: Instant) -> bool {
        let (_, took_over) = self.generator.ensure(now);
        if took_over {
            for pane in self.panes.values_mut() {
                pane.needs_read |= pane.reference_token.is_some();
            }
        }
        took_over
    }

    fn forget_pane(&mut self, pane_id: &str) {
        if self.panes.remove(pane_id).is_some() {
            self.waiting.retain(|id| id != pane_id);
        }
    }

    fn persist(&mut self) {
        if std::mem::take(&mut self.dirty) {
            self.store.save_target(&self.target, &self.records);
        }
    }

    fn schedule(&mut self, now: Instant) {
        if !self.generator.held() {
            return;
        }
        self.schedule_read();
        self.schedule_analysis(now);
    }

    fn schedule_read(&mut self) {
        if self.read_in_flight.is_some() {
            return;
        }
        let next = self
            .panes
            .iter()
            .find(|(_, pane)| pane.needs_read && pane.reference_token.is_some())
            .map(|(id, _)| id.clone());
        let Some(id) = next else {
            return;
        };
        let pane = self.panes.get_mut(&id).expect("found above");
        pane.needs_read = false;
        let record = self.records.get(&id);
        let reference = pane.reference_token.clone().expect("filtered above");
        let same_file =
            record.is_some_and(|record| record.read_reference.as_ref() == Some(&reference));
        // An in-memory reader appends after the checkpoint; a reader that
        // lost its events (a restart) starts at the last human record, and a
        // new file starts at its beginning.
        let checkpoint = if !same_file {
            None
        } else if pane.events_loaded {
            record.and_then(|record| record.checkpoint.clone())
        } else {
            record.and_then(|record| record.anchor.clone())
        };
        let (kind, value) = pane.reference.clone().expect("a token has a reference");
        let subagents = record
            .filter(|_| same_file)
            .map(|record| record.facts.subagents.clone())
            .unwrap_or_default();
        let turns = record
            .filter(|_| same_file)
            .and_then(|record| record.turns.clone());
        let job = ReadJob {
            pane: id.clone(),
            generation: pane.generation,
            reference,
            from_start: !pane.events_loaded || checkpoint.is_none(),
            state_change_seq: record.map(|record| record.state_change_seq),
            request: LabelTranscriptRequest {
                agent: pane.agent,
                reference_kind: kind,
                reference_value: value,
                cwd: pane.cwd.clone(),
                checkpoint,
                subagents,
                turns,
            },
        };
        self.read_in_flight = Some(id);
        self.reader.submit(job);
    }

    fn schedule_analysis(&mut self, now: Instant) {
        while self.analysis_in_flight.is_none() {
            let Some(id) = self.waiting.pop_front() else {
                return;
            };
            self.decide(&id, now);
        }
    }

    fn log_failure(&mut self, pane_id: &str, kind: &str, reason: &str) {
        let Some(pane) = self.panes.get_mut(pane_id) else {
            return;
        };
        let key = format!("{kind}:{reason}");
        if pane.last_failure.as_deref() == Some(key.as_str()) {
            return;
        }
        pane.last_failure = Some(key);
        crate::diagnostic!(json!({
            "component": "labels",
            "kind": kind,
            "target": self.target,
            "pane_id": pane_id,
            "reference": pane.reference_token,
            "generation": pane.generation,
            "reason": reason,
        }));
    }

    fn handle_read(
        &mut self,
        pane_id: &str,
        generation: u64,
        reference: &str,
        (from_start, from_beginning, asked_seq): (bool, bool, Option<u64>),
        result: Result<LabelTranscript, ReadFailure>,
        (now, now_unix_ms): (Instant, u64),
    ) -> bool {
        if self.read_in_flight.as_deref() == Some(pane_id) {
            self.read_in_flight = None;
        }
        let Some(pane) = self.panes.get(pane_id) else {
            return false;
        };
        if pane.generation != generation || pane.reference_token.as_deref() != Some(reference) {
            return false;
        }
        let transcript = match result {
            Ok(transcript) => transcript,
            Err(ReadFailure::Unavailable(reason)) => {
                self.log_failure(pane_id, "read.unavailable", &reason);
                if let Some(pane) = self.panes.get_mut(pane_id) {
                    pane.follow_up_at = Some(now + UNAVAILABLE_RETRY);
                }
                return false;
            }
            Err(ReadFailure::Refused(reason)) => {
                self.log_failure(pane_id, "read.refused", &reason);
                return false;
            }
        };
        let mut changed = false;
        let target = self.target.clone();
        let record = self
            .records
            .get_mut(pane_id)
            .expect("an observed pane has a record");
        let owner = transcript.confirmed.owner.clone();
        // Forgetting the verdicts forgets which submits they claimed too, so
        // the messages are judged again against every submit kept.
        let mut verdicts_forgotten = false;
        if record.owner.as_deref() != Some(owner.as_str()) {
            if record.owner.is_some() || record.goal.is_some() {
                crate::diagnostic!(json!({
                    "component": "labels",
                    "kind": "session.reset",
                    "target": target,
                    "pane_id": pane_id,
                    "reference": reference,
                    "generation": generation,
                }));
            }
            record.reset_session(Some(owner));
            record.forget_position();
            self.waiting.retain(|id| id != pane_id);
            verdicts_forgotten = true;
            changed = true;
        } else if transcript.rescanned.is_some()
            || record
                .incarnation
                .as_ref()
                .is_some_and(|incarnation| *incarnation != transcript.confirmed.incarnation)
        {
            crate::diagnostic!(json!({
                "component": "labels",
                "kind": "session.transcript_restarted",
                "target": target,
                "pane_id": pane_id,
                "generation": generation,
                "reason": transcript.rescanned,
            }));
            record.reset_analysis();
            verdicts_forgotten = true;
        }
        if record.proven_reference.as_deref() != Some(reference) {
            record.proven_reference = Some(reference.to_owned());
            changed = true;
        }
        record.read_reference = Some(reference.to_owned());
        record.checkpoint = Some(transcript.checkpoint.clone());
        if from_start || transcript.anchor.is_some() {
            record.anchor = transcript.anchor.clone();
        }
        record.incarnation = Some(transcript.confirmed.incarnation.clone());
        // The wait is bound to the state the read was asked under, and known
        // only once the backlog is read (D-06).
        let waited = record.turn_read();
        record.turns = transcript.turns.clone();
        record.turns_seq = asked_seq.filter(|_| !transcript.has_more);
        changed |= record.turn_read() != waited;
        self.dirty = true;
        let pane = self.panes.get_mut(pane_id).expect("checked above");
        if verdicts_forgotten {
            pane.claimed_submit = None;
        }
        let submits = self.input.submits(&target, pane_id);
        changed |= record.facts.fold(
            ReadFacts {
                events: &transcript.events,
                title: transcript.title.as_deref(),
                custom_title: transcript.custom_title.as_deref(),
                sightings: &transcript.pr_sightings,
                subagents: &transcript.subagents,
                from_beginning: from_beginning || transcript.rescanned.is_some(),
            },
            InputView {
                submits: &submits,
                observed_since_unix_ms: pane.input_observed_since_unix_ms,
                claimed: &mut pane.claimed_submit,
            },
            &LogTarget {
                target: &target,
                pane_id,
            },
        );
        changed |= record.facts.judge_created(&self.pull_request_times);
        self.note_sighted(pane_id, &transcript.pr_sightings, now_unix_ms);
        let pane = self.panes.get_mut(pane_id).expect("checked above");
        let new_reason = transcript
            .skipped_reasons
            .keys()
            .any(|reason| !pane.skip_reasons_logged.contains(reason));
        if transcript.skipped_lines > 0 || new_reason {
            crate::diagnostic!(json!({
                "component": "labels",
                "kind": "read.lines_skipped",
                "target": target,
                "pane_id": pane_id,
                "lines": transcript.skipped_lines,
                "reasons": transcript.skipped_reasons,
            }));
            pane.skip_reasons_logged
                .extend(transcript.skipped_reasons.keys().cloned());
        }
        pane.last_failure = None;
        if from_start || transcript.rescanned.is_some() {
            pane.events.clear();
        }
        pane.events.extend(transcript.events);
        retain_bounded(&mut pane.events);
        pane.events_loaded = true;
        if transcript.has_more {
            // Finish the backlog before naming a task from part of it.
            pane.needs_read = true;
            return changed;
        }
        let armed = std::mem::take(&mut pane.follow_up_armed);
        let submitted_before = self.analysis_in_flight.clone();
        changed |= self.decide(pane_id, now);
        // The turn started but its prompt was not written yet: one more
        // read shortly after, rather than naming the task at its end.
        if armed
            && let Some(pane) = self.panes.get_mut(pane_id)
            && pane.status == "working"
            && self.analysis_in_flight == submitted_before
            && !self.waiting.iter().any(|id| id == pane_id)
        {
            let record = &self.records[pane_id];
            let start_owed = turn_key(pane.events.make_contiguous())
                .is_none_or(|turn| record.analysis_turn_start == Some(turn));
            if start_owed {
                pane.follow_up_at = Some(now + FOLLOW_UP_READ);
            }
        }
        // Herdr can read the agent at rest before its session file records
        // how the turn ended; the wait is then not known and the bell holds.
        // Read again shortly, a few times per state, instead of until the
        // state moves.
        if let Some(pane) = self.panes.get_mut(pane_id)
            && pane.agent.reports_turns()
            && pane.status != "working"
            && pane.follow_up_at.is_none()
            && pane.unsettled_reads < UNSETTLED_TURN_READS
            && self.records[pane_id]
                .turn_read()
                .is_some_and(|(_, waiting)| waiting.is_none())
        {
            pane.unsettled_reads += 1;
            pane.follow_up_at = Some(now + FOLLOW_UP_READ);
        }
        changed
    }

    /// Whether the pane owes an analysis now and, if the analyzer is free
    /// for this server, hands it in. Returns whether anything shown changed.
    fn decide(&mut self, pane_id: &str, now: Instant) -> bool {
        if !self.summaries {
            return false;
        }
        let Some(pane) = self.panes.get_mut(pane_id) else {
            return false;
        };
        let Some(record) = self.records.get_mut(pane_id) else {
            return false;
        };
        let (Some(owner), Some(reference)) = (record.owner.clone(), pane.reference_token.clone())
        else {
            return false;
        };
        if !record.proven_for(Some(&reference)) || !pane.events_loaded {
            return false;
        }
        let events = pane.events.make_contiguous();
        let working = pane.status == "working";
        // An interruption is the user's own act, not a new task: the turn is
        // settled and no request is spent on it.
        if !working && interrupted(events) {
            let turn = turn_key(events);
            if record.analysis_turn_start != turn || record.analysis_turn_end != turn {
                record.analysis_turn_start = turn;
                record.analysis_turn_end = turn;
                self.dirty = true;
            }
            return false;
        }
        let Some(turn) = turn_key(events) else {
            return false;
        };
        let initial_context = record.goal.is_none() || record.task_input_cursor.is_none();
        let Some(phase) = analysis_phase(
            newest_user_is_last(events),
            working,
            record.analysis_turn_start == Some(turn),
            record.analysis_turn_end == Some(turn),
        ) else {
            pane.next_analysis_at = None;
            return false;
        };
        if pane.next_analysis_at.is_some_and(|at| now < at) {
            return false;
        }
        if self.analysis_in_flight.as_deref() == Some(pane_id) {
            return false;
        }
        if self.analysis_in_flight.is_some() {
            if !self.waiting.iter().any(|id| id == pane_id) {
                self.waiting.push_back(pane_id.to_owned());
            }
            return false;
        }
        // The goal reads the operator's requests only (D-09).
        let facts = &record.facts;
        let operator = |event: &LabelEvent| facts.feeds_analysis(event.offset);
        let operator_turn = events
            .iter()
            .rfind(|event| event.kind == LabelEventKind::Human)
            .is_some_and(operator);
        let context = if initial_context {
            analysis_context(events, &operator)
        } else {
            let mut delta = new_human_turns(events, record.task_input_cursor);
            delta.retain(operator);
            rolling_analysis_context(
                record.goal.as_deref().unwrap_or_default(),
                if phase == AnalysisPhase::TurnStart {
                    &delta
                } else {
                    &[]
                },
                events,
            )
        };
        if context.is_empty() {
            return false;
        }
        let generation = pane.generation;
        let retry = if phase == AnalysisPhase::TurnEnd && pane.end_retried == Some(turn) {
            ":retry"
        } else {
            ""
        };
        let request_id = format!(
            "{}/{pane_id}:{owner}:{generation}:{turn:016x}:{}{retry}:{:016x}",
            self.target,
            phase.label(),
            context_fingerprint(&context)
        );
        let request =
            context_label::request(&format!("{}/{pane_id}", self.target), request_id, &context);
        let meta = AnalysisMeta {
            pane: pane_id.to_owned(),
            generation,
            owner,
            turn,
            phase,
            task_input_cursor: task_input_cursor(events),
            initial_context,
            operator_turn,
            context_chars: context.chars().count(),
        };
        self.analysis_in_flight = Some(pane_id.to_owned());
        let sender = self.sender.clone();
        let wake = Arc::clone(&self.wake);
        self.analyzer.submit(AnalysisJob {
            request,
            done: Box::new(move |result| {
                let _ = sender.send(WorkerResult::Analysis(AnalysisOutcome { meta, result }));
                wake();
            }),
        });
        false
    }

    fn handle_analysis(&mut self, outcome: AnalysisOutcome, now: Instant) -> bool {
        let AnalysisOutcome {
            meta: outcome,
            result,
        } = outcome;
        if self.analysis_in_flight.as_deref() == Some(outcome.pane.as_str()) {
            self.analysis_in_flight = None;
        }
        let pane_id = outcome.pane.as_str();
        let current = self.panes.get(pane_id).is_some_and(|pane| {
            pane.generation == outcome.generation
                && self
                    .records
                    .get(pane_id)
                    .and_then(|record| record.owner.as_deref())
                    == Some(outcome.owner.as_str())
        });
        if !current {
            crate::diagnostic!(json!({
                "component": "labels",
                "kind": "analysis.discarded_session",
                "target": self.target,
                "pane_id": pane_id,
                "generation": outcome.generation,
            }));
            if self.panes.contains_key(pane_id) && !self.waiting.iter().any(|id| id == pane_id) {
                self.waiting.push_back(pane_id.to_owned());
            }
            return false;
        }
        // Switched off while it ran: nothing it said is kept, and the turn
        // is asked again when the switch comes back.
        if !self.summaries {
            return false;
        }
        match result {
            Ok((provider, analysis)) => {
                let record = self.records.get_mut(pane_id).expect("checked above");
                let previous_goal = record.goal.clone();
                // Only the first analysis or the start of a turn the
                // operator asked for may move the goal (D-09).
                let may_update_goal = outcome.initial_context
                    || previous_goal.is_none()
                    || (outcome.phase == AnalysisPhase::TurnStart
                        && outcome.operator_turn
                        && analysis.goal_changed);
                let goal_changed =
                    may_update_goal && previous_goal.as_deref() != Some(analysis.goal.as_str());
                if goal_changed {
                    record.goal = Some(analysis.goal.clone());
                }
                record.line = analysis.line.clone();
                record.end = Some(analysis.end);
                record.task_input_cursor = outcome.task_input_cursor.or(record.task_input_cursor);
                match outcome.phase {
                    AnalysisPhase::TurnStart => record.analysis_turn_start = Some(outcome.turn),
                    // Recording the start too keeps a turn first seen at its
                    // end from going back for a task it no longer needs.
                    AnalysisPhase::TurnEnd => {
                        record.analysis_turn_start = Some(outcome.turn);
                        record.analysis_turn_end = Some(outcome.turn);
                    }
                }
                self.dirty = true;
                // Enough to reconstruct a verdict later without any content.
                crate::diagnostic!(json!({
                    "component": "labels",
                    "kind": "analysis.recorded",
                    "target": self.target,
                    "pane_id": pane_id,
                    "generation": outcome.generation,
                    "phase": outcome.phase.label(),
                    "goal_changed": goal_changed,
                    "operator_turn": outcome.operator_turn,
                    "goal_chars": analysis.goal.chars().count(),
                    "line_chars": analysis.line.chars().count(),
                    "end": analysis.end,
                    "context_chars": outcome.context_chars,
                    "turn": format!("{:016x}", outcome.turn),
                    "provider": provider.to_string(),
                }));
                if let Some(pane) = self.panes.get_mut(pane_id) {
                    pane.next_analysis_at = None;
                }
                true
            }
            // A stopping daemon records nothing; the next one asks again.
            Err(AnalysisFailure::Stopped) => false,
            Err(failure) => {
                // The turn's own line and end are unknown now: the row shows
                // its facts and never stops on a guess; the goal stays
                // (D-33, B20).
                let record = self.records.get_mut(pane_id).expect("checked above");
                let cleared = record.end.is_some() || !record.line.is_empty();
                if cleared {
                    record.end = None;
                    record.line.clear();
                    self.dirty = true;
                }
                let pane = self.panes.get_mut(pane_id).expect("checked above");
                if outcome.phase == AnalysisPhase::TurnEnd
                    && failure.timed_out()
                    && pane.end_retried != Some(outcome.turn)
                {
                    pane.end_retried = Some(outcome.turn);
                    pane.next_analysis_at = Some(now);
                    self.log_failure(pane_id, "analysis.retried", &failure.detail());
                    return cleared;
                }
                match failure.retry_after() {
                    Some(wait) => {
                        if let Some(pane) = self.panes.get_mut(pane_id) {
                            pane.next_analysis_at = Some(now + wait);
                        }
                        self.log_failure(
                            pane_id,
                            "analysis.provider_unavailable",
                            &failure.detail(),
                        );
                    }
                    None => {
                        // Park the turn so it is not asked again until the
                        // user takes the next one.
                        let record = self.records.get_mut(pane_id).expect("checked above");
                        record.analysis_turn_start = Some(outcome.turn);
                        record.analysis_turn_end = Some(outcome.turn);
                        self.dirty = true;
                        self.log_failure(pane_id, "analysis.abandoned", &failure.detail());
                    }
                }
                cleared
            }
        }
    }
}

fn provider(kind: Option<&str>) -> Option<Agent> {
    match kind? {
        "claude" => Some(Agent::Claude),
        "codex" => Some(Agent::Codex),
        "opencode" => Some(Agent::OpenCode),
        _ => None,
    }
}

/// The worker's one reader thread: reads run one at a time, in the order
/// the worker hands them in, and each answer wakes the coordinator.
struct Reader {
    jobs: Option<Sender<ReadJob>>,
    thread: Option<JoinHandle<()>>,
}

impl Reader {
    fn spawn(
        source: Arc<dyn TranscriptSource>,
        results: Sender<WorkerResult>,
        wake: Wake,
    ) -> Result<Self, String> {
        let (jobs, receiver) = channel::<ReadJob>();
        let thread = std::thread::Builder::new()
            .name("herdr-core-labels-reader".to_owned())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    // The answer must arrive whatever happens here: a panic
                    // that escaped would leave this server's reads in flight
                    // forever.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        source.read(&job.request)
                    }))
                    .unwrap_or_else(|_| Err(ReadFailure::Refused("reader_panicked".to_owned())));
                    if results
                        .send(WorkerResult::Read {
                            pane: job.pane,
                            generation: job.generation,
                            reference: job.reference,
                            from_start: job.from_start,
                            from_beginning: job.request.checkpoint.is_none(),
                            state_change_seq: job.state_change_seq,
                            result: Box::new(result),
                        })
                        .is_err()
                    {
                        return;
                    }
                    wake();
                }
            })
            .map_err(|error| format!("label reader could not be started: {error}"))?;
        Ok(Self {
            jobs: Some(jobs),
            thread: Some(thread),
        })
    }

    fn submit(&self, job: ReadJob) {
        if let Some(jobs) = self.jobs.as_ref() {
            let _ = jobs.send(job);
        }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
