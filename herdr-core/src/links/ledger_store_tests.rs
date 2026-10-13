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
            turn_witness: None,
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
    let record = store.ledger_record(&session()).unwrap();
    assert!(record.tasks.is_empty());
    assert_eq!(record.requests[0].text, "legacy request");
    assert_eq!(
        store.ledger_search("legacy", false).unwrap()[0].session,
        session()
    );
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

#[test]
fn an_off_or_failed_turn_remains_raw_after_restart_and_a_late_answer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("links.sqlite3");
    let (mut store, _) = LinkStore::open(&path).unwrap();
    request(&mut store, 1, "요약이 꺼진 동안 한 요청");
    chapter(&mut store);
    store
        .apply_ledger_update(&LedgerUpdate::Unjudged {
            session: session(),
            offsets: vec![1],
        })
        .unwrap();
    drop(store);
    let (mut store, _) = LinkStore::open(&path).unwrap();
    end(
        &mut store,
        1,
        TurnJudgment {
            task: TaskDecision::New {
                name: "과거 요청을 뒤늦게 묶으면 안 됨".into(),
            },
            missed: vec![NewMissedWork {
                kind: MissedKind::Leftover,
                text: "과거 보고를 뒤늦게 등록하면 안 됨".into(),
            }],
            ..Default::default()
        },
    );
    let record = store.ledger_record(&session()).unwrap();
    assert!(record.tasks.is_empty());
    assert!(record.missed.is_empty());
    assert_eq!(record.requests[0].text, "요약이 꺼진 동안 한 요청");
    assert!(!record.requests[0].judged);
}

#[test]
fn the_durable_issue_intent_keeps_the_original_attempt_and_closes_without_an_output() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("links.sqlite3");
    let (mut store, _) = LinkStore::open(&path).unwrap();
    request(&mut store, 1, "구현해줘");
    chapter(&mut store);
    end(
        &mut store,
        1,
        TurnJudgment {
            missed: vec![NewMissedWork {
                kind: MissedKind::Problem,
                text: "모바일 검증이 남음".into(),
            }],
            ..Default::default()
        },
    );
    let source = MissedSource {
        session: session(),
        id: store.ledger_record(&session()).unwrap().missed[0]
            .id
            .clone(),
    };
    let reserve = |title: &str, at| LedgerCall::ReserveIssue {
        source: source.clone(),
        repository: "owner/repo".into(),
        title: title.into(),
        body: "본문".into(),
        at_unix_ms: at,
    };
    let LedgerAnswer::Attempt(first) = store.ledger_call(reserve("처음 제목", 2000)).unwrap()
    else {
        panic!("expected attempt")
    };
    assert!(!first.reconcile);
    assert!(store.ledger_record(&session()).unwrap().missed[0].creating);
    assert!(
        store
            .apply_ledger_update(&LedgerUpdate::MissedStatus {
                session: session(),
                id: source.id.clone(),
                status: MissedStatus::Dismissed
            })
            .is_err()
    );
    drop(store);
    let (mut store, _) = LinkStore::open(&path).unwrap();
    store.recover_issue_attempts().unwrap();
    let LedgerAnswer::Attempt(retry) = store.ledger_call(reserve("수정한 제목", 4000)).unwrap()
    else {
        panic!("expected attempt")
    };
    assert!(retry.reconcile);
    assert_eq!(
        (retry.title.as_str(), retry.started_at_unix_ms),
        ("처음 제목", 2000)
    );
    store
        .ledger_call(LedgerCall::FinishIssue {
            source: source.clone(),
            number: 7,
            url: "https://github.com/owner/repo/issues/7".into(),
        })
        .unwrap();
    drop(store);
    let (mut store, _) = LinkStore::open(&path).unwrap();
    let record = store.ledger_record(&session()).unwrap();
    assert_eq!(record.missed[0].status, MissedStatus::Issue);
    assert_eq!(record.missed[0].issue_number, Some(7));
    assert!(
        record.outputs.is_empty(),
        "Hide-created issues attach to missed work, not session outputs (D-35)"
    );
    let LedgerAnswer::Attempt(completed) = store.ledger_call(reserve("다시 누름", 5000)).unwrap()
    else {
        panic!("expected attempt")
    };
    assert_eq!(completed.number, Some(7));
    assert!(
        store
            .ledger_call(LedgerCall::FinishIssue {
                source,
                number: 8,
                url: "https://github.com/owner/repo/issues/8".into()
            })
            .is_err()
    );
}

