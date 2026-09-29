//! One repository's worktrees on the machine that holds it: what Git says
//! about each (the facts the projects tree, the Overview and the deletion
//! gate read), the check a new branch passes before anything is created, and
//! the confirmed removal of one linked worktree (PRD S5.5 B27-B29), forced
//! only as far as the operator accepted.
//!
//! The core calls these in process for this machine; a device's helper
//! answers the same functions (`Call::Worktrees`, `Call::BranchCheck`,
//! `Call::WorktreeRemove`), so a local and a device repository obey one
//! rule set. The policy built on the facts - the deletion gate, pull request
//! bases, disk and agent decoration - stays with the core.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, HostError, HostResult};

/// The most one `git` invocation may take. A repository that cannot answer in
/// this time reports its status unavailable rather than holding every other
/// project's answer behind it; a status over evicted iCloud files ran for
/// minutes before this bound existed.
pub const GIT_DEADLINE: Duration = Duration::from_secs(15);

/// A repository's worktrees, as Git reports them. `None` from [`read`] is a
/// folder that is not a repository, which is not a failure.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepositoryWorktrees {
    pub root_path: String,
    pub shared_git_path: Option<String>,
    pub default_branch: Option<String>,
    pub branches: Vec<String>,
    pub base_branch: Option<String>,
    pub base_source: String,
    pub base_branch_fallback: Option<String>,
    pub worktrees: Vec<WorktreeFacts>,
    /// Why this repository has no worktree list. An empty list with no reason
    /// means the repository genuinely has none.
    pub unavailable_reason: Option<String>,
}

/// One worktree's Git facts.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorktreeFacts {
    pub path: String,
    pub branch: Option<String>,
    pub head_sha: Option<String>,
    /// Git lists the worktree but its path is not on disk.
    pub missing: bool,
    pub is_main: bool,
    /// Another listed worktree lies inside this one.
    pub nested: bool,
    pub dirty: bool,
    pub changed_file_count: u32,
    pub base_branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub added_lines: u32,
    pub removed_lines: u32,
    pub merged: Option<bool>,
    pub upstream_state: String,
    pub unpushed: Option<Unpushed>,
    pub behind_upstream: Option<u32>,
    pub created_at_unix_ms: Option<u64>,
    pub last_commit_unix_seconds: Option<u64>,
    pub last_commit_subject: Option<String>,
    pub last_fetch_at_unix_ms: Option<u64>,
    pub measured_at_unix_ms: Option<u64>,
    pub unavailable_reason: Option<String>,
}

/// Commits a branch has that its upstream does not, and the upstream's remote.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Unpushed {
    pub remote: String,
    pub count: u32,
}

/// Reads the repository that holds `path`: its main worktree, default
/// branch, local branches and every listed worktree. `bases` names a branch's
/// pull request base; `base_override` is the operator's chosen base.
pub fn read(
    path: &Path,
    bases: &BTreeMap<String, String>,
    base_override: Option<&str>,
) -> Option<RepositoryWorktrees> {
    if !path.exists() {
        return Some(RepositoryWorktrees {
            root_path: path.to_string_lossy().into_owned(),
            unavailable_reason: Some(format!("Repository unavailable: {}", path.display())),
            ..RepositoryWorktrees::default()
        });
    }
    // A folder that is not a repository has no worktrees and is not a
    // failure; the card and the tree both present it as a plain folder.
    let root = main_worktree(path)?;
    let root_path = root.to_string_lossy().into_owned();
    Some(read_project(&root, root_path, bases, base_override))
}

/// Reads `root`, already known to be a main worktree.
pub fn read_project(
    root: &Path,
    root_path: String,
    bases: &BTreeMap<String, String>,
    base_override: Option<&str>,
) -> RepositoryWorktrees {
    let default_branch = default_branch(root);
    let branches = local_branches(root);
    let valid_override = base_override.filter(|base| resolvable_base(root, base).is_some());
    let base_branch = valid_override
        .map(str::to_owned)
        .or_else(|| default_branch.clone());
    let base_branch_fallback = base_override
        .filter(|_| valid_override.is_none())
        .map(|base| {
            if default_branch.is_some() {
                format!("Base branch {base} is unavailable; using repository default")
            } else {
                format!("Base branch {base} is unavailable; repository base is unknown")
            }
        });
    let base_source = if valid_override.is_some() {
        "specified"
    } else if default_branch.is_some() {
        "origin_head"
    } else {
        "unknown"
    }
    .to_owned();
    let listed = match git(root, &["worktree", "list", "--porcelain"]) {
        Ok(output) => output,
        Err(reason) => {
            return RepositoryWorktrees {
                root_path,
                default_branch,
                unavailable_reason: Some(reason),
                ..RepositoryWorktrees::default()
            };
        }
    };
    let mut worktrees: Vec<WorktreeFacts> = parse_worktree_list(&listed)
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
    }
    // The main worktree leads so the project path and the first row agree.
    if let Some(index) = worktrees.iter().position(|worktree| worktree.is_main)
        && index != 0
    {
        let main = worktrees.remove(index);
        worktrees.insert(0, main);
    }
    RepositoryWorktrees {
        shared_git_path: git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .ok()
        .map(|value| value.trim().to_owned()),
        root_path,
        default_branch,
        branches,
        base_branch,
        base_source,
        base_branch_fallback,
        worktrees,
        unavailable_reason: None,
    }
}

