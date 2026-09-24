//! Every worktree of every open repository, with the counts the sidebar row
//! and the summary card show.
//!
//! A worktree earns a row because git lists it, not because Herdr has a pane
//! in it: that is the whole point of the projects tree, and it is why this
//! reader exists rather than the catalog forking `git` itself. Each read runs
//! on a worker thread ([`crate::reader::BackgroundRead`]), so a repository
//! with many worktrees costs wall time on that thread and never coordinator
//! latency.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::model::{
    ProjectWorktreesSnapshot, UnpushedSnapshot, WorktreeCatalogSnapshot,
    WorktreeDeletionGateSnapshot, WorktreeSnapshot,
};
use crate::reader::BackgroundRead;

/// What to describe: one entry per project the navigator is showing.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorktreeRequest {
    pub projects: Vec<WorktreeProjectRequest>,
    /// Bumped when something invalidates every answer at once - a worktree
    /// removal, most of all. A changed request is always due, so this is how
    /// an unchanged project list still gets re-read.
    pub generation: u64,
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
}

type FileStamps = Vec<(PathBuf, Option<std::time::SystemTime>, u64)>;

/// One project's freshness key: the request that describes it, the stamps of
/// its own git directory, and a counter its own working-tree files advance.
/// Each project carries its own key so a commit in one repository re-reads
/// that repository alone. The first version keyed the whole catalog on every
/// project's stamps together, and a checkpoint commit anywhere re-ran
/// `git status` in every registered repository, including one on iCloud
/// whose evicted files that status pulled back down for minutes.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ProjectObservation {
    request: WorktreeProjectRequest,
    stamps: FileStamps,
    content_generation: u64,
}

type ObservedRequest = (u64, Vec<ProjectObservation>);

/// One project's answer. `None` is a folder that is not a repository, which
/// contributes no project entry at all. The paths are what the working-tree
/// sampler stats between reads.
type ProjectAnswer = (PathBuf, Option<ProjectWorktreesSnapshot>, Vec<PathBuf>);

const FILE_STATS_PER_WAKE: usize = 32;

pub struct WorktreeReader {
    inner: BackgroundRead<ObservedRequest, Vec<ProjectAnswer>>,
    /// Every path the last answer named, tagged with the requested root it
    /// belongs to.
    known_paths: Vec<(PathBuf, PathBuf)>,
    known_stamps: BTreeMap<PathBuf, (Option<std::time::SystemTime>, u64)>,
    scan_cursor: usize,
    content_generations: BTreeMap<PathBuf, u64>,
}

impl WorktreeReader {
    pub fn new() -> Self {
        Self {
            inner: BackgroundRead::on_change(Duration::ZERO, {
                let cache = std::sync::Mutex::new(ProjectCache::new());
                move |request: &ObservedRequest| read_observed(&cache, request)
            }),
            known_paths: Vec::new(),
            known_stamps: BTreeMap::new(),
            scan_cursor: 0,
            content_generations: BTreeMap::new(),
        }
    }

    pub fn read_if_due(&mut self, request: WorktreeRequest) -> Option<WorktreeCatalogSnapshot> {
        let observations = request
            .projects
            .iter()
            .map(|project| {
                let mut stamps = Vec::new();
                if let Some(repository) = crate::git_dir::discover(&project.root_path) {
                    for base in [repository.git_dir, repository.common_dir] {
                        for name in [
                            "HEAD",
                            "index",
                            "packed-refs",
                            "refs",
                            "worktrees",
                            "FETCH_HEAD",
                        ] {
                            stat_tree(&base.join(name), &mut stamps);
                        }
                    }
                }
                ProjectObservation {
                    request: project.clone(),
                    stamps,
                    content_generation: self
                        .content_generations
                        .get(&project.root_path)
                        .copied()
                        .unwrap_or(0),
                }
            })
            .collect();
        // Sample a bounded slice of the paths discovered by the worker. A
        // complete scan is spread across wakes, so content-only edits are
        // eventually observed without making coordinator latency grow with
        // repository size and without spawning a subprocess.
        let sample_count = FILE_STATS_PER_WAKE.min(self.known_paths.len());
        for offset in 0..sample_count {
            let index = (self.scan_cursor + offset) % self.known_paths.len();
            let (project, path) = self.known_paths[index].clone();
            let stamp = stamp(&path);
            match self.known_stamps.get_mut(&path) {
                Some(previous) if *previous != stamp => {
                    *previous = stamp;
                    let generation = self.content_generations.entry(project).or_insert(0);
                    *generation = generation.wrapping_add(1);
                }
                Some(_) => {}
                None => {
                    self.known_stamps.insert(path, stamp);
                }
            }
        }
        if !self.known_paths.is_empty() {
            self.scan_cursor = (self.scan_cursor + sample_count) % self.known_paths.len();
        }
        let answers = self.inner.poll((request.generation, observations))?;
        let mut projects: Vec<ProjectWorktreesSnapshot> = Vec::new();
        let mut paths = Vec::new();
        for (root, project, project_paths) in answers {
            paths.extend(project_paths.into_iter().map(|path| (root.clone(), path)));
            // Two registrations inside one repository describe one project.
            if let Some(project) = project
                && !projects
                    .iter()
                    .any(|existing| existing.root_path == project.root_path)
            {
                projects.push(project);
            }
        }
        if self.known_paths != paths {
            self.known_paths = paths;
            self.known_stamps.clear();
            self.scan_cursor = 0;
        }
        self.content_generations
            .retain(|root, _| request.projects.iter().any(|p| &p.root_path == root));
        Some(WorktreeCatalogSnapshot { projects })
    }
}

