//! The human loop (factory-human-loop PRD): the label starts work, the
//! intake review completes the card and assumes instead of asking, every
//! question meets Factory AI first, checks send work back, follow-up
//! candidates, the one GitHub sign-in to-do, the worker's four-part report,
//! the seven-day numbers and the store's move from schema 1.

use crate::support::*;
use hide_factory::adapters::{EnvSignal, Failure, FileFact, IntakeFacts, OutsideEvent};
use hide_factory::command::{Command, FollowUpChoice, VerificationChoice};
use hide_factory::judgment::JudgmentInput;
use hide_factory::model::*;
use serde_json::{Value, json};

fn tick_until(h: &mut Bench, f: &str, t: &str, state: TaskState) {
    for _ in 0..40 {
        if h.state(f, t) == state {
            return;
        }
        h.advance(1_000);
        h.engine.tick();
    }
    panic!("{t} never reached {state:?}: {:?}", h.state(f, t));
}

fn github(h: &mut Bench) -> String {
    let created = h.op(Command::Init {
        project: PROJECT.into(),
        verification: Some(VerificationChoice::Ci {
            checks: vec!["test".into()],
        }),
        merge_mode: Some(MergeMode::Manual),
        confirm: true,
    });
    created["factory"]["id"].as_str().unwrap().to_owned()
}

/// Labels issue `number` and lets the outside read find it.
fn label(h: &mut Bench, f: &str, number: u64, title: &str, body: &str) -> String {
    h.world().outside.push_back(OutsideEvent::Labeled {
        issue: IssueRef::Github { number },
        title: title.into(),
        body: body.into(),
    });
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    h.engine
        .tasks_of(f)
        .find(|task| task.card.title == title)
        .map(|task| task.id.clone())
        .expect("the label made a Task")
}

fn intake(criteria: &[&str], assumptions: Value) -> Value {
    json!({
        "questions": [], "dependencies": [], "flags": [], "split": [], "fits_scope": true,
        "criteria": criteria, "out_of_scope": ["the admin screen"], "assumptions": assumptions,
    })
}

#[test]
fn a_label_starts_with_a_card_the_review_completed_and_assumptions_on_the_record() {
    let mut h = Bench::new(true);
    let f = github(&mut h);
    h.world().facts = IntakeFacts {
        files: vec![FileFact {
            path: "src/lib.rs".into(),
            text: "pub fn calc() {}".into(),
        }],
        related: Vec::new(),
    };
    h.world().intake.insert(
        "Bare".into(),
        intake(
            &["calc returns the sum"],
            json!([{"text": "Use the existing calc module", "reason": "the issue names no other"}]),
        ),
    );
    h.world()
        .intake
        .insert("Checked".into(), intake(&["something else"], json!([])));
    let bare = label(&mut h, &f, 7, "Bare", "Make calc add numbers.");
    let checked = label(&mut h, &f, 8, "Checked", "Do it\n- [ ] it works");
    h.engine.tick();
    // The review saw the repository's facts first (D-02).
    let judged = h
        .world()
        .judged
        .iter()
        .find_map(|j| match &j.input {
            JudgmentInput::IntakeReview { card, facts, .. } if card.title == "Bare" => {
                Some(facts.clone())
            }
            _ => None,
        })
        .expect("an intake review");
    assert_eq!(judged.files[0].path, "src/lib.rs");

    let task = h.task(&f, &bare);
    assert_eq!(
        task.card.criteria,
        vec!["calc returns the sum"],
        "filled (B2)"
    );
    assert_eq!(task.card.out_of_scope, vec!["the admin screen"]);
    assert_eq!(
        task.card.goal, "Make calc add numbers.",
        "the body as written"
    );
    let assumption = task
        .decisions
        .iter()
        .find(|d| d.source == Some(DecisionSource::Assumption))
        .expect("the assumption is a decision (B3)");
    assert_eq!(assumption.by, OBSERVER);
    assert_eq!(
        assumption.reason.as_deref(),
        Some("the issue names no other")
    );
    assert!(assumption.overridable());
    assert!(
        task.activity.iter().any(|entry| matches!(
            entry.event,
            ActivityEvent::Intake {
                label: true,
                criteria: 1,
                assumptions: 1
            }
        )),
        "the intake is the Task's first activity line"
    );
    assert!(
        task.open_questions().next().is_none(),
        "no card confirmation (B1)"
    );
    assert_ne!(task.state, TaskState::Drafting);
    let checked = h.task(&f, &checked);
    assert_eq!(
        checked.card.criteria,
        vec!["it works"],
        "the issue's checkboxes stay"
    );

    // The first worker prompt carries the decision record (B35).
    tick_until(&mut h, &f, &bare, TaskState::Running);
    let prompt = h
        .world()
        .spawned
        .iter()
        .find(|s| s.task == bare)
        .map(|s| s.prompt.clone())
        .unwrap();
    assert!(prompt.contains("Use the existing calc module"), "{prompt}");
    assert!(prompt.contains("hide factory done --result"), "{prompt}");
    assert!(!prompt.contains("--summary"), "{prompt}");
}

