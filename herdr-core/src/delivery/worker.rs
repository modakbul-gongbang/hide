//! One bounded writer. Runtime owns the published immutable ledger; owned
//! candidates are validated and persisted before a result or publication.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::handle::ChangeNotifier;
use crate::runtime::Runtime;
use crate::runtime::delivery::Observation;

use super::ledger::{self, Ledger};
use super::{Actor, Command, mailbox, watch};

const QUEUE_LIMIT: usize = 64;
const SAVE_BATCH: usize = 32;

#[derive(Clone)]
pub(crate) struct Client {
    requests: SyncSender<Request>,
}

pub struct Prepared {
    client: Client,
    actor: Actor,
    target: Option<Observation>,
    command: Command,
}

impl Prepared {
    pub(crate) fn new(
        client: Client,
        actor: Actor,
        target: Option<Observation>,
        command: Command,
    ) -> Self {
        Self {
            client,
            actor,
            target,
            command,
        }
    }

    /// Called by the request worker after owner-thread preparation. Its wait
    /// and registration-only subprocess probe never hold Runtime's mutex.
    pub fn run(self, timeout: Duration) -> Result<Value, String> {
        if matches!(&self.command, Command::WatchStart { .. }) {
            let target = self.target.as_ref().ok_or("target_unavailable")?;
            match conflict_probe(target) {
                Ok(true) => return Err("conflict".into()),
                Ok(false) => {}
                Err(code) => crate::diagnostic!(json!({
                    "component":"delivery","kind":"watch.conflict_unverified",
                    "pane_id":target.actor.pane_id,"code":code,
                })),
            }
        }
        self.client.submit(
            Effect::Command {
                actor: self.actor,
                target: self.target.map(Box::new),
                command: self.command,
            },
            timeout,
        )
    }
}

impl Client {
    pub(crate) fn submit(&self, effect: Effect, timeout: Duration) -> Result<Value, String> {
        let (reply, result) = mpsc::sync_channel(1);
        let request = Request { effect, reply };
        self.requests
            .try_send(request)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "capacity".to_owned(),
                mpsc::TrySendError::Disconnected(_) => "delivery_unavailable".to_owned(),
            })?;
        result
            .recv_timeout(timeout)
            .map_err(|_| "delivery_timeout".to_owned())?
    }
}

struct Request {
    effect: Effect,
    reply: SyncSender<Result<Value, String>>,
}

pub(crate) enum Effect {
    Command {
        actor: Actor,
        target: Option<Box<Observation>>,
        command: Command,
    },
    Tick(Vec<watch::Reading>),
    BellAttempt {
        id: String,
        observed: Box<Observation>,
    },
    Bell {
        id: String,
        recipient: Actor,
        sent: bool,
    },
}

#[derive(Clone)]
pub(crate) struct WatchWork {
    pub id: String,
    pub observation: Option<Observation>,
    pub gone: bool,
    pub status: String,
    pub state_change_seq: Option<u64>,
    pub status_changed_at_unix_ms: u64,
    pub home: Option<PathBuf>,
    pub channel: Option<Arc<dyn crate::host_access::HostChannel>>,
}

fn read_activity(work: WatchWork) -> watch::Reading {
    let mut reading = watch::Reading {
        id: work.id,
        status: work.status,
        status_available: work.observation.is_some(),
        state_change_seq: work.state_change_seq,
        status_changed_at_unix_ms: work.status_changed_at_unix_ms,
        session_modified_at_unix_ms: None,
        failure: None,
        gone: work.gone,
    };
    if work.gone {
        return reading;
    }
    let Some(observation) = work.observation else {
        reading.failure = Some("projection_unavailable".into());
        return reading;
    };
    reading.status = observation.status;
    reading.state_change_seq = observation.state_change_seq;
    reading.status_changed_at_unix_ms = observation.status_changed_at_unix_ms;
    let result = (|| {
        let request = observation.session.ok_or("session_reference_missing")?;
        if observation.actor.device_id == "local" {
            let home = work.home.ok_or("session_home_unavailable")?;
            hide_session::session_activity::read(&home, &request)
                .map_err(|_| "session_activity_failed")
        } else {
            let channel = work.channel.ok_or("helper_unavailable")?;
            crate::host_access::call_as::<hide_session::session_activity::SessionActivity>(
                channel.as_ref(),
                hide_host::protocol::Call::SessionActivity { request },
                Duration::from_secs(5),
            )
            .map_err(|error| match error {
                crate::host_access::HostCallError::NotConnected(_) => "helper_unavailable",
                crate::host_access::HostCallError::Busy => "helper_busy",
                crate::host_access::HostCallError::Refused(_) => "session_activity_refused",
                crate::host_access::HostCallError::Unknown(_) => "helper_timeout_or_format",
            })
        }
    })();
    match result {
        Ok(activity) => reading.session_modified_at_unix_ms = Some(activity.modified_at_unix_ms),
        Err(code) => reading.failure = Some(code.into()),
    }
    reading
}

