//! Every worktree of every open repository, with the counts the sidebar row
//! and the summary card show.
//!
//! A worktree earns a row because git lists it, not because Herdr has a pane
//! in it: that is the whole point of the projects tree, and it is why this
//! reader exists rather than the catalog forking `git` itself. Each read runs
//! on a worker thread ([`crate::reader::BackgroundRead`]), so a repository
//! with many worktrees costs wall time on that thread and never coordinator
//! latency.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hide_node_link::cleanup::RepositoryDirs;
use hide_node_link::protocol::Call;
use hide_node_link::worktrees::{GIT_WATCH_LIMIT, GitWatchReport};

use crate::model::{
    ProjectWorktreesSnapshot, UnpushedSnapshot, WorktreeCatalogSnapshot,
    WorktreeDeletionGateSnapshot, WorktreeSnapshot,
};
use crate::reader::BackgroundRead;

/// What to describe: one entry per project the navigator is showing.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorktreeRequest {
    pub projects: Vec<WorktreeProjectRequest>,
    /// Bumped when something invalidates every answer at once - a manual
    /// refresh, most of all. A changed request is always due, so this is how
    /// an unchanged project list still gets re-read.
    pub generation: u64,
    /// The local worktree removals the runtime had settled. It invalidates
    /// no answer - a removal changes its own repository's Git directory, which
    /// that project's freshness key already watches - and travels back with
    /// the answer, so the runtime can tell a read that started before a
    /// removal from one that started after it.
    pub removals: u64,
}

/// One read's catalog and the [`WorktreeRequest::removals`] it started under.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorktreeAnswer {
    pub catalog: WorktreeCatalogSnapshot,
    pub removals: u64,
    /// The input this answer actually read, to keep loading visible when a
    /// later Overview opening overtook an in-flight answer.
    pub request: WorktreeRequest,
    /// The Git facts observed by this read still match the reader's latest
    /// watch state. A ref write can overtake a read without changing the
    /// runtime's explicit request.
    pub observations_current: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeProjectRequest {
    /// A path inside the repository; the reader resolves it to the main
    /// worktree itself.
    pub root_path: PathBuf,
    /// The base branch to measure a branch against, when the pull request
    /// lookup has already named one. A branch missing here falls back to the
    /// repository default branch.
    pub bases: BTreeMap<String, String>,
    pub base_override: Option<String>,
    /// An explicit read of this project, independent of other repositories.
    pub generation: u64,
}

/// One project's freshness key: its request and OS watch generation.
/// Each project carries its own key so a commit in one repository re-reads
/// that repository alone. The first version keyed the whole catalog on every
/// project's stamps together, and a checkpoint commit anywhere re-ran
/// `git status` in every registered repository, including one on iCloud
/// whose evicted files that status pulled back down for minutes.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ProjectObservation {
    request: WorktreeProjectRequest,
    git_generation: u64,
}

/// The generation, the settled removals and every project's observation.
type ObservedRequest = (u64, u64, Vec<ProjectObservation>);

/// One project's answer. `None` is a folder that is not a repository, which
/// contributes no project entry at all.
type ProjectAnswer = (PathBuf, Option<ProjectWorktreesSnapshot>);

const GIT_WATCH_DEBOUNCE: Duration = Duration::from_millis(300);
/// How long a Git watch that ended is left before it is started again.
const GIT_WATCH_RETRY: Duration = Duration::from_secs(30);
/// A Git watch runs until it is stopped; this only bounds a node that never
/// answers at all.
const GIT_WATCH_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);
/// One repository lookup on the node while the watched set changes.
const REPOSITORY_TIMEOUT: Duration = Duration::from_secs(10);

/// One Git watch on the node that holds the repositories, over every
/// project's common directory (`Call::GitWatch`). The coordinator drains its
/// reports on each wake, keeps only the projects whose Git facts changed,
/// and lets its existing background reader do the Git work once a project
/// has been quiet for [`GIT_WATCH_DEBOUNCE`]. A change of the watched set
/// starts a new watch and keeps the old one until the new one is watching,
/// so a repository in both is never unwatched; a watch that ended, or could
/// not start, is started again after [`GIT_WATCH_RETRY`], and once it is
/// watching every project is read again, because anything may have changed
/// in between.
/// The Explorer watcher in hided polls only the selected checkout's visible
/// directories, so it cannot observe every registered project's Git refs
/// without idle polling and another daemon-to-core event path.
struct GitWatch {
    node: std::sync::Arc<dyn crate::node_access::NodeLink>,
    subscription: Option<GitSubscription>,
    /// The watch `subscription` replaced, until `subscription` is watching.
    outgoing: Option<GitSubscription>,
    /// When a watch that ended is started again.
    retry_at: Option<Instant>,
    /// Each watched Git common directory and the projects that share it.
    roots: BTreeMap<PathBuf, Vec<PathBuf>>,
    /// Projects whose Git facts changed, with the last time they did.
    pending: BTreeMap<PathBuf, Instant>,
    registered: BTreeMap<PathBuf, PathBuf>,
    requested_roots: Vec<PathBuf>,
}

