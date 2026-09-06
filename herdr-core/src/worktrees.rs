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
use std::process::Command;
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
type ObservedRequest = (WorktreeRequest, FileStamps, u64);

const FILE_STATS_PER_WAKE: usize = 32;

pub struct WorktreeReader {
    inner: BackgroundRead<ObservedRequest, (WorktreeCatalogSnapshot, Vec<PathBuf>)>,
    known_paths: Vec<PathBuf>,
    known_stamps: BTreeMap<PathBuf, (Option<std::time::SystemTime>, u64)>,
    scan_cursor: usize,
    content_generation: u64,
}

impl WorktreeReader {
    pub fn new() -> Self {
        Self {
            inner: BackgroundRead::on_change(Duration::ZERO, |request: &ObservedRequest| {
                let mut catalog = read(&request.0);
                let mut paths = Vec::new();
                for row in catalog.projects.iter_mut().flat_map(|p| &mut p.worktrees) {
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
                                    for parent in
                                        path.ancestors().take_while(|p| p.starts_with(&root))
                                    {
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
                (catalog, paths)
            }),
            known_paths: Vec::new(),
            known_stamps: BTreeMap::new(),
            scan_cursor: 0,
            content_generation: 0,
        }
    }

    pub fn read_if_due(&mut self, request: WorktreeRequest) -> Option<WorktreeCatalogSnapshot> {
        let mut signature = Vec::new();
        for project in &request.projects {
            let dotgit = project.root_path.join(".git");
            let gitdir = if dotgit.is_file() {
                std::fs::read_to_string(&dotgit).ok().and_then(|text| {
                    text.strip_prefix("gitdir: ")
                        .map(|p| project.root_path.join(p.trim()))
                })
            } else {
                Some(dotgit)
            };
            if let Some(gitdir) = gitdir {
                let common = std::fs::read_to_string(gitdir.join("commondir"))
                    .ok()
                    .map(|p| gitdir.join(p.trim()))
                    .unwrap_or_else(|| gitdir.clone());
                for base in [gitdir, common] {
                    for name in [
                        "HEAD",
                        "index",
                        "packed-refs",
                        "refs",
                        "worktrees",
                        "FETCH_HEAD",
                    ] {
                        stat_tree(&base.join(name), &mut signature);
                    }
                }
            }
        }
        // Sample a bounded slice of the paths discovered by the worker. A
        // complete scan is spread across wakes, so content-only edits are
        // eventually observed without making coordinator latency grow with
        // repository size and without spawning a subprocess.
        let sample_count = FILE_STATS_PER_WAKE.min(self.known_paths.len());
        for offset in 0..sample_count {
            let index = (self.scan_cursor + offset) % self.known_paths.len();
            let path = self.known_paths[index].clone();
            let stamp = stamp(&path);
            match self.known_stamps.get_mut(&path) {
                Some(previous) if *previous != stamp => {
                    *previous = stamp;
                    self.content_generation = self.content_generation.wrapping_add(1);
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
        let answer = self
            .inner
            .poll((request, signature, self.content_generation));
        answer.map(|(catalog, paths)| {
            if self.known_paths != paths {
                self.known_paths = paths;
                self.known_stamps.clear();
                self.scan_cursor = 0;
                self.content_generation = self.content_generation.wrapping_add(1);
            }
            catalog
        })
    }
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
    remote: bool,
    pane_count: usize,
    running_agent_count: usize,
) -> WorktreeDeletionGateSnapshot {
    let blocked_reason = if remote {
        Some("Worktree deletion is available for local repositories only")
    } else if worktree.is_main {
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
        warnings.push(format!("{running_agent_count} running agents"));
    }
    WorktreeDeletionGateSnapshot {
        blocked_reason,
        warnings,
        button_label: if pane_count > 0 {
            format!("Close {pane_count} panes and delete")
        } else {
            "Delete worktree…".to_owned()
        },
        can_delete_branch: !worktree.missing
            && worktree.branch.is_some()
            && worktree.merged == Some(true),
    }
}

fn read(request: &WorktreeRequest) -> WorktreeCatalogSnapshot {
    let mut projects: Vec<ProjectWorktreesSnapshot> = Vec::new();
    for project in &request.projects {
        if !project.root_path.exists() {
            projects.push(ProjectWorktreesSnapshot {
                root_path: project.root_path.to_string_lossy().into_owned(),
                unavailable_reason: Some(format!(
                    "Repository unavailable: {}",
                    project.root_path.display()
                )),
                ..ProjectWorktreesSnapshot::default()
            });
            continue;
        }
        let Some(root) = main_worktree(&project.root_path) else {
            // A folder that is not a repository has no worktrees and is not a
            // failure; the card and the tree both present it as a plain
            // folder, so it contributes no project entry at all.
            continue;
        };
        let root_path = root.to_string_lossy().into_owned();
        if projects
            .iter()
            .any(|existing| existing.root_path == root_path)
        {
            continue;
        }
        projects.push(read_project(
            &root,
            root_path,
            &project.bases,
            project.base_override.as_deref(),
        ));
    }
    WorktreeCatalogSnapshot { projects }
}

fn read_project(
    root: &Path,
    root_path: String,
    bases: &BTreeMap<String, String>,
    base_override: Option<&str>,
) -> ProjectWorktreesSnapshot {
    let default_branch = default_branch(root);
    let valid_override = base_override.filter(|base| resolvable_base(root, base).is_some());
    let base_branch = valid_override
        .map(str::to_owned)
        .or_else(|| default_branch.clone());
    let base_branch_fallback = base_override
        .filter(|_| valid_override.is_none())
        .map(|base| format!("Base branch {base} is unavailable; using repository default"));
    let base_source = if valid_override.is_some() {
        "specified"
    } else {
        "default"
    }
    .to_owned();
    let listed = match git(root, &["worktree", "list", "--porcelain"]) {
        Ok(output) => output,
        Err(reason) => {
            eprintln!(
                "{}",
                serde_json::json!({
                    "component": "worktrees",
                    "kind": "worktree_list.failed",
                    "project": root_path,
                    "message": reason,
                })
            );
            return ProjectWorktreesSnapshot {
                root_path,
                default_branch,
                worktrees: Vec::new(),
                unavailable_reason: Some(reason),
                ..ProjectWorktreesSnapshot::default()
            };
        }
    };

    let mut worktrees: Vec<WorktreeSnapshot> = parse_worktree_list(&listed, root)
        .into_iter()
        .filter(|listed| !listed.bare)
        .map(|listed| {
            describe(
                listed,
                bases,
                base_branch.as_deref(),
                if base_override.is_some() {
                    base_branch.as_deref()
                } else {
                    None
                },
            )
        })
        .collect();
    let paths: Vec<PathBuf> = worktrees.iter().map(|w| PathBuf::from(&w.path)).collect();
    for worktree in &mut worktrees {
        worktree.nested = paths
            .iter()
            .any(|p| p != Path::new(&worktree.path) && p.starts_with(&worktree.path));
        worktree.deletion_gate = deletion_gate(
            worktree,
            worktree.branch == base_branch && base_branch.is_some(),
            false,
            0,
            0,
        );
    }
    // The main worktree leads so the project path and the first row agree.
    if let Some(index) = worktrees.iter().position(|worktree| worktree.is_main)
        && index != 0
    {
        let main = worktrees.remove(index);
        worktrees.insert(0, main);
    }
    ProjectWorktreesSnapshot {
        root_path,
        default_branch,
        worktrees,
        unavailable_reason: None,
        base_branch,
        base_branch_fallback,
        base_source,
    }
}

/// One record of `git worktree list --porcelain`, before any counting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListedWorktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub is_main: bool,
    pub bare: bool,
    pub head_sha: Option<String>,
}

/// Splits porcelain worktree records. Records are separated by a blank line
/// and each begins with `worktree <path>`; `branch refs/heads/<name>` names
/// the checked-out branch, and a detached head has no such line.
///
/// The first record is the main worktree, which is git's documented order and
/// what tells a linked worktree apart without a second `rev-parse`.
pub fn parse_worktree_list(output: &str, _root: &Path) -> Vec<ListedWorktree> {
    output
        .split("\n\n")
        .filter_map(|record| {
            let path = record
                .lines()
                .find_map(|line| line.strip_prefix("worktree "))?;
            Some(ListedWorktree {
                path: PathBuf::from(path),
                branch: record
                    .lines()
                    .find_map(|line| line.strip_prefix("branch refs/heads/"))
                    .map(str::to_owned),
                head_sha: record
                    .lines()
                    .find_map(|line| line.strip_prefix("HEAD "))
                    .map(str::to_owned),
                bare: record.lines().any(|line| line == "bare"),
                is_main: false,
            })
        })
        .enumerate()
        .map(|(index, mut row)| {
            row.is_main = index == 0;
            row
        })
        .collect()
}

fn describe(
    listed: ListedWorktree,
    bases: &BTreeMap<String, String>,
    default_branch: Option<&str>,
    base_override: Option<&str>,
) -> WorktreeSnapshot {
    let path = listed.path.to_string_lossy().into_owned();
    if !listed.path.exists() {
        // Nothing can be counted against a path that is not there, and
        // reporting zeros would read as a clean checkout rather than a gone
        // one. The row shows `missing` and stops.
        return WorktreeSnapshot {
            path,
            branch: listed.branch,
            missing: true,
            is_main: listed.is_main,
            head_sha: listed.head_sha,
            ..WorktreeSnapshot::default()
        };
    }

    let mut unavailable_reason = None;
    let (dirty, changed_file_count) = record_failure(
        working_tree_state(&listed.path),
        &mut unavailable_reason,
        (false, 0),
    );
    let base_branch = base_override
        .map(str::to_owned)
        .or_else(|| resolve_base(listed.branch.as_deref(), bases, default_branch));
    let (ahead, behind) = base_branch
        .as_deref()
        .map(|base| ahead_behind(&listed.path, base))
        .map(|result| record_failure(result, &mut unavailable_reason, (0, 0)))
        .unwrap_or((0, 0));
    let (added_lines, removed_lines) = base_branch
        .as_deref()
        .map(|base| committed_line_delta(&listed.path, base))
        .map(|result| record_failure(result, &mut unavailable_reason, (0, 0)))
        .unwrap_or((0, 0));

    let merged = base_branch
        .as_deref()
        .and_then(|base| resolvable_base(&listed.path, base))
        .and_then(|base| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&listed.path)
                .args(["merge-base", "--is-ancestor", "HEAD", &base])
                .output()
                .ok()?;
            match output.status.code() {
                Some(0) => Some(true),
                Some(1) => Some(false),
                _ => None,
            }
        });
    let (upstream_state, unpushed) = record_failure(
        upstream(&listed.path, listed.branch.as_deref()),
        &mut unavailable_reason,
        ("unavailable".into(), None),
    );
    let last_commit_unix_seconds = git(&listed.path, &["log", "-1", "--format=%ct"])
        .ok()
        .and_then(|s| s.trim().parse().ok());
    let last_fetch_at_unix_ms = git(
        &listed.path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()
    .and_then(|s| std::fs::metadata(Path::new(s.trim()).join("FETCH_HEAD")).ok())
    .and_then(|m| m.modified().ok())
    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
    .map(|d| d.as_millis() as u64);
    WorktreeSnapshot {
        path,
        branch: listed.branch,
        missing: false,
        is_main: listed.is_main,
        dirty,
        changed_file_count,
        base_branch,
        ahead,
        behind,
        added_lines,
        removed_lines,
        unpushed,
        upstream_state,
        merged,
        head_sha: listed.head_sha,
        last_commit_unix_seconds,
        last_fetch_at_unix_ms,
        measured_at_unix_ms: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        ),
        unavailable_reason,
        ..WorktreeSnapshot::default()
    }
}

fn record_failure<T>(
    result: Result<T, String>,
    unavailable_reason: &mut Option<String>,
    default: T,
) -> T {
    match result {
        Ok(value) => value,
        Err(reason) => {
            *unavailable_reason = Some(reason);
            default
        }
    }
}

/// What a branch is measured against: its pull request's base when `gh` has
/// named one, the repository default branch otherwise.
///
/// The branch a base names is not compared with itself - the default branch's
/// own worktree shows no base row - and a branch with neither a pull request
/// nor a known default gets no comparison rather than a misleading zero.
pub fn resolve_base(
    branch: Option<&str>,
    bases: &BTreeMap<String, String>,
    default_branch: Option<&str>,
) -> Option<String> {
    branch
        .and_then(|branch| bases.get(branch).cloned())
        .or_else(|| default_branch.map(str::to_owned))
        .filter(|base| branch != Some(base.as_str()))
}

/// Whether anything is uncommitted, and how many files that is.
fn working_tree_state(path: &Path) -> Result<(bool, u32), String> {
    let output = git(
        path,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--no-renames",
            "--untracked-files=all",
        ],
    )?;
    let count = output
        .split('\0')
        .filter(|record| record.len() > 3)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    Ok((count > 0, count))
}

/// Commits on this branch since the base, and commits on the base this branch
/// does not have. `base...HEAD` is the merge-base comparison the card's
/// `↑A ↓B` states, not a raw two-dot range.
fn ahead_behind(path: &Path, base: &str) -> Result<(u32, u32), String> {
    let base_ref =
        resolvable_base(path, base).ok_or_else(|| format!("Base branch {base} is unavailable"))?;
    let output = git(
        path,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{base_ref}...HEAD"),
        ],
    )?;
    Ok(parse_ahead_behind(&output))
}