#[test]
fn a_review_question_meets_factory_ai_first_and_the_mode_table_answers_it() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.world().intake.insert(
        "Choice".into(),
        json!({"questions": [{"text": "Which format?", "stopped": "the start", "suggestion": "json", "choices": [
            {"choice": "json", "result": "the output is JSON"},
            {"choice": "csv", "result": "the output is CSV"}
        ]}], "dependencies": [], "flags": [], "split": []}),
    );
    // Kind A in assist: Factory AI answers (D-27, B6).
    h.world().observer.push_back(classified("A", "json"));
    let id = h.add("Choice", &[])["task"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    h.engine.tick();
    h.engine.tick();
    let task = h.task(&f, &id);
    let question = &task.questions[0];
    assert_eq!(question.answer.as_ref().unwrap().relayed_by, OBSERVER);
    assert_ne!(task.state, TaskState::Drafting, "the answer readied it");
    assert!(
        h.world()
            .judged
            .iter()
            .any(|j| matches!(j.input, JudgmentInput::ObserverClassify { .. })),
        "a review question is sorted like a worker's"
    );

    // Kind D is a person's in every mode, with what each choice leads to.
    h.world().intake.insert(
        "Permission".into(),
        json!({"questions": [{"text": "May it delete old rows?", "stopped": "the start", "suggestion": "no", "choices": [
            {"choice": "yes", "result": "old rows go"},
            {"choice": "no", "result": "old rows stay"}
        ]}], "dependencies": [], "flags": [], "split": []}),
    );
    h.world().observer.push_back(classified("D", ""));
    let asked = h.add("Permission", &[])["task"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    h.engine.tick();
    h.engine.tick();
    let summary = h.engine.summary();
    let item = summary
        .inbox
        .iter()
        .find(|item| item.task.as_deref() == Some(asked.as_str()))
        .expect("a person's question");
    assert_eq!(item.stopped.as_deref(), Some("the start"));
    assert_eq!(item.outcomes.len(), 2);
    assert_eq!(item.outcomes[1].result, "old rows stay");
    assert_eq!(item.decision_kind, Some(DecisionKind::D));
}

fn send_back(fix: &str) -> Value {
    json!({
        "verdict": "send_back",
        "send_back": fix,
        "criteria": [{"criterion": "it works", "state": "unmet", "reason": "no test"}],
        "questions": [],
        "flags": [],
    })
}

#[test]
fn a_check_sends_the_work_back_and_each_send_back_counts_toward_the_limit() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Checked", &[]);
    h.world()
        .drift
        .insert(t.clone(), send_back("Add the missing test"));
    for round in 1..=2 {
        h.done(&f, &t);
        h.engine.tick();
        h.engine.tick();
        assert_eq!(h.state(&f, &t), TaskState::Running, "round {round}");
        let task = h.task(&f, &t);
        assert_eq!(task.failures, round, "a send-back is a failure (B9)");
        let record = task.decisions.last().unwrap();
        assert_eq!(
            (record.by.as_str(), record.source),
            (OBSERVER, Some(DecisionSource::SendBack))
        );
        assert_eq!(task.criteria_check[0].state, CriterionState::Unmet);
        let wake = h.world().wakes.last().cloned().unwrap();
        assert!(wake.1.contains("Add the missing test"), "{wake:?}");
    }
    let shown = h.op(Command::Show { task: t.clone() });
    let check = &shown["task"]["checklist"];
    assert!(check.as_array().is_some(), "{shown}");

    h.done(&f, &t);
    h.engine.tick();
    h.engine.tick();
    let task = h.task(&f, &t);
    assert_eq!(
        (task.state, task.stop),
        (TaskState::Stopped, Some(StopReason::VerifyFailed))
    );
}

