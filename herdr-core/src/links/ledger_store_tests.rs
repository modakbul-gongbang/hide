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
        super::super::SCHEMA_VERSION
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
    connection
        .pragma_update(None, "user_version", super::super::SCHEMA_VERSION + 1)
        .unwrap();
    drop(connection);
    assert_eq!(LinkStore::open(&path).err().unwrap(), "links_store_newer");
}

/// The shape a build before session naming left (a read-only copy of an
/// installed `links.sqlite3`, 2026-10-14): Pi children recorded under their
/// files, one of them under a Pi parent's file, while the reader recorded
/// both sessions and their requests under the ids in their headers.
#[test]
fn edges_left_under_read_files_move_to_the_readers_names_once() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("links.sqlite3");
    let device = "8f1c2d3e-4a5b-4c6d-8e7f-90a1b2c3d4e5";
    let lead = "/Users/example/.pi/agent/sessions/--work-app--/2026-10-10T04-36-40-242Z_01a1.jsonl";
    let fork = "/Users/example/.pi/agent/sessions/--work-app--/2026-10-10T05-23-34-783Z_01a2.jsonl";
    let unread =
        "/Users/example/.pi/agent/sessions/--work-app--/2026-10-10T06-00-00-000Z_01a3.jsonl";
    let (store, _) = LinkStore::open(&path).unwrap();
    store
        .connection
        .execute_batch(&format!(
            "DELETE FROM meta WHERE key='session_names';
             INSERT INTO sessions(device,agent,id,path,cwd,started_at) VALUES
              ('{device}','claude','claude-lead',NULL,'/work/app',1),
              ('{device}','pi','01a1','{lead}','/work/app',2),
              ('{device}','pi','01a2','{fork}','/work/app',3);
             INSERT INTO cursors(device,path,agent,stamp,session) VALUES
              ('{device}','{lead}','pi','s','01a1'),('{device}','{fork}','pi','s','01a2');
             INSERT INTO session_parents VALUES
              ('{device}','pi','{lead}','claude','claude-lead','bell-guard-lead','{device}'),
              ('{device}','pi','{fork}','pi','{lead}','re-pi','{device}'),
              ('{device}','pi','{unread}','claude','claude-lead','bell-guard-lead','{device}');
             INSERT INTO ledger_requests(device,agent,id,offset,at,text,author) VALUES
              ('{device}','pi','01a1',0,2,'brief from the lead','\"operator\"'),
              ('{device}','pi','01a2',0,3,'brief from re-pi','\"operator\"');"
        ))
        .unwrap();
    drop(store);

    let ((store, _), records) = crate::diagnostics::capture(|| LinkStore::open(&path).unwrap());
    let pi = |id: &str| SessionIdentity {
        device_id: device.into(),
        agent: "pi".into(),
        session_id: id.into(),
    };
    let lead_record = store
        .ledger_record(&SessionIdentity {
            device_id: device.into(),
            agent: "claude".into(),
            session_id: "claude-lead".into(),
        })
        .unwrap();
    assert_eq!(lead_record.children.len(), 1);
    assert_eq!(lead_record.children[0].session, pi("01a1"));
    assert_eq!(lead_record.children[0].children[0].session, pi("01a2"));
    for (id, parent) in [("01a1", "bell-guard-lead"), ("01a2", "re-pi")] {
        assert_eq!(
            store.ledger_record(&pi(id)).unwrap().requests[0].author,
            WorkAuthor::Parent
        );
        assert_eq!(
            store
                .ledger_context(&pi(id))
                .unwrap()
                .parent_name
                .as_deref(),
            Some(parent)
        );
    }
    // A file the reader never read keeps its edge for the read to name.
    assert_eq!(
        store
            .connection
            .query_row(
                "SELECT count(*) FROM session_parents WHERE id=?1",
                [unread],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert!(
        records.iter().any(|record| {
            record["kind"] == "store.session_names_moved" && record["files"] == 2
        })
    );
    drop(store);
    let (_, again) = crate::diagnostics::capture(|| LinkStore::open(&path).unwrap());
    assert!(
        !again
            .iter()
            .any(|record| record["kind"] == "store.session_names_moved")
    );
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
fn an_untitled_ended_record_and_raw_search_keep_the_sessions_provider_title() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("links.sqlite3");
    let (mut store, _) = LinkStore::open(&path).unwrap();
    request(&mut store, 1, "A raw request kept after the pane ends");
    assert_eq!(store.ledger_record(&session()).unwrap().title, "Codex");
    drop(store);
    let (store, _) = LinkStore::open(&path).unwrap();
    assert_eq!(store.ledger_record(&session()).unwrap().title, "Codex");
    assert_eq!(
        store.ledger_search("raw request", false).unwrap()[0].title,
        "Codex"
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
fn record_fallbacks_use_the_sessions_title_for_every_supported_agent() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _) = LinkStore::open(&directory.path().join("links.sqlite3")).unwrap();
    for (agent, expected) in [
        ("claude", "Claude"),
        ("codex", "Codex"),
        ("grok", "grok"),
        ("opencode", "OpenCode"),
        ("pi", "pi"),
        ("omp", "omp"),
        ("cursor", "cursor"),
    ] {
        let owner = SessionIdentity {
            agent: agent.into(),
            ..session()
        };
        store
            .apply_ledger_update(&LedgerUpdate::Requests {
                session: owner.clone(),
                cwd: "/repo".into(),
                path: None,
                requests: vec![RecordedRequest {
                    offset: 1,
                    at_unix_ms: 1_000,
                    text: "An untitled request".into(),
                    author: WorkAuthor::Operator,
                    task_ids: vec![],
                    chapter_id: None,
                    judged: false,
                }],
            })
            .unwrap();
        let record = store.ledger_record(&owner).unwrap();
        assert_eq!(record.title, expected);
        assert_eq!(record.fallback_title, expected);
    }
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
        key: AttemptKey::Missed(source.clone()),
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
            key: AttemptKey::Missed(source.clone()),
            number: 7,
            url: "https://github.com/owner/repo/issues/7".into(),
        })
        .unwrap();
    drop(store);
    let (mut store, _) = LinkStore::open(&path).unwrap();
    let record = store.ledger_record(&session()).unwrap();
    assert_eq!(record.missed[0].status, MissedStatus::Backlog);
    assert_eq!(record.missed[0].issue_number, Some(7));
    assert_eq!(
        record.missed[0].backlog_key.as_deref(),
        Some("github:owner/repo#7")
    );
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
                key: AttemptKey::Missed(source),
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