/// A running `Call::GitWatch` and its reports. Dropping it stops the watch,
/// which ends on the node within one report.
struct GitSubscription {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    reports: std::sync::mpsc::Receiver<Result<GitWatchReport, String>>,
    /// The node has said it is watching; a change before that may be missed.
    watching: bool,
    /// Started after a watch ended or could not start, so once it is
    /// watching every project is read again.
    catch_up: bool,
}

impl GitSubscription {
    fn start(
        node: &std::sync::Arc<dyn crate::node_access::NodeLink>,
        common_dirs: Vec<String>,
        catch_up: bool,
    ) -> Option<Self> {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (sender, reports) = std::sync::mpsc::channel();
        let node = std::sync::Arc::clone(node);
        let stopped = std::sync::Arc::clone(&stop);
        let started = std::thread::Builder::new()
            .name("hide-git-watch".to_owned())
            .spawn(move || {
                let ended = crate::node_access::call_as_with_progress::<(), GitWatchReport>(
                    node.as_ref(),
                    Call::GitWatch { common_dirs },
                    GIT_WATCH_TIMEOUT,
                    |report| {
                        !stopped.load(std::sync::atomic::Ordering::Relaxed)
                            && sender.send(Ok(report)).is_ok()
                    },
                );
                let reason = match ended {
                    Ok(()) => "the Git watch ended".to_owned(),
                    Err(error) => error.to_string(),
                };
                let _ = sender.send(Err(reason));
            });
        match started {
            Ok(_) => Some(Self {
                stop,
                reports,
                watching: false,
                catch_up,
            }),
            Err(error) => {
                crate::diagnostic!(serde_json::json!({
                    "component": "worktrees", "kind": "git_watch.start_failed", "message": error.to_string()
                }));
                None
            }
        }
    }
}

impl Drop for GitSubscription {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl GitWatch {
    fn new(node: std::sync::Arc<dyn crate::node_access::NodeLink>) -> Self {
        Self {
            node,
            subscription: None,
            outgoing: None,
            retry_at: None,
            roots: BTreeMap::new(),
            pending: BTreeMap::new(),
            registered: BTreeMap::new(),
            requested_roots: Vec::new(),
        }
    }

    fn reconcile(&mut self, projects: &[WorktreeProjectRequest]) {
        let requested_roots: Vec<_> = projects
            .iter()
            .map(|project| project.root_path.clone())
            .collect();
        let restart_due = self.subscription.is_none()
            && !self.registered.is_empty()
            && self.retry_at.is_some_and(|at| at <= Instant::now());
        if requested_roots == self.requested_roots {
            if restart_due {
                let common: BTreeSet<_> = self.registered.values().cloned().collect();
                self.start(&common, true);
            }
            return;
        }
        self.requested_roots = requested_roots;
        let desired: BTreeMap<PathBuf, PathBuf> = projects
            .iter()
            .take(GIT_WATCH_LIMIT)
            .filter_map(|project| {
                crate::node_access::call_as::<Option<RepositoryDirs>>(
                    self.node.as_ref(),
                    Call::Repository {
                        path: project.root_path.to_string_lossy().into_owned(),
                    },
                    REPOSITORY_TIMEOUT,
                )
                .ok()
                .flatten()
                .map(|repository| {
                    (
                        project.root_path.clone(),
                        PathBuf::from(repository.common_dir),
                    )
                })
            })
            .collect();
        if projects.len() > GIT_WATCH_LIMIT {
            crate::diagnostic!(serde_json::json!({
                "component": "worktrees", "kind": "git_watch.over_budget",
                "projects": projects.len(), "cap": GIT_WATCH_LIMIT
            }));
        }
        let desired_common: BTreeSet<_> = desired.values().cloned().collect();
        let previous_common: BTreeSet<_> = self.registered.values().cloned().collect();
        if desired_common != previous_common {
            // A watch waiting to restart has missed changes; one running
            // has not, and stays until its replacement is watching.
            let catch_up = self.subscription.is_none() && !previous_common.is_empty();
            if let Some(running) = self.subscription.take()
                && self.outgoing.is_none()
            {
                self.outgoing = Some(running);
            }
            self.start(&desired_common, catch_up);
        } else if restart_due {
            self.start(&desired_common, true);
        }
        let mut roots = BTreeMap::<PathBuf, Vec<PathBuf>>::new();
        for (project, common) in &desired {
            roots
                .entry(common.clone())
                .or_default()
                .push(project.clone());
        }
        self.roots = roots;
        self.registered = desired;
        self.pending
            .retain(|project, _| self.registered.contains_key(project));
    }

