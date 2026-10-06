//! The engine's rules, driven through its command, letter and tick entry
//! points over a recording fake world and an injected clock.

mod support;

use std::collections::BTreeMap;

use hide_factory::Inbound;
use hide_factory::adapters::{
    EnvSignal, Failure, MainCheck, MemoryPressure, OutsideEvent, VerifyPoll, WorkerStatus,
};
use hide_factory::command::{CardInput, Command, VerificationChoice};
use hide_factory::judgment::JudgmentInput;
use hide_factory::model::*;
use serde_json::json;
use support::*;

fn set_workers(h: &mut Bench, factory: &str, n: u32) {
    let answer = h.op(Command::Config {
        project: Some(PROJECT.into()),
        set: vec![("max_workers".into(), n.to_string())],
    });
    assert_eq!(answer["ok"], true, "{answer}");
    let _ = factory;
}

/// Ticks until the Task reaches `state`, failing with its last state.
fn tick_until(h: &mut Bench, factory: &str, id: &str, state: TaskState) {
    for _ in 0..20 {
        if h.state(factory, id) == state {
            return;
        }
        h.engine.tick();
    }
    panic!(
        "{id} never reached {state:?}; it is {:?}",
        h.state(factory, id)
    );
}

fn open_question(h: &Bench, factory: &str, id: &str) -> Question {
    h.task(factory, id)
        .open_questions()
        .next()
        .cloned()
        .expect("an open question")
}

// ------------------------------------------------------------------- init

#[test]
fn init_previews_without_writing_and_creates_once_confirmed() {
    let mut h = Bench::new(true);
    let preview = h.op(Command::Init {
        project: PROJECT.into(),
        verification: None,
        merge_mode: None,
        confirm: false,
    });
    assert_eq!(preview["preview"], true, "{preview}");
    assert!(h.world().writes.is_empty(), "a preview writes nothing");
    assert!(h.engine.factories().next().is_none());
    assert_eq!(preview["writes"], json!(["label factory"]));

    // Auto merge needs a verification (B2).
    let refused = h.op(Command::Init {
        project: PROJECT.into(),
        verification: Some(VerificationChoice::None),
        merge_mode: Some(MergeMode::Auto),
        confirm: true,
    });
    assert_eq!(refused["reason"], "auto_needs_verification", "{refused}");

    let created = h.op(Command::Init {
        project: PROJECT.into(),
        verification: Some(VerificationChoice::Ci {
            checks: vec!["test".into()],
        }),
        merge_mode: Some(MergeMode::Auto),
        confirm: true,
    });
    assert_eq!(created["created"], true, "{created}");
    assert_eq!(h.writes("label.create"), vec!["label.create factory"]);
    let factory = h.engine.factories().next().unwrap().clone();
    assert_eq!(factory.config.default_runtime, Runtime::Claude);

    // The same project again shows the existing Factory (B5).
    let again = h.op(Command::Init {
        project: PROJECT.into(),
        verification: None,
        merge_mode: None,
        confirm: true,
    });
    assert_eq!(again["existing"], true, "{again}");
    assert_eq!(h.writes("label.create").len(), 1);
}

#[test]
fn a_machine_with_only_codex_defaults_new_workers_to_codex() {
    let mut h = Bench::new(false);
    h.world().runtimes = vec![Runtime::Codex];
    let preview = h.op(Command::Init {
        project: PROJECT.into(),
        verification: None,
        merge_mode: Some(MergeMode::Manual),
        confirm: false,
    });
    assert_eq!(preview["default_runtime"], "codex", "{preview}");
    h.op(Command::Init {
        project: PROJECT.into(),
        verification: None,
        merge_mode: Some(MergeMode::Manual),
        confirm: true,
    });
    let factory = h.engine.factories().next().unwrap().clone();
    assert_eq!(factory.config.default_runtime, Runtime::Codex);
}

#[test]
fn a_closed_factory_refuses_tasks_and_comes_back_with_its_records() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Keep me", &[]);
    // Not while a Task runs (B74).
    assert_eq!(h.state(&f, &t), TaskState::Running);
    let refused = h.op(Command::Close { project: None });
    assert_eq!(refused["reason"], "factory_has_running_tasks", "{refused}");
    h.op(Command::Cancel { task: t.clone() });
    let closed = h.op(Command::Close { project: None });
    assert_eq!(closed["ok"], true, "{closed}");
    let refused = h.add("New", &[]);
    assert_eq!(refused["reason"], "factory_closed", "{refused}");

    let reopened = h.op(Command::Init {
        project: PROJECT.into(),
        verification: None,
        merge_mode: None,
        confirm: true,
    });
    assert_eq!(reopened["reopened"], true, "{reopened}");
    assert_eq!(h.task(&f, &t).card.title, "Keep me");
}

// -------------------------------------------------------------------- add

#[test]
fn a_card_missing_its_shape_is_refused_with_each_field() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let answer = h.add_card(CardInput {
        title: Some("only a title".into()),
        ..CardInput::default()
    });
    assert_eq!(answer["reason"], "card_invalid", "{answer}");
    let fields: Vec<&str> = answer["detail"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| field["field"].as_str().unwrap())
        .collect();
    assert_eq!(fields, vec!["goal", "criteria"]);

    let answer = h.add("Depends on nothing real", &["T-9"]);
    assert_eq!(answer["reason"], "card_invalid", "{answer}");
    assert!(
        h.engine.tasks_of(&f).next().is_none(),
        "a refused card creates no Task"
    );
}

#[test]
fn a_dependency_that_closes_a_loop_is_refused_with_its_path() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    set_workers(&mut h, &f, 1);
    h.world().hold_judgments = true;
    let a = h.add("A", &[])["task"]["id"].as_str().unwrap().to_owned();
    let b = h.add("B", &[&a])["task"]["id"].as_str().unwrap().to_owned();
    let c = h.add("C", &[&b])["task"]["id"].as_str().unwrap().to_owned();
    let refused = h.op(Command::Dep {
        task: a.clone(),
        on: c.clone(),
        remove: false,
    });
    assert_eq!(refused["reason"], "dependency_cycle", "{refused}");
    assert_eq!(refused["detail"]["cycle"], json!([c, b, a]));
}

#[test]
fn a_review_that_cannot_run_keeps_the_task_drafting_and_asks_for_the_provider() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.world().judge_down = true;
    let answer = h.add("Needs review", &[]);
    let id = answer["task"]["id"].as_str().unwrap().to_owned();
    assert_eq!(answer["result"], "pending", "{answer}");
    h.engine.tick();
    assert_eq!(h.state(&f, &id), TaskState::Drafting, "never skipped (B19)");
    let inbox = h.op(Command::Inbox);
    assert!(
        inbox["items"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Background AI"),
        "{inbox}"
    );

    h.world().judge_down = false;
    let question = open_question(&h, &f, &id);
    h.op(Command::Answer {
        task: id.clone(),
        question: Some(question.id),
        choice: Some("retry-review".into()),
        text: None,
    });
    h.engine.tick();
    assert_ne!(h.state(&f, &id), TaskState::Drafting);
}

#[test]
fn review_questions_hold_the_task_until_answered_and_re_adding_is_idempotent() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.world().intake.insert(
        "Vague".into(),
        json!({"questions": [{"text": "Which API?", "suggestion": "REST"}], "dependencies": [], "flags": [], "split": []}),
    );
    let id = h.add("Vague", &[])["task"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    h.engine.tick();
    assert_eq!(h.engine.add_answer(&f, &id)["result"], "needs_answers");
    assert_eq!(h.state(&f, &id), TaskState::Drafting);

    // The same card again: still one Task (B9, B73).
    let again = h.op(Command::Add {
        project: None,
        task: Some(id.clone()),
        issue: None,
        card: card("Vague", &[]),
        producer_pane: None,
    });
    assert_eq!(again["task"]["id"], id.as_str());
    assert_eq!(h.engine.tasks_of(&f).count(), 1);

    let question = open_question(&h, &f, &id);
    h.op(Command::Answer {
        task: id.clone(),
        question: Some(question.id),
        choice: Some("suggestion".into()),
        text: None,
    });
    assert_eq!(h.task(&f, &id).issue, Some(IssueRef::Local { number: 1 }));
    assert_ne!(h.state(&f, &id), TaskState::Drafting);
    assert_eq!(h.writes("issue.create").len(), 1);
}

#[test]
fn a_re_add_keeps_a_dependency_a_person_added() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let a = h.ready("First", &[]);
    let b = h.add("Second", &[])["task"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let added = h.op(Command::Dep {
        task: b.clone(),
        on: a.clone(),
        remove: false,
    });
    assert_eq!(added["ok"], true, "{added}");
    h.engine.tick();
    // The producer sends the card it knows, without that edge.
    for _ in 0..2 {
        h.op(Command::Add {
            project: None,
            task: Some(b.clone()),
            issue: None,
            card: card("Second", &[]),
            producer_pane: None,
        });
    }
    let task = h.task(&f, &b);
    assert_eq!(task.card.depends_on, vec![a.clone()]);
    assert!(
        !task
            .open_questions()
            .any(|q| matches!(q.kind, QuestionKind::ScopeChange { .. })),
        "the same card is no scope change"
    );
}

