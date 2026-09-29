//! Reviewed cleanup of a project's disk: the build caches and dependencies a
//! tool makes again, and clean linked worktrees merged into local main.
//! All inspection and filesystem effects run on the existing action-worker
//! path, never under the runtime lock.
use super::{LiveContext, control_request};
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
    /// `agent_working`, `process` or `port`.
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
    /// The reason the worktree may not be removed, in words for the log; the
    /// shell reads `exclusion_code`.
    pub exclusion: Option<String>,
    pub exclusion_code: Option<&'static str>,
    pub exclusion_count: Option<u32>,
    pub in_use: Option<InUse>,
    pub result: Option<String>,
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
        let path = std::fs::canonicalize(&checkout.path).unwrap_or_else(|_| checkout.path.clone());
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
            Ok(std::fs::canonicalize(&path).unwrap_or(path))
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
    let canonical_current = current.and_then(|current| std::fs::canonicalize(current).ok());
    let paths: Vec<PathBuf> = rows.iter().map(|r| PathBuf::from(&r.path)).collect();
    for row in &mut rows {
        row.in_use = in_use.get(Path::new(&row.path)).cloned();
        if row.exclusion.is_some() || only.is_some_and(|only| !only.contains(&row.path)) {
            continue;
        }
        let checked = (|| -> Result<(), Exclusion> {
            let path = Path::new(&row.path);
            let canonical = std::fs::canonicalize(path)
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
        repository_root: root.to_string_lossy().into_owned(),
        main_head: main_head.ok(),
        rows,
        phase: "review".into(),
        usage_ready: true,
        ..Default::default()
    })
}

fn confirm(
    review: &CleanupSnapshot,
    selected: &[String],
    mut refresh: impl FnMut() -> Result<CleanupSnapshot, String>,
    mut remove: impl FnMut(&str) -> Result<String, String>,
) -> CleanupSnapshot {
    let mut result = review.clone();
    result.phase = "complete".into();
    // What confirmation rereads is read once, before anything is removed.
    let mut fresh: Option<Result<CleanupSnapshot, String>> = None;
    for row in &mut result.rows {
        if !selected.contains(&row.path) || row.result.as_deref() == Some("removed") {
            continue;
        }
        let validation = (|| -> Result<bool, String> {
            if let Some(reason) = &row.exclusion {
                return Err(format!("Excluded: {reason}"));
            }
            let fresh = fresh.get_or_insert_with(&mut refresh).clone()?;
            let Some(now) = fresh.rows.iter().find(|r| r.path == row.path) else {
                if !Path::new(&row.path)
                    .try_exists()
                    .map_err(|e| e.to_string())?
                {
                    return Ok(false);
                }
                return Err("Registration changed but the folder remains. Review again; no files were removed.".into());
            };
            if fresh.main_head != review.main_head
                || now.head != row.head
                || now.branch != row.branch
                || now.exclusion.is_some()
            {
                return Err(format!(
                    "State changed. {} Review again before removing.",
                    now.exclusion
                        .as_deref()
                        .unwrap_or("The reviewed branch or main HEAD moved.")
                ));
            }
            Ok(true)
        })();
        let removed = validation.and_then(|needed| {
            if needed {
                remove(&row.path)
            } else {
                Ok("Worktree was already removed. Branch and Git history stay.".into())
            }
        });
        match removed {
            Ok(message) => {
                row.result = Some("removed".into());
                row.message = Some(message);
            }
            Err(message) => {
                row.result = Some("refused".into());
                row.message = Some(message);
            }
        }
    }
    result
}

use hide_host::worktrees::remove_worktree;

/// Hands a snapshot to the runtime, which keeps it only while its id is the
/// live one.
fn publish(context: &LiveContext, snapshot: CleanupSnapshot) {
    if let Some(runtime) = context.runtime.upgrade() {
        if let Ok(mut guard) = runtime.lock() {
            guard.ingest_cleanup(snapshot);
        }
        context.notifier.notify();
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
                repository_root: input.root.to_string_lossy().into_owned(),
                free_bytes: disk::volume_free_bytes(&input.root),
                ..Default::default()
            };
            let usage = live_in_use(&context, &input.checkouts);
            let (in_use, usage_error) = match usage {
                Ok(map) => (map, None),
                Err(message) => (InUseMap::new(), Some(message)),
            };
            match listed(&input.root) {
                Ok(mut rows) => {
                    for row in &mut rows {
                        row.in_use = in_use.get(Path::new(&row.path)).cloned();
                    }
                    publish(
                        &context,
                        CleanupSnapshot {
                            phase: "loading".into(),
                            rows,
                            usage_ready: usage_error.is_none(),
                            usage_error: usage_error.clone(),
                            ..base.clone()
                        },
                    );
                }
                Err(message) => {
                    publish(
                        &context,
                        CleanupSnapshot {
                            phase: "failed".into(),
                            message: Some(message),
                            ..base
                        },
                    );
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
            publish(&context, answer);
        })
        .map(|_| ())
        .map_err(|e| format!("Cleanup worker could not start: {e}"))
}

pub fn spawn_confirm(
    context: LiveContext,
    review: CleanupSnapshot,
    input: ReviewInput,
    selected: Vec<String>,
) -> Result<(), String> {
    std::thread::Builder::new()
        .name("hide-worktree-cleanup".into())
        .spawn(move || {
            let root = PathBuf::from(&review.repository_root);
            let refresh = || {
                let in_use = live_in_use(&context, &input.checkouts)?;
                inspect(
                    &root,
                    input.current.as_deref(),
                    pane_paths(&context),
                    &in_use,
                    Some(&selected),
                )
            };
            let mut answer = confirm(&review, &selected, refresh, |path| {
                remove_worktree(&root, Path::new(path), false)
            });
            answer.id = review.id;
            publish(&context, answer);
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
        std::fs::create_dir_all(worktree.join("target/clone/.git")).unwrap();

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
            || Ok(f.review(&f.main, Ok(Vec::new()))),
            |path| remove_worktree(&f.main, Path::new(path), false),
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
            || panic!("cancel must not inspect"),
            |_| panic!("cancel must not delete"),
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
            || Ok(f.review(&f.main, Ok(vec![]))),
            |path| {
                effects.push(path.to_owned());
                if Path::new(path) == failed {
                    return Err("Git refused removal".into());
                }
                git(&f.main, &["worktree", "remove", "--", path])
                    .map(|_| "Worktree removed".to_owned())
            },
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
                .filter(|r| r.result.as_deref() == Some("refused"))
                .count(),
            2
        );
        let retry = confirm(
            &result,
            &paths,
            || Ok(f.review(&f.main, Ok(vec![]))),
            |path| {
                assert_ne!(Path::new(path), clean);
                git(&f.main, &["worktree", "remove", "--", path])
                    .map(|_| "Worktree removed".to_owned())
            },
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
}
