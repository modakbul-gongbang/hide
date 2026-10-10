//! Puts a pane's hook tokens back on a Herdr that lost them.
//!
//! The hook helper reports a pane's `hide_hooks`, `hide_sub_working` and
//! `hide_sub_done` tokens after each of its events and keeps what it
//! reported in the pane's file under `~/.hide/agent-hooks/panes/`
//! (`hide_agent_hooks::counters`). Herdr keeps a token only as long as the
//! server that took it: a live handoff or a restart starts the new server
//! with none, while the pane ids stay (measured on the pinned Herdr, and
//! the reason a file can be paired with a pane at all). Until the agent's
//! next event the pane then reads as a session that predates Hide's setup,
//! with a Reopen that restarts a healthy session (issue 799).
//!
//! The rule is the lineage writer's: the agent list the coordinator already
//! reads each second is the observation, and a pane that lacks its token
//! while its file has one is written, not a pane that changed. It differs in
//! what makes a pane worth asking about again. A lineage patch is a pure
//! function of the ledger, so it can be compared with the list on every
//! read. A file is read from disk, so each pane is asked once for each
//! connection to Herdr (every connect is a bootstrap, and a handoff or a
//! restart ends the connection); a pane with no file, or one an older helper
//! wrote, is not asked again, because the helper's own next event reports
//! the token and writes the file in the same step.
//!
//! The file is read and the report is sent on a worker thread: the
//! coordinator never waits on either (`docs/ARCHITECTURE.md`, the
//! session-sync thread never blocks). One batch is in flight and one waits,
//! at most [`BATCH_LIMIT`] panes each; panes beyond that stay unasked and are
//! taken by the next observation. A failure is retried at most [`ATTEMPTS`]
//! times for a connection and written to the diagnostic log with the pane.

use crate::agent_hooks::PaneHookTokens;
use crate::session_sync::ProjectedAgent;
use hide_agent_hooks::counters::{self, Restore};
use hide_herdr_client::{ApiConnector, request_with_connector};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Panes in one batch. A device with more agents than this is taken in
/// several observations, a second apart.
const BATCH_LIMIT: usize = 256;
/// How many times a pane whose restore failed is asked in one connection.
const ATTEMPTS: u8 = 3;
/// One report holds the worker this long at most.
const REPORT_TIMEOUT: Duration = Duration::from_secs(1);
/// How many times a report is sent again because the pane's file changed
/// while it was on its way.
const RESEND_LIMIT: usize = 3;
/// Completions that can be waiting for the coordinator: one batch in
/// flight, one queued.
const COMPLETION_CAPACITY: usize = BATCH_LIMIT * 2;

