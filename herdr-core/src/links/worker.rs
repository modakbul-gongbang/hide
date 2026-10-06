//! The link worker: one named thread that holds the record's only write
//! connection and does every file and SQLite read and write for it outside
//! `Mutex<Runtime>` (D-27, D-29).
//!
//! Its mailbox holds the newest copy of each core fact (projects, panes,
//! parents) and the newest panel read; older ones are replaced, never queued
//! (D-40). Between them it reads changed session files from this machine,
//! one file and one read budget at a time, yielding after 20 ms so a
//! backfill never competes with input.

use super::store::{IssueLinks, LinkStore, Opened, PrLinks};
use super::{BACKFILL_MS, PaneFact, ParentFact, ProjectFacts, ProjectLinkSummary, now_ms};
use hide_session::links::{self, Candidate, ReadRequest};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How often the session folders are listed for changed files.
const LIST_EVERY: Duration = Duration::from_secs(15);
/// How often the panes' spans are extended while they stay.
const PANES_EVERY: Duration = Duration::from_secs(60);
/// How often expired sessions are removed (D-18).
const PRUNE_EVERY: Duration = Duration::from_secs(30 * 60);
/// The longest a summary waits after a write, so a burst publishes once.
const SUMMARY_AFTER: Duration = Duration::from_secs(2);
/// One turn of file reading.
const TURN: Duration = Duration::from_millis(20);
/// The rest between two turns while files wait to be read.
const REST: Duration = Duration::from_millis(30);
/// A listing reaches back this far before the last one started, so a file
/// written while it ran is listed again.
const LIST_OVERLAP_MS: u64 = 60_000;

const BACKFILL_DONE: &str = "backfill_done";
const LISTED_AT: &str = "listed_at";

