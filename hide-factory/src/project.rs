//! The real Task source, merge target and verifier: local git for every
//! Factory, `gh` for a GitHub Factory, the verify runner for a bundle. One
//! value serves every Factory on the machine and chooses by its source.
//!
//! Every write converges on an intent key (B73): an issue carries the Task
//! marker, a pull request is found by its head branch, a merge by its pull
//! request's state, a label create is `--force`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::adapters::*;
use crate::engine::task_marker;
use crate::exec::{Runner, checked, classify};
use crate::model::*;
use crate::store::sha256_hex;
use crate::verify::{Job, Prepare, VerifyRunner};

/// Local issues (`L-<n>`) for a project without GitHub, owned by the core.
pub trait IssueBook: Send {
    /// Creates an issue, or returns the one whose body carries `marker`.
    fn create(
        &mut self,
        project: &str,
        title: &str,
        body: &str,
        marker: &str,
    ) -> Result<u32, Failure>;
    fn read(&mut self, project: &str, number: u32) -> Result<IssueText, Failure>;
}

pub const LABEL: &str = "factory";
/// The check name a Factory without its own CI uses in [`MainCheck`].
const MAIN_WORKTREE: &str = "factory-main";

pub struct Projects {
    pub runner: Box<dyn Runner>,
    pub issues: Box<dyn IssueBook>,
    pub verify: VerifyRunner,
    /// Issue bodies already seen, to tell an edit from the first read.
    bodies: BTreeMap<(String, u64), String>,
    /// When each commit's checks last answered Pending: they are asked again
    /// only after `ci_poll_every`, not on every engine tick (B75).
    ci_pending: BTreeMap<String, Instant>,
    ci_poll_every: Duration,
}

/// How often a commit whose checks are still running is asked again.
pub const CI_POLL_EVERY: Duration = Duration::from_secs(30);
/// Commits remembered as pending; the oldest is forgotten past this.
const CI_PENDING_LIMIT: usize = 256;

/// One `Projects` shared by the engine's source, merge and verifier ports.
#[derive(Clone)]
pub struct SharedProjects(pub Arc<Mutex<Projects>>);

impl SharedProjects {
    pub fn new(runner: Box<dyn Runner>, issues: Box<dyn IssueBook>, logs: PathBuf) -> Self {
        Self(Arc::new(Mutex::new(Projects {
            runner,
            issues,
            verify: VerifyRunner::new(logs),
            bodies: BTreeMap::new(),
            ci_pending: BTreeMap::new(),
            ci_poll_every: CI_POLL_EVERY,
        })))
    }

    /// How long a commit with running checks waits before it is asked again;
    /// tests that script gh answers turn by turn pass zero.
    pub fn with_ci_poll_every(self, every: Duration) -> Self {
        self.lock().ci_poll_every = every;
        self
    }

    fn lock(&self) -> MutexGuard<'_, Projects> {
        self.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }
}

/// The runtimes a worker can start with here, in the order D-45 prefers.
fn installed_runtimes() -> Vec<Runtime> {
    let Ok(path) = hide_platform::host::login_path() else {
        return Vec::new();
    };
    [Runtime::Claude, Runtime::Codex]
        .into_iter()
        .filter(|runtime| hide_platform::host::find_program(&path, runtime.as_str()).is_some())
        .collect()
}

fn worktree(task: &Task) -> Result<PathBuf, Failure> {
    task.worker
        .as_ref()
        .map(|worker| PathBuf::from(&worker.worktree))
        .ok_or_else(|| Failure::task("worktree", "the Task has no worktree"))
}

fn repo(factory: &Factory) -> Result<String, Failure> {
    factory
        .repo
        .clone()
        .ok_or_else(|| Failure::task("github", "the Factory has no repository"))
}

/// The Factory-owned checkout main verification and reverts run in.
pub fn main_worktree(factory: &Factory) -> PathBuf {
    let project = Path::new(&factory.project);
    let name = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    project
        .with_file_name(format!("{name}.worktrees"))
        .join(MAIN_WORKTREE)
}

impl Projects {
    fn git(&mut self, stage: &str, cwd: &Path, args: &[&str]) -> Result<String, Failure> {
        checked(self.runner.as_mut(), stage, "git", args, Some(cwd))
    }

    fn gh(&mut self, stage: &str, args: &[&str]) -> Result<String, Failure> {
        checked(self.runner.as_mut(), stage, "gh", args, None)
    }

    fn gh_json(&mut self, stage: &str, args: &[&str]) -> Result<Value, Failure> {
        let text = self.gh(stage, args)?;
        serde_json::from_str(&text)
            .map_err(|error| Failure::task(stage, format!("gh answered no JSON: {error}")))
    }

    /// The ref Task branches are compared and merged against.
    fn base(&mut self, factory: &Factory) -> Result<String, Failure> {
        match factory.source {
            SourceKind::Local => Ok(factory.default_branch.clone()),
            SourceKind::Github => {
                let project = PathBuf::from(&factory.project);
                self.git(
                    "git.fetch",
                    &project,
                    &["fetch", "--quiet", "origin", &factory.default_branch],
                )?;
                Ok(format!("origin/{}", factory.default_branch))
            }
        }
    }

