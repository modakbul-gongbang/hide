//! Reviewed cleanup of a project's disk: the build caches and dependencies a
//! tool makes again, and clean linked worktrees merged into local main.
//! All inspection and filesystem effects run on the existing action-worker
//! path, never under the runtime lock.
use super::{LiveContext, control_request};
use crate::disk_layers::{Layer, LayerFolder, verify_folder};
use crate::model::ListeningPortSnapshot;
use crate::{disk, github, worktrees::git};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CleanupSnapshot {
    pub id: u64,
    pub workspace_id: String,
    pub repository_root: String,
    pub phase: String,
    pub main_head: Option<String>,
    pub rows: Vec<CleanupRow>,
    pub message: Option<String>,
    /// Why "in use" could not be read (no Herdr connection, a pane or port
    /// read failed). While it stands nothing may be chosen.
    pub usage_error: Option<String>,
    /// The in-use reads are in, so build caches and dependencies may be chosen
    /// while worktree eligibility is still being checked.
    pub usage_ready: bool,
    pub progress: Option<CleanupProgress>,
    pub cell_results: Vec<CellResult>,
    /// The project volume's free bytes when the review read them, before the
    /// removal started, and after the removed folders were deleted.
    pub free_bytes: Option<u64>,
    pub free_before: Option<u64>,
    pub free_after: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CleanupProgress {
    pub done: usize,
    pub total: usize,
}

/// Why a checkout cannot be emptied while it is being worked in.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InUse {
    /// `agent_working`, `process`, `port`, or `unverified` for a checkout the
    /// runtime has no facts about, which is never read as idle.
    pub code: &'static str,
    pub name: Option<String>,
    pub port: Option<u16>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CleanupRow {
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub is_main: bool,
    /// The reason the worktree may not be removed, in words for the log
    /// only; the shell reads `exclusion_code`.
    #[serde(skip)]
    pub exclusion: Option<String>,
    pub exclusion_code: Option<&'static str>,
    pub exclusion_count: Option<u32>,
    pub in_use: Option<InUse>,
    /// `removed`, `skipped` (a recheck found it changed or busy) or `failed`
    /// (Git refused the removal), once a confirmation has settled the row.
    pub result: Option<String>,
    /// Why a `skipped` or `failed` row was not removed: an exclusion code,
    /// `changed`, `not_found`, `unverified`, or `remove_refused`.
    pub result_code: Option<&'static str>,
    /// The allocated bytes of the worktree this run removed.
    pub bytes: Option<u64>,
    /// The words behind `result_code`, for the log only.
    #[serde(skip)]
    pub message: Option<String>,
}

/// What became of one chosen build-cache or dependency cell.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CellResult {
    pub path: String,
    pub layer: &'static str,
    /// `removed`, `skipped` or `failed`.
    pub outcome: &'static str,
    pub bytes: u64,
    pub folders: usize,
    pub reason_code: Option<&'static str>,
    /// The words behind `reason_code`, for the log only.
    #[serde(skip)]
    pub reason: Option<String>,
}

/// What the runtime knows about one checkout, copied under the lock for the
/// worker's in-use read.
#[derive(Clone, Debug)]
pub(crate) struct CheckoutFacts {
    pub path: PathBuf,
    /// Agents of the checkout whose activity is Working.
    pub agent_working: usize,
    /// Panes that are not an agent's; their foreground process is read.
    pub terminal_panes: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReviewInput {
    pub workspace_id: String,
    pub root: PathBuf,
    /// The checkout the operator is looking at, when it is in this project.
    pub current: Option<PathBuf>,
    pub checkouts: Vec<CheckoutFacts>,
    /// The folders each checkout's layer cells were measured from.
    pub folders: HashMap<PathBuf, Vec<LayerFolder>>,
    /// Each worktree's measured allocated bytes, for the result's totals.
    pub bytes: HashMap<PathBuf, u64>,
}

/// A build cache or dependency cell the operator chose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CellChoice {
    pub path: String,
    pub layer: Layer,
}

/// How many terminal panes one review asks Herdr about. A project past it
/// cannot be judged, and says so instead of guessing.
const PANE_READ_LIMIT: usize = 256;

pub(crate) type InUseMap = HashMap<PathBuf, InUse>;

/// Which checkouts are being worked in: an agent is Working, a program other
/// than a shell holds a terminal pane of the checkout, or a process started
/// inside it listens on a port. `process` answers a pane's program, and any
/// read that fails fails the whole answer: an unread checkout is not idle.
pub(crate) fn read_in_use(
    checkouts: &[CheckoutFacts],
    ports: Result<Vec<ListeningPortSnapshot>, String>,
    mut process: impl FnMut(&str) -> Result<Option<String>, String>,
) -> Result<InUseMap, String> {
    let ports = ports?;
    let mut in_use = InUseMap::new();
    let mut reads = 0usize;
    for checkout in checkouts {
        let path = hide_platform::fs::identity::canonical(&checkout.path)
            .unwrap_or_else(|_| checkout.path.clone());
        if checkout.agent_working > 0 {
            in_use.insert(
                checkout.path.clone(),
                InUse {
                    code: "agent_working",
                    name: None,
                    port: None,
                },
            );
            continue;
        }
        if let Some(port) = ports
            .iter()
            .filter(|listener| Path::new(&listener.cwd).starts_with(&path))
            .map(|listener| listener.port)
            .min()
        {
            in_use.insert(
                checkout.path.clone(),
                InUse {
                    code: "port",
                    name: None,
                    port: Some(port),
                },
            );
            continue;
        }
        for pane in &checkout.terminal_panes {
            reads += 1;
            if reads > PANE_READ_LIMIT {
                return Err(format!(
                    "More than {PANE_READ_LIMIT} terminal panes are open, too many to check for running programs"
                ));
            }
            if let Some(name) = process(pane)? {
                in_use.insert(
                    checkout.path.clone(),
                    InUse {
                        code: "process",
                        name: Some(name),
                        port: None,
                    },
                );
                break;
            }
        }
    }
    Ok(in_use)
}

/// The in-use read against the live Herdr and the machine's listening ports.
fn live_in_use(context: &LiveContext, checkouts: &[CheckoutFacts]) -> Result<InUseMap, String> {
    let ports = crate::ports::read_now();
    let ports = match ports.unavailable_reason {
        Some(reason) => Err(format!("Listening ports could not be read: {reason}")),
        None => Ok(ports.entries),
    };
    read_in_use(checkouts, ports, |pane| {
        let value = control_request(
            context.api_connector.as_ref(),
            "pane.process_info",
            crate::wire::pane_process_info_params(pane)?,
        )?;
        let group = crate::wire::pane_process_group(value)?;
        if group.foreground_pids.is_empty() || group.shell_holds_terminal() {
            return Ok(None);
        }
        Ok(Some(
            group
                .foreground_program()
                .or_else(|| group.foreground_names.first().map(String::as_str))
                .unwrap_or("a program")
                .to_owned(),
        ))
    })
}

fn canonical(path: &Path) -> PathBuf {
    hide_platform::fs::identity::canonical(path).unwrap_or_else(|_| path.to_path_buf())
}

fn unverified_use() -> InUse {
    InUse {
        code: "unverified",
        name: None,
        port: None,
    }
}

/// A registered worktree the runtime has no facts about cannot be shown to
/// be idle, so it reads as unverified and nothing in it is emptied.
fn mark_unverified(
    in_use: &mut InUseMap,
    facts: &[CheckoutFacts],
    registered: impl IntoIterator<Item = PathBuf>,
) {
    let known: std::collections::HashSet<PathBuf> =
        facts.iter().map(|facts| canonical(&facts.path)).collect();
    for path in registered {
        if !known.contains(&canonical(&path)) {
            in_use.entry(path).or_insert_with(unverified_use);
        }
    }
}

/// Whether one checkout is in use right now: the facts are copied again
/// under the lock and the checkout's panes and ports are read again, so a
/// decision is never older than the move it guards. A checkout without facts
/// is unverified.
fn read_in_use_now(
    path: &Path,
    facts: impl FnOnce() -> Option<Vec<CheckoutFacts>>,
    read: impl FnOnce(&[CheckoutFacts]) -> Result<InUseMap, String>,
) -> Result<Option<InUse>, String> {
    let facts = facts().ok_or("The project is no longer available")?;
    let key = canonical(path);
    let Some(checkout) = facts
        .into_iter()
        .find(|facts| canonical(&facts.path) == key)
    else {
        return Ok(Some(unverified_use()));
    };
    let map = read(std::slice::from_ref(&checkout))?;
    Ok(map.get(&checkout.path).cloned())
}

fn fresh_in_use(
    context: &LiveContext,
    workspace_id: &str,
    path: &Path,
) -> Result<Option<InUse>, String> {
    read_in_use_now(
        path,
        || {
            let runtime = context.runtime.upgrade()?;
            let guard = runtime.lock().ok()?;
            guard.cleanup_facts(workspace_id)
        },
        |facts| live_in_use(context, facts),
    )
}

/// A worktree that may not be removed, and why.
struct Exclusion {
    code: &'static str,
    count: Option<u32>,
    message: String,
}

impl Exclusion {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            count: None,
            message: message.into(),
        }
    }
}

/// A Git or filesystem read that failed leaves the worktree unverified.
impl From<String> for Exclusion {
    fn from(message: String) -> Self {
        Self::new("unverified", message)
    }
}