fn local_branches(root: &Path) -> Vec<String> {
    git(
        root,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .map(|output| {
        let mut branches = output
            .lines()
            .map(str::trim)
            .filter(|branch| !branch.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        branches.sort();
        branches.dedup();
        branches
    })
    .unwrap_or_default()
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
pub fn parse_worktree_list(output: &str) -> Vec<ListedWorktree> {
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
                    .filter(|sha| sha.chars().any(|character| character != '0'))
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

/// The facts of one listed worktree.
pub fn describe(
    listed: ListedWorktree,
    bases: &BTreeMap<String, String>,
    default_branch: Option<&str>,
    base_override: Option<&str>,
) -> WorktreeFacts {
    let path = listed.path.to_string_lossy().into_owned();
    if !listed.path.exists() {
        // Nothing can be counted against a path that is not there, and
        // reporting zeros would read as a clean checkout rather than a gone
        // one. The row shows `missing` and stops.
        return WorktreeFacts {
            path,
            branch: listed.branch,
            missing: true,
            is_main: listed.is_main,
            head_sha: listed.head_sha,
            ..WorktreeFacts::default()
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
            let output = output_within(
                Command::new("git").arg("-C").arg(&listed.path).args([
                    "merge-base",
                    "--is-ancestor",
                    "HEAD",
                    &base,
                ]),
                GIT_DEADLINE,
            )
            .ok()??;
            match output.status.code() {
                Some(0) => Some(true),
                Some(1) => Some(false),
                _ => None,
            }
        });
    let (upstream_state, unpushed, behind_upstream) = record_failure(
        upstream(&listed.path, listed.branch.as_deref()),
        &mut unavailable_reason,
        ("unavailable".into(), None, None),
    );
    // A linked worktree is as old as its `.git/worktrees/<name>` entry, which
    // git writes once at `worktree add` and never touches again. It is read
    // off the gitfile, not asked of git, so the pass forks nothing extra for
    // it; the main worktree has no such entry and carries no time.
    let created_at_unix_ms = if listed.is_main {
        None
    } else {
        hide_project::git::discover(&listed.path)
            .and_then(|repository| std::fs::metadata(repository.git_dir).ok())
            .and_then(|metadata| metadata.created().ok())
            .and_then(|created| created.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis() as u64)
    };
    let last_commit = git(&listed.path, &["log", "-1", "--format=%ct%x00%s"]).ok();
    let commit_fields = last_commit
        .as_deref()
        .and_then(|value| value.trim_end().split_once('\0'));
    let last_commit_unix_seconds = commit_fields.and_then(|(time, _)| time.parse().ok());
    let last_commit_subject = commit_fields.map(|(_, subject)| subject.to_owned());
    let last_fetch_at_unix_ms = git(
        &listed.path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()
    .and_then(|s| std::fs::metadata(Path::new(s.trim()).join("FETCH_HEAD")).ok())
    .and_then(|m| m.modified().ok())
    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
    .map(|d| d.as_millis() as u64);
    WorktreeFacts {
        path,
        branch: listed.branch,
        missing: false,
        is_main: listed.is_main,
        nested: false,
        dirty,
        changed_file_count,
        base_branch,
        ahead,
        behind,
        added_lines,
        removed_lines,
        unpushed,
        upstream_state,
        behind_upstream,
        created_at_unix_ms,
        merged,
        head_sha: listed.head_sha,
        last_commit_unix_seconds,
        last_commit_subject,
        last_fetch_at_unix_ms,
        measured_at_unix_ms: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        ),
        unavailable_reason,
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

/// Commits this branch has that its upstream does not, with the remote's
/// name, and the commits the upstream has that this branch does not.
///
/// A branch with no upstream returns `None` for both: it has nowhere to push
/// and nothing to be behind, which the card states by omitting the row rather
/// than by showing a zero that would read as "fully pushed" or "up to date".
/// One `rev-list --left-right --count @{u}...HEAD` answers both directions,
/// the same shape `ahead_behind` already reads against the base, so the
/// second number costs no second process.
type UpstreamFacts = (String, Option<Unpushed>, Option<u32>);

fn upstream(path: &Path, branch: Option<&str>) -> Result<UpstreamFacts, String> {
    let Some(branch) = branch else {
        return Ok(("no_upstream".into(), None, None));
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
        return Ok(("no_upstream".into(), None, None));
    }
    if git(path, &["rev-parse", "--verify", "@{u}"]).is_err() {
        return Ok(("gone".into(), None, None));
    }
    let counts = git(
        path,
        &["rev-list", "--left-right", "--count", "@{u}...HEAD"],
    )
    .ok()
    .and_then(|output| parse_left_right(&output));
    let Some((behind, count)) = counts else {
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
        Some(Unpushed { remote, count }),
        Some(behind),
    ))
}

/// `--left-right --count` prints `<left>\t<right>`; both fields have to be
/// there and numeric, or the answer is no answer rather than a pair of zeros.
fn parse_left_right(output: &str) -> Option<(u32, u32)> {
    let mut fields = output.split_whitespace();
    let left = fields.next()?.parse().ok()?;
    let right = fields.next()?.parse().ok()?;
    Some((left, right))
}

/// The repository's default branch is only what `origin/HEAD` names.
///
/// The main worktree's current branch is deliberately excluded: using it as
/// a base would make a feature branch look like project policy and hide the
/// very mismatch the migration action exists to explain.
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
    None
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

/// Every `git` this module runs, by directory and subcommand, so a test can
/// assert that a quiet repository is not read again. Compiled only with the
/// `call-log` feature, which the core enables for its own tests.
#[cfg(feature = "call-log")]
pub static GIT_CALLS: std::sync::Mutex<Vec<(PathBuf, String)>> = std::sync::Mutex::new(Vec::new());

/// Runs `git` in `cwd` within [`GIT_DEADLINE`], answering its stdout or a
/// one-line reason.
pub fn git(cwd: &Path, arguments: &[&str]) -> Result<String, String> {
    git_within(cwd, arguments, GIT_DEADLINE)
}

/// [`git`] with its own bound, for the one command that may legitimately
/// outlast a read: a removal that deletes the folder in place.
fn git_within(cwd: &Path, arguments: &[&str], deadline: Duration) -> Result<String, String> {
    #[cfg(feature = "call-log")]
    GIT_CALLS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push((cwd.to_owned(), arguments.first().unwrap_or(&"").to_string()));
    let output = output_within(
        Command::new("git")
            .arg("--no-optional-locks")
            .arg("-C")
            .arg(cwd)
            .args(arguments),
        deadline,
    )
    .map_err(|error| format!("git could not be run: {error}"))?;
    let Some(output) = output else {
        return Err(format!(
            "git {} did not finish within {} s",
            arguments[0],
            deadline.as_secs()
        ));
    };
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

/// The first ignored folder of the worktree that holds a Git repository of
/// its own, relative to the worktree.
pub fn ignored_repository(worktree: &Path) -> Result<Option<String>, String> {
    let listed = git(
        worktree,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
        ],
    )?;
    let mut budget = WalkBudget {
        entries: IGNORED_WALK_ENTRIES,
        until: std::time::Instant::now() + IGNORED_WALK_TIME,
    };
    for folder in listed.split('\0').filter(|entry| entry.ends_with('/')) {
        if holds_repository(&worktree.join(folder), &mut budget)? {
            return Ok(Some(folder.trim_end_matches('/').to_owned()));
        }
    }
    Ok(None)
}

/// How much of the ignored folders one removal looks through for a nested
/// repository. A walk that runs out, or a folder it cannot read, refuses the
/// removal: `git worktree remove` would delete what was not looked at.
const IGNORED_WALK_ENTRIES: usize = 2_000_000;
const IGNORED_WALK_TIME: Duration = Duration::from_secs(30);

struct WalkBudget {
    entries: usize,
    until: std::time::Instant,
}

/// Whether `folder` or any folder below it holds a `.git`. Links are not
/// followed: removal deletes the link, not what it points to.
fn holds_repository(folder: &Path, budget: &mut WalkBudget) -> Result<bool, String> {
    let unchecked = |reason: String| {
        format!(
            "The worktree was not removed: {reason}, so Hide could not confirm it holds no other Git repository. Remove it in the terminal after checking."
        )
    };
    let mut pending = vec![folder.to_path_buf()];
    while let Some(folder) = pending.pop() {
        if std::fs::symlink_metadata(folder.join(".git")).is_ok() {
            return Ok(true);
        }
        let entries = std::fs::read_dir(&folder).map_err(|error| {
            unchecked(format!("{} could not be read ({error})", folder.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                unchecked(format!("{} could not be read ({error})", folder.display()))
            })?;
            budget.entries = budget.entries.checked_sub(1).ok_or_else(|| {
                unchecked(format!(
                    "its ignored folders hold more than {IGNORED_WALK_ENTRIES} entries"
                ))
            })?;
            if std::time::Instant::now() >= budget.until {
                return Err(unchecked(format!(
                    "its ignored folders took longer than {} seconds to look through",
                    IGNORED_WALK_TIME.as_secs()
                )));
            }
            let kind = entry.file_type().map_err(|error| {
                unchecked(format!(
                    "{} could not be read ({error})",
                    entry.path().display()
                ))
            })?;
            if kind.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Ok(false)
}

/// `Command::output` within [`GIT_DEADLINE`], for the branch checks.
trait WithinDeadline {
    fn output_within_deadline(&mut self) -> Result<std::process::Output, String>;
}

impl WithinDeadline for Command {
    fn output_within_deadline(&mut self) -> Result<std::process::Output, String> {
        output_within(self, GIT_DEADLINE)
            .map_err(|error| format!("git could not be run: {error}"))?
            .ok_or_else(|| {
                format!(
                    "git did not finish within {} seconds and was stopped",
                    GIT_DEADLINE.as_secs()
                )
            })
    }
}

/// Runs `command` to completion, or kills it at `deadline` and answers
/// `None`. Both pipes are drained on their own threads the whole time, so a
/// child whose output outgrows the pipe buffer is never left blocked on a
/// write nobody reads. The child leads its own process group and the deadline
/// stops the whole group, so a hook, filter or fsmonitor Git started cannot
/// keep a pipe open past it; a descendant that left the group is waited for
/// only a moment and then left behind with its pipe.
pub fn output_within(
    command: &mut Command,
    deadline: Duration,
) -> std::io::Result<Option<std::process::Output>> {
    use std::io::Read;
    use std::process::Stdio;
    use std::sync::mpsc;

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            if let Some(mut pipe) = pipe {
                let _: std::io::Result<usize> = pipe.read_to_end(&mut buffer);
            }
            let _ = sender.send(buffer);
        });
        receiver
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if started.elapsed() >= deadline {
            kill_group(&mut child)?;
            child.wait()?;
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let drained = std::time::Instant::now().max(started + deadline) + Duration::from_secs(1);
    let collect = |receiver: mpsc::Receiver<Vec<u8>>| {
        receiver
            .recv_timeout(drained.saturating_duration_since(std::time::Instant::now()))
            .ok()
    };
    let (stdout, stderr) = (collect(stdout), collect(stderr));
    // A pipe still open once git has ended is held by something it started,
    // so what was read may be cut short: the call did not finish, never an
    // empty answer a safety check would read as clean.
    let (Some(status), Some(stdout), Some(stderr)) = (status, stdout, stderr) else {
        // The deadline path has already stopped the group.
        if status.is_some() {
            let _ = kill_group(&mut child);
        }
        return Ok(None);
    };
    Ok(Some(std::process::Output {
        status,
        stdout,
        stderr,
    }))
}

/// Stops `child` and every process in its group, which it leads when it was
/// spawned by [`output_within`] or the diff reader.
pub(crate) fn kill_group(child: &mut std::process::Child) -> std::io::Result<()> {
    if stop_group(child.id()) {
        return Ok(());
    }
    child.kill()
}

/// Sends SIGKILL to the process group `leader` leads; whether one was there.
/// A group id is not reused while its leader is unreaped or any member lives,
/// so this reaches only what that child started, even after the child itself
/// has ended; once every member has left, the id could name a later group,
/// which needs the process ids to wrap within the one second a held pipe is
/// waited for.
pub(crate) fn stop_group(leader: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: killpg only sends a signal.
        unsafe { libc::killpg(leader as libc::pid_t, libc::SIGKILL) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = leader;
        false
    }
}

// --- creation --------------------------------------------------------------

/// Git's own rule for a new branch name, then a refusal when the branch
/// already exists and no worktree holds it, asked before anything is created
/// so a refused name leaves the repository untouched. A branch another
/// worktree holds passes: Herdr's `worktree.create` answers Git's own
/// "already used by worktree" refusal for it.
pub fn check_new_branch(repository_root: &Path, branch: &str) -> HostResult<()> {
    // A leading `-` is refused first because Git would read it as an option.
    let invalid = || {
        HostError::new(
            ErrorCode::InvalidRequest,
            format!("{branch} is not a valid branch name"),
        )
    };
    if branch.starts_with('-') {
        return Err(invalid());
    }
    let checked = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(repository_root)
        .args(["check-ref-format", "--branch", branch])
        .output_within_deadline()
        .map_err(io_error)?;
    if !checked.status.success() {
        return Err(invalid());
    }
    let reference = format!("refs/heads/{branch}");
    let exists = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(repository_root)
        .args(["show-ref", "--verify", "--quiet", &reference])
        .output_within_deadline()
        .map_err(io_error)?;
    if !exists.status.success() {
        return match exists.status.code() {
            Some(1) => Ok(()),
            _ => Err(io_error(
                String::from_utf8_lossy(&exists.stderr).trim().to_owned(),
            )),
        };
    }
    let worktrees = git(repository_root, &["worktree", "list", "--porcelain"]).map_err(io_error)?;
    if worktrees
        .lines()
        .any(|line| line == format!("branch {reference}"))
    {
        return Ok(());
    }
    // Ask Git itself for the branch-exists diagnostic. This command is
    // side-effect free because the branch was proven to exist above.
    let refusal = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(repository_root)
        .args(["branch", "--", branch])
        .output_within_deadline()
        .map_err(io_error)?;
    if refusal.status.success() {
        return Err(io_error(
            "git branch existence check unexpectedly succeeded".into(),
        ));
    }
    let detail = String::from_utf8_lossy(&refusal.stderr).trim().to_owned();
    Err(HostError::new(
        ErrorCode::AlreadyExists,
        if detail.is_empty() {
            format!("git branch exited with {}", refusal.status)
        } else {
            detail
        },
    ))
}

/// The real path of an existing directory, or `None`; how the core confirms
/// that the folder Herdr says it created is there and is the one it listed.
pub fn directory(path: &Path) -> Option<String> {
    let real = std::fs::canonicalize(path).ok()?;
    real.is_dir().then(|| real.to_string_lossy().into_owned())
}

// --- removal ---------------------------------------------------------------

/// One linked worktree as Git registers it, read with NUL porcelain so no
/// folder name is interpreted.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Registered {
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub locked: bool,
    /// Prunable or bare: Git lists it but cannot use it.
    pub unavailable: bool,
}

/// Every worktree Git registers for `root`, the main worktree first.
pub fn registered(root: &Path) -> Result<Vec<Registered>, String> {
    let text = git(root, &["worktree", "list", "--porcelain", "-z"])?;
    let mut rows = Vec::new();
    for record in text.split("\0\0").filter(|v| !v.is_empty()) {
        let fields: Vec<_> = record.split('\0').collect();
        let path = fields
            .iter()
            .find_map(|v| v.strip_prefix("worktree "))
            .ok_or("Git returned an invalid worktree record. Refresh the review.")?;
        rows.push(Registered {
            path: path.into(),
            branch: fields
                .iter()
                .find_map(|v| v.strip_prefix("branch refs/heads/"))
                .map(str::to_owned),
            head: fields
                .iter()
                .find_map(|v| v.strip_prefix("HEAD "))
                .map(str::to_owned),
            locked: fields.iter().any(|v| v.starts_with("locked")),
            unavailable: fields
                .iter()
                .any(|v| v.starts_with("prunable") || *v == "bare"),
        });
    }
    if rows.is_empty() {
        return Err("Git returned no main checkout. Refresh the review.".into());
    }
    Ok(rows)
}

/// Removes the worktree folder and its registration, keeping the branch.
/// `force` removes it with uncommitted changes too, which the operator must
/// have accepted.
///
/// Every build cache the checkout owns lives inside it (`target/`,
/// `web/dist/`), so this is the whole cleanup. Deleting a multi-GB build
/// folder takes longer than any Git read may, so the folder is first moved
/// aside into the repository's [`TRASH`] folder, one rename on the same
/// volume; `git worktree remove` then finds it missing and drops only the
/// registration, and the files are deleted in the background
/// ([`sweep_trash`]). The folder is moved only when Git would remove it -
/// registered, not the main worktree, not locked, and clean unless `force` -
/// so every refusal is still Git's own. A folder that cannot be moved (the
/// checkout sits on another volume than its repository) is removed by Git in
/// place, within [`REMOVE_DEADLINE`].
pub fn remove_worktree(
    repository_root: &Path,
    checkout: &Path,
    force: bool,
) -> Result<String, String> {
    let common = git(
        repository_root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()
    .map(|path| PathBuf::from(path.trim()));
    let set_aside = common
        .as_deref()
        .and_then(|common| set_aside(repository_root, common, checkout, force));
    let checkout_arg = checkout.to_string_lossy();
    let mut arguments = vec!["worktree", "remove"];
    if force {
        arguments.push("--force");
    }
    arguments.extend(["--", &checkout_arg]);
    // Moved aside, Git only drops the registration, which is a quick write;
    // only a folder left in place needs the removal's own bound.
    let deadline = if set_aside.is_some() {
        GIT_DEADLINE
    } else {
        REMOVE_DEADLINE
    };
    let answer = git_within(repository_root, &arguments, deadline);
    // A folder moved aside goes back while Git still registers it, so a
    // refused removal leaves the worktree exactly where it was; the same
    // rule decides what a sweep may delete.
    if let Some(aside) = set_aside.as_ref()
        && still_registered(&aside.admin)
    {
        std::fs::rename(&aside.entry, checkout).map_err(|error| {
            format!(
                "Git kept the worktree but its folder could not be moved back from {} ({error}). Move it back to {} before retrying.",
                aside.entry.display(),
                checkout.display()
            )
        })?;
    }
    // Every removal sweeps, so an entry a stopped process left behind goes
    // with the next removal in its repository, including its own retry.
    if let Some(common) = common.as_deref() {
        sweep_trash(&common.join(TRASH));
    }
    // What is on disk decides, so an answer cut short (a held pipe) after
    // Git removed the folder still reads as removed, and a failure whose
    // readback also fails keeps Git's own reason.
    let listed = registered(repository_root)
        .map(|rows| rows.iter().any(|row| Path::new(&row.path) == checkout));
    let remains = match (listed, &answer) {
        (Ok(listed), Err(_)) => listed || checkout.try_exists().unwrap_or(true),
        (Ok(listed), Ok(_)) => {
            listed || checkout.try_exists().map_err(|error| error.to_string())?
        }
        (Err(_), Err(_)) => true,
        (Err(error), Ok(_)) => return Err(error),
    };
    match (answer, remains) {
        (_, false) => {
            Ok("Worktree folder and its build output removed. Branch and Git history kept.".into())
        }
        (Err(error), true) => Err(error),
        (Ok(_), true) => Err(
            "Git acknowledged removal but the folder or registration remains. Review again.".into(),
        ),
    }
}

/// The bound on a `git worktree remove` that deletes the folder itself,
/// which only a checkout that could not be moved aside needs. It stays under
/// the core's wait for the removal's answer, so a removal that runs out is
/// reported by the host rather than left unknown.
pub const REMOVE_DEADLINE: Duration = Duration::from_secs(60);

/// The folder under the repository's shared Git directory where removed
/// worktree folders wait to be deleted. Git ignores it, the operator never
/// browses it, and it is on the checkout's volume in the usual layout.
pub const TRASH: &str = "hide-removed";

/// A worktree folder moved into its repository's [`TRASH`]: the entry the
/// folder now is, and Git's administrative folder that registers it.
struct SetAside {
    entry: PathBuf,
    admin: PathBuf,
}

/// Moves `checkout` into the repository's [`TRASH`] when Git would remove
/// it. The entry is named `<nanos>-<pid>-<admin id>`, so [`sweep_trash`] can
/// tell from Git's own registration whether it is still a worktree. `None`
/// leaves the folder where it is for Git to remove or refuse in place.
fn set_aside(root: &Path, common: &Path, checkout: &Path, force: bool) -> Option<SetAside> {
    let rows = registered(root).ok()?;
    let index = rows
        .iter()
        .position(|row| Path::new(&row.path) == checkout)?;
    if index == 0 || rows[index].locked || rows[index].unavailable {
        return None;
    }
    if !std::fs::symlink_metadata(checkout).ok()?.is_dir() {
        return None;
    }
    // Git's own check before it deletes a folder it was not forced to.
    if !force
        && !git(
            checkout,
            &["status", "--porcelain", "--ignore-submodules=none"],
        )
        .ok()?
        .trim()
        .is_empty()
    {
        return None;
    }
    let id = admin_id(checkout)?;
    let admin = common.join("worktrees").join(&id);
    if !admin.is_dir() {
        return None;
    }
    let trash = common.join(TRASH);
    std::fs::create_dir_all(&trash).ok()?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let entry = trash.join(format!("{nanos}-{}-{id}", std::process::id()));
    std::fs::rename(checkout, &entry).ok()?;
    Some(SetAside { entry, admin })
}

/// The name of the worktree's administrative folder, from the `gitdir:`
/// line of its `.git` file.
fn admin_id(checkout: &Path) -> Option<String> {
    let text = std::fs::read_to_string(checkout.join(".git")).ok()?;
    let gitdir = text
        .lines()
        .find_map(|line| line.strip_prefix("gitdir: "))?;
    Path::new(gitdir.trim())
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

/// Moves a folder that is not a worktree into the repository's [`TRASH`] with
/// one rename, so it is gone from its checkout at once and [`sweep_trash`]
/// deletes it in the background. The rename stays on one volume, so a folder
/// on another volume than the repository's Git directory is an error and is
/// left where it is. The entry's name has no registered worktree id, so a
/// sweep always treats it as deletable.
pub fn set_aside_folder(common: &Path, folder: &Path) -> Result<PathBuf, std::io::Error> {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let trash = common.join(TRASH);
    std::fs::create_dir_all(&trash)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let entry = trash.join(format!("{nanos}-{}-~folder{sequence}", std::process::id()));
    std::fs::rename(folder, &entry)?;
    Ok(entry)
}

/// Starts the deletion of everything waiting in the repository's [`TRASH`]
/// and waits up to `wait` for it to finish, sweeping again every few seconds
/// so a deletion that stopped is retried. Returns how many entries remain.
pub fn drain_trash(common: &Path, wait: Duration) -> usize {
    let trash = common.join(TRASH);
    let until = std::time::Instant::now() + wait;
    let mut swept = None;
    loop {
        if swept.is_none_or(|at: std::time::Instant| at.elapsed() >= Duration::from_secs(5)) {
            sweep_trash(&trash);
            swept = Some(std::time::Instant::now());
        }
        let remaining = std::fs::read_dir(&trash).map_or(0, Iterator::count);
        if remaining == 0 || std::time::Instant::now() >= until {
            return remaining;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Whether `folder` or any folder below it holds a `.git`, looked through
/// within the same bounds a worktree removal uses.
pub fn contains_repository(folder: &Path) -> Result<bool, String> {
    holds_repository(
        folder,
        &mut WalkBudget {
            entries: IGNORED_WALK_ENTRIES,
            until: std::time::Instant::now() + IGNORED_WALK_TIME,
        },
    )
}

/// Entries of a trash folder being deleted by this process, so two sweeps
/// never delete the same folder at once.
static DELETING: std::sync::Mutex<std::collections::BTreeSet<PathBuf>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// Deletes, each on its own thread, every entry of `trash` that is no longer
/// a worktree: Git has no administrative folder for it, or that folder now
/// belongs to another worktree whose `.git` exists. An entry Git still
/// registers - a removal that has not reached Git yet, or one whose folder
/// could not be moved back - is kept.
pub fn sweep_trash(trash: &Path) {
    let Some(common) = trash.parent() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(trash) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(admin) = name.splitn(3, '-').nth(2) else {
            continue;
        };
        if still_registered(&common.join("worktrees").join(admin)) {
            continue;
        }
        if !DELETING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(path.clone())
        {
            continue;
        }
        let spawned = std::thread::Builder::new()
            .name("hide-worktree-trash".into())
            .spawn({
                let path = path.clone();
                move || {
                    let result = std::fs::remove_dir_all(&path);
                    DELETING
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .remove(&path);
                    if let Err(error) = result
                        && error.kind() != std::io::ErrorKind::NotFound
                    {
                        eprintln!(
                            "{}",
                            serde_json::json!({
                                "component": "worktree_trash",
                                "kind": "delete_failed",
                                "entry": path,
                                "message": error.to_string(),
                            })
                        );
                    }
                }
            });
        if spawned.is_err() {
            DELETING
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&path);
        }
    }
}

/// Whether Git's administrative folder `admin` still registers the folder
/// that was moved away from it: it exists and the `.git` it names is gone.
fn still_registered(admin: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(admin.join("gitdir")) else {
        // No readable registration: missing means removed; anything else is
        // kept rather than guessed at.
        return admin.exists();
    };
    let named = Path::new(text.trim());
    let named = if named.is_absolute() {
        named.to_path_buf()
    } else {
        admin.join(named)
    };
    !named.exists()
}

/// One operator-confirmed worktree deletion, as the runtime recorded it when
/// the operator confirmed and Herdr then confirmed every pane was gone.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfirmedRemoval {
    pub repository_root: String,
    pub checkout_path: String,
    pub expected_head_sha: Option<String>,
    pub expected_branch: Option<String>,
    pub protected_base_branch: Option<String>,
    /// The branch to delete after the folder is gone; `None` keeps it.
    pub delete_branch: Option<String>,
    /// Delete the branch with `git branch -D`: the operator was told it
    /// holds commits the base does not. Otherwise `-d`, which keeps an
    /// unmerged branch. Absent from an older sender, which means `-d`.
    #[serde(default)]
    pub force_delete_branch: bool,
    /// The operator accepted losing the folder's changes: dirt, a worktree
    /// inside it and an ignored repository no longer stop the removal, and
    /// Git removes with `--force`. Absent from an older sender, which means
    /// none of that is accepted.
    #[serde(default)]
    pub discard_changes: bool,
}

/// What a confirmed removal did, as the helper answers it: a stopped
/// removal is an answer too, distinct from a request that got none.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemovalOutcome {
    /// Whether the folder is gone; `false` means the worktree remains.
    pub removed: bool,
    pub message: String,
}

impl From<Result<String, String>> for RemovalOutcome {
    fn from(result: Result<String, String>) -> Self {
        match result {
            Ok(message) => Self {
                removed: true,
                message,
            },
            Err(message) => Self {
                removed: false,
                message,
            },
        }
    }
}

/// Rechecks the confirmed target against Git's registration and removes it.
/// The recheck is the last line between a confirmation the operator gave
/// minutes ago and the folder as it is now: a moved HEAD or a branch that
/// became the protected base stops the removal and leaves the folder where it
/// is. A nested worktree, a dirty file or an ignored repository stops it too,
/// unless the operator accepted discarding them, in which case Git removes
/// with `--force`.
///
/// The branch is deleted with `-d` unless the operator was told it is
/// unmerged (`-D`); a `-d` that Git refuses keeps the branch and reports
/// why, and the folder's removal still stands.
pub fn remove_confirmed(request: &ConfirmedRemoval) -> Result<String, String> {
    let root = Path::new(&request.repository_root);
    let target = Path::new(&request.checkout_path);
    let stopped = |detail: String| {
        format!(
            "Worktree removal stopped: {detail}. The worktree remains; panes already closed stay closed."
        )
    };
    let rows = registered(root).map_err(|error| {
        stopped(format!(
            "could not re-read the worktree registration: {error}"
        ))
    })?;
    let Some(index) = rows.iter().position(|row| Path::new(&row.path) == target) else {
        return Err(stopped(
            "the worktree is no longer registered at this path".into(),
        ));
    };
    if index == 0 {
        return Err(stopped("the main worktree cannot be deleted".into()));
    }
    let current = &rows[index];
    if current.head != request.expected_head_sha || current.branch != request.expected_branch {
        return Err(stopped(
            "the worktree identity changed after confirmation".into(),
        ));
    }
    if current.branch.is_some() && current.branch == request.protected_base_branch {
        return Err(stopped(
            "the worktree now holds the protected base branch".into(),
        ));
    }
    if !request.discard_changes
        && rows
            .iter()
            .any(|row| Path::new(&row.path) != target && Path::new(&row.path).starts_with(target))
    {
        return Err(stopped(
            "the worktree now contains a nested worktree".into(),
        ));
    }
    if !request.discard_changes
        && target
            .try_exists()
            .map_err(|error| stopped(error.to_string()))?
    {
        let status = git(
            target,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )
        .map_err(|error| stopped(format!("could not recheck the worktree state: {error}")))?;
        let changed = status
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        if changed > 0 {
            return Err(format!(
                "Not deleted: {} changed after you confirmed, likely written by an agent as it stopped. \
                 The worktree and its files are kept; the panes already closed stay closed. \
                 Delete again and tick Discard to remove it anyway.",
                if changed == 1 {
                    "1 file".to_owned()
                } else {
                    format!("{changed} files")
                }
            ));
        }
        // Git leaves ignored folders out of the status above, and removing
        // the worktree deletes them, including a repository cloned into one
        // (B28): such a nested repository stops the removal.
        if let Some(nested) = ignored_repository(target)
            .map_err(|error| stopped(format!("could not recheck ignored folders: {error}")))?
        {
            return Err(stopped(format!(
                "the ignored folder {nested} holds its own Git repository"
            )));
        }
    }
    remove_worktree(root, target, request.discard_changes).map_err(|error| {
        format!("git worktree remove failed: {error}. The worktree remains; panes already closed stay closed.")
    })?;
    let path = target.display();
    let Some(branch) = request.delete_branch.as_deref() else {
        return Ok(format!("Deleted {path}. Its local branch was kept."));
    };
    let flag = if request.force_delete_branch {
        "-D"
    } else {
        "-d"
    };
    match git(root, &["branch", flag, "--", branch]) {
        Ok(_) => Ok(format!("Deleted {path} and local branch {branch}.")),
        Err(detail) => Ok(format!("Deleted {path}; branch {branch} remains: {detail}")),
    }
}

fn io_error(message: String) -> HostError {
    HostError::new(ErrorCode::Io, message)
}

#[cfg(test)]
mod ignored_repository_tests {
    use super::*;

    fn run(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    /// D-15: the deadline stops Git's descendants too, so one that still
    /// holds the output pipe cannot keep the helper's worker past it.
    #[cfg(unix)]
    #[test]
    fn a_descendant_holding_the_pipe_does_not_outlast_the_deadline() {
        let started = std::time::Instant::now();
        let output = output_within(
            Command::new("sh").args(["-c", "sleep 30 & sleep 30"]),
            Duration::from_millis(200),
        )
        .unwrap();
        assert!(output.is_none());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
    }

    /// B28: a pipe something git started still holds after git ended is not
    /// read as an empty, successful answer, which a dirty or nested-repository
    /// check would take for clean; the call did not finish.
    #[cfg(unix)]
    #[test]
    fn output_held_open_after_the_command_ended_is_not_an_empty_success() {
        let started = std::time::Instant::now();
        let output = output_within(
            Command::new("sh").args(["-c", "sleep 30 & echo partial"]),
            Duration::from_millis(200),
        )
        .unwrap();
        assert!(output.is_none(), "{output:?}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
    }

    /// B28: a repository cloned into an ignored folder is found, because a
    /// worktree removal would delete it; ignored build output is not.
    #[test]
    fn a_repository_inside_an_ignored_folder_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        run(repo, &["init", "-q"]);
        std::fs::write(repo.join(".gitignore"), "vendor/\nbuild/\n").unwrap();
        std::fs::create_dir_all(repo.join("build/out")).unwrap();
        std::fs::write(repo.join("build/out/a.o"), "x").unwrap();
        assert_eq!(ignored_repository(repo).unwrap(), None);

        std::fs::create_dir_all(repo.join("vendor/lib")).unwrap();
        run(&repo.join("vendor/lib"), &["init", "-q"]);
        assert_eq!(ignored_repository(repo).unwrap(), Some("vendor".to_owned()));
        std::fs::remove_dir_all(repo.join("vendor")).unwrap();

        // Deeper than any fixed limit would look (GOPATH style).
        let deep = repo.join("build/src/github.com/org/repo");
        std::fs::create_dir_all(&deep).unwrap();
        run(&deep, &["init", "-q"]);
        assert_eq!(ignored_repository(repo).unwrap(), Some("build".to_owned()));
        std::fs::remove_dir_all(deep.join(".git")).unwrap();

        // A folder that cannot be read refuses rather than passing unseen.
        use std::os::unix::fs::PermissionsExt;
        let sealed = repo.join("build/src/sealed");
        std::fs::create_dir_all(&sealed).unwrap();
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o000)).unwrap();
        let refused = ignored_repository(repo);
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(refused.unwrap_err().contains("could not confirm"));
    }
}
