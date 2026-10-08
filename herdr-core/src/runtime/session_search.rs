//! One owned search worker, one latest query slot, no I/O under Runtime.
use super::*;
use crate::node_access::{LinkError, NodeLink, call_as};
use hide_node_link::protocol::Call;
use hide_session::search::{FILE_LIMIT, IndexStep, SearchIndex, SearchPage};
use std::collections::VecDeque;
use std::sync::Condvar;
use std::time::Duration;

use crate::model::SessionSearchSnapshot as SearchSnapshot;
/// One read of a session file on its node: at most the 1 MiB read budget.
const NODE_READ_TIMEOUT: Duration = Duration::from_secs(10);
#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SearchPayload {
    pub workspace_id: String,
    pub device_id: String,
    pub query: String,
    #[serde(default = "all_providers")]
    pub provider: String,
    #[serde(default)]
    pub clear: bool,
    pub days: Option<u16>,
}
fn all_providers() -> String {
    "all".into()
}
#[derive(Clone)]
struct Request {
    generation: u64,
    project: String,
    database: PathBuf,
    query: String,
    provider: String,
    workspace_id: String,
    device_id: String,
    rows: Arc<Vec<SessionRowSnapshot>>,
    /// The node that holds the rows' session files.
    node: Arc<dyn NodeLink>,
    clear: bool,
    days: Option<u16>,
    rejected_control: Option<String>,
}
#[derive(Default)]
struct Mailbox {
    pending: Option<Request>,
    controls: VecDeque<Request>,
    stop: bool,
    cancel: bool,
}
#[derive(Clone)]
pub(super) struct SearchClient(Arc<(Mutex<Mailbox>, Condvar)>);
pub(crate) struct SearchWorker {
    client: SearchClient,
    join: Option<thread::JoinHandle<()>>,
}
impl Drop for SearchWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.client.0;
        if let Ok(mut box_) = lock.lock() {
            box_.stop = true;
            wake.notify_one();
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
impl SearchClient {
    pub(super) fn invalidate(&self) {
        if let Ok(mut mailbox) = self.0.0.lock() {
            mailbox.cancel = true;
            mailbox.pending = None;
            self.0.1.notify_one();
        }
    }
}
impl SearchWorker {
    pub(crate) fn spawn(
        runtime: Weak<Mutex<Runtime>>,
        notifier: ChangeNotifier,
    ) -> Result<Self, String> {
        let client = SearchClient(Arc::new((Mutex::new(Mailbox::default()), Condvar::new())));
        let worker_client = client.clone();
        let join = thread::Builder::new()
            .name("hide-session-search".into())
            .spawn(move || run(worker_client, runtime, notifier, |_| {}))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            join: Some(join),
        })
    }
    pub(crate) fn install(&self, runtime: &mut Runtime) {
        runtime.search_client = Some(self.client.clone());
    }
}
impl Runtime {
    pub(super) fn request_session_search(&mut self, mut payload: SearchPayload) -> bool {
        if let Some(agent) = hide_session::Agent::from_kind(&payload.provider) {
            payload.provider = agent.as_str().to_owned();
        }
        let Some(sessions) = self.snapshot.project_sessions.as_ref() else {
            return false;
        };
        if sessions.workspace_id != payload.workspace_id
            || sessions.device_id != payload.device_id
            || sessions.unavailable_reason.is_some()
        {
            return false;
        }
        self.search_generation += 1;
        if payload.query.chars().count() > 256
            || (payload.provider != "all"
                && !hide_session::Agent::from_kind(&payload.provider)
                    .is_some_and(|agent| agent.has_session_file()))
            || payload.days.is_some_and(|d| ![0, 30, 90, 365].contains(&d))
        {
            if let Some(client) = &self.search_client {
                client.invalidate();
            }
            self.snapshot.session_search = Some(SearchSnapshot {
                query: payload.query.trim().to_owned(),
                provider: payload.provider,
                workspace_id: payload.workspace_id,
                device_id: payload.device_id,
                loading: false,
                page: SearchPage::default(),
                failure: Some(
                    "Search up to 256 characters; choose Off, 30, 90 or 365 days.".into(),
                ),
                ..self.snapshot.session_search.take().unwrap_or_default()
            });
            return true;
        }
        let query = payload.query.trim().to_owned();
        self.snapshot.session_search = Some(SearchSnapshot {
            query: query.clone(),
            provider: payload.provider.clone(),
            workspace_id: payload.workspace_id.clone(),
            device_id: payload.device_id.clone(),
            loading: true,
            ..self.snapshot.session_search.take().unwrap_or_default()
        });
        let Some(client) = self.search_client.clone() else {
            self.snapshot.session_search.as_mut().unwrap().failure =
                Some("Conversation search worker is unavailable.".into());
            self.snapshot.session_search.as_mut().unwrap().loading = false;
            return true;
        };
        let Some(project) = self.project_sessions_work.project_id.clone() else {
            return true;
        };
        let node = match self.node_link(&payload.device_id) {
            Ok(node) => node,
            Err(reason) => {
                let search = self.snapshot.session_search.as_mut().unwrap();
                search.failure = Some(reason);
                search.loading = false;
                return true;
            }
        };
        let rows = self
            .project_sessions_work
            .known
            .get(&payload.workspace_id)
            .cloned()
            .unwrap_or_default();
        let mut request = Request {
            generation: self.search_generation,
            project,
            database: self.state_path.with_file_name("session-search.sqlite3"),
            query,
            provider: payload.provider,
            workspace_id: payload.workspace_id,
            device_id: payload.device_id,
            rows,
            node,
            clear: payload.clear,
            days: payload.days,
            rejected_control: None,
        };
        if let Ok(mut mailbox) = client.0.0.lock() {
            if request.clear || request.days.is_some() {
                if mailbox.controls.len() >= 8 {
                    if let Some(search) = self.snapshot.session_search.as_mut() {
                        search.loading = false;
                        search.control_failure=Some("Search index controls are busy. Try again after the current control completes.".into());
                    }
                    request.rejected_control = Some("Search index controls are busy. Try again after the current control completes.".into());
                } else {
                    mailbox.controls.push_back(request.clone());
                }
            }
            request.clear = false;
            request.days = None;
            mailbox.pending = Some(request);
            client.0.1.notify_one();
        }
        true
    }
    pub(super) fn ingest_search(&mut self, generation: u64, answer: SearchSnapshot) -> bool {
        if generation != self.search_generation {
            return false;
        }
        let Some(sessions) = self.snapshot.project_sessions.as_ref() else {
            return false;
        };
        if sessions.unavailable_reason.is_some() {
            return false;
        }
        if self.snapshot.session_search.as_ref() == Some(&answer) {
            return false;
        }
        self.snapshot.session_search = Some(answer);
        true
    }
    pub(super) fn start_project_search(&mut self) {
        let Some(s) = self.snapshot.project_sessions.as_ref() else {
            return;
        };
        self.request_session_search(SearchPayload {
            workspace_id: s.workspace_id.clone(),
            device_id: s.device_id.clone(),
            provider: "all".into(),
            query: self
                .snapshot
                .session_search
                .as_ref()
                .map(|s| s.query.clone())
                .unwrap_or_default(),
            clear: false,
            days: None,
        });
    }
}
fn run(
    client: SearchClient,
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    mut before_publish: impl FnMut(&SearchSnapshot),
) {
    let mut index: Option<SearchIndex> = None;
    let mut current: Option<Request> = None;
    let mut queue = VecDeque::new();
    let mut state = SearchSnapshot::default();
    let mut refresh = Instant::now();
    let mut retention_at: Option<Instant> = None;
    let mut indexing_failure = None;
    let mut setup_failure = None;
    // Project outcomes are durable. A storage failure that also prevents
    // recording its outcome stays visible until a subsequent control succeeds.
    let mut storage_control_failure: Option<String> = None;
    loop {
        let (request, control, cancel) = {
            let (lock, wake) = &*client.0;
            let Ok(mailbox) = lock.lock() else {
                return;
            };
            let Ok((mut mailbox, _)) = wake.wait_timeout_while(
                mailbox,
                if queue.is_empty() {
                    Duration::from_secs(5)
                } else {
                    Duration::from_millis(40)
                },
                |b| b.pending.is_none() && b.controls.is_empty() && !b.cancel && !b.stop,
            ) else {
                return;
            };
            if mailbox.stop && mailbox.controls.is_empty() {
                return;
            }
            if mailbox.stop {
                mailbox.pending = None;
                mailbox.cancel = true;
            }
            (
                mailbox.pending.take(),
                mailbox.controls.pop_front(),
                std::mem::take(&mut mailbox.cancel),
            )
        };
        if cancel {
            indexing_failure = None;
            setup_failure = None;
            current = None;
            queue.clear();
            state = SearchSnapshot::default();
        }
        let mut controlled_project = None;
        if let Some(control) = control {
            if index.is_none() {
                match SearchIndex::open(&control.database) {
                    Ok(db) => index = Some(db),
                    Err(e) => {
                        storage_control_failure = Some(format!("Search index control failed: {e}"))
                    }
                }
            }
            if let Some(db) = index.as_mut() {
                let result = if let Some(days) = control.days {
                    db.set_days(&control.project, days)
                } else {
                    db.clear(&control.project)
                };
                let failure = result
                    .as_ref()
                    .err()
                    .map(|e| format!("Search index control failed: {e}"));
                match db.record_control_outcome(&control.project, failure.as_deref()) {
                    Ok(()) => {
                        if result.is_ok() {
                            storage_control_failure = None;
                        }
                    }
                    Err(e) => {
                        storage_control_failure = Some(format!(
                            "Search index control outcome could not be saved: {e}"
                        ))
                    }
                }
                if result.is_ok() {
                    controlled_project = Some(control.project);
                }
            }
        }
        if request.is_none()
            && controlled_project.is_some()
            && current
                .as_ref()
                .is_some_and(|r| Some(&r.project) == controlled_project.as_ref())
        {
            // Retention/rebuild restarts this scope, even after query coalescing.
            if let Some(r) = current.as_ref() {
                queue = (0..r.rows.len().min(FILE_LIMIT)).collect();
                state.indexed = 0;
                if let Some(db) = index.as_ref() {
                    match db.days(&r.project) {
                        Ok(days) => {
                            state.days = days;
                            state.policy_loaded = true;
                        }
                        Err(e) => {
                            state.policy_loaded = false;
                            state.failure = Some(e);
                        }
                    }
                }
            }
        }
        if let Some(request) = request {
            setup_failure = None;
            let scope_changed = current
                .as_ref()
                .is_none_or(|r| r.project != request.project);
            if scope_changed {
                state = SearchSnapshot::default();
                indexing_failure = None;
            }
            let changed = current.as_ref().is_none_or(|r| {
                r.project != request.project || !Arc::ptr_eq(&r.rows, &request.rows)
            });
            if index.is_none() {
                match SearchIndex::open(&request.database) {
                    Ok(db) => index = Some(db),
                    Err(e) => setup_failure = Some(format!("Search index could not open: {e}")),
                }
            }
            if let Some(db) = index.as_mut() {
                if let Some(error) = &request.rejected_control
                    && let Err(e) = db.record_control_outcome(&request.project, Some(error))
                {
                    storage_control_failure = Some(format!(
                        "Search index control outcome could not be saved: {e}"
                    ));
                }
                match db.days(&request.project) {
                    Ok(days) => {
                        state.days = days;
                        state.policy_loaded = true;
                    }
                    Err(e) => {
                        state.policy_loaded = false;
                        setup_failure = Some(e);
                    }
                }
                if changed || controlled_project.as_ref() == Some(&request.project) {
                    queue = (0..request.rows.len().min(FILE_LIMIT)).collect();
                    state.indexed = 0;
                    state.total = request.rows.len().min(FILE_LIMIT);
                    indexing_failure = None;
                    if request.rows.len() > FILE_LIMIT {
                        setup_failure=Some("Only the newest 2,000 sessions are indexed. Narrow retention to reduce the corpus.".into());
                    }
                    let keep = request
                        .rows
                        .iter()
                        .take(FILE_LIMIT)
                        .map(|r| r.id.clone())
                        .collect::<Vec<_>>();
                    if let Err(e) = db.retain(&request.project, &keep) {
                        setup_failure = Some(e);
                    }
                    refresh = Instant::now();
                }
            }
            if request.rows.len() > FILE_LIMIT {
                setup_failure = Some("Only the newest 2,000 sessions are indexed. Narrow retention to reduce the corpus.".into());
            }
            state.query = request.query.clone();
            state.provider = request.provider.clone();
            state.workspace_id = request.workspace_id.clone();
            state.device_id = request.device_id.clone();
            state.loading = false;
            state.page = SearchPage::default();
            current = Some(request);
        }
        if retention_at.is_none_or(|at| at.elapsed() >= Duration::from_secs(30))
            && let Some(db) = index.as_mut()
        {
            if let Err(e) = db.prune_all(now_ms()) {
                indexing_failure = Some(format!("Expired copies could not be removed: {e}"));
            }
            retention_at = Some(Instant::now());
        }
        let Some(request) = current.as_ref() else {
            continue;
        };
        state.failure = setup_failure.clone().or_else(|| indexing_failure.clone());
        state.control_failure = match index
            .as_ref()
            .map(|db| db.control_failure(&request.project))
        {
            Some(Ok(failure)) => failure.or_else(|| storage_control_failure.clone()),
            Some(Err(e)) => Some(format!(
                "Search index control outcome could not be read: {e}"
            )),
            None => storage_control_failure.clone(),
        };
        let cutoff = now_ms().saturating_sub(u64::from(state.days) * 86_400_000);
        if state.policy_loaded && state.days == 0 {
            queue.clear();
            state.indexed = 0;
        } else if state.policy_loaded
            && queue.is_empty()
            && refresh.elapsed() >= Duration::from_secs(30)
        {
            queue = (0..state.total).collect();
            indexing_failure = None;
            state.indexed = 0;
            refresh = Instant::now();
        }
        if let Some(db) = index.as_mut() {
            // One read or validation chunk is at most 1 MiB. Bound each turn
            // by eight chunks and yield between chunks after 20 ms.
            let turn = Instant::now();
            for chunk in 0..8 {
                if chunk > 0 && turn.elapsed() >= Duration::from_millis(20) {
                    break;
                }
                let Some(i) = queue.pop_front() else {
                    break;
                };
                let row = &request.rows[i];
                let Some(agent) = hide_session::Agent::from_kind(&row.provider)
                    .filter(|agent| agent.has_session_file())
                else {
                    state.failure = Some(format!("Unsupported session provider: {}", row.provider));
                    queue.clear();
                    break;
                };
                let step = db
                    .saved(&request.project, &row.id)
                    .map_err(Failure::Index)
                    .and_then(|saved| {
                        read_on_node(request.node.as_ref(), agent, &row.locator, saved)
                    });
                let applied = step.and_then(|step| {
                    db.apply(&request.project, &row.id, &row.locator, cutoff, step)
                        .map_err(Failure::Index)
                });
                match applied {
                    Ok(true) => queue.push_back(i),
                    Ok(false) => state.indexed += 1,
                    Err(Failure::Node(reason)) => {
                        // The node, not the file, failed: the rows read before
                        // stay, and the next refresh reads this one again.
                        state.failure = Some(format!(
                            "Session files could not be read on their device: {reason}"
                        ));
                        queue.clear();
                        break;
                    }
                    Err(Failure::Index(error)) => {
                        // Failed or missing sources cannot keep searchable old rows.
                        if let Err(e) = db.remove(&request.project, Some(&row.id)) {
                            state.failure = Some(format!("Search index invalidation failed: {e}"));
                        } else {
                            state.failure =
                                Some(format!("Some sessions could not be indexed: {error}"));
                        }
                        state.indexed += 1;
                    }
                }
            }
            let was_indexing = state.indexing || state.total > state.indexed;
            state.indexing = !queue.is_empty();
            if was_indexing && !state.indexing {
                if let Err(e) = db.prune(&request.project, cutoff) {
                    state.failure = Some(format!("Expired copies could not be removed: {e}"));
                }
                refresh = Instant::now();
            }
            if state.failure != setup_failure {
                indexing_failure = state.failure.clone();
            }
            state.failure = setup_failure.clone().or_else(|| indexing_failure.clone());
            match if state.days == 0 {
                Ok(SearchPage::default())
            } else {
                let allowed = (request.provider != "all").then(|| {
                    request
                        .rows
                        .iter()
                        .filter(|row| {
                            hide_session::Agent::from_kind(&row.provider)
                                == hide_session::Agent::from_kind(&request.provider)
                        })
                        .map(|row| row.id.clone())
                        .collect::<Vec<_>>()
                });
                let node = request.node.as_ref();
                db.search_scoped(
                    &request.project,
                    &request.query,
                    cutoff,
                    allowed.as_deref(),
                    &mut |paths| {
                        call_as(
                            node,
                            Call::SessionStamps {
                                paths: paths.to_vec(),
                            },
                            NODE_READ_TIMEOUT,
                        )
                        .map_err(|error| error.to_string())
                    },
                )
            } {
                Ok(page) => state.page = page,
                Err(e) => {
                    state.page = SearchPage::default();
                    state.failure = Some(format!("Content search could not finish: {e}"));
                }
            }
        }
        before_publish(&state);
        let Some(runtime) = runtime.upgrade() else {
            return;
        };
        let changed = runtime
            .lock()
            .map(|mut r| r.ingest_search(request.generation, state.clone()))
            .unwrap_or(false);
        drop(runtime);
        if changed {
            notifier.notify();
        }
    }
}
/// Why a session file was not indexed: its read or the index refused it
/// (the file's rows go), or its node could not be reached (they stay).
enum Failure {
    Index(String),
    Node(String),
}