/// What the worker hands back to the runtime.
pub trait Sink: Send + 'static {
    /// Each project's counts and chips, by workspace id, when one changed.
    fn summaries(&self, summaries: BTreeMap<String, ProjectLinkSummary>);
    /// A panel read's answer; `generation` fences a superseded one.
    fn panel(&self, generation: u64, answer: Result<PanelAnswer, String>);
    /// Whether the first fill or a listing's reads are running (B24).
    fn filling(&self, filling: bool);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelTarget {
    Pr(u64),
    Issue(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelRequest {
    pub generation: u64,
    /// The project's record key.
    pub project: String,
    pub target: PanelTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelAnswer {
    Pr(Option<PrLinks>),
    Issue(IssueLinks),
}

#[derive(Default)]
struct Mailbox {
    stop: bool,
    projects: Option<Arc<Vec<ProjectFacts>>>,
    panes: Option<Arc<Vec<PaneFact>>>,
    parents: Option<Arc<Vec<ParentFact>>>,
    panel: Option<PanelRequest>,
    /// The open panel closed: its reads stop following writes.
    panel_closed: bool,
}

/// The runtime's handle on the worker's mailbox.
#[derive(Clone)]
pub struct LinkClient(Arc<(Mutex<Mailbox>, Condvar)>);

impl LinkClient {
    fn post(&self, put: impl FnOnce(&mut Mailbox)) {
        let (lock, wake) = &*self.0;
        if let Ok(mut mailbox) = lock.lock() {
            put(&mut mailbox);
            wake.notify_one();
        }
    }

    pub fn projects(&self, projects: Arc<Vec<ProjectFacts>>) {
        self.post(|mailbox| mailbox.projects = Some(projects));
    }

    pub fn panes(&self, panes: Arc<Vec<PaneFact>>) {
        self.post(|mailbox| mailbox.panes = Some(panes));
    }

    pub fn parents(&self, parents: Arc<Vec<ParentFact>>) {
        self.post(|mailbox| mailbox.parents = Some(parents));
    }

    pub fn read(&self, request: PanelRequest) {
        self.post(|mailbox| {
            mailbox.panel = Some(request);
            mailbox.panel_closed = false;
        });
    }

    pub fn close_panel(&self) {
        self.post(|mailbox| {
            mailbox.panel = None;
            mailbox.panel_closed = true;
        });
    }
}

/// Where the worker reads and writes.
#[derive(Clone, Debug)]
pub struct Paths {
    pub store: PathBuf,
    /// The search index, whose `policy` holds each project's Copied history.
    pub search: PathBuf,
    /// The home whose session folders are this machine's.
    pub home: Option<PathBuf>,
    pub local_device: String,
}

pub struct LinkWorker {
    client: LinkClient,
    join: Option<thread::JoinHandle<()>>,
}

impl Drop for LinkWorker {
    fn drop(&mut self) {
        self.client.post(|mailbox| mailbox.stop = true);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl LinkWorker {
    pub fn spawn(paths: Paths, sink: impl Sink) -> Result<Self, String> {
        let client = LinkClient(Arc::new((Mutex::new(Mailbox::default()), Condvar::new())));
        let mailbox = client.clone();
        let join = thread::Builder::new()
            .name("hide-links".into())
            .spawn(move || run(&mailbox, &paths, &sink))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            client,
            join: Some(join),
        })
    }

    pub fn client(&self) -> LinkClient {
        self.client.clone()
    }
}

struct State {
    projects: Arc<Vec<ProjectFacts>>,
    panes: Arc<Vec<PaneFact>>,
    panel: Option<PanelRequest>,
    panel_answer: Option<Result<PanelAnswer, String>>,
    summaries: BTreeMap<String, ProjectLinkSummary>,
    queue: VecDeque<Candidate>,
    filling: bool,
    dirty_at: Option<Instant>,
    listed: Option<Instant>,
    panes_at: Option<Instant>,
    pruned: Option<Instant>,
}

fn run(client: &LinkClient, paths: &Paths, sink: &impl Sink) {
    let mut store = match LinkStore::open(&paths.store) {
        Ok((store, opened)) => {
            if opened == Opened::Rebuilt {
                crate::diagnostic!(serde_json::json!({
                    "component": "links", "kind": "store.rebuilt", "code": "links_store_corrupt",
                }));
            }
            Some(store)
        }
        Err(code) => {
            crate::diagnostic!(serde_json::json!({
                "component": "links", "kind": "store.open_failed", "code": code,
            }));
            None
        }
    };
    let mut state = State {
        projects: Arc::default(),
        panes: Arc::default(),
        panel: None,
        panel_answer: None,
        summaries: BTreeMap::new(),
        queue: VecDeque::new(),
        filling: false,
        dirty_at: None,
        listed: None,
        panes_at: None,
        pruned: None,
    };
    loop {
        let (lock, wake) = &*client.0;
        let Ok(mut mailbox) = lock.lock() else { return };
        let wait = next_wait(&state);
        if !mailbox.stop
            && mailbox.projects.is_none()
            && mailbox.panes.is_none()
            && mailbox.parents.is_none()
            && mailbox.panel.is_none()
            && !mailbox.panel_closed
            && wait > Duration::ZERO
        {
            mailbox = match wake.wait_timeout(mailbox, wait) {
                Ok((mailbox, _)) => mailbox,
                Err(_) => return,
            };
        }
        if mailbox.stop {
            return;
        }
        let projects = mailbox.projects.take();
        let panes = mailbox.panes.take();
        let parents = mailbox.parents.take();
        let panel = mailbox.panel.take();
        let panel_closed = std::mem::take(&mut mailbox.panel_closed);
        drop(mailbox);

        if panel_closed {
            state.panel = None;
            state.panel_answer = None;
        }
        let Some(store) = store.as_mut() else {
            if let Some(request) = panel {
                sink.panel(
                    request.generation,
                    Err("links_store_unavailable".to_owned()),
                );
            }
            continue;
        };
        if let Some(projects) = projects {
            for project in projects.iter() {
                if let Err(code) = store.apply_project(project, now_ms()) {
                    log_write(&code, "project");
                }
            }
            state.projects = projects;
            state.dirty_at.get_or_insert_with(Instant::now);
        }
        if let Some(panes) = panes {
            state.panes = panes;
            state.panes_at = None;
        }
        if state.panes_at.is_none_or(|at| at.elapsed() >= PANES_EVERY) {
            if !state.panes.is_empty() {
                if let Err(code) = store.apply_panes(&state.panes, now_ms()) {
                    log_write(&code, "panes");
                }
                state.dirty_at.get_or_insert_with(Instant::now);
            }
            state.panes_at = Some(Instant::now());
        }
        if let Some(parents) = parents {
            if let Err(code) = store.apply_parents(&parents) {
                log_write(&code, "parents");
            }
            state.dirty_at.get_or_insert_with(Instant::now);
        }
        if let Some(request) = panel {
            state.panel = Some(request);
            state.panel_answer = None;
            answer_panel(store, paths, &mut state, sink);
        }
        if let Some(home) = paths.home.as_deref() {
            if state.listed.is_none_or(|at| at.elapsed() >= LIST_EVERY) && state.queue.is_empty() {
                list(store, home, paths, &mut state);
            }
            read_turn(store, home, paths, &mut state);
            let filling = !state.queue.is_empty();
            if filling != state.filling {
                state.filling = filling;
                sink.filling(filling);
            }
        }
        if state.pruned.is_none_or(|at| at.elapsed() >= PRUNE_EVERY) {
            prune(store, paths);
            state.pruned = Some(Instant::now());
            state.dirty_at.get_or_insert_with(Instant::now);
        }
        if state
            .dirty_at
            .is_some_and(|at| at.elapsed() >= SUMMARY_AFTER || state.queue.is_empty())
        {
            state.dirty_at = None;
            publish_summaries(store, &mut state, sink);
            if state.panel.is_some() {
                answer_panel(store, paths, &mut state, sink);
            }
        }
    }
}

fn next_wait(state: &State) -> Duration {
    if !state.queue.is_empty() {
        return REST;
    }
    let mut wait = LIST_EVERY;
    if let Some(at) = state.dirty_at {
        wait = wait.min(SUMMARY_AFTER.saturating_sub(at.elapsed()));
    }
    if let Some(at) = state.listed {
        wait = wait.min(LIST_EVERY.saturating_sub(at.elapsed()));
    }
    wait
}

fn log_write(code: &str, what: &str) {
    crate::diagnostic!(serde_json::json!({
        "component": "links", "kind": "store.write_failed", "what": what, "code": code,
    }));
}

/// Lists the session files changed since the last listing (the last 90
/// days the first time, D-17) and queues the ones whose stamp moved.
fn list(store: &LinkStore, home: &Path, paths: &Paths, state: &mut State) {
    let started = now_ms();
    state.listed = Some(Instant::now());
    let since = match (store.meta(BACKFILL_DONE), store.meta(LISTED_AT)) {
        (Ok(Some(_)), Ok(Some(at))) => at
            .parse::<u64>()
            .unwrap_or(0)
            .saturating_sub(LIST_OVERLAP_MS),
        (Ok(_), Ok(_)) => started.saturating_sub(BACKFILL_MS),
        (Err(code), _) | (_, Err(code)) => {
            log_write(&code, "listing");
            return;
        }
    };
    let candidates = match links::candidates(home, since) {
        Ok(candidates) => candidates,
        Err(code) => {
            crate::diagnostic!(serde_json::json!({
                "component": "links", "kind": "listing.failed", "code": code,
            }));
            return;
        }
    };
    let stamps = match store.stamps(&paths.local_device) {
        Ok(stamps) => stamps,
        Err(code) => {
            log_write(&code, "listing");
            return;
        }
    };
    // Oldest first, so a session's earlier file is read before the one that
    // continues it.
    state.queue = candidates
        .into_iter()
        .rev()
        .filter(|candidate| stamps.get(&candidate.path) != Some(&candidate.stamp))
        .collect();
    if let Err(code) = store.set_meta(LISTED_AT, &started.to_string()) {
        log_write(&code, "listing");
    }
    if state.queue.is_empty() {
        mark_filled(store);
    }
}

fn mark_filled(store: &LinkStore) {
    if store.meta(BACKFILL_DONE).is_ok_and(|done| done.is_none()) {
        if let Err(code) = store.set_meta(BACKFILL_DONE, "1") {
            log_write(&code, "backfill");
        } else {
            crate::diagnostic!(serde_json::json!({"component": "links", "kind": "backfill.done"}));
        }
    }
}

/// Reads queued files for one turn: one read budget per file per step, a
/// file with more waiting goes back to the front.
fn read_turn(store: &mut LinkStore, home: &Path, paths: &Paths, state: &mut State) {
    if state.queue.is_empty() {
        return;
    }
    let turn = Instant::now();
    while turn.elapsed() < TURN {
        let Some(candidate) = state.queue.pop_front() else {
            break;
        };
        let checkpoint = match store.checkpoint(&paths.local_device, &candidate.path) {
            Ok(checkpoint) => checkpoint,
            Err(code) => {
                log_write(&code, "cursor");
                continue;
            }
        };
        let answer = links::read(
            home,
            &[ReadRequest {
                agent: candidate.agent,
                path: candidate.path.clone(),
                checkpoint,
            }],
        )
        .remove(0);
        if let Some(code) = answer.error.as_deref() {
            crate::diagnostic!(serde_json::json!({
                "component": "links", "kind": "session.read_failed",
                "agent": candidate.agent.as_str(), "code": code,
                "session_id": answer.facts.session_id,
            }));
        }
        let more = answer.has_more && answer.error.is_none();
        if let Err(code) = store.apply_answer(&paths.local_device, &answer, &candidate.stamp) {
            log_write(&code, "session");
            continue;
        }
        state.dirty_at.get_or_insert_with(Instant::now);
        if more {
            state.queue.push_front(candidate);
        }
    }
    if state.queue.is_empty() {
        mark_filled(store);
    }
}

fn prune(store: &mut LinkStore, paths: &Paths) {
    let policies = match store.policies(&paths.search) {
        Ok(policies) => policies,
        Err(code) => {
            // An unreadable policy is not Off: nothing is removed this pass.
            crate::diagnostic!(serde_json::json!({
                "component": "links", "kind": "policy.read_failed", "code": code,
            }));
            return;
        }
    };
    let days = |project: &str| policies.get(project).copied();
    let exists = |path: &str| Path::new(path).is_file();
    match store.prune(&days, &paths.local_device, &exists, now_ms()) {
        Ok(0) => {}
        Ok(removed) => crate::diagnostic!(serde_json::json!({
            "component": "links", "kind": "retention.pruned", "sessions": removed,
        })),
        Err(code) => log_write(&code, "retention"),
    }
}

fn publish_summaries(store: &LinkStore, state: &mut State, sink: &impl Sink) {
    let mut summaries = BTreeMap::new();
    for project in state.projects.iter() {
        match store.summary(&project.key) {
            Ok(summary) => {
                summaries.insert(project.workspace_id.clone(), summary);
            }
            Err(code) => {
                crate::diagnostic!(serde_json::json!({
                    "component": "links", "kind": "summary.failed",
                    "workspace_id": project.workspace_id, "code": code,
                }));
                if let Some(held) = state.summaries.get(&project.workspace_id) {
                    summaries.insert(project.workspace_id.clone(), held.clone());
                }
            }
        }
    }
    if summaries != state.summaries {
        state.summaries = summaries.clone();
        sink.summaries(summaries);
    }
}

fn answer_panel(store: &LinkStore, paths: &Paths, state: &mut State, sink: &impl Sink) {
    let Some(request) = state.panel.as_ref() else {
        return;
    };
    let local = Some(paths.local_device.as_str());
    let answer = match &request.target {
        PanelTarget::Pr(number) => store
            .pr_panel(&request.project, *number, local)
            .map(PanelAnswer::Pr),
        PanelTarget::Issue(key) => store
            .issue_panel(&request.project, key, local)
            .map(PanelAnswer::Issue),
    };
    if let Err(code) = &answer {
        crate::diagnostic!(serde_json::json!({
            "component": "links", "kind": "panel.read_failed", "code": code,
            "generation": request.generation,
        }));
    }
    if state.panel_answer.as_ref() != Some(&answer) {
        state.panel_answer = Some(answer.clone());
        sink.panel(request.generation, answer);
    }
}
