//! Label worker regressions (PRD labels-in-hided D-04, D-10, B4-B12),
//! driven through real session files and a scripted provider.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use hide_ai::{
    AiBackend, AiError, AiRequest, AiResponse, AiRouter, AiSettings, Availability, CancelToken,
    ModelCatalog, NoopLogSink, ProviderId, RouterConfig,
};
use hide_session::label_transcript::{LabelTranscript, LabelTranscriptRequest};
use hide_session::turns::Waiting;
use serde_json::{Value, json};

use super::NodeTranscripts;
use super::analyzer::LabelAnalyzer;
use super::facts::Requester;
use super::input::OperatorInput;
use super::store::{LOCAL_TARGET, LabelStore};
use super::worker::{LabelWorker, ObservedAgent, ReadFailure, TranscriptSource, WorkerConfig};
use crate::sidebar::{AgentLabel, SessionSnapshotPayload};

/// A provider that answers from a queue and can hold an answer back.
struct Scripted {
    answers: Mutex<VecDeque<Value>>,
    calls: AtomicUsize,
    /// How many of the next requests run out of time.
    timeouts: AtomicUsize,
    /// While set, an answer waits for `release`.
    held: Mutex<bool>,
    released: Condvar,
}

impl Scripted {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(VecDeque::new()),
            calls: AtomicUsize::new(0),
            timeouts: AtomicUsize::new(0),
            held: Mutex::new(false),
            released: Condvar::new(),
        })
    }

    /// A v5 answer: `end` is the turn's end, and a question's `line` is the
    /// reply it asks for, any other's the goal's progress.
    fn answer(&self, goal: &str, end: &str, reply: &str) {
        let line = if end == "question" {
            reply.to_owned()
        } else {
            format!("{goal} 진행")
        };
        self.answers.lock().unwrap().push_back(json!({
            "goal": goal, "goal_changed": true, "line": line, "end": end,
        }));
    }

    fn time_out(&self, times: usize) {
        self.timeouts.store(times, Ordering::SeqCst);
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn hold(&self) {
        *self.held.lock().unwrap() = true;
    }

    fn release(&self) {
        *self.held.lock().unwrap() = false;
        self.released.notify_all();
    }
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
    fn execute(&self, _: &AiRequest, cancel: &CancelToken) -> Result<AiResponse, AiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self
            .timeouts
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok()
        {
            return Err(AiError::Timeout);
        }
        let mut held = self.held.lock().unwrap();
        while *held {
            // A provider child ends when its request is cancelled.
            if cancel.is_cancelled() {
                return Err(AiError::Cancelled);
            }
            held = self
                .released
                .wait_timeout(held, Duration::from_millis(20))
                .unwrap()
                .0;
        }
        drop(held);
        let value = self
            .answers
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| AiError::Transient("no scripted answer".to_owned()))?;
        Ok(AiResponse {
            value,
            usage: Default::default(),
        })
    }
}

/// Counts the reads it passes on to this machine's files.
struct CountingSource {
    inner: super::NodeTranscripts,
    reads: AtomicUsize,
}

impl TranscriptSource for CountingSource {
    fn read(&self, request: &LabelTranscriptRequest) -> Result<LabelTranscript, ReadFailure> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.read(request)
    }
}

struct Harness {
    home: tempfile::TempDir,
    state: tempfile::TempDir,
    locks: tempfile::TempDir,
    backend: Arc<Scripted>,
    analyzer: Arc<LabelAnalyzer>,
    input: Arc<OperatorInput>,
}

impl Harness {
    fn new() -> Self {
        let backend = Scripted::new();
        let router_backend: Arc<dyn AiBackend> = backend.clone();
        let analyzer = LabelAnalyzer::spawn_with(
            Box::new(|| AiSettings {
                chosen: true,
                ..AiSettings::default()
            }),
            Box::new(move |_| {
                Arc::new(AiRouter::new(
                    vec![Arc::clone(&router_backend)],
                    RouterConfig {
                        priority: vec![ProviderId::CLAUDE],
                        max_transient_attempts: 1,
                        ..RouterConfig::default()
                    },
                    Arc::new(NoopLogSink),
                ))
            }),
            Arc::default(),
        )
        .unwrap();
        Self {
            home: tempfile::tempdir().unwrap(),
            state: tempfile::tempdir().unwrap(),
            locks: tempfile::tempdir().unwrap(),
            backend,
            analyzer: Arc::new(analyzer),
            input: Arc::new(super::input::OperatorInput::new(LOCAL_TARGET)),
        }
    }

    fn store(&self) -> Arc<LabelStore> {
        Arc::new(LabelStore::open(
            Some(self.state.path()),
            Some(self.home.path()),
            LOCAL_TARGET,
        ))
    }

    fn worker(&self, store: Arc<LabelStore>) -> (LabelWorker, Receiver<()>, Arc<CountingSource>) {
        let source = Arc::new(CountingSource {
            inner: {
                let node: Arc<dyn crate::node_access::NodeLink> =
                    Arc::new(hide_node::Local::new(Some(self.home.path().to_path_buf())));
                super::NodeTranscripts::new(Box::new(move || Ok(Arc::clone(&node))))
            },
            reads: AtomicUsize::new(0),
        });
        let (worker, woken) = self.spawn(store, LOCAL_TARGET, "local.lock", source.clone());
        (worker, woken, source)
    }

    /// A device's worker, reading through whatever channel `channel` hands out.
    fn device_worker(&self, channel: super::ChannelSource) -> (LabelWorker, Receiver<()>) {
        self.spawn(
            self.store(),
            "device:mini",
            "device-mini.lock",
            Arc::new(NodeTranscripts::new(channel)),
        )
    }

    fn spawn(
        &self,
        store: Arc<LabelStore>,
        target: &str,
        lock_name: &str,
        source: Arc<dyn TranscriptSource>,
    ) -> (LabelWorker, Receiver<()>) {
        let (wake, woken) = channel();
        let wake = Mutex::new(wake);
        let worker = LabelWorker::spawn(
            WorkerConfig {
                target: target.to_owned(),
                lock_path: Some(self.locks.path().join(lock_name)),
                input: Arc::clone(&self.input),
            },
            store,
            Arc::clone(&self.analyzer),
            source,
            Arc::new(move || {
                let _ = wake.lock().unwrap().send(());
            }),
        )
        .unwrap();
        (worker, woken)
    }

    /// A Claude session file the worker reads by path, where Claude keeps
    /// it: a read refuses a file outside the home's transcript root.
    fn session(&self, name: &str, session_id: &str, turns: &[(&str, &str)]) -> PathBuf {
        let project = self.home.path().join(".claude/projects/-project");
        std::fs::create_dir_all(&project).unwrap();
        let path = project.join(format!("{name}.jsonl"));
        let mut lines = String::new();
        for (index, (kind, text)) in turns.iter().enumerate() {
            let at = format!("2026-10-01T00:00:{index:02}Z");
            let record = if *kind == "user" {
                json!({"type":"user","sessionId":session_id,"timestamp":at,
                    "origin":{"kind":"human"},"message":{"role":"user","content":text}})
            } else {
                json!({"type":"assistant","sessionId":session_id,"timestamp":at,
                    "message":{"role":"assistant","content":[{"type":"text","text":text}]}})
            };
            lines.push_str(&format!("{record}\n"));
        }
        std::fs::write(&path, lines).unwrap();
        path
    }
}