#[derive(Debug)]
struct Batch {
    generation: u64,
    panes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Outcome {
    /// The tokens are back on the pane.
    Restored,
    /// The helper never counted this pane here.
    NoRecord,
    /// An older helper wrote the file, so its hook version is not known.
    Unversioned,
    Failed(String),
}

#[derive(Debug)]
struct Completion {
    pane: String,
    generation: u64,
    outcome: Outcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Standing {
    /// Asked of the worker in this batch, on this attempt.
    InFlight { generation: u64, attempts: u8 },
    /// Answered for this connection: put back, or nothing to put back.
    Settled,
    /// The worker could not put it back this many times.
    Failed { attempts: u8 },
}

pub(crate) struct Restorer {
    sender: Option<mpsc::SyncSender<Batch>>,
    worker: Option<JoinHandle<()>>,
    completed: mpsc::Receiver<Completion>,
    stopping: Arc<AtomicBool>,
    generation: u64,
    /// Where each pane stands in this connection. Panes Herdr no longer lists
    /// are dropped on every observation, so it holds at most the agent list.
    standing: HashMap<String, Standing>,
    /// Whether this connection has already said it had more panes than a
    /// batch holds.
    deferral_reported: bool,
}

impl Restorer {
    /// `home` is the account whose hook files these are: this machine's, so
    /// the coordinator of a device (whose files are the device's) makes none.
    pub(crate) fn new(home: PathBuf, connector: Arc<dyn ApiConnector>) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel::<Batch>(1);
        let (done, completed) = mpsc::sync_channel(COMPLETION_CAPACITY);
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&stopping);
        let worker = thread::Builder::new()
            .name("hide-hook-tokens".into())
            .spawn(move || {
                while let Ok(batch) = receiver.recv() {
                    let mut tally = Tally::default();
                    for pane in batch.panes {
                        if stop.load(Ordering::Acquire) {
                            return;
                        }
                        let outcome = restore(&home, connector.as_ref(), &pane);
                        tally.add(&pane, &outcome);
                        // The channel holds every outstanding completion
                        // (`COMPLETION_CAPACITY`), so this cannot fail while
                        // the coordinator lives.
                        let _ = done.try_send(Completion {
                            pane,
                            generation: batch.generation,
                            outcome,
                        });
                    }
                    tally.report(batch.generation);
                }
            })
            .map_err(|error| format!("the hook token worker did not start: {error}"))?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
            completed,
            stopping,
            generation: 0,
            standing: HashMap::new(),
            deferral_reported: false,
        })
    }

    /// Looks at one read of the agent list. `reconnected` is true for the
    /// list read right after a connect: Herdr may be another server than the
    /// one the panes were last asked of, so every pane is asked again.
    pub(crate) fn observe(&mut self, agents: &[ProjectedAgent], reconnected: bool) {
        if reconnected {
            self.standing.clear();
            self.deferral_reported = false;
        }
        while let Ok(done) = self.completed.try_recv() {
            self.settle(done);
        }
        let listed: HashSet<&str> = agents.iter().map(|agent| agent.pane_id.as_str()).collect();
        self.standing
            .retain(|pane, _| listed.contains(pane.as_str()));
        let mut wanted = Vec::new();
        let mut attempts = Vec::new();
        for agent in agents.iter().filter(|agent| lacks_hook_token(agent)) {
            match self.standing.get(&agent.pane_id) {
                None => attempts.push(0),
                Some(Standing::Failed { attempts: made }) if *made < ATTEMPTS => {
                    attempts.push(*made)
                }
                Some(_) => continue,
            }
            wanted.push(agent.pane_id.clone());
        }
        if wanted.is_empty() {
            return;
        }
        if wanted.len() > BATCH_LIMIT && !self.deferral_reported {
            self.deferral_reported = true;
            crate::diagnostic!(json!({
                "component": "hook_tokens",
                "kind": "restore.deferred",
                "panes": wanted.len(),
                "batch_limit": BATCH_LIMIT,
            }));
        }
        wanted.truncate(BATCH_LIMIT);
        let Some(generation) = self.generation.checked_add(1) else {
            return;
        };
        let Some(sender) = self.sender.as_ref() else {
            return;
        };
        let batch = Batch {
            generation,
            panes: wanted.clone(),
        };
        match sender.try_send(batch) {
            Ok(()) => {
                self.generation = generation;
                for (pane, made) in wanted.into_iter().zip(attempts) {
                    self.standing.insert(
                        pane,
                        Standing::InFlight {
                            generation,
                            attempts: made,
                        },
                    );
                }
            }
            // The worker is still on the last batch: these panes are asked
            // for again by the next observation.
            Err(mpsc::TrySendError::Full(_)) => {}
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.sender = None;
                crate::diagnostic!(json!({
                    "component": "hook_tokens",
                    "kind": "restore.worker_ended",
                }));
            }
        }
    }

    fn settle(&mut self, done: Completion) {
        let Some(Standing::InFlight {
            generation,
            attempts,
        }) = self.standing.get(&done.pane).copied()
        else {
            return;
        };
        // A batch asked before the last connect describes another server.
        if generation != done.generation {
            return;
        }
        let standing = match done.outcome {
            Outcome::Restored | Outcome::NoRecord | Outcome::Unversioned => Standing::Settled,
            Outcome::Failed(_) => {
                let attempts = attempts.saturating_add(1);
                if attempts >= ATTEMPTS {
                    crate::diagnostic!(json!({
                        "component": "hook_tokens",
                        "kind": "restore.gave_up",
                        "pane_id": done.pane,
                        "attempts": attempts,
                    }));
                }
                Standing::Failed { attempts }
            }
        };
        self.standing.insert(done.pane, standing);
    }
}

