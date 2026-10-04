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
use serde_json::{Value, json};

use super::DeviceTranscripts;
use super::analyzer::LabelAnalyzer;
use super::facts::Requester;
use super::input::OperatorInput;
use super::store::{LOCAL_TARGET, LabelStore};
use super::worker::{
    LabelWorker, LocalTranscripts, ObservedAgent, ReadFailure, TranscriptSource, WorkerConfig,
};
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
        ProviderId::Claude
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
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
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
    inner: LocalTranscripts,
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
            Box::new(AiSettings::default),
            Box::new(move |_| {
                Arc::new(AiRouter::new(
                    vec![Arc::clone(&router_backend)],
                    RouterConfig {
                        priority: vec![ProviderId::Claude],
                        max_transient_attempts: 1,
                        ..RouterConfig::default()
                    },
                    Arc::new(NoopLogSink),
                ))
            }),
        )
        .unwrap();
        Self {
            home: tempfile::tempdir().unwrap(),
            state: tempfile::tempdir().unwrap(),
            locks: tempfile::tempdir().unwrap(),
            backend,
            analyzer: Arc::new(analyzer),
            input: Arc::default(),
        }
    }

    fn store(&self) -> Arc<LabelStore> {
        Arc::new(LabelStore::open(
            Some(self.state.path()),
            Some(self.home.path()),
        ))
    }

    fn worker(&self, store: Arc<LabelStore>) -> (LabelWorker, Receiver<()>, Arc<CountingSource>) {
        let source = Arc::new(CountingSource {
            inner: LocalTranscripts {
                home: self.home.path().to_path_buf(),
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
            Arc::new(DeviceTranscripts::new(channel)),
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

/// Takes results until nothing is read or analyzed.
fn settle(worker: &mut LabelWorker, woken: &Receiver<()>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !worker.settled() {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "the worker did not settle");
        let _ = woken.recv_timeout(left.min(Duration::from_millis(200)));
        worker.drain(Instant::now());
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
        worker.drain(Instant::now());
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
        worker.drain(Instant::now());
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

impl crate::host_access::HostChannel for OlderHelper {
    fn call(
        &self,
        _: hide_host::protocol::Call,
        _: Duration,
    ) -> Result<crate::host_access::HostAnswer, crate::host_access::HostCallError> {
        Err(crate::host_access::HostCallError::Refused(
            hide_host::error::HostError::new(
                hide_host::error::ErrorCode::InvalidRequest,
                "unknown variant `label_transcript`",
            ),
        ))
    }
}

/// The device helper in process, reading transcripts under its own HOME
/// (the harness's), as `hide-host-helper` does under the device's.
struct HelperAt(PathBuf);

impl crate::host_access::HostChannel for HelperAt {
    fn call(
        &self,
        call: hide_host::protocol::Call,
        timeout: Duration,
    ) -> Result<crate::host_access::HostAnswer, crate::host_access::HostCallError> {
        match call {
            hide_host::protocol::Call::LabelTranscript { request } => {
                hide_host::serve::label_transcript(&self.0, &request)
                    .map(crate::host_access::HostAnswer::Parsed)
                    .map_err(crate::host_access::HostCallError::Refused)
            }
            call => crate::host_access::InProcessHost.call(call, timeout),
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
    let helper: Arc<dyn crate::host_access::HostChannel> =
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
    let helper: Arc<dyn crate::host_access::HostChannel> =
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
        Ok(Arc::new(OlderHelper) as Arc<dyn crate::host_access::HostChannel>)
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
        provider: ProviderId::Claude,
        ..AiSettings::default()
    };
    hide_ai::settings::save(home.path(), &chosen).unwrap();
    let no_runtime = std::sync::Weak::new();
    assert_eq!(
        super::analysis_settings(&no_runtime, Some(home.path())).provider,
        ProviderId::Claude
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
    let reopened = LabelStore::open(Some(harness.state.path()), None);
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
        machine_id: None,
        herdr_socket_path: None,
        herdr_bin_path: None,
        app_state_path: state.path().join("core-state.json").display().to_string(),
        host_helper_dir: None,
        host_helper_root: None,
        host_cli_dir: None,
        workspace_views_path: None,
        shortcut_import_path: None,
        local_issues_path: None,
        kit_dir: None,
    };
    assert!(
        crate::Core::create(options("relative/home".to_owned())).is_none(),
        "a relative home names no account folder"
    );
    let core =
        crate::Core::create(options(home.path().display().to_string())).expect("a core starts");
    drop(core);
    let imported = LabelStore::open(Some(state.path()), None).target(LOCAL_TARGET);
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
        worker.drain(Instant::now());
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