fn agent(path: &Path, status: &str, seq: u64) -> ObservedAgent {
    ObservedAgent {
        pane_id: "w1:p1".to_owned(),
        agent: Some("claude".to_owned()),
        status: Some(status.to_owned()),
        reference: Some(("path".to_owned(), path.display().to_string())),
        cwd: None,
        state_change_seq: seq,
    }
}

fn observe(worker: &mut LabelWorker, agent: &ObservedAgent) {
    let live = HashSet::from([agent.pane_id.clone()]);
    worker.observe(
        std::slice::from_ref(agent),
        Some(&live),
        Instant::now(),
        1_000,
    );
    worker.tick(Instant::now());
}

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Takes results until nothing is read or analyzed.
fn settle(worker: &mut LabelWorker, woken: &Receiver<()>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !worker.settled() {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "the worker did not settle");
        let _ = woken.recv_timeout(left.min(Duration::from_millis(200)));
        worker.drain(Instant::now(), unix_now_ms());
    }
}

/// What the row would carry for the agent as it is now.
fn shown(worker: &LabelWorker, agent: &ObservedAgent) -> Option<AgentLabel> {
    let (kind, value) = agent.reference.clone().unwrap();
    let mut payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents": [{
        "pane_id": agent.pane_id, "agent": agent.agent, "agent_status": agent.status,
        "state_change_seq": agent.state_change_seq,
        "agent_session": {"kind": kind, "value": value},
    }]}))
    .unwrap();
    worker.overlay().apply(&mut payload);
    payload.agents.remove(0).label
}

fn task(label: Option<AgentLabel>) -> Option<String> {
    label.and_then(|label| label.task)
}

#[test]
fn a_finished_turn_is_named_once_and_unchanged_panes_spend_nothing() {
    let harness = Harness::new();
    let (mut worker, woken, source) = harness.worker(harness.store());
    let path = harness.session(
        "a",
        "native-a",
        &[("user", "파서 버그 고쳐줘"), ("assistant", "고쳤습니다")],
    );
    harness.backend.answer("파서 버그 수정 작업", "done", "");
    let idle = agent(&path, "idle", 3);

    observe(&mut worker, &idle);
    settle(&mut worker, &woken);
    assert_eq!(
        task(shown(&worker, &idle)).as_deref(),
        Some("파서 버그 수정 작업")
    );
    assert_eq!(harness.backend.calls(), 1);

    let reads = source.reads.load(Ordering::SeqCst);
    for _ in 0..3 {
        observe(&mut worker, &idle);
        settle(&mut worker, &woken);
    }
    assert_eq!(harness.backend.calls(), 1, "an unchanged pane asks nothing");
    assert_eq!(
        source.reads.load(Ordering::SeqCst),
        reads,
        "nor reads anything"
    );
}

#[test]
fn an_agent_listed_before_its_pane_is_still_read_and_named() {
    let harness = Harness::new();
    let (mut worker, woken, _) = harness.worker(harness.store());
    let path = harness.session(
        "a",
        "native-a",
        &[("user", "새 창 에이전트 요청"), ("assistant", "끝났습니다")],
    );
    harness.backend.answer("새 창 에이전트 작업", "done", "");
    let idle = agent(&path, "idle", 1);

    // Herdr's agent list already has the pane; its pane list does not yet.
    let panes_without_it = HashSet::new();
    worker.observe(
        std::slice::from_ref(&idle),
        Some(&panes_without_it),
        Instant::now(),
        1_000,
    );
    worker.tick(Instant::now());
    settle(&mut worker, &woken);
    assert_eq!(
        task(shown(&worker, &idle)).as_deref(),
        Some("새 창 에이전트 작업")
    );
}

#[test]
fn a_running_agent_is_not_asking_anything() {
    let harness = Harness::new();
    let (mut worker, woken, _) = harness.worker(harness.store());
    let path = harness.session(
        "a",
        "native-a",
        &[("user", "배포할까?"), ("assistant", "A와 B 중 고르세요")],
    );
    harness
        .backend
        .answer("배포 방식 결정 작업", "question", "A/B 선택");
    let idle = agent(&path, "idle", 3);
    observe(&mut worker, &idle);
    settle(&mut worker, &woken);
    let label = shown(&worker, &idle).unwrap();
    assert!(label.question);
    assert_eq!(label.expected_reply.as_deref(), Some("A/B 선택"));

    let working = agent(&path, "working", 4);
    worker.observe(std::slice::from_ref(&working), None, Instant::now(), 2_000);
    let label = shown(&worker, &working).unwrap();
    assert!(!label.question);
    assert_eq!(label.expected_reply, None);
    assert_eq!(label.task.as_deref(), Some("배포 방식 결정 작업"));
}

#[test]
fn a_turn_that_ran_between_two_looks_ends_the_question() {
    let harness = Harness::new();
    let (mut worker, woken, _) = harness.worker(harness.store());
    let path = harness.session(
        "a",
        "native-a",
        &[("user", "배포할까?"), ("assistant", "A와 B 중 고르세요")],
    );
    harness
        .backend
        .answer("배포 방식 결정 작업", "question", "A/B 선택");
    let asking = agent(&path, "idle", 3);
    observe(&mut worker, &asking);
    settle(&mut worker, &woken);
    assert!(shown(&worker, &asking).unwrap().question);

    // Herdr went working and then done inside one burst: only the done and
    // its sequence two changes on are seen.
    let finished = agent(&path, "done", 5);
    worker.observe(std::slice::from_ref(&finished), None, Instant::now(), 2_000);
    let label = shown(&worker, &finished).unwrap();
    assert!(!label.question);
    assert_eq!(label.expected_reply, None);
    assert_eq!(label.task.as_deref(), Some("배포 방식 결정 작업"));
}

#[test]
fn another_session_shows_nothing_of_the_last_one_until_it_is_proven() {
    let harness = Harness::new();
    let (mut worker, woken, _) = harness.worker(harness.store());
    let a = harness.session(
        "a",
        "native-a",
        &[("user", "세션 A 요청"), ("assistant", "A 끝")],
    );
    let b = harness.session(
        "b",
        "native-b",
        &[("user", "세션 B 요청"), ("assistant", "B 끝")],
    );
    harness.backend.answer("세션 A의 작업 이름", "done", "");
    harness.backend.answer("세션 B의 작업 이름", "done", "");
    harness.backend.answer("다시 세션 A의 작업", "done", "");

    let on_a = agent(&a, "idle", 1);
    observe(&mut worker, &on_a);
    settle(&mut worker, &woken);
    assert_eq!(
        task(shown(&worker, &on_a)).as_deref(),
        Some("세션 A의 작업 이름")
    );

    // A reused pane (or a new session) hides A at once, before any read.
    let on_b = agent(&b, "idle", 2);
    worker.observe(std::slice::from_ref(&on_b), None, Instant::now(), 2_000);
    assert_eq!(shown(&worker, &on_b), None);
    worker.tick(Instant::now());
    settle(&mut worker, &woken);
    assert_eq!(
        task(shown(&worker, &on_b)).as_deref(),
        Some("세션 B의 작업 이름")
    );

    // Back to A: B's label never stands in for it, and A is proven again.
    let back = agent(&a, "idle", 3);
    worker.observe(std::slice::from_ref(&back), None, Instant::now(), 3_000);
    assert_eq!(shown(&worker, &back), None);
    worker.tick(Instant::now());
    settle(&mut worker, &woken);
    assert_eq!(
        task(shown(&worker, &back)).as_deref(),
        Some("다시 세션 A의 작업")
    );
}