    fn ensure_main_worktree(&mut self, factory: &Factory) -> Result<PathBuf, Failure> {
        let path = main_worktree(factory);
        if !path.join(".git").exists() {
            let project = PathBuf::from(&factory.project);
            let text = path.display().to_string();
            self.git(
                "git.worktree",
                &project,
                &[
                    "worktree",
                    "add",
                    "--detach",
                    &text,
                    &factory.default_branch,
                ],
            )?;
        }
        Ok(path)
    }

    fn main_job(&mut self, factory: &Factory, id: &str, sha: &str) -> Result<(), Failure> {
        let Verification::Commands { commands } = &factory.config.verification else {
            return Ok(());
        };
        let path = self.ensure_main_worktree(factory)?;
        self.verify.submit(Job {
            id: id.to_owned(),
            cwd: path.clone(),
            commands: commands.clone(),
            prepare: vec![Prepare {
                program: "git".into(),
                args: vec![
                    "checkout".into(),
                    "--quiet".into(),
                    "--detach".into(),
                    sha.into(),
                ],
                cwd: path,
            }],
            timeout: Duration::from_millis(factory.config.verify_timeout_ms),
        })
    }

    fn main_result(
        &mut self,
        factory: &Factory,
        id: &str,
        sha: &str,
    ) -> Result<MainCheck, Failure> {
        if !self.verify.known(id) {
            self.main_job(factory, id, sha)?;
        }
        Ok(match self.verify.poll(self.runner.as_mut(), id) {
            VerifyPoll::Pending => MainCheck::Pending,
            VerifyPoll::Passed => MainCheck::Green,
            VerifyPoll::Failed { check, link } => MainCheck::Red {
                link: format!("{check}: {link}"),
            },
            VerifyPoll::Environment { check, .. } => {
                // The environment's failure says nothing about main: ask again.
                self.verify.forget(id);
                let _ = check;
                MainCheck::Pending
            }
        })
    }

    /// Required check runs on a commit (B35, B44), read at most once per
    /// `ci_poll_every` while they are pending.
    fn checks(&mut self, factory: &Factory, sha: &str) -> Result<MainCheck, Failure> {
        if self
            .ci_pending
            .get(sha)
            .is_some_and(|at| at.elapsed() < self.ci_poll_every)
        {
            return Ok(MainCheck::Pending);
        }
        let answer = self.read_checks(factory, sha)?;
        if answer == MainCheck::Pending {
            if self.ci_pending.len() >= CI_PENDING_LIMIT
                && let Some(oldest) = self
                    .ci_pending
                    .iter()
                    .min_by_key(|(_, at)| **at)
                    .map(|(sha, _)| sha.clone())
            {
                self.ci_pending.remove(&oldest);
            }
            self.ci_pending.insert(sha.to_owned(), Instant::now());
        } else {
            self.ci_pending.remove(sha);
        }
        Ok(answer)
    }

    fn read_checks(&mut self, factory: &Factory, sha: &str) -> Result<MainCheck, Failure> {
        let repo = repo(factory)?;
        let path = format!("repos/{repo}/commits/{sha}/check-runs?per_page=100");
        let value = self.gh_json("github.checks", &["api", &path])?;
        let required = match &factory.config.verification {
            Verification::Ci { checks } => checks.clone(),
            _ => Vec::new(),
        };
        let runs: Vec<&Value> = value["check_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|run| {
                required.is_empty()
                    || required
                        .iter()
                        .any(|name| run["name"].as_str() == Some(name))
            })
            .collect();
        // No run yet is not a pass: CI has not answered for this commit.
        if runs.is_empty() {
            return Ok(MainCheck::Pending);
        }
        if let Some(missing) = required
            .iter()
            .find(|name| !runs.iter().any(|run| run["name"].as_str() == Some(name)))
        {
            let _ = missing;
            return Ok(MainCheck::Pending);
        }
        for run in &runs {
            if run["status"].as_str() != Some("completed") {
                return Ok(MainCheck::Pending);
            }
        }
        for run in &runs {
            match run["conclusion"].as_str() {
                Some("success") | Some("neutral") => {}
                // A skipped or cancelled run decides nothing (B45).
                Some("skipped") | Some("cancelled") | Some("stale") => {
                    return Ok(MainCheck::Pending);
                }
                _ => {
                    let name = run["name"].as_str().unwrap_or("check");
                    let url = run["html_url"].as_str().unwrap_or_default();
                    return Ok(MainCheck::Red {
                        link: format!("{name}: {url}"),
                    });
                }
            }
        }
        Ok(MainCheck::Green)
    }

    fn head_sha(&mut self, task: &Task) -> Result<String, Failure> {
        let path = worktree(task)?;
        Ok(self
            .git("git.head", &path, &["rev-parse", "HEAD"])?
            .trim()
            .to_owned())
    }

    fn pr_view(&mut self, factory: &Factory, number: u64) -> Result<Value, Failure> {
        let repo = repo(factory)?;
        let number = number.to_string();
        self.gh_json(
            "github.pr_view",
            &[
                "pr",
                "view",
                &number,
                "--repo",
                &repo,
                "--json",
                "number,url,state,headRefName,headRefOid,mergeCommit",
            ],
        )
    }
}