impl Drop for Restorer {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// An agent Herdr lists that carries no hook version: the state a handoff
/// leaves every instrumented pane in, and the state of a session the hook
/// never reached.
fn lacks_hook_token(agent: &ProjectedAgent) -> bool {
    agent.agent.is_some() && PaneHookTokens::read(&agent.tokens).version.is_none()
}

/// Puts one pane's tokens back from its file.
fn restore(home: &std::path::Path, connector: &dyn ApiConnector, pane: &str) -> Outcome {
    let mut sent = None;
    for _ in 0..=RESEND_LIMIT {
        let found = match counters::restore_of(home, pane) {
            Ok(Restore::NoRecord) => return finish(sent, Outcome::NoRecord),
            Ok(Restore::Unversioned) => return finish(sent, Outcome::Unversioned),
            Ok(Restore::Report { counters, version }) => (counters, version),
            Err(error) => return Outcome::Failed(format!("the pane's file: {error}")),
        };
        // The same report the helper sends, so the tokens are the helper's.
        // A file another event changed while this was on its way is sent
        // again, the way the helper does, so Herdr ends on the latest.
        if sent == Some(found) {
            return Outcome::Restored;
        }
        if let Err(error) = request_with_connector(
            connector,
            "pane.report_metadata",
            hide_agent_hooks::report::report_params(pane, found.1, found.0),
            REPORT_TIMEOUT,
        ) {
            return Outcome::Failed(format!("pane.report_metadata: {error}"));
        }
        sent = Some(found);
    }
    Outcome::Restored
}

fn finish(sent: Option<(counters::PaneCounters, u32)>, unreadable: Outcome) -> Outcome {
    // A file that was there when it was sent and is gone now was swept with
    // its pane; what was sent is the pane's last word.
    if sent.is_some() {
        Outcome::Restored
    } else {
        unreadable
    }
}

#[derive(Default)]
struct Tally {
    restored: usize,
    no_record: usize,
    unversioned: usize,
    failed: usize,
}

impl Tally {
    fn add(&mut self, pane: &str, outcome: &Outcome) {
        match outcome {
            Outcome::Restored => self.restored += 1,
            Outcome::NoRecord => self.no_record += 1,
            Outcome::Unversioned => self.unversioned += 1,
            Outcome::Failed(message) => {
                self.failed += 1;
                crate::diagnostic!(json!({
                    "component": "hook_tokens",
                    "kind": "restore.failed",
                    "pane_id": pane,
                    "message": message,
                }));
            }
        }
    }

