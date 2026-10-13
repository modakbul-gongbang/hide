use super::*;
use hide_node_link::{LinkAnswer, LinkError, sessions::ReaderFeatures};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct OwnNode {
    env: hide_host::serve::Env,
    features: ReaderFeatures,
    calls: AtomicUsize,
    fail_read: AtomicBool,
}

impl OwnNode {
    fn new(home: &std::path::Path) -> Self {
        Self {
            env: hide_host::serve::Env::standalone(Some(home.to_owned())),
            features: ReaderFeatures::implemented(),
            calls: AtomicUsize::new(0),
            fail_read: AtomicBool::new(false),
        }
    }
}

impl NodeLink for OwnNode {
    fn reader_features(&self) -> Option<&ReaderFeatures> {
        Some(&self.features)
    }
    fn call(&self, call: Call, _: Duration) -> Result<LinkAnswer, LinkError> {
        assert!(matches!(
            call,
            Call::UsageFiles { .. } | Call::UsageRead { .. }
        ));
        self.calls.fetch_add(1, Ordering::SeqCst);
        if matches!(call, Call::UsageRead { .. }) && self.fail_read.load(Ordering::SeqCst) {
            return Err(LinkError::Unknown(
                "private path and body never reach diagnostics".into(),
            ));
        }
        hide_host::serve::handle_in(call, &self.env)
            .map(LinkAnswer::Parsed)
            .map_err(LinkError::Refused)
    }
}

fn fixture(home: &std::path::Path, id: &str) -> PathBuf {
    let folder = home.join(hide_session::CLAUDE_SESSIONS);
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(format!("{id}.jsonl"));
    let line = serde_json::json!({"type":"assistant", "sessionId":"fixture", "requestId":id, "timestamp":jiff::Timestamp::now().to_string(), "message":{"id":id,"model":"fixture-model","content":[{"text":"PRIVATE_BODY_SENTINEL"}],"usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":900,"cache_creation_input_tokens":30}}});
    std::fs::write(&path, format!("{line}\n")).unwrap();
    path
}

fn finish(engine: &mut Engine, node: &OwnNode) -> Report {
    for _ in 0..300 {
        engine.step(
            node,
            Activity {
                visible: true,
                open: true,
            },
        );
        if engine.agents.iter().all(|work| !work.progress.reading) {
            return engine.report();
        }
    }
    panic!("fixture backfill did not complete");
}

#[test]
fn usage_worker_resumes_a_partly_read_unchanged_file_after_restart() {
    let home = tempfile::tempdir().unwrap();
    let source = fixture(home.path(), "first");
    let template: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&source).unwrap()).unwrap();
    let mut body = String::new();
    for i in 0..5000 {
        let mut line = template.clone();
        line["requestId"] = format!("request-{i}").into();
        line["message"]["id"] = format!("message-{i}").into();
        body.push_str(&line.to_string());
        body.push('\n');
    }
    std::fs::write(&source, body).unwrap();
    let path = home.path().join("ledger.sqlite3");
    let node = OwnNode::new(home.path());
    let mut first = Engine::open(&path, "physical-device".into()).unwrap();
    for _ in 0..100 {
        first.step(
            &node,
            Activity {
                visible: true,
                open: true,
            },
        );
        if first.agents.iter().any(|work| {
            work.active
                .as_ref()
                .is_some_and(|(_, saved)| saved.is_some())
        }) {
            break;
        }
    }
    let partial = first
        .store
        .daily()
        .unwrap()
        .iter()
        .map(|row| row.totals.calls)
        .sum::<u64>();
    assert!(partial > 0 && partial < 5000);
    drop(first);
    let mut restarted = Engine::open(&path, "physical-device".into()).unwrap();
    // A transient refusal after that persisted partial read must not turn
    // its unchanged stamp into an EOF proof either.
    node.fail_read.store(true, Ordering::SeqCst);
    for _ in 0..100 {
        restarted.step(
            &node,
            Activity {
                visible: true,
                open: true,
            },
        );
        if restarted
            .agents
            .iter()
            .any(|work| work.progress.failure.is_some())
        {
            break;
        }
    }
    assert!(restarted.agents.iter().any(|work| work.progress.reading && work.progress.failure == Some("usage_read_failed")));
    drop(restarted);
    node.fail_read.store(false, Ordering::SeqCst);
    let mut restarted = Engine::open(&path, "physical-device".into()).unwrap();
    assert_eq!(
        finish(&mut restarted, &node)
            .projection
            .unwrap()
            .year
            .all
            .amount
            .calls,
        5000
    );
}

