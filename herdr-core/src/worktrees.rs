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
    ProjectWorktreesSnapshot, UnpushedSnapshot, WorktreeCatalogSnapshot, WorktreeSnapshot,
};
use crate::reader::BackgroundRead;

/// How stale the worktree list may be. A worktree is added, committed to, or
/// removed at human pace, so this is far slower than the changes view's
/// window and far cheaper than the per-tick fork it replaces.
const REFRESH_INTERVAL: Duration = Duration::from_secs(10);

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
}

pub struct WorktreeReader {
    inner: BackgroundRead<WorktreeRequest, WorktreeCatalogSnapshot>,
}

impl WorktreeReader {
    pub fn new() -> Self {
        Self {
            inner: BackgroundRead::new(REFRESH_INTERVAL, Duration::ZERO, read),
        }
    }

    pub fn read_if_due(&mut self, request: WorktreeRequest) -> Option<WorktreeCatalogSnapshot> {
        self.inner.poll(request)
    }
}

impl Default for WorktreeReader {
    fn default() -> Self {
        Self::new()
    }
}

fn read(request: &WorktreeRequest) -> WorktreeCatalogSnapshot {
    let mut projects: Vec<ProjectWorktreesSnapshot> = Vec::new();
    for project in &request.projects {
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
        projects.push(read_project(&root, root_path, &project.bases));
    }
    WorktreeCatalogSnapshot { projects }
}

fn read_project(
    root: &Path,
    root_path: String,
    bases: &BTreeMap<String, String>,
) -> ProjectWorktreesSnapshot {
    let default_branch = default_branch(root);
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
            };
        }
    };

    let mut worktrees: Vec<WorktreeSnapshot> = parse_worktree_list(&listed, root)
        .into_iter()
        .map(|listed| describe(listed, bases, default_branch.as_deref()))
        .collect();
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
    }
}

/// One record of `git worktree list --porcelain`, before any counting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListedWorktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub is_main: bool,
}

/// Splits porcelain worktree records. Records are separated by a blank line
/// and each begins with `worktree <path>`; `branch refs/heads/<name>` names
/// the checked-out branch, and a detached head has no such line.
///
/// The first record is the main worktree, which is git's documented order and
/// what tells a linked worktree apart without a second `rev-parse`.
pub fn parse_worktree_list(output: &str, _root: &Path) -> Vec<ListedWorktree> {
    let mut listed = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch: Option<String> = None;

    let flush = |path: &mut Option<PathBuf>,
                 branch: &mut Option<String>,
                 listed: &mut Vec<ListedWorktree>| {
        if let Some(path) = path.take() {
            let is_main = listed.is_empty();
            listed.push(ListedWorktree {
                path,
                branch: branch.take(),
                is_main,
            });
        } else {
            *branch = None;
        }
    };

    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("worktree ") {
            flush(&mut path, &mut branch, &mut listed);
            path = Some(PathBuf::from(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("branch ") {
            branch = Some(
                rest.trim()
                    .strip_prefix("refs/heads/")
                    .unwrap_or(rest.trim())
                    .to_owned(),
            );
        }
    }
    flush(&mut path, &mut branch, &mut listed);
    listed
}

fn describe(
    listed: ListedWorktree,
    bases: &BTreeMap<String, String>,
    default_branch: Option<&str>,
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
            ..WorktreeSnapshot::default()
        };
    }

    let (dirty, changed_file_count) = working_tree_state(&listed.path);
    let base_branch = resolve_base(listed.branch.as_deref(), bases, default_branch);
    let (ahead, behind) = base_branch
        .as_deref()
        .map(|base| ahead_behind(&listed.path, base))
        .unwrap_or((0, 0));
    let (added_lines, removed_lines) = base_branch
        .as_deref()
        .map(|base| committed_line_delta(&listed.path, base))
        .unwrap_or((0, 0));

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
        unpushed: unpushed(&listed.path),
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
fn working_tree_state(path: &Path) -> (bool, u32) {
    let Ok(output) = git(
        path,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--no-renames",
            "--untracked-files=all",
        ],
    ) else {
        return (false, 0);
    };
    let count = output
        .split('\0')
        .filter(|record| record.len() > 3)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    (count > 0, count)
}

/// Commits on this branch since the base, and commits on the base this branch
/// does not have. `base...HEAD` is the merge-base comparison the card's
/// `↑A ↓B` states, not a raw two-dot range.
fn ahead_behind(path: &Path, base: &str) -> (u32, u32) {
    let Some(base_ref) = resolvable_base(path, base) else {
        return (0, 0);
    };
    let Ok(output) = git(
        path,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{base_ref}...HEAD"),
        ],
    ) else {
        return (0, 0);
    };
    parse_ahead_behind(&output)
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

fn committed_line_delta(path: &Path, base: &str) -> (u32, u32) {
    let Some(base_ref) = resolvable_base(path, base) else {
        return (0, 0);
    };
    let Ok(output) = git(
        path,
        &["diff", "--shortstat", &format!("{base_ref}...HEAD")],
    ) else {
        return (0, 0);
    };
    parse_shortstat(&output)
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
fn unpushed(path: &Path) -> Option<UnpushedSnapshot> {
    let upstream = git(
        path,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .ok()?
    .trim()
    .to_owned();
    if upstream.is_empty() {
        return None;
    }
    let remote = upstream
        .split_once('/')
        .map(|(remote, _)| remote.to_owned())
        .unwrap_or(upstream);
    let count = git(path, &["rev-list", "--count", "@{u}..HEAD"])
        .ok()?
        .trim()
        .parse()
        .unwrap_or(0);
    Some(UnpushedSnapshot { remote, count })
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
            },
            &BTreeMap::new(),
            Some("main"),
        );
        assert!(described.missing);
        assert!(!described.dirty);
        assert_eq!(described.ahead, 0);
        assert_eq!(described.behind, 0);
        assert_eq!(described.unpushed, None);
    }
}
