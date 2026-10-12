//! One owned writer, reading only the core's own node. No runtime lock is
//! held while listing, parsing, transacting, pruning or building a report.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use hide_node_link::{NodeLink, call_as, protocol::Call};
use hide_session::{
    Agent,
    usage::{Candidate, Candidates, Checkpoint, Step},
};

use super::prices::{Cache, Fetch, Table};
use super::store::{Daily, Store};

const REST: Duration = Duration::from_millis(20);
const CALL_TIMEOUT: Duration = Duration::from_secs(2);
const RETRY: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub agent: Agent,
    pub reading: bool,
    pub files_done: usize,
    pub files_total: usize,
    pub limited: bool,
    pub skipped: u64,
    pub failure: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub daily: Arc<Vec<Daily>>,
    pub progress: Vec<Progress>,
    pub failure: Option<&'static str>,
    pub prices: Option<Arc<Table>>,
    pub price_failure: Option<&'static str>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Activity {
    pub visible: bool,
    pub open: bool,
}

#[derive(Default)]
struct Mailbox {
    stop: bool,
    activity: Activity,
}

#[derive(Clone)]
pub struct Client(Arc<(Mutex<Mailbox>, Condvar)>);

impl Client {
    /// Two booleans replace prior intent; input and session-sync events do
    /// no ledger work and cannot grow a queue.
    pub fn activity(&self, activity: Activity) {
        let (lock, wake) = &*self.0;
        if let Ok(mut mailbox) = lock.lock()
            && mailbox.activity != activity
        {
            mailbox.activity = activity;
            wake.notify_one();
        }
    }

    fn wait(&self, duration: Duration) -> Option<Activity> {
        let (lock, wake) = &*self.0;
        let guard = lock.lock().ok()?;
        if guard.stop {
            return None;
        }
        let (guard, _) = wake.wait_timeout(guard, duration).ok()?;
        (!guard.stop).then_some(guard.activity)
    }
}