#[test]
fn an_analysis_that_lands_after_the_session_changed_is_dropped() {
    let harness = Harness::new();
    let (mut worker, woken, _) = harness.worker(harness.store());
    let a = harness.session(
        "a",
        "native-a",
        &[("user", "세션 A 요청"), ("assistant", "A 끝")],
    );
    let b = harness.session(
        "b",
        "native-b",
        &[("user", "세션 B 요청"), ("assistant", "B 끝")],
    );
    harness.backend.answer("늦게 도착한 A 작업", "done", "");
    harness.backend.answer("세션 B의 작업 이름", "done", "");
    harness.backend.hold();

    let on_a = agent(&a, "idle", 1);
    observe(&mut worker, &on_a);
    let deadline = Instant::now() + Duration::from_secs(20);
    while harness.backend.calls() == 0 {
        assert!(Instant::now() < deadline, "A was never analyzed");
        let _ = woken.recv_timeout(Duration::from_millis(50));
        worker.drain(Instant::now(), unix_now_ms());
    }
    let on_b = agent(&b, "idle", 2);
    observe(&mut worker, &on_b);
    harness.backend.release();
    settle(&mut worker, &woken);
    assert_eq!(
        task(shown(&worker, &on_b)).as_deref(),
        Some("세션 B의 작업 이름")
    );
}

#[test]
fn a_restart_restores_the_label_with_no_read_and_no_request() {
    let harness = Harness::new();
    let path = harness.session(
        "a",
        "native-a",
        &[("user", "재시작 전 요청"), ("assistant", "완료")],
    );
    harness.backend.answer("재시작 전에 붙은 작업", "done", "");
    let idle = agent(&path, "idle", 5);
    {
        let (mut worker, woken, _) = harness.worker(harness.store());
        observe(&mut worker, &idle);
        settle(&mut worker, &woken);
        assert!(shown(&worker, &idle).is_some());
    }
    let (mut worker, woken, source) = harness.worker(harness.store());
    observe(&mut worker, &idle);
    assert_eq!(
        task(shown(&worker, &idle)).as_deref(),
        Some("재시작 전에 붙은 작업")
    );
    settle(&mut worker, &woken);
    assert_eq!(source.reads.load(Ordering::SeqCst), 0);
    assert_eq!(harness.backend.calls(), 1);
}

#[test]
fn an_imported_label_shows_only_for_the_session_it_was_proven_for() {
    let harness = Harness::new();
    let owner = hide_session::label_reference_token("claude", "id", "native-a").unwrap();
    let plugin = harness
        .home
        .path()
        .join(".local/state/hide.agent-context-labels");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(
        plugin.join("display-state.json"),
        json!({"panes": {
            "w1:p1": {"session_owner": owner, "state_change_seq": 4, "changed_unix_ms": 1,
                      "task": "플러그인이 붙인 작업"},
            "w1:p2": {"state_change_seq": 4, "changed_unix_ms": 1, "task": "주인 없는 라벨"}
        }})
        .to_string(),
    )
    .unwrap();
    let (mut worker, _woken, _) = harness.worker(harness.store());
    let by_id = |pane: &str, id: &str| ObservedAgent {
        pane_id: pane.to_owned(),
        agent: Some("claude".to_owned()),
        status: Some("idle".to_owned()),
        reference: Some(("id".to_owned(), id.to_owned())),
        cwd: None,
        state_change_seq: 4,
    };
    let proven = by_id("w1:p1", "native-a");
    let other = by_id("w1:p1", "native-b");
    let ownerless = by_id("w1:p2", "native-c");
    worker.observe(
        &[proven.clone(), ownerless.clone()],
        None,
        Instant::now(),
        1_000,
    );
    assert_eq!(
        task(shown(&worker, &proven)).as_deref(),
        Some("플러그인이 붙인 작업")
    );
    assert_eq!(shown(&worker, &ownerless), None);
    assert_eq!(shown(&worker, &other), None);
    assert_eq!(harness.backend.calls(), 0);
}

#[test]
fn a_second_daemon_on_the_same_server_stands_by_and_shows_no_labels() {
    let harness = Harness::new();
    let path = harness.session("a", "native-a", &[("user", "요청"), ("assistant", "끝")]);
    harness.backend.answer("첫 데몬이 붙인 작업", "done", "");
    let idle = agent(&path, "idle", 1);
    let (mut first, woken, _) = harness.worker(harness.store());
    observe(&mut first, &idle);
    settle(&mut first, &woken);
    assert!(shown(&first, &idle).is_some());

    let (mut second, second_woken, source) = harness.worker(harness.store());
    observe(&mut second, &idle);
    assert_eq!(shown(&second, &idle), None);
    assert_eq!(source.reads.load(Ordering::SeqCst), 0);
    assert_eq!(harness.backend.calls(), 1);

    // The first daemon exits; the standby takes over on its next attempt,
    // catches up by reading, and spends no request on a turn already named.
    drop(first);
    second.tick(Instant::now() + Duration::from_secs(31));
    settle(&mut second, &second_woken);
    assert_eq!(
        task(shown(&second, &idle)).as_deref(),
        Some("첫 데몬이 붙인 작업")
    );
    assert_eq!(source.reads.load(Ordering::SeqCst), 1);
    assert_eq!(harness.backend.calls(), 1);
}

#[test]
fn a_shutdown_answers_the_running_request_without_recording_it() {
    let harness = Harness::new();
    let path = harness.session("a", "native-a", &[("user", "요청"), ("assistant", "끝")]);
    harness.backend.answer("기록되면 안 되는 작업", "done", "");
    harness.backend.hold();
    let idle = agent(&path, "idle", 1);
    let store = harness.store();
    let (mut worker, woken, _) = harness.worker(Arc::clone(&store));
    observe(&mut worker, &idle);
    let deadline = Instant::now() + Duration::from_secs(20);
    while harness.backend.calls() == 0 {
        assert!(Instant::now() < deadline, "nothing was analyzed");
        let _ = woken.recv_timeout(Duration::from_millis(50));
        worker.drain(Instant::now(), unix_now_ms());
    }
    harness.analyzer.shutdown();
    settle(&mut worker, &woken);
    let record = &store.target(LOCAL_TARGET)["w1:p1"];
    assert_eq!(record.goal, None);
    assert_eq!(
        record.analysis_turn_end, None,
        "the turn is asked again next time"
    );
}