/// `--left-right --count` prints `<behind>\t<ahead>`: the left side is the
/// base, so its count is what this branch is missing.
pub fn parse_ahead_behind(output: &str) -> (u32, u32) {
    let mut fields = output.split_whitespace();
    let behind = fields
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let ahead = fields
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    (ahead, behind)
}

fn committed_line_delta(path: &Path, base: &str) -> Result<(u32, u32), String> {
    let base_ref =
        resolvable_base(path, base).ok_or_else(|| format!("Base branch {base} is unavailable"))?;
    let output = git(
        path,
        &["diff", "--shortstat", &format!("{base_ref}...HEAD")],
    )?;
    Ok(parse_shortstat(&output))
}

/// `git diff --shortstat` prints, for example,
/// ` 3 files changed, 42 insertions(+), 7 deletions(-)`. Either half may be
/// missing when a change is all additions or all removals.
pub fn parse_shortstat(output: &str) -> (u32, u32) {
    let mut added = 0;
    let mut removed = 0;
    for part in output.split(',') {
        let part = part.trim();
        let Some(count) = part
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        if part.contains("insertion") {
            added = count;
        } else if part.contains("deletion") {
            removed = count;
        }
    }
    (added, removed)
}

/// The base branch as a ref this worktree can actually resolve.
///
/// A base is a branch name from a pull request or a repository default, so it
/// may exist only as a remote-tracking ref in this clone. The local branch is
/// preferred, the remote-tracking ref is the fallback, and a base that
/// resolves as neither yields no comparison rather than a silent zero.
fn resolvable_base(path: &Path, base: &str) -> Option<String> {
    for candidate in [base.to_owned(), format!("origin/{base}")] {
        if git(
            path,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{candidate}^{{commit}}"),
            ],
        )
        .is_ok()
        {
            return Some(candidate);
        }
    }
    None
}