#[test]
fn a_prd_changed_while_running_becomes_the_task_s_only_once_approved() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Spec", &[]);
    assert_eq!(h.state(&f, &t), TaskState::Running);
    let prd = h.dir.path().join("prd.md");
    std::fs::write(&prd, "# v2\n").unwrap();
    let mut changed = card("Spec", &[]);
    changed.goal = Some("Make Spec work the v2 way".into());
    changed.prd = Some(prd.to_string_lossy().into_owned());
    let readd = |h: &mut Bench, card: CardInput| {
        h.op(Command::Add {
            project: None,
            task: Some(t.clone()),
            issue: None,
            card,
            producer_pane: None,
        })
    };
    readd(&mut h, changed.clone());
    readd(&mut h, changed);
    let before = h.task(&f, &t);
    assert_eq!(
        before.card.goal, "Make Spec work",
        "nothing changes before approval"
    );
    assert!(before.attachments.is_empty());
    assert_eq!(
        before.open_questions().count(),
        1,
        "a second re-add replaces the first"
    );

    let question = open_question(&h, &f, &t);
    h.op(Command::Answer {
        task: t.clone(),
        question: Some(question.id),
        choice: Some("approve".into()),
        text: None,
    });
    let after = h.task(&f, &t);
    assert_eq!(after.card.goal, "Make Spec work the v2 way");
    assert_eq!(after.attachments.len(), 1);
    let path = after.attachments[0].path.clone();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "# v2\n");
    let told = h
        .world()
        .messages
        .iter()
        .any(|(task, body)| *task == t && body.contains(&path) && body.contains("the v2 way"));
    assert!(told, "the worker gets the new card and the PRD copy");
}

// --------------------------------------------------------------------- DAG

/// Six Tasks with a fork and a join run in topological order with at most
/// two workers, and each starts only after its predecessors merged (B20, B21).
#[test]
fn a_six_task_graph_runs_in_dependency_order_within_the_slot_cap() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    set_workers(&mut h, &f, 2);
    h.world().hold_judgments = true;
    let t1 = h.add("Root", &[])["task"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let t2 = h.add("Left", &[&t1])["task"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let t3 = h.add("Right", &[&t1])["task"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let t4 = h.add("Join", &[&t2, &t3])["task"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let t5 = h.add("Alone", &[])["task"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let t6 = h.add("Last", &[&t4])["task"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    h.world().hold_judgments = false;

    let mut starts: Vec<String> = Vec::new();
    let mut max_running = 0;
    let mut concurrent = false;
    for _ in 0..60 {
        h.engine.tick();
        let running: Vec<String> = h
            .engine
            .tasks_of(&f)
            .filter(|task| task.state == TaskState::Running)
            .map(|task| task.id.clone())
            .collect();
        max_running = max_running.max(running.len());
        concurrent |= running.len() == 2;
        for id in &running {
            if !starts.contains(id) {
                starts.push(id.clone());
            }
            h.done(&f, id);
        }
        if h.engine
            .tasks_of(&f)
            .all(|task| task.state == TaskState::Done)
        {
            break;
        }
    }
    assert!(
        h.engine
            .tasks_of(&f)
            .all(|task| task.state == TaskState::Done),
        "every Task finished"
    );
    assert_eq!(max_running, 2, "never more than the slot cap");
    assert!(concurrent, "independent Tasks ran together");
    let merges: Vec<String> = h
        .writes("merge ")
        .iter()
        .map(|w| w.trim_start_matches("merge ").to_owned())
        .collect();
    assert_eq!(merges.len(), 6, "each Task merged exactly once: {merges:?}");
    let merged_at = |id: &str| merges.iter().position(|m| m == id).unwrap();
    let started_at = |id: &str| h.world().spawned.iter().position(|s| s.task == id).unwrap();
    for (task, predecessors) in [
        (&t2, vec![&t1]),
        (&t3, vec![&t1]),
        (&t4, vec![&t2, &t3]),
        (&t6, vec![&t4]),
    ] {
        for predecessor in predecessors {
            assert!(
                merged_at(predecessor) < merged_at(task),
                "{predecessor} merges before {task}"
            );
            assert!(started_at(predecessor) < started_at(task));
        }
    }
    // Priority equal: oldest first; the root and the lone Task go first.
    assert_eq!(&starts[..2], &[t1.clone(), t5.clone()]);
    assert_eq!(h.world().spawned.len(), 6, "one worker per Task");
}

#[test]
fn a_dependent_starts_only_after_its_predecessor_merged_not_when_it_is_verifying() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let a = h.ready("A", &[]);
    let b = h.ready("B", &[&a]);
    assert_eq!(h.state(&f, &b), TaskState::Waiting);
    h.world()
        .verify
        .insert(a.clone(), [VerifyPoll::Pending, VerifyPoll::Pending].into());
    h.done(&f, &a);
    h.engine.tick();
    assert_eq!(h.state(&f, &a), TaskState::Verifying);
    assert_eq!(
        h.state(&f, &b),
        TaskState::Waiting,
        "verifying is not merged"
    );
    let card = h.engine.summary().factories[0].columns[1].cards.clone();
    assert_eq!(card[0].waiting_for.as_deref(), Some("L-1"));
    tick_until(&mut h, &f, &a, TaskState::Done);
    tick_until(&mut h, &f, &b, TaskState::Running);
}

#[test]
fn slots_go_by_priority_then_age_across_factories() {
    let mut h = Bench::new(false);
    let f1 = h.factory(true);
    let f2 = h.factory_at("/work/other", true);
    set_workers(&mut h, &f1, 1);
    h.world().hold_judgments = true;
    let old = h.add("Old", &[])["task"]["id"].as_str().unwrap().to_owned();
    h.advance(MINUTE_MS);
    let answer = h.op(Command::Add {
        project: Some("/work/other".into()),
        task: None,
        issue: None,
        card: CardInput {
            priority: Some(5),
            ..card("Urgent elsewhere", &[])
        },
        producer_pane: None,
    });
    let urgent = answer["task"]["id"].as_str().unwrap().to_owned();
    h.world().hold_judgments = false;
    h.engine.tick();
    assert_eq!(h.state(&f2, &urgent), TaskState::Running);
    assert_eq!(h.state(&f1, &old), TaskState::Waiting);
    h.done(&f2, &urgent);
    tick_until(&mut h, &f1, &old, TaskState::Running);
}

// ------------------------------------------------------------- worker reports

#[test]
fn a_question_with_a_default_waits_only_before_merge_and_the_deadline_applies_it() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Asks", &[]);
    let answer = h.as_worker(
        &f,
        &t,
        Command::Ask {
            text: "Name the flag?".into(),
            suggestion: "--fast".into(),
            default_action: "use --fast".into(),
            deadline_hours: Some(24),
            letter: None,
        },
    );
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(
        h.state(&f, &t),
        TaskState::Running,
        "the worker keeps going (B26)"
    );
    h.done(&f, &t);
    for _ in 0..5 {
        h.engine.tick();
    }
    assert_eq!(
        h.state(&f, &t),
        TaskState::Verifying,
        "merge waits for the answer or the deadline"
    );
    assert!(h.writes("merge").is_empty());
    h.advance(24 * HOUR_MS);
    tick_until(&mut h, &f, &t, TaskState::Done);
    let question = &h.task(&f, &t).questions[0];
    assert_eq!(question.answer.as_ref().unwrap().relayed_by, "deadline");
}

#[test]
fn a_different_answer_before_the_deadline_sends_the_worker_back() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Asks", &[]);
    h.as_worker(
        &f,
        &t,
        Command::Ask {
            text: "Name the flag?".into(),
            suggestion: "--fast".into(),
            default_action: "use --fast".into(),
            deadline_hours: Some(24),
            letter: None,
        },
    );
    h.done(&f, &t);
    h.engine.tick();
    let question = open_question(&h, &f, &t);
    h.op(Command::Answer {
        task: t.clone(),
        question: Some(question.id),
        choice: None,
        text: Some("use --quick".into()),
    });
    assert_eq!(h.state(&f, &t), TaskState::Running);
    assert!(
        h.world()
            .wakes
            .iter()
            .any(|(task, body)| *task == t && body.contains("--quick"))
    );
}

#[test]
fn a_question_needs_a_suggestion_and_a_deadline() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Asks", &[]);
    let refused = h.as_worker(
        &f,
        &t,
        Command::Block {
            text: "Which?".into(),
            suggestion: " ".into(),
            deadline_hours: Some(24),
            letter: None,
        },
    );
    assert_eq!(refused["reason"], "suggestion_required", "{refused}");
    let refused = h.as_worker(
        &f,
        &t,
        Command::Block {
            text: "Which?".into(),
            suggestion: "this one".into(),
            deadline_hours: None,
            letter: None,
        },
    );
    assert_eq!(refused["reason"], "deadline_required", "{refused}");
    assert!(h.task(&f, &t).questions.is_empty());
}

#[test]
fn a_blocking_question_releases_the_slot_and_the_answer_wakes_the_same_session() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    set_workers(&mut h, &f, 1);
    let blocked = h.ready("Blocks", &[]);
    let other = h.ready("Other", &[]);
    assert_eq!(h.state(&f, &other), TaskState::Waiting);
    h.as_worker(
        &f,
        &blocked,
        Command::Block {
            text: "Which database?".into(),
            suggestion: "sqlite".into(),
            deadline_hours: Some(24),
            letter: Some("letter-7".into()),
        },
    );
    assert_eq!(h.state(&f, &blocked), TaskState::Blocked);
    assert!(h.world().sleeps.contains(&blocked));
    h.engine.tick();
    assert_eq!(
        h.state(&f, &other),
        TaskState::Running,
        "the slot went to the next Task"
    );

    // No automatic progress past the deadline; it rises as days waited.
    h.advance(2 * DAY_MS);
    h.engine.tick();
    assert_eq!(h.state(&f, &blocked), TaskState::Blocked);
    let inbox = h.op(Command::Inbox);
    assert_eq!(inbox["items"][0]["remaining"], "2일째 기다림", "{inbox}");

    let question = open_question(&h, &f, &blocked);
    h.op(Command::Answer {
        task: blocked.clone(),
        question: Some(question.id),
        choice: Some("suggestion".into()),
        text: None,
    });
    assert_eq!(h.state(&f, &blocked), TaskState::Waiting);
    h.done(&f, &other);
    tick_until(&mut h, &f, &blocked, TaskState::Running);
    let spawned = h
        .world()
        .spawned
        .iter()
        .filter(|s| s.task == blocked)
        .count();
    assert_eq!(spawned, 1, "the same session, not a new worker");
    assert!(
        h.world()
            .wakes
            .iter()
            .any(|(task, body)| *task == blocked && body.contains("sqlite"))
    );
}