#[test]
fn an_unrelated_finding_waits_as_a_follow_up_and_becomes_what_a_person_picks() {
    let mut h = Bench::new(true);
    let f = github(&mut h);
    let t = label(&mut h, &f, 7, "Source", "Do it\n- [ ] works");
    tick_until(&mut h, &f, &t, TaskState::Running);
    for text in [
        "The README is stale",
        "Logs are noisy",
        "Unused import",
        "Typo",
    ] {
        let found = h.as_worker(
            &f,
            &t,
            Command::Propose {
                class: DiscoveryClass::Unrelated,
                text: text.into(),
                card: None,
                autonomy: None,
                reclassify: None,
                letter: None,
            },
        );
        assert_eq!(found["ok"], true, "{found}");
    }
    let summary = h.engine.summary();
    assert!(summary.inbox.is_empty(), "never a person's item (B17)");
    assert_eq!(summary.factories[0].follow_ups.len(), 4);

    let issue = h.op(Command::FollowUp {
        task: t.clone(),
        discovery: "D1".into(),
        choice: FollowUpChoice::Issue,
    });
    assert_eq!(issue["ok"], true, "{issue}");
    assert!(
        h.writes("follow_up.create The README is stale labelled=false")
            .len()
            == 1
    );
    let started = h.op(Command::FollowUp {
        task: t.clone(),
        discovery: "D2".into(),
        choice: FollowUpChoice::Factory,
    });
    assert_eq!(started["ok"], true, "{started}");
    assert_eq!(
        h.writes("follow_up.create Logs are noisy labelled=true")
            .len(),
        1
    );
    let new = h
        .engine
        .tasks_of(&f)
        .find(|task| task.card.title == "Logs are noisy")
        .cloned()
        .expect("the labelled issue is a Task at once (B18)");
    assert!(new.label_path && new.issue.is_some());
    h.world().follow_up_failure = Some(Failure::task("github.follow_up", "502"));
    let failed = h.op(Command::FollowUp {
        task: t.clone(),
        discovery: "D3".into(),
        choice: FollowUpChoice::Issue,
    });
    assert_eq!(failed["reason"], "follow_up_failed", "{failed}");
    let views = h.engine.summary().factories[0].follow_ups.clone();
    let line = views.iter().find(|v| v.discovery == "D3").unwrap();
    assert!(line.failure.is_some(), "the line keeps why (B19)");
    let again = h.op(Command::FollowUp {
        task: t.clone(),
        discovery: "D3".into(),
        choice: FollowUpChoice::Issue,
    });
    assert_eq!(again["ok"], true, "pressed again: {again}");
    let discarded = h.op(Command::FollowUp {
        task: t.clone(),
        discovery: "D4".into(),
        choice: FollowUpChoice::Discard,
    });
    assert_eq!(discarded["ok"], true, "{discarded}");
    let task = h.task(&f, &t);
    assert!(task.activity.iter().any(|entry| matches!(
        entry.event,
        ActivityEvent::FollowUp {
            state: FollowUpState::Discarded,
            ..
        }
    )));
    assert!(h.engine.summary().factories[0].follow_ups.is_empty());
    let settled = h.op(Command::FollowUp {
        task: t.clone(),
        discovery: "D4".into(),
        choice: FollowUpChoice::Issue,
    });
    assert_eq!(settled["reason"], "follow_up_settled");
}

