//! The local path against a real git repository in a private folder: the
//! pre-merge check, the verify bundle, the merge into the main checkout, the
//! main verification and the revert of one merge (B38, B42, B44, B45).
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use hide_factory::adapters::*;
use hide_factory::exec::SystemRunner;
use hide_factory::model::*;
use hide_factory::project::{IssueBook, SharedProjects};
use serde_json::json;

#[derive(Default)]
struct Book(BTreeMap<u32, (String, String)>);

impl IssueBook for Book {
    fn create(
        &mut self,
        _project: &str,
        title: &str,
        body: &str,
        marker: &str,
    ) -> Result<u32, Failure> {
        if let Some((n, _)) = self.0.iter().find(|(_, (_, b))| b.contains(marker)) {
            return Ok(*n);
        }
        let n = self.0.len() as u32 + 1;
        self.0.insert(n, (title.into(), body.into()));
        Ok(n)
    }
    fn read(&mut self, _project: &str, number: u32) -> Result<IssueText, Failure> {
        let (title, body) = self
            .0
            .get(&number)
            .cloned()
            .ok_or_else(|| Failure::task("issue", "missing"))?;
        Ok(IssueText {
            title,
            body,
            open: true,
        })
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

struct Fixture {
    _root: tempfile::TempDir,
    project: PathBuf,
    projects: SharedProjects,
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("repo");
    std::fs::create_dir(&project).unwrap();
    git(&project, &["init", "--quiet", "--initial-branch=main"]);
    git(&project, &["config", "user.email", "factory@example.com"]);
    git(&project, &["config", "user.name", "Factory"]);
    // The machine's own signing setting must not reach the fixture.
    git(&project, &["config", "commit.gpgsign", "false"]);
    write(&project, "a.txt", "one\n");
    write(&project, "Makefile", "test:\n\ttrue\n");
    git(&project, &["add", "."]);
    git(&project, &["commit", "--quiet", "-m", "base"]);
    let runner = SystemRunner {
        stop: Arc::new(AtomicBool::new(false)),
    };
    let projects = SharedProjects::new(
        Box::new(runner),
        Box::new(Book::default()),
        root.path().join("logs"),
    );
    Fixture {
        _root: root,
        project,
        projects,
    }
}

fn factory(project: &Path, commands: &[&str]) -> Factory {
    serde_json::from_value(json!({
        "id": "f-local", "project": project.display().to_string(), "project_name": "repo", "source": "local",
        "default_branch": "main",
        "config": Config { verification: Verification::Commands { commands: commands.iter().map(|c| (*c).to_owned()).collect() }, ..Config::default() },
        "closed": false, "created_at": 0, "next_task": 1, "next_local_issue": 1, "main": MainHealth::default(),
        "outside_read_at": null, "outside_read_failures": 0, "watch_day": 0, "watch_sent_today": 0, "watch_last_at": null
    }))
    .unwrap()
}

/// A Task with a worktree on its own branch carrying one commit.
fn task(fixture: &Fixture, id: &str, file: &str, text: &str) -> Task {
    let mut task = Task::draft(
        "f-local",
        id,
        1,
        Card {
            title: format!("Task {id}"),
            ..Card::default()
        },
        0,
    );
    let branch = task.branch_slug();
    let worktree = fixture.project.with_file_name(format!("wt-{id}"));
    git(
        &fixture.project,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            &branch,
            worktree.to_str().unwrap(),
        ],
    );
    write(&worktree, file, text);
    git(&worktree, &["add", "."]);
    git(&worktree, &["commit", "--quiet", "-m", id]);
    task.worker = Some(WorkerRef {
        factory: String::new(),
        agent: None,
        name: id.into(),
        pane: None,
        runtime: Runtime::Claude,
        worktree: worktree.display().to_string(),
        branch,
        started_at: 0,
        asleep: true,
    });
    task
}

fn settle(projects: &mut SharedProjects, factory: &Factory, run: &VerifyRun) -> VerifyPoll {
    let guard = Instant::now() + Duration::from_secs(120);
    loop {
        let poll = projects.poll(factory, run);
        if poll != VerifyPoll::Pending || Instant::now() > guard {
            return poll;
        }
        std::thread::yield_now();
    }
}

fn settle_main(projects: &mut SharedProjects, factory: &Factory, sha: &str) -> MainCheck {
    let guard = Instant::now() + Duration::from_secs(120);
    loop {
        let check = projects.main_check(factory, sha).unwrap();
        if check != MainCheck::Pending || Instant::now() > guard {
            return check;
        }
        std::thread::yield_now();
    }
}

#[test]
fn a_local_task_is_verified_merged_checked_on_main_and_reverted_alone() {
    let mut fx = fixture();
    let probe = fx.projects.probe(fx.project.to_str().unwrap()).unwrap();
    assert!(!probe.github);
    assert_eq!(probe.default_branch, "main");
    assert_eq!(probe.verify_candidates, vec!["make test"]);

    let factory = factory(&fx.project, &["test -f b.txt"]);
    let mut t = task(&fx, "T-1", "b.txt", "two\n");
    assert_eq!(fx.projects.diff_lines(&factory, &t).unwrap(), 1);
    assert_eq!(
        fx.projects.changed_paths(&factory, &t).unwrap(),
        vec!["b.txt"]
    );
    assert_eq!(fx.projects.premerge(&factory, &t).unwrap(), PreMerge::Clean);

    let run = fx.projects.start(&factory, &t).unwrap();
    assert_eq!(settle(&mut fx.projects, &factory, &run), VerifyPoll::Passed);
    t.attempts.push(Attempt {
        number: 1,
        commit: run.commit.clone(),
        started_at: 0,
        stage: AttemptStage::Task,
        outcome: Some(AttemptOutcome::Passed),
        log: None,
    });
    // main moved meanwhile: the bundle runs on main merged into the Task.
    write(&fx.project, "c.txt", "three\n");
    git(&fx.project, &["add", "."]);
    git(&fx.project, &["commit", "--quiet", "-m", "outside"]);
    let factory = Factory {
        config: Config {
            verification: Verification::Commands {
                commands: vec!["test -f b.txt && test -f c.txt".into()],
            },
            ..factory.config.clone()
        },
        ..factory
    };
    let run = fx.projects.start_premerge(&factory, &t).unwrap();
    assert_eq!(settle(&mut fx.projects, &factory, &run), VerifyPoll::Passed);

    assert!(!fx.projects.main_dirty(&factory).unwrap());
    let sha = fx.projects.merge(&factory, &t, MergeMethod::Merge).unwrap();
    assert_eq!(git(&fx.project, &["rev-parse", "HEAD"]), sha);
    assert!(fx.project.join("b.txt").exists());
    assert_eq!(
        fx.projects.merge(&factory, &t, MergeMethod::Merge).unwrap(),
        sha,
        "a merged branch is not merged again"
    );
    assert_eq!(
        settle_main(&mut fx.projects, &factory, &sha),
        MainCheck::Green
    );

    let revert = fx.projects.revert(&factory, &t, &sha).unwrap();
    assert_eq!(revert.pr, None);
    let check = fx.projects.revert_check(&factory, &revert).unwrap();
    assert_eq!(
        check,
        MainCheck::Pending,
        "the revert is verified before it lands"
    );
    let guard = Instant::now() + Duration::from_secs(120);
    let mut check = check;
    while check == MainCheck::Pending && Instant::now() < guard {
        check = fx.projects.revert_check(&factory, &revert).unwrap();
        std::thread::yield_now();
    }
    // The bundle needs b.txt, which the revert removes: it fails.
    assert!(matches!(check, MainCheck::Red { .. }), "{check:?}");
    // The operator is on another branch: the revert does not land there.
    git(&fx.project, &["checkout", "--quiet", "-b", "elsewhere"]);
    let refused = fx.projects.merge_revert(&factory, &revert).unwrap_err();
    assert_eq!(refused.detail, "main_dirty");
    assert!(fx.project.join("b.txt").exists());
    git(&fx.project, &["checkout", "--quiet", "main"]);
    let landed = fx.projects.merge_revert(&factory, &revert).unwrap();
    assert_eq!(git(&fx.project, &["rev-parse", "HEAD"]), landed);
    assert!(!fx.project.join("b.txt").exists());
    assert!(
        fx.project.join("c.txt").exists(),
        "only that merge is undone"
    );
}

#[test]
fn a_conflict_and_a_dirty_main_are_found_before_merge() {
    let mut fx = fixture();
    let factory = factory(&fx.project, &[]);
    let mut t = task(&fx, "T-2", "a.txt", "branch\n");
    write(&fx.project, "a.txt", "main\n");
    git(&fx.project, &["commit", "--quiet", "-am", "main edit"]);
    assert_eq!(
        fx.projects.premerge(&factory, &t).unwrap(),
        PreMerge::Conflict {
            files: vec!["a.txt".into()]
        }
    );
    // Merged anyway, the conflict fails the merge and leaves no MERGE_HEAD.
    let worktree = PathBuf::from(&t.worker.as_ref().unwrap().worktree);
    t.attempts.push(Attempt {
        number: 1,
        commit: Some(git(&worktree, &["rev-parse", "HEAD"])),
        started_at: 0,
        stage: AttemptStage::Task,
        outcome: Some(AttemptOutcome::Passed),
        log: None,
    });
    assert!(fx.projects.merge(&factory, &t, MergeMethod::Merge).is_err());
    assert!(!fx.project.join(".git/MERGE_HEAD").exists());
    assert!(!fx.projects.main_dirty(&factory).unwrap());

    write(&fx.project, "a.txt", "uncommitted\n");
    assert!(fx.projects.main_dirty(&factory).unwrap());
}

#[test]
fn a_risk_path_and_a_quick_check_gate_the_merge() {
    let mut fx = fixture();
    let mut factory = factory(&fx.project, &[]);
    factory.config.risk_paths = vec!["migrations/**".into()];
    std::fs::create_dir(fx.project.with_file_name("unused")).unwrap();
    let mut t = task(&fx, "T-3", "m.sql", "x\n");
    let worktree = PathBuf::from(&t.worker.as_ref().unwrap().worktree);
    std::fs::create_dir(worktree.join("migrations")).unwrap();
    write(&worktree, "migrations/001.sql", "create\n");
    git(&worktree, &["add", "."]);
    git(&worktree, &["commit", "--quiet", "-m", "migration"]);
    assert_eq!(
        fx.projects.premerge(&factory, &t).unwrap(),
        PreMerge::RiskPath {
            paths: vec!["migrations/001.sql".into()]
        }
    );
    factory.config.quick_check = Some("test -f missing".into());
    assert_eq!(
        fx.projects.premerge(&factory, &t).unwrap(),
        PreMerge::QuickCheckFailed {
            check: "test -f missing".into()
        }
    );
    t.worker = None;
    assert!(
        fx.projects.premerge(&factory, &t).is_err(),
        "no worktree is a failure, not a pass"
    );
}

/// Real git against a bare remote; `gh` answers that no pull request is open
/// and opens one when asked.
struct RealGitFakeGh(SystemRunner);

impl hide_factory::exec::Runner for RealGitFakeGh {
    fn run(
        &mut self,
        program: &str,
        args: &[String],
        cwd: Option<&Path>,
    ) -> Result<hide_factory::exec::Output, Failure> {
        if program == "git" {
            return self.0.run(program, args, cwd);
        }
        let stdout = match args.get(1).map(String::as_str) {
            Some("list") => "[]".to_owned(),
            Some("create") => "https://github.com/o/r/pull/7\n".to_owned(),
            other => panic!("unexpected gh {other:?}"),
        };
        Ok(hide_factory::exec::Output {
            code: Some(0),
            stdout,
            stderr: String::new(),
        })
    }
}

#[test]
fn a_report_after_the_remote_deleted_the_task_branch_still_pushes() {
    let fx = fixture();
    let remote = fx.project.with_file_name("remote.git");
    git(
        fx.project.parent().unwrap(),
        &["init", "--quiet", "--bare", remote.to_str().unwrap()],
    );
    git(
        &fx.project,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&fx.project, &["push", "--quiet", "origin", "main"]);
    let mut projects = SharedProjects::new(
        Box::new(RealGitFakeGh(SystemRunner {
            stop: Arc::new(AtomicBool::new(false)),
        })),
        Box::new(Book::default()),
        fx.project.with_file_name("gh-logs"),
    );
    let mut factory = factory(&fx.project, &[]);
    factory.source = SourceKind::Github;
    factory.repo = Some("o/r".into());
    let t = task(&fx, "T-1", "b.txt", "two\n");
    let branch = t.worker.as_ref().unwrap().branch.clone();
    let worktree = PathBuf::from(&t.worker.as_ref().unwrap().worktree);
    let pr = projects.open_pr(&factory, &t, "body").unwrap().unwrap();
    assert_eq!(pr.number, 7);
    // The pull request merged and the remote deleted its branch; the Task
    // was reverted and reports again with a new commit.
    git(&remote, &["update-ref", "-d", &format!("refs/heads/{branch}")]);
    write(&worktree, "b.txt", "three\n");
    git(&worktree, &["commit", "--quiet", "-am", "again"]);
    projects.open_pr(&factory, &t, "body").unwrap();
    assert_eq!(
        git(&remote, &["rev-parse", &format!("refs/heads/{branch}")]),
        git(&worktree, &["rev-parse", "HEAD"]),
    );
}