/// An issue address a session printed reaches the project's issue read only
/// when it is in the project's repository: a test fixture's `acme/repo` is
/// never sent to GitHub, where it does not resolve.
#[test]
fn issue_sightings_outside_the_projects_repository_are_not_read() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _) = LinkStore::open(&directory.path().join("links.sqlite3")).unwrap();
    request(&mut store, 1, "이슈 정리");
    for (repo, number) in [("owner/repo", 1), ("acme/repo", 2)] {
        store.connection.execute("INSERT INTO ledger_sightings VALUES('device-a','codex','session-a','issue',?1,?2,1000)",params![repo,number]).unwrap();
    }
    let mut project = crate::links::ProjectFacts {
        key: "project-a".into(),
        device_id: "device-a".into(),
        workspace_id: "workspace-a".into(),
        root: "/repo".into(),
        repository: None,
        repository_id: None,
        worktrees: Vec::new(),
        prs: Vec::new(),
        prs_read: true,
        outputs: Vec::new(),
    };
    store.apply_project(&project, 1).unwrap();
    assert_eq!(
        store.ledger_issue_refs("project-a").unwrap(),
        [],
        "no repository answered yet"
    );
    let own = crate::issues::IssueReference::parse("owner/repo#1", None).unwrap();
    project.repository = Some("owner/repo".into());
    project.repository_id = Some("R_1".into());
    store.apply_project(&project, 2).unwrap();
    assert_eq!(
        store.ledger_issue_refs("project-a").unwrap(),
        std::slice::from_ref(&own)
    );
    // A renamed repository keeps the sightings made under its old name.
    project.repository = Some("owner/renamed".into());
    store.apply_project(&project, 3).unwrap();
    assert_eq!(store.ledger_issue_refs("project-a").unwrap(), [own]);
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