#[test]
fn a_lost_github_sign_in_is_one_to_do_and_a_passing_check_lets_the_steps_continue() {
    let mut h = Bench::new(true);
    let f = github(&mut h);
    h.world().observe_failure = Some(Failure::environment(
        "observe",
        EnvSignal::GithubAuth,
        "401",
    ));
    // The sign-in stays lost until the operator signs in again.
    h.world().access_failure = Some(Failure::environment(
        "github.access",
        EnvSignal::GithubAuth,
        "401",
    ));
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    let summary = h.engine.summary();
    let todos: Vec<_> = summary
        .inbox
        .iter()
        .filter(|item| item.kind == "github")
        .collect();
    assert_eq!(todos.len(), 1, "one per Factory (B33)");
    assert_eq!(todos[0].command.as_deref(), Some("gh auth login"));
    let reads = h.world().observed;
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    assert_eq!(
        h.world().observed,
        reads,
        "nothing is asked while it is refused"
    );

    let refused = h.op(Command::Resolve {
        project: None,
        item: "github".into(),
    });
    assert_eq!(refused["reason"], "github_still_blocked", "{refused}");
    h.world().access_failure = None;
    h.world().observe_failure = None;
    let resolved = h.op(Command::Resolve {
        project: None,
        item: "github".into(),
    });
    assert_eq!(resolved["ok"], true, "{resolved}");
    assert!(h.engine.summary().factories[0].github_block.is_none());
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    assert!(h.world().observed > reads, "the reads continue");
    assert!(
        h.engine
            .summary()
            .inbox
            .iter()
            .all(|item| item.factory != f || item.kind != "github")
    );
}

#[test]
fn a_missing_permission_waits_for_the_person_s_press_since_a_read_cannot_prove_it() {
    let mut h = Bench::new(true);
    let f = github(&mut h);
    h.world().observe_failure = Some(Failure::environment(
        "observe",
        EnvSignal::GithubForbidden,
        "HTTP 403: requires one of the following scopes: ['read:org']",
    ));
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    let block = h.engine.summary().factories[0].github_block.clone();
    assert!(block.as_ref().is_some_and(|b| b.forbidden), "{block:?}");
    // The reads the check makes pass, but the refused step would still be refused.
    h.world().observe_failure = None;
    for _ in 0..3 {
        h.advance(6 * MINUTE_MS);
        h.engine.tick();
    }
    assert!(
        h.engine.summary().factories[0].github_block.is_some(),
        "not cleared on its own"
    );
    let resolved = h.op(Command::Resolve {
        project: None,
        item: "github".into(),
    });
    assert_eq!(resolved["ok"], true, "{resolved}");
    assert!(
        h.engine
            .summary()
            .inbox
            .iter()
            .all(|item| item.factory != f || item.kind != "github")
    );
}

#[test]
fn done_takes_four_parts_and_refuses_the_retired_summary_with_the_new_ones() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Reported", &[]);
    let retired = h.as_worker(
        &f,
        &t,
        Command::Done {
            result: None,
            changed: Vec::new(),
            verified: Vec::new(),
            unverified: Vec::new(),
            summary: Some("did it".into()),
            breaking: false,
            raw: None,
        },
    );
    assert_eq!(retired["reason"], "summary_replaced", "{retired}");
    assert!(
        retired["next_action"]
            .as_str()
            .unwrap()
            .contains("--result")
    );
    assert_eq!(h.state(&f, &t), TaskState::Running);
    let done = h.as_worker(
        &f,
        &t,
        Command::Done {
            result: Some("calc adds numbers".into()),
            changed: vec!["calc.rs".into()],
            verified: vec!["cargo test".into()],
            unverified: vec!["the release build".into()],
            summary: None,
            breaking: false,
            raw: None,
        },
    );
    assert_eq!(done["ok"], true, "{done}");
    let task = h.task(&f, &t);
    let report = task.report.clone().unwrap();
    assert_eq!(report.result, "calc adds numbers");
    assert_eq!(report.unverified, vec!["the release build"]);
    assert!(
        task.decisions.is_empty(),
        "a report is not a decision (B30)"
    );
    assert!(
        task.activity
            .iter()
            .any(|entry| matches!(entry.event, ActivityEvent::Report { .. }))
    );
}

