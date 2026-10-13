use super::*;

fn session() -> SessionIdentity {
    SessionIdentity {
        device_id: "device-a".into(),
        agent: "codex".into(),
        session_id: "session-a".into(),
    }
}

fn request(store: &mut LinkStore, offset: u64, text: &str) {
    store
        .apply_ledger_update(&LedgerUpdate::Requests {
            session: session(),
            cwd: "/repo".into(),
            path: None,
            requests: vec![RecordedRequest {
                offset,
                at_unix_ms: offset * 1_000,
                text: text.into(),
                author: WorkAuthor::Operator,
                task_ids: vec![],
                chapter_id: None,
                judged: false,
            }],
        })
        .unwrap();
}

fn chapter(store: &mut LinkStore) {
    store
        .apply_ledger_update(&LedgerUpdate::Chapter {
            session: session(),
            goal: "세션 작업 기록".into(),
            at_unix_ms: 1_000,
        })
        .unwrap();
}

fn end(store: &mut LinkStore, offset: u64, judgment: TurnJudgment) {
    store
        .apply_ledger_update(&LedgerUpdate::TurnEnd {
            session: session(),
            offset,
            at_unix_ms: offset * 1_000 + 500,
            end: LabelEnd::Done,
            work_author: true,
            judgment,
        })
        .unwrap();
}

#[test]
fn a_v1_store_keeps_its_sessions_and_links_through_migration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("links.sqlite3");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch(super::super::SCHEMA).unwrap();
    connection
        .execute_batch(
            "PRAGMA user_version=1; INSERT INTO sessions(device,agent,id,last_request) \
        VALUES('device-a','codex','session-a','legacy request');",
        )
        .unwrap();
    drop(connection);
    let (store, opened) = LinkStore::open(&path).unwrap();
    assert_eq!(opened, super::super::Opened::Existing);
    assert_eq!(
        store
            .connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        store
            .connection
            .query_row("SELECT last_request FROM sessions", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "legacy request"
    );
    assert!(store.ledger_record(&session()).unwrap().tasks.is_empty());
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.pragma_update(None, "user_version", 3).unwrap();
    drop(connection);
    assert_eq!(LinkStore::open(&path).err().unwrap(), "links_store_newer");
}

#[test]
fn questions_stay_requests_and_summary_off_search_excludes_judgments() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _) = LinkStore::open(&directory.path().join("links.sqlite3")).unwrap();
    request(&mut store, 1, "기록 화면을 만들어줘");
    chapter(&mut store);
    end(
        &mut store,
        1,
        TurnJudgment {
            task: TaskDecision::New {
                name: "기록 화면".into(),
            },
            ..Default::default()
        },
    );
    request(&mut store, 2, "어떻게 작동해?");
    end(&mut store, 2, TurnJudgment::default());
    let record = store.ledger_record(&session()).unwrap();
    assert_eq!(record.tasks.len(), 1);
    assert_eq!(record.requests.len(), 2);
    assert!(record.requests[0].task_ids.is_empty());
    assert!(!store.ledger_search("세션 작업", true).unwrap().is_empty());
    assert!(store.ledger_search("세션 작업", false).unwrap().is_empty());
    assert_eq!(
        store.ledger_search("어떻게", false).unwrap()[0].matched,
        "어떻게 작동해?"
    );
    assert!(
        store.ledger_search("%", false).unwrap().is_empty(),
        "query text is not a wildcard"
    );
}

#[test]
fn turn_replay_and_status_undo_survive_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("links.sqlite3");
    let (mut store, _) = LinkStore::open(&path).unwrap();
    request(&mut store, 1, "화면을 만들어줘");
    chapter(&mut store);
    let judgment = TurnJudgment {
        task: TaskDecision::New {
            name: "화면".into(),
        },
        missed: vec![NewMissedWork {
            kind: MissedKind::Problem,
            text: "모바일 줄바꿈이 남음".into(),
        }],
        ..Default::default()
    };
    end(&mut store, 1, judgment.clone());
    end(&mut store, 1, judgment);
    let record = store.ledger_record(&session()).unwrap();
    assert_eq!(record.tasks.len(), 1);
    assert_eq!(record.missed.len(), 1);
    let id = record.missed[0].id.clone();
    store
        .apply_ledger_update(&LedgerUpdate::MissedStatus {
            session: session(),
            id: id.clone(),
            status: MissedStatus::Dismissed,
        })
        .unwrap();
    drop(store);
    let (mut store, _) = LinkStore::open(&path).unwrap();
    assert_eq!(
        store.ledger_record(&session()).unwrap().missed[0].status,
        MissedStatus::Dismissed
    );
    store
        .apply_ledger_update(&LedgerUpdate::MissedStatus {
            session: session(),
            id,
            status: MissedStatus::Open,
        })
        .unwrap();
    assert_eq!(
        store.ledger_context(&session()).unwrap().open_missed.len(),
        1
    );
}

#[test]
fn a_done_task_waits_for_every_assigned_pr_to_merge() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _) = LinkStore::open(&directory.path().join("links.sqlite3")).unwrap();
    request(&mut store, 1, "첫 화면");
    chapter(&mut store);
    end(
        &mut store,
        1,
        TurnJudgment {
            task: TaskDecision::New {
                name: "첫 화면".into(),
            },
            ..Default::default()
        },
    );
    assert!(
        store.ledger_record(&session()).unwrap().tasks[0].done,
        "no PR satisfies the empty requirement"
    );
    store
        .connection
        .execute(
            "INSERT INTO ledger_outputs VALUES('device-a','codex','session-a','pr','owner/repo',7, \
        'First screen','https://github.com/owner/repo/pull/7','open',1200,1200,'task-1')",
            [],
        )
        .unwrap();
    assert!(!store.ledger_record(&session()).unwrap().tasks[0].done);
    store
        .connection
        .execute("UPDATE ledger_outputs SET state='closed'", [])
        .unwrap();
    assert!(
        !store.ledger_record(&session()).unwrap().tasks[0].done,
        "closed without merge is not landed"
    );
    store
        .connection
        .execute("UPDATE ledger_outputs SET state='merged'", [])
        .unwrap();
    assert!(store.ledger_record(&session()).unwrap().tasks[0].done);
}

#[test]
fn request_retention_discards_the_oldest_and_keeps_recent_questions() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _) = LinkStore::open(&directory.path().join("links.sqlite3")).unwrap();
    let requests = (1..=REQUEST_LIMIT as u64)
        .map(|offset| RecordedRequest {
            offset,
            at_unix_ms: offset * 1_000,
            text: format!("request {offset}"),
            author: WorkAuthor::Operator,
            task_ids: vec![],
            chapter_id: None,
            judged: false,
        })
        .collect();
    store
        .apply_ledger_update(&LedgerUpdate::Requests {
            session: session(),
            cwd: "/repo".into(),
            path: None,
            requests,
        })
        .unwrap();
    request(&mut store, REQUEST_LIMIT as u64 + 1, "recent question");
    let record = store.ledger_record(&session()).unwrap();
    assert_eq!(record.requests.len(), REQUEST_LIMIT);
    assert_eq!(record.requests[0].text, "recent question");
    assert!(record.requests.iter().all(|request| request.offset != 1));
}