fn verify_id(task: &Task, stage: &str) -> String {
    format!(
        "{}:{}:{}:{}",
        task.factory,
        task.id,
        stage,
        task.attempts.len()
    )
}

impl TaskSource for SharedProjects {
    fn probe(&mut self, project: &str) -> Result<ProjectProbe, Failure> {
        let mut this = self.lock();
        let path = PathBuf::from(project);
        this.git("probe", &path, &["rev-parse", "--show-toplevel"])?;
        let branch = this
            .git("probe", &path, &["rev-parse", "--abbrev-ref", "HEAD"])?
            .trim()
            .to_owned();
        let remote = checked(
            this.runner.as_mut(),
            "probe",
            "git",
            &["remote", "get-url", "origin"],
            Some(&path),
        )
        .unwrap_or_default();
        let mut probe = ProjectProbe {
            default_branch: branch,
            verify_candidates: verify_candidates(&path),
            runtimes: installed_runtimes(),
            ..ProjectProbe::default()
        };
        if remote.contains("github.com") {
            let view = this.gh_json("probe", &["repo", "view", remote.trim(), "--json", "nameWithOwner,defaultBranchRef,mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed"])?;
            probe.github = true;
            probe.repo = view["nameWithOwner"].as_str().map(str::to_owned);
            if let Some(branch) = view["defaultBranchRef"]["name"].as_str() {
                probe.default_branch = branch.to_owned();
            }
            for (key, method) in [
                ("mergeCommitAllowed", MergeMethod::Merge),
                ("squashMergeAllowed", MergeMethod::Squash),
                ("rebaseMergeAllowed", MergeMethod::Rebase),
            ] {
                if view[key].as_bool() == Some(true) {
                    probe.merge_methods.push(method);
                }
            }
            let repo = probe.repo.clone().unwrap_or_default();
            let path = format!(
                "repos/{repo}/branches/{}/protection/required_status_checks",
                probe.default_branch
            );
            let args: Vec<String> = ["api", &path].iter().map(|a| (*a).to_owned()).collect();
            let answer = this.runner.run("gh", &args, None)?;
            if answer.ok() {
                let value: Value = serde_json::from_str(&answer.stdout).unwrap_or_default();
                probe.required_checks = value["contexts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
            } else if !answer.stderr.contains("404") {
                return Err(classify("probe", &answer));
            }
        }
        Ok(probe)
    }

    fn planned_writes(&self, probe: &ProjectProbe) -> Vec<String> {
        if probe.github {
            vec![format!(
                "GitHub label `{LABEL}` on {}",
                probe.repo.clone().unwrap_or_default()
            )]
        } else {
            Vec::new()
        }
    }

    fn prepare(&mut self, factory: &Factory) -> Result<(), Failure> {
        if factory.source != SourceKind::Github {
            return Ok(());
        }
        let repo = repo(factory)?;
        self.lock().gh(
            "github.label",
            &[
                "label",
                "create",
                LABEL,
                "--repo",
                &repo,
                "--force",
                "--color",
                "5319e7",
                "--description",
                "Software Factory Task",
            ],
        )?;
        Ok(())
    }

    fn create_issue(
        &mut self,
        factory: &Factory,
        task: &Task,
        body: &str,
    ) -> Result<IssueRef, Failure> {
        let marker = task_marker(factory, task);
        let mut this = self.lock();
        match factory.source {
            SourceKind::Local => {
                let number =
                    this.issues
                        .create(&factory.project, &task.card.title, body, &marker)?;
                Ok(IssueRef::Local { number })
            }
            SourceKind::Github => {
                let repo = repo(factory)?;
                let search = format!("\"{marker}\" in:body");
                let found = this.gh_json(
                    "github.issue_find",
                    &[
                        "issue",
                        "list",
                        "--repo",
                        &repo,
                        "--state",
                        "all",
                        "--search",
                        &search,
                        "--json",
                        "number,body",
                    ],
                )?;
                if let Some(number) = found
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|issue| issue["body"].as_str().is_some_and(|b| b.contains(&marker)))
                    .and_then(|issue| issue["number"].as_u64())
                {
                    return Ok(IssueRef::Github { number });
                }
                let url = this.gh(
                    "github.issue_create",
                    &[
                        "issue",
                        "create",
                        "--repo",
                        &repo,
                        "--title",
                        &task.card.title,
                        "--body",
                        body,
                        "--label",
                        LABEL,
                    ],
                )?;
                number_from_url(&url)
                    .map(|number| IssueRef::Github { number })
                    .ok_or_else(|| Failure::task("github.issue_create", "gh answered no issue URL"))
            }
        }
    }

    fn label_issue(&mut self, factory: &Factory, issue: &IssueRef) -> Result<(), Failure> {
        let IssueRef::Github { number } = issue else {
            return Ok(());
        };
        let repo = repo(factory)?;
        let number = number.to_string();
        self.lock().gh(
            "github.label_issue",
            &[
                "issue",
                "edit",
                &number,
                "--repo",
                &repo,
                "--add-label",
                LABEL,
            ],
        )?;
        Ok(())
    }