#[test]
fn the_providers_answer_is_judged_before_it_is_shown() {
    let question_without_reply = super::context_label::parse_text(
        r#"{"goal":"배포 방식 결정 작업","goal_changed":true,"line":"","end":"question"}"#,
    )
    .unwrap();
    assert_eq!(
        question_without_reply.end,
        super::analysis::LabelEnd::Done,
        "a question needs a reply to ask for"
    );
    let long = super::context_label::parse_text(
        r#"{"goal":"배포 방식 결정 작업","goal_changed":false,"line":"이 줄은 사십 자를 훌쩍 넘기는 아주 긴 결과 문장이라서 화면 한 줄에 맞게 반드시 잘려야 합니다","end":"done"}"#,
    )
    .unwrap();
    assert_eq!(long.line.chars().count(), 40);
    assert!(super::context_label::parse_text(r#"{"goal":"짧음"}"#).is_err());
    assert!(
        super::context_label::parse_text(
            r#"{"goal":"배포 방식 결정 작업","goal_changed":true,"line":"","end":"stuck"}"#
        )
        .is_err(),
        "an end outside the five is refused"
    );
}

/// A helper from before protocol 12, which does not know the call.
struct OlderHelper;

impl crate::node_access::NodeLink for OlderHelper {
    fn call(
        &self,
        _: hide_node_link::protocol::Call,
        _: Duration,
    ) -> Result<crate::node_access::LinkAnswer, crate::node_access::LinkError> {
        Err(crate::node_access::LinkError::Refused(
            hide_node_link::error::HostError::new(
                hide_node_link::error::ErrorCode::InvalidRequest,
                "unknown variant `label_transcript`",
            ),
        ))
    }
}

/// The device helper in process, reading transcripts under its own HOME
/// (the harness's), as `hide-host-helper` does under the device's.
struct HelperAt(PathBuf);

impl crate::node_access::NodeLink for HelperAt {
    fn call(
        &self,
        call: hide_node_link::protocol::Call,
        timeout: Duration,
    ) -> Result<crate::node_access::LinkAnswer, crate::node_access::LinkError> {
        match call {
            hide_node_link::protocol::Call::LabelTranscript { request } => {
                hide_host::serve::label_transcript(&self.0, &request)
                    .map(crate::node_access::LinkAnswer::Parsed)
                    .map_err(crate::node_access::LinkError::Refused)
            }
            call => hide_node::Local::of_process().call(call, timeout),
        }
    }

    fn in_process(&self) -> bool {
        true
    }
}

/// B13: a device pane's conversation comes through the helper protocol and
/// is labeled here the same way as this Mac's.
#[test]
fn a_device_panes_label_is_read_through_its_helper() {
    let harness = Harness::new();
    let path = harness.session(
        "device",
        "native-d",
        &[
            ("user", "미니에서 배치 돌려줘"),
            ("assistant", "돌렸습니다"),
        ],
    );
    harness.backend.answer("미니 배치 실행 작업", "done", "");
    let helper: Arc<dyn crate::node_access::NodeLink> =
        Arc::new(HelperAt(harness.home.path().to_path_buf()));
    let (mut worker, woken) = harness.device_worker(Box::new(move || Ok(Arc::clone(&helper))));
    let idle = agent(&path, "idle", 1);
    observe(&mut worker, &idle);
    settle(&mut worker, &woken);
    assert_eq!(
        task(shown(&worker, &idle)).as_deref(),
        Some("미니 배치 실행 작업")
    );
}

/// B14: while the device cannot be reached its pane keeps the last label,
/// and the read it owes runs once the helper is back.
#[test]
fn a_disconnected_device_keeps_its_label_and_catches_up_after_it_returns() {
    let harness = Harness::new();
    let path = harness.session(
        "device",
        "native-d",
        &[("user", "첫 요청"), ("assistant", "첫 답")],
    );
    harness.backend.answer("첫 번째 기기 작업", "done", "");
    harness.backend.answer("두 번째 기기 작업", "done", "");
    let connected = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let helper: Arc<dyn crate::node_access::NodeLink> =
        Arc::new(HelperAt(harness.home.path().to_path_buf()));
    let link = Arc::clone(&connected);
    let (mut worker, woken) = harness.device_worker(Box::new(move || {
        if link.load(Ordering::SeqCst) {
            Ok(Arc::clone(&helper))
        } else {
            Err("device_helper_not_ready")
        }
    }));
    observe(&mut worker, &agent(&path, "idle", 1));
    settle(&mut worker, &woken);

    connected.store(false, Ordering::SeqCst);
    harness.session(
        "device",
        "native-d",
        &[
            ("user", "첫 요청"),
            ("assistant", "첫 답"),
            ("user", "두 번째 요청"),
            ("assistant", "두 번째 답"),
        ],
    );
    let moved = agent(&path, "idle", 2);
    observe(&mut worker, &moved);
    settle(&mut worker, &woken);
    assert_eq!(
        task(shown(&worker, &moved)).as_deref(),
        Some("첫 번째 기기 작업")
    );
    assert_eq!(harness.backend.calls(), 1);

    connected.store(true, Ordering::SeqCst);
    worker.tick(Instant::now() + Duration::from_secs(16));
    settle(&mut worker, &woken);
    assert_eq!(harness.backend.calls(), 2);
    // A turn first seen at its end updates the progress; the task title
    // moves only at a turn's start (the plugin's rule, kept).
    assert_eq!(
        shown(&worker, &moved)
            .and_then(|label| label.progress)
            .as_deref(),
        Some("두 번째 기기 작업 진행")
    );
}

/// B15: an older helper leaves the pane on its provider name and spends no
/// request; the device's kit row is what offers the fix.
#[test]
fn a_helper_that_cannot_read_conversations_leaves_the_provider_name() {
    let harness = Harness::new();
    let path = harness.session(
        "device",
        "native-d",
        &[("user", "요청"), ("assistant", "답")],
    );
    let (mut worker, woken) = harness.device_worker(Box::new(|| {
        Ok(Arc::new(OlderHelper) as Arc<dyn crate::node_access::NodeLink>)
    }));
    let idle = agent(&path, "idle", 1);
    observe(&mut worker, &idle);
    settle(&mut worker, &woken);
    assert_eq!(shown(&worker, &idle), None);
    assert_eq!(harness.backend.calls(), 0);
}

#[test]
fn an_analysis_before_the_runtime_read_the_settings_uses_the_saved_choice() {
    let home = tempfile::tempdir().unwrap();
    let chosen = AiSettings {
        provider: ProviderId::CLAUDE,
        ..AiSettings::default()
    };
    hide_ai::settings::save(home.path(), &chosen).unwrap();
    let no_runtime = std::sync::Weak::new();
    assert_eq!(
        super::analysis_settings(&no_runtime, Some(home.path())).provider,
        ProviderId::CLAUDE
    );
}

/// A source whose read panics, as a broken parser would.
struct Panicking;

impl TranscriptSource for Panicking {
    fn read(&self, _: &LabelTranscriptRequest) -> Result<LabelTranscript, ReadFailure> {
        panic!("reader fixture panic");
    }
}

#[test]
fn a_read_that_panics_does_not_stop_the_servers_reads() {
    let harness = Harness::new();
    let path = harness.session("a", "native-a", &[("user", "요청"), ("assistant", "끝")]);
    let (mut worker, woken) = harness.spawn(
        harness.store(),
        LOCAL_TARGET,
        "local.lock",
        Arc::new(Panicking),
    );
    observe(&mut worker, &agent(&path, "idle", 1));
    // The panicked read is answered, so nothing stays in flight.
    settle(&mut worker, &woken);
}

#[test]
fn concurrent_workers_keep_local_records_on_disk_and_device_records_in_memory() {
    let harness = Harness::new();
    let store = harness.store();
    let mut threads: Vec<_> = (0..8)
        .map(|index| {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                for round in 0..20u64 {
                    let mut records = std::collections::BTreeMap::new();
                    records.insert(
                        format!("w{index}:p1"),
                        super::store::PaneRecord::first_seen(round, round),
                    );
                    store.save_target(&format!("device:{index}"), &records);
                }
            })
        })
        .collect();
    let local = Arc::clone(&store);
    threads.push(std::thread::spawn(move || {
        for round in 0..20u64 {
            local.save_target(
                super::store::LOCAL_TARGET,
                &std::collections::BTreeMap::from([(
                    "local:p1".to_owned(),
                    super::store::PaneRecord::first_seen(round, round),
                )]),
            );
        }
    }));
    for thread in threads {
        thread.join().unwrap();
    }
    let reopened = LabelStore::open(Some(harness.state.path()), None, LOCAL_TARGET);
    assert_eq!(reopened.target(super::store::LOCAL_TARGET).len(), 1);
    assert_eq!(
        reopened.target(super::store::LOCAL_TARGET),
        store.target(super::store::LOCAL_TARGET),
        "the final local write must survive reopening the whole file"
    );
    for index in 0..8 {
        assert_eq!(store.target(&format!("device:{index}")).len(), 1);
        assert!(reopened.target(&format!("device:{index}")).is_empty());
    }
}