fn same_target(left: &WatchWork, right: &WatchWork) -> bool {
    let (Some(left_observed), Some(right_observed)) = (&left.observation, &right.observation)
    else {
        // An unresolved watch keeps its own original clocks; it is not a
        // current sample that can be shared with another execution.
        return false;
    };
    left_observed.actor == right_observed.actor
        && left_observed.session == right_observed.session
        && left_observed.host_scope == right_observed.host_scope
        && left_observed.status == right_observed.status
        && left_observed.state_change_seq == right_observed.state_change_seq
        && left_observed.status_changed_at_unix_ms == right_observed.status_changed_at_unix_ms
        && left.home == right.home
        && match (&left.channel, &right.channel) {
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            (None, None) => true,
            _ => false,
        }
}

fn group_targets(work: Vec<WatchWork>) -> Vec<Vec<WatchWork>> {
    let mut groups = Vec::<Vec<WatchWork>>::new();
    for work in work {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| same_target(&group[0], &work))
        {
            group.push(work);
        } else {
            groups.push(vec![work]);
        }
    }
    groups
}

fn read_target(mut group: Vec<WatchWork>) -> Vec<watch::Reading> {
    let sample = read_activity(group.remove(0));
    let mut readings = Vec::with_capacity(group.len() + 1);
    for work in group {
        let mut reading = sample.clone();
        reading.id = work.id;
        readings.push(reading);
    }
    readings.push(sample);
    readings
}

fn watch_loop(runtime: Weak<Mutex<Runtime>>, client: Client, stop: Arc<AtomicBool>) {
    let mut next = Instant::now();
    let mut failure_logs = std::collections::HashMap::<String, Instant>::new();
    let mut rejection_logs = std::collections::HashMap::<String, Instant>::new();
    let mut slow_log = None::<Instant>;
    while !stop.load(Ordering::Acquire) {
        if Instant::now() < next {
            thread::sleep(Duration::from_millis(100));
            continue;
        }
        let started = Instant::now();
        let work = match runtime.upgrade() {
            Some(runtime) => match runtime.lock() {
                Ok(mut guard) => guard.delivery_watch_work(),
                Err(_) => break,
            },
            None => break,
        };
        let active: std::collections::HashSet<_> =
            work.iter().map(|work| work.id.clone()).collect();
        failure_logs.retain(|id, _| active.contains(id));
        rejection_logs.retain(|id, _| active.contains(id));
        let mut readings = Vec::with_capacity(work.len());
        let groups = group_targets(work);
        let target_count = groups.len();
        let mut work = groups.into_iter();
        loop {
            if stop.load(Ordering::Acquire) {
                break;
            }
            let group: Vec<_> = work.by_ref().take(4).collect();
            if group.is_empty() {
                break;
            }
            let answers = thread::scope(|scope| {
                let threads: Vec<_> = group
                    .into_iter()
                    .map(|work| scope.spawn(move || read_target(work)))
                    .collect();
                threads
                    .into_iter()
                    .map(|thread| thread.join())
                    .collect::<Vec<_>>()
            });
            for answer in answers {
                match answer {
                    Ok(sample) => readings.extend(sample),
                    Err(_) => {
                        crate::diagnostic!(
                            json!({"component":"delivery","kind":"watch.reader_failed"})
                        );
                    }
                }
            }
        }
        if !stop.load(Ordering::Acquire) && !readings.is_empty() {
            match client.submit(Effect::Tick(readings), Duration::from_secs(5)) {
                Ok(answer) => {
                    if let Some(rejections) = answer["rejections"].as_array() {
                        for rejection in rejections {
                            let Some(id) = rejection[0].as_str() else {
                                continue;
                            };
                            if rejection_logs
                                .get(id)
                                .is_none_or(|last| last.elapsed() >= Duration::from_secs(600))
                            {
                                crate::diagnostic!(
                                    json!({"component":"delivery","kind":"watch.update_rejected",
                                    "watch_id":id,"code":rejection[1]})
                                );
                                rejection_logs.insert(id.to_owned(), Instant::now());
                            }
                        }
                    }
                    if let Some(failures) = answer["failures"].as_array() {
                        for failure in failures {
                            let Some(id) = failure[0].as_str() else {
                                continue;
                            };
                            if failure_logs
                                .get(id)
                                .is_none_or(|last| last.elapsed() >= Duration::from_secs(600))
                            {
                                crate::diagnostic!(
                                    json!({"component":"delivery","kind":"watch.activity_read_failed","watch_id":id,"consecutive_failures":failure[1]})
                                );
                                failure_logs.insert(id.to_owned(), Instant::now());
                            }
                        }
                    }
                }
                Err(code) => crate::diagnostic!(
                    json!({"component":"delivery","kind":"watch.transaction_failed","code":code})
                ),
            }
        }
        if started.elapsed() > Duration::from_secs(1)
            && slow_log.is_none_or(|last| last.elapsed() >= Duration::from_secs(600))
        {
            crate::diagnostic!(
                json!({"component":"delivery","kind":"watch.slow_tick","elapsed_ms":started.elapsed().as_millis(),"targets":target_count,"watches":active.len()})
            );
            slow_log = Some(Instant::now());
        }
        next = started + Duration::from_millis(super::TICK_MS);
        if next < Instant::now() {
            next = Instant::now() + Duration::from_millis(super::TICK_MS);
        }
    }
}

