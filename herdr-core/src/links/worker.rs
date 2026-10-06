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
use crate::host_access::{HostCallError, HostChannel, call_as};
use hide_host::protocol::Call;
use hide_session::links::{self, Candidate, ReadRequest};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How often the session folders are listed for changed files.
const LIST_EVERY: Duration = Duration::from_secs(15);
/// How often the panes' spans are extended while they stay.
const PANES_EVERY: Duration = Duration::from_secs(60);
/// How often each connected device is asked for its changed files (D-21).
const DEVICE_EVERY: Duration = Duration::from_secs(60);
/// How long one device call may take before the next turn.
const DEVICE_TIMEOUT: Duration = Duration::from_secs(10);
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
    /// The devices whose helper is connected now (D-21).
    fn devices(&self) -> Vec<(String, Arc<dyn HostChannel>)>;
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
    /// Per device: its files waiting to be read, and when it was last listed.
    devices: HashMap<String, DeviceQueue>,
    /// When the connected devices were last asked for.
    devices_at: Option<Instant>,
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
        devices: HashMap::new(),
        devices_at: None,
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
        }
        device_turn(store, sink, &mut state);
        let filling = !state.queue.is_empty()
            || state
                .devices
                .values()
                .any(|device| !device.queue.is_empty());
        if filling != state.filling {
            state.filling = filling;
            sink.filling(filling);
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
    if !state.queue.is_empty()
        || state
            .devices
            .values()
            .any(|device| !device.queue.is_empty())
    {
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

#[derive(Default)]
struct DeviceQueue {
    queue: VecDeque<Candidate>,
    listed: Option<Instant>,
}

/// One step for each connected device: list its changed files every
/// [`DEVICE_EVERY`], then read up to a few of them per turn through its
/// helper. A device that is gone keeps every row it gave; when it returns,
/// its listing starts where the last one did, so what changed meanwhile is
/// read then (B40).
fn device_turn(store: &mut LinkStore, sink: &impl Sink, state: &mut State) {
    let reading = state
        .devices
        .values()
        .any(|device| !device.queue.is_empty());
    if !reading
        && state
            .devices_at
            .is_some_and(|at| at.elapsed() < DEVICE_EVERY)
    {
        return;
    }
    state.devices_at = Some(Instant::now());
    let devices = sink.devices();
    state
        .devices
        .retain(|id, _| devices.iter().any(|(device, _)| device == id));
    for (device, channel) in devices {
        let entry = state.devices.entry(device.clone()).or_default();
        if entry.queue.is_empty() && entry.listed.is_none_or(|at| at.elapsed() >= DEVICE_EVERY) {
            entry.listed = Some(Instant::now());
            if let Err(code) = list_device(store, &device, channel.as_ref(), entry) {
                log_device(&device, "listing", &code);
            }
        }
        if entry.queue.is_empty() {
            continue;
        }
        let batch = entry
            .queue
            .drain(..entry.queue.len().min(links::READ_FILE_LIMIT))
            .collect::<Vec<_>>();
        let mut requests = Vec::with_capacity(batch.len());
        for candidate in &batch {
            match store.checkpoint(&device, &candidate.path) {
                Ok(checkpoint) => requests.push(ReadRequest {
                    agent: candidate.agent,
                    path: candidate.path.clone(),
                    checkpoint,
                }),
                Err(code) => log_write(&code, "cursor"),
            }
        }
        let answers = match call_as::<Vec<links::ReadAnswer>>(
            channel.as_ref(),
            Call::LinkRead { requests },
            DEVICE_TIMEOUT,
        ) {
            Ok(answers) => answers,
            Err(error) => {
                // The files wait for the device's next turn.
                for candidate in batch.into_iter().rev() {
                    entry.queue.push_front(candidate);
                }
                log_device(&device, "read", &device_code(&error));
                continue;
            }
        };
        for candidate in batch {
            let Some(answer) = answers.iter().find(|answer| answer.path == candidate.path) else {
                continue;
            };
            if let Some(code) = answer.error.as_deref() {
                log_device(&device, "session", code);
            }
            if let Err(code) = store.apply_answer(&device, answer, &candidate.stamp) {
                log_write(&code, "session");
                continue;
            }
            state.dirty_at.get_or_insert_with(Instant::now);
            if answer.has_more && answer.error.is_none() {
                entry.queue.push_back(candidate);
            }
        }
    }
}

fn list_device(
    store: &LinkStore,
    device: &str,
    channel: &dyn HostChannel,
    entry: &mut DeviceQueue,
) -> Result<(), String> {
    let key = format!("{LISTED_AT}:{device}");
    let started = now_ms();
    let since = match store.meta(&key)? {
        Some(at) => at
            .parse::<u64>()
            .unwrap_or(0)
            .saturating_sub(LIST_OVERLAP_MS),
        None => started.saturating_sub(BACKFILL_MS),
    };
    let candidates = call_as::<Vec<Candidate>>(
        channel,
        Call::LinkFiles {
            since_unix_ms: since,
        },
        DEVICE_TIMEOUT,
    )
    .map_err(|error| device_code(&error))?;
    let stamps = store.stamps(device)?;
    entry.queue = candidates
        .into_iter()
        .rev()
        .filter(|candidate| stamps.get(&candidate.path) != Some(&candidate.stamp))
        .collect();
    store.set_meta(&key, &started.to_string())
}

fn device_code(error: &HostCallError) -> String {
    match error {
        HostCallError::NotConnected(_) => "device_helper_not_connected".to_owned(),
        HostCallError::Busy => "device_helper_busy".to_owned(),
        HostCallError::Unknown(_) => "device_helper_unknown".to_owned(),
        // A helper older than protocol 18 does not know the call.
        HostCallError::Refused(error) if error.code == hide_host::ErrorCode::InvalidRequest => {
            "device_helper_unsupported".to_owned()
        }
        HostCallError::Refused(error) => error.message.clone(),
    }
}

fn log_device(device: &str, what: &str, code: &str) {
    crate::diagnostic!(serde_json::json!({
        "component": "links", "kind": "device.read_failed", "device_id": device,
        "what": what, "code": code,
    }));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_access::HostAnswer;
    use crate::links::{FileState, PrFact, ProjectFacts, SessionRole, WorktreeFact};
    use std::sync::atomic::{AtomicBool, Ordering};

    const CREATED: u64 = 1_790_000_000_000;

    /// A connected device whose helper answers the two link calls from its
    /// own home, the way `hide-host-helper serve` does.
    struct Device {
        home: PathBuf,
        connected: AtomicBool,
    }

    impl HostChannel for Device {
        fn call(&self, call: Call, _timeout: Duration) -> Result<HostAnswer, HostCallError> {
            if !self.connected.load(Ordering::SeqCst) {
                return Err(HostCallError::NotConnected("gone".into()));
            }
            let value = match call {
                Call::LinkFiles { since_unix_ms } => {
                    serde_json::to_value(links::candidates(&self.home, since_unix_ms).unwrap())
                }
                Call::LinkRead { requests } => {
                    serde_json::to_value(links::read(&self.home, &requests))
                }
                other => panic!("unexpected call {other:?}"),
            };
            Ok(HostAnswer::Parsed(value.unwrap()))
        }
    }

    #[derive(Default)]
    struct Seen {
        panel: Option<Result<PanelAnswer, String>>,
        summaries: BTreeMap<String, ProjectLinkSummary>,
    }

    struct TestSink {
        seen: Arc<Mutex<Seen>>,
        device: Arc<Device>,
    }

    impl Sink for TestSink {
        fn summaries(&self, summaries: BTreeMap<String, ProjectLinkSummary>) {
            self.seen.lock().unwrap().summaries = summaries;
        }
        fn panel(&self, _generation: u64, answer: Result<PanelAnswer, String>) {
            self.seen.lock().unwrap().panel = Some(answer);
        }
        fn filling(&self, _filling: bool) {}
        fn devices(&self) -> Vec<(String, Arc<dyn HostChannel>)> {
            vec![(
                "mini".to_owned(),
                self.device.clone() as Arc<dyn HostChannel>,
            )]
        }
    }

    fn iso(ms: u64) -> String {
        jiff::Timestamp::from_millisecond(ms as i64)
            .unwrap()
            .to_string()
    }

    /// The device's session that made PR 7 from its own checkout.
    fn device_session(home: &Path) {
        let dir = home.join(".claude/projects/-mini-app");
        std::fs::create_dir_all(&dir).unwrap();
        let lines = [
            serde_json::json!({
                "type": "user", "isSidechain": false, "uuid": "u1", "parentUuid": null,
                "message": {"role": "user", "content": "mini에서 PR 올려 줘"},
                "timestamp": iso(CREATED - 60_000), "promptId": "p",
                "origin": {"kind": "human"}, "userType": "external", "entrypoint": "cli",
                "cwd": "/mini/app", "sessionId": "s-mini", "gitBranch": "feat",
            }),
            serde_json::json!({
                "type": "pr-link", "sessionId": "s-mini", "prNumber": 7,
                "prRepository": "acme/app", "prUrl": "https://github.com/acme/app/pull/7",
                "timestamp": iso(CREATED + 1_000),
            }),
        ];
        let text = lines.map(|line| line.to_string()).join("\n") + "\n";
        std::fs::write(dir.join("s-mini.jsonl"), text).unwrap();
    }

    fn wait_until(what: &str, ready: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            #[allow(clippy::disallowed_methods)] // a polling helper bounded by a deadline
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// B20, B40: a connected device's session reaches a Mac project's panel
    /// with its device, its rows stay while the device is gone, and what it
    /// adds meanwhile is read when it is back.
    #[test]
    fn a_devices_session_joins_the_panel_and_outlives_the_connection() {
        let state = tempfile::tempdir().unwrap();
        let device_home = tempfile::tempdir().unwrap();
        device_session(device_home.path());
        let device = Arc::new(Device {
            home: device_home.path().to_path_buf(),
            connected: AtomicBool::new(true),
        });
        let seen = Arc::new(Mutex::new(Seen::default()));
        let worker = LinkWorker::spawn(
            Paths {
                store: state.path().join("links.sqlite3"),
                search: state.path().join("session-search.sqlite3"),
                home: None,
                local_device: "local".into(),
            },
            TestSink {
                seen: Arc::clone(&seen),
                device: Arc::clone(&device),
            },
        )
        .unwrap();
        let client = worker.client();
        client.projects(Arc::new(vec![ProjectFacts {
            key: "app".into(),
            device_id: "local".into(),
            workspace_id: "w".into(),
            root: "/work/app".into(),
            repository: Some("acme/app".into()),
            repository_id: None,
            worktrees: vec![WorktreeFact {
                path: "/work/app".into(),
                branch: Some("main".into()),
            }],
            prs: vec![PrFact {
                repository: "acme/app".into(),
                number: 7,
                branch: "feat".into(),
                title: "PR 7".into(),
                url: "https://github.com/acme/app/pull/7".into(),
                created_at: Some(CREATED),
                closed_at: None,
                merged_at: None,
                issues: Vec::new(),
                hide_issue_known: false,
            }],
            prs_read: true,
        }]));
        client.read(PanelRequest {
            generation: 1,
            project: "app".into(),
            target: PanelTarget::Pr(7),
        });
        let lines = || match &seen.lock().unwrap().panel {
            Some(Ok(PanelAnswer::Pr(Some(links)))) => links.sessions.clone(),
            _ => Vec::new(),
        };
        wait_until("the device's line", || !lines().is_empty());
        let line = &lines()[0];
        assert_eq!(
            (line.device_id.as_str(), line.role),
            ("mini", SessionRole::Created)
        );
        assert_eq!(line.request.as_deref(), Some("mini에서 PR 올려 줘"));
        // This machine cannot look at a device's file.
        assert_eq!(line.file, FileState::Unknown);
        wait_until("the chip", || {
            seen.lock()
                .unwrap()
                .summaries
                .get("w")
                .is_some_and(|summary| summary.sessions.contains_key("s-mini"))
        });

        // Gone: nothing is read and nothing leaves (B40).
        device.connected.store(false, Ordering::SeqCst);
        client.read(PanelRequest {
            generation: 2,
            project: "app".into(),
            target: PanelTarget::Pr(7),
        });
        wait_until("the reread", || {
            matches!(
                &seen.lock().unwrap().panel,
                Some(Ok(PanelAnswer::Pr(Some(_))))
            )
        });
        assert_eq!(lines().len(), 1);
        drop(worker);
    }
}