/// The last answer per requested root, with the observation that produced it.
type ProjectCache = BTreeMap<
    PathBuf,
    (
        u64,
        ProjectObservation,
        Option<ProjectWorktreesSnapshot>,
        Vec<PathBuf>,
    ),
>;

/// The worker's read: every project whose observation moved is read again,
/// every other one is answered from the last read. The cache lives with the
/// worker because only the worker produces what it holds; a project that
/// leaves the request leaves the cache with it.
fn read_observed(
    cache: &std::sync::Mutex<ProjectCache>,
    request: &ObservedRequest,
) -> Vec<ProjectAnswer> {
    let (generation, observations) = request;
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.retain(|root, _| observations.iter().any(|o| &o.request.root_path == root));
    observations
        .iter()
        .map(|observation| {
            let root = observation.request.root_path.clone();
            if let Some((cached_generation, cached, project, paths)) = cache.get(&root)
                && cached_generation == generation
                && cached == observation
            {
                return (root, project.clone(), paths.clone());
            }
            let (project, paths) = read_project_request(&observation.request);
            cache.insert(
                root.clone(),
                (
                    *generation,
                    observation.clone(),
                    project.clone(),
                    paths.clone(),
                ),
            );
            (root, project, paths)
        })
        .collect()
}

/// One project's rows and the working-tree paths behind them.
fn read_project_request(
    project: &WorktreeProjectRequest,
) -> (Option<ProjectWorktreesSnapshot>, Vec<PathBuf>) {
    let Some(mut snapshot) = read(project) else {
        return (None, Vec::new());
    };
    let mut paths = Vec::new();
    for row in &mut snapshot.worktrees {
        let root = PathBuf::from(&row.path);
        paths.push(root.clone());
        if !row.missing {
            match git(
                &root,
                &[
                    "ls-files",
                    "-z",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                ],
            ) {
                Ok(files) => {
                    for file in files.split('\0').filter(|f| !f.is_empty()) {
                        let path = root.join(file);
                        for parent in path.ancestors().take_while(|p| p.starts_with(&root)) {
                            paths.push(parent.to_owned());
                        }
                    }
                }
                Err(reason) => row.unavailable_reason = Some(reason),
            }
        }
    }
    paths.sort();
    paths.dedup();
    (Some(snapshot), paths)
}

impl Default for WorktreeReader {
    fn default() -> Self {
        Self::new()
    }
}

fn stat_one(path: &Path, stamps: &mut Vec<(PathBuf, Option<std::time::SystemTime>, u64)>) {
    let (modified, len) = stamp(path);
    stamps.push((path.to_owned(), modified, len));
}

fn stamp(path: &Path) -> (Option<std::time::SystemTime>, u64) {
    let meta = std::fs::metadata(path).ok();
    (
        meta.as_ref().and_then(|m| m.modified().ok()),
        meta.map_or(0, |m| m.len()),
    )
}

fn stat_tree(path: &Path, stamps: &mut Vec<(PathBuf, Option<std::time::SystemTime>, u64)>) {
    stat_one(path, stamps);
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(path) {
        let mut paths: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        paths.sort();
        for child in paths {
            stat_tree(&child, stamps);
        }
    }
}

/// The sole deletion policy, consumed by all three presentation surfaces.
pub fn deletion_gate(
    worktree: &WorktreeSnapshot,
    is_base: bool,
    pane_count: usize,
    running_agent_count: usize,
) -> WorktreeDeletionGateSnapshot {
    let blocked_reason = if worktree.is_main {
        Some("The main worktree cannot be deleted")
    } else if is_base {
        Some("The current base branch worktree cannot be deleted")
    } else if worktree.dirty {
        Some("Commit or discard uncommitted changes first")
    } else if worktree.nested {
        Some("Delete nested worktrees first")
    } else if worktree.unavailable_reason.is_some() && !worktree.missing {
        Some("Git status is unavailable")
    } else {
        None
    }
    .map(str::to_owned);
    let mut warnings = Vec::new();
    if !worktree.missing {
        if worktree.merged != Some(true) {
            warnings.push(format!("ahead {} unmerged", worktree.ahead));
        }
        if worktree.upstream_state != "pushed" {
            warnings.push("not pushed".to_owned());
        }
    }
    if running_agent_count > 0 {
        warnings.push(if running_agent_count == 1 {
            "1 running agent".to_owned()
        } else {
            format!("{running_agent_count} running agents")
        });
    }
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
        can_delete_branch: !worktree.missing
            && worktree.branch.is_some()
            && worktree.merged == Some(true),
    }
}

/// One project's rows: the host's Git facts with this machine's policy and
/// decoration slots on them. The facts come from `hide_host::worktrees`, the
/// same code a device's helper runs for a device's repository.
fn read(project: &WorktreeProjectRequest) -> Option<ProjectWorktreesSnapshot> {
    let facts = hide_host::worktrees::read(
        &project.root_path,
        &project.bases,
        project.base_override.as_deref(),
    )?;
    Some(project_snapshot(facts))
}

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
    facts: hide_host::worktrees::RepositoryWorktrees,
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

fn worktree_snapshot(facts: hide_host::worktrees::WorktreeFacts) -> WorktreeSnapshot {
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

pub(crate) use hide_host::worktrees::git;
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
}

#[cfg(test)]
#[path = "worktree_behavior_tests.rs"]
mod behavior_tests;