pub(crate) struct Worker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    producers: Vec<JoinHandle<()>>,
}

impl Worker {
    pub(crate) fn spawn(
        runtime: Weak<Mutex<Runtime>>,
        notifier: ChangeNotifier,
        path: PathBuf,
    ) -> Result<(Self, Client), String> {
        let (requests, receiver) = mpsc::sync_channel(QUEUE_LIMIT);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let store_runtime = runtime.clone();
        let thread = thread::Builder::new()
            .name("hide-delivery-store".into())
            .spawn(move || run(store_runtime, notifier, path, receiver, stopped))
            .map_err(|_| "delivery_unavailable".to_owned())?;
        let client = Client { requests };
        let mut worker = Self {
            stop,
            thread: Some(thread),
            producers: Vec::new(),
        };
        let bell_runtime = runtime.clone();
        let bell_client = client.clone();
        let bell_stop = Arc::clone(&worker.stop);
        let producer = thread::Builder::new()
            .name("hide-delivery-doorbell".into())
            .spawn(move || super::doorbell::run(bell_runtime, bell_client, bell_stop))
            .map_err(|_| "delivery_unavailable".to_owned())?;
        worker.producers.push(producer);
        let watch_runtime = runtime;
        let watch_client = client.clone();
        let watch_stop = Arc::clone(&worker.stop);
        let producer = thread::Builder::new()
            .name("hide-delivery-watch".into())
            .spawn(move || watch_loop(watch_runtime, watch_client, watch_stop))
            .map_err(|_| "delivery_unavailable".to_owned())?;
        worker.producers.push(producer);
        Ok((worker, client))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for producer in self.producers.drain(..) {
            if producer.join().is_err() {
                crate::diagnostic!(json!({"component":"delivery","kind":"producer.join_failed"}));
            }
        }
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                crate::diagnostic!(json!({"component":"delivery","kind":"worker.join_failed"}));
            }
        }
    }
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn apply(ledger: &mut Ledger, request: &Request, now: u64) -> Result<(Value, bool), String> {
    let (actor, target, command) = match &request.effect {
        Effect::Tick(readings) => {
            let tick = watch::tick(ledger, readings, now)?;
            return Ok((
                json!({"failures":tick.failures,"rejections":tick.rejections}),
                tick.transitions,
            ));
        }
        Effect::BellAttempt { id, observed } => {
            let letter = ledger
                .letters
                .iter_mut()
                .find(|letter| letter.id == *id && letter.recipient.same_identity(&observed.actor))
                .ok_or("letter_unavailable")?;
            return Ok((json!({"attempt":letter.reserve_bell(now)?}), false));
        }
        Effect::Bell {
            id,
            recipient,
            sent,
        } => {
            if let Some(letter) = ledger.letters.iter_mut().find(|letter| {
                letter.id == *id
                    && letter.recipient.same_identity(recipient)
                    && letter.state == super::ledger::State::Pending
            }) {
                if *sent {
                    letter.bell_sent = true;
                } else {
                    letter.bell_errors = letter.bell_errors.saturating_add(1).min(3);
                }
            }
            return Ok((Value::Null, false));
        }
        Effect::Command {
            actor,
            target,
            command,
        } => (actor, target, command),
    };
    let value = match command {
        Command::WatchStart { .. } => {
            let target = target.as_ref().ok_or("target_unavailable")?;
            let watch = watch::start(
                ledger,
                actor,
                &target.actor,
                target.status_changed_at_unix_ms,
            )?;
            let stored = ledger
                .watches
                .iter_mut()
                .find(|stored| stored.id == watch.id)
                .ok_or("watch_unavailable")?;
            if stored.last_status == "unknown" {
                stored.last_status = target.status.clone();
                stored.last_state_change_seq = target.state_change_seq;
                stored.status_changed_at_unix_ms = target.status_changed_at_unix_ms;
            }
            Ok(json!(stored))
        }
        Command::WatchStop { id } => {
            watch::stop(ledger, actor, id)?;
            Ok(json!({"stopped":id}))
        }
        Command::WatchList => Ok(json!(
            ledger
                .watches
                .iter()
                .filter(|watch| watch.parent.same_identity(actor))
                .collect::<Vec<_>>()
        )),
        command => mailbox::apply(
            ledger,
            actor,
            target.as_ref().map(|target| &target.actor),
            command,
            now,
        ),
    }?;
    Ok((
        value,
        matches!(
            command,
            Command::WatchStart { .. } | Command::WatchStop { .. }
        ),
    ))
}