    /// Starts the watch over `common`, or schedules another try when the
    /// thread cannot start. No common directory is no watch.
    fn start(&mut self, common: &BTreeSet<PathBuf>, catch_up: bool) {
        self.subscription = None;
        self.retry_at = None;
        if common.is_empty() {
            self.outgoing = None;
            return;
        }
        self.subscription = GitSubscription::start(
            &self.node,
            common
                .iter()
                .map(|common| common.to_string_lossy().into_owned())
                .collect(),
            catch_up,
        );
        if self.subscription.is_none() {
            self.retry_at = Some(Instant::now() + GIT_WATCH_RETRY);
        }
    }

    /// Moves what the watch reported into `pending`: a changed Git fact
    /// marks the projects of its repository, and lost reports (an overflow,
    /// or a watch restarted after a gap) mark every watched project, because
    /// any of them may have changed. A watch that ended is started again
    /// after [`GIT_WATCH_RETRY`].
    fn collect(&mut self) {
        let now = Instant::now();
        let mut changed = Vec::new();
        if let Some(outgoing) = self.outgoing.as_mut() {
            while let Ok(report) = outgoing.reports.try_recv() {
                if let Ok(GitWatchReport::Changed { common_dirs }) = report {
                    changed.extend(common_dirs);
                }
            }
        }
        let Some(subscription) = self.subscription.as_mut() else {
            self.mark(&changed, now);
            return;
        };
        let mut ended = None;
        let mut every = false;
        while let Ok(report) = subscription.reports.try_recv() {
            match report {
                Ok(GitWatchReport::Watching { unwatched }) => {
                    subscription.watching = true;
                    every |= std::mem::take(&mut subscription.catch_up);
                    for (common, message) in unwatched {
                        crate::diagnostic!(serde_json::json!({
                            "component": "worktrees", "kind": "git_watch.project_failed",
                            "common_dir": common, "message": message
                        }));
                    }
                }
                Ok(GitWatchReport::Changed { common_dirs }) => changed.extend(common_dirs),
                Ok(GitWatchReport::Overflow { reason }) => {
                    crate::diagnostic!(serde_json::json!({
                        "component": "worktrees", "kind": "git_watch.overflow",
                        "message": reason, "projects": self.registered.len()
                    }));
                    every = true;
                }
                Ok(GitWatchReport::Quiet) => {}
                Err(reason) => ended = Some(reason),
            }
        }
        if subscription.watching {
            self.outgoing = None;
        }
        self.mark(&changed, now);
        if every {
            for project in self.registered.keys() {
                self.pending.insert(project.clone(), now);
            }
        }
        if let Some(reason) = ended {
            crate::diagnostic!(serde_json::json!({
                "component": "worktrees", "kind": "git_watch.ended", "message": reason
            }));
            self.subscription = None;
            self.retry_at = Some(now + GIT_WATCH_RETRY);
        }
    }

    /// Marks the projects of each changed common directory.
    fn mark(&mut self, common_dirs: &[String], now: Instant) {
        for common in common_dirs {
            for project in self.roots.get(Path::new(common)).into_iter().flatten() {
                self.pending.insert(project.clone(), now);
            }
        }
    }

    fn settled(&mut self) -> Vec<PathBuf> {
        self.collect();
        let now = Instant::now();
        let settled: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, last)| now.duration_since(**last) >= GIT_WATCH_DEBOUNCE)
            .map(|(project, _)| project.clone())
            .collect();
        for project in &settled {
            self.pending.remove(project);
        }
        settled
    }

    fn has_pending(&mut self) -> bool {
        self.collect();
        !self.pending.is_empty()
    }
}

pub struct WorktreeReader {
    inner: BackgroundRead<ObservedRequest, (ObservedRequest, Vec<ProjectAnswer>)>,
    git_watch: GitWatch,
    git_generations: BTreeMap<PathBuf, u64>,
}