#[test]
fn late_parent_retracts_judgments_and_refuses_in_flight_operator_work() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _) = LinkStore::open(&directory.path().join("links.sqlite3")).unwrap();
    request(&mut store, 1, "initial parent prompt");
    chapter(&mut store);
    end(
        &mut store,
        1,
        TurnJudgment {
            task: TaskDecision::New {
                name: "parent work".into(),
            },
            missed: vec![NewMissedWork {
                kind: MissedKind::Problem,
                text: "parent follow-up".into(),
            }],
            ..Default::default()
        },
    );
    request(&mut store, 2, "operator continuation");
    store
        .apply_ledger_update(&LedgerUpdate::TurnEnd {
            session: session(),
            offset: 2,
            at_unix_ms: 2500,
            end: LabelEnd::Blocked,
            work_author: true,
            turn_witness: None,
            judgment: TurnJudgment {
                task: TaskDecision::Continue {
                    ids: vec!["task-1".into()],
                },
                resolved: vec![ResolvedMissedWork {
                    id: "missed-1-0".into(),
                    issue_number: None,
                }],
                ..Default::default()
            },
        })
        .unwrap();
    request(&mut store, 3, "operator new work");
    store.connection.execute("INSERT INTO ledger_outputs VALUES('device-a','codex','session-a','pr','owner/repo',7,'invalid continuation PR','https://github.com/owner/repo/pull/7','open',2200,2200,'task-1')", []).unwrap();
    end(
        &mut store,
        3,
        TurnJudgment {
            task: TaskDecision::New {
                name: "invalid work".into(),
            },
            missed: vec![NewMissedWork {
                kind: MissedKind::Problem,
                text: "invalid follow-up".into(),
            }],
            ..Default::default()
        },
    );
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
    // An END and a raw request captured before lineage arrived settle later.
    request(&mut store, 3, "stale captured operator request");
    end(
        &mut store,
        3,
        TurnJudgment {
            task: TaskDecision::New {
                name: "stale in-flight work".into(),
            },
            ..Default::default()
        },
    );
    let record = store.ledger_record(&session()).unwrap();
    assert_eq!(record.requests.len(), 1);
    assert_eq!(record.tasks.len(), 1);
    assert_eq!(record.tasks[0].name, "parent work");
    assert_eq!(record.tasks[0].end, LabelEnd::Done);
    assert!(
        record.tasks[0].done,
        "a retracted continuation's PR cannot hold up valid work"
    );
    assert!(record.outputs[0].task_id.is_none());
    assert!(
        store
            .ledger_context(&session())
            .unwrap()
            .open_tasks
            .is_empty()
    );
    assert_eq!(record.last_end, Some(LabelEnd::Done));
    assert_eq!(record.missed.len(), 1);
    assert_eq!(record.missed[0].status, MissedStatus::Open);
    // A root-context verdict queued before the parent must not blacklist
    // a canonical request discovered after the cache recovers.
    store
        .apply_ledger_update(&LedgerUpdate::ExcludeRequests {
            session: session(),
            parent_name: None,
            offsets: vec![4],
        })
        .unwrap();
    let facts = hide_session::links::LinkFacts {
        requests: vec![hide_session::links::RequestMark {
            offset: 4,
            at_unix_ms: 4000,
            text: "real parent request".into(),
            sender: Some("lead".into()),
            letter_kind: Some("request".into()),
        }],
        ..Default::default()
    };
    let tx = store.connection.transaction().unwrap();
    apply_reader(&tx, "device-a", "codex", "session-a", &facts).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        store.ledger_record(&session()).unwrap().requests[0].text,
        "real parent request"
    );
}

#[test]
fn nested_repository_ownership_uses_the_deepest_path_on_the_sessions_device() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _) = LinkStore::open(&directory.path().join("links.sqlite3")).unwrap();
    request(&mut store, 1, "inner work");
    store
        .connection
        .execute("UPDATE sessions SET cwd='/repo/tool/task'", [])
        .unwrap();
    for (key, device, root, repo) in [
        ("a-outer", "device-a", "/repo", "owner/outer"),
        ("b-inner", "device-a", "/repo/tool", "owner/inner"),
        ("c-other", "device-b", "/repo/tool/task", "owner/other"),
    ] {
        store
            .connection
            .execute(
                "INSERT INTO projects VALUES(?1,?2,?3,?4,?4,1000)",
                params![key, device, root, repo],
            )
            .unwrap();
    }
    let record = store.ledger_record(&session()).unwrap();
    assert_eq!(record.project.as_deref(), Some("/repo/tool"));
    assert_eq!(record.repository.as_deref(), Some("owner/inner"));
    // The inner task worktree has disappeared; its learned path still
    // belongs to the inner repository for surviving-checkout fallback.
    store
        .connection
        .execute("UPDATE sessions SET cwd='/elsewhere/old-task'", [])
        .unwrap();
    store
        .connection
        .execute(
            "INSERT INTO worktrees VALUES('b-inner','/elsewhere/old-task','task',1000,2000)",
            [],
        )
        .unwrap();
    assert_eq!(
        store
            .ledger_record(&session())
            .unwrap()
            .repository
            .as_deref(),
        Some("owner/inner")
    );
}