#[test]
fn issue_creation_uses_both_window_edges_and_late_facts_attach_only_the_resolving_turn() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _) = LinkStore::open(&directory.path().join("links.sqlite3")).unwrap();
    request(&mut store, 1, "처음 요청");
    chapter(&mut store);
    end(
        &mut store,
        1,
        TurnJudgment {
            missed: vec![NewMissedWork {
                kind: MissedKind::FollowUp,
                text: "회귀 검증 이슈 등록".into(),
            }],
            ..Default::default()
        },
    );
    request(&mut store, 2, "등록해줘");
    let id = store.ledger_record(&session()).unwrap().missed[0]
        .id
        .clone();
    end(
        &mut store,
        2,
        TurnJudgment {
            resolved: vec![ResolvedMissedWork {
                id: id.clone(),
                issue_number: Some(1),
            }],
            ..Default::default()
        },
    );
    assert_eq!(
        store.ledger_record(&session()).unwrap().missed[0].status,
        MissedStatus::Resolved
    );
    for (number, at, created) in [
        (1, 2300, 2300),
        (2, 8000, 10000),
        (3, 7999, 10000),
        (4, 40000, 10000),
        (5, 40001, 10000),
    ] {
        store.connection.execute("INSERT INTO ledger_sightings VALUES('device-a','codex','session-a','issue','owner/repo',?1,?2)",params![number,at]).unwrap();
        store.connection.execute("INSERT INTO ledger_output_facts VALUES('owner/repo','issue',?1,'Issue',?2,'open',?3,0,50000)",params![number,format!("https://github.com/owner/repo/issues/{number}"),created]).unwrap();
    }
    store.refresh_ledger_outputs().unwrap();
    let record = store.ledger_record(&session()).unwrap();
    let numbers = record
        .outputs
        .iter()
        .map(|output| output.number)
        .collect::<BTreeSet<_>>();
    assert_eq!(numbers, BTreeSet::from([1, 2, 4]));
    assert_eq!(record.missed[0].status, MissedStatus::Issue);
    assert_eq!(record.missed[0].issue_number, Some(1));
    store
        .apply_ledger_update(&LedgerUpdate::MissedStatus {
            session: session(),
            id,
            status: MissedStatus::Open,
        })
        .unwrap_err();
}

#[test]
fn search_returns_one_session_with_its_latest_matching_request_and_own_title_when_off() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _) = LinkStore::open(&directory.path().join("links.sqlite3")).unwrap();
    request(&mut store, 1, "기록 첫 요청");
    request(&mut store, 2, "기록 마지막 요청");
    chapter(&mut store);
    store
        .apply_ledger_update(&LedgerUpdate::Title {
            session: session(),
            native_title: Some("세션 자기 제목".into()),
            manual_title: None,
            state: "seen".into(),
        })
        .unwrap();
    let on = store.ledger_search("기록", true).unwrap();
    assert_eq!(on.len(), 1);
    assert_eq!(on[0].matched, "기록 마지막 요청");
    let off = store.ledger_search("기록", false).unwrap();
    assert_eq!(off.len(), 1);
    assert_eq!(off[0].title, "세션 자기 제목");
    assert_eq!(off[0].matched, "기록 마지막 요청");
}