impl WorktreeReader {
    /// Reads the repositories of the node `node` links to.
    pub fn new(node: std::sync::Arc<dyn crate::node_access::NodeLink>) -> Self {
        let git_watch = GitWatch::new(std::sync::Arc::clone(&node));
        Self {
            inner: BackgroundRead::on_change(Duration::ZERO, {
                let cache = std::sync::Mutex::new(ProjectCache::new());
                move |request: &ObservedRequest| read_observed(node.as_ref(), &cache, request)
            }),
            git_watch,
            git_generations: BTreeMap::new(),
        }
    }

    pub fn read_if_due(&mut self, request: WorktreeRequest) -> Option<WorktreeAnswer> {
        self.git_watch.reconcile(&request.projects);
        for project in self.git_watch.settled() {
            let generation = self.git_generations.entry(project).or_insert(0);
            *generation = generation.wrapping_add(1);
        }
        let observations = request
            .projects
            .iter()
            .map(|project| ProjectObservation {
                request: project.clone(),
                git_generation: self
                    .git_generations
                    .get(&project.root_path)
                    .copied()
                    .unwrap_or(0),
            })
            .collect();
        let observed_request = (request.generation, request.removals, observations);
        let (answered_request, answers) = self.inner.poll(observed_request.clone())?;
        let observations_current =
            answered_request == observed_request && !self.git_watch.has_pending();
        let mut projects: Vec<ProjectWorktreesSnapshot> = Vec::new();
        for (_, project) in answers {
            // Two registrations inside one repository describe one project.
            if let Some(project) = project
                && !projects
                    .iter()
                    .any(|existing| existing.root_path == project.root_path)
            {
                projects.push(project);
            }
        }
        self.git_generations
            .retain(|root, _| request.projects.iter().any(|p| &p.root_path == root));
        Some(WorktreeAnswer {
            catalog: WorktreeCatalogSnapshot { projects },
            removals: answered_request.1,
            observations_current,
            request: WorktreeRequest {
                projects: answered_request
                    .2
                    .into_iter()
                    .map(|observation| observation.request)
                    .collect(),
                generation: answered_request.0,
                removals: answered_request.1,
            },
        })
    }
}

/// The last answer per requested root, with the observation that produced it.
type ProjectCache = BTreeMap<PathBuf, (u64, ProjectObservation, Option<ProjectWorktreesSnapshot>)>;

/// The worker's read: every project whose observation moved is read again,
/// every other one is answered from the last read. The cache lives with the
/// worker because only the worker produces what it holds; a project that
/// leaves the request leaves the cache with it.
fn read_observed(
    node: &dyn crate::node_access::NodeLink,
    cache: &std::sync::Mutex<ProjectCache>,
    request: &ObservedRequest,
) -> (ObservedRequest, Vec<ProjectAnswer>) {
    let (generation, removals, observations) = request;
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.retain(|root, _| observations.iter().any(|o| &o.request.root_path == root));
    let answers = observations
        .iter()
        .map(|observation| {
            let root = observation.request.root_path.clone();
            if let Some((cached_generation, cached, project)) = cache.get(&root)
                && cached_generation == generation
                && cached == observation
            {
                return (root, project.clone());
            }
            let project = read(node, &observation.request);
            cache.insert(
                root.clone(),
                (*generation, observation.clone(), project.clone()),
            );
            (root, project)
        })
        .collect();
    ((*generation, *removals, observations.clone()), answers)
}