    fn report(&self, generation: u64) {
        crate::diagnostic!(json!({
            "component": "hook_tokens",
            "kind": "restore.batch",
            "generation": generation,
            "restored": self.restored,
            "no_record": self.no_record,
            "unversioned": self.unversioned,
            "failed": self.failed,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_herdr::FakeHerdr;
    use hide_agent_hooks::HOOK_VERSION;
    use hide_agent_hooks::counters::{Change, PaneCounters, change};
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::time::Instant;

    fn agent(pane: &str, tokens: &[(&str, &str)]) -> ProjectedAgent {
        ProjectedAgent {
            pane_id: pane.into(),
            name: None,
            workspace_id: "w1".into(),
            tab_id: "w1:t1".into(),
            cwd: None,
            agent: Some("claude".into()),
            agent_status: Some("idle".into()),
            agent_session: None,
            spawned_from_pane_id: None,
            spawned_from_machine_id: None,
            declared_parent_session: None,
            lineage_session: None,
            state_change_seq: 1,
            tokens: tokens
                .iter()
                .map(|(key, value)| ((*key).to_owned(), json!(value)))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    fn herdr() -> FakeHerdr {
        FakeHerdr::start("hook-tokens", |method, _| match method {
            "pane.report_metadata" => json!({"type": "ok"}),
            other => panic!("unexpected {other}"),
        })
    }

    fn restorer(home: &tempfile::TempDir, herdr: &FakeHerdr) -> Restorer {
        Restorer::new(home.path().to_path_buf(), Arc::new(herdr.connector())).unwrap()
    }

    /// Two subagents started and one stopped in `pane`: one working, one done.
    fn count(home: &tempfile::TempDir, pane: &str) {
        for step in [Change::Started, Change::Started, Change::Stopped] {
            change(home.path(), pane, step).unwrap();
        }
    }

    fn reports(herdr: &FakeHerdr) -> Vec<Value> {
        herdr
            .calls()
            .into_iter()
            .inspect(|(method, _)| assert_eq!(method, "pane.report_metadata"))
            .map(|(_, params)| params)
            .collect()
    }

    /// Asks again until no pane is out with the worker, so what the worker
    /// did is on the fake and in `standing` when this returns.
    #[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
    fn until_answered(restorer: &mut Restorer, agents: &[ProjectedAgent]) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            restorer.observe(agents, false);
            if !restorer
                .standing
                .values()
                .any(|standing| matches!(standing, Standing::InFlight { .. }))
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the worker never answered every pane"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_pane_that_lost_its_tokens_gets_what_its_file_last_said() {
        let home = tempfile::tempdir().unwrap();
        let herdr = herdr();
        count(&home, "w1:p1");
        let agents = [agent("w1:p1", &[])];
        let mut restorer = restorer(&home, &herdr);
        restorer.observe(&agents, true);
        until_answered(&mut restorer, &agents);
        assert_eq!(
            reports(&herdr),
            [hide_agent_hooks::report::report_params(
                "w1:p1",
                HOOK_VERSION,
                PaneCounters {
                    working: 1,
                    done: 1
                }
            )]
        );
    }

    #[test]
    fn a_pane_that_still_has_its_version_is_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let herdr = herdr();
        count(&home, "w1:p1");
        count(&home, "w1:p2");
        let version = HOOK_VERSION.to_string();
        let agents = [
            agent("w1:p1", &[("hide_hooks", &version)]),
            agent("w1:p2", &[]),
        ];
        let mut restorer = restorer(&home, &herdr);
        restorer.observe(&agents, true);
        until_answered(&mut restorer, &agents);
        let sent = reports(&herdr);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["pane_id"], "w1:p2");
    }

    #[test]
    fn a_pane_with_no_agent_is_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let herdr = herdr();
        count(&home, "w1:p1");
        count(&home, "w1:p2");
        let mut bare = agent("w1:p1", &[]);
        bare.agent = None;
        let agents = [bare, agent("w1:p2", &[])];
        let mut restorer = restorer(&home, &herdr);
        restorer.observe(&agents, true);
        until_answered(&mut restorer, &agents);
        let sent = reports(&herdr);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["pane_id"], "w1:p2");
    }

    #[test]
    fn a_pane_with_no_file_or_a_file_without_a_version_is_not_guessed_at() {
        let home = tempfile::tempdir().unwrap();
        let herdr = herdr();
        // `w1:p2`'s file is one an older helper wrote: counts, no version.
        change(home.path(), "w1:p2", Change::Started).unwrap();
        let file = hide_agent_hooks::counters::state_directory(home.path()).join("w1_p2.json");
        std::fs::write(&file, br#"{"working":1,"done":0}"#).unwrap();
        count(&home, "w1:p3");
        let agents = [
            agent("w1:p1", &[]),
            agent("w1:p2", &[]),
            agent("w1:p3", &[]),
        ];
        let mut restorer = restorer(&home, &herdr);
        restorer.observe(&agents, true);
        until_answered(&mut restorer, &agents);
        let sent = reports(&herdr);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["pane_id"], "w1:p3");
        // Neither is asked again while the connection holds: the helper's
        // next event reports the token and writes the file in one step.
        restorer.observe(&agents, false);
        until_answered(&mut restorer, &agents);
        assert_eq!(reports(&herdr).len(), 1);
    }

    #[test]
    fn a_settled_pane_is_asked_again_only_after_the_connection_is_new() {
        let home = tempfile::tempdir().unwrap();
        let herdr = herdr();
        count(&home, "w1:p1");
        let agents = [agent("w1:p1", &[])];
        let mut restorer = restorer(&home, &herdr);
        restorer.observe(&agents, true);
        until_answered(&mut restorer, &agents);
        // The list still lacks the token: the read began before the report
        // landed, or the pane's agent has not reported since. Not a loss.
        restorer.observe(&agents, false);
        until_answered(&mut restorer, &agents);
        assert_eq!(reports(&herdr).len(), 1);
        // A new connection may be another server: ask again.
        restorer.observe(&agents, true);
        until_answered(&mut restorer, &agents);
        assert_eq!(reports(&herdr).len(), 2);
    }

    #[test]
    fn a_file_the_hook_changed_while_the_report_was_on_its_way_is_sent_again() {
        let home = tempfile::tempdir().unwrap();
        count(&home, "w1:p1");
        let events = home.path().to_path_buf();
        let mut answered = 0;
        let herdr = FakeHerdr::start("hook-tokens-race", move |_, _| {
            answered += 1;
            if answered == 1 {
                // The hook's next event lands before Herdr answers.
                change(&events, "w1:p1", Change::Started).unwrap();
            }
            json!({"type": "ok"})
        });
        let agents = [agent("w1:p1", &[])];
        let mut restorer = restorer(&home, &herdr);
        restorer.observe(&agents, true);
        until_answered(&mut restorer, &agents);
        let sent = reports(&herdr);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0]["tokens"]["hide_sub_working"], "1");
        assert_eq!(sent[1]["tokens"]["hide_sub_working"], "2");
        assert_eq!(sent[1]["tokens"]["hide_sub_done"], "1");
    }