    fn read_issue(&mut self, factory: &Factory, issue: &IssueRef) -> Result<IssueText, Failure> {
        let mut this = self.lock();
        match issue {
            IssueRef::Local { number } => this.issues.read(&factory.project, *number),
            IssueRef::Github { number } => {
                let repo = repo(factory)?;
                let number = number.to_string();
                let value = this.gh_json(
                    "github.issue_view",
                    &[
                        "issue",
                        "view",
                        &number,
                        "--repo",
                        &repo,
                        "--json",
                        "title,body,state",
                    ],
                )?;
                Ok(IssueText {
                    title: value["title"].as_str().unwrap_or_default().to_owned(),
                    body: value["body"].as_str().unwrap_or_default().to_owned(),
                    open: value["state"].as_str() == Some("OPEN"),
                })
            }
        }
    }

    fn observe(
        &mut self,
        factory: &Factory,
        tasks: &[&Task],
    ) -> Result<Vec<OutsideEvent>, Failure> {
        let mut this = self.lock();
        let mut events = Vec::new();
        if factory.source == SourceKind::Local {
            for task in tasks {
                let Some(IssueRef::Local { number }) = task.issue else {
                    continue;
                };
                if matches!(task.state, TaskState::Done | TaskState::Cancelled) {
                    continue;
                }
                if let Ok(text) = this.issues.read(&factory.project, number)
                    && !text.open
                {
                    events.push(OutsideEvent::IssueClosed {
                        issue: IssueRef::Local { number },
                    });
                }
            }
            return Ok(events);
        }
        let repo = repo(factory)?;
        let issues = this.gh_json(
            "github.observe",
            &[
                "issue",
                "list",
                "--repo",
                &repo,
                "--label",
                LABEL,
                "--state",
                "all",
                "--limit",
                "200",
                "--json",
                "number,title,body,state",
            ],
        )?;
        let prs = this.gh_json(
            "github.observe",
            &[
                "pr",
                "list",
                "--repo",
                &repo,
                "--state",
                "all",
                "--limit",
                "100",
                "--json",
                "number,url,state,headRefName,closingIssuesReferences",
            ],
        )?;
        let held: BTreeMap<u64, &&Task> = tasks
            .iter()
            .filter_map(|task| match task.issue {
                Some(IssueRef::Github { number }) => Some((number, task)),
                _ => None,
            })
            .collect();
        let labelled: BTreeMap<u64, &Value> = issues
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|issue| issue["number"].as_u64().map(|n| (n, issue)))
            .collect();
        for (number, issue) in &labelled {
            let issue_ref = IssueRef::Github { number: *number };
            let body = issue["body"].as_str().unwrap_or_default();
            let hash = sha256_hex(body.as_bytes());
            match held.get(number) {
                None => {
                    if issue["state"].as_str() == Some("OPEN")
                        && !body.contains("<!-- hide-factory:")
                    {
                        events.push(OutsideEvent::Labeled {
                            issue: issue_ref,
                            title: issue["title"].as_str().unwrap_or_default().to_owned(),
                            body: body.to_owned(),
                        });
                    }
                }
                Some(task) => {
                    let key = (factory.id.clone(), *number);
                    let previous = this.bodies.insert(key, hash.clone());
                    if previous.is_some_and(|previous| previous != hash)
                        || task
                            .source_body_hash
                            .as_ref()
                            .is_some_and(|known| *known != hash)
                    {
                        events.push(OutsideEvent::BodyEdited {
                            issue: issue_ref.clone(),
                            body_hash: hash,
                        });
                    }
                    let closing: Vec<&Value> = prs
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|pr| {
                            pr["closingIssuesReferences"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .any(|i| i["number"].as_u64() == Some(*number))
                        })
                        .filter(|pr| {
                            task.pr
                                .as_ref()
                                .is_none_or(|own| pr["number"].as_u64() != Some(own.number))
                        })
                        .filter(|pr| {
                            pr["headRefName"].as_str() != Some(task.branch_slug().as_str())
                        })
                        .collect();
                    if let Some(pr) = closing
                        .iter()
                        .find(|pr| pr["state"].as_str() != Some("CLOSED"))
                    {
                        events.push(OutsideEvent::ClosingPr {
                            issue: issue_ref.clone(),
                            pr: pr["number"].as_u64().unwrap_or_default(),
                            url: pr["url"].as_str().unwrap_or_default().to_owned(),
                            merged: pr["state"].as_str() == Some("MERGED"),
                        });
                    } else if issue["state"].as_str() == Some("CLOSED") && !task.state.merged() {
                        events.push(OutsideEvent::IssueClosed {
                            issue: issue_ref.clone(),
                        });
                    }
                    if task.state == TaskState::Done
                        && issue["state"].as_str() == Some("OPEN")
                        && task.done_at.is_some()
                    {
                        events.push(OutsideEvent::IssueReopened { issue: issue_ref });
                    }
                }
            }
        }
        for (number, task) in held {
            if !labelled.contains_key(&number)
                && !matches!(task.state, TaskState::Done | TaskState::Cancelled)
            {
                events.push(OutsideEvent::LabelRemoved {
                    issue: IssueRef::Github { number },
                });
            }
        }
        Ok(events)
    }
}