/// The review's rows: Git's registration with the reason each one is not a
/// candidate.
fn listed(root: &Path) -> Result<Vec<CleanupRow>, String> {
    Ok(hide_host::worktrees::registered(root)?
        .into_iter()
        .enumerate()
        .map(|(index, registered)| {
            let (code, message) = if index == 0 {
                (Some("main"), Some("Main checkout"))
            } else if registered.locked {
                (
                    Some("locked"),
                    Some("Worktree is locked. Unlock it separately before reviewing again."),
                )
            } else if registered.unavailable {
                (Some("unavailable"), Some("Worktree is unavailable"))
            } else {
                (None, None)
            };
            CleanupRow {
                exclusion: message.map(Into::into),
                exclusion_code: code,
                is_main: index == 0,
                path: registered.path,
                branch: registered.branch,
                head: registered.head,
                ..Default::default()
            }
        })
        .collect())
}

fn pane_paths(context: &LiveContext) -> Result<Vec<PathBuf>, String> {
    let response = control_request(
        context.api_connector.as_ref(),
        "session.snapshot",
        serde_json::json!({}),
    )?;
    verified_pane_paths(
        crate::wire::cleanup_usage_paths(response).map_err(|e| e.message().to_owned())?,
    )
}

fn verified_pane_paths(paths: Vec<Option<String>>) -> Result<Vec<PathBuf>, String> {
    paths
        .into_iter()
        .map(|cwd| {
            let cwd = cwd.ok_or("A live pane's folder is unknown. Resolve it before cleanup.")?;
            let path = PathBuf::from(cwd);
            if !path.is_absolute()
                || path.components().any(|component| {
                    matches!(
                        component,
                        std::path::Component::CurDir | std::path::Component::ParentDir
                    )
                })
            {
                return Err(
                    "A live pane's folder is not an absolute normalized path. Resolve it before cleanup."
                        .into(),
                );
            }
            // A pane can outlive the worktree folder it launched in. Keep its
            // absolute path for component-wise ownership checks instead of
            // letting that one stale folder disable cleanup for every current
            // worktree. Existing paths still resolve aliases before matching.
            Ok(hide_platform::fs::identity::canonical(&path).unwrap_or(path))
        })
        .collect()
}

fn verified_merge(
    root: &Path,
    main: &str,
    branch: &str,
    head: &str,
    merged_proofs: &mut impl FnMut(&str) -> Result<Vec<github::MergedPullRequestProof>, String>,
) -> Result<(), Exclusion> {
    let unverified = |message: &str| Exclusion::new("merge_unverified", message);
    let range = format!("{main}..{head}");
    let count = git(root, &["rev-list", "--count", &range, "--"])?;
    match count.trim().parse::<u64>() {
        Ok(0) => return Ok(()),
        Err(_) => return Err(unverified("Merge status could not be verified")),
        Ok(_) => {}
    }

    let proofs = merged_proofs(branch)?;
    let Some(proof) = proofs.iter().find(|proof| proof.head_oid == head) else {
        return Err(Exclusion::new(
            "not_merged",
            "Not merged into local main, and no merged pull request exactly matches this worktree HEAD",
        ));
    };
    let merge_oid = proof.merge_oid.as_deref().ok_or_else(|| {
        unverified("The matching merged pull request has no merge commit identity")
    })?;
    git(root, &["merge-base", "--is-ancestor", merge_oid, main]).map_err(|_| {
        unverified(
            "The matching pull request is merged on GitHub but its merge commit is not in local main. Update main and review again.",
        )
    })?;
    Ok(())
}

/// The registered worktrees with the reason each may not be removed. `only`
/// limits the checks to those paths, so a confirmation rereads what it is
/// about to remove and nothing else.
fn inspect(
    root: &Path,
    current: Option<&Path>,
    panes: Result<Vec<PathBuf>, String>,
    in_use: &InUseMap,
    only: Option<&[String]>,
) -> Result<CleanupSnapshot, String> {
    inspect_with_merge_proofs(root, current, panes, in_use, only, |branch| {
        github::merged_pull_request_proofs(root, branch)
    })
}

fn inspect_with_merge_proofs(
    root: &Path,
    current: Option<&Path>,
    panes: Result<Vec<PathBuf>, String>,
    in_use: &InUseMap,
    only: Option<&[String]>,
    mut merged_proofs: impl FnMut(&str) -> Result<Vec<github::MergedPullRequestProof>, String>,
) -> Result<CleanupSnapshot, String> {
    let mut rows = listed(root)?;
    let main_head = git(root, &["rev-parse", "--verify", "refs/heads/main^{commit}"])
        .map(|v| v.trim().to_owned());
    let canonical_current =
        current.and_then(|current| hide_platform::fs::identity::canonical(current).ok());
    let paths: Vec<PathBuf> = rows.iter().map(|r| PathBuf::from(&r.path)).collect();
    for row in &mut rows {
        row.in_use = in_use.get(Path::new(&row.path)).cloned();
        if row.exclusion.is_some() || only.is_some_and(|only| !only.contains(&row.path)) {
            continue;
        }
        let checked = (|| -> Result<(), Exclusion> {
            let path = Path::new(&row.path);
            let canonical = hide_platform::fs::identity::canonical(path)
                .map_err(|_| Exclusion::new("missing", "Folder is missing or unreadable"))?;
            if canonical != path {
                return Err(Exclusion::new(
                    "alias",
                    "Folder is an alias. Refresh authoritative worktree paths.",
                ));
            }
            if canonical_current.as_ref() == Some(&canonical) {
                return Err(Exclusion::new("current", "Current checkout"));
            }
            // Removing an ancestor would also remove another checkout's files.
            if paths
                .iter()
                .any(|other| other != path && other.starts_with(path))
            {
                return Err(Exclusion::new(
                    "contains_worktree",
                    "Contains another registered worktree",
                ));
            }
            if row.in_use.is_some() {
                return Err(Exclusion::new(
                    "in_use",
                    "In use: an agent is working or a program is running here.",
                ));
            }
            let panes = panes.as_ref().map_err(Clone::clone)?;
            if panes.iter().any(|cwd| cwd.starts_with(&canonical)) {
                return Err(Exclusion::new(
                    "pane_open",
                    "In use by a live pane or agent. Close or move it, then review again.",
                ));
            }
            let main = main_head.as_ref().map_err(|_| Exclusion::new("main_unavailable", "Local main is unavailable. Fetching or another base cannot establish eligibility."))?;
            let head = row
                .head
                .as_ref()
                .ok_or_else(|| Exclusion::new("unverified", "Worktree HEAD is unknown"))?;
            if row.branch.is_none() {
                return Err(Exclusion::new(
                    "detached",
                    "Detached HEAD. Review this worktree separately.",
                ));
            }
            let changed = git(path, &["status", "--porcelain=v1", "--untracked-files=all"])?
                .lines()
                .count();
            if changed > 0 {
                return Err(Exclusion {
                    code: "dirty",
                    count: Some(u32::try_from(changed).unwrap_or(u32::MAX)),
                    message:
                        "Uncommitted or untracked files. Commit or move them, then review again."
                            .into(),
                });
            }
            verified_merge(
                root,
                main,
                row.branch.as_deref().expect("branch checked above"),
                head,
                &mut merged_proofs,
            )?;
            // Removing the worktree deletes its ignored folders too, and a
            // repository cloned into one is lost with it.
            match hide_host::worktrees::ignored_repository(path)? {
                Some(folder) => Err(Exclusion::new(
                    "nested_repository",
                    format!("The ignored folder {folder} holds its own Git repository"),
                )),
                None => Ok(()),
            }
        })();
        if let Err(exclusion) = checked {
            row.exclusion = Some(exclusion.message);
            row.exclusion_code = Some(exclusion.code);
            row.exclusion_count = exclusion.count;
        }
    }
    Ok(CleanupSnapshot {
        repository_root: hide_platform::path::to_wire_lossy(root),
        main_head: main_head.ok(),
        rows,
        phase: "review".into(),
        usage_ready: true,
        ..Default::default()
    })
}

/// Why a chosen worktree was not removed, as a code the shell reads and the
/// words for the log.
struct Refusal {
    outcome: &'static str,
    code: &'static str,
    message: String,
}

impl Refusal {
    fn skipped(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            outcome: "skipped",
            code,
            message: message.into(),
        }
    }
}

/// Removes each chosen worktree that a fresh look (`refresh`, made for that
/// one worktree just before its removal) still finds eligible and idle.
/// Each row ends `removed`, `skipped` (a recheck found it changed, busy or
/// gone) or `failed` (Git refused the removal).
fn confirm(
    review: &CleanupSnapshot,
    selected: &[String],
    mut refresh: impl FnMut(&str) -> Result<CleanupSnapshot, String>,
    mut remove: impl FnMut(&str) -> Result<String, String>,
    mut settled: impl FnMut(&CleanupRow),
) -> CleanupSnapshot {
    let mut result = review.clone();
    result.phase = "complete".into();
    for row in &mut result.rows {
        if !selected.contains(&row.path) || row.result.as_deref() == Some("removed") {
            continue;
        }
        let validation = (|| -> Result<bool, Refusal> {
            if let Some(reason) = &row.exclusion {
                return Err(Refusal::skipped(
                    row.exclusion_code.unwrap_or("changed"),
                    format!("Excluded: {reason}"),
                ));
            }
            let fresh =
                refresh(&row.path).map_err(|message| Refusal::skipped("unverified", message))?;
            let Some(now) = fresh.rows.iter().find(|r| r.path == row.path) else {
                if !Path::new(&row.path)
                    .try_exists()
                    .map_err(|e| Refusal::skipped("unverified", e.to_string()))?
                {
                    return Err(Refusal::skipped(
                        "not_found",
                        "Worktree was already removed. Branch and Git history stay.",
                    ));
                }
                return Err(Refusal::skipped(
                    "changed",
                    "Registration changed but the folder remains. Review again; no files were removed.",
                ));
            };
            if fresh.main_head != review.main_head
                || now.head != row.head
                || now.branch != row.branch
                || now.exclusion.is_some()
            {
                return Err(Refusal::skipped(
                    now.exclusion_code.unwrap_or("changed"),
                    format!(
                        "State changed. {} Review again before removing.",
                        now.exclusion
                            .as_deref()
                            .unwrap_or("The reviewed branch or main HEAD moved.")
                    ),
                ));
            }
            Ok(true)
        })();
        match validation.map(|_| remove(&row.path)) {
            Ok(Ok(message)) => {
                row.result = Some("removed".into());
                row.message = Some(message);
            }
            Ok(Err(message)) => {
                row.result = Some("failed".into());
                row.result_code = Some("remove_refused");
                row.message = Some(message);
            }
            Err(refusal) => {
                row.result = Some(refusal.outcome.into());
                row.result_code = Some(refusal.code);
                row.message = Some(refusal.message);
            }
        }
        settled(row);
    }
    result
}