/// A core told its home reads labels from that home, not the process
/// `HOME`: a test daemon with a private home must never import the
/// operator's label state (2026-10-02 security review).
#[test]
fn a_core_given_its_own_home_imports_labels_from_that_home_only() {
    let home = tempfile::tempdir().unwrap();
    let plugin = home.path().join(".local/state/hide.agent-context-labels");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(
        plugin.join("display-state.json"),
        json!({"panes": {"w9:p1": {"session_owner": "v1:owned", "state_change_seq": 1,
            "changed_unix_ms": 1_u64, "task": "개인 홈의 라벨"}}})
        .to_string(),
    )
    .unwrap();
    let state = tempfile::tempdir().unwrap();
    let options = |home: String| crate::CoreOptions {
        schema_version: crate::SCHEMA_VERSION,
        home: Some(home),
        node_id: crate::node::test_node(),
        herdr_socket_path: None,
        herdr_bin_path: None,
        app_state_path: state.path().join("core-state.json").display().to_string(),
        host_helper_dir: None,
        host_helper_root: None,
        host_cli_dir: None,
        workspace_views_path: None,
        shortcut_import_path: None,
        local_issues_path: None,
    };
    assert!(
        crate::Core::create(
            options("relative/home".to_owned()),
            std::sync::Arc::new(hide_node::Local::of_process()),
            None,
        )
        .is_none(),
        "a relative home names no account folder"
    );
    let core = crate::Core::create(
        options(home.path().display().to_string()),
        std::sync::Arc::new(hide_node::Local::new(Some(home.path().to_path_buf()))),
        None,
    )
    .expect("a core starts");
    drop(core);
    let imported = LabelStore::open(Some(state.path()), None, LOCAL_TARGET).target(LOCAL_TARGET);
    assert_eq!(
        imported.keys().cloned().collect::<Vec<_>>(),
        ["w9:p1"],
        "only the named home's label state is imported"
    );
}

/// `2026-10-01T00:00:<second>Z`, the time `Harness::session` stamps a turn.
fn turn_at(second: u64) -> u64 {
    1_790_812_800_000 + second * 1_000
}

#[test]
fn a_request_the_operator_submitted_is_theirs_and_survives_a_restart_without_a_read() {
    let harness = Harness::new();
    let store = harness.store();
    let (mut worker, woken, source) = harness.worker(Arc::clone(&store));
    let path = harness.session(
        "a",
        "native-a",
        &[
            ("user", "요청 보기 만들어줘"),
            ("assistant", "만들었습니다\nPR을 열었어요"),
            (
                "user",
                "Hide letter r1 from ci-lead (claude) [request]\nCI 다시 봐줘",
            ),
        ],
    );
    harness.backend.answer("요청 보기", "done", "");
    harness.input.record("w1:p1", turn_at(0) - 300, false);
    let idle = agent(&path, "idle", 3);
    observe(&mut worker, &idle);
    settle(&mut worker, &woken);

    let facts = store.target(LOCAL_TARGET)["w1:p1"].facts.clone();
    let operator = facts.operator_request.as_ref().unwrap();
    assert_eq!(operator.text, "요청 보기 만들어줘");
    assert_eq!(operator.requester, Requester::Operator);
    assert!(operator.first);
    let other = facts.other_request.as_ref().unwrap();
    assert_eq!(other.requester, Requester::Named("ci-lead".to_owned()));
    assert_eq!(
        facts.reply.as_ref().unwrap().text,
        "만들었습니다\nPR을 열었어요"
    );

    // A restarted daemon has no submits and reads nothing for an unchanged
    // pane; what it shows is what was stored.
    drop(worker);
    let reads = source.reads.load(Ordering::SeqCst);
    let (mut restarted, woken, source) = harness.worker(harness.store());
    observe(&mut restarted, &idle);
    settle(&mut restarted, &woken);
    assert_eq!(
        source.reads.load(Ordering::SeqCst),
        0,
        "{reads} reads before"
    );
    assert_eq!(harness.store().target(LOCAL_TARGET)["w1:p1"].facts, facts);
}

#[test]
fn a_message_no_submit_explains_is_another_agents() {
    let harness = Harness::new();
    let store = harness.store();
    let (mut worker, woken, _) = harness.worker(Arc::clone(&store));
    let path = harness.session("a", "native-a", &[("user", "이 테스트 돌려줘")]);
    harness.backend.answer("테스트", "done", "");
    let working = agent(&path, "working", 1);
    observe(&mut worker, &working);
    settle(&mut worker, &woken);
    let facts = store.target(LOCAL_TARGET)["w1:p1"].facts.clone();
    assert!(facts.operator_request.is_none());
    assert_eq!(facts.other_request.unwrap().requester, Requester::Agent);
}

/// Takes results and the provider waits that came due until nothing is
/// left, so a retry made on the next tick is taken too.
fn settle_with_ticks(worker: &mut LabelWorker, woken: &Receiver<()>) {
    for _ in 0..4 {
        settle(worker, woken);
        worker.tick(Instant::now());
    }
    settle(worker, woken);
}

/// PRD overview-request-view D-09, B22: a turn another agent started never
/// moves the goal, and the operator's next request may.
#[test]
fn only_a_turn_the_operator_started_moves_the_goal() {
    let harness = Harness::new();
    let store = harness.store();
    let (mut worker, woken, _) = harness.worker(Arc::clone(&store));
    let first = [("user", "요청 보기 만들어줘"), ("assistant", "만들었어요")];
    let path = harness.session("a", "native-a", &first);
    harness.input.record("w1:p1", turn_at(0) - 300, false);
    harness.backend.answer("요청 보기 화면 만들기", "done", "");
    observe(&mut worker, &agent(&path, "idle", 3));
    settle(&mut worker, &woken);

    let by_agent = [
        first[0],
        first[1],
        (
            "user",
            "Hide letter r1 from ci-lead (claude) [request]\nCI 다시 봐줘",
        ),
    ];
    harness.session("a", "native-a", &by_agent);
    harness.backend.answer("CI 다시 보는 작업", "working", "");
    let working = agent(&path, "working", 4);
    observe(&mut worker, &working);
    settle(&mut worker, &woken);
    assert_eq!(harness.backend.calls(), 2);
    assert_eq!(
        task(shown(&worker, &working)).as_deref(),
        Some("요청 보기 화면 만들기")
    );

    let by_operator = [
        by_agent[0],
        by_agent[1],
        by_agent[2],
        ("assistant", "다시 돌렸어요"),
        ("user", "이제 설정 화면도 새로 만들어줘"),
    ];
    harness.session("a", "native-a", &by_operator);
    harness.input.record("w1:p1", turn_at(4) - 300, false);
    harness
        .backend
        .answer("요청 보기와 설정 화면", "working", "");
    let working = agent(&path, "working", 6);
    observe(&mut worker, &working);
    settle(&mut worker, &woken);
    assert_eq!(
        task(shown(&worker, &working)).as_deref(),
        Some("요청 보기와 설정 화면")
    );
}