impl MergeTarget for SharedProjects {
    fn main_head(&mut self, factory: &Factory) -> Result<String, Failure> {
        let mut this = self.lock();
        let base = this.base(factory)?;
        let project = PathBuf::from(&factory.project);
        Ok(this
            .git("git.main_head", &project, &["rev-parse", &base])?
            .trim()
            .to_owned())
    }

    fn open_pr(
        &mut self,
        factory: &Factory,
        task: &Task,
        body: &str,
    ) -> Result<Option<PullRequest>, Failure> {
        if factory.source != SourceKind::Github {
            return Ok(None);
        }
        let repo = repo(factory)?;
        let branch = task
            .worker
            .as_ref()
            .map(|w| w.branch.clone())
            .unwrap_or_else(|| task.branch_slug());
        let mut this = self.lock();
        let existing = this.gh_json(
            "github.pr_find",
            &[
                "pr",
                "list",
                "--repo",
                &repo,
                "--head",
                &branch,
                "--state",
                "all",
                "--json",
                "number,url,state,headRefName",
            ],
        )?;
        if let Some(pr) = existing.as_array().and_then(|prs| prs.first()) {
            // A harness opened it, or an earlier attempt did (B36, B73).
            return Ok(Some(PullRequest {
                number: pr["number"].as_u64().unwrap_or_default(),
                url: pr["url"].as_str().unwrap_or_default().to_owned(),
                head: branch,
                by_factory: false,
                open: pr["state"].as_str() == Some("OPEN"),
            }));
        }
        let path = worktree(task)?;
        this.git(
            "git.push",
            &path,
            &["push", "--quiet", "--set-upstream", "origin", &branch],
        )?;
        let url = this.gh(
            "github.pr_create",
            &[
                "pr",
                "create",
                "--repo",
                &repo,
                "--head",
                &branch,
                "--base",
                &factory.default_branch,
                "--title",
                &task.card.title,
                "--body",
                body,
            ],
        )?;
        let number = number_from_url(&url)
            .ok_or_else(|| Failure::task("github.pr_create", "gh answered no pull request URL"))?;
        Ok(Some(PullRequest {
            number,
            url: url.trim().to_owned(),
            head: branch,
            by_factory: true,
            open: true,
        }))
    }

    fn close_pr(&mut self, factory: &Factory, pr: &PullRequest) -> Result<(), Failure> {
        let repo = repo(factory)?;
        let number = pr.number.to_string();
        let mut this = self.lock();
        let state = this.pr_view(factory, pr.number)?;
        if state["state"].as_str() != Some("OPEN") {
            return Ok(());
        }
        this.gh(
            "github.pr_close",
            &["pr", "close", &number, "--repo", &repo],
        )?;
        Ok(())
    }

    fn reopen_pr(&mut self, factory: &Factory, pr: &PullRequest) -> Result<(), Failure> {
        let repo = repo(factory)?;
        let number = pr.number.to_string();
        let mut this = self.lock();
        let state = this.pr_view(factory, pr.number)?;
        if state["state"].as_str() != Some("CLOSED") {
            return Ok(());
        }
        this.gh(
            "github.pr_reopen",
            &["pr", "reopen", &number, "--repo", &repo],
        )?;
        Ok(())
    }

    fn diff_lines(&mut self, factory: &Factory, task: &Task) -> Result<u32, Failure> {
        let mut this = self.lock();
        let base = this.base(factory)?;
        let path = worktree(task)?;
        let range = format!("{base}...HEAD");
        let text = this.git("git.diff", &path, &["diff", "--numstat", &range])?;
        Ok(text
            .lines()
            .map(|line| {
                let mut parts = line.split_whitespace();
                let added: u32 = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                let removed: u32 = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                added + removed
            })
            .sum())
    }

    fn changed_paths(&mut self, factory: &Factory, task: &Task) -> Result<Vec<String>, Failure> {
        let mut this = self.lock();
        let base = this.base(factory)?;
        let path = worktree(task)?;
        let range = format!("{base}...HEAD");
        Ok(this
            .git("git.diff", &path, &["diff", "--name-only", &range])?
            .lines()
            .map(str::to_owned)
            .collect())
    }

    fn diff_text(&mut self, factory: &Factory, task: &Task) -> Result<String, Failure> {
        let mut this = self.lock();
        let base = this.base(factory)?;
        let path = worktree(task)?;
        let range = format!("{base}...HEAD");
        let text = this.git("git.diff", &path, &["diff", "--stat", "--patch", &range])?;
        Ok(crate::judgment::cut(&text, crate::judgment::DIFF_LIMIT))
    }