use hide_host::worktrees::remove_worktree;

/// Hands a snapshot to the runtime, which keeps it only while its id is the
/// live one. A runtime that cannot be locked is logged, never skipped quietly.
fn publish(context: &LiveContext, snapshot: CleanupSnapshot) {
    let id = snapshot.id;
    if let Some(runtime) = context.runtime.upgrade() {
        match runtime.lock() {
            Ok(mut guard) => {
                guard.ingest_cleanup(snapshot);
            }
            Err(_) => crate::diagnostic!(serde_json::json!({
                "component": "cleanup",
                "kind": "publish.lock_poisoned",
                "cleanup_id": id,
            })),
        }
        context.notifier.notify();
    }
}

/// Marks the worker done, so the lane is free again.
fn worker_finished(context: &LiveContext, id: u64) {
    if let Some(runtime) = context.runtime.upgrade() {
        match runtime.lock() {
            Ok(mut guard) => guard.cleanup_worker_finished(id),
            Err(_) => crate::diagnostic!(serde_json::json!({
                "component": "cleanup",
                "kind": "worker_finished.lock_poisoned",
                "cleanup_id": id,
            })),
        }
    }
}

/// Owns a worker's place in the lane: while it lives the daemon accepts no
/// other review or dismissal, and however the worker ends - it returns or it
/// panics - the lane is freed, and a worker that did not reach its own final
/// snapshot leaves a terminal one behind instead of a review stuck loading.
struct WorkerGuard {
    publish: Box<dyn Fn(CleanupSnapshot) + Send>,
    finished: Box<dyn Fn() + Send>,
    last: CleanupSnapshot,
    terminal_phase: &'static str,
    armed: bool,
}

impl WorkerGuard {
    fn new(
        publish: Box<dyn Fn(CleanupSnapshot) + Send>,
        finished: Box<dyn Fn() + Send>,
        start: CleanupSnapshot,
        terminal_phase: &'static str,
    ) -> Self {
        Self {
            publish,
            finished,
            last: start,
            terminal_phase,
            armed: true,
        }
    }

    fn for_context(
        context: &LiveContext,
        start: CleanupSnapshot,
        terminal_phase: &'static str,
    ) -> Self {
        let (a, b, id) = (context.clone(), context.clone(), start.id);
        Self::new(
            Box::new(move |snapshot| publish(&a, snapshot)),
            Box::new(move || worker_finished(&b, id)),
            start,
            terminal_phase,
        )
    }

    /// A step of the run, remembered so an unwind can say how far it got.
    fn publish(&mut self, snapshot: CleanupSnapshot) {
        self.last = snapshot.clone();
        (self.publish)(snapshot);
    }

    /// The run's own final snapshot.
    fn finish(mut self, snapshot: CleanupSnapshot) {
        self.armed = false;
        self.last = snapshot.clone();
        (self.publish)(snapshot);
    }
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if self.armed {
            let mut last = self.last.clone();
            last.phase = self.terminal_phase.into();
            last.message = Some("The cleanup worker stopped before it finished".into());
            crate::diagnostic!(serde_json::json!({
                "component": "cleanup",
                "kind": "worker.stopped_early",
                "cleanup_id": last.id,
                "phase": self.terminal_phase,
            }));
            (self.publish)(last);
        }
        (self.finished)();
    }
}

/// The review worker: reads what is in use and publishes the rows with it at
/// once, so build caches can be chosen, then checks each worktree's
/// eligibility (Git, and GitHub for a squash merge) and publishes the answer.
pub fn spawn_review(context: LiveContext, id: u64, input: ReviewInput) -> Result<(), String> {
    std::thread::Builder::new()
        .name("hide-worktree-cleanup".into())
        .spawn(move || {
            let base = CleanupSnapshot {
                id,
                workspace_id: input.workspace_id.clone(),
                repository_root: hide_platform::path::to_wire_lossy(&input.root),
                phase: "loading".into(),
                free_bytes: disk::volume_free_bytes(&input.root),
                ..Default::default()
            };
            let mut guard = WorkerGuard::for_context(&context, base.clone(), "failed");
            let usage = live_in_use(&context, &input.checkouts);
            let rows = listed(&input.root);
            let (in_use, usage_error) = match (usage, &rows) {
                (Ok(mut map), Ok(rows)) => {
                    mark_unverified(
                        &mut map,
                        &input.checkouts,
                        rows.iter().map(|row| PathBuf::from(&row.path)),
                    );
                    (map, None)
                }
                (Ok(map), Err(_)) => (map, None),
                (Err(message), _) => (InUseMap::new(), Some(message)),
            };
            match rows {
                Ok(mut rows) => {
                    for row in &mut rows {
                        row.in_use = in_use.get(Path::new(&row.path)).cloned();
                    }
                    guard.publish(CleanupSnapshot {
                        rows,
                        usage_ready: usage_error.is_none(),
                        usage_error: usage_error.clone(),
                        ..base.clone()
                    });
                }
                Err(message) => {
                    guard.finish(CleanupSnapshot {
                        phase: "failed".into(),
                        message: Some(message),
                        ..base
                    });
                    return;
                }
            }
            // Without the in-use reads a worktree's eligibility cannot be
            // decided, so each one is unverified and none can be chosen.
            let panes = match &usage_error {
                None => pane_paths(&context),
                Some(message) => Err(message.clone()),
            };
            let answer = match inspect(&input.root, input.current.as_deref(), panes, &in_use, None)
            {
                Ok(value) => CleanupSnapshot {
                    id,
                    workspace_id: input.workspace_id.clone(),
                    usage_error,
                    free_bytes: base.free_bytes,
                    ..value
                },
                Err(message) => CleanupSnapshot {
                    phase: "failed".into(),
                    message: Some(message),
                    usage_error,
                    ..base
                },
            };
            guard.finish(answer);
        })
        .map(|_| ())
        .map_err(|e| format!("Cleanup worker could not start: {e}"))
}

/// What emptying cells needs besides the cells themselves.
struct CellRun<'a> {
    id: u64,
    workspace_id: &'a str,
    /// The repository's shared Git directory, where removed folders wait.
    common: &'a Path,
    folders: &'a HashMap<PathBuf, Vec<LayerFolder>>,
}

/// A folder that cannot be moved, and the code the shell reads for it.
struct Kept {
    code: &'static str,
    reason: String,
}

fn cell_result(
    path: &str,
    layer: Layer,
    outcome: &'static str,
    bytes: u64,
    folders: usize,
    kept: Option<Kept>,
) -> CellResult {
    CellResult {
        path: path.to_owned(),
        layer: layer.code(),
        outcome,
        bytes,
        folders,
        reason_code: kept.as_ref().map(|kept| kept.code),
        reason: kept.map(|kept| kept.reason),
    }
}