/// D-33, B20: a turn's end that ran out of time is asked once more; a second
/// timeout leaves the row on its facts and asks nothing else.
#[test]
fn a_turn_end_that_timed_out_is_asked_once_more_and_no_more() {
    let harness = Harness::new();
    let (mut worker, woken, _) = harness.worker(harness.store());
    let path = harness.session(
        "a",
        "native-a",
        &[("user", "고쳐줘"), ("assistant", "고쳤어요")],
    );
    harness.backend.time_out(1);
    harness.backend.answer("파서 버그 수정 작업", "done", "");
    let idle = agent(&path, "idle", 3);
    observe(&mut worker, &idle);
    settle_with_ticks(&mut worker, &woken);
    assert_eq!(harness.backend.calls(), 2);
    assert_eq!(
        task(shown(&worker, &idle)).as_deref(),
        Some("파서 버그 수정 작업")
    );

    let harness = Harness::new();
    let store = harness.store();
    let (mut worker, woken, _) = harness.worker(Arc::clone(&store));
    let path = harness.session(
        "a",
        "native-a",
        &[("user", "고쳐줘"), ("assistant", "고쳤어요")],
    );
    harness.backend.time_out(5);
    let idle = agent(&path, "idle", 3);
    observe(&mut worker, &idle);
    settle_with_ticks(&mut worker, &woken);
    assert_eq!(harness.backend.calls(), 2, "one retry, not more");
    let record = &store.target(LOCAL_TARGET)["w1:p1"];
    assert_eq!(record.end, None);
    assert!(record.analysis_turn_end.is_some(), "the turn is parked");
}

/// D-11, B21: turning agent summaries off ends the running request, asks
/// for nothing and takes every AI field off the row; turning them back on
/// shows the stored label and asks for the current turn only.
#[test]
fn turning_summaries_off_ends_the_request_and_on_asks_for_the_current_turn() {
    let harness = Harness::new();
    let store = harness.store();
    let (mut worker, woken, _) = harness.worker(Arc::clone(&store));
    let first = [("user", "파서 버그 고쳐줘"), ("assistant", "고쳤습니다")];
    let path = harness.session("a", "native-a", &first);
    harness.backend.answer("파서 버그 수정 작업", "done", "");
    let idle = agent(&path, "idle", 3);
    observe(&mut worker, &idle);
    settle(&mut worker, &woken);
    assert!(shown(&worker, &idle).is_some());

    harness.session(
        "a",
        "native-a",
        &[first[0], first[1], ("user", "테스트도 돌려줘")],
    );
    harness.backend.hold();
    let working = agent(&path, "working", 4);
    observe(&mut worker, &working);
    let deadline = Instant::now() + Duration::from_secs(20);
    while harness.backend.calls() < 2 {
        assert!(Instant::now() < deadline, "the turn was not asked");
        let _ = woken.recv_timeout(Duration::from_millis(50));
        worker.drain(Instant::now(), unix_now_ms());
    }
    assert!(worker.set_summaries(false, Instant::now()));
    settle(&mut worker, &woken);
    harness.backend.release();
    assert_eq!(shown(&worker, &working), None, "no AI field while off");
    worker.tick(Instant::now());
    assert_eq!(harness.backend.calls(), 2, "nothing is asked while off");
    let record = &store.target(LOCAL_TARGET)["w1:p1"];
    assert_eq!(record.goal.as_deref(), Some("파서 버그 수정 작업"));

    harness.backend.answer("파서 버그 수정 작업", "working", "");
    assert!(worker.set_summaries(true, Instant::now()));
    assert_eq!(
        task(shown(&worker, &working)).as_deref(),
        Some("파서 버그 수정 작업")
    );
    settle(&mut worker, &woken);
    assert_eq!(harness.backend.calls(), 3, "the current turn, once");
}

/// D-19: a transcript that starts over is judged again against the submits
/// still kept, so the operator's request stays the operator's.
#[test]
fn a_restarted_transcript_keeps_the_operators_request_the_operators() {
    let harness = Harness::new();
    let store = harness.store();
    let (mut worker, woken, _) = harness.worker(Arc::clone(&store));
    let turns = [
        ("user", "요청 보기 만들어줘"),
        ("assistant", "만들었어요"),
        ("assistant", "테스트도 돌렸어요"),
    ];
    let path = harness.session("a", "native-a", &turns);
    harness.input.record("w1:p1", turn_at(0) - 300, false);
    harness.backend.answer("요청 보기 화면 만들기", "done", "");
    observe(&mut worker, &agent(&path, "idle", 3));
    settle(&mut worker, &woken);
    let requester = |store: &LabelStore| {
        store.target(LOCAL_TARGET)["w1:p1"]
            .facts
            .operator_request
            .as_ref()
            .map(|request| request.requester.clone())
    };
    assert_eq!(requester(&store), Some(Requester::Operator));

    // The same file, shorter: the reader starts it over.
    harness.session("a", "native-a", &turns[..2]);
    harness.backend.answer("요청 보기 화면 만들기", "done", "");
    observe(&mut worker, &agent(&path, "idle", 4));
    settle_with_ticks(&mut worker, &woken);
    assert_eq!(requester(&store), Some(Requester::Operator));
    assert!(
        store.target(LOCAL_TARGET)["w1:p1"]
            .facts
            .other_request
            .is_none(),
        "the request is not read as another agent's"
    );
}