    fn premerge(&mut self, factory: &Factory, task: &Task) -> Result<PreMerge, Failure> {
        let mut this = self.lock();
        let base = this.base(factory)?;
        let path = worktree(task)?;
        // merge-tree answers 1 with the conflicted files listed (B38).
        let args: Vec<String> = [
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            &base,
            "HEAD",
        ]
        .iter()
        .map(|a| (*a).to_owned())
        .collect();
        let output = this.runner.run("git", &args, Some(&path))?;
        match output.code {
            Some(0) => {}
            Some(1) => {
                let files: Vec<String> = output
                    .stdout
                    .lines()
                    .skip(1)
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_owned)
                    .collect();
                return Ok(PreMerge::Conflict { files });
            }
            _ => return Err(classify("git.merge_tree", &output)),
        }
        if let Some(check) = factory.config.quick_check.clone() {
            let (program, flag) = crate::exec::shell();
            let args = vec![flag.to_owned(), check.clone()];
            let output = this.runner.run(program, &args, Some(&path))?;
            if !output.ok() {
                let failure = classify("quick_check", &output);
                if failure.signal.is_some() {
                    return Err(failure);
                }
                return Ok(PreMerge::QuickCheckFailed { check });
            }
        }
        if !factory.config.risk_paths.is_empty() {
            let range = format!("{base}...HEAD");
            let changed: Vec<String> = this
                .git("git.diff", &path, &["diff", "--name-only", &range])?
                .lines()
                .map(str::to_owned)
                .collect();
            let paths: Vec<String> = changed
                .into_iter()
                .filter(|p| {
                    factory
                        .config
                        .risk_paths
                        .iter()
                        .any(|risk| path_matches(risk, p))
                })
                .collect();
            if !paths.is_empty() {
                return Ok(PreMerge::RiskPath { paths });
            }
        }
        Ok(PreMerge::Clean)
    }

    fn main_dirty(&mut self, factory: &Factory) -> Result<bool, Failure> {
        let mut this = self.lock();
        let project = PathBuf::from(&factory.project);
        let branch = this.git(
            "git.status",
            &project,
            &["rev-parse", "--abbrev-ref", "HEAD"],
        )?;
        if branch.trim() != factory.default_branch {
            return Ok(true);
        }
        let status = this.git(
            "git.status",
            &project,
            &["status", "--porcelain", "--untracked-files=no"],
        )?;
        Ok(!status.trim().is_empty())
    }

    fn merge(
        &mut self,
        factory: &Factory,
        task: &Task,
        method: MergeMethod,
    ) -> Result<String, Failure> {
        let mut this = self.lock();
        match factory.source {
            SourceKind::Local => {
                let project = PathBuf::from(&factory.project);
                let branch = task
                    .worker
                    .as_ref()
                    .map(|w| w.branch.clone())
                    .unwrap_or_else(|| task.branch_slug());
                let ancestor = this.runner.run(
                    "git",
                    &[
                        "merge-base".into(),
                        "--is-ancestor".into(),
                        branch.clone(),
                        "HEAD".into(),
                    ],
                    Some(&project),
                )?;
                if !ancestor.ok() {
                    let message = format!(
                        "Merge {} {}: {}",
                        task.display_id(),
                        branch,
                        task.card.title
                    );
                    this.git(
                        "git.merge",
                        &project,
                        &["merge", "--no-ff", "--no-edit", "-m", &message, &branch],
                    )?;
                }
                Ok(this
                    .git("git.merge", &project, &["rev-parse", "HEAD"])?
                    .trim()
                    .to_owned())
            }
            SourceKind::Github => {
                let repo = repo(factory)?;
                let pr = task
                    .pr
                    .clone()
                    .ok_or_else(|| Failure::task("github.merge", "the Task has no pull request"))?;
                let view = this.pr_view(factory, pr.number)?;
                if view["state"].as_str() == Some("MERGED") {
                    return Ok(view["mergeCommit"]["oid"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned());
                }
                // Pinned to the commit that passed verification: a push after
                // it makes GitHub refuse the merge (B39).
                let head = task
                    .attempts
                    .iter()
                    .rev()
                    .find(|attempt| matches!(attempt.outcome, Some(AttemptOutcome::Passed)))
                    .and_then(|attempt| attempt.commit.clone())
                    .unwrap_or_else(|| view["headRefOid"].as_str().unwrap_or_default().to_owned());
                let number = pr.number.to_string();
                let flag = match method {
                    MergeMethod::Merge => "--merge",
                    MergeMethod::Squash => "--squash",
                    MergeMethod::Rebase => "--rebase",
                };
                this.gh(
                    "github.merge",
                    &[
                        "pr",
                        "merge",
                        &number,
                        "--repo",
                        &repo,
                        flag,
                        "--match-head-commit",
                        &head,
                    ],
                )?;
                let view = this.pr_view(factory, pr.number)?;
                Ok(view["mergeCommit"]["oid"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned())
            }
        }
    }

    fn main_check(&mut self, factory: &Factory, sha: &str) -> Result<MainCheck, Failure> {
        let mut this = self.lock();
        match &factory.config.verification {
            Verification::None => Ok(MainCheck::None),
            Verification::Ci { .. } => this.checks(factory, sha),
            Verification::Commands { .. } => {
                let id = format!("{}:main:{sha}", factory.id);
                this.main_result(factory, &id, sha)
            }
        }
    }

    fn rerun_main(&mut self, factory: &Factory, sha: &str) -> Result<(), Failure> {
        let mut this = self.lock();
        match &factory.config.verification {
            Verification::Ci { .. } => {
                let repo = repo(factory)?;
                let path = format!("repos/{repo}/actions/runs?head_sha={sha}&per_page=50");
                let runs = this.gh_json("github.runs", &["api", &path])?;
                for run in runs["workflow_runs"].as_array().into_iter().flatten() {
                    if matches!(
                        run["conclusion"].as_str(),
                        Some("cancelled") | Some("skipped") | Some("stale")
                    ) && let Some(id) = run["id"].as_u64()
                    {
                        let id = id.to_string();
                        this.gh("github.rerun", &["run", "rerun", &id, "--repo", &repo])?;
                    }
                }
                Ok(())
            }
            Verification::Commands { .. } => {
                let id = format!("{}:main:{sha}", factory.id);
                this.verify.forget(&id);
                this.main_job(factory, &id, sha)
            }
            Verification::None => Ok(()),
        }
    }

    fn revert(&mut self, factory: &Factory, task: &Task, sha: &str) -> Result<RevertRef, Failure> {
        let mut this = self.lock();
        let base = this.base(factory)?;
        let path = this.ensure_main_worktree(factory)?;
        this.git(
            "git.revert",
            &path,
            &["checkout", "--quiet", "--detach", &base],
        )?;
        let parents = this.git(
            "git.revert",
            &path,
            &["rev-list", "--parents", "-n", "1", sha],
        )?;
        let mut args = vec!["revert", "--no-edit"];
        if parents.split_whitespace().count() > 2 {
            args.extend(["-m", "1"]);
        }
        args.push(sha);
        this.git("git.revert", &path, &args)?;
        let commit = this
            .git("git.revert", &path, &["rev-parse", "HEAD"])?
            .trim()
            .to_owned();
        if factory.source == SourceKind::Local {
            return Ok(RevertRef {
                task: task.id.clone(),
                sha: sha.to_owned(),
                pr: None,
                commit: Some(commit),
            });
        }
        let repo = repo(factory)?;
        let branch = format!("factory/revert-{}", task.id.to_ascii_lowercase());
        let refspec = format!("HEAD:refs/heads/{branch}");
        this.git(
            "git.push",
            &path,
            &["push", "--quiet", "--force", "origin", &refspec],
        )?;
        let existing = this.gh_json(
            "github.pr_find",
            &[
                "pr", "list", "--repo", &repo, "--head", &branch, "--state", "open", "--json",
                "number",
            ],
        )?;
        let number = match existing
            .as_array()
            .and_then(|prs| prs.first())
            .and_then(|pr| pr["number"].as_u64())
        {
            Some(number) => number,
            None => {
                let title = format!("Revert {} {}", task.display_id(), task.card.title);
                let body = format!(
                    "{}\n\nmain verification failed after this merge ({sha}); the Factory reverts it alone.",
                    task_marker(factory, task)
                );
                let url = this.gh(
                    "github.pr_create",
                    &[
                        "pr",
                        "create",
                        "--repo",
                        &repo,
                        "--head",
                        &branch,
                        "--base",
                        &factory.default_branch,
                        "--title",
                        &title,
                        "--body",
                        &body,
                    ],
                )?;
                number_from_url(&url).ok_or_else(|| {
                    Failure::task("github.pr_create", "gh answered no pull request URL")
                })?
            }
        };
        Ok(RevertRef {
            task: task.id.clone(),
            sha: sha.to_owned(),
            pr: Some(number),
            commit: Some(commit),
        })
    }

    fn revert_check(
        &mut self,
        factory: &Factory,
        revert: &RevertRef,
    ) -> Result<MainCheck, Failure> {
        let mut this = self.lock();
        let commit = revert.commit.clone().unwrap_or_default();
        match &factory.config.verification {
            Verification::None => Ok(MainCheck::None),
            Verification::Ci { .. } => this.checks(factory, &commit),
            Verification::Commands { .. } => {
                let id = format!("{}:revert:{commit}", factory.id);
                this.main_result(factory, &id, &commit)
            }
        }
    }

    fn merge_revert(&mut self, factory: &Factory, revert: &RevertRef) -> Result<String, Failure> {
        let mut this = self.lock();
        let commit = revert.commit.clone().unwrap_or_default();
        match (factory.source, revert.pr) {
            (SourceKind::Github, Some(number)) => {
                let repo = repo(factory)?;
                let view = this.pr_view(factory, number)?;
                if view["state"].as_str() != Some("MERGED") {
                    let number = number.to_string();
                    this.gh(
                        "github.revert_merge",
                        &[
                            "pr",
                            "merge",
                            &number,
                            "--repo",
                            &repo,
                            "--merge",
                            "--match-head-commit",
                            &commit,
                        ],
                    )?;
                }
                let view = this.pr_view(factory, number)?;
                Ok(view["mergeCommit"]["oid"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned())
            }
            _ => {
                let project = PathBuf::from(&factory.project);
                this.git(
                    "git.revert_merge",
                    &project,
                    &["merge", "--ff-only", &commit],
                )?;
                Ok(commit)
            }
        }
    }
}

impl Verifier for SharedProjects {
    fn start(&mut self, factory: &Factory, task: &Task) -> Result<VerifyRun, Failure> {
        let mut this = self.lock();
        match &factory.config.verification {
            Verification::Commands { commands } => {
                let id = verify_id(task, "task");
                let cwd = worktree(task)?;
                let commit = this.head_sha(task)?;
                this.verify.forget(&id);
                this.verify.submit(Job {
                    id: id.clone(),
                    cwd,
                    commands: commands.clone(),
                    prepare: Vec::new(),
                    timeout: Duration::from_millis(factory.config.verify_timeout_ms),
                })?;
                let log = this.verify.log_path(&id).display().to_string();
                Ok(VerifyRun {
                    id,
                    log: Some(log),
                    commit: Some(commit),
                })
            }
            Verification::Ci { .. } => {
                let sha = this.head_sha(task)?;
                Ok(VerifyRun {
                    id: format!("ci:{sha}"),
                    log: task.pr.as_ref().map(|pr| pr.url.clone()),
                    commit: Some(sha),
                })
            }
            Verification::None => Err(Failure::task("verify", "no verification")),
        }
    }

    fn start_premerge(&mut self, factory: &Factory, task: &Task) -> Result<VerifyRun, Failure> {
        let mut this = self.lock();
        let Verification::Commands { commands } = &factory.config.verification else {
            return Err(Failure::task(
                "verify",
                "pre-merge runs only for a verify bundle",
            ));
        };
        let base = this.base(factory)?;
        let cwd = worktree(task)?;
        let id = verify_id(task, "premerge");
        this.verify.forget(&id);
        // The bundle runs on the latest main merged into the Task (B38).
        let prepare = vec![Prepare {
            program: "git".into(),
            args: vec!["merge".into(), "--no-edit".into(), "--quiet".into(), base],
            cwd: cwd.clone(),
        }];
        this.verify.submit(Job {
            id: id.clone(),
            cwd,
            commands: commands.clone(),
            prepare,
            timeout: Duration::from_millis(factory.config.verify_timeout_ms),
        })?;
        let log = this.verify.log_path(&id).display().to_string();
        Ok(VerifyRun {
            id,
            log: Some(log),
            commit: None,
        })
    }

    fn poll(&mut self, factory: &Factory, run: &VerifyRun) -> VerifyPoll {
        let mut this = self.lock();
        if let Some(sha) = run.id.strip_prefix("ci:") {
            return match this.checks(factory, sha) {
                Ok(MainCheck::Green) | Ok(MainCheck::None) => VerifyPoll::Passed,
                Ok(MainCheck::Pending) => VerifyPoll::Pending,
                Ok(MainCheck::Red { link }) => VerifyPoll::Failed {
                    check: link.split(':').next().unwrap_or("check").to_owned(),
                    link,
                },
                Err(failure) => match failure.signal {
                    Some(signal) => VerifyPoll::Environment {
                        signal,
                        check: "github.checks".into(),
                    },
                    None => VerifyPoll::Pending,
                },
            };
        }
        let id = run.id.clone();
        let Projects { runner, verify, .. } = &mut *this;
        verify.poll(runner.as_mut(), &id)
    }

    fn cancel(&mut self, run: &VerifyRun) {
        self.lock().verify.cancel(&run.id);
    }
}

/// `prefix/**`, `*.ext` or an exact path.
fn path_matches(pattern: &str, path: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return path.starts_with(&format!("{prefix}/"));
    }
    if let Some(suffix) = pattern.strip_prefix("*") {
        return path.ends_with(suffix);
    }
    path == pattern || path.starts_with(&format!("{}/", pattern.trim_end_matches('/')))
}

fn number_from_url(text: &str) -> Option<u64> {
    text.trim()
        .lines()
        .last()?
        .rsplit('/')
        .next()?
        .trim()
        .parse()
        .ok()
}

/// Verify command candidates read from the project's files (B1).
fn verify_candidates(project: &Path) -> Vec<String> {
    let mut candidates = Vec::new();
    if project.join("Cargo.toml").exists() {
        candidates.push("cargo test".into());
    }
    if let Ok(text) = std::fs::read_to_string(project.join("package.json"))
        && let Ok(value) = serde_json::from_str::<Value>(&text)
        && value["scripts"]["test"].is_string()
    {
        let tool = if project.join("pnpm-lock.yaml").exists() {
            "pnpm"
        } else {
            "npm"
        };
        candidates.push(format!("{tool} test"));
    }
    if let Ok(text) = std::fs::read_to_string(project.join("Makefile"))
        && text.lines().any(|line| line.starts_with("test:"))
    {
        candidates.push("make test".into());
    }
    if project.join("pyproject.toml").exists() {
        candidates.push("pytest".into());
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_patterns_match_folders_extensions_and_exact_paths() {
        assert!(path_matches("migrations/**", "migrations/001.sql"));
        assert!(!path_matches("migrations/**", "src/migrations.rs"));
        assert!(path_matches("*.sql", "db/a.sql"));
        assert!(path_matches("Cargo.lock", "Cargo.lock"));
        assert!(path_matches("auth", "auth/token.rs"));
        assert_eq!(
            number_from_url("https://github.com/o/r/pull/42\n"),
            Some(42)
        );
    }
}