/// Commits this branch has that its upstream does not, with the remote's name.
///
/// A branch with no upstream returns `None`: it has nowhere to push, which the
/// card states by omitting the row rather than by showing a zero that would
/// read as "fully pushed".
fn upstream(
    path: &Path,
    branch: Option<&str>,
) -> Result<(String, Option<UnpushedSnapshot>), String> {
    let Some(branch) = branch else {
        return Ok(("no_upstream".into(), None));
    };
    let configured = git(
        path,
        &[
            "for-each-ref",
            "--format=%(upstream)",
            &format!("refs/heads/{branch}"),
        ],
    )?;
    if configured.trim().is_empty() {
        return Ok(("no_upstream".into(), None));
    }
    if git(path, &["rev-parse", "--verify", "@{u}"]).is_err() {
        return Ok(("gone".into(), None));
    }
    let count = git(path, &["rev-list", "--count", "@{u}..HEAD"])
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    let Some(count) = count else {
        return Err("git rev-list could not determine upstream commit count".into());
    };
    let remote = configured
        .trim()
        .trim_start_matches("refs/remotes/")
        .split('/')
        .next()
        .unwrap_or("")
        .to_owned();
    Ok((
        if count == 0 { "pushed" } else { "unpushed" }.into(),
        Some(UnpushedSnapshot { remote, count }),
    ))
}