#[test]
fn verification_fails_count_toward_the_limit_and_then_stop_the_task() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Flaky", &[]);
    let failed = || VerifyPoll::Failed {
        check: "cargo test".into(),
        link: "log".into(),
    };
    h.world()
        .verify
        .insert(t.clone(), [failed(), failed(), failed()].into());
    for round in 1..=2 {
        h.done(&f, &t);
        h.engine.tick();
        assert_eq!(
            h.state(&f, &t),
            TaskState::Running,
            "round {round} goes back to the worker"
        );
        assert_eq!(h.task(&f, &t).failures, round);
        let wake = h.world().wakes.last().cloned().unwrap();
        assert!(wake.1.contains(&format!("({round}/3)")), "{wake:?}");
    }
    h.done(&f, &t);
    h.engine.tick();
    let task = h.task(&f, &t);
    assert_eq!(
        (task.state, task.stop),
        (TaskState::Stopped, Some(StopReason::VerifyFailed))
    );
    assert_eq!(
        h.world().verify_runs.len(),
        3,
        "a failure never runs again by itself (D-23)"
    );
    let refused = h.op(Command::Merge { task: t.clone() });
    assert_eq!(refused["reason"], "action_not_allowed_in_state");
    assert_eq!(
        refused["detail"]["allowed"],
        json!(["answer", "retry", "cancel"])
    );
    h.op(Command::Retry { task: t.clone() });
    assert_eq!(h.task(&f, &t).failures, 0);
    tick_until(&mut h, &f, &t, TaskState::Running);
}

#[test]
fn an_environment_failure_is_not_counted_and_the_task_waits() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Unlucky", &[]);
    h.world().verify.insert(
        t.clone(),
        [VerifyPoll::Environment {
            signal: EnvSignal::Network,
            check: "cargo test".into(),
        }]
        .into(),
    );
    h.done(&f, &t);
    h.engine.tick();
    let task = h.task(&f, &t);
    assert_eq!(task.failures, 0);
    assert_eq!(task.environment_failures, 1);
    tick_until(&mut h, &f, &t, TaskState::Running);
}

#[test]
fn three_tasks_failing_the_same_check_together_are_the_environment() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let ids: Vec<String> = ["One", "Two", "Three"]
        .iter()
        .map(|t| h.ready(t, &[]))
        .collect();
    for id in &ids {
        h.world().verify.insert(
            id.clone(),
            [VerifyPoll::Failed {
                check: "cargo test".into(),
                link: "l".into(),
            }]
            .into(),
        );
        h.done(&f, id);
    }
    h.engine.tick();
    for id in &ids {
        let task = h.task(&f, id);
        assert_eq!(task.failures, 0, "{id}: counts go back (B60)");
        assert_eq!(task.state, TaskState::Waiting, "{id}");
    }
    h.engine.tick();
    assert!(
        ids.iter().all(|id| h.state(&f, id) == TaskState::Waiting),
        "new starts halt"
    );
    h.advance(31 * MINUTE_MS);
    h.engine.tick();
    assert!(ids.iter().all(|id| h.state(&f, id) == TaskState::Running));
}

#[test]
fn a_turn_ended_without_a_report_stops_the_task() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Quiet", &[]);
    let since = h.world().now + MINUTE_MS;
    h.world()
        .worker_status
        .insert(t.clone(), WorkerStatus::Resting { since });
    h.advance(2 * MINUTE_MS);
    h.engine.tick();
    assert_eq!(
        h.state(&f, &t),
        TaskState::Running,
        "one minute at rest is not yet two"
    );
    h.advance(MINUTE_MS);
    h.engine.tick();
    let task = h.task(&f, &t);
    assert_eq!(
        (task.state, task.stop),
        (TaskState::Stopped, Some(StopReason::NoReport))
    );
}

#[test]
fn a_watch_inactivity_letter_about_a_worker_marks_it_stalled() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Stuck", &[]);
    let pane = h.task(&f, &t).worker.unwrap().pane.unwrap();
    let answer = h.engine.letter(Inbound {
        id: "w-1".into(),
        factory: f.clone(),
        sender_pane: pane,
        kind: "watch".into(),
        body: "no activity for 30 minutes".into(),
    });
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(h.task(&f, &t).stop, Some(StopReason::Stalled));
}

#[test]
fn a_plain_letter_from_a_harness_becomes_a_blocking_question_once() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Bench", &[]);
    let pane = h.task(&f, &t).worker.unwrap().pane.unwrap();
    let letter = Inbound {
        id: "letter-1".into(),
        factory: f.clone(),
        sender_pane: pane.clone(),
        kind: "block".into(),
        body: "Should the API be versioned?\nRecommendation: yes, v1".into(),
    };
    h.engine.letter(letter.clone());
    let duplicate = h.engine.letter(letter);
    assert_eq!(duplicate["duplicate"], true);
    let task = h.task(&f, &t);
    assert_eq!(task.state, TaskState::Blocked);
    assert_eq!(task.questions.len(), 1);
    assert_eq!(task.questions[0].suggestion, "yes, v1");
    assert_eq!(task.questions[0].letter.as_deref(), Some("letter-1"));

    // A pane that is no Factory worker cannot report.
    let refused = h.engine.letter(Inbound {
        id: "x".into(),
        factory: f,
        sender_pane: "stranger".into(),
        kind: "report".into(),
        body: "done".into(),
    });
    assert_eq!(refused["reason"], "sender_not_a_worker");
}

#[test]
fn a_cancelled_task_s_worker_stays_a_worker_and_is_stopped() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Mine", &[]);
    let worker = h.task(&f, &t).worker.unwrap();
    let pane = worker.pane.clone().unwrap();
    let answer = h.op(Command::Cancel { task: t.clone() });
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(h.task(&f, &t).state, TaskState::Cancelled);
    assert!(h.world().stops.contains(&t), "the worker is stopped");
    // Its pane and its folder still name the Task, never an operator.
    let bound = Some((f.clone(), t.clone()));
    assert_eq!(h.engine.role_for(Some(&pane), None), bound);
    let inside = format!("{}/src", worker.worktree);
    assert_eq!(h.engine.role_for(None, Some(&inside)), bound);
}

#[test]
fn a_worker_s_reports_stop_at_the_cap() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Chatty", &[]);
    for n in 0..hide_factory::engine::REPORT_LIMIT {
        let kept = h.as_worker(
            &f,
            &t,
            Command::Decide {
                text: format!("d{n}"),
            },
        );
        assert_eq!(kept["ok"], true, "{kept}");
    }
    let refused = h.as_worker(
        &f,
        &t,
        Command::Decide {
            text: "one more".into(),
        },
    );
    assert_eq!(refused["reason"], "report_limit", "{refused}");
    assert_eq!(
        h.task(&f, &t).decisions.len(),
        hide_factory::engine::REPORT_LIMIT
    );
}

#[test]
fn a_worker_cannot_act_as_a_person() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Mine", &[]);
    for command in [
        Command::Merge { task: t.clone() },
        Command::Answer {
            task: t.clone(),
            question: None,
            choice: None,
            text: Some("x".into()),
        },
        Command::Config {
            project: None,
            set: vec![("merge_mode".into(), "manual".into())],
        },
        Command::Cancel { task: t.clone() },
    ] {
        let refused = h.as_worker(&f, &t, command);
        assert_eq!(refused["reason"], "role_not_allowed", "{refused}");
    }
    // An operator has no Task of its own to report on.
    let refused = h.op(Command::Done {
        summary: None,
        breaking: false,
        letter: None,
    });
    assert_eq!(refused["reason"], "role_not_allowed");
}

// ---------------------------------------------------------------- discoveries