#[test]
fn a_mixed_device_tree_rolls_outputs_and_missed_items_into_the_spawn_task_and_open_context() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _) = LinkStore::open(&directory.path().join("links.sqlite3")).unwrap();
    request(&mut store, 1, "위임해 기록 화면 구현");
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
    let child = SessionIdentity {
        device_id: "device-b".into(),
        agent: "claude".into(),
        session_id: "child".into(),
    };
    let grandchild = SessionIdentity {
        device_id: "device-a".into(),
        agent: "opencode".into(),
        session_id: "grandchild".into(),
    };
    for (session, parent, at) in [
        (&child, session(), 1_100),
        (&grandchild, child.clone(), 1_200),
    ] {
        store
            .apply_ledger_update(&LedgerUpdate::Requests {
                session: session.clone(),
                cwd: "/repo".into(),
                path: None,
                requests: vec![RecordedRequest {
                    offset: 1,
                    at_unix_ms: at,
                    text: "부모가 준 일".into(),
                    author: WorkAuthor::Parent,
                    task_ids: vec![],
                    chapter_id: None,
                    judged: false,
                }],
            })
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE sessions SET started_at=?4 WHERE device=?1 AND agent=?2 AND id=?3",
                params![session.device_id, session.agent, session.session_id, at],
            )
            .unwrap();
        store
            .apply_parents(&[crate::links::ParentFact {
                device_id: session.device_id.clone(),
                agent: session.agent.clone(),
                session_id: session.session_id.clone(),
                parent_device_id: parent.device_id,
                parent_agent: parent.agent,
                parent_session_id: parent.session_id,
                parent_name: "parent".into(),
            }])
            .unwrap();
        store
            .apply_ledger_update(&LedgerUpdate::Chapter {
                session: session.clone(),
                goal: "구현".into(),
                at_unix_ms: at,
            })
            .unwrap();
        store
            .apply_ledger_update(&LedgerUpdate::TurnEnd {
                session: session.clone(),
                offset: 1,
                at_unix_ms: 1_400,
                end: LabelEnd::Done,
                work_author: true,
                turn_witness: None,
                judgment: TurnJudgment {
                    task: TaskDecision::New {
                        name: "구현".into(),
                    },
                    missed: vec![NewMissedWork {
                        kind: MissedKind::Leftover,
                        text: "정리할 일".into(),
                    }],
                    ..Default::default()
                },
            })
            .unwrap();
    }
    store.connection.execute("INSERT INTO ledger_outputs VALUES('device-a','opencode','grandchild','pr','acme/app',12,'child PR','https://github.com/acme/app/pull/12','open',1200,1200,NULL)", []).unwrap();
    for state in ["open", "ci_failed", "closed", "merged"] {
        store
            .connection
            .execute("UPDATE ledger_outputs SET state=?1", [state])
            .unwrap();
        let record = store.ledger_record(&session()).unwrap();
        assert_eq!(record.children[0].session, child);
        assert_eq!(record.children[0].children[0].session, grandchild);
        assert_eq!(record.outputs.len(), 1);
        assert_eq!(record.outputs[0].source, grandchild);
        assert_eq!(
            record.outputs[0].task_id.as_deref(),
            Some(record.tasks[0].id.as_str())
        );
        assert_eq!(record.tasks[0].done, state == "merged");
        assert_eq!(
            store.ledger_context(&session()).unwrap().open_tasks.len(),
            usize::from(state != "merged")
        );
    }
    assert_eq!(
        store.ledger_context(&child).unwrap().parent_name.as_deref(),
        Some("parent")
    );
}

#[test]
fn delegated_raw_records_accept_only_the_initial_and_parents_work_requests() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _) = LinkStore::open(&directory.path().join("links.sqlite3")).unwrap();
    request(&mut store, 1, "initial parent prompt");
    request(&mut store, 2, "operator direct input");
    store
        .apply_parents(&[crate::links::ParentFact {
            device_id: session().device_id,
            agent: session().agent,
            session_id: session().session_id,
            parent_device_id: "device-b".into(),
            parent_agent: "claude".into(),
            parent_session_id: "parent".into(),
            parent_name: "lead".into(),
        }])
        .unwrap();
    let facts = hide_session::links::LinkFacts {
        requests: vec![
            hide_session::links::RequestMark {
                offset: 3,
                at_unix_ms: 3_000,
                text: "parent work request".into(),
                sender: Some("lead".into()),
                letter_kind: Some("request".into()),
            },
            hide_session::links::RequestMark {
                offset: 4,
                at_unix_ms: 4_000,
                text: "other request".into(),
                sender: Some("peer".into()),
                letter_kind: Some("request".into()),
            },
            hide_session::links::RequestMark {
                offset: 5,
                at_unix_ms: 5_000,
                text: "parent report".into(),
                sender: Some("lead".into()),
                letter_kind: Some("report".into()),
            },
            hide_session::links::RequestMark {
                offset: 6,
                at_unix_ms: 6_000,
                text: "operator later input".into(),
                sender: None,
                letter_kind: None,
            },
        ],
        ..Default::default()
    };
    let tx = store.connection.transaction().unwrap();
    apply_reader(&tx, "device-a", "codex", "session-a", &facts).unwrap();
    tx.commit().unwrap();
    let record = store.ledger_record(&session()).unwrap();
    assert_eq!(
        record
            .requests
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>(),
        ["parent work request", "initial parent prompt"]
    );
    assert!(
        record
            .requests
            .iter()
            .all(|r| r.author == WorkAuthor::Parent)
    );
}