#[test]
fn usage_worker_keeps_totals_after_restart_copy_and_source_deletion() {
    let home = tempfile::tempdir().unwrap();
    let source = fixture(home.path(), "first");
    let path = home.path().join("ledger.sqlite3");
    let node = OwnNode::new(home.path());
    let mut first = Engine::open(&path, "physical-device".into()).unwrap();
    let report = finish(&mut first, &node);
    let amount = &report.projection.as_ref().unwrap().year.all.amount;
    assert_eq!(amount.calls, 1);
    assert_eq!(amount.new_tokens, 60);
    assert_eq!(amount.tokens.cache_read, 900);
    assert!(
        !serde_json::to_string(report.projection.as_ref().unwrap().as_ref())
            .unwrap()
            .contains("PRIVATE_BODY_SENTINEL")
    );
    drop(first);

    let copy = source.with_file_name("copied.jsonl");
    std::fs::copy(&source, &copy).unwrap();
    std::fs::remove_file(&source).unwrap();
    let mut restarted = Engine::open(&path, "physical-device".into()).unwrap();
    for _ in 0..100 {
        restarted.step(
            &node,
            Activity {
                visible: true,
                open: true,
            },
        );
    }
    assert_eq!(
        &restarted.report().projection.unwrap().year.all.amount,
        amount
    );
    assert!(
        restarted
            .store
            .checkpoint("physical-device", Agent::Claude, source.to_str().unwrap())
            .unwrap()
            .is_none(),
        "gone files release cursor capacity while their totals remain"
    );
    std::fs::remove_file(copy).unwrap();
    drop(restarted);
    assert_eq!(
        Engine::open(&path, "physical-device".into())
            .unwrap()
            .report()
            .projection
            .unwrap()
            .year
            .all
            .amount,
        *amount
    );
}

#[test]
fn usage_worker_refusal_keeps_backfill_open_and_recovers_from_durable_progress() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path(), "first");
    let path = home.path().join("ledger.sqlite3");
    let node = OwnNode::new(home.path());
    node.fail_read.store(true, Ordering::SeqCst);
    let mut first = Engine::open(&path, "physical-device".into()).unwrap();
    for _ in 0..100 {
        first.step(
            &node,
            Activity {
                visible: true,
                open: false,
            },
        );
    }
    let failed = first.report();
    let claude = failed
        .progress
        .iter()
        .find(|row| row.agent == Agent::Claude)
        .unwrap();
    assert!(claude.reading);
    assert_eq!(claude.failure, Some("usage_read_failed"));
    assert_eq!(failed.projection.unwrap().year.all.amount.calls, 0);
    drop(first);
    node.fail_read.store(false, Ordering::SeqCst);
    let mut restarted = Engine::open(&path, "physical-device".into()).unwrap();
    assert_eq!(
        finish(&mut restarted, &node)
            .projection
            .unwrap()
            .year
            .all
            .amount
            .calls,
        1
    );
}

#[test]
fn usage_worker_sleeps_without_windows_and_joins_when_its_owner_ends() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path(), "first");
    let node = Arc::new(OwnNode::new(home.path()));
    let (sent, received) = std::sync::mpsc::channel();
    let worker = Worker::spawn(
        home.path().join("ledger.sqlite3"),
        "physical-device".into(),
        node.clone(),
        move |report| {
            sent.send(report).unwrap();
        },
    )
    .unwrap();
    received.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(node.calls.load(Ordering::SeqCst), 0);
    drop(worker);
    assert!(matches!(
        received.recv_timeout(Duration::from_secs(2)),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
    ));
    assert_eq!(node.calls.load(Ordering::SeqCst), 0);
}