#[test]
fn a_proposed_task_waits_for_a_person_and_its_worker_cannot_propose() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Parent", &[]);
    let answer = h.as_worker(
        &f,
        &t,
        Command::Propose {
            class: DiscoveryClass::Prerequisite,
            text: "needs a helper".into(),
            card: Some(card("Helper", &[])),
            autonomy: None,
            reclassify: None,
            letter: None,
        },
    );
    assert_eq!(answer["task"], serde_json::Value::Null, "{answer}");
    assert_eq!(
        h.engine.tasks_of(&f).count(),
        1,
        "nothing starts without a person (B30)"
    );
    let question = open_question(&h, &f, &t);
    h.op(Command::Answer {
        task: t.clone(),
        question: Some(question.id),
        choice: Some("approve".into()),
        text: None,
    });
    let child = h
        .engine
        .tasks_of(&f)
        .find(|task| task.card.title == "Helper")
        .unwrap()
        .id
        .clone();
    assert_eq!(h.task(&f, &child).proposed_by.as_deref(), Some(t.as_str()));
    assert_eq!(h.task(&f, &t).card.depends_on, vec![child.clone()]);
    assert_eq!(
        h.task(&f, &t).state,
        TaskState::Blocked,
        "the proposer waits on the approved prerequisite and gives its slot back (D-16)"
    );
    tick_until(&mut h, &f, &child, TaskState::Running);
    let refused = h.as_worker(
        &f,
        &child,
        Command::Propose {
            class: DiscoveryClass::Prerequisite,
            text: "and another".into(),
            card: Some(card("Grandchild", &[])),
            autonomy: None,
            reclassify: None,
            letter: None,
        },
    );
    assert_eq!(refused["reason"], "proposal_depth_exceeded");

    // Reclassifying away from a person is refused.
    h.as_worker(
        &f,
        &t,
        Command::Propose {
            class: DiscoveryClass::Unrelated,
            text: "odd log".into(),
            card: None,
            autonomy: None,
            reclassify: None,
            letter: None,
        },
    );
    let discovery = h.task(&f, &t).discoveries.last().unwrap().id.clone();
    let refused = h.as_worker(
        &f,
        &t,
        Command::Propose {
            class: DiscoveryClass::InScope,
            text: "odd log".into(),
            card: None,
            autonomy: None,
            reclassify: Some(discovery),
            letter: None,
        },
    );
    assert_eq!(
        refused["reason"], "reclassify_away_from_person",
        "{refused}"
    );
}

#[test]
fn a_proposal_the_review_finds_outside_its_scope_waits_for_a_person() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.op(Command::Config {
        project: None,
        set: vec![("autonomy".into(), "lint_format=on".into())],
    });
    h.world().intake.insert(
        "Rewrite the parser".into(),
        json!({"questions": [], "dependencies": [], "flags": [], "split": [], "fits_scope": false}),
    );
    let t = h.ready("Parent", &[]);
    let answer = h.as_worker(
        &f,
        &t,
        Command::Propose {
            class: DiscoveryClass::Prerequisite,
            text: "needs a new parser".into(),
            card: Some(card("Rewrite the parser", &[])),
            autonomy: Some("lint_format".into()),
            reclassify: None,
            letter: None,
        },
    );
    let child = answer["task"].as_str().unwrap().to_owned();
    for _ in 0..5 {
        h.engine.tick();
    }
    let task = h.task(&f, &child);
    assert_eq!(task.state, TaskState::Drafting, "it does not start alone");
    assert_eq!(task.autonomy, None);
    let question = open_question(&h, &f, &child);
    assert!(
        question.text.contains("Lint and format"),
        "{}",
        question.text
    );
    // A person's approval lets it run as an ordinary Task.
    h.op(Command::Answer {
        task: child.clone(),
        question: Some(question.id),
        choice: Some("suggestion".into()),
        text: None,
    });
    tick_until(&mut h, &f, &child, TaskState::Running);
}

#[test]
fn an_enabled_autonomy_scope_starts_alone_and_the_third_new_task_stops_the_parent() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.op(Command::Config {
        project: None,
        set: vec![("autonomy".into(), "lint_format=on".into())],
    });
    let t = h.ready("Parent", &[]);
    let watches = |h: &Bench| {
        let world = h.world();
        world
            .submitted
            .iter()
            .chain(&world.judged)
            .filter(|j| matches!(j.input, JudgmentInput::Watch { .. }))
            .count()
    };
    let mut watched = 0;
    for n in 1..=3 {
        if h.state(&f, &t) != TaskState::Running {
            h.op(Command::Resume { task: t.clone() });
        }
        watched = watches(&h);
        let answer = h.as_worker(
            &f,
            &t,
            Command::Propose {
                class: DiscoveryClass::Prerequisite,
                text: format!("lint {n}"),
                card: Some(card(&format!("Lint {n}"), &[])),
                autonomy: Some("lint_format".into()),
                reclassify: None,
                letter: None,
            },
        );
        assert_eq!(answer["ok"], true, "{answer}");
        let child = answer["task"].as_str().unwrap().to_owned();
        assert!(
            h.task(&f, &t).card.depends_on.contains(&child),
            "the parent waits on its prerequisite"
        );
        if n < 3 {
            // Run the prerequisite to completion so the parent runs again.
            tick_until(&mut h, &f, &child, TaskState::Running);
            h.done(&f, &child);
            tick_until(&mut h, &f, &child, TaskState::Done);
            tick_until(&mut h, &f, &t, TaskState::Running);
        }
    }
    let task = h.task(&f, &t);
    assert_eq!(
        (task.state, task.stop),
        (TaskState::Stopped, Some(StopReason::NewTaskCap))
    );
    assert_eq!(
        watches(&h),
        watched + 1,
        "reaching the cap reads the board (B69)"
    );
    assert!(matches!(
        open_question(&h, &f, &t).kind,
        QuestionKind::NewTaskCap
    ));
}

#[test]
fn an_autonomy_task_over_the_diff_limit_goes_to_a_person() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.op(Command::Config {
        project: None,
        set: vec![("autonomy".into(), "docs_links=on".into())],
    });
    let t = h.ready("Parent", &[]);
    let answer = h.as_worker(
        &f,
        &t,
        Command::Propose {
            class: DiscoveryClass::Prerequisite,
            text: "fix links".into(),
            card: Some(card("Links", &[])),
            autonomy: Some("docs_links".into()),
            reclassify: None,
            letter: None,
        },
    );
    let child = answer["task"].as_str().unwrap().to_owned();
    h.world().diff_lines = 500;
    tick_until(&mut h, &f, &child, TaskState::Running);
    h.done(&f, &child);
    tick_until(&mut h, &f, &child, TaskState::MergeWaiting);
    assert_eq!(h.task(&f, &child).gates, vec![Gate::AutonomyDiff]);
}

// ---------------------------------------------------------------- GitHub path

fn github_factory(h: &mut Bench, mode: MergeMode) -> String {
    let created = h.op(Command::Init {
        project: PROJECT.into(),
        verification: Some(VerificationChoice::Ci {
            checks: vec!["test".into()],
        }),
        merge_mode: Some(mode),
        confirm: true,
    });
    created["factory"]["id"].as_str().unwrap().to_owned()
}

#[test]
fn ci_with_no_named_check_takes_the_required_checks_and_never_none() {
    let mut h = Bench::new(true);
    let created = h.op(Command::Init {
        project: PROJECT.into(),
        verification: Some(VerificationChoice::Ci { checks: vec![] }),
        merge_mode: Some(MergeMode::Auto),
        confirm: true,
    });
    assert_eq!(created["ok"], true, "{created}");
    let factory = h.engine.factories().next().unwrap().clone();
    assert_eq!(
        factory.config.verification,
        Verification::Ci {
            checks: vec!["test".into()]
        },
        "the branch protection's required checks stand in"
    );
    let refused = h.op(Command::Config {
        project: Some(PROJECT.into()),
        set: vec![("ci".into(), " , ".into())],
    });
    assert_eq!(refused["reason"], "ci_checks_required", "{refused}");
}

#[test]
fn each_github_report_pushes_before_its_checks_are_read_and_answers_at_once() {
    let mut h = Bench::new(true);
    let f = github_factory(&mut h, MergeMode::Auto);
    let t = h.ready("Fix again", &[]);
    h.world().verify.insert(
        t.clone(),
        [VerifyPoll::Failed {
            check: "test".into(),
            link: "ci".into(),
        }]
        .into(),
    );
    let answer = h.done(&f, &t);
    assert_eq!(answer["state"], "verifying", "{answer}");
    assert!(
        h.world().pushes.is_empty(),
        "the reply does not wait on git"
    );
    assert!(h.world().verify_runs.is_empty());
    tick_until(&mut h, &f, &t, TaskState::Running);
    assert_eq!(h.world().pushes, vec![t.clone()]);
    h.done(&f, &t);
    tick_until(&mut h, &f, &t, TaskState::Done);
    assert_eq!(h.world().pushes, vec![t.clone(), t.clone()]);
    assert_eq!(h.writes("pr.open").len(), 1, "one pull request");
}

#[test]
fn a_manual_task_waits_for_merge_and_every_github_write_happens_once() {
    let mut h = Bench::new(true);
    let f = github_factory(&mut h, MergeMode::Manual);
    let t = h.ready("Ship it", &[]);
    assert_eq!(h.task(&f, &t).issue, Some(IssueRef::Github { number: 101 }));
    h.done(&f, &t);
    tick_until(&mut h, &f, &t, TaskState::MergeWaiting);
    assert_eq!(h.task(&f, &t).gates, vec![Gate::ManualMode]);
    let inbox = h.op(Command::Inbox);
    assert_eq!(inbox["items"][0]["group"], "merge", "{inbox}");
    for _ in 0..5 {
        h.engine.tick();
    }
    let merged = h.op(Command::Merge { task: t.clone() });
    assert_eq!(merged["ok"], true, "{merged}");
    assert_eq!(h.state(&f, &t), TaskState::Done);
    for _ in 0..5 {
        h.engine.tick();
    }
    assert_eq!(
        h.world().writes,
        vec![
            "label.create factory",
            "issue.create T-1",
            "pr.open T-1",
            "merge T-1"
        ],
    );
    assert_eq!(
        h.world().removed,
        vec![t.clone()],
        "worktree goes once main is green (B43)"
    );
}