/// Empties the chosen cells: right before each move the folder is judged
/// again from the files, and nothing moves unless it is still ignored, still
/// vouched for as its layer, reached through no link, holding no repository
/// and no file Git tracks (one `git ls-files` per checkout), and the
/// checkout is read as idle again (`in_use_now`) after all of that. A moved
/// folder leaves its checkout at once and is deleted afterwards; nothing here
/// deletes. Every outcome is one result and one diagnostic, and the entries
/// this run put in the trash are added to `moved`.
fn empty_cells(
    run: &CellRun,
    cells: &[CellChoice],
    mut in_use_now: impl FnMut(&Path) -> Result<Option<InUse>, String>,
    moved: &mut std::collections::BTreeSet<PathBuf>,
    mut settled: impl FnMut(&CellResult),
) -> Vec<CellResult> {
    let mut results = Vec::new();
    let mut checkouts: Vec<&str> = Vec::new();
    for cell in cells {
        if !checkouts.contains(&cell.path.as_str()) {
            checkouts.push(&cell.path);
        }
    }
    // One look through folders for repositories for the whole run.
    let mut walk = hide_host::worktrees::WalkBudget::for_run();
    let mut record = |result: CellResult| {
        crate::diagnostic!(serde_json::json!({
            "component": "cleanup",
            "kind": format!("cell.{}", result.outcome),
            "cleanup_id": run.id,
            "workspace_id": run.workspace_id,
            "checkout": result.path,
            "layer": result.layer,
            "bytes": result.bytes,
            "folders": result.folders,
            "reason_code": result.reason_code,
            "reason": result.reason,
        }));
        settled(&result);
        results.push(result);
    };
    for checkout in checkouts {
        let root = PathBuf::from(checkout);
        let chosen: Vec<Layer> = cells
            .iter()
            .filter(|cell| cell.path == checkout)
            .map(|cell| cell.layer)
            .collect();
        let exclude_dir = crate::git_dir::discover(&root).map(|repo| repo.common_dir.join("info"));
        // Judge every folder of every chosen layer, then ask Git once.
        let mut judged: Vec<(Layer, &LayerFolder, Option<Kept>)> = Vec::new();
        for layer in &chosen {
            for folder in run
                .folders
                .get(&root)
                .into_iter()
                .flatten()
                .filter(|folder| folder.layer == *layer)
            {
                let kept = match verify_folder(&root, &folder.path, *layer, exclude_dir.as_deref())
                {
                    Err(refusal) => Some(Kept {
                        code: refusal.code(),
                        reason: format!(
                            "The folder is no longer what was measured ({})",
                            refusal.code()
                        ),
                    }),
                    Ok(()) => {
                        match hide_host::worktrees::contains_repository(&folder.path, &mut walk) {
                            Ok(true) => Some(Kept {
                                code: "nested_repository",
                                reason: "The folder holds another Git repository".into(),
                            }),
                            Ok(false) => None,
                            Err(message) => Some(Kept {
                                code: "unverified",
                                reason: message,
                            }),
                        }
                    }
                };
                judged.push((*layer, folder, kept));
            }
        }
        let tracked = tracked_folders(
            &root,
            judged
                .iter()
                .filter(|(_, _, kept)| kept.is_none())
                .map(|(_, folder, _)| folder.path.as_path()),
        );
        for (_, folder, kept) in &mut judged {
            if kept.is_some() {
                continue;
            }
            match &tracked {
                Err(message) => {
                    *kept = Some(Kept {
                        code: "unverified",
                        reason: message.clone(),
                    });
                }
                Ok(tracked) if tracked.contains(&folder.path) => {
                    *kept = Some(Kept {
                        code: "tracked_files",
                        reason: "Git tracks a file inside the folder".into(),
                    });
                }
                Ok(_) => {}
            }
        }
        // The last look, right before the moves.
        let busy = match in_use_now(&root) {
            Err(message) => Some(Kept {
                code: "unverified",
                reason: message,
            }),
            Ok(Some(use_)) if use_.code == "unverified" => Some(Kept {
                code: "unverified",
                reason: "Nothing is known about the checkout's panes".into(),
            }),
            Ok(Some(_)) => Some(Kept {
                code: "in_use",
                reason: "The checkout became busy after the review".into(),
            }),
            Ok(None) => None,
        };
        if let Some(busy) = busy {
            for layer in chosen {
                record(cell_result(
                    checkout,
                    layer,
                    "skipped",
                    0,
                    0,
                    Some(Kept {
                        code: busy.code,
                        reason: busy.reason.clone(),
                    }),
                ));
            }
            continue;
        }
        for layer in chosen {
            let (mut moved_count, mut bytes, mut failed) = (0usize, 0u64, None);
            let mut first_kept: Option<Kept> = None;
            let mut total = 0usize;
            for (_, folder, kept) in judged.iter_mut().filter(|(l, _, _)| *l == layer) {
                total += 1;
                if let Some(kept) = kept.take() {
                    first_kept.get_or_insert(kept);
                    continue;
                }
                match hide_host::worktrees::set_aside_folder(run.common, &folder.path) {
                    Ok(entry) => {
                        moved.insert(entry);
                        moved_count += 1;
                        bytes = bytes.saturating_add(folder.bytes);
                    }
                    Err(error) => {
                        failed.get_or_insert(Kept {
                            code: "io",
                            reason: format!("The folder could not be moved aside: {error}"),
                        });
                    }
                }
            }
            record(match (moved_count, failed, first_kept, total) {
                (_, _, _, 0) => cell_result(
                    checkout,
                    layer,
                    "skipped",
                    0,
                    0,
                    Some(Kept {
                        code: "changed",
                        reason: "No measured folder is left in this cell".into(),
                    }),
                ),
                (0, Some(failure), _, _) => {
                    cell_result(checkout, layer, "failed", 0, 0, Some(failure))
                }
                (0, None, kept, _) => cell_result(checkout, layer, "skipped", 0, 0, kept),
                (moved, failure, kept, _) => {
                    cell_result(checkout, layer, "removed", bytes, moved, failure.or(kept))
                }
            });
        }
    }
    results
}

/// Spelled the way a comparison may not tell apart: canonically composed
/// (macOS hands out decomposed Hangul, Git prints what the index holds) and
/// case-folded. Folding more than the file system does only skips more.
fn folded(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    text.nfc()
        .collect::<String>()
        .to_lowercase()
        .nfc()
        .collect()
}

/// The folders among `folders` that hold a file Git tracks, from one
/// `git ls-files -z` for the checkout. The whole index is listed and compared
/// by folded spelling, so a folder that the file system spells differently
/// from the index is still found, and no pathspec list can grow past what an
/// argument list may hold.
fn tracked_folders<'a>(
    root: &Path,
    folders: impl Iterator<Item = &'a Path>,
) -> Result<Vec<PathBuf>, String> {
    let folders: Vec<&Path> = folders.collect();
    if folders.is_empty() {
        return Ok(Vec::new());
    }
    let listed = git(root, &["ls-files", "-z"])?;
    let mut index: Vec<String> = listed
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(folded)
        .collect();
    index.sort_unstable();
    Ok(folders
        .into_iter()
        .filter(|folder| {
            let Ok(relative) = folder.strip_prefix(root) else {
                // Not under the checkout: cannot be shown untracked.
                return true;
            };
            // Git lists the index in the wire's relative spelling, `/`
            // between names on every system.
            let relative = hide_platform::path::RelPath::from_native(relative)
                .map(hide_platform::path::RelPath::into_string)
                .unwrap_or_else(|_| relative.to_string_lossy().into_owned());
            let below = format!("{}/", folded(&relative));
            let at = index.partition_point(|entry| entry.as_str() < below.as_str());
            index.get(at).is_some_and(|entry| entry.starts_with(&below))
        })
        .map(Path::to_path_buf)
        .collect())
}

/// How long the removal waits for the background deletion of what it moved
/// before it reports what it has: a folder that will not go stays in the
/// trash for the next removal's sweep.
const DELETE_WAIT: std::time::Duration = std::time::Duration::from_secs(600);