#[test]
fn the_seven_day_numbers_count_what_people_moved_how_long_starts_took_and_overrides() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let empty = h.engine.summary().factories[0].metrics.clone();
    assert_eq!(
        (
            empty.person_items_tenths,
            empty.start_median_ms,
            empty.override_percent
        ),
        (None, None, None),
        "nothing to count reads '-' (D-45)"
    );
    let t = h.ready("Counted", &[]);
    // Factory AI answers one question; a person changes it.
    h.world().observer.push_back(classified("B", "postgres"));
    h.as_worker(
        &f,
        &t,
        Command::Ask {
            text: "Which database?".into(),
            suggestion: "sqlite".into(),
            default_action: "sqlite".into(),
            deadline_hours: Some(24),
            letter: None,
            choices: Vec::new(),
        },
    );
    h.engine.tick();
    let changed = h.op(Command::Answer {
        task: t.clone(),
        question: None,
        choice: None,
        text: Some("mysql".into()),
        change: false,
        decision: Some("R1".into()),
    });
    assert_eq!(changed["ok"], true, "{changed}");
    h.done(&f, &t);
    tick_until(&mut h, &f, &t, TaskState::Done);
    let metrics = h.engine.summary().factories[0].metrics.clone();
    assert_eq!(metrics.finished, 1);
    assert_eq!(
        metrics.person_items_tenths,
        Some(0),
        "an override is not an item"
    );
    assert_eq!(metrics.started, 1);
    assert!(metrics.start_median_ms.is_some());
    assert_eq!((metrics.ai_decisions, metrics.overridden), (1, 1));
    assert_eq!(metrics.override_percent, Some(100));
    // A week later nothing counts.
    h.advance(8 * DAY_MS);
    let later = h.engine.summary().factories[0].metrics.clone();
    assert_eq!(
        (later.finished, later.started, later.ai_decisions),
        (0, 0, 0)
    );
}

/// Rewrites every stored record of `table` through `change`, then marks the
/// store schema 1, as a build before this one left it.
fn seed_v1(path: &std::path::Path, table: &str, change: impl Fn(&mut Value)) {
    let db = rusqlite::Connection::open(path).unwrap();
    let rows: Vec<(String, String)> = db
        .prepare(&format!("SELECT id, data FROM {table}"))
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for (id, data) in rows {
        let mut value: Value = serde_json::from_str(&data).unwrap();
        change(&mut value);
        db.execute(
            &format!("UPDATE {table} SET data = ?1 WHERE id = ?2"),
            rusqlite::params![value.to_string(), id],
        )
        .unwrap();
    }
    db.pragma_update(None, "user_version", 1).unwrap();
}

/// A Task with one open worker question whose record a schema 1 store
/// would hold, and the state folder's store path.
fn old_store() -> (Bench, String, String, std::path::PathBuf) {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Old", &[]);
    h.world().observer.push_back(classified("D", ""));
    let asked = h.as_worker(
        &f,
        &t,
        Command::Ask {
            text: "Which database?".into(),
            suggestion: "sqlite".into(),
            default_action: "sqlite".into(),
            deadline_hours: Some(24),
            letter: None,
            choices: Vec::new(),
        },
    );
    assert_eq!(asked["ok"], true, "{asked}");
    h.engine.tick();
    let path = h.dir.path().join("factory.sqlite3");
    (h, f, t, path)
}