#[test]
fn human_gates_hold_an_auto_task_in_merge_waiting() {
    let mut h = Bench::new(true);
    let f = github_factory(&mut h, MergeMode::Auto);
    let t = h.add_card(CardInput {
        review_directly: true,
        ..card("Look at me", &[])
    })["task"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    h.engine.tick();
    let b = h.ready("Breaking", &[]);
    h.done(&f, &t);
    h.as_worker(
        &f,
        &b,
        Command::Done {
            summary: None,
            breaking: true,
            letter: None,
        },
    );
    tick_until(&mut h, &f, &t, TaskState::MergeWaiting);
    tick_until(&mut h, &f, &b, TaskState::MergeWaiting);
    assert_eq!(h.task(&f, &t).gates, vec![Gate::ReviewDirectly]);
    assert_eq!(h.task(&f, &b).gates, vec![Gate::BreakingChange]);
    let refused = h.op(Command::RequestChanges {
        task: b.clone(),
        comment: String::new(),
    });
    assert_eq!(refused["reason"], "comment_required");
    h.op(Command::RequestChanges {
        task: b.clone(),
        comment: "keep the old name".into(),
    });
    assert_eq!(h.state(&f, &b), TaskState::Running);
}

#[test]
fn a_conflict_sends_the_worker_to_rebase_without_counting_a_failure() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Conflicts", &[]);
    h.world().premerge.insert(
        t.clone(),
        [hide_factory::adapters::PreMerge::Conflict {
            files: vec!["src/lib.rs".into()],
        }]
        .into(),
    );
    h.done(&f, &t);
    tick_until(&mut h, &f, &t, TaskState::Running);
    assert_eq!(h.task(&f, &t).failures, 0);
    assert!(h.world().wakes.last().unwrap().1.contains("rebase"));
}

#[test]
fn a_dirty_local_main_holds_the_merge() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Local", &[]);
    h.world().main_dirty = true;
    h.done(&f, &t);
    tick_until(&mut h, &f, &t, TaskState::MergeWaiting);
    assert_eq!(h.task(&f, &t).gates, vec![Gate::DirtyMain]);
    let refused = h.op(Command::Merge { task: t.clone() });
    assert_eq!(refused["reason"], "main_dirty");
    h.world().main_dirty = false;
    let merged = h.op(Command::Merge { task: t.clone() });
    assert_eq!(merged["ok"], true, "{merged}");
}

#[test]
fn a_refused_merge_waits_for_a_person_with_its_reason_and_is_tried_once() {
    let mut h = Bench::new(true);
    let f = github_factory(&mut h, MergeMode::Auto);
    let t = h.ready("Refused", &[]);
    h.world().merge_refusal = Some(hide_factory::adapters::Failure::task(
        "github.merge",
        "base branch policy prohibits the merge",
    ));
    h.done(&f, &t);
    tick_until(&mut h, &f, &t, TaskState::MergeWaiting);
    for _ in 0..10 {
        h.engine.tick();
    }
    assert_eq!(h.world().merge_attempts, 1, "not retried every tick");
    assert_eq!(h.task(&f, &t).gates, vec![Gate::MergeRefused]);
    // A person's merge tries again and lands.
    let merged = h.op(Command::Merge { task: t.clone() });
    assert_eq!(merged["ok"], true, "{merged}");
}

#[test]
fn a_broken_main_holds_auto_merge_without_reading_it_every_tick() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.engine.tick();
    {
        let mut world = h.world();
        world.head = "outside1".into();
        world.main_checks.insert(
            "outside1".into(),
            MainCheck::Red {
                link: "run/9".into(),
            },
        );
    }
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    assert!(h.engine.factories().next().unwrap().main.broken);
    let t = h.ready("Held", &[]);
    h.done(&f, &t);
    let before = h.world().premerge_calls;
    for _ in 0..20 {
        h.engine.tick();
    }
    assert_eq!(h.world().premerge_calls, before, "no merge-tree on red");
    assert_eq!(h.state(&f, &t), TaskState::Verifying);
    assert!(h.writes("merge").is_empty());
}

#[test]
fn an_outside_push_still_running_its_checks_is_read_until_it_finishes() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.engine.tick();
    {
        let mut world = h.world();
        world.head = "outside2".into();
        world.main_checks.insert(
            "outside2".into(),
            MainCheck::Red {
                link: "run/10".into(),
            },
        );
        world.main_check_script.insert(
            "outside2".into(),
            [
                MainCheck::Pending,
                MainCheck::Pending,
                MainCheck::Red {
                    link: "run/10".into(),
                },
            ]
            .into(),
        );
    }
    // The push is read at the next outside read, then every paced interval.
    for _ in 0..6 {
        h.advance(MINUTE_MS);
        h.engine.tick();
    }
    let factory = h.engine.factories().next().unwrap().clone();
    assert!(factory.main.broken, "the late red result counts (B47)");
    assert!(h.world().main_check_script["outside2"].is_empty());
    assert_eq!(factory.id, f);
}

// ------------------------------------------------------------- main breaks

#[test]
fn a_recovery_cut_by_a_restart_goes_to_a_person() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let a = h.ready("A", &[]);
    let b = h.ready("B", &[]);
    {
        let mut world = h.world();
        world
            .main_checks
            .insert("sha0001-T-1".into(), MainCheck::Pending);
        world.main_checks.insert(
            "sha0002-T-2".into(),
            MainCheck::Red {
                link: "run/2".into(),
            },
        );
    }
    h.done(&f, &a);
    h.done(&f, &b);
    tick_until(&mut h, &f, &b, TaskState::Landed);
    h.engine.tick();
    let factory = h.engine.factories().next().unwrap().clone();
    assert!(factory.main.broken && factory.main.recovering);

    let mut h = h.restart();
    let factory = h.engine.factories().next().unwrap().clone();
    assert!(factory.main.needs_person, "nothing guesses the cut phase");
    assert!(!factory.main.recovering);
    let action = h
        .task(&f, &b)
        .open_questions()
        .find(|q| q.kind == QuestionKind::Action)
        .cloned()
        .expect("an action for a person");
    assert!(action.choices.contains(&"retry-revert".to_owned()));
    h.engine.tick();
    assert!(h.writes("revert").is_empty());
}

#[test]
fn a_broken_main_reverts_only_the_first_failing_merge_and_the_task_lands_again() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let a = h.ready("A", &[]);
    let b = h.ready("B", &[]);
    {
        let mut world = h.world();
        world
            .main_checks
            .insert("sha0001-T-1".into(), MainCheck::Pending);
        world.main_checks.insert(
            "sha0002-T-2".into(),
            MainCheck::Red {
                link: "run/2".into(),
            },
        );
    }
    h.done(&f, &a);
    h.done(&f, &b);
    tick_until(&mut h, &f, &b, TaskState::Landed);
    h.engine.tick();
    let factory = h.engine.factories().next().unwrap().clone();
    assert!(factory.main.broken, "one red run stops auto merge (B44)");
    assert_eq!(
        h.writes("main.rerun"),
        vec!["main.rerun sha0001-T-1"],
        "the pending run is asked again once"
    );
    assert!(
        h.writes("revert").is_empty(),
        "no revert before the culprit is known"
    );

    // A third Task verified meanwhile does not merge.
    let c = h.ready("C", &[]);
    h.done(&f, &c);
    h.engine.tick();
    h.engine.tick();
    assert!(h.writes("merge T-3").is_empty());

    h.world()
        .main_checks
        .insert("sha0001-T-1".into(), MainCheck::Green);
    tick_until(&mut h, &f, &b, TaskState::Running);
    assert_eq!(
        h.writes("revert"),
        vec!["revert.open T-2", "revert.merge T-2"]
    );
    assert_eq!(h.state(&f, &a), TaskState::Done);
    assert!(
        h.world()
            .wakes
            .iter()
            .any(|(task, body)| *task == b && body.contains("revert"))
    );
    assert_eq!(
        h.world().spawned.iter().filter(|s| s.task == b).count(),
        1,
        "the same Task, no new one (B46)"
    );
    tick_until(&mut h, &f, &c, TaskState::Done);
    h.done(&f, &b);
    tick_until(&mut h, &f, &b, TaskState::Done);
    assert_eq!(h.writes("merge T-2").len(), 2);
}

#[test]
fn an_undecidable_revert_stops_for_a_person() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let a = h.ready("A", &[]);
    h.world().main_checks.insert(
        "sha0001-T-1".into(),
        MainCheck::Red {
            link: "run/1".into(),
        },
    );
    h.world().revert_check = Some(MainCheck::Red {
        link: "run/r".into(),
    });
    h.done(&f, &a);
    tick_until(&mut h, &f, &a, TaskState::Landed);
    h.engine.tick();
    let factory = h.engine.factories().next().unwrap().clone();
    assert!(factory.main.needs_person, "B48");
    let question = h
        .task(&f, &a)
        .open_questions()
        .find(|q| matches!(q.kind, QuestionKind::Action))
        .cloned()
        .unwrap();
    assert!(question.choices.contains(&"revert T-1".to_owned()));
}