/// The sole deletion policy, consumed by all three presentation surfaces.
///
/// Main, locked or unmeasured worktrees are refused; other loss is a
/// warning the confirmation shows, and the loss itself is a choice the
/// operator makes there (`discard_label`, `can_delete_branch`).
pub fn deletion_gate(
    worktree: &WorktreeSnapshot,
    is_base: bool,
    pane_count: usize,
) -> WorktreeDeletionGateSnapshot {
    let blocked_reason = if worktree.is_main {
        Some("The main worktree cannot be deleted".to_owned())
    } else if let Some(reason) = worktree.lock_reason.as_deref() {
        let name = worktree
            .branch
            .as_deref()
            .or_else(|| {
                Path::new(&worktree.path)
                    .file_name()
                    .and_then(|name| name.to_str())
            })
            .unwrap_or("selected checkout");
        Some(hide_node_link::worktrees::locked_removal_reason(
            name, reason,
        ))
    } else {
        worktree.ignored_scan_unavailable.clone()
    };
    let status_unknown = worktree.unavailable_reason.is_some() && !worktree.missing;
    let files = |count: u32| match count {
        0 => "uncommitted changes".to_owned(),
        1 => "1 changed file".to_owned(),
        count => format!("{count} changed files"),
    };
    let mut warnings = Vec::new();
    if worktree.dirty {
        warnings.push(format!(
            "{} not committed",
            files(worktree.changed_file_count)
        ));
    }
    if worktree.nested {
        warnings.push("another worktree lies inside it".to_owned());
    }
    if is_base {
        warnings.push("holds the base branch".to_owned());
    }
    if status_unknown {
        warnings.push("Git status unavailable".to_owned());
    }
    if !worktree.missing {
        if worktree.merged != Some(true) {
            warnings.push(format!("ahead {} unmerged", worktree.ahead));
        }
        if worktree.upstream_state != "pushed" {
            warnings.push("not pushed".to_owned());
        }
    }
    let mut discard_label = match (worktree.dirty, worktree.nested) {
        (true, true) => Some(format!(
            "Discard {} and the worktree inside it",
            files(worktree.changed_file_count)
        )),
        (true, false) => Some(format!("Discard {}", files(worktree.changed_file_count))),
        (false, true) => Some("Discard the worktree inside it".to_owned()),
        (false, false) => status_unknown.then(|| "Discard any uncommitted changes".to_owned()),
    };
    if !worktree.ignored_repositories.is_empty() {
        let names = worktree.ignored_repositories.join(", ");
        warnings.push(format!(
            "{} ignored repositories",
            worktree.ignored_repositories.len()
        ));
        discard_label = Some(match discard_label {
            Some(label) => format!("{label} and ignored repositories {names}"),
            None => format!("Discard ignored repositories {names}"),
        });
    }
    let can_delete_branch = !worktree.missing && worktree.branch.is_some() && !is_base;
    let branch_warning = match worktree.merged {
        _ if !can_delete_branch => None,
        Some(true) => None,
        Some(false) if worktree.ahead == 0 => {
            Some("Git does not count it as merged; it is deleted anyway".to_owned())
        }
        Some(false) => Some(format!(
            "{} not on {} {} lost with it",
            if worktree.ahead == 1 {
                "1 commit".to_owned()
            } else {
                format!("{} commits", worktree.ahead)
            },
            worktree.base_branch.as_deref().unwrap_or("the base"),
            if worktree.ahead == 1 { "is" } else { "are" },
        )),
        None => Some("Git could not tell whether it is merged; it is deleted anyway".to_owned()),
    };
    WorktreeDeletionGateSnapshot {
        blocked_reason,
        warnings,
        button_label: if pane_count == 1 {
            "Close 1 pane and delete".to_owned()
        } else if pane_count > 0 {
            format!("Close {pane_count} panes and delete")
        } else {
            "Delete worktree…".to_owned()
        },
        can_delete_branch,
        branch_warning,
        discard_label,
    }
}

/// One project's rows: the node's Git facts with this machine's policy and
/// decoration slots on them, asked the way a device's repository is
/// (`Call::Worktrees`). A refusal is the project's unavailable reason.
fn read(
    node: &dyn crate::node_access::NodeLink,
    project: &WorktreeProjectRequest,
) -> Option<ProjectWorktreesSnapshot> {
    let unavailable = |reason: String| {
        Some(ProjectWorktreesSnapshot {
            root_path: project.root_path.to_string_lossy().into_owned(),
            unavailable_reason: Some(reason),
            ..Default::default()
        })
    };
    let Some(path) = project.root_path.to_str() else {
        return unavailable(format!(
            "Repository path is not valid UTF-8: {}",
            project.root_path.display()
        ));
    };
    match crate::node_access::call_as::<Option<hide_node_link::worktrees::RepositoryWorktrees>>(
        node,
        hide_node_link::protocol::Call::Worktrees {
            path: path.to_owned(),
            bases: project.bases.clone(),
            base_override: project.base_override.clone(),
        },
        WORKTREES_TIMEOUT,
    ) {
        Ok(facts) => facts.map(project_snapshot),
        Err(crate::node_access::LinkError::Refused(error)) => unavailable(error.message),
        Err(error) => unavailable(error.to_string()),
    }
}

/// How long one repository's read may take on its node.
const WORKTREES_TIMEOUT: Duration = Duration::from_secs(60);

#[cfg(test)]
fn read_project(
    root: &Path,
    root_path: String,
    bases: &BTreeMap<String, String>,
    base_override: Option<&str>,
) -> ProjectWorktreesSnapshot {
    project_snapshot(hide_host::worktrees::read_project(
        root,
        root_path,
        bases,
        base_override,
    ))
}