/// Reads one step of the session file at `path` on `node`, from `saved`.
fn read_on_node(
    node: &dyn NodeLink,
    agent: hide_session::Agent,
    path: &str,
    saved: Option<hide_session::search::SavedFile>,
) -> Result<IndexStep, Failure> {
    call_as(
        node,
        Call::SessionIndexRead {
            agent,
            path: path.to_owned(),
            saved,
        },
        NODE_READ_TIMEOUT,
    )
    .map_err(|error| match error {
        LinkError::Refused(refusal) => Failure::Index(refusal.message),
        other => Failure::Node(other.to_string()),
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProjectSessionsSnapshot;
    use std::fs;
    #[test]
    fn drop_drains_off_that_was_still_queued_with_copied_rows() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("session-search.sqlite3");
        let source = temp.path().join("s.jsonl");
        fs::write(&source, r#"{"type":"response_item","timestamp":"2026-10-01T00:00:00Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"copied private body"}]}}"#.to_owned()+"\n").unwrap();
        let mut index = SearchIndex::open(&database).unwrap();
        while hide_session::search_read::update(
            &mut index,
            "p",
            "s",
            hide_session::Agent::Codex,
            &source,
            0,
        )
        .unwrap()
        {}
        assert_eq!(
            hide_session::search_read::search(&index, "p", "private body", 0)
                .unwrap()
                .hits
                .len(),
            1
        );
        drop(index);
        let client = SearchClient(Arc::new((Mutex::new(Mailbox::default()), Condvar::new())));
        client.0.0.lock().unwrap().controls.push_back(Request {
            generation: 1,
            project: "p".into(),
            database: database.clone(),
            query: String::new(),
            provider: "all".into(),
            workspace_id: "p".into(),
            device_id: "local".into(),
            rows: Arc::new(vec![]),
            node: Arc::new(hide_node::Local::of_process()),
            clear: false,
            days: Some(0),
            rejected_control: None,
        });
        let (release, held) = std::sync::mpsc::channel();
        let worker_client = client.clone();
        let worker = SearchWorker {
            client: client.clone(),
            join: Some(thread::spawn(move || {
                held.recv().unwrap();
                run(worker_client, Weak::new(), ChangeNotifier::noop(), |_| {});
            })),
        };
        // Drop raises `stop` and wakes the mailbox, so this waits on the
        // mailbox's own condition variable rather than a time.
        let check = thread::spawn(move || {
            let (lock, wake) = &*client.0;
            let (mailbox, timeout) = wake
                .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(10), |mailbox| {
                    !mailbox.stop
                })
                .unwrap();
            assert!(!timeout.timed_out(), "Drop never raised stop");
            assert_eq!(
                mailbox.controls.len(),
                1,
                "Off must still be queued at Drop"
            );
            drop(mailbox);
            release.send(()).unwrap();
        });
        let started = Instant::now();
        drop(worker);
        check.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
        let index = SearchIndex::open(&database).unwrap();
        assert_eq!(index.days("p").unwrap(), 0);
        assert!(
            hide_session::search_read::search(&index, "p", "private body", 0)
                .unwrap()
                .hits
                .is_empty()
        );
    }
    /// A node that cannot be reached keeps the rows read before; only a
    /// read the node refused drops the file's rows.
    #[test]
    fn an_unreachable_node_is_not_read_as_a_failed_file() {
        struct Answering(LinkError);
        impl NodeLink for Answering {
            fn call(
                &self,
                _call: Call,
                _timeout: Duration,
            ) -> Result<crate::node_access::LinkAnswer, LinkError> {
                Err(match &self.0 {
                    LinkError::Refused(refusal) => LinkError::Refused(refusal.clone()),
                    _ => LinkError::NotConnected("gone".into()),
                })
            }
        }
        let read = |error| read_on_node(&Answering(error), hide_session::Agent::Codex, "/s", None);
        assert!(matches!(
            read(LinkError::NotConnected("gone".into())),
            Err(Failure::Node(_))
        ));
        let refused = hide_node_link::HostError::new(
            hide_node_link::ErrorCode::Io,
            "Session source changed before indexing.",
        );
        assert!(matches!(
            read(LinkError::Refused(refused)),
            Err(Failure::Index(reason)) if reason == "Session source changed before indexing."
        ));
    }
    fn scope(r: &mut Runtime, project: &str) {
        r.snapshot.project_sessions = Some(ProjectSessionsSnapshot {
            device_id: crate::node::test_node().to_string(),
            workspace_id: project.into(),
            unavailable_reason: None,
            loading: false,
            failure: None,
            rows: Arc::new(vec![]),
            detail: None,
        });
        r.project_sessions_work.project_id = Some(project.into());
    }
    fn payload(project: &str, query: &str, days: Option<u16>) -> SearchPayload {
        SearchPayload {
            workspace_id: project.into(),
            device_id: crate::node::test_node().to_string(),
            query: query.into(),
            provider: "all".into(),
            clear: false,
            days,
        }
    }
    fn held_worker(
        shared: &Arc<Mutex<Runtime>>,
    ) -> (
        SearchWorker,
        std::sync::mpsc::Receiver<SearchSnapshot>,
        std::sync::mpsc::Sender<()>,
    ) {
        let client = SearchClient(Arc::new((Mutex::new(Mailbox::default()), Condvar::new())));
        let worker_client = client.clone();
        let weak = Arc::downgrade(shared);
        let (frames, observed) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel();
        let worker = SearchWorker {
            client,
            join: Some(thread::spawn(move || {
                run(worker_client, weak, ChangeNotifier::noop(), |s| {
                    if frames.send(s.clone()).is_ok() {
                        let _ = held.recv_timeout(Duration::from_secs(5));
                    }
                })
            })),
        };
        worker.install(&mut shared.lock().unwrap());
        (worker, observed, release)
    }
    #[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
    fn terminal(shared: &Arc<Mutex<Runtime>>, query: &str) -> SearchSnapshot {
        let at = Instant::now();
        loop {
            let state = shared
                .lock()
                .unwrap()
                .snapshot
                .session_search
                .clone()
                .unwrap();
            if state.query == query && !state.loading {
                return state;
            }
            assert!(
                at.elapsed() < Duration::from_secs(5),
                "query must reach its terminal outcome"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn real_sql_query_failure_recovers_and_dispatched_old_answer_is_fenced() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("session-search.sqlite3");
        let mut r = super::super::tests::runtime();
        r.state_path = temp.path().join("state.json");
        scope(&mut r, "p");
        let shared = Arc::new(Mutex::new(r));
        let (worker, frames, release) = held_worker(&shared);
        let next = || frames.recv_timeout(Duration::from_secs(5)).unwrap();
        shared
            .lock()
            .unwrap()
            .request_session_search(payload("p", "initial", None));
        assert!(next().failure.is_none());
        release.send(()).unwrap();
        terminal(&shared, "initial");
        let sql = rusqlite::Connection::open(&database).unwrap();
        sql.execute_batch("ALTER TABLE messages RENAME TO saved_messages")
            .unwrap();
        shared
            .lock()
            .unwrap()
            .request_session_search(payload("p", "failed query", None));
        assert!(
            next()
                .failure
                .unwrap()
                .contains("Content search could not finish")
        );
        release.send(()).unwrap();
        assert!(terminal(&shared, "failed query").failure.is_some());
        sql.execute_batch("ALTER TABLE saved_messages RENAME TO messages")
            .unwrap();
        shared
            .lock()
            .unwrap()
            .request_session_search(payload("p", "dispatched old", None));
        let old = next();
        assert_eq!(old.query, "dispatched old");
        assert!(old.failure.is_none());
        // The old query finished on the real worker, but its publication is held.
        shared
            .lock()
            .unwrap()
            .request_session_search(payload("p", "latest", None));
        release.send(()).unwrap();
        let latest = next();
        assert_eq!(latest.query, "latest");
        assert!(latest.failure.is_none());
        assert_eq!(
            shared
                .lock()
                .unwrap()
                .snapshot
                .session_search
                .as_ref()
                .unwrap()
                .query,
            "latest"
        );
        assert!(
            shared
                .lock()
                .unwrap()
                .snapshot
                .session_search
                .as_ref()
                .unwrap()
                .loading,
            "late old answer must not finish latest intent"
        );
        release.send(()).unwrap();
        assert!(terminal(&shared, "latest").failure.is_none());
        drop(sql);
        drop(worker);
    }
    #[test]
    fn eight_actual_failed_controls_and_queue_rejection_preserve_other_scope_and_query() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("session-search.sqlite3");
        drop(SearchIndex::open(&database).unwrap());
        let sql = rusqlite::Connection::open(&database).unwrap();
        sql.execute_batch("CREATE TRIGGER fail_policy BEFORE INSERT ON policy WHEN NEW.project IN ('p0','p1','p2','p3','p4','p5','p6','p7') BEGIN SELECT RAISE(ABORT,'fixture policy refusal'); END;").unwrap();
        let mut r = super::super::tests::runtime();
        r.state_path = temp.path().join("state.json");
        scope(&mut r, "p0");
        let shared = Arc::new(Mutex::new(r));
        let (worker, frames, release) = held_worker(&shared);
        let next = || frames.recv_timeout(Duration::from_secs(5)).unwrap();
        for i in 0..8 {
            let p = format!("p{i}");
            {
                let mut r = shared.lock().unwrap();
                scope(&mut r, &p);
                r.request_session_search(payload(&p, &p, Some(0)));
            }
            let failed = next();
            assert!(
                failed
                    .control_failure
                    .unwrap()
                    .contains("fixture policy refusal")
            );
            release.send(()).unwrap();
            assert!(terminal(&shared, &p).control_failure.is_some());
        }
        // Hold a real worker answer so both requests land before it reads the
        // mailbox; an idle worker could take "superseded" between the two.
        {
            let mut r = shared.lock().unwrap();
            scope(&mut r, "p8");
            r.request_session_search(payload("p8", "barrier", None));
        }
        assert_eq!(next().query, "barrier");
        {
            let mut r = shared.lock().unwrap();
            r.request_session_search(payload("p8", "superseded", Some(0)));
            r.request_session_search(payload("p8", "latest ninth", None));
        }
        release.send(()).unwrap();
        let ninth = next();
        assert_eq!(ninth.query, "latest ninth");
        assert_eq!(ninth.days, 0);
        assert!(ninth.control_failure.is_none());
        release.send(()).unwrap();
        terminal(&shared, "latest ninth");
        // Hold a real worker answer while filling the bounded accepted-control queue.
        shared
            .lock()
            .unwrap()
            .request_session_search(payload("p8", "held", None));
        assert_eq!(next().query, "held");
        {
            let mut r = shared.lock().unwrap();
            for _ in 0..8 {
                r.request_session_search(payload("p8", "accepted", Some(30)));
            }
            scope(&mut r, "p9");
            r.request_session_search(payload("p9", "surviving latest", Some(0)));
            assert!(
                r.snapshot
                    .session_search
                    .as_ref()
                    .unwrap()
                    .control_failure
                    .as_ref()
                    .unwrap()
                    .contains("busy")
            );
        }
        release.send(()).unwrap();
        for _ in 0..8 {
            let answer = next();
            assert_eq!(answer.query, "surviving latest");
            assert!(answer.control_failure.as_ref().unwrap().contains("busy"));
            release.send(()).unwrap();
        }
        let answer = terminal(&shared, "surviving latest");
        assert_eq!(answer.days, 90);
        assert!(answer.control_failure.is_some());
        drop(worker);
        drop(sql);
        let index = SearchIndex::open(&database).unwrap();
        for i in 0..8 {
            assert!(
                index
                    .control_failure(&format!("p{i}"))
                    .unwrap()
                    .unwrap()
                    .contains("fixture policy refusal")
            );
        }
        assert_eq!(index.days("p8").unwrap(), 30);
        assert!(index.control_failure("p8").unwrap().is_none());
        assert!(
            index
                .control_failure("p9")
                .unwrap()
                .unwrap()
                .contains("busy")
        );
    }
}