#[test]
fn an_outside_push_that_breaks_main_drafts_a_fix_and_never_reverts() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.engine.tick();
    {
        let mut world = h.world();
        world.head = "outside1".into();
        world.main_checks.insert(
            "outside1".into(),
            MainCheck::Red {
                link: "run/9".into(),
            },
        );
    }
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    let factory = h.engine.factories().next().unwrap().clone();
    assert!(factory.main.broken);
    let fix = h.engine.tasks_of(&f).next().unwrap().clone();
    assert_eq!(fix.state, TaskState::Drafting, "a draft a person confirms");
    assert!(h.writes("revert").is_empty());

    h.world()
        .main_checks
        .insert("outside1".into(), MainCheck::Green);
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    assert!(
        !h.engine.factories().next().unwrap().main.broken,
        "green again resumes auto merge"
    );
}

// ------------------------------------------------------------- outside work

#[test]
fn an_outside_pull_request_takes_a_running_task_and_its_merge_finishes_it() {
    let mut h = Bench::new(true);
    let f = github_factory(&mut h, MergeMode::Auto);
    let t = h.ready("Outside", &[]);
    let after = h.ready("After", &[&t]);
    let issue = h.task(&f, &t).issue.unwrap();
    h.world().outside.push_back(OutsideEvent::ClosingPr {
        issue: issue.clone(),
        pr: 55,
        url: "pr/55".into(),
        merged: false,
    });
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    assert_eq!(h.state(&f, &t), TaskState::Outside);
    assert!(h.world().stops.contains(&t), "the worker stops (B50)");
    assert!(h.world().removed.is_empty(), "the worktree stays");
    h.world().outside.push_back(OutsideEvent::ClosingPr {
        issue,
        pr: 55,
        url: "pr/55".into(),
        merged: true,
    });
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    assert_eq!(h.state(&f, &t), TaskState::Done);
    tick_until(&mut h, &f, &after, TaskState::Running);
    // The stopped worker's own work was never merged: its worktree keeps the
    // keep period from the takeover, not from the Task's last change.
    h.advance(6 * DAY_MS);
    h.engine.tick();
    assert!(h.world().removed.is_empty(), "the worktree stays 7 days");
    h.advance(2 * DAY_MS);
    h.engine.tick();
    assert_eq!(h.world().removed, vec![t.clone()]);
}

#[test]
fn a_task_waiting_long_on_a_person_keeps_its_worktree_after_an_outside_pull_request() {
    let mut h = Bench::new(true);
    let f = github_factory(&mut h, MergeMode::Auto);
    let t = h.ready("Waits", &[]);
    let pane = h.task(&f, &t).worker.unwrap().pane.unwrap();
    h.engine.letter(Inbound {
        id: "letter-1".into(),
        factory: f.clone(),
        sender_pane: pane,
        kind: "block".into(),
        body: "Which license?".into(),
    });
    assert_eq!(h.state(&f, &t), TaskState::Blocked);
    h.advance(9 * DAY_MS);
    let issue = h.task(&f, &t).issue.unwrap();
    h.world().outside.push_back(OutsideEvent::ClosingPr {
        issue,
        pr: 56,
        url: "pr/56".into(),
        merged: false,
    });
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    assert_eq!(h.state(&f, &t), TaskState::Outside);
    h.advance(6 * DAY_MS);
    h.engine.tick();
    assert!(h.world().removed.is_empty(), "{:?}", h.world().removed);
    let revived = h.op(Command::Revive { task: t.clone() });
    assert_eq!(revived["ok"], true, "{revived}");
}

#[test]
fn an_issue_closed_without_a_pull_request_cancels_and_a_label_creates_a_draft() {
    let mut h = Bench::new(true);
    let f = github_factory(&mut h, MergeMode::Auto);
    let t = h.ready("Closed", &[]);
    let issue = h.task(&f, &t).issue.unwrap();
    {
        let mut world = h.world();
        world.outside.push_back(OutsideEvent::IssueClosed { issue });
        world.outside.push_back(OutsideEvent::Labeled {
            issue: IssueRef::Github { number: 7 },
            title: "From label".into(),
            body: "Do it\n- [ ] works".into(),
        });
    }
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    assert_eq!(h.state(&f, &t), TaskState::Cancelled);
    let labelled = h
        .engine
        .tasks_of(&f)
        .find(|task| task.card.title == "From label")
        .unwrap()
        .clone();
    assert_eq!(
        labelled.state,
        TaskState::Drafting,
        "a person confirms the card (B15)"
    );
    assert_eq!(labelled.card.criteria, vec!["works"]);
    let question = open_question(&h, &f, &labelled.id);
    h.op(Command::Answer {
        task: labelled.id.clone(),
        question: Some(question.id),
        choice: Some("confirm".into()),
        text: None,
    });
    assert_eq!(
        h.writes("issue.create").len(),
        1,
        "the labelled issue is not created again"
    );
}

#[test]
fn three_failed_outside_reads_mark_the_board_stale_and_back_off() {
    let mut h = Bench::new(true);
    let _f = github_factory(&mut h, MergeMode::Auto);
    h.world().observe_failure = Some(Failure::environment(
        "observe",
        EnvSignal::GithubServer,
        "502",
    ));
    for _ in 0..3 {
        h.advance(40 * MINUTE_MS);
        h.engine.tick();
    }
    assert!(h.engine.summary().factories[0].stale);
    let reads = h.world().observed;
    h.engine.tick();
    assert_eq!(h.world().observed, reads, "backed off");
}

// -------------------------------------------------------------- environment

#[test]
fn a_low_disk_holds_new_starts_and_the_hold_clears_on_recheck() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.world().disk_free = Some(1 << 30);
    let t = h.ready("Held", &[]);
    assert_eq!(h.state(&f, &t), TaskState::Waiting);
    assert!(h.task(&f, &t).held.is_some());
    h.world().disk_free = Some(100 << 30);
    h.engine.tick();
    assert_eq!(
        h.state(&f, &t),
        TaskState::Waiting,
        "re-checked after a minute"
    );
    h.advance(MINUTE_MS);
    h.engine.tick();
    assert_eq!(h.state(&f, &t), TaskState::Running);

    // Critical memory pressure holds; warn starts (B57).
    let u = h.ready("Warned", &[]);
    let _ = u;
    h.world().memory = Some(MemoryPressure::Warn);
    h.advance(MINUTE_MS);
    h.engine.tick();
    assert_eq!(h.state(&f, &u), TaskState::Running);
}

#[test]
fn a_diagnosis_runs_an_enabled_recovery_and_an_approved_proposal_runs_too() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.world().spawn_failure = Some(Failure::task("worker.spawn", "refused"));
    let stopped = h.ready("Refused", &[]);
    assert_eq!(h.state(&f, &stopped), TaskState::Stopped);
    h.world().spawn_failure = None;
    h.world().disk_free = Some(1 << 30);
    let _held = h.ready("Held", &[]);
    let enabled = h.op(Command::Config {
        project: Some(PROJECT.into()),
        set: vec![("recovery".into(), "restart_worker=on".into())],
    });
    assert_eq!(enabled["ok"], true, "{enabled}");
    h.world().env_diagnosis = Some(json!({"cause": "disk", "action": "restart_worker"}));
    h.advance(31 * MINUTE_MS);
    h.engine.tick();
    h.engine.tick();
    assert_ne!(
        h.state(&f, &stopped),
        TaskState::Stopped,
        "the enabled action ran (B61)"
    );

    // An action outside the enabled scope is a proposal; approving runs it.
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.world().spawn_failure = Some(Failure::task("worker.spawn", "refused"));
    let stopped = h.ready("Refused", &[]);
    h.world().spawn_failure = None;
    h.world().disk_free = Some(1 << 30);
    let _held = h.ready("Held", &[]);
    h.world().env_diagnosis = Some(json!({"cause": "disk", "action": "restart_worker"}));
    h.advance(31 * MINUTE_MS);
    h.engine.tick();
    h.engine.tick();
    assert_eq!(h.state(&f, &stopped), TaskState::Stopped);
    let (owner, proposal) = h
        .engine
        .tasks_of(&f)
        .find_map(|t| {
            t.open_questions()
                .find(|q| matches!(q.kind, QuestionKind::Proposal { .. }))
                .map(|q| (t.id.clone(), q.id.clone()))
        })
        .expect("a proposal for a person");
    let answered = h.op(Command::Answer {
        task: owner,
        question: Some(proposal),
        choice: Some("approve".into()),
        text: None,
    });
    assert_eq!(answered["ok"], true, "{answered}");
    assert_ne!(h.state(&f, &stopped), TaskState::Stopped);
}

#[test]
fn a_usage_limit_moves_new_starts_to_the_other_runtime() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("First", &[]);
    let mut failure = Failure::environment("worker", EnvSignal::UsageLimit, "limit");
    failure.reset_at = Some(h.world().now + HOUR_MS);
    h.world().verify_start_failure = Some(failure);
    h.done(&f, &t);
    let u = h.ready("Second", &[]);
    assert_eq!(h.state(&f, &u), TaskState::Running);
    let runtimes: BTreeMap<String, Runtime> = h
        .world()
        .spawned
        .iter()
        .map(|s| (s.task.clone(), s.runtime))
        .collect();
    assert_eq!(runtimes[&t], Runtime::Claude);
    assert_eq!(runtimes[&u], Runtime::Codex);
}