/// A repository's host facts as the snapshot's project entry, each row
/// carrying the deletion gate its facts decide. The gate's pane and agent
/// counts are filled when the catalog places the row.
pub fn project_snapshot(
    facts: hide_node_link::worktrees::RepositoryWorktrees,
) -> ProjectWorktreesSnapshot {
    if let Some(reason) = facts.unavailable_reason.as_deref() {
        crate::diagnostic!(serde_json::json!({
            "component": "worktrees",
            "kind": "worktree_list.failed",
            "project": facts.root_path,
            "message": reason,
        }));
    }
    let base_branch = facts.base_branch.clone();
    ProjectWorktreesSnapshot {
        shared_git_path: facts.shared_git_path,
        root_path: facts.root_path,
        default_branch: facts.default_branch,
        branches: facts.branches,
        worktrees: facts
            .worktrees
            .into_iter()
            .map(|row| {
                let mut worktree = worktree_snapshot(row);
                worktree.deletion_gate = deletion_gate(
                    &worktree,
                    worktree.branch == base_branch && base_branch.is_some(),
                    0,
                );
                worktree
            })
            .collect(),
        unavailable_reason: facts.unavailable_reason,
        base_branch: facts.base_branch,
        base_branch_fallback: facts.base_branch_fallback,
        base_source: facts.base_source,
        ..Default::default()
    }
}

fn worktree_snapshot(facts: hide_node_link::worktrees::WorktreeFacts) -> WorktreeSnapshot {
    if let Some(reason) = facts.unavailable_reason.as_deref() {
        crate::diagnostic!(serde_json::json!({
            "component": "worktrees",
            "kind": "worktree_status.unavailable",
            "path": facts.path,
            "message": reason,
        }));
    }
    WorktreeSnapshot {
        path: facts.path,
        branch: facts.branch,
        head_sha: facts.head_sha,
        missing: facts.missing,
        is_main: facts.is_main,
        lock_reason: facts.lock_reason,
        ignored_repositories: facts.ignored_repositories,
        ignored_scan_unavailable: facts.ignored_scan_unavailable,
        nested: facts.nested,
        dirty: facts.dirty,
        changed_file_count: facts.changed_file_count,
        base_branch: facts.base_branch,
        ahead: facts.ahead,
        behind: facts.behind,
        added_lines: facts.added_lines,
        removed_lines: facts.removed_lines,
        merged: facts.merged,
        upstream_state: facts.upstream_state,
        unpushed: facts.unpushed.map(|unpushed| UnpushedSnapshot {
            remote: unpushed.remote,
            count: unpushed.count,
        }),
        behind_upstream: facts.behind_upstream,
        created_at_unix_ms: facts.created_at_unix_ms,
        last_commit_unix_seconds: facts.last_commit_unix_seconds,
        last_commit_subject: facts.last_commit_subject,
        last_fetch_at_unix_ms: facts.last_fetch_at_unix_ms,
        measured_at_unix_ms: facts.measured_at_unix_ms,
        unavailable_reason: facts.unavailable_reason,
        ..WorktreeSnapshot::default()
    }
}

#[cfg(test)]
fn git_call_count(root: &Path, command: &str) -> usize {
    hide_host::worktrees::GIT_CALLS
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, cmd)| path == root && cmd == command)
        .count()
}

#[cfg(test)]
use hide_host::worktrees::output_within;

#[cfg(test)]
mod tests {
    use super::*;
    use hide_host::worktrees::{
        ListedWorktree, describe, parse_ahead_behind, parse_shortstat, parse_worktree_list,
        resolve_base,
    };