    #[test]
    fn a_refused_report_is_asked_again_a_few_times_and_then_left() {
        let home = tempfile::tempdir().unwrap();
        count(&home, "w1:p1");
        let herdr = FakeHerdr::start_with_errors("hook-tokens-refused", |_, _| {
            Err(("pane_not_found".into(), "no such pane".into()))
        });
        let agents = [agent("w1:p1", &[])];
        let mut restorer = restorer(&home, &herdr);
        restorer.observe(&agents, true);
        until_answered(&mut restorer, &agents);
        for _ in 0..4 {
            until_answered(&mut restorer, &agents);
        }
        assert_eq!(herdr.methods().len(), usize::from(ATTEMPTS));
        assert_eq!(
            restorer.standing.get("w1:p1"),
            Some(&Standing::Failed { attempts: ATTEMPTS })
        );
        // The next connection starts the count over.
        restorer.observe(&agents, true);
        until_answered(&mut restorer, &agents);
        for _ in 0..4 {
            until_answered(&mut restorer, &agents);
        }
        assert_eq!(herdr.methods().len(), usize::from(ATTEMPTS) * 2);
    }

    #[test]
    fn more_panes_than_a_batch_holds_are_taken_by_the_next_observations() {
        let home = tempfile::tempdir().unwrap();
        let herdr = herdr();
        let panes = BATCH_LIMIT + 5;
        let agents: Vec<_> = (0..panes)
            .map(|index| {
                let pane = format!("w1:p{index}");
                change(home.path(), &pane, Change::Started).unwrap();
                agent(&pane, &[])
            })
            .collect();
        let mut restorer = restorer(&home, &herdr);
        restorer.observe(&agents, true);
        until_answered(&mut restorer, &agents);
        until_answered(&mut restorer, &agents);
        let mut restored: Vec<_> = reports(&herdr)
            .iter()
            .map(|params| params["pane_id"].as_str().unwrap().to_owned())
            .collect();
        restored.sort();
        restored.dedup();
        assert_eq!(restored.len(), panes);
    }

    #[test]
    fn a_pane_herdr_stopped_listing_is_forgotten() {
        let home = tempfile::tempdir().unwrap();
        let herdr = herdr();
        count(&home, "w1:p1");
        let agents = [agent("w1:p1", &[])];
        let mut restorer = restorer(&home, &herdr);
        restorer.observe(&agents, true);
        until_answered(&mut restorer, &agents);
        assert_eq!(restorer.standing.len(), 1);
        restorer.observe(&[], false);
        assert!(restorer.standing.is_empty());
    }
}