#[test]
fn a_worker_its_usage_limit_stopped_waits_and_the_next_start_uses_the_other_runtime() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Limited", &[]);
    let now = h.world().now;
    h.world()
        .worker_status
        .insert(t.clone(), WorkerStatus::Resting { since: now });
    h.world()
        .usage_limits
        .insert(Runtime::Claude, now + HOUR_MS);
    h.advance(3 * MINUTE_MS);
    h.engine.tick();
    let task = h.task(&f, &t);
    assert_ne!(task.state, TaskState::Stopped, "not a no-report stop");
    assert_eq!(task.failures, 0);
    let u = h.ready("Next", &[]);
    let spawned = h
        .world()
        .spawned
        .iter()
        .find(|s| s.task == u)
        .map(|s| s.runtime);
    assert_eq!(spawned, Some(Runtime::Codex));
}

#[test]
fn a_finished_task_is_unread_until_a_person_opens_it() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Read me", &[]);
    h.done(&f, &t);
    tick_until(&mut h, &f, &t, TaskState::Done);
    let unread = |h: &Bench| {
        h.engine.summary().factories[0]
            .columns
            .iter()
            .flat_map(|column| column.cards.iter())
            .find(|card| card.task == t)
            .unwrap()
            .unread
    };
    assert!(unread(&h));
    h.as_worker(&f, &t, Command::Show { task: t.clone() });
    assert!(unread(&h), "a worker reading it is not the person");
    h.op(Command::Show { task: t.clone() });
    assert!(!unread(&h));
}

// ------------------------------------------------------------ control

#[test]
fn pause_resume_cancel_and_revive_keep_the_same_worker_and_pull_request() {
    let mut h = Bench::new(true);
    let f = github_factory(&mut h, MergeMode::Manual);
    let t = h.ready("Controlled", &[]);
    h.op(Command::Pause { task: t.clone() });
    assert_eq!(h.state(&f, &t), TaskState::Paused);
    assert!(h.world().sleeps.contains(&t));
    h.op(Command::Resume { task: t.clone() });
    h.engine.tick();
    assert_eq!(h.state(&f, &t), TaskState::Running);
    h.done(&f, &t);
    tick_until(&mut h, &f, &t, TaskState::MergeWaiting);
    h.op(Command::Cancel { task: t.clone() });
    assert_eq!(h.state(&f, &t), TaskState::Cancelled);
    assert_eq!(h.writes("pr.close"), vec!["pr.close 1"]);
    let revived = h.op(Command::Revive { task: t.clone() });
    assert_eq!(revived["ok"], true, "{revived}");
    assert_eq!(h.writes("pr.reopen"), vec!["pr.reopen 1"]);
    assert_eq!(h.world().spawned.len(), 1);

    // Past the keep period the worktree and local branch go and revive is
    // refused (D-58).
    h.op(Command::Cancel { task: t.clone() });
    h.advance(8 * DAY_MS);
    h.engine.tick();
    assert!(h.world().removed.contains(&t));
    assert!(h.world().branches_deleted.contains(&t));
    let refused = h.op(Command::Revive { task: t.clone() });
    assert_eq!(refused["reason"], "revive_expired");
}

#[test]
fn judgment_bodies_and_letters_are_kept_with_their_task() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Kept", &[]);
    h.engine.letter(Inbound {
        id: "letter-9".into(),
        factory: f.clone(),
        sender_pane: h.task(&f, &t).worker.unwrap().pane.unwrap(),
        kind: "request".into(),
        body: "Factory: which name? please".into(),
    });
    let kinds: Vec<(String, String)> = h
        .engine
        .records(&f, &t, 50)
        .into_iter()
        .map(|r| (r.kind, r.reference))
        .collect();
    assert!(
        kinds.iter().any(|(k, _)| k == "judgment.input"),
        "{kinds:?}"
    );
    assert!(
        kinds.iter().any(|(k, _)| k == "judgment.output"),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&("letter.in".into(), "letter-9".into())),
        "{kinds:?}"
    );
}

#[test]
fn a_cancelled_task_leaves_no_question_for_a_person_or_a_deadline() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Asks then goes", &[]);
    h.as_worker(
        &f,
        &t,
        Command::Ask {
            text: "Name the flag?".into(),
            suggestion: "--fast".into(),
            default_action: "use --fast".into(),
            deadline_hours: Some(1),
            letter: None,
        },
    );
    h.op(Command::Cancel { task: t.clone() });
    assert_eq!(h.task(&f, &t).open_questions().count(), 0);
    h.advance(2 * HOUR_MS);
    h.engine.tick();
    let question = &h.task(&f, &t).questions[0];
    assert_eq!(question.answer.as_ref().unwrap().relayed_by, "cancel");
    assert_eq!(h.state(&f, &t), TaskState::Cancelled);
}

#[test]
fn a_blocked_task_takes_only_an_answer() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Blocked", &[]);
    h.as_worker(
        &f,
        &t,
        Command::Block {
            text: "?".into(),
            suggestion: "x".into(),
            deadline_hours: Some(1),
            letter: None,
        },
    );
    for command in [
        Command::Pause { task: t.clone() },
        Command::Cancel { task: t.clone() },
        Command::Retry { task: t.clone() },
    ] {
        let refused = h.op(command);
        assert_eq!(
            refused["reason"], "action_not_allowed_in_state",
            "{refused}"
        );
        assert_eq!(refused["detail"]["allowed"], json!(["answer"]));
        assert_eq!(refused["detail"]["state"], "막힘");
    }
}

// ------------------------------------------------------------- restart

#[test]
fn a_restart_keeps_every_task_question_and_applied_letter() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let a = h.ready("Survives", &[]);
    let b = h.ready("Also", &[&a]);
    let pane = h.task(&f, &a).worker.unwrap().pane.unwrap();
    let letter = Inbound {
        id: "letter-9".into(),
        factory: f.clone(),
        sender_pane: pane,
        kind: "block".into(),
        body: "Q?\nSuggestion: yes".into(),
    };
    h.engine.letter(letter.clone());
    let before: Vec<Task> = h.engine.tasks_of(&f).cloned().collect();

    let mut h = h.restart();
    let after: Vec<Task> = h.engine.tasks_of(&f).cloned().collect();
    assert_eq!(before, after);
    assert_eq!(
        h.engine.letter(letter)["duplicate"],
        true,
        "applied once across the restart (B73)"
    );
    // Numbering continues: a new Task does not reuse an id.
    let c = h.add("Third", &[]);
    assert_eq!(c["task"]["id"], "T-3");
    let question = open_question(&h, &f, &a);
    assert_eq!(question.id, "Q1");
    h.op(Command::Answer {
        task: a.clone(),
        question: Some(question.id),
        choice: Some("suggestion".into()),
        text: None,
    });
    tick_until(&mut h, &f, &a, TaskState::Running);
    assert_eq!(h.state(&f, &b), TaskState::Waiting);
}

#[test]
fn a_restart_during_verification_runs_it_again() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let t = h.ready("Verifying", &[]);
    h.world()
        .verify
        .insert(t.clone(), [VerifyPoll::Pending].into());
    h.done(&f, &t);
    h.engine.tick();
    assert_eq!(h.state(&f, &t), TaskState::Verifying);
    let mut h = h.restart();
    assert_eq!(
        h.world().verify_runs.len(),
        2,
        "the child of the old process is gone"
    );
    tick_until(&mut h, &f, &t, TaskState::Done);
    assert_eq!(
        h.task(&f, &t)
            .attempts
            .iter()
            .filter(|a| a.stage == AttemptStage::Task)
            .count(),
        1
    );
}

// --------------------------------------------------------------- summary

#[test]
fn the_inbox_orders_blocking_questions_first_then_answers_merges_and_stops() {
    let mut h = Bench::new(false);
    let f = h.factory(false);
    let merge = h.ready("Merge me", &[]);
    let blocked = h.ready("Blocked", &[]);
    let asks = h.ready("Asks", &[]);
    h.done(&f, &merge);
    tick_until(&mut h, &f, &merge, TaskState::MergeWaiting);
    h.as_worker(
        &f,
        &asks,
        Command::Ask {
            text: "a?".into(),
            suggestion: "s".into(),
            default_action: "d".into(),
            deadline_hours: Some(5),
            letter: None,
        },
    );
    h.advance(MINUTE_MS);
    h.as_worker(
        &f,
        &blocked,
        Command::Block {
            text: "b?".into(),
            suggestion: "s".into(),
            deadline_hours: Some(5),
            letter: None,
        },
    );
    let summary = h.engine.summary();
    let groups: Vec<(&str, &str)> = summary
        .inbox
        .iter()
        .map(|item| (item.group.as_str(), item.task.as_str()))
        .collect();
    assert_eq!(
        groups,
        vec![
            ("answer", blocked.as_str()),
            ("answer", asks.as_str()),
            ("merge", merge.as_str())
        ]
    );
    assert_eq!(summary.my_turn, 3);
    assert_eq!(summary.inbox[1].remaining.as_deref(), Some("5시간 남음"));
    let flow = &summary.factories[0].flow;
    // Merge waiting sits in the running column (D-47).
    assert_eq!((flow.running, flow.waiting), (3, 0));
}