fn as_schema_one(task: &mut Value) {
    let template = task["questions"][0].clone();
    let question = |id: &str, kind: Value, text: &str, answer: Value| {
        let mut question = template.clone();
        question["id"] = json!(id);
        question["kind"] = kind;
        question["text"] = json!(text);
        question["answer"] = answer;
        question
    };
    let operator =
        |text: &str| json!({"text": text, "chose": text, "relayed_by": "operator", "at": 3});
    task["questions"] = json!([
        template.clone(),
        question(
            "Q2",
            json!({"kind": "notice"}),
            "the worktree stayed",
            operator("ok")
        ),
        question(
            "Q3",
            json!({"kind": "proposal", "command": "brew upgrade gh", "impact": "gh gets newer"}),
            "gh is too old",
            Value::Null,
        ),
        question(
            "Q4",
            json!({"kind": "confirm_card"}),
            "confirm the card",
            Value::Null
        ),
    ]);
    task["decisions"] = json!([
        {"text": "the worktree stayed -> ok", "by": "operator", "at": 3},
        {"text": "done: added calc", "by": format!("worker:{}", task["id"].as_str().unwrap()), "at": 4},
        {"text": "Which DB? -> sqlite", "by": "operator", "at": 5},
    ]);
    task["discoveries"] = json!([
        {"id": "D1", "class": "unrelated", "text": "README is stale", "at": 2, "task": null},
    ]);
    task.as_object_mut().unwrap().remove("activity");
}

#[test]
fn a_schema_one_store_moves_once_and_keeps_its_copy() {
    let (h, f, t, path) = old_store();
    let h = {
        let Bench {
            dir,
            shared,
            engine,
        } = h;
        drop(engine);
        seed_v1(&path, "tasks", as_schema_one);
        seed_v1(&path, "factories", |factory| {
            factory["config"]["recovery"] = json!([]);
        });
        let engine =
            hide_factory::Engine::open(&path, &dir.path().join("factory-files"), ports(&shared))
                .expect("a schema 1 store opens");
        Bench {
            dir,
            shared,
            engine,
        }
    };
    let copy = path.with_file_name("factory.v1.sqlite3");
    let version: i64 = rusqlite::Connection::open(&copy)
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1, "the copy is the store as it was");

    let task = h.task(&f, &t);
    let kinds: Vec<_> = task.questions.iter().map(|q| q.kind.clone()).collect();
    assert_eq!(
        kinds.len(),
        2,
        "notices and proposals left the questions: {kinds:?}"
    );
    let card = &task.questions[1];
    assert_eq!(card.answer.as_ref().unwrap().relayed_by, "migration");
    let texts: Vec<_> = task.decisions.iter().map(|d| d.text.as_str()).collect();
    assert_eq!(
        texts,
        vec!["Which DB? -> sqlite"],
        "only real decisions stay"
    );
    assert_eq!(task.report.as_ref().unwrap().result, "added calc");
    assert!(task.activity.iter().any(|entry| matches!(
        &entry.event,
        ActivityEvent::Note { text } if text == "the worktree stayed"
    )));
    assert!(
        task.discoveries[0].follow_up.is_some(),
        "a follow-up candidate"
    );
    let factory = h.engine.summary().factories[0].clone();
    assert_eq!(factory.follow_ups.len(), 1);
    let todo = h
        .engine
        .summary()
        .inbox
        .into_iter()
        .find(|item| item.kind == "command")
        .expect("the open proposal is a command to-do");
    assert_eq!(todo.command.as_deref(), Some("brew upgrade gh"));

    // The move runs once: a second open finds schema 2 and changes nothing.
    let before = h.task(&f, &t);
    let h = h.restart();
    assert_eq!(h.task(&f, &t), before);
}

#[test]
fn a_failed_move_leaves_the_schema_one_store_as_it_was() {
    let (h, _f, t, path) = old_store();
    let Bench {
        dir,
        shared,
        engine,
    } = h;
    drop(engine);
    seed_v1(&path, "tasks", |task| {
        as_schema_one(task);
        task["state"] = json!("no_such_state");
    });
    let read = |path: &std::path::Path| -> (i64, String) {
        let db = rusqlite::Connection::open(path).unwrap();
        (
            db.pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap(),
            db.query_row("SELECT data FROM tasks WHERE id = ?1", [&t], |row| {
                row.get(0)
            })
            .unwrap(),
        )
    };
    let before = read(&path);
    let refused =
        hide_factory::Engine::open(&path, &dir.path().join("factory-files"), ports(&shared));
    let error = refused.err().expect("the move is refused");
    assert!(error.0.contains("migration"), "{error}");
    assert_eq!(read(&path), before, "nothing moved");
    assert!(path.with_file_name("factory.v1.sqlite3").exists());
}
