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
    Bell {
        id: String,
        recipient: Actor,
        sent: bool,
    },
}

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

fn watch_loop(runtime: Weak<Mutex<Runtime>>, client: Client, stop: Arc<AtomicBool>) {
    let mut next = Instant::now();
    let mut failure_logs = std::collections::HashMap::<String, Instant>::new();
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
        let mut readings = Vec::with_capacity(work.len());
        let mut work = work.into_iter();
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
                    .map(|work| scope.spawn(move || read_activity(work)))
                    .collect();
                threads
                    .into_iter()
                    .map(|thread| thread.join())
                    .collect::<Vec<_>>()
            });
            for answer in answers {
                match answer {
                    Ok(reading) => readings.push(reading),
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
                json!({"component":"delivery","kind":"watch.slow_tick","elapsed_ms":started.elapsed().as_millis(),"targets":active.len()})
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
            return Ok((json!({"failures":tick.failures}), tick.transitions));
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
                } => runtime
                    .lock()
                    .map(|guard| {
                        guard.delivery_identity_current(actor)
                            && (!matches!(command, Command::WatchStart { .. })
                                || target.as_ref().is_some_and(|target| {
                                    guard.delivery_identity_current(&target.actor)
                                }))
                    })
                    .unwrap_or(false),
                _ => true,
            };
            let result = if current {
                apply(&mut candidate, request, now)
            } else {
                Err("caller_identity_changed".into())
            };
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
        let saved = if changed {
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
            let published = runtime
                .lock()
                .map(|mut guard| guard.publish_delivery(Arc::new(candidate), transitions))
                .unwrap_or(false);
            if published {
                notifier.notify();
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