pub struct Worker {
    client: Client,
    join: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn spawn(
        path: PathBuf,
        device: String,
        own: Arc<dyn NodeLink>,
        mut publish: impl FnMut(Report) + Send + 'static,
    ) -> Result<Self, &'static str> {
        let client = Client(Arc::new((Mutex::new(Mailbox::default()), Condvar::new())));
        let worker_client = client.clone();
        let join = thread::Builder::new()
            .name("hide-usage-ledger".into())
            .spawn(move || {
                let mut previous = None;
                while let Some(activity) = worker_client.wait(Duration::from_millis(1)) {
                    match Engine::open(&path, device.clone()) {
                        Ok(mut engine) => {
                            let mut activity = activity;
                            let mut published = None;
                            loop {
                                if activity.visible {
                                    engine.prices_step();
                                    engine.step(own.as_ref(), activity);
                                }
                                let interval =
                                    Duration::from_secs(if activity.open { 5 } else { 30 });
                                if published.is_none_or(|at: Instant| at.elapsed() >= interval) {
                                    let report = engine.report();
                                    if previous.as_ref() != Some(&report) {
                                        publish(report.clone());
                                        previous = Some(report);
                                    }
                                    published = Some(Instant::now());
                                }
                                let Some(next) = worker_client.wait(if activity.visible {
                                    REST
                                } else {
                                    Duration::from_secs(1)
                                }) else {
                                    return;
                                };
                                activity = next;
                            }
                        }
                        Err(code) => {
                            let report = Report {
                                daily: Arc::new(Vec::new()),
                                progress: Vec::new(),
                                failure: Some(code),
                                prices: None,
                                price_failure: None,
                            };
                            if previous.as_ref() != Some(&report) {
                                diagnostic(code, None, 0);
                                publish(report.clone());
                                previous = Some(report);
                            }
                            if worker_client.wait(RETRY).is_none() {
                                return;
                            }
                        }
                    }
                }
            })
            .map_err(|_| "usage_worker_start_failed")?;
        Ok(Self {
            client,
            join: Some(join),
        })
    }

    pub fn client(&self) -> Client {
        self.client.clone()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.client.0;
        if let Ok(mut guard) = lock.lock() {
            guard.stop = true;
            wake.notify_one();
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct AgentWork {
    progress: Progress,
    queue: VecDeque<Candidate>,
    active: Option<(Candidate, Option<Checkpoint>)>,
    listed: Option<Instant>,
    failed: bool,
}

struct Engine {
    store: Store,
    device: String,
    agents: Vec<AgentWork>,
    next: usize,
    dirty: bool,
    daily: Arc<Vec<Daily>>,
    failure: Option<&'static str>,
    pruned: Instant,
    pruning: bool,
    prune_retry: Option<Instant>,
    prices: Cache,
    price_fetch: Option<Fetch>,
    price_failure: Option<&'static str>,
    price_retry: Option<Instant>,
}

impl Engine {
    fn open(path: &std::path::Path, device: String) -> Result<Self, &'static str> {
        let store = Store::open(path)?;
        let agents = Agent::supported()
            .filter(|agent| *agent != Agent::Cursor)
            .map(|agent| {
                Ok(AgentWork {
                    progress: Progress {
                        agent,
                        reading: !store.backfill_complete(&device, agent)?,
                        files_done: 0,
                        files_total: 0,
                        limited: false,
                        skipped: 0,
                        failure: None,
                    },
                    queue: VecDeque::new(),
                    active: None,
                    listed: None,
                    failed: false,
                })
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        let daily = Arc::new(store.daily()?);
        let (prices, price_failure) = match store.prices() {
            Ok(prices) => (prices, None),
            Err(code) => {
                diagnostic(code, None, 0);
                (Cache::default(), Some(code))
            }
        };
        Ok(Self {
            store,
            device,
            agents,
            next: 0,
            dirty: false,
            daily,
            failure: None,
            pruned: Instant::now(),
            pruning: true,
            prune_retry: None,
            prices,
            price_fetch: None,
            price_failure,
            price_retry: None,
        })
    }

    fn prices_step(&mut self) {
        if let Some(outcome) = self.price_fetch.as_mut().and_then(Fetch::take) {
            self.price_fetch = None;
            let outcome = outcome.and_then(|table| {
                let mut cache = self.prices.clone();
                cache.table = Some(table);
                self.store.save_prices(&cache)?;
                self.prices = cache;
                Ok(())
            });
            self.price_failure = outcome.err();
            if let Some(code) = self.price_failure {
                diagnostic(code, None, 0);
            }
        }
        let now = crate::links::now_ms();
        if self.price_fetch.is_some()
            || !self.prices.due(now)
            || self.price_retry.is_some_and(|at| at.elapsed() < RETRY)
        {
            return;
        }
        // Persist the attempt before any network activity, including failed
        // attempts, so restarts cannot retry more than once per day.
        let mut attempted = self.prices.clone();
        attempted.attempted_at = Some(now);
        match self.store.save_prices(&attempted) {
            Ok(()) => {
                self.prices = attempted;
                match Fetch::start() {
                    Ok(fetch) => self.price_fetch = Some(fetch),
                    Err(code) => {
                        self.price_failure = Some(code);
                        diagnostic(code, None, 0);
                    }
                }
            }
            Err(code) => {
                self.price_retry = Some(Instant::now());
                if self.price_failure != Some(code) {
                    diagnostic(code, None, 0);
                }
                self.price_failure = Some(code);
            }
        }
    }

    /// One node read and one atomic store step per turn, followed by a rest.
    /// Round-robin selection prevents a large agent history starving another.
    fn step(&mut self, own: &dyn NodeLink, activity: Activity) {
        let now = crate::links::now_ms();
        let zone = jiff::tz::TimeZone::system();
        if (self.pruning || self.pruned.elapsed() >= Duration::from_secs(30 * 60))
            && self.prune_retry.is_none_or(|at| at.elapsed() >= RETRY)
        {
            match self.store.prune(now, &zone) {
                Ok(more) => {
                    self.pruning = more;
                    self.pruned = Instant::now();
                    self.dirty = true;
                    self.prune_retry = None;
                }
                Err(code) => {
                    self.prune_retry = Some(Instant::now());
                    self.failed(code);
                }
            }
            return;
        }
        let index = self.next;
        self.next = (self.next + 1) % self.agents.len();
        let work = &mut self.agents[index];
        let agent = work.progress.agent;
        let interval = Duration::from_secs(if activity.open { 3 } else { 30 });
        if work.active.is_none() && work.queue.is_empty() {
            if work
                .listed
                .is_some_and(|at| at.elapsed() < if work.failed { RETRY } else { interval })
            {
                return;
            }
            work.listed = Some(Instant::now());
            let listed: Result<Candidates, _> = call_as(
                own,
                Call::UsageFiles {
                    agent,
                    since_unix_ms: 0,
                },
                CALL_TIMEOUT,
            );
            match listed {
                Ok(listed) if listed.files.len() <= hide_session::usage::CANDIDATE_LIMIT => {
                    if !listed.capped
                        && listed.skipped == 0
                        && let Err(code) =
                            self.store
                                .retire_cursors(&self.device, agent, &listed.files)
                    {
                        work.fail(code);
                        return;
                    }
                    work.progress.files_done = 0;
                    work.progress.files_total = listed.files.len();
                    work.progress.limited = listed.capped;
                    work.progress.skipped = listed.skipped;
                    work.progress.failure = None;
                    work.failed = listed.capped || listed.skipped > 0;
                    if listed.capped {
                        diagnostic(
                            "usage_candidate_capacity",
                            Some(agent),
                            listed.files.len() as u64,
                        );
                    }
                    if listed.skipped > 0 {
                        diagnostic("usage_discovery_skipped", Some(agent), listed.skipped);
                    }
                    work.queue = listed.files.into();
                }
                _ => {
                    work.fail("usage_list_failed");
                    return;
                }
            }
            if work.queue.is_empty() {
                self.finish(index, now, &zone);
            }
            return;
        }
        if work.active.is_none() {
            let Some(candidate) = work.queue.pop_front() else {
                return;
            };
            let Some(path) = candidate.path.to_str() else {
                work.fail("usage_path_invalid");
                return;
            };
            match self.store.checkpoint(&self.device, agent, path) {
                Ok(saved) => {
                    if agent != Agent::OpenCode
                        && saved.as_ref().is_some_and(|(stamp, checkpoint)| {
                            stamp == &candidate && checkpoint.is_current()
                        })
                    {
                        work.progress.files_done += 1;
                        if work.queue.is_empty() {
                            self.finish(index, now, &zone);
                        }
                        return;
                    }
                    work.active = Some((candidate, saved.map(|(_, checkpoint)| checkpoint)));
                }
                Err(code) => {
                    work.fail(code);
                    return;
                }
            }
        }
        let (candidate, checkpoint) = work.active.as_ref().unwrap();
        let step: Result<Step, _> = call_as(
            own,
            Call::UsageRead {
                agent,
                path: candidate.path.to_string_lossy().into_owned(),
                checkpoint: checkpoint.clone(),
            },
            CALL_TIMEOUT,
        );
        match step {
            Ok(step) => match self
                .store
                .ingest(&self.device, agent, candidate, &step, now, &zone)
            {
                Ok(ingested) => {
                    self.dirty |= ingested.added > 0;
                    work.progress.limited |= ingested.limited > 0;
                    let skipped = step
                        .skipped
                        .values()
                        .copied()
                        .sum::<u64>()
                        .saturating_add(ingested.changed as u64)
                        .saturating_add(ingested.expired as u64);
                    work.progress.skipped = work.progress.skipped.saturating_add(skipped);
                    if skipped > 0 {
                        diagnostic("usage_records_skipped", Some(agent), skipped);
                    }
                    if ingested.limited > 0 {
                        diagnostic("usage_event_capacity", Some(agent), ingested.limited as u64);
                    }
                    if step.has_more {
                        work.active.as_mut().unwrap().1 = Some(step.checkpoint);
                    } else {
                        work.active = None;
                        work.progress.files_done += 1;
                        if work.queue.is_empty() {
                            self.finish(index, now, &zone);
                        }
                    }
                }
                Err(code) => {
                    work.active = None;
                    work.fail(code);
                }
            },
            Err(_) => {
                work.active = None;
                work.fail("usage_read_failed");
            }
        }
    }

    fn finish(&mut self, index: usize, now: u64, zone: &jiff::tz::TimeZone) {
        let work = &mut self.agents[index];
        if work.progress.reading && !work.failed {
            match self
                .store
                .complete_backfill(&self.device, work.progress.agent, now, zone)
            {
                Ok(()) => {
                    work.progress.reading = false;
                    self.pruning = true;
                }
                Err(code) => work.fail(code),
            }
        }
    }

    fn failed(&mut self, code: &'static str) {
        if self.failure != Some(code) {
            diagnostic(code, None, 0);
        }
        self.failure = Some(code);
    }

    fn report(&mut self) -> Report {
        if self.dirty {
            match self.store.daily() {
                Ok(daily) => {
                    self.daily = Arc::new(daily);
                    self.dirty = false;
                    self.failure = None;
                }
                Err(code) => self.failed(code),
            }
        }
        Report {
            daily: self.daily.clone(),
            progress: self
                .agents
                .iter()
                .map(|work| work.progress.clone())
                .collect(),
            failure: self.failure,
            prices: self.prices.table.clone().map(Arc::new),
            price_failure: self.price_failure,
        }
    }
}

impl AgentWork {
    fn fail(&mut self, code: &'static str) {
        if self.progress.failure != Some(code) {
            diagnostic(code, Some(self.progress.agent), 0);
        }
        self.progress.failure = Some(code);
        self.failed = true;
    }
}

fn diagnostic(code: &'static str, agent: Option<Agent>, count: u64) {
    crate::diagnostic!(
        serde_json::json!({ "component": "usage_ledger", "kind": code, "agent": agent.map(|agent| agent.as_str()), "count": count })
    );
}

#[cfg(test)]
mod tests;