fn run(
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    path: PathBuf,
    requests: Receiver<Request>,
    stop: Arc<AtomicBool>,
) {
    let mut maintenance_at = 0;
    while !stop.load(Ordering::Acquire) {
        let mut batch = Vec::new();
        match requests.recv_timeout(Duration::from_millis(250)) {
            Ok(request) => batch.push(request),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        while batch.len() < SAVE_BATCH {
            match requests.try_recv() {
                Ok(request) => batch.push(request),
                Err(_) => break,
            }
        }
        let Some(runtime) = runtime.upgrade() else {
            break;
        };
        let state = runtime
            .lock()
            .map_err(|_| "delivery_unavailable".to_owned())
            .and_then(|guard| guard.delivery_state());
        let state = match state {
            Ok(state) => state,
            Err(code) => {
                for request in batch {
                    let _ = request.reply.send(Err(code.clone()));
                }
                continue;
            }
        };
        let now = now();
        if batch.is_empty() {
            if now.saturating_sub(maintenance_at) < 1_000 {
                continue;
            }
            maintenance_at = now;
            if !state.letters.iter().any(|letter| {
                (letter.state == super::ledger::State::Pending
                    && now.saturating_sub(letter.created_at_unix_ms) >= super::DELIVERY_EXPIRY_MS)
                    || (!letter.open()
                        && letter.finished_at_unix_ms.is_some_and(|finished| {
                            now.saturating_sub(finished) >= super::RETENTION_MS
                        }))
            }) {
                continue;
            }
        }
        let mut candidate = (*state).clone();
        candidate.expire(now);
        let mut results = Vec::with_capacity(batch.len());
        let mut transitions = false;
        for request in &batch {
            let before = candidate.clone();
            let current = match &request.effect {
                Effect::Command {
                    actor,
                    target,
                    command,
                } => {
                    // The intent lookup uses the owned candidate outside Runtime.
                    // Replays converge even after the original recipient leaves.
                    let target_required = matches!(command, Command::WatchStart { .. })
                        || matches!(command, Command::Send { intent, .. }
                            if super::mailbox::existing_intent(&candidate, actor, intent, now).is_none());
                    runtime
                        .lock()
                        .map_err(|_| "delivery_unavailable".to_owned())
                        .and_then(|guard| {
                            actor.require_native_identity()?;
                            if !guard.delivery_identity_current(actor) {
                                return Err("caller_identity_changed".into());
                            }
                            if target_required {
                                let target = target.as_ref().ok_or("target_unavailable")?;
                                if matches!(command, Command::Send { .. }) {
                                    target.actor.require_native_identity()?;
                                }
                                if !guard.delivery_identity_current(&target.actor) {
                                    return Err("target_identity_changed".into());
                                }
                            }
                            Ok(())
                        })
                }
                Effect::BellAttempt { id, observed } => runtime
                    .lock()
                    .map_err(|_| "delivery_unavailable".to_owned())
                    .and_then(|guard| {
                        if guard.delivery_bell_current(id, observed, None) {
                            Ok(())
                        } else {
                            Err("doorbell_observation_changed".into())
                        }
                    }),
                _ => Ok(()),
            };
            let result = current.and_then(|()| apply(&mut candidate, request, now));
            let result = result.and_then(|(value, publish)| {
                candidate.bytes()?;
                transitions |= publish;
                Ok(value)
            });
            if result.is_err() {
                candidate = before;
            }
            results.push(result);
        }
        let changed = candidate != *state;
        let mut saved = if changed {
            ledger::save(&path, &candidate)
        } else {
            Ok(())
        };
        if let Err(code) = &saved {
            crate::diagnostic!(
                json!({"component":"delivery","kind":"ledger.save_failed","code":code})
            );
        }
        if saved.is_ok() && changed {
            match runtime.lock() {
                Ok(mut guard) => {
                    let published = guard.publish_delivery(Arc::new(candidate), transitions);
                    drop(guard);
                    if published {
                        notifier.notify();
                    }
                }
                Err(_) => {
                    saved = Err("delivery_unavailable".into());
                    crate::diagnostic!(
                        json!({"component":"delivery","kind":"ledger.publish_failed"})
                    );
                }
            }
        }
        for (request, result) in batch.into_iter().zip(results) {
            let result = saved.as_ref().map_err(Clone::clone).and(result);
            let _ = request.reply.send(result);
        }
    }
    while let Ok(request) = requests.try_recv() {
        let _ = request.reply.send(Err("delivery_unavailable".into()));
    }
}

#[derive(Deserialize)]
struct ProbeReply<T> {
    ok: bool,
    value: Vec<T>,
}
#[derive(Deserialize)]
struct CoordinationWatch {
    target: String,
    status: String,
}
#[derive(Deserialize)]
struct Participant {
    id: String,
    machine: String,
    #[serde(rename = "hostScope")]
    host_scope: String,
    pane: Option<String>,
    session: String,
    connection: String,
}

fn probe_read<T: for<'a> Deserialize<'a>>(
    topic: &str,
    deadline: Instant,
) -> Result<Vec<T>, &'static str> {
    const OUTPUT: usize = 1024 * 1024;
    if Instant::now() >= deadline {
        return Err("conflict_probe_timeout");
    }
    let mut command = ProcessCommand::new("hcoord");
    command
        .args([topic, "list", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped());
    let mut child = hide_platform::process::OwnedChild::spawn(&mut command)
        .map_err(|_| "conflict_probe_unavailable")?;
    let stdout = child.take_stdout().ok_or("conflict_probe_failed")?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(OUTPUT as u64 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let status = loop {
        if Instant::now() >= deadline {
            break Err("conflict_probe_timeout");
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => break Err("conflict_probe_failed"),
        }
    };
    let _ = child.kill_tree();
    let bytes = reader
        .join()
        .map_err(|_| "conflict_probe_failed")?
        .map_err(|_| "conflict_probe_failed")?;
    if bytes.len() > OUTPUT {
        return Err("conflict_probe_bounds");
    }
    if !status?.success() {
        return Err("conflict_probe_failed");
    }
    let response: ProbeReply<T> =
        serde_json::from_slice(&bytes).map_err(|_| "conflict_probe_format")?;
    if !response.ok {
        return Err("conflict_probe_failed");
    }
    if response.value.len() > 2048 {
        return Err("conflict_probe_bounds");
    }
    Ok(response.value)
}

fn conflict_probe(target: &Observation) -> Result<bool, &'static str> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let watches = probe_read::<CoordinationWatch>("watch", deadline)?;
    let active: Vec<_> = watches
        .into_iter()
        .filter(|watch| watch.status == "active")
        .collect();
    if active.is_empty() {
        return Ok(false);
    }
    let participants = probe_read::<Participant>("agent", deadline)?;
    // The existing local hcoord route explicitly uses machine=local and the
    // exact socket as hostScope. "default", host-name aliases and unknown
    // remote routes cannot establish a binding and take B36's diagnostic path.
    let scope = target
        .host_scope
        .as_deref()
        .ok_or("conflict_binding_unavailable")?;
    let session = target
        .actor
        .session
        .as_deref()
        .ok_or("conflict_binding_unavailable")?;
    if target.actor.device_id != "local" {
        return Err("conflict_binding_unavailable");
    }
    let mut incomplete = false;
    for watch in active {
        let mut matches = participants
            .iter()
            .filter(|participant| participant.id == watch.target);
        let Some(participant) = matches.next() else {
            incomplete = true;
            continue;
        };
        if matches.next().is_some() {
            incomplete = true;
            continue;
        }
        if participant.machine != "local" || participant.host_scope != scope {
            incomplete = true;
            continue;
        }
        if participant.pane.as_deref() == Some(target.raw_pane_id.as_str()) {
            if participant.connection != "connected"
                || crate::wire::session_digest(&participant.session).as_deref() != Some(session)
            {
                incomplete = true;
                continue;
            }
            return Ok(true);
        }
    }
    if incomplete {
        Err("conflict_binding_unavailable")
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CoreOptions, SCHEMA_VERSION};
    use crate::sidebar::SessionSnapshotPayload;

    struct ActivityPeer(std::sync::atomic::AtomicU64);
    impl crate::host_access::HostChannel for ActivityPeer {
        fn call(
            &self,
            call: hide_host::protocol::Call,
            timeout: Duration,
        ) -> Result<crate::host_access::HostAnswer, crate::host_access::HostCallError> {
            assert!(matches!(
                call,
                hide_host::protocol::Call::SessionActivity { .. }
            ));
            assert_eq!(timeout, Duration::from_secs(5));
            let sample = self.0.fetch_add(100, Ordering::Relaxed) + 100;
            Ok(json!({"modified_at_unix_ms":sample,"bytes":10}).into())
        }
    }

    #[test]
    fn two_parent_watches_share_one_target_sample_and_refresh_it_next_tick() {
        let mut ledger = Ledger::default();
        let parent = |id: &str| Actor {
            pane_id: id.into(),
            name: id.into(),
            kind: "codex".into(),
            device_id: "local".into(),
            session: Some(format!("native-{id}")),
        };
        let target = Actor {
            pane_id: "remote:device:pane:target".into(),
            name: "target".into(),
            kind: "codex".into(),
            device_id: "device".into(),
            session: Some("native-target".into()),
        };
        let first = watch::start(&mut ledger, &parent("one"), &target, 1).unwrap();
        let second = watch::start(&mut ledger, &parent("two"), &target, 1).unwrap();
        let peer = Arc::new(ActivityPeer(std::sync::atomic::AtomicU64::new(0)));
        let channel: Arc<dyn crate::host_access::HostChannel> = peer.clone();
        let observed = Observation {
            actor: target,
            raw_pane_id: "target".into(),
            status: "idle".into(),
            state_change_seq: Some(1),
            status_changed_at_unix_ms: 1,
            last_input_at_unix_ms: 0,
            session: Some(hide_session::session_activity::SessionActivityRequest {
                agent: hide_session::Agent::Codex,
                reference_kind: "id".into(),
                reference_value: "native-target".into(),
                cwd: None,
            }),
            host_scope: Some("fixture".into()),
        };
        for expected in [100, 200] {
            let work = [&first, &second]
                .into_iter()
                .map(|watch| WatchWork {
                    id: watch.id.clone(),
                    observation: Some(observed.clone()),
                    gone: false,
                    status: "idle".into(),
                    state_change_seq: Some(1),
                    status_changed_at_unix_ms: 1,
                    home: None,
                    channel: Some(channel.clone()),
                })
                .collect();
            let readings: Vec<_> = group_targets(work)
                .into_iter()
                .flat_map(read_target)
                .collect();
            watch::tick(&mut ledger, &readings, 300).unwrap();
            assert_eq!(ledger.watches.len(), 2);
            assert!(
                ledger
                    .watches
                    .iter()
                    .all(|watch| watch.last_activity_at_unix_ms == expected)
            );
            assert_eq!(peer.0.load(Ordering::Relaxed), expected);
        }
        assert_ne!(ledger.watches[0].parent, ledger.watches[1].parent);
    }

    fn observe_recipient(runtime: &mut Runtime, native: Option<&str>, reference: Option<Value>) {
        let payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
            {"id":"sender","pane_id":"sender","agent":"codex","agent_status":"working","state_change_seq":1,"lineage_session":"sender-session"},
            {"id":"recipient","pane_id":"recipient","agent":"codex","agent_status":"idle","state_change_seq":1,"lineage_session":native,"agent_session":reference},
        ]})).unwrap();
        runtime.observe_delivery("local", &payload, None);
    }

    #[test]
    fn uncertain_native_acquisition_or_loss_preserves_unbound_watch_and_original_clocks() {
        for (original, current) in [
            (None, Some("recipient-session")),
            (Some("recipient-session"), None),
            (None, None),
        ] {
            let root = tempfile::tempdir().unwrap();
            let (runtime, parent, observed, _) = fixture(root.path());
            let mut target = observed.actor;
            target.session = original.map(str::to_owned);
            let mut ledger = Ledger::default();
            watch::start(&mut ledger, &parent, &target, 10).unwrap();
            ledger.watches[0].last_status = "idle".into();
            ledger.watches[0].last_state_change_seq = Some(1);
            ledger.watches[0].warning_count = 1;
            ledger.watches[0].first_warning_at_unix_ms = Some(20);
            let before = ledger.watches[0].clone();
            let mut guard = runtime.lock().unwrap();
            guard.publish_delivery(Arc::new(ledger.clone()), false);
            observe_recipient(
                &mut guard,
                current,
                Some(json!({"kind":"path","value":"unproven-file"})),
            );
            let work = guard.delivery_watch_work();
            drop(guard);
            assert_eq!(work.len(), 1);
            assert!(!work[0].gone);
            assert!(work[0].observation.is_none());
            let sample = read_activity(work.into_iter().next().unwrap());
            assert_eq!(sample.failure.as_deref(), Some("projection_unavailable"));
            assert!(!sample.status_available);
            assert_eq!(sample.session_modified_at_unix_ms, None);
            watch::tick(&mut ledger, &[sample], 100).unwrap();
            let after = &ledger.watches[0];
            assert_eq!(after.target, before.target);
            assert_eq!(after.view(), before.view());
            assert_eq!(
                after.status_changed_at_unix_ms,
                before.status_changed_at_unix_ms
            );
            assert_eq!(after.last_status, before.last_status);
            assert_eq!(after.last_state_change_seq, before.last_state_change_seq);
            assert!(ledger.letters.is_empty());
        }
    }

    #[test]
    fn same_native_metadata_acquisition_is_accepted_but_positive_replacement_or_absence_ends_watch()
    {
        let root = tempfile::tempdir().unwrap();
        let (runtime, parent, observed, _) = fixture(root.path());
        let path = root
            .path()
            .join(".codex/sessions/2026/01/01/recipient.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"recipient-session\"}}\nprivate\n",
        )
        .unwrap();
        let mut ledger = Ledger::default();
        watch::start(&mut ledger, &parent, &observed.actor, 1).unwrap();
        let mut guard = runtime.lock().unwrap();
        guard.publish_delivery(Arc::new(ledger.clone()), false);
        observe_recipient(
            &mut guard,
            Some("recipient-session"),
            Some(json!({"kind":"path","value":path})),
        );
        let work = guard.delivery_watch_work();
        drop(guard);
        let sample = read_activity(work.into_iter().next().unwrap());
        assert!(sample.status_available);
        assert_eq!(sample.failure, None);
        let activity = sample.session_modified_at_unix_ms.unwrap();
        watch::tick(&mut ledger, &[sample], activity).unwrap();
        assert_eq!(ledger.watches[0].last_activity_at_unix_ms, activity);
        assert_eq!(ledger.watches[0].target, observed.actor);

        for present in [true, false] {
            let mut guard = runtime.lock().unwrap();
            guard.publish_delivery(Arc::new(ledger.clone()), false);
            if present {
                observe_recipient(&mut guard, Some("replacement-session"), None);
            } else {
                let empty: SessionSnapshotPayload =
                    serde_json::from_value(json!({"agents":[]})).unwrap();
                guard.observe_delivery("local", &empty, None);
            }
            let work = guard.delivery_watch_work();
            drop(guard);
            assert!(work[0].gone);
            let mut candidate = ledger.clone();
            watch::tick(
                &mut candidate,
                &[read_activity(work.into_iter().next().unwrap())],
                activity,
            )
            .unwrap();
            assert!(candidate.watches.is_empty());
        }
    }

    fn fixture(root: &std::path::Path) -> (Arc<Mutex<Runtime>>, Actor, Observation, PathBuf) {
        let state = root.join("state");
        let options: CoreOptions = serde_json::from_value(json!({
            "schema_version":SCHEMA_VERSION,"home":root,"herdr_socket_path":null,
            "app_state_path":state.join("app.json"),
        }))
        .unwrap();
        let mut runtime = Runtime::new(
            options,
            crate::environment::EnvironmentReport {
                statuses: Vec::new(),
                home_path: Some(root.to_owned()),
                codex_home: None,
            },
        );
        let payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
            {"id":"sender","pane_id":"sender","agent":"codex","agent_status":"working","state_change_seq":1,"lineage_session":"sender-session"},
            {"id":"recipient","pane_id":"recipient","agent":"codex","agent_status":"working","state_change_seq":1,"lineage_session":"recipient-session"},
        ]})).unwrap();
        runtime.observe_delivery("local", &payload, None);
        let actor = Actor {
            pane_id: "sender".into(),
            name: "sender".into(),
            kind: "codex".into(),
            device_id: "local".into(),
            session: Some("sender-session".into()),
        };
        let target = runtime
            .delivery_observation(&Actor {
                pane_id: "recipient".into(),
                name: "recipient".into(),
                kind: "codex".into(),
                device_id: "local".into(),
                session: Some("recipient-session".into()),
            })
            .unwrap();
        (
            Arc::new(Mutex::new(runtime)),
            actor,
            target,
            hide_kit::layout::delivery_ledger(&state),
        )
    }

    #[test]
    fn stale_new_recipient_is_refused_while_retained_intent_replays_after_exit() {
        for replacement in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (runtime, actor, target, path) = fixture(root.path());
            let sender = json!({"id":"sender","pane_id":"sender","agent":"codex","agent_status":"working","state_change_seq":1,"lineage_session":"sender-session"});
            let changed: SessionSnapshotPayload = serde_json::from_value(json!({"agents": if replacement {
                vec![sender.clone(), json!({"id":"recipient","pane_id":"recipient","agent":"codex","agent_status":"working","state_change_seq":1,"lineage_session":"replacement-session"})]
            } else { vec![sender.clone()] }})).unwrap();
            runtime
                .lock()
                .unwrap()
                .observe_delivery("local", &changed, None);
            let (worker, client) = Worker::spawn(
                Arc::downgrade(&runtime),
                ChangeNotifier::noop(),
                path.clone(),
            )
            .unwrap();
            let command = Command::Send {
                target: "recipient".into(),
                intent: "once".into(),
                body: "private".into(),
            };
            assert_eq!(
                client
                    .submit(
                        Effect::Command {
                            actor: actor.clone(),
                            target: Some(Box::new(target.clone())),
                            command: command.clone()
                        },
                        Duration::from_secs(5)
                    )
                    .unwrap_err(),
                "target_identity_changed"
            );
            assert!(
                runtime
                    .lock()
                    .unwrap()
                    .delivery_state()
                    .unwrap()
                    .letters
                    .is_empty()
            );
            assert!(ledger::load(&path).unwrap().letters.is_empty());

            let present: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[sender.clone(),
                {"id":"recipient","pane_id":"recipient","agent":"codex","agent_status":"working","state_change_seq":1,"lineage_session":"recipient-session"}
            ]})).unwrap();
            runtime
                .lock()
                .unwrap()
                .observe_delivery("local", &present, None);
            let first = client
                .submit(
                    Effect::Command {
                        actor: actor.clone(),
                        target: Some(Box::new(target)),
                        command: command.clone(),
                    },
                    Duration::from_secs(5),
                )
                .unwrap();
            let absent: SessionSnapshotPayload =
                serde_json::from_value(json!({"agents":[sender]})).unwrap();
            runtime
                .lock()
                .unwrap()
                .observe_delivery("local", &absent, None);
            let replay = client
                .submit(
                    Effect::Command {
                        actor,
                        target: None,
                        command,
                    },
                    Duration::from_secs(5),
                )
                .unwrap();
            assert_eq!(first["id"], replay["id"]);
            let persisted = ledger::load(&path).unwrap();
            assert_eq!(persisted.letters.len(), 1);
            assert_eq!(persisted.letters[0].id, first["id"].as_str().unwrap());
            drop(worker);
        }
    }

    #[test]
    fn success_is_durable_and_a_failed_save_never_publishes_the_letter() {
        for writable in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let (runtime, actor, target, path) = fixture(root.path());
            if !writable {
                std::fs::write(path.parent().unwrap(), b"blocked parent").unwrap();
            }
            let (worker, client) = Worker::spawn(
                Arc::downgrade(&runtime),
                ChangeNotifier::noop(),
                path.clone(),
            )
            .unwrap();
            let result = client.submit(
                Effect::Command {
                    actor,
                    target: Some(Box::new(target)),
                    command: Command::Send {
                        target: "recipient".into(),
                        intent: "send-once".into(),
                        body: "private fixture".into(),
                    },
                },
                Duration::from_secs(5),
            );
            if writable {
                let id = result.unwrap()["id"].as_str().unwrap().to_owned();
                let persisted = ledger::load(&path).unwrap();
                assert_eq!(persisted.letters[0].id, id);
                assert_eq!(
                    runtime
                        .lock()
                        .unwrap()
                        .delivery_state()
                        .unwrap()
                        .letters
                        .len(),
                    1
                );
                drop(worker);
                let (restarted, _, _, _) = fixture(root.path());
                assert_eq!(
                    restarted.lock().unwrap().delivery_state().unwrap().letters[0].id,
                    id
                );
            } else {
                assert_eq!(result.unwrap_err(), "ledger_unavailable");
                assert!(
                    runtime
                        .lock()
                        .unwrap()
                        .delivery_state()
                        .unwrap()
                        .letters
                        .is_empty()
                );
                assert_eq!(
                    std::fs::read(path.parent().unwrap()).unwrap(),
                    b"blocked parent"
                );
                drop(worker);
            }
        }
    }
}