    #[test]
    fn porcelain_records_name_the_main_worktree_and_each_branch() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\n\
                      worktree /repo.worktrees/feature\nHEAD def\nbranch refs/heads/feature\n\n";
        let listed = parse_worktree_list(output);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].path, PathBuf::from("/repo"));
        assert_eq!(listed[0].branch.as_deref(), Some("main"));
        assert!(listed[0].is_main);
        assert_eq!(listed[1].branch.as_deref(), Some("feature"));
        assert!(!listed[1].is_main);
    }

    /// A detached head has no `branch` line. The record must still produce a
    /// worktree, because the folder is on disk and the row is about the
    /// folder.
    #[test]
    fn a_detached_worktree_is_listed_without_a_branch() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\n\
                      worktree /repo.worktrees/detached\nHEAD def\ndetached\n\n";
        let listed = parse_worktree_list(output);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[1].branch, None);
        assert_eq!(listed[1].path, PathBuf::from("/repo.worktrees/detached"));
    }

    /// The branch line belongs to the record it follows. Reading it into the
    /// next record would label every worktree with its predecessor's branch.
    #[test]
    fn a_branchless_record_does_not_inherit_the_previous_branch() {
        let output = "worktree /repo\nbranch refs/heads/main\n\n\
                      worktree /a\ndetached\n\n\
                      worktree /b\nbranch refs/heads/second\n\n";
        let listed = parse_worktree_list(output);
        assert_eq!(
            listed
                .iter()
                .map(|entry| entry.branch.as_deref())
                .collect::<Vec<_>>(),
            [Some("main"), None, Some("second")]
        );
    }

    #[test]
    fn left_right_counts_read_the_base_side_as_behind() {
        assert_eq!(parse_ahead_behind("0\t2\n"), (2, 0));
        assert_eq!(parse_ahead_behind("3\t1\n"), (1, 3));
        assert_eq!(parse_ahead_behind(""), (0, 0));
    }

    #[test]
    fn shortstat_reads_each_half_independently() {
        assert_eq!(
            parse_shortstat(" 3 files changed, 42 insertions(+), 7 deletions(-)\n"),
            (42, 7)
        );
        assert_eq!(
            parse_shortstat(" 1 file changed, 5 insertions(+)\n"),
            (5, 0)
        );
        assert_eq!(parse_shortstat(" 1 file changed, 9 deletions(-)\n"), (0, 9));
        assert_eq!(parse_shortstat(""), (0, 0));
    }

    /// A pull request's base wins over the repository default, a branch with
    /// no pull request falls back to the default, and the default branch's own
    /// worktree is compared with nothing.
    #[test]
    fn the_pull_request_base_overrides_the_repository_default() {
        let bases = BTreeMap::from([("feature".to_owned(), "release".to_owned())]);
        assert_eq!(
            resolve_base(Some("feature"), &bases, Some("main")).as_deref(),
            Some("release")
        );
        assert_eq!(
            resolve_base(Some("other"), &bases, Some("main")).as_deref(),
            Some("main")
        );
        assert_eq!(resolve_base(Some("main"), &bases, Some("main")), None);
        assert_eq!(resolve_base(Some("other"), &bases, None), None);
        assert_eq!(
            resolve_base(None, &bases, Some("main")).as_deref(),
            Some("main")
        );
    }

    /// A worktree git lists but disk does not have reports `missing` and no
    /// counts, so an absent checkout never reads as a clean one.
    #[test]
    fn a_missing_worktree_reports_no_counts() {
        let described = describe(
            ListedWorktree {
                path: PathBuf::from("/definitely/not/here/hide-test"),
                branch: Some("gone".to_owned()),
                is_main: false,
                bare: false,
                head_sha: None,
                lock_reason: None,
            },
            &BTreeMap::new(),
            Some("main"),
            None,
        );
        assert!(described.missing);
        assert!(!described.dirty);
        assert_eq!(described.ahead, 0);
        assert_eq!(described.behind, 0);
        assert_eq!(described.unpushed, None);
    }

    /// A node whose projects are each their own repository, and whose every
    /// `Call::GitWatch` forwards the reports the test sends on that watch's
    /// own channel, acknowledging each, until the test drops the sender.
    struct DrivenWatches {
        watches:
            std::sync::Mutex<std::sync::mpsc::Receiver<std::sync::mpsc::Receiver<GitWatchReport>>>,
        forwarded: std::sync::Mutex<std::sync::mpsc::Sender<()>>,
    }

    impl crate::node_access::NodeLink for DrivenWatches {
        fn call(
            &self,
            call: Call,
            timeout: Duration,
        ) -> Result<hide_node_link::link::LinkAnswer, hide_node_link::link::LinkError> {
            self.call_with_progress(call, timeout, &mut |_| true)
        }

        fn call_with_progress(
            &self,
            call: Call,
            _timeout: Duration,
            progress: &mut dyn FnMut(serde_json::Value) -> bool,
        ) -> Result<hide_node_link::link::LinkAnswer, hide_node_link::link::LinkError> {
            match call {
                Call::Repository { path } => Ok(serde_json::json!(RepositoryDirs {
                    root: path.clone(),
                    git_dir: format!("{path}/.git"),
                    common_dir: format!("{path}/.git"),
                })
                .into()),
                Call::GitWatch { .. } => {
                    let reports = self.watches.lock().unwrap().recv().unwrap();
                    let forwarded = self.forwarded.lock().unwrap().clone();
                    while let Ok(report) = reports.recv() {
                        let go_on = progress(serde_json::to_value(report).unwrap());
                        forwarded.send(()).unwrap();
                        if !go_on {
                            break;
                        }
                    }
                    Ok(serde_json::Value::Null.into())
                }
                other => panic!("unexpected call {other:?}"),
            }
        }
    }

    struct Driver {
        watch: GitWatch,
        watches: std::sync::mpsc::Sender<std::sync::mpsc::Receiver<GitWatchReport>>,
        forwarded: std::sync::mpsc::Receiver<()>,
    }

    impl Driver {
        fn new() -> Self {
            let (watches, waiting) = std::sync::mpsc::channel();
            let (acknowledge, forwarded) = std::sync::mpsc::channel();
            let node = DrivenWatches {
                watches: std::sync::Mutex::new(waiting),
                forwarded: std::sync::Mutex::new(acknowledge),
            };
            Self {
                watch: GitWatch::new(std::sync::Arc::new(node)),
                watches,
                forwarded,
            }
        }

        /// The sender of the next watch the node starts.
        fn next_watch(&self) -> std::sync::mpsc::Sender<GitWatchReport> {
            let (sender, reports) = std::sync::mpsc::channel();
            self.watches.send(reports).unwrap();
            sender
        }

        /// Sends `report` on `on` and collects it once the node forwarded it.
        fn report(&mut self, on: &std::sync::mpsc::Sender<GitWatchReport>, report: GitWatchReport) {
            on.send(report).unwrap();
            self.forwarded
                .recv_timeout(Duration::from_secs(5))
                .expect("the node forwarded the report");
            self.watch.collect();
        }
    }

    fn projects(roots: &[&str]) -> Vec<WorktreeProjectRequest> {
        roots
            .iter()
            .map(|root| WorktreeProjectRequest {
                root_path: PathBuf::from(root),
                bases: BTreeMap::new(),
                base_override: None,
                generation: 0,
            })
            .collect()
    }

    fn watching() -> GitWatchReport {
        GitWatchReport::Watching {
            unwatched: Vec::new(),
        }
    }

    /// A watch that ended may have missed any change, so once its restart is
    /// watching every project is read again; the first watch reads nothing.
    #[test]
    fn a_git_watch_restarted_after_it_ended_rereads_every_project() {
        let mut driver = Driver::new();
        let first = driver.next_watch();
        let both = projects(&["/a", "/b"]);
        driver.watch.reconcile(&both);
        driver.report(&first, watching());
        assert!(driver.watch.pending.is_empty());
        drop(first);
        let deadline = Instant::now() + Duration::from_secs(5);
        while driver.watch.subscription.is_some() {
            assert!(
                Instant::now() < deadline,
                "the ended watch was never collected"
            );
            std::thread::yield_now();
            driver.watch.collect();
        }
        assert!(driver.watch.retry_at.is_some());

        driver.watch.retry_at = Some(Instant::now());
        let second = driver.next_watch();
        driver.watch.reconcile(&both);
        assert!(
            driver.watch.pending.is_empty(),
            "nothing is read before the watch is up"
        );
        driver.report(&second, watching());
        assert_eq!(
            driver.watch.pending.keys().cloned().collect::<Vec<_>>(),
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    /// Adding a project starts a new watch, and the old one keeps reporting
    /// until the new one is watching, so a change in between is not lost and
    /// nothing is read again for the swap itself.
    #[test]
    fn a_change_while_the_watched_set_changes_is_kept() {
        let mut driver = Driver::new();
        let first = driver.next_watch();
        driver.watch.reconcile(&projects(&["/a"]));
        driver.report(&first, watching());
        let second = driver.next_watch();
        driver.watch.reconcile(&projects(&["/a", "/b"]));
        driver.report(
            &first,
            GitWatchReport::Changed {
                common_dirs: vec!["/a/.git".to_owned()],
            },
        );
        assert_eq!(
            driver.watch.pending.keys().cloned().collect::<Vec<_>>(),
            vec![PathBuf::from("/a")]
        );
        driver.watch.pending.clear();
        driver.report(&second, watching());
        assert!(driver.watch.outgoing.is_none());
        assert!(driver.watch.pending.is_empty());
    }
}

#[cfg(test)]
#[path = "worktree_behavior_tests.rs"]
mod behavior_tests;