pub fn spawn_confirm(
    context: LiveContext,
    review: CleanupSnapshot,
    input: ReviewInput,
    selected: Vec<String>,
    cells: Vec<CellChoice>,
) -> Result<(), String> {
    std::thread::Builder::new()
        .name("hide-worktree-cleanup".into())
        .spawn(move || {
            let root = PathBuf::from(&review.repository_root);
            let id = review.id;
            let total = selected.len() + cells.len();
            let mut done = 0usize;
            let mut answer = CleanupSnapshot {
                phase: "removing".into(),
                free_before: disk::volume_free_bytes(&root),
                progress: Some(CleanupProgress { done, total }),
                ..review.clone()
            };
            let mut guard = WorkerGuard::for_context(&context, answer.clone(), "complete");
            guard.publish(answer.clone());
            let common = crate::git_dir::discover(&root)
                .map(|repo| repo.common_dir)
                .unwrap_or_else(|| root.join(".git"));
            let mut ours = std::collections::BTreeSet::new();
            // Worktrees first: each is rechecked against what is on disk and
            // in Herdr right now, just before its own removal, and removed
            // without force.
            let refresh = |path: &str| {
                let in_use = fresh_in_use(&context, &review.workspace_id, Path::new(path))?
                    .map(|use_| InUseMap::from([(PathBuf::from(path), use_)]))
                    .unwrap_or_default();
                inspect(
                    &root,
                    input.current.as_deref(),
                    pane_paths(&context),
                    &in_use,
                    Some(&[path.to_owned()]),
                )
            };
            let confirmed = confirm(
                &review,
                &selected,
                refresh,
                |path| {
                    let before = hide_host::worktrees::trash_entries(&common);
                    let removed = remove_worktree(&root, Path::new(path), false);
                    ours.extend(
                        hide_host::worktrees::trash_entries(&common)
                            .difference(&before)
                            .cloned(),
                    );
                    removed
                },
                |row| {
                    let bytes = (row.result.as_deref() == Some("removed"))
                        .then(|| input.bytes.get(Path::new(&row.path)).copied())
                        .flatten();
                    crate::diagnostic!(serde_json::json!({
                        "component": "cleanup",
                        "kind": format!("worktree.{}", row.result.as_deref().unwrap_or("unknown")),
                        "cleanup_id": id,
                        "workspace_id": review.workspace_id,
                        "checkout": row.path,
                        "layer": "worktree",
                        "bytes": bytes,
                        "reason_code": row.result_code,
                        "reason": row.message,
                    }));
                    done += 1;
                },
            );
            answer.rows = confirmed.rows;
            for row in &mut answer.rows {
                if row.result.as_deref() == Some("removed") && row.bytes.is_none() {
                    row.bytes = input.bytes.get(Path::new(&row.path)).copied();
                }
            }
            answer.progress = Some(CleanupProgress { done, total });
            guard.publish(answer.clone());
            let run = CellRun {
                id,
                workspace_id: &review.workspace_id,
                common: &common,
                folders: &input.folders,
            };
            empty_cells(
                &run,
                &cells,
                |path| fresh_in_use(&context, &review.workspace_id, path),
                &mut ours,
                |result| {
                    answer.cell_results.push(result.clone());
                    done += 1;
                    answer.progress = Some(CleanupProgress { done, total });
                    guard.publish(answer.clone());
                },
            );
            // The folders left their checkouts at once; the volume only has
            // its space back once they are deleted.
            let remaining = hide_host::worktrees::drain_trash(&common, &ours, DELETE_WAIT);
            if remaining > 0 {
                answer.message = Some(format!(
                    "{remaining} removed folders are still being deleted in the background"
                ));
            }
            answer.phase = "complete".into();
            answer.progress = Some(CleanupProgress { done: total, total });
            answer.free_after = disk::volume_free_bytes(&root);
            answer.id = id;
            guard.finish(answer);
        })
        .map(|_| ())
        .map_err(|e| format!("Cleanup worker could not start: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hide_host::worktrees::{ConfirmedRemoval, remove_confirmed};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        main: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "hide-cleanup-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(root.join("main")).unwrap();
            let root = std::fs::canonicalize(root).unwrap();
            let main = root.join("main");
            git(&main, &["init", "-b", "main"]).unwrap();
            std::fs::write(main.join("tracked"), "keep").unwrap();
            // Build output is ignored, as in this repository, so a checkout
            // that has been built still counts as clean and removable.
            std::fs::write(main.join(".gitignore"), "target/\n").unwrap();
            git(&main, &["add", "."]).unwrap();
            git(
                &main,
                &[
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit",
                    "-m",
                    "Initial",
                ],
            )
            .unwrap();
            Self { root, main }
        }
        fn add(&self, name: &str) -> PathBuf {
            let path = self.root.join(name);
            git(
                &self.main,
                &[
                    "worktree",
                    "add",
                    "-b",
                    name,
                    path.to_str().unwrap(),
                    "main",
                ],
            )
            .unwrap();
            path
        }
        fn review(&self, current: &Path, usage: Result<Vec<PathBuf>, String>) -> CleanupSnapshot {
            inspect_with_merge_proofs(
                &self.main,
                Some(current),
                usage,
                &InUseMap::new(),
                None,
                |_| Ok(Vec::new()),
            )
            .unwrap()
        }
        fn review_with_proofs(
            &self,
            current: &Path,
            proofs: Vec<github::MergedPullRequestProof>,
        ) -> CleanupSnapshot {
            inspect_with_merge_proofs(
                &self.main,
                Some(current),
                Ok(Vec::new()),
                &InUseMap::new(),
                None,
                |_| Ok(proofs.clone()),
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn commit(path: &Path, message: &str) {
        git(
            path,
            &[
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                message,
            ],
        )
        .unwrap();
    }

    fn confirmed(f: &Fixture, worktree: &Path, delete_branch: bool) -> ConfirmedRemoval {
        ConfirmedRemoval {
            repository_root: f.main.to_string_lossy().into_owned(),
            checkout_path: worktree.to_string_lossy().into_owned(),
            expected_head_sha: Some(
                git(worktree, &["rev-parse", "HEAD"])
                    .unwrap()
                    .trim()
                    .to_owned(),
            ),
            expected_branch: worktree
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
            protected_base_branch: Some("main".into()),
            delete_branch: delete_branch
                .then(|| worktree.file_name().unwrap().to_string_lossy().into_owned()),
            force_delete_branch: false,
            discard_changes: false,
            expected_ignored_repositories: Vec::new(),
        }
    }

    fn branch_exists(f: &Fixture, branch: &str) -> bool {
        !git(&f.main, &["branch", "--list", branch])
            .unwrap()
            .trim()
            .is_empty()
    }

    #[test]
    fn a_confirmed_removal_keeps_a_dirty_worktree_and_removes_it_once_clean() {
        let f = Fixture::new();
        let worktree = f.add("linked");
        let request = confirmed(&f, &worktree, false);
        std::fs::write(worktree.join("tracked"), "dirty").unwrap();

        let refused = remove_confirmed(&request).unwrap_err();
        assert!(
            refused.starts_with("Not deleted: 1 file changed after you confirmed"),
            "{refused}"
        );
        assert!(
            refused.contains("panes already closed stay closed"),
            "{refused}"
        );
        assert!(worktree.exists());

        std::fs::write(worktree.join("tracked"), "keep").unwrap();
        let removed = remove_confirmed(&request).unwrap();
        assert!(removed.contains("local branch was kept"), "{removed}");
        assert!(!worktree.exists());
        assert!(branch_exists(&f, "linked"));
    }

    /// The operator ticked both boxes: the dirty folder goes with `--force`
    /// and the unmerged branch with `-D`, exactly as chosen.
    #[test]
    fn an_accepted_discard_removes_a_dirty_worktree_and_force_deletes_its_unmerged_branch() {
        let f = Fixture::new();
        let worktree = f.add("linked");
        commit(&worktree, "not merged anywhere");
        let mut request = confirmed(&f, &worktree, true);
        request.discard_changes = true;
        request.force_delete_branch = true;
        std::fs::write(worktree.join("tracked"), "dirty").unwrap();
        std::fs::write(worktree.join("untracked"), "new").unwrap();
        std::fs::create_dir_all(worktree.join("target/clone")).unwrap();
        git(&worktree.join("target/clone"), &["init", "-q"]).unwrap();
        request.expected_ignored_repositories = vec!["target/clone".to_owned()];

        let removed = remove_confirmed(&request).unwrap();
        assert!(removed.contains("and local branch linked"), "{removed}");
        assert!(!worktree.exists());
        assert!(!branch_exists(&f, "linked"));
    }

    /// Discard accepted but the branch deletion left safe: the folder goes,
    /// and the unmerged branch survives `-d` with Git's reason.
    #[test]
    fn an_accepted_discard_still_keeps_an_unmerged_branch_under_safe_deletion() {
        let f = Fixture::new();
        let worktree = f.add("linked");
        commit(&worktree, "not merged anywhere");
        let mut request = confirmed(&f, &worktree, true);
        request.discard_changes = true;
        std::fs::write(worktree.join("tracked"), "dirty").unwrap();

        let removed = remove_confirmed(&request).unwrap();
        assert!(removed.contains("branch linked remains"), "{removed}");
        assert!(!worktree.exists());
        assert!(branch_exists(&f, "linked"));
    }

    /// A worktree inside the one being deleted is lost with it, so only an
    /// accepted discard lets the removal through.
    #[test]
    fn a_worktree_inside_is_removed_only_with_an_accepted_discard() {
        let f = Fixture::new();
        let outer = f.add("outer");
        let inner = outer.join("inner");
        git(
            &f.main,
            &["worktree", "add", "--detach", inner.to_str().unwrap()],
        )
        .unwrap();
        let mut request = confirmed(&f, &outer, false);
        let refused = remove_confirmed(&request).unwrap_err();
        assert!(refused.contains("nested worktree"), "{refused}");
        assert!(inner.exists());

        request.discard_changes = true;
        remove_confirmed(&request).unwrap();
        assert!(!outer.exists());
    }

    #[test]
    fn a_confirmed_removal_of_a_missing_folder_clears_the_registration_and_keeps_the_branch() {
        let f = Fixture::new();
        let worktree = f.add("linked");
        let request = confirmed(&f, &worktree, false);
        std::fs::remove_dir_all(&worktree).unwrap();

        remove_confirmed(&request).unwrap();
        assert!(
            !git(&f.main, &["worktree", "list", "--porcelain"])
                .unwrap()
                .contains(&*worktree.to_string_lossy())
        );
        assert!(branch_exists(&f, "linked"));
    }

    #[test]
    fn a_confirmed_removal_deletes_a_merged_branch_only_with_safe_deletion() {
        let f = Fixture::new();
        let merged = f.add("merged");
        assert!(
            remove_confirmed(&confirmed(&f, &merged, true))
                .unwrap()
                .contains("and local branch merged")
        );
        assert!(!branch_exists(&f, "merged"));

        let ahead = f.add("ahead");
        commit(&ahead, "not merged anywhere");
        let message = remove_confirmed(&confirmed(&f, &ahead, true)).unwrap();
        assert!(message.contains("branch ahead remains"), "{message}");
        assert!(!ahead.exists());
        assert!(branch_exists(&f, "ahead"));
    }

    #[test]
    fn a_confirmed_removal_stops_when_the_worktree_changed_after_confirmation() {
        let f = Fixture::new();
        let worktree = f.add("linked");
        let request = confirmed(&f, &worktree, true);
        commit(&worktree, "moved after confirmation");

        let refused = remove_confirmed(&request).unwrap_err();
        assert!(refused.contains("identity changed"), "{refused}");
        assert!(worktree.exists());
        assert!(branch_exists(&f, "linked"));
    }

    #[test]
    fn a_confirmed_removal_refuses_the_main_checkout_and_the_protected_base() {
        let f = Fixture::new();
        let mut main = confirmed(&f, &f.main, false);
        main.expected_branch = Some("main".into());
        assert!(
            remove_confirmed(&main)
                .unwrap_err()
                .contains("main worktree")
        );

        let worktree = f.add("base");
        let mut base = confirmed(&f, &worktree, false);
        base.protected_base_branch = Some("base".into());
        assert!(
            remove_confirmed(&base)
                .unwrap_err()
                .contains("protected base")
        );
        assert!(worktree.exists());

        let nested = f.root.join("base").join("inner");
        git(
            &f.main,
            &[
                "worktree",
                "add",
                "-b",
                "inner",
                nested.to_str().unwrap(),
                "main",
            ],
        )
        .unwrap();
        let refused = remove_confirmed(&confirmed(&f, &worktree, false)).unwrap_err();
        assert!(refused.contains("nested worktree"), "{refused}");
        assert!(nested.exists());
    }

    #[test]
    fn a_stale_missing_pane_folder_does_not_block_unrelated_worktrees() {
        let f = Fixture::new();
        let clean = f.add("eligible");
        let stale = f.root.join("removed-worktree").join("pane");
        assert!(!stale.exists());

        let unrelated = f.review(
            &f.main,
            verified_pane_paths(vec![Some(stale.to_string_lossy().into_owned())]),
        );
        assert_eq!(
            unrelated
                .rows
                .iter()
                .find(|row| Path::new(&row.path) == clean)
                .unwrap()
                .exclusion,
            None
        );

        let matching_stale = clean.join("deleted-pane-cwd");
        assert!(!matching_stale.exists());
        let in_use = f.review(
            &f.main,
            verified_pane_paths(vec![Some(matching_stale.to_string_lossy().into_owned())]),
        );
        assert!(
            in_use
                .rows
                .iter()
                .find(|row| Path::new(&row.path) == clean)
                .unwrap()
                .exclusion
                .as_deref()
                .unwrap()
                .contains("In use")
        );
    }

    #[test]
    fn squash_merge_requires_the_exact_pr_head_and_a_merge_commit_in_local_main() {
        let f = Fixture::new();
        let squashed = f.add("squashed");
        std::fs::write(squashed.join("feature"), "merged through a squash").unwrap();
        git(&squashed, &["add", "."]).unwrap();
        git(
            &squashed,
            &[
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-m",
                "Feature",
            ],
        )
        .unwrap();
        let feature_head = git(&squashed, &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_owned();
        git(&f.main, &["merge", "--squash", "squashed"]).unwrap();
        git(
            &f.main,
            &[
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-m",
                "Squash feature",
            ],
        )
        .unwrap();
        let merge_head = git(&f.main, &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_owned();

        let matching = f.review_with_proofs(
            &f.main,
            vec![github::MergedPullRequestProof {
                head_oid: feature_head.clone(),
                merge_oid: Some(merge_head),
            }],
        );
        let row = matching
            .rows
            .iter()
            .find(|row| Path::new(&row.path) == squashed)
            .unwrap();
        assert_eq!(row.exclusion, None);

        let unavailable = inspect_with_merge_proofs(
            &f.main,
            Some(&f.main),
            Ok(Vec::new()),
            &InUseMap::new(),
            None,
            |_| Err("GitHub merge proof is unavailable".into()),
        )
        .unwrap();
        assert!(
            unavailable
                .rows
                .iter()
                .find(|row| Path::new(&row.path) == squashed)
                .unwrap()
                .exclusion
                .as_deref()
                .unwrap()
                .contains("unavailable")
        );

        let reused_branch = f.review_with_proofs(
            &f.main,
            vec![github::MergedPullRequestProof {
                head_oid: "a-different-head".into(),
                merge_oid: Some(feature_head.clone()),
            }],
        );
        assert!(
            reused_branch
                .rows
                .iter()
                .find(|row| Path::new(&row.path) == squashed)
                .unwrap()
                .exclusion
                .as_deref()
                .unwrap()
                .contains("exactly matches")
        );

        let local_main_behind = f.review_with_proofs(
            &f.main,
            vec![github::MergedPullRequestProof {
                head_oid: feature_head.clone(),
                merge_oid: Some(feature_head),
            }],
        );
        assert!(
            local_main_behind
                .rows
                .iter()
                .find(|row| Path::new(&row.path) == squashed)
                .unwrap()
                .exclusion
                .as_deref()
                .unwrap()
                .contains("not in local main")
        );
    }

    #[test]
    fn removal_deletes_the_selected_worktree_with_its_build_output_and_keeps_the_branch() {
        let f = Fixture::new();
        let clean = f.add("with-cache");
        std::fs::create_dir_all(clean.join("target/debug")).unwrap();
        std::fs::write(clean.join("target/debug/artifact"), "regenerable").unwrap();
        let review = f.review(&f.main, Ok(Vec::new()));
        let selected = vec![clean.to_string_lossy().into_owned()];

        let result = confirm(
            &review,
            &selected,
            |_| Ok(f.review(&f.main, Ok(Vec::new()))),
            |path| remove_worktree(&f.main, Path::new(path), false),
            |_| {},
        );

        assert!(!clean.exists());
        assert_eq!(
            result
                .rows
                .iter()
                .find(|row| row.path == selected[0])
                .and_then(|row| row.result.as_deref()),
            Some("removed")
        );
        assert!(git(&f.main, &["show-ref", "--verify", "refs/heads/with-cache"]).is_ok());
    }

    #[test]
    fn review_and_cancel_preserve_files_and_explain_every_exclusion() {
        let f = Fixture::new();
        let clean = f.add("clean");
        let dirty = f.add("dirty");
        let untracked = f.add("untracked");
        let busy = f.add("busy");
        let current = f.add("current");
        let ahead = f.add("ahead");
        let locked = f.add("locked");
        std::fs::write(dirty.join("tracked"), "changed").unwrap();
        std::fs::write(untracked.join("new"), "keep").unwrap();
        std::fs::create_dir(busy.join("subdir")).unwrap();
        git(&f.main, &["worktree", "lock", locked.to_str().unwrap()]).unwrap();
        git(
            &ahead,
            &[
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "Unmerged",
            ],
        )
        .unwrap();
        let before = git(&f.main, &["worktree", "list", "--porcelain", "-z"]).unwrap();
        let review = f.review(&current, Ok(vec![busy.join("subdir")]));
        let row = |path: &Path| {
            review
                .rows
                .iter()
                .find(|r| Path::new(&r.path) == path)
                .unwrap()
        };
        assert!(row(&clean).exclusion.is_none());
        for (path, reason) in [
            (&f.main, "Main"),
            (&dirty, "Uncommitted"),
            (&untracked, "Uncommitted"),
            (&busy, "In use"),
            (&current, "Current"),
            (&ahead, "Not merged"),
            (&locked, "locked"),
        ] {
            assert!(row(path).exclusion.as_ref().unwrap().contains(reason));
        }
        let cancelled = confirm(
            &review,
            &[],
            |_| panic!("cancel must not inspect"),
            |_| panic!("cancel must not delete"),
            |_| {},
        );
        assert!(cancelled.rows.iter().all(|r| r.result.is_none()));
        assert_eq!(
            git(&f.main, &["worktree", "list", "--porcelain", "-z"]).unwrap(),
            before
        );
        for path in [&clean, &dirty, &untracked, &busy, &current, &ahead, &locked] {
            assert!(path.join("tracked").exists());
        }
        let unknown = f.review(&current, Err("Usage unavailable".into()));
        assert!(unknown.rows.iter().all(|r| r.exclusion.is_some()));
    }

    #[test]
    fn confirm_rechecks_stale_state_and_converges_partial_success_without_repeating_removal() {
        let f = Fixture::new();
        let clean = f.add("merged");
        let changed = f.add("changed");
        let failed = f.add("failed");
        let review = f.review(&f.main, Ok(vec![]));
        let paths: Vec<_> = [&clean, &changed, &failed]
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        std::fs::write(changed.join("new"), "arrived after review").unwrap();
        let mut effects = Vec::new();
        let result = confirm(
            &review,
            &paths,
            |_| Ok(f.review(&f.main, Ok(vec![]))),
            |path| {
                effects.push(path.to_owned());
                if Path::new(path) == failed {
                    return Err("Git refused removal".into());
                }
                git(&f.main, &["worktree", "remove", "--", path])
                    .map(|_| "Worktree removed".to_owned())
            },
            |_| {},
        );
        assert!(!clean.exists());
        assert!(changed.join("new").exists());
        assert!(failed.exists());
        assert_eq!(effects.len(), 2, "stale target never reached remove");
        assert_eq!(
            result
                .rows
                .iter()
                .filter(|r| r.result.as_deref() == Some("removed"))
                .count(),
            1
        );
        assert_eq!(
            result
                .rows
                .iter()
                .filter(|r| r.result.as_deref() == Some("skipped"))
                .count(),
            1,
            "the worktree that changed after the review is skipped, by its code"
        );
        assert_eq!(
            result
                .rows
                .iter()
                .filter(|r| r.result.as_deref() == Some("failed"))
                .map(|r| r.result_code)
                .collect::<Vec<_>>(),
            [Some("remove_refused")],
            "a removal Git refuses is a failure, not a recheck"
        );
        let retry = confirm(
            &result,
            &paths,
            |_| Ok(f.review(&f.main, Ok(vec![]))),
            |path| {
                assert_ne!(Path::new(path), clean);
                git(&f.main, &["worktree", "remove", "--", path])
                    .map(|_| "Worktree removed".to_owned())
            },
            |_| {},
        );
        assert_eq!(
            retry
                .rows
                .iter()
                .filter(|r| r.result.as_deref() == Some("removed"))
                .count(),
            2
        );
        assert!(git(&f.main, &["show-ref", "--verify", "refs/heads/merged"]).is_ok());
        assert_eq!(
            std::fs::read_to_string(f.main.join("tracked")).unwrap(),
            "keep"
        );
    }

    fn facts(path: &str, working: usize, panes: &[&str]) -> CheckoutFacts {
        CheckoutFacts {
            path: PathBuf::from(path),
            agent_working: working,
            terminal_panes: panes.iter().map(|pane| (*pane).to_owned()).collect(),
        }
    }

    fn listener(port: u16, cwd: &str) -> ListeningPortSnapshot {
        ListeningPortSnapshot {
            host: "127.0.0.1".into(),
            port,
            cwd: cwd.to_owned(),
        }
    }

    #[test]
    fn a_checkout_is_in_use_by_a_working_agent_a_running_program_or_its_own_server() {
        let checkouts = [
            facts("/fixture/agent", 1, &["p-agent"]),
            facts("/fixture/build", 0, &["p-idle", "p-cargo"]),
            facts("/fixture/server", 0, &[]),
            facts("/fixture/idle", 0, &["p-idle"]),
            // A neighbour whose name only starts the same way is not this checkout.
            facts("/fixture/idle-two", 0, &[]),
        ];
        let in_use = read_in_use(
            &checkouts,
            Ok(vec![
                listener(5173, "/fixture/server/web"),
                listener(8080, "/fixture/idle-two-staging"),
            ]),
            |pane| {
                assert_ne!(pane, "p-agent", "a checkout already known busy is not read");
                Ok((pane == "p-cargo").then(|| "cargo".to_owned()))
            },
        )
        .unwrap();
        assert_eq!(in_use[Path::new("/fixture/agent")].code, "agent_working");
        let build = &in_use[Path::new("/fixture/build")];
        assert_eq!(
            (build.code, build.name.as_deref()),
            ("process", Some("cargo"))
        );
        let server = &in_use[Path::new("/fixture/server")];
        assert_eq!((server.code, server.port), ("port", Some(5173)));
        assert!(!in_use.contains_key(Path::new("/fixture/idle")));
        assert!(!in_use.contains_key(Path::new("/fixture/idle-two")));
    }

    #[test]
    fn an_unreadable_pane_or_port_list_fails_the_whole_answer_instead_of_reading_idle() {
        let checkouts = [facts("/fixture/a", 0, &["p1"])];
        assert!(read_in_use(&checkouts, Err("lsof is missing".into()), |_| Ok(None)).is_err());
        assert!(
            read_in_use(&checkouts, Ok(Vec::new()), |_| Err(
                "pane.process_info failed".into()
            ))
            .is_err()
        );
        let many: Vec<_> = (0..=PANE_READ_LIMIT).map(|n| format!("p{n}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let error = read_in_use(&[facts("/fixture/b", 0, &many)], Ok(Vec::new()), |_| {
            Ok(None)
        })
        .unwrap_err();
        assert!(error.contains("too many"), "{error}");
    }

    #[test]
    fn every_reason_a_worktree_cannot_be_removed_carries_a_code_and_a_count_where_one_helps() {
        let f = Fixture::new();
        let clean = f.add("clean");
        let dirty = f.add("dirty");
        let busy = f.add("busy");
        let nested = f.add("nested");
        let ahead = f.add("ahead");
        let locked = f.add("locked");
        let current = f.add("current");
        std::fs::write(dirty.join("tracked"), "changed").unwrap();
        std::fs::write(dirty.join("new"), "untracked").unwrap();
        std::fs::create_dir_all(nested.join("target/clone/.git")).unwrap();
        commit(&ahead, "Unmerged");
        git(&f.main, &["worktree", "lock", locked.to_str().unwrap()]).unwrap();
        let mut in_use = InUseMap::new();
        in_use.insert(
            busy.clone(),
            InUse {
                code: "process",
                name: Some("cargo".into()),
                port: None,
            },
        );
        let review = inspect_with_merge_proofs(
            &f.main,
            Some(&current),
            Ok(Vec::new()),
            &in_use,
            None,
            |_| Ok(Vec::new()),
        )
        .unwrap();
        let row = |path: &Path| {
            review
                .rows
                .iter()
                .find(|r| Path::new(&r.path) == path)
                .unwrap()
        };
        assert_eq!(row(&clean).exclusion_code, None);
        assert!(row(&f.main).is_main);
        for (path, code) in [
            (&f.main, "main"),
            (&dirty, "dirty"),
            (&busy, "in_use"),
            (&nested, "nested_repository"),
            (&ahead, "not_merged"),
            (&locked, "locked"),
            (&current, "current"),
        ] {
            assert_eq!(row(path).exclusion_code, Some(code), "{}", path.display());
        }
        assert_eq!(row(&dirty).exclusion_count, Some(2));
        assert_eq!(
            row(&busy).in_use.as_ref().unwrap().name.as_deref(),
            Some("cargo")
        );
        assert_eq!(row(&f.main).in_use, None, "an idle main carries no reason");
        assert!(review.usage_ready);
    }

    /// A built checkout: a Rust and a Node package, each with folders a
    /// tool makes again, all ignored, next to tracked source.
    struct Built {
        f: Fixture,
        folders: HashMap<PathBuf, Vec<LayerFolder>>,
    }

    impl Built {
        fn new() -> Self {
            let f = Fixture::new();
            let main = f.main.clone();
            std::fs::write(main.join(".gitignore"), "target/\nnode_modules/\ndist/\n").unwrap();
            std::fs::write(main.join("Cargo.toml"), "[package]\n").unwrap();
            std::fs::write(main.join("package.json"), "{}\n").unwrap();
            std::fs::create_dir_all(main.join("target/debug")).unwrap();
            std::fs::write(main.join("target/debug/artifact"), vec![1u8; 20_000]).unwrap();
            std::fs::write(
                main.join("target/CACHEDIR.TAG"),
                "Signature: 8a477f597d28d172789f06886806bc55\n",
            )
            .unwrap();
            std::fs::create_dir_all(main.join("node_modules/dep")).unwrap();
            std::fs::write(main.join("node_modules/dep/index.js"), vec![2u8; 8_000]).unwrap();
            std::fs::create_dir_all(main.join("dist")).unwrap();
            std::fs::write(main.join("dist/app.js"), vec![3u8; 4_000]).unwrap();
            git(&main, &["add", ".gitignore", "Cargo.toml", "package.json"]).unwrap();
            commit(&main, "Manifests");
            let measured = disk::read(&disk::DiskRequest {
                paths: vec![main.clone()],
                ..Default::default()
            });
            let folders = HashMap::from([(main, measured[0].folders.clone())]);
            Self { f, folders }
        }

        fn common(&self) -> PathBuf {
            self.f.main.join(".git")
        }

        fn run(&self, in_use: Result<&InUseMap, &str>, cells: &[(Layer,)]) -> Vec<CellResult> {
            self.run_with(
                |path| match in_use {
                    Err(message) => Err(message.to_owned()),
                    Ok(map) => Ok(map.get(path).cloned()),
                },
                cells,
            )
        }

        fn run_with(
            &self,
            in_use_now: impl FnMut(&Path) -> Result<Option<InUse>, String>,
            cells: &[(Layer,)],
        ) -> Vec<CellResult> {
            let run = CellRun {
                id: 1,
                workspace_id: "w1",
                common: &self.common(),
                folders: &self.folders,
            };
            let cells: Vec<_> = cells
                .iter()
                .map(|(layer,)| CellChoice {
                    path: self.f.main.to_string_lossy().into_owned(),
                    layer: *layer,
                })
                .collect();
            let mut moved = std::collections::BTreeSet::new();
            let results = empty_cells(&run, &cells, in_use_now, &mut moved, |_| {});
            assert!(moved.iter().all(|entry| entry.starts_with(self.common())));
            results
        }
    }

    /// What a forced add leaves: a file Git tracks inside an ignored folder.
    /// (Written without the force flag, which this file never spells.)
    fn track_in_ignored(main: &Path, file: &str) {
        let ignore = std::fs::read_to_string(main.join(".gitignore")).unwrap();
        std::fs::write(main.join(".gitignore"), "").unwrap();
        git(main, &["add", file]).unwrap();
        std::fs::write(main.join(".gitignore"), ignore).unwrap();
    }

    fn outcome(results: &[CellResult], layer: Layer) -> (&'static str, Option<&'static str>) {
        let result = results.iter().find(|r| r.layer == layer.code()).unwrap();
        (result.outcome, result.reason_code)
    }

    #[test]
    fn a_chosen_cell_leaves_its_checkout_at_once_and_the_trash_deletes_it() {
        let built = Built::new();
        let main = &built.f.main;
        let results = built.run(
            Ok(&InUseMap::new()),
            &[(Layer::BuildCache,), (Layer::Dependencies,)],
        );
        let cache = results.iter().find(|r| r.layer == "build_cache").unwrap();
        // `target` and `dist` are both build cache.
        assert_eq!((cache.outcome, cache.folders), ("removed", 2));
        assert!(cache.bytes >= 24_000);
        assert_eq!(outcome(&results, Layer::Dependencies), ("removed", None));
        for gone in ["target", "node_modules", "dist"] {
            assert!(!main.join(gone).exists(), "{gone} left the checkout");
        }
        for kept in ["tracked", "Cargo.toml", "package.json", ".gitignore"] {
            assert!(main.join(kept).exists(), "{kept} is source");
        }
        assert_eq!(
            hide_host::worktrees::drain_trash(
                &built.common(),
                &hide_host::worktrees::trash_entries(&built.common()),
                std::time::Duration::from_secs(30),
            ),
            0
        );
        assert_eq!(
            std::fs::read_dir(built.common().join("hide-removed"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn a_checkout_that_became_busy_since_the_review_or_cannot_be_read_loses_nothing() {
        let built = Built::new();
        let busy = InUseMap::from([(
            built.f.main.clone(),
            InUse {
                code: "agent_working",
                name: None,
                port: None,
            },
        )]);
        let results = built.run(Ok(&busy), &[(Layer::BuildCache,)]);
        assert_eq!(
            outcome(&results, Layer::BuildCache),
            ("skipped", Some("in_use"))
        );
        let results = built.run(Err("pane.process_info failed"), &[(Layer::Dependencies,)]);
        assert_eq!(
            outcome(&results, Layer::Dependencies),
            ("skipped", Some("unverified"))
        );
        for kept in ["target", "node_modules", "dist"] {
            assert!(built.f.main.join(kept).exists());
        }
    }

    #[test]
    fn a_folder_that_changed_after_the_measurement_is_kept_and_named() {
        let built = Built::new();
        let main = &built.f.main;
        // A link swapped in for the folder: what it points at is never touched.
        let outside = built.f.root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("precious"), "keep").unwrap();
        std::fs::remove_dir_all(main.join("node_modules")).unwrap();
        hide_platform::fs::link::create_link(&outside, &main.join("node_modules")).unwrap();
        // A file Git tracks appeared in an ignored folder.
        std::fs::write(main.join("dist/keep.txt"), "tracked on purpose").unwrap();
        track_in_ignored(main, "dist/keep.txt");
        // Another repository was cloned into the build folder.
        std::fs::create_dir_all(main.join("target/vendored/.git")).unwrap();

        let results = built.run(
            Ok(&InUseMap::new()),
            &[(Layer::BuildCache,), (Layer::Dependencies,)],
        );
        assert_eq!(
            outcome(&results, Layer::Dependencies),
            ("skipped", Some("symlink"))
        );
        // The build-cache cell had two folders: none could move.
        assert_eq!(outcome(&results, Layer::BuildCache).0, "skipped");
        assert!(matches!(
            outcome(&results, Layer::BuildCache).1,
            Some("tracked_files" | "nested_repository")
        ));
        assert_eq!(
            std::fs::read_to_string(outside.join("precious")).unwrap(),
            "keep"
        );
        assert!(main.join("dist/keep.txt").exists());
        assert!(main.join("target/vendored/.git").exists());
    }

    #[test]
    fn one_folder_kept_does_not_stop_its_cell_and_the_reason_stays_on_the_result() {
        let built = Built::new();
        let main = &built.f.main;
        std::fs::write(main.join("dist/keep.txt"), "tracked on purpose").unwrap();
        track_in_ignored(main, "dist/keep.txt");
        let results = built.run(Ok(&InUseMap::new()), &[(Layer::BuildCache,)]);
        let cache = &results[0];
        assert_eq!((cache.outcome, cache.folders), ("removed", 1));
        assert_eq!(cache.reason_code, Some("tracked_files"));
        assert!(!main.join("target").exists());
        assert!(main.join("dist/keep.txt").exists());
    }

    #[test]
    fn repeating_a_confirmed_cell_removes_nothing_a_second_time() {
        let built = Built::new();
        let first = built.run(Ok(&InUseMap::new()), &[(Layer::Dependencies,)]);
        assert_eq!(outcome(&first, Layer::Dependencies), ("removed", None));
        let again = built.run(Ok(&InUseMap::new()), &[(Layer::Dependencies,)]);
        assert_eq!(
            outcome(&again, Layer::Dependencies),
            ("skipped", Some("not_found"))
        );
        assert_eq!(again[0].bytes, 0);
    }

    #[test]
    fn a_checkouts_folders_are_asked_of_git_once() {
        let built = Built::new();
        built.run(
            Ok(&InUseMap::new()),
            &[(Layer::BuildCache,), (Layer::Dependencies,)],
        );
        let asked = hide_host::worktrees::GIT_CALLS
            .lock()
            .unwrap()
            .iter()
            .filter(|(cwd, command)| *cwd == built.f.main && command == "ls-files")
            .count();
        assert_eq!(asked, 1, "three folders, one ls-files");
    }

    fn ls_files_asked(root: &Path) -> usize {
        hide_host::worktrees::GIT_CALLS
            .lock()
            .unwrap()
            .iter()
            .filter(|(cwd, command)| cwd == root && command == "ls-files")
            .count()
    }

    #[test]
    fn a_registered_worktree_the_runtime_has_no_facts_about_reads_unverified_never_idle() {
        let known = [facts("/fixture/known", 0, &[])];
        let mut in_use = InUseMap::new();
        mark_unverified(
            &mut in_use,
            &known,
            [
                PathBuf::from("/fixture/known"),
                PathBuf::from("/fixture/unknown"),
            ],
        );
        assert!(!in_use.contains_key(Path::new("/fixture/known")));
        assert_eq!(in_use[Path::new("/fixture/unknown")].code, "unverified");

        // The fresh read behaves the same way, and a project that is gone is
        // an error, not an idle answer.
        let read = |_: &[CheckoutFacts]| Ok(InUseMap::new());
        let now = read_in_use_now(
            Path::new("/fixture/unknown"),
            || Some(vec![facts("/fixture/known", 0, &[])]),
            read,
        )
        .unwrap();
        assert_eq!(now.map(|use_| use_.code), Some("unverified"));
        let now = read_in_use_now(
            Path::new("/fixture/known"),
            || Some(vec![facts("/fixture/known", 0, &[])]),
            read,
        )
        .unwrap();
        assert_eq!(now, None);
        assert!(read_in_use_now(Path::new("/fixture/known"), || None, read).is_err());
    }

    #[test]
    fn a_checkout_that_turns_busy_after_the_folders_were_judged_loses_nothing() {
        let built = Built::new();
        let root = built.f.main.clone();
        let asked_before = ls_files_asked(&root);
        let mut looked = 0;
        let results = built.run_with(
            |_| {
                looked += 1;
                // Every judgement is done, Git has been asked, and nothing
                // has moved yet.
                assert_eq!(ls_files_asked(&root), asked_before + 1);
                for folder in ["target", "node_modules", "dist"] {
                    assert!(root.join(folder).exists());
                }
                Ok(Some(InUse {
                    code: "process",
                    name: Some("cargo".into()),
                    port: None,
                }))
            },
            &[(Layer::BuildCache,), (Layer::Dependencies,)],
        );
        assert_eq!(looked, 1, "one look per checkout, right before the moves");
        for layer in [Layer::BuildCache, Layer::Dependencies] {
            assert_eq!(outcome(&results, layer), ("skipped", Some("in_use")));
        }
        for folder in ["target", "node_modules", "dist"] {
            assert!(root.join(folder).exists(), "{folder} stayed");
        }
    }

    #[test]
    fn a_folder_the_index_spells_differently_is_still_found_tracked() {
        let f = Fixture::new();
        let main = &f.main;
        // The index holds `Dist/keep.txt` and `한글/keep.txt`; the file
        // system, and so the measurement, may hand back another spelling.
        std::fs::create_dir_all(main.join("Dist")).unwrap();
        std::fs::write(main.join("Dist/keep.txt"), "x").unwrap();
        let composed = "\u{d55c}\u{ae00}";
        std::fs::create_dir_all(main.join(composed)).unwrap();
        std::fs::write(main.join(composed).join("keep.txt"), "x").unwrap();
        git(main, &["add", "Dist/keep.txt"]).unwrap();
        git(main, &["add", "--", &format!("{composed}/keep.txt")]).unwrap();
        let decomposed = "\u{1112}\u{1161}\u{11ab}\u{1100}\u{1173}\u{11af}";
        let candidates = [
            main.join("dist"),
            main.join(decomposed),
            main.join("elsewhere"),
            main.join("Dist"),
        ];
        let tracked = tracked_folders(main, candidates.iter().map(PathBuf::as_path)).unwrap();
        assert_eq!(
            tracked,
            [main.join("dist"), main.join(decomposed), main.join("Dist")]
        );
    }

    #[test]
    fn thousands_of_folders_cost_one_ls_files_and_no_argument_list() {
        let f = Fixture::new();
        let main = &f.main;
        let folders: Vec<PathBuf> = (0..20_000)
            .map(|n| main.join(format!("package-{n}/node_modules")))
            .collect();
        let asked_before = ls_files_asked(main);
        let tracked = tracked_folders(main, folders.iter().map(PathBuf::as_path)).unwrap();
        assert!(tracked.is_empty());
        assert_eq!(ls_files_asked(main), asked_before + 1);
        // `tracked` is a file at the top of the fixture, not a folder below
        // a candidate that merely starts the same way.
        let near = [main.join("track"), main.join("tracked")];
        let tracked = tracked_folders(main, near.iter().map(PathBuf::as_path)).unwrap();
        assert!(tracked.is_empty());
    }

    type Published = std::sync::Arc<std::sync::Mutex<Vec<CleanupSnapshot>>>;

    fn guard(
        terminal: &'static str,
    ) -> (
        WorkerGuard,
        Published,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        let published = Published::default();
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (sink, flag) = (published.clone(), finished.clone());
        let guard = WorkerGuard::new(
            Box::new(move |snapshot| sink.lock().unwrap().push(snapshot)),
            Box::new(move || flag.store(true, Ordering::SeqCst)),
            CleanupSnapshot {
                id: 4,
                phase: "removing".into(),
                ..Default::default()
            },
            terminal,
        );
        (guard, published, finished)
    }

    #[test]
    fn a_worker_that_panics_leaves_a_terminal_answer_and_frees_the_lane() {
        let (guard, published, finished) = guard("complete");
        let stopped = std::thread::spawn(move || {
            let mut guard = guard;
            guard.publish(CleanupSnapshot {
                id: 4,
                phase: "removing".into(),
                progress: Some(CleanupProgress { done: 1, total: 3 }),
                ..Default::default()
            });
            panic!("the worker died mid-run");
        })
        .join();
        assert!(stopped.is_err());
        let published = published.lock().unwrap();
        let last = published.last().unwrap();
        assert_eq!((last.id, last.phase.as_str()), (4, "complete"));
        assert_eq!(last.progress, Some(CleanupProgress { done: 1, total: 3 }));
        assert!(last.message.is_some());
        assert!(finished.load(Ordering::SeqCst));
    }

    #[test]
    fn a_worker_that_finishes_publishes_only_its_own_answer_and_frees_the_lane() {
        let (guard, published, finished) = guard("failed");
        guard.finish(CleanupSnapshot {
            id: 4,
            phase: "review".into(),
            ..Default::default()
        });
        let published = published.lock().unwrap();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].phase, "review");
        assert!(finished.load(Ordering::SeqCst));
    }

    #[test]
    fn the_shell_reads_codes_and_sizes_and_never_the_words_meant_for_the_log() {
        let snapshot = CleanupSnapshot {
            rows: vec![CleanupRow {
                path: "/w/a".into(),
                exclusion: Some("Git says the branch is not merged".into()),
                exclusion_code: Some("not_merged"),
                result: Some("skipped".into()),
                result_code: Some("changed"),
                bytes: Some(4096),
                message: Some("The worktree changed after the review".into()),
                ..Default::default()
            }],
            cell_results: vec![CellResult {
                path: "/w/a".into(),
                layer: "build_cache",
                outcome: "skipped",
                bytes: 0,
                folders: 0,
                reason_code: Some("tracked_files"),
                reason: Some("Git tracks a file inside the folder".into()),
            }],
            ..Default::default()
        };
        let wire = serde_json::to_value(&snapshot).unwrap();
        let row = wire["rows"][0].as_object().unwrap();
        assert!(!row.contains_key("exclusion") && !row.contains_key("message"));
        assert_eq!(row["exclusion_code"], "not_merged");
        assert_eq!(
            (&row["result"], &row["result_code"]),
            (&"skipped".into(), &"changed".into())
        );
        assert_eq!(row["bytes"], 4096);
        let cell = wire["cell_results"][0].as_object().unwrap();
        assert!(!cell.contains_key("reason"));
        assert_eq!(cell["reason_code"], "tracked_files");
    }
}