/// `백로그로` on a Local project marks the line with the memo it made, the
/// memo's origin reads the line back, and `되돌리기` opens the line again.
#[test]
fn a_memo_from_missed_work_marks_its_line_and_undo_opens_it_again() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _) = LinkStore::open(&directory.path().join("links.sqlite3")).unwrap();
    request(&mut store, 1, "구현해줘");
    chapter(&mut store);
    end(
        &mut store,
        1,
        TurnJudgment {
            missed: vec![NewMissedWork {
                kind: MissedKind::FollowUp,
                text: "문서 갱신".into(),
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
    let key = "local:/repo#3".to_owned();
    store
        .ledger_call(LedgerCall::BacklogLocal {
            source: source.clone(),
            backlog_key: key.clone(),
        })
        .unwrap();
    let missed = &store.ledger_record(&session()).unwrap().missed[0];
    assert_eq!(missed.status, MissedStatus::Backlog);
    assert_eq!(missed.backlog_key.as_deref(), Some(key.as_str()));
    let LedgerAnswer::Keys(keys) = store.ledger_call(LedgerCall::MissedKeys).unwrap() else {
        panic!("expected the missed lines' items")
    };
    assert_eq!(
        keys,
        std::slice::from_ref(&key),
        "the backlog row's flag names the memo"
    );
    assert!(
        store
            .ledger_call(LedgerCall::BacklogLocal {
                source,
                backlog_key: "local:/repo#4".into(),
            })
            .is_err(),
        "a line already in the backlog makes no second item"
    );
    let LedgerAnswer::Origin(Some(origin)) = store
        .ledger_call(LedgerCall::BacklogOrigin {
            backlog_key: key.clone(),
        })
        .unwrap()
    else {
        panic!("expected the memo's origin")
    };
    assert_eq!(origin.session, session());
    assert_eq!(origin.missed.text, "문서 갱신");

    store
        .ledger_call(LedgerCall::ReleaseBacklog { backlog_key: key })
        .unwrap();
    let missed = &store.ledger_record(&session()).unwrap().missed[0];
    assert_eq!(missed.status, MissedStatus::Open);
    assert_eq!(missed.backlog_key, None);
    let LedgerAnswer::Keys(keys) = store.ledger_call(LedgerCall::MissedKeys).unwrap() else {
        panic!("expected the missed lines' items")
    };
    assert!(keys.is_empty(), "an undone item is no longer flagged");
}

/// A typed item whose create was not confirmed is offered after a restart;
/// a confirmed or put-away one is not.
#[test]
fn only_unconfirmed_typed_items_are_pending_after_a_restart() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _) = LinkStore::open(&directory.path().join("links.sqlite3")).unwrap();
    let intent = |name: &str| BacklogIntent {
        intent: name.into(),
        device: "mac".into(),
        project: "/repo".into(),
        workspace_id: "w".into(),
    };
    for (name, at) in [("made", 1), ("lost", 2), ("dropped", 3)] {
        store
            .ledger_call(LedgerCall::ReserveIssue {
                key: AttemptKey::Intent(intent(name)),
                repository: "owner/repo".into(),
                title: format!("{name} item"),
                body: String::new(),
                at_unix_ms: at,
            })
            .unwrap();
    }
    store
        .ledger_call(LedgerCall::FinishIssue {
            key: AttemptKey::Intent(intent("made")),
            number: 5,
            url: "https://github.com/owner/repo/issues/5".into(),
        })
        .unwrap();
    store
        .ledger_call(LedgerCall::UnknownIssue(AttemptKey::Intent(intent("lost"))))
        .unwrap();
    store
        .ledger_call(LedgerCall::DismissBacklog {
            intent: "dropped".into(),
        })
        .unwrap();
    let LedgerAnswer::Pending(rows) = store.ledger_call(LedgerCall::PendingBacklog).unwrap() else {
        panic!("expected pending rows")
    };
    assert_eq!(
        rows.iter()
            .map(|row| (row.intent.intent.as_str(), row.title.as_str()))
            .collect::<Vec<_>>(),
        [("lost", "lost item")]
    );
}