// ------------------------------------------------------------ slow worker start

#[test]
fn a_start_carried_out_off_the_engine_is_asked_for_again_on_the_next_tick() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.world().spawn_failure = Some(Failure::start_pending("worker.spawn"));
    let a = h.ready("A", &[]);
    h.engine.tick();
    assert_eq!(h.state(&f, &a), TaskState::Waiting);
    h.world().spawn_failure = None;
    h.engine.tick();
    assert_eq!(h.state(&f, &a), TaskState::Running, "no 30-second wait");
}

#[test]
fn a_start_cancelled_on_its_way_is_abandoned_and_a_revive_is_a_new_attempt() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.world().spawn_failure = Some(Failure::start_pending("worker.spawn"));
    let t = h.ready("On its way", &[]);
    assert_eq!(h.state(&f, &t), TaskState::Waiting);
    h.op(Command::Cancel { task: t.clone() });
    h.engine.tick();
    assert_eq!(h.world().abandoned, vec![t.clone()]);
    h.world().spawn_failure = None;
    h.op(Command::Revive { task: t.clone() });
    tick_until(&mut h, &f, &t, TaskState::Running);
    let attempts: Vec<u32> = h.world().spawn_asks.iter().map(|r| r.attempt).collect();
    assert_eq!(attempts.first(), Some(&0));
    assert_eq!(attempts.last(), Some(&1), "never the abandoned intent");
}

#[test]
fn a_resumed_worker_still_starting_holds_its_slot_without_a_failure_per_tick() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    set_workers(&mut h, &f, 1);
    let t = h.ready("Resumed", &[]);
    h.world()
        .worker_status
        .insert(t.clone(), WorkerStatus::Gone);
    h.advance(5 * MINUTE_MS);
    h.engine.tick();
    assert_eq!(h.state(&f, &t), TaskState::Stopped);
    h.world().spawn_failure = Some(Failure::start_pending("worker.spawn"));
    h.op(Command::Retry { task: t.clone() });
    let other = h.ready("Other", &[]);
    for _ in 0..5 {
        h.engine.tick();
    }
    assert_eq!(h.state(&f, &other), TaskState::Waiting, "the slot is held");
    let failures = h
        .engine
        .events(&f, Some(&t), 200)
        .iter()
        .filter(|e| e.kind == "external.failed")
        .count();
    assert_eq!(failures, 0, "a pending start is not a failure");
    assert!(
        h.world()
            .spawn_asks
            .iter()
            .any(|r| r.task == t && r.resume.is_some())
    );
}

#[test]
fn a_worker_whose_agent_has_not_started_holds_its_slot_and_is_asked_again_later() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    set_workers(&mut h, &f, 1);
    h.world().spawn_failure = Some(Failure::starting(
        "worker.spawn",
        "native_identity_unavailable",
    ));
    let a = h.ready("A", &[]);
    h.engine.tick();
    assert_eq!(h.state(&f, &a), TaskState::Waiting);
    let b = h.ready("B", &[]);
    for _ in 0..3 {
        h.advance(5_000);
        h.engine.tick();
    }
    let starting = |h: &Bench| {
        h.engine
            .events(&f, None, 100)
            .iter()
            .filter(|e| e.kind == "worker.starting")
            .count()
    };
    assert_eq!(starting(&h), 1, "recorded once, not once per tick");
    assert!(
        h.engine
            .events(&f, None, 100)
            .iter()
            .all(|e| e.kind != "external.failed"),
        "a slow start is not a failure"
    );
    assert_eq!(
        h.state(&f, &b),
        TaskState::Waiting,
        "A's slot is still held"
    );
    assert!(h.task(&f, &a).open_questions().next().is_none());

    // Ten quiet minutes: the person is told to look at the pane.
    h.advance(10 * 60_000);
    h.engine.tick();
    let notice = open_question(&h, &f, &a);
    assert!(
        notice.text.contains("시작되지 않았습니다"),
        "{}",
        notice.text
    );

    // The agent comes up: the same spawn now answers, and B still waits.
    h.world().spawn_failure = None;
    h.advance(30_000);
    h.engine.tick();
    assert_eq!(h.state(&f, &a), TaskState::Running);
    assert_eq!(h.state(&f, &b), TaskState::Waiting);
    assert_eq!(h.world().spawned.len(), 1);
}

#[test]
fn a_refused_worker_start_stops_the_task_once_and_a_retry_starts_it() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    h.world().spawn_failure = Some(Failure::task(
        "worker.spawn",
        "Codex support could not be confirmed",
    ));
    let a = h.ready("A", &[]);
    for _ in 0..5 {
        h.advance(2_000);
        h.engine.tick();
    }
    let task = h.task(&f, &a);
    assert_eq!(task.state, TaskState::Stopped);
    assert_eq!(task.stop, Some(StopReason::WorkerStart));
    let failed = h
        .engine
        .events(&f, Some(&a), 100)
        .iter()
        .filter(|e| e.kind == "external.failed")
        .count();
    assert_eq!(failed, 1, "a refusal is not asked again every tick");
    let inbox = h.engine.summary().inbox;
    let item = inbox.iter().find(|i| i.task == a).expect("stopped item");
    assert!(
        item.text.contains("Codex support could not be confirmed"),
        "{}",
        item.text
    );

    h.world().spawn_failure = None;
    let answer = h.op(Command::Retry { task: a.clone() });
    assert_eq!(answer["ok"], true, "{answer}");
    h.engine.tick();
    assert_eq!(h.state(&f, &a), TaskState::Running);
    assert_eq!(h.task(&f, &a).stop_detail, None);
    assert_eq!(
        h.world().spawned[0].attempt,
        1,
        "the retry is a new start, not the refused one again"
    );
}

// ------------------------------------------------------------------ watch

#[test]
fn a_periodic_check_reads_each_running_task_on_the_watch_cadence() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let answer = h.op(Command::Check {
        project: None,
        at: CheckPoint::Periodic,
        instruction: "Does it still match the card?".into(),
    });
    assert_eq!(answer["ok"], true, "{answer}");
    let running = h.ready("Running", &[]);
    let waiting = h.ready("Waiting", &[&running]);
    h.world().drift.insert(
        running.clone(),
        json!({"pass": false, "questions": [{"text": "Drifting?", "suggestion": "keep", "default_action": "keep going"}], "flags": []}),
    );
    let periodic = |h: &Bench| {
        h.world()
            .judged
            .iter()
            .filter(|j| j.id.contains(":periodic:"))
            .filter_map(|j| j.task.clone())
            .collect::<Vec<_>>()
    };
    h.engine.tick();
    assert!(periodic(&h).is_empty(), "nothing before the interval");
    h.advance(30 * 60_000);
    h.engine.tick();
    h.engine.tick();
    assert_eq!(periodic(&h), vec![running.clone()]);
    let question = open_question(&h, &f, &running);
    assert_eq!(question.text, "Drifting?");
    assert_eq!(
        h.state(&f, &running),
        TaskState::Running,
        "a check never stops the work"
    );
    assert_eq!(h.task(&f, &waiting).open_questions().count(), 0);
}

#[test]
fn the_watch_raises_actionable_warnings_once_each_within_the_daily_cap() {
    let mut h = Bench::new(false);
    let f = h.factory(true);
    let a = h.ready("A", &[]);
    let b = h.ready("B", &[]);
    let asked = h.as_worker(
        &f,
        &a,
        Command::Ask {
            text: "Which name?".into(),
            suggestion: "calc".into(),
            default_action: "use calc".into(),
            deadline_hours: Some(24),
            letter: None,
        },
    );
    assert_eq!(asked["ok"], true, "{asked}");
    let notices = |h: &Bench, id: &str| {
        h.task(&f, id)
            .open_questions()
            .filter(|q| q.text.starts_with("감시:"))
            .count()
    };
    h.world().watch.push_back(json!({"warnings": [
        {"text": "nothing to do here"},
        {"text": "A still waits on its answer", "action": "answer A", "task": a},
        {"text": "B has been quiet", "action": "look at B", "task": b},
    ]}));
    // The watch reads the board every 30 minutes (B69).
    h.advance(30 * 60_000);
    h.engine.tick();
    h.engine.tick();
    assert_eq!(
        notices(&h, &b),
        1,
        "an actionable warning reaches the inbox"
    );
    assert_eq!(notices(&h, &a), 0, "A is already in the inbox");
    let logged = h
        .engine
        .events(&f, None, 200)
        .iter()
        .filter(|e| e.kind == "watch.logged")
        .count();
    assert_eq!(logged, 2, "no action, or already open: the log only");

    // At most five a day.
    for n in 0..6 {
        h.world().watch.push_back(json!({"warnings": [
            {"text": format!("finding {n}"), "action": "check", "task": null},
        ]}));
        h.advance(30 * 60_000);
        h.engine.tick();
        h.engine.tick();
    }
    let sent: usize = h
        .engine
        .tasks_of(&f)
        .map(|t| {
            t.questions
                .iter()
                .filter(|q| q.text.starts_with("감시:"))
                .count()
        })
        .sum();
    assert_eq!(sent, 5, "the daily cap holds");
    assert!(
        h.engine
            .events(&f, None, 400)
            .iter()
            .any(|e| e.kind == "watch.capped")
    );
}
