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
                let all: Vec<Value> = hub.issues.iter().map(|(n, (title, body, state))| json!({"number": n, "title": title, "body": body, "state": state})).collect();
                ok(Value::Array(all).to_string())
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
                let all: Vec<Value> = hub.prs.values().filter(|pr| head.is_none_or(|h| pr["headRefName"] == h)).cloned().collect();
                ok(Value::Array(all).to_string())
            }
            ["pr", "create", ..] => {
                hub.next += 1;
                let number = hub.next;
                let head = flag(args, "--head").unwrap().to_owned();
                hub.prs.insert(number, json!({"number": number, "url": format!("https://github.com/o/r/pull/{number}"), "state": "OPEN", "headRefName": head, "headRefOid": "headsha", "mergeCommit": null, "closingIssuesReferences": []}));
                hub.writes.push(format!("pr create {number}"));
                ok(format!("https://github.com/o/r/pull/{number}\n"))
            }
            ["pr", "view", number, ..] => ok(hub.prs[&number.parse::<u64>().unwrap()].to_string()),
            ["pr", "merge", number, ..] => {
                let number: u64 = number.parse().unwrap();
                assert_eq!(flag(args, "--match-head-commit"), Some("headsha"), "the head SHA is pinned");
                let pr = hub.prs.get_mut(&number).unwrap();
                pr["state"] = json!("MERGED");
                pr["mergeCommit"] = json!({"oid": format!("merge{number}")});
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
        "outside_read_at": null, "outside_read_failures": 0, "watch_day": 0, "watch_sent_today": 0, "watch_last_at": null
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
    assert_eq!(gh.0.lock().unwrap().check_reads, 2, "each commit is asked once");
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
    }
    let held = task("T-1", Some(1));
    let closed = task("T-3", Some(3));
    let gone = task("T-4", Some(4));
    let events = p.observe(&factory, &[&held, &closed, &gone]).unwrap();
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
fn a_github_failure_carries_its_signal() {
    let gh = FakeGh::default();
    let mut p = projects(&gh);
    gh.0.lock().unwrap().fail_next =
        Some("HTTP 401: Bad credentials (https://api.github.com/)".into());
    let failure = p.prepare(&factory()).unwrap_err();
    assert_eq!(failure.signal, Some(EnvSignal::GithubAuth));
}