/// The repository's default branch: what `origin/HEAD` points at when the
/// clone knows, and the main worktree's own branch when it does not.
fn default_branch(root: &Path) -> Option<String> {
    if let Ok(output) = git(
        root,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) {
        let branch = output.trim().trim_start_matches("origin/").to_owned();
        if !branch.is_empty() {
            return Some(branch);
        }
    }
    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()?
        .trim()
        .to_owned();
    (!branch.is_empty() && branch != "HEAD").then_some(branch)
}

/// The repository's main working tree, which is what identifies a project.
fn main_worktree(path: &Path) -> Option<PathBuf> {
    let common = git(
        path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()?
    .trim()
    .to_owned();
    if common.is_empty() {
        return None;
    }
    PathBuf::from(common).parent().map(Path::to_path_buf)
}

fn git(cwd: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(cwd)
        .args(arguments)
        .output()
        .map_err(|error| format!("git could not be run: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!("git {} exited with {}", arguments[0], output.status)
        } else {
            format!("git {}: {stderr}", arguments[0])
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_records_name_the_main_worktree_and_each_branch() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\n\
                      worktree /repo.worktrees/feature\nHEAD def\nbranch refs/heads/feature\n\n";
        let listed = parse_worktree_list(output, Path::new("/repo"));
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
        let listed = parse_worktree_list(output, Path::new("/repo"));
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
        let listed = parse_worktree_list(output, Path::new("/repo"));
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