/// A Claude session whose request is followed by one tool output per entry
/// of `outputs` (its text and when it was printed), then a reply.
fn tool_session(harness: &Harness, name: &str, outputs: &[(String, u64)]) -> PathBuf {
    let project = harness.home.path().join(".claude/projects/-project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join(format!("{name}.jsonl"));
    let first = outputs.first().map_or(1_000, |(_, at)| at - 1_000);
    let last = outputs.last().map_or(2_000, |(_, at)| at + 1_000);
    let mut records = vec![json!({"type":"user","sessionId":name,"timestamp":first,
        "origin":{"kind":"human"},"message":{"role":"user","content":"PR 올려줘"}})];
    for (index, (text, at)) in outputs.iter().enumerate() {
        records.push(json!({"type":"user","sessionId":name,"timestamp":at,
            "message":{"role":"user","content":[{"type":"tool_result",
            "tool_use_id":format!("t{index}"),"content":text}]}}));
    }
    records.push(json!({"type":"assistant","sessionId":name,"timestamp":last,
        "message":{"role":"assistant","content":[{"type":"text","text":"올렸습니다"}]}}));
    let lines: String = records.iter().map(|record| format!("{record}\n")).collect();
    std::fs::write(&path, lines).unwrap();
    path
}

fn addresses(sighted: &[super::worker::SightedPullRequest]) -> Vec<(&str, &str, u64)> {
    sighted
        .iter()
        .map(|sighting| {
            (
                sighting.pane_id.as_str(),
                sighting.repository.as_str(),
                sighting.number,
            )
        })
        .collect()
}

#[test]
fn a_read_hands_on_each_pull_request_the_core_has_not_read_once() {
    let harness = Harness::new();
    let (mut worker, woken, _) = harness.worker(harness.store());
    let now = unix_now_ms();
    let path = tool_session(
        &harness,
        "native-pr",
        &[(
            "https://github.com/Owner/Repo/pull/12\nhttps://github.com/owner/repo/pull/3"
                .to_owned(),
            now - 60_000,
        )],
    );
    worker.set_pull_request_times(Arc::new(super::facts::PullRequestTimes::from([(
        ("owner/repo".to_owned(), 3),
        1,
    )])));
    harness
        .backend
        .answer("풀 리퀘스트 올리기 작업", "done", "");
    let idle = agent(&path, "idle", 3);

    observe(&mut worker, &idle);
    settle(&mut worker, &woken);
    assert_eq!(
        addresses(&worker.take_sighted()),
        vec![("w1:p1", "owner/repo", 12)],
        "the address the core holds stays; the other is handed on once, in lowercase"
    );
    assert!(worker.take_sighted().is_empty());
}

#[test]
fn a_read_keeps_the_newest_recent_sightings_and_reports_the_rest() {
    let harness = Harness::new();
    let (mut worker, woken, _) = harness.worker(harness.store());
    let now = unix_now_ms();
    let output = |numbers: std::ops::RangeInclusive<u64>| {
        numbers
            .map(|number| format!("https://github.com/owner/repo/pull/{number}\n"))
            .collect::<String>()
    };
    // One old address, then five outputs of eight, oldest first.
    let mut outputs = vec![(output(99..=99), now - 20 * 60 * 1_000)];
    for batch in 0..5u64 {
        outputs.push((
            output(batch * 8 + 1..=batch * 8 + 8),
            now - 50_000 + batch * 10_000,
        ));
    }
    let path = tool_session(&harness, "native-many", &outputs);
    harness
        .backend
        .answer("풀 리퀘스트 올리기 작업", "done", "");
    let idle = agent(&path, "idle", 3);

    observe(&mut worker, &idle);
    let ((), records) = crate::diagnostics::capture(|| settle(&mut worker, &woken));
    let mut kept: Vec<u64> = worker
        .take_sighted()
        .iter()
        .map(|sighting| sighting.number)
        .collect();
    kept.sort_unstable();
    assert_eq!(
        kept,
        (9..=40).collect::<Vec<_>>(),
        "the newest 32, and not the old one"
    );
    let capped: Vec<_> = records
        .iter()
        .filter(|record| record["kind"] == "read.sighted_capped")
        .collect();
    assert_eq!(capped.len(), 1, "{records:?}");
    assert_eq!(
        capped[0]["dropped"], 8,
        "the old address is skipped, not dropped"
    );
}

#[test]
fn a_devices_worker_keeps_no_sightings() {
    let harness = Harness::new();
    let helper: Arc<dyn crate::node_access::NodeLink> =
        Arc::new(HelperAt(harness.home.path().to_path_buf()));
    let (mut worker, woken) = harness.device_worker(Box::new(move || Ok(Arc::clone(&helper))));
    let path = tool_session(
        &harness,
        "native-device",
        &[(
            "https://github.com/owner/repo/pull/12".to_owned(),
            unix_now_ms() - 60_000,
        )],
    );
    harness
        .backend
        .answer("풀 리퀘스트 올리기 작업", "done", "");
    let idle = agent(&path, "idle", 3);

    observe(&mut worker, &idle);
    settle(&mut worker, &woken);
    assert_eq!(
        task(shown(&worker, &idle)).as_deref(),
        Some("풀 리퀘스트 올리기 작업"),
        "the session was read"
    );
    assert!(
        worker.take_sighted().is_empty(),
        "a device's pull requests are not read on this Mac"
    );
}

/// A Codex rollout under the harness home whose last turn ran in plan mode,
/// proposed a plan and finished (PRD codex-plan-approval-hold D-03), and the
/// agent Herdr lists for it.
fn codex_plan_session(harness: &Harness, status: &str, seq: u64) -> (PathBuf, ObservedAgent) {
    let id = "0199a000-0000-7000-8000-0000000000b2";
    let folder = harness.home.path().join(".codex/sessions/2026/10/07");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(format!("rollout-2026-10-07T01-00-00-{id}.jsonl"));
    let records = [
        json!({"timestamp":"2026-10-07T01:00:00.000Z","type":"session_meta",
            "payload":{"id":id,"cwd":"/work/app","cli_version":"0.160.1"}}),
        codex_event(
            "task_started",
            "turn-1",
            json!({"collaboration_mode_kind":"plan"}),
        ),
        json!({"timestamp":"2026-10-07T01:00:01.000Z","type":"response_item",
            "payload":{"type":"message","role":"user",
                "content":[{"type":"input_text","text":"계획을 세워줘"}]}}),
        codex_event(
            "item_completed",
            "turn-1",
            json!({"item":{"type":"Plan","id":"i1","text":"1. 고친다"}}),
        ),
        codex_event(
            "task_complete",
            "turn-1",
            json!({"last_agent_message":null}),
        ),
    ];
    let lines: String = records.iter().map(|record| format!("{record}\n")).collect();
    std::fs::write(&path, lines).unwrap();
    let agent = ObservedAgent {
        agent: Some("codex".to_owned()),
        ..agent(&path, status, seq)
    };
    (path, agent)
}

fn codex_event(kind: &str, turn: &str, extra: Value) -> Value {
    let mut payload = json!({"type": kind, "turn_id": turn});
    payload
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    json!({"timestamp":"2026-10-07T01:00:02.000Z","type":"event_msg","payload":payload})
}

/// What the overlay says the agent waits for, and whether its row carries
/// the approval wait (`None` when no read of its session is laid at all),
/// for the agent as Herdr lists it now.
fn waits(worker: &LabelWorker, agent: &ObservedAgent) -> (Option<Waiting>, Option<bool>) {
    let (kind, value) = agent.reference.clone().unwrap();
    let mut payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents": [{
        "pane_id": agent.pane_id, "agent": agent.agent, "agent_status": agent.status,
        "state_change_seq": agent.state_change_seq,
        "agent_session": {"kind": kind, "value": value},
    }]}))
    .unwrap();
    let overlay = worker.overlay();
    let waiting = overlay.waiting(&payload.agents[0]);
    overlay.apply(&mut payload);
    let row = payload
        .agents
        .remove(0)
        .facts
        .map(|facts| facts.awaiting_operator);
    (waiting, row)
}

/// D-02, D-06: the wait is read without any AI, and it holds for the Herdr
/// state it was read under only; a newer state is not known until read.
#[test]
fn a_codex_plan_wait_is_known_only_for_the_state_it_was_read_under() {
    let harness = Harness::new();
    let (mut worker, woken, _) = harness.worker(harness.store());
    worker.set_summaries(false, Instant::now());
    let (path, done) = codex_plan_session(&harness, "done", 5);
    observe(&mut worker, &done);
    settle(&mut worker, &woken);
    assert_eq!(
        waits(&worker, &done),
        (Some(Waiting::PlanApproval), Some(true))
    );
    assert_eq!(harness.backend.calls(), 0, "no analysis was asked");

    // Herdr moves on before the read of the new state lands.
    let working = ObservedAgent {
        status: Some("working".to_owned()),
        state_change_seq: 6,
        ..done.clone()
    };
    assert_eq!(waits(&worker, &working), (None, Some(false)));

    // Approving starts the next turn; read for that state, nothing waits.
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    use std::io::Write;
    writeln!(
        file,
        "{}",
        codex_event(
            "task_started",
            "turn-2",
            json!({"collaboration_mode_kind":"default"})
        )
    )
    .unwrap();
    observe(&mut worker, &working);
    settle(&mut worker, &woken);
    assert_eq!(
        waits(&worker, &working),
        (Some(Waiting::Nothing), Some(false))
    );
}

/// B5: a default-mode turn whose end Codex has not written yet, when Herdr
/// already reads done, waits for nothing; only a plan-mode turn could wait.
#[test]
fn a_default_turn_not_yet_ended_in_the_file_waits_for_nothing() {
    let harness = Harness::new();
    let (mut worker, woken, _) = harness.worker(harness.store());
    worker.set_summaries(false, Instant::now());
    let (path, _) = codex_plan_session(&harness, "done", 5);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    use std::io::Write;
    writeln!(
        file,
        "{}",
        codex_event(
            "task_started",
            "turn-2",
            json!({"collaboration_mode_kind":"default"})
        )
    )
    .unwrap();
    let done = ObservedAgent {
        agent: Some("codex".to_owned()),
        ..agent(&path, "done", 6)
    };
    observe(&mut worker, &done);
    settle(&mut worker, &woken);
    assert_eq!(waits(&worker, &done), (Some(Waiting::Nothing), Some(false)));
}

