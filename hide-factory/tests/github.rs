//! The GitHub path over a fake `gh` that keeps an in-memory repository: each
//! write the Factory makes is recorded, and a retried write converges on the
//! one already made (B73). No real GitHub is touched.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hide_factory::adapters::*;
use hide_factory::exec::{Output, Runner};
use hide_factory::model::*;
use hide_factory::project::{IssueBook, SharedProjects};
use serde_json::{Value, json};

#[derive(Default)]
struct Hub {
    issues: BTreeMap<u64, (String, String, String)>,
    prs: BTreeMap<u64, Value>,
    next: u64,
    writes: Vec<String>,
    checks: Vec<Value>,
    check_reads: usize,
    fail_next: Option<String>,
    /// GitHub answers the merge before it names the merge commit.
    commit_later: bool,
    /// The most issues one list answers, newest first.
    list_cap: Option<usize>,
    /// Each `git push` refspec, in order.
    pushes: Vec<String>,
    /// `issue view` answers this error for these numbers.
    view_errors: BTreeMap<u64, String>,
}

#[derive(Clone, Default)]
struct FakeGh(Arc<Mutex<Hub>>);

fn ok(stdout: impl Into<String>) -> Output {
    Output {
        code: Some(0),
        stdout: stdout.into(),
        stderr: String::new(),
    }
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

impl Runner for FakeGh {
    fn run(
        &mut self,
        program: &str,
        args: &[String],
        _cwd: Option<&Path>,
    ) -> Result<Output, Failure> {
        let mut hub = self.0.lock().unwrap();
        if program == "git" {
            return Ok(match args.first().map(String::as_str) {
                Some("rev-parse") => ok("headsha\n"),
                Some("push") => {
                    hub.pushes.push(args.last().cloned().unwrap_or_default());
                    ok("")
                }
                _ => ok(""),
            });
        }
        if let Some(stderr) = hub.fail_next.take() {
            return Ok(Output {
                code: Some(1),
                stdout: String::new(),
                stderr,
            });
        }
        let words: Vec<&str> = args.iter().map(String::as_str).collect();
        Ok(match words.as_slice() {
            ["label", "create", ..] => {
                hub.writes.push("label create".into());
                ok("")
            }
            ["issue", "list", ..] if flag(args, "--search").is_some() => {
                let needle = flag(args, "--search").unwrap().split('"').nth(1).unwrap_or_default().to_owned();
                let found: Vec<Value> = hub.issues.iter().filter(|(_, (_, body, _))| body.contains(&needle)).map(|(n, (_, body, _))| json!({"number": n, "body": body})).collect();
                ok(Value::Array(found).to_string())
            }
            ["issue", "list", ..] => {
                let all: Vec<Value> = hub.issues.iter().rev().take(hub.list_cap.unwrap_or(usize::MAX)).map(|(n, (title, body, state))| json!({"number": n, "title": title, "body": body, "state": state})).collect();
                ok(Value::Array(all).to_string())
            }
            ["issue", "view", number, ..] => {
                let number: u64 = number.parse().unwrap();
                if let Some(stderr) = hub.view_errors.get(&number) {
                    return Ok(Output {
                        code: Some(1),
                        stdout: String::new(),
                        stderr: stderr.clone(),
                    });
                }
                ok(match hub.issues.get(&number) {
                    Some((title, body, state)) => json!({"number": number, "title": title, "body": body, "state": state, "labels": [{"name": "factory"}]}),
                    None => json!({"number": number, "title": "", "body": "", "state": "OPEN", "labels": []}),
                }
                .to_string())
            }
            ["issue", "create", ..] => {
                hub.next += 1;
                let number = hub.next;
                hub.issues.insert(number, (flag(args, "--title").unwrap().into(), flag(args, "--body").unwrap().into(), "OPEN".into()));
                hub.writes.push(format!("issue create {number}"));
                ok(format!("https://github.com/o/r/issues/{number}\n"))
            }
            ["issue", "edit", number, ..] => {
                hub.writes.push(format!("issue label {number}"));
                ok("")
            }
            ["pr", "list", ..] => {
                let head = flag(args, "--head");
                let open = flag(args, "--state") == Some("open");
                let all: Vec<Value> = hub.prs.values().filter(|pr| head.is_none_or(|h| pr["headRefName"] == h)).filter(|pr| !open || pr["state"] == "OPEN").cloned().collect();
                ok(Value::Array(all).to_string())
            }
            ["pr", "create", ..] => {
                hub.next += 1;
                let number = hub.next;
                let head = flag(args, "--head").unwrap().to_owned();
                hub.prs.insert(number, json!({"number": number, "url": format!("https://github.com/o/r/pull/{number}"), "state": "OPEN", "headRefName": head, "isCrossRepository": false, "headRefOid": "headsha", "mergeCommit": null, "closingIssuesReferences": []}));
                hub.writes.push(format!("pr create {number}"));
                ok(format!("https://github.com/o/r/pull/{number}\n"))
            }
            ["pr", "view", number, ..] => ok(hub.prs[&number.parse::<u64>().unwrap()].to_string()),
            ["pr", "merge", number, ..] => {
                let number: u64 = number.parse().unwrap();
                assert_eq!(flag(args, "--match-head-commit"), Some("headsha"), "the head SHA is pinned");
                let later = hub.commit_later;
                let pr = hub.prs.get_mut(&number).unwrap();
                pr["state"] = json!("MERGED");
                pr["mergeCommit"] = if later { Value::Null } else { json!({"oid": format!("merge{number}")}) };
                hub.writes.push(format!("pr merge {number}"));
                ok("")
            }
            ["pr", "close", number, ..] => {
                let number: u64 = number.parse().unwrap();
                hub.prs.get_mut(&number).unwrap()["state"] = json!("CLOSED");
                hub.writes.push(format!("pr close {number}"));
                ok("")
            }
            ["pr", "reopen", number, ..] => {
                let number: u64 = number.parse().unwrap();
                hub.prs.get_mut(&number).unwrap()["state"] = json!("OPEN");
                hub.writes.push(format!("pr reopen {number}"));
                ok("")
            }
            ["api", path] if path.contains("/check-runs") => {
                hub.check_reads += 1;
                ok(json!({"check_runs": hub.checks}).to_string())
            }
            ["api", path] if path.contains("/actions/runs") => ok(json!({"workflow_runs": [{"id": 7, "conclusion": "cancelled"}, {"id": 8, "conclusion": "success"}]}).to_string()),
            ["run", "rerun", id, ..] => {
                hub.writes.push(format!("run rerun {id}"));
                ok("")
            }
            other => panic!("gh call outside the allow list: {other:?}"),
        })
    }
}

struct NoIssues;
impl IssueBook for NoIssues {
    fn create(&mut self, _: &str, _: &str, _: &str, _: &str) -> Result<u32, Failure> {
        unreachable!("a GitHub Factory never writes local issues")
    }
    fn read(&mut self, _: &str, _: u32) -> Result<IssueText, Failure> {
        unreachable!()
    }
}

fn factory() -> Factory {
    serde_json::from_value(json!({
        "id": "f-1", "project": "/work/r", "project_name": "r", "source": "github", "repo": "o/r",
        "default_branch": "main", "config": Config { verification: Verification::Ci { checks: vec!["test".into()] }, ..Config::default() },
        "closed": false, "created_at": 0, "next_task": 2, "next_local_issue": 1, "main": MainHealth::default(),
        "outside_read_at": null, "outside_read_failures": 0, "watch_day": 0, "watch_sent_today": 0, "watch_last_at": null,
        "github_approval": {"account": "octo", "repo": "o/r", "at": 0}
    }))
    .unwrap()
}

fn task(id: &str, issue: Option<u64>) -> Task {
    let mut value = json!({
        "factory": "f-1", "id": id, "seq": 1, "issue": issue.map(|n| json!({"kind": "github", "number": n})),
        "card": {"title": "Add it", "goal": "g", "criteria": ["c"], "out_of_scope": [], "open_decisions": [], "depends_on": [], "external": []},
        "human": HumanFields::default(), "state": "running", "state_since": 0, "created_at": 0, "updated_at": 0,
    });
    let defaults = serde_json::to_value(Task::draft("f-1", id, 1, Card::default(), 0)).unwrap();
    for (key, default) in defaults.as_object().unwrap() {
        value
            .as_object_mut()
            .unwrap()
            .entry(key.clone())
            .or_insert(default.clone());
    }
    let mut task: Task = serde_json::from_value(value).unwrap();
    // The Task-stage verification passed on the pushed head.
    task.attempts.push(Attempt {
        number: 1,
        commit: Some("headsha".into()),
        started_at: 0,
        stage: AttemptStage::Task,
        outcome: Some(AttemptOutcome::Passed),
        log: None,
    });
    task.worker = Some(WorkerRef {
        factory: String::new(),
        agent: None,
        name: "w".into(),
        pane: None,
        runtime: Runtime::Claude,
        worktree: "/work/r.worktrees/t".into(),
        branch: task.branch_slug(),
        started_at: 0,
        asleep: false,
    });
    task
}

fn projects(gh: &FakeGh) -> SharedProjects {
    SharedProjects::new(
        Box::new(gh.clone()),
        Box::new(NoIssues),
        PathBuf::from("/nonexistent/logs"),
    )
    .with_ci_poll_every(Duration::ZERO)
}

#[test]
fn each_github_write_happens_once_when_retried() {
    let gh = FakeGh::default();
    let mut p = projects(&gh);
    let factory = factory();
    p.prepare(&factory).unwrap();
    let mut t = task("T-1", None);
    let first = p
        .create_issue(
            &factory,
            &t,
            &format!("{}\nbody", hide_factory::engine::task_marker(&factory, &t)),
        )
        .unwrap();
    // A retry after a lost answer finds the issue by its marker.
    let again = p
        .create_issue(
            &factory,
            &t,
            &format!("{}\nbody", hide_factory::engine::task_marker(&factory, &t)),
        )
        .unwrap();
    assert_eq!(first, again);
    t.issue = Some(first);

    let pr = p.open_pr(&factory, &t, "body").unwrap().unwrap();
    assert!(pr.by_factory);
    let retried = p.open_pr(&factory, &t, "body").unwrap().unwrap();
    assert_eq!(retried.number, pr.number);
    t.pr = Some(pr.clone());

    let sha = p.merge(&factory, &t, MergeMethod::Squash).unwrap();
    assert_eq!(
        p.merge(&factory, &t, MergeMethod::Squash).unwrap(),
        sha,
        "a merged pull request is not merged again"
    );
    p.close_pr(&factory, &pr).unwrap();
    let writes = gh.0.lock().unwrap().writes.clone();
    assert_eq!(
        writes,
        vec![
            "label create",
            "issue create 1",
            "pr create 2",
            "pr merge 2"
        ]
    );
}

#[test]
fn a_merge_without_its_commit_yet_is_asked_again_and_not_merged_twice() {
    let gh = FakeGh::default();
    gh.0.lock().unwrap().commit_later = true;
    let mut p = projects(&gh);
    let factory = factory();
    let mut t = task("T-1", Some(1));
    t.pr = p.open_pr(&factory, &t, "body").unwrap();
    let number = t.pr.as_ref().unwrap().number;
    let failure = p.merge(&factory, &t, MergeMethod::Merge).unwrap_err();
    assert_eq!(failure.stage, "github.merge");
    assert!(
        p.merge(&factory, &t, MergeMethod::Merge).is_err(),
        "still unnamed"
    );
    gh.0.lock().unwrap().prs.get_mut(&number).unwrap()["mergeCommit"] = json!({"oid": "m1"});
    assert_eq!(p.merge(&factory, &t, MergeMethod::Merge).unwrap(), "m1");
    let merges =
        gh.0.lock()
            .unwrap()
            .writes
            .iter()
            .filter(|w| w.starts_with("pr merge"))
            .count();
    assert_eq!(merges, 1, "the merge is made once");
}

#[test]
fn every_report_pushes_and_a_merged_pull_request_is_not_reused() {
    let gh = FakeGh::default();
    let mut p = projects(&gh);
    let factory = factory();
    let mut t = task("T-1", Some(1));
    let first = p.open_pr(&factory, &t, "body").unwrap().unwrap();
    // A second report after a failed check pushes the fix to the same pull
    // request.
    let second = p.open_pr(&factory, &t, "body").unwrap().unwrap();
    assert_eq!(second.number, first.number);
    assert_eq!(
        gh.0.lock().unwrap().pushes,
        vec!["HEAD:refs/heads/factory/1-add-it"; 2]
    );
    // Merged, reverted and relanding: a new pull request, never the old one.
    t.pr = Some(first.clone());
    p.merge(&factory, &t, MergeMethod::Merge).unwrap();
    let reland = p.open_pr(&factory, &t, "body").unwrap().unwrap();
    assert_ne!(reland.number, first.number);
    assert!(reland.by_factory);
}

#[test]
fn a_factory_without_a_recorded_approval_writes_nothing_to_github() {
    let gh = FakeGh::default();
    let mut p = projects(&gh);
    let factory = Factory {
        github_approval: None,
        ..factory()
    };
    let mut t = task("T-1", Some(1));
    t.pr = Some(PullRequest {
        number: 2,
        url: String::new(),
        head: "factory/1-add-it".into(),
        by_factory: true,
        open: true,
    });
    let pr = t.pr.clone().unwrap();
    let refusals = [
        p.prepare(&factory).err(),
        p.create_issue(&factory, &task("T-2", None), "body").err(),
        p.label_issue(&factory, &IssueRef::Github { number: 1 })
            .err(),
        p.open_pr(&factory, &task("T-3", Some(3)), "body").err(),
        p.close_pr(&factory, &pr).err(),
        p.reopen_pr(&factory, &pr).err(),
        p.merge(&factory, &t, MergeMethod::Squash).err(),
        p.rerun_main(&factory, "headsha").err(),
    ];
    for refusal in refusals {
        assert_eq!(refusal.expect("refused").stage, "github.approval");
    }
    let hub = gh.0.lock().unwrap();
    assert!(hub.writes.is_empty(), "{:?}", hub.writes);
    assert!(hub.pushes.is_empty(), "{:?}", hub.pushes);
}

#[test]
fn a_merge_without_a_verified_commit_is_refused() {
    let gh = FakeGh::default();
    let mut p = projects(&gh);
    let factory = factory();
    let mut t = task("T-1", Some(1));
    t.pr = p.open_pr(&factory, &t, "body").unwrap();
    t.attempts.last_mut().unwrap().outcome = Some(AttemptOutcome::Failed {
        check: "test".into(),
        link: String::new(),
    });
    let failure = p.merge(&factory, &t, MergeMethod::Merge).unwrap_err();
    assert_eq!(failure.detail, "no verified commit to merge");
    assert!(
        !gh.0
            .lock()
            .unwrap()
            .writes
            .iter()
            .any(|w| w.starts_with("pr merge"))
    );
}

#[test]
fn a_github_revert_is_one_pull_request_merged_once_at_its_commit() {
    let gh = FakeGh::default();
    let mut p = projects(&gh);
    let factory = factory();
    let t = task("T-1", Some(1));
    let revert = p.revert(&factory, &t, "bad1").unwrap();
    let number = revert.pr.expect("a revert pull request");
    assert_eq!(revert.commit.as_deref(), Some("headsha"));
    // Asked again after a lost answer: the open pull request is found.
    let again = p.revert(&factory, &t, "bad1").unwrap();
    assert_eq!(again.pr, Some(number));
    let landed = p.merge_revert(&factory, &revert).unwrap();
    assert_eq!(landed, format!("merge{number}"));
    assert_eq!(p.merge_revert(&factory, &revert).unwrap(), landed);
    let hub = gh.0.lock().unwrap();
    assert_eq!(
        hub.writes,
        vec![format!("pr create {number}"), format!("pr merge {number}")]
    );
    assert!(
        hub.pushes
            .iter()
            .all(|refspec| refspec == "HEAD:refs/heads/factory/revert-t-1")
    );
}

#[test]
fn required_checks_decide_and_a_cancelled_run_is_asked_again() {
    let gh = FakeGh::default();
    let mut p = projects(&gh);
    let factory = factory();
    let mut t = task("T-1", Some(1));
    t.pr = Some(PullRequest {
        number: 3,
        url: "u".into(),
        head: "h".into(),
        by_factory: true,
        open: true,
    });
    let run = p.start(&factory, &t).unwrap();
    assert_eq!(p.poll(&factory, &run), VerifyPoll::Pending, "no run yet");
    gh.0.lock().unwrap().checks = vec![
        json!({"name": "test", "status": "completed", "conclusion": "failure", "html_url": "https://ci/1"}),
        json!({"name": "other", "status": "in_progress"}),
    ];
    assert_eq!(
        p.poll(&factory, &run),
        VerifyPoll::Failed {
            check: "test".into(),
            link: "test: https://ci/1".into()
        }
    );
    gh.0.lock().unwrap().checks =
        vec![json!({"name": "test", "status": "completed", "conclusion": "cancelled"})];
    assert_eq!(p.main_check(&factory, "abc").unwrap(), MainCheck::Pending);
    p.rerun_main(&factory, "abc").unwrap();
    assert_eq!(gh.0.lock().unwrap().writes, vec!["run rerun 7"]);
    gh.0.lock().unwrap().checks =
        vec![json!({"name": "test", "status": "completed", "conclusion": "success"})];
    assert_eq!(p.main_check(&factory, "abc").unwrap(), MainCheck::Green);
}

#[test]
fn running_checks_are_not_asked_again_on_every_tick() {
    let gh = FakeGh::default();
    let mut p = projects(&gh).with_ci_poll_every(Duration::from_secs(3600));
    let factory = factory();
    gh.0.lock().unwrap().checks = vec![json!({"name": "test", "status": "in_progress"})];
    assert_eq!(p.main_check(&factory, "abc").unwrap(), MainCheck::Pending);
    assert_eq!(p.main_check(&factory, "abc").unwrap(), MainCheck::Pending);
    assert_eq!(gh.0.lock().unwrap().check_reads, 1);
    assert_eq!(p.main_check(&factory, "def").unwrap(), MainCheck::Pending);
    assert_eq!(
        gh.0.lock().unwrap().check_reads,
        2,
        "each commit is asked once"
    );
}

#[test]
fn outside_work_is_read_from_issues_and_closing_pull_requests() {
    let gh = FakeGh::default();
    let mut p = projects(&gh);
    let factory = factory();
    {
        let mut hub = gh.0.lock().unwrap();
        hub.issues.insert(
            1,
            (
                "Held".into(),
                "<!-- hide-factory: f-1/T-1 -->".into(),
                "OPEN".into(),
            ),
        );
        hub.issues
            .insert(2, ("New one".into(), "please do".into(), "OPEN".into()));
        hub.issues.insert(
            3,
            (
                "Closed".into(),
                "<!-- hide-factory: f-1/T-3 -->".into(),
                "CLOSED".into(),
            ),
        );
        hub.prs.insert(9, json!({"number": 9, "url": "pr/9", "state": "OPEN", "headRefName": "someone/fix", "closingIssuesReferences": [{"number": 1}]}));
        // An open pull request from a fork closes issue 3's held Task's issue
        // too, but anyone can open one: it takes nothing until merged.
        hub.prs.insert(10, json!({"number": 10, "url": "pr/10", "state": "OPEN", "headRefName": "fix", "isCrossRepository": true, "closingIssuesReferences": [{"number": 5}]}));
        hub.issues.insert(
            5,
            (
                "Forked".into(),
                "<!-- hide-factory: f-1/T-5 -->".into(),
                "OPEN".into(),
            ),
        );
    }
    let held = task("T-1", Some(1));
    let closed = task("T-3", Some(3));
    let gone = task("T-4", Some(4));
    let forked = task("T-5", Some(5));
    let events = p
        .observe(&factory, &[&held, &closed, &gone, &forked])
        .unwrap();
    assert!(
        !events.iter().any(|event| matches!(
            event,
            OutsideEvent::ClosingPr {
                issue: IssueRef::Github { number: 5 },
                ..
            }
        )),
        "{events:?}"
    );
    assert!(
        events.contains(&OutsideEvent::ClosingPr {
            issue: IssueRef::Github { number: 1 },
            pr: 9,
            url: "pr/9".into(),
            merged: false
        }),
        "{events:?}"
    );
    assert!(events.contains(&OutsideEvent::Labeled {
        issue: IssueRef::Github { number: 2 },
        title: "New one".into(),
        body: "please do".into()
    }));
    assert!(events.contains(&OutsideEvent::IssueClosed {
        issue: IssueRef::Github { number: 3 }
    }));
    assert!(events.contains(&OutsideEvent::LabelRemoved {
        issue: IssueRef::Github { number: 4 }
    }));
    assert!(
        gh.0.lock().unwrap().writes.is_empty(),
        "reading writes nothing"
    );
}

#[test]
fn a_held_issue_past_the_list_limit_keeps_its_task() {
    let gh = FakeGh::default();
    let mut p = projects(&gh);
    let factory = factory();
    {
        let mut hub = gh.0.lock().unwrap();
        hub.issues.insert(
            1,
            (
                "Old".into(),
                "<!-- hide-factory: f-1/T-1 -->".into(),
                "OPEN".into(),
            ),
        );
        hub.issues
            .insert(2, ("Newer".into(), "later".into(), "OPEN".into()));
        hub.list_cap = Some(1);
    }
    let held = task("T-1", Some(1));
    let events = p.observe(&factory, &[&held]).unwrap();
    assert!(
        !events.contains(&OutsideEvent::LabelRemoved {
            issue: IssueRef::Github { number: 1 }
        }),
        "{events:?}"
    );
}

#[test]
fn a_held_issue_that_cannot_be_read_keeps_its_task_and_a_deleted_one_is_gone() {
    let gh = FakeGh::default();
    let mut p = projects(&gh);
    let factory = factory();
    {
        let mut hub = gh.0.lock().unwrap();
        hub.issues
            .insert(9, ("Listed".into(), "body".into(), "CLOSED".into()));
        hub.list_cap = Some(1);
        hub.view_errors.insert(
            1,
            "issue view refused: transferred to another repository".into(),
        );
        hub.view_errors.insert(
            2,
            "GraphQL: Could not resolve to an issue or pull request with the number of 2.".into(),
        );
    }
    let odd = task("T-1", Some(1));
    let deleted = task("T-2", Some(2));
    let listed = task("T-9", Some(9));
    let events = p.observe(&factory, &[&odd, &deleted, &listed]).unwrap();
    assert!(
        !events.contains(&OutsideEvent::LabelRemoved {
            issue: IssueRef::Github { number: 1 }
        }),
        "{events:?}"
    );
    assert!(events.contains(&OutsideEvent::LabelRemoved {
        issue: IssueRef::Github { number: 2 }
    }));
    // The other Tasks' reads go on.
    assert!(events.contains(&OutsideEvent::IssueClosed {
        issue: IssueRef::Github { number: 9 }
    }));
}

#[test]
fn another_task_s_pull_request_naming_an_issue_takes_no_task() {
    let gh = FakeGh::default();
    let mut p = projects(&gh);
    let factory = factory();
    {
        let mut hub = gh.0.lock().unwrap();
        for n in [1, 2] {
            hub.issues.insert(
                n,
                (
                    format!("Issue {n}"),
                    format!("<!-- hide-factory: f-1/T-{n} -->"),
                    "OPEN".into(),
                ),
            );
        }
        hub.prs.insert(
            5,
            json!({"number": 5, "url": "https://github.com/o/r/pull/5", "state": "OPEN",
                "headRefName": "factory/1-add-it", "isCrossRepository": false,
                "closingIssuesReferences": [{"number": 1}, {"number": 2}]}),
        );
    }
    let mut a = task("T-1", Some(1));
    a.pr = Some(PullRequest {
        number: 5,
        url: "https://github.com/o/r/pull/5".into(),
        head: "factory/1-add-it".into(),
        by_factory: true,
        open: true,
    });
    let b = task("T-2", Some(2));
    let events = p.observe(&factory, &[&a, &b]).unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, OutsideEvent::ClosingPr { .. })),
        "{events:?}"
    );
}