/// B1, B3: Herdr can read Codex at rest before its session file records the
/// end of the plan turn. The read for that state is not settled, so the
/// session is read again shortly, a bounded number of times, and the wait
/// is found without waiting for Herdr's state to move.
#[test]
fn a_plan_turn_the_file_has_not_ended_yet_is_read_again_until_it_settles() {
    let harness = Harness::new();
    let (mut worker, woken, source) = harness.worker(harness.store());
    worker.set_summaries(false, Instant::now());
    let (path, done) = codex_plan_session(&harness, "done", 5);
    let whole = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = whole.split_inclusive('\n').collect();
    let (unfinished, end) = lines.split_at(lines.len() - 1);
    std::fs::write(&path, unfinished.concat()).unwrap();
    observe(&mut worker, &done);
    settle(&mut worker, &woken);
    assert_eq!(waits(&worker, &done), (None, Some(false)));

    // Codex writes the end of the turn; Herdr's state does not move.
    std::fs::write(&path, [unfinished.concat(), end.concat()].concat()).unwrap();
    worker.tick(Instant::now() + Duration::from_secs(4));
    settle(&mut worker, &woken);
    assert_eq!(source.reads.load(Ordering::SeqCst), 2);
    assert_eq!(
        waits(&worker, &done),
        (Some(Waiting::PlanApproval), Some(true))
    );

    // A file that never settles is read again only a few times per state.
    std::fs::write(&path, unfinished.concat()).unwrap();
    let later = ObservedAgent {
        state_change_seq: 6,
        ..done.clone()
    };
    observe(&mut worker, &later);
    settle(&mut worker, &woken);
    for step in 1..=6 {
        worker.tick(Instant::now() + Duration::from_secs(4 * step));
        settle(&mut worker, &woken);
    }
    assert_eq!(source.reads.load(Ordering::SeqCst), 2 + 1 + 3);
    assert_eq!(waits(&worker, &later), (None, Some(false)));
}

/// B8: a restarted daemon shows the wait it read for the same state without
/// reading the session again; a record from before turns were read is read
/// once, and does not claim a wait it could not read.
#[test]
fn a_restart_keeps_the_plan_wait_and_reads_a_record_without_one_once() {
    let harness = Harness::new();
    let (_, done) = codex_plan_session(&harness, "done", 5);
    {
        let (mut worker, woken, _) = harness.worker(harness.store());
        worker.set_summaries(false, Instant::now());
        observe(&mut worker, &done);
        settle(&mut worker, &woken);
    }
    let (mut worker, woken, source) = harness.worker(harness.store());
    worker.set_summaries(false, Instant::now());
    observe(&mut worker, &done);
    assert_eq!(
        waits(&worker, &done),
        (Some(Waiting::PlanApproval), Some(true))
    );
    settle(&mut worker, &woken);
    assert_eq!(source.reads.load(Ordering::SeqCst), 0);
    drop(worker);

    // The same record as an older daemon wrote it, with no turn read.
    let store = harness.store();
    let mut records = store.target(LOCAL_TARGET);
    let record = records.get_mut(&done.pane_id).unwrap();
    record.turns = None;
    record.turns_seq = None;
    store.save_target(LOCAL_TARGET, &records);
    let (mut worker, woken, source) = harness.worker(harness.store());
    worker.set_summaries(false, Instant::now());
    observe(&mut worker, &done);
    assert_eq!(waits(&worker, &done), (None, Some(false)));
    settle(&mut worker, &woken);
    assert_eq!(source.reads.load(Ordering::SeqCst), 1);
    // That read resumes at the person's message, after the turn's start, so
    // the turn's mode is not read: the wait stays not known (the bell holds)
    // until Herdr's next state is read.
    assert_eq!(waits(&worker, &done), (None, Some(false)));
}

/// A device helper that predates turn reads: its answer has no `turns`.
struct HelperWithoutTurns(PathBuf);

impl crate::node_access::NodeLink for HelperWithoutTurns {
    fn call(
        &self,
        call: hide_node_link::protocol::Call,
        timeout: Duration,
    ) -> Result<crate::node_access::LinkAnswer, crate::node_access::LinkError> {
        match call {
            hide_node_link::protocol::Call::LabelTranscript { request } => {
                hide_host::serve::label_transcript(&self.0, &request)
                    .map(|mut answer| {
                        answer.as_object_mut().unwrap().remove("turns");
                        crate::node_access::LinkAnswer::Parsed(answer)
                    })
                    .map_err(crate::node_access::LinkError::Refused)
            }
            call => hide_node::Local::of_process().call(call, timeout),
        }
    }

    fn in_process(&self) -> bool {
        true
    }
}

/// D-09, B9 and B6: a device's Codex pane is read through its helper the
/// same way, and a helper that does not report turns leaves the wait not
/// known rather than "nothing".
#[test]
fn a_device_codex_plan_wait_comes_through_its_helper_and_an_older_helper_is_not_known() {
    let harness = Harness::new();
    let (_, done) = codex_plan_session(&harness, "done", 5);
    {
        let helper: Arc<dyn crate::node_access::NodeLink> =
            Arc::new(HelperAt(harness.home.path().to_path_buf()));
        let (mut worker, woken) = harness.device_worker(Box::new(move || Ok(Arc::clone(&helper))));
        worker.set_summaries(false, Instant::now());
        observe(&mut worker, &done);
        settle(&mut worker, &woken);
        assert_eq!(
            waits(&worker, &done),
            (Some(Waiting::PlanApproval), Some(true))
        );
    }

    // The generator lock is free again, so this worker reads.
    let older: Arc<dyn crate::node_access::NodeLink> =
        Arc::new(HelperWithoutTurns(harness.home.path().to_path_buf()));
    let (mut worker, woken) = harness.device_worker(Box::new(move || Ok(Arc::clone(&older))));
    worker.set_summaries(false, Instant::now());
    observe(&mut worker, &done);
    settle(&mut worker, &woken);
    // The read landed and proved the session; only the wait is not known.
    assert_eq!(waits(&worker, &done), (None, Some(false)));
}

/// B6: a Codex session whose file is not there yet is not known to wait for
/// nothing; once the file is written, the next state reads it.
#[test]
fn a_codex_session_not_found_is_not_known_until_a_later_state_reads_it() {
    let harness = Harness::new();
    let (mut worker, woken, source) = harness.worker(harness.store());
    worker.set_summaries(false, Instant::now());
    let by_id = |seq: u64| ObservedAgent {
        pane_id: "w1:p1".to_owned(),
        agent: Some("codex".to_owned()),
        status: Some("idle".to_owned()),
        reference: Some((
            "id".to_owned(),
            "0199a000-0000-7000-8000-0000000000b2".to_owned(),
        )),
        cwd: None,
        state_change_seq: seq,
    };
    observe(&mut worker, &by_id(2));
    settle(&mut worker, &woken);
    assert_eq!(source.reads.load(Ordering::SeqCst), 1);
    assert_eq!(waits(&worker, &by_id(2)), (None, None));

    codex_plan_session(&harness, "idle", 3);
    observe(&mut worker, &by_id(3));
    settle(&mut worker, &woken);
    assert_eq!(source.reads.load(Ordering::SeqCst), 2);
    assert_eq!(
        waits(&worker, &by_id(3)),
        (Some(Waiting::PlanApproval), Some(true))
    );
}