#[test]
fn a_worker_s_closing_keyword_in_a_decision_stays_text() {
    let mut t = task("T-1", Some(1));
    t.decisions.push(DecisionRecord {
        text: "done: Fixes #2 and ``` too".into(),
        by: "worker:T-1".into(),
        at: 0,
    });
    let body = hide_factory::engine::pr_body(&t, &factory());
    let fence = body
        .find("````text")
        .expect("a fence longer than the text's");
    let fixes = body.find("Fixes #2").unwrap();
    let close = body[fixes..].find("````").map(|i| i + fixes).unwrap();
    assert!(fence < fixes && fixes < close, "{body}");
    assert!(body.trim_end().ends_with("Closes #1"));
}

#[test]
fn a_fork_pull_request_on_the_task_or_revert_branch_name_is_never_adopted() {
    let gh = FakeGh::default();
    let mut p = projects(&gh);
    let factory = factory();
    let t = task("T-1", Some(1));
    {
        let mut hub = gh.0.lock().unwrap();
        for (number, head) in [(40, t.branch_slug()), (41, "factory/revert-t-1".into())] {
            hub.prs.insert(
                number,
                json!({"number": number, "url": format!("https://github.com/x/r/pull/{number}"),
                    "state": "OPEN", "headRefName": head, "isCrossRepository": true,
                    "headRefOid": "strangersha", "closingIssuesReferences": []}),
            );
        }
    }
    let pr = p.open_pr(&factory, &t, "body").unwrap().unwrap();
    assert!(pr.by_factory, "the Factory opened its own");
    assert_ne!(pr.number, 40);
    let revert = p.revert(&factory, &t, "bad1").unwrap();
    assert_ne!(revert.pr, Some(41));
}

#[test]
fn a_github_failure_carries_its_signal() {
    let gh = FakeGh::default();
    let mut p = projects(&gh);
    gh.0.lock().unwrap().fail_next =
        Some("HTTP 401: Bad credentials (https://api.github.com/)".into());
    let failure = p.prepare(&factory()).unwrap_err();
    assert_eq!(failure.signal, Some(EnvSignal::GithubAuth));
}
