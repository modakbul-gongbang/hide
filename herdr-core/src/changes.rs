//! Reads one checkout's Git working-tree state for the right panel's changes
//! view.
//!
//! Every `git` invocation happens here, on the session-sync coordinator
//! thread, never while the runtime mutex is held and never on a per-event
//! path: [`ChangesReader::read_if_due`] recomputes only when the request
//! changes or the refresh window lapses, and it produces nothing at all while
//! neither Changes nor Explorer is visible and no diff tab needs it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::model::{
    ChangedFileDiffSnapshot, ChangedFileSnapshot, ChangedFileStatus, ChangesSnapshot,
};

/// How stale the list may be while the view is open. Short enough that an edit
/// made in a terminal pane shows up by the time the operator looks over, long
/// enough that it is nowhere near a per-tick fork.
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// The diff text is carried whole on the snapshot wire, so it is bounded. A
/// diff past this is truncated with its reason stated rather than either
/// silently cut or allowed to dominate the wire.
const MAX_DIFF_BYTES: usize = 256 * 1024;

/// What the runtime wants read: the checkout to describe and the file whose
/// diff to fetch. Absent while neither Changes nor Explorer is showing and no
/// diff tab is active, which keeps the reader out of the common hidden-panel
/// path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangesRequest {
    pub root_path: PathBuf,
    pub selected_path: Option<String>,
    /// Whether the selection is in the committed group, which decides what
    /// its diff is taken against.
    pub selected_committed: bool,
    /// What the committed group is measured against, from the worktree
    /// reader's answer. `None` means there is no comparison to make and only
    /// the uncommitted group is produced.
    pub base_branch: Option<String>,
}

pub struct ChangesReader {
    request: Option<ChangesRequest>,
    read_at: Option<Instant>,
}

impl ChangesReader {
    pub fn new() -> Self {
        Self {
            request: None,
            read_at: None,
        }
    }

    /// Returns a projection when one is due for `request`, and `None` when the
    /// cached answer still stands. A changed request always reads immediately,
    /// so selecting a file does not wait out the window.
    pub fn read_if_due(&mut self, request: Option<ChangesRequest>) -> Option<ChangesSnapshot> {
        if self.request == request
            && self
                .read_at
                .is_some_and(|read_at| read_at.elapsed() < REFRESH_INTERVAL)
        {
            return None;
        }
        self.request = request;
        self.read_at = Some(Instant::now());
        Some(match self.request.as_ref() {
            Some(request) => read(request),
            // The view is closed, so there is nothing to describe. An empty
            // projection here also drops the retained diff text off the wire.
            None => ChangesSnapshot::default(),
        })
    }
}

impl Default for ChangesReader {
    fn default() -> Self {
        Self::new()
    }
}

fn read(request: &ChangesRequest) -> ChangesSnapshot {
    let root_path = request.root_path.to_string_lossy().into_owned();
    let toplevel = match git_toplevel(&request.root_path) {
        Ok(toplevel) => toplevel,
        Err(reason) => {
            return ChangesSnapshot {
                root_path: Some(root_path),
                unavailable_reason: Some(reason),
                ..ChangesSnapshot::default()
            };
        }
    };

    let mut entries = match git_status(&toplevel) {
        Ok(status) => parse_status(&status, &toplevel),
        Err(reason) => {
            return ChangesSnapshot {
                root_path: Some(root_path),
                unavailable_reason: Some(reason),
                ..ChangesSnapshot::default()
            };
        }
    };
    // One `--numstat` for the whole working tree rather than one per row: the
    // per-file numbers are a column on a list that is already being read.
    if let Ok(numstat) = git_numstat(&toplevel, &["diff", "--numstat", "-z", "HEAD"]) {
        apply_line_counts(&mut entries, &numstat);
    }

    // A base the reader could not resolve means there is nothing to compare
    // against, so the committed group is absent rather than empty - an empty
    // group would claim the branch has no commits.
    let (base_branch, committed) = match request.base_branch.as_deref() {
        Some(base) => match read_committed(&toplevel, base) {
            Some(committed) => (Some(base.to_owned()), committed),
            None => (None, Vec::new()),
        },
        None => (None, Vec::new()),
    };

    // A selection that is no longer changed is dropped rather than kept
    // pointing at a diff that no longer exists.
    let group = if request.selected_committed {
        &committed
    } else {
        &entries
    };
    let selected = request
        .selected_path
        .as_ref()
        .and_then(|path| group.iter().find(|entry| &entry.path == path));
    let diff = selected.map(|entry| {
        if request.selected_committed {
            read_committed_diff(&toplevel, entry, base_branch.as_deref())
        } else {
            read_diff(&toplevel, entry)
        }
    });

    ChangesSnapshot {
        root_path: Some(root_path),
        selected_path: selected.map(|entry| entry.path.clone()),
        selected_committed: request.selected_committed && selected.is_some(),
        entries,
        committed,
        base_branch,
        diff,
        unavailable_reason: None,
    }
}

/// The files this branch's commits changed since `base`, with their line
/// counts. `None` when the base does not resolve in this checkout.
fn read_committed(toplevel: &Path, base: &str) -> Option<Vec<ChangedFileSnapshot>> {
    let base_ref = resolvable_base(toplevel, base)?;
    let range = format!("{base_ref}...HEAD");
    let statuses = git_text(
        toplevel,
        &["diff", "--name-status", "-z", "--find-renames", &range],
    )
    .ok()?;
    let mut entries = parse_name_status(&statuses, toplevel);
    if let Ok(numstat) = git_numstat(toplevel, &["diff", "--numstat", "-z", &range]) {
        apply_line_counts(&mut entries, &numstat);
    }
    Some(entries)
}

/// The base as a ref this checkout can resolve: the local branch first, its
/// remote-tracking form second. Mirrors the worktree reader's rule, so the
/// card's counts and this list are measured against the same commit.
fn resolvable_base(toplevel: &Path, base: &str) -> Option<String> {
    for candidate in [base.to_owned(), format!("origin/{base}")] {
        if git_text(
            toplevel,
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

/// Splits `--name-status -z` output. Records alternate status and path, both
/// NUL-terminated, so a path with a space or a newline survives intact.
pub fn parse_name_status(output: &str, toplevel: &Path) -> Vec<ChangedFileSnapshot> {
    let fields = output.split('\0').collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let code = fields[index];
        if code.is_empty() || index + 1 >= fields.len() {
            break;
        }
        index += 1;
        let (previous_relative_path, relative_path) = if code.starts_with('R') {
            if index + 1 >= fields.len() {
                break;
            }
            let previous = fields[index].to_owned();
            let current = fields[index + 1];
            index += 2;
            (Some(previous), current)
        } else {
            let current = fields[index];
            index += 1;
            (None, current)
        };
        if relative_path.is_empty() {
            continue;
        }
        entries.push(ChangedFileSnapshot {
            path: toplevel.join(relative_path).to_string_lossy().into_owned(),
            relative_path: relative_path.to_owned(),
            previous_relative_path,
            // `--name-status` reports one letter where porcelain reports two.
            status: ChangedFileStatus::from_porcelain(&format!("{code} ")),
            added_lines: None,
            removed_lines: None,
        });
    }
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    entries
}

/// Splits `--numstat -z` output into per-path line counts. A binary file is
/// reported as `-\t-`, which stays absent rather than becoming a zero.
pub fn parse_numstat(output: &str) -> Vec<(String, Option<u32>, Option<u32>)> {
    let fields = output.split('\0').collect::<Vec<_>>();
    let mut counts = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let record = fields[index];
        if record.is_empty() {
            index += 1;
            continue;
        }
        let mut columns = record.splitn(3, '\t');
        let added = columns.next().and_then(|value| value.parse().ok());
        let removed = columns.next().and_then(|value| value.parse().ok());
        let Some(path) = columns.next() else {
            index += 1;
            continue;
        };
        if path.is_empty() && index + 2 < fields.len() {
            counts.push((fields[index + 2].to_owned(), added, removed));
            index += 3;
        } else {
            counts.push((path.to_owned(), added, removed));
            index += 1;
        }
    }
    counts
}

fn apply_line_counts(entries: &mut [ChangedFileSnapshot], counts: &str) {
    for (path, added, removed) in parse_numstat(counts) {
        if let Some(entry) = entries.iter_mut().find(|entry| entry.relative_path == path) {
            entry.added_lines = added;
            entry.removed_lines = removed;
        }
    }
}

fn git_numstat(toplevel: &Path, arguments: &[&str]) -> Result<String, String> {
    git_text(toplevel, arguments)
}

fn git_text(cwd: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = run_git(cwd, arguments)?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            arguments[0],
            git_error_text(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A committed file's diff is against the base, not the index: the group is
/// "what this branch changed", so its diff must be the same comparison.
fn read_committed_diff(
    toplevel: &Path,
    entry: &ChangedFileSnapshot,
    base_branch: Option<&str>,
) -> ChangedFileDiffSnapshot {
    let Some(base) = base_branch.and_then(|base| resolvable_base(toplevel, base)) else {
        return ChangedFileDiffSnapshot {
            path: entry.path.clone(),
            text: String::new(),
            notice: Some("The base branch could not be resolved in this checkout.".to_owned()),
        };
    };
    match git_text(
        toplevel,
        &[
            "diff",
            &format!("{base}...HEAD"),
            "--",
            entry
                .previous_relative_path
                .as_deref()
                .unwrap_or(&entry.relative_path),
            &entry.relative_path,
        ],
    ) {
        Ok(text) => truncate_diff(entry.path.clone(), text),
        Err(reason) => ChangedFileDiffSnapshot {
            path: entry.path.clone(),
            text: String::new(),
            notice: Some(reason),
        },
    }
}

fn git_toplevel(root: &Path) -> Result<PathBuf, String> {
    let output = run_git(root, &["rev-parse", "--show-toplevel"])?;
    if !output.status.success() {
        return Err(format!(
            "{} is not inside a Git repository",
            root.to_string_lossy()
        ));
    }
    let toplevel = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if toplevel.is_empty() {
        return Err("Git reported no repository root for this checkout".to_owned());
    }
    Ok(PathBuf::from(toplevel))
}

fn git_status(toplevel: &Path) -> Result<String, String> {
    let output = run_git(
        toplevel,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--find-renames",
            "--untracked-files=all",
        ],
    )?;
    if !output.status.success() {
        return Err(format!(
            "git status failed: {}",
            git_error_text(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn read_diff(toplevel: &Path, entry: &ChangedFileSnapshot) -> ChangedFileDiffSnapshot {
    let text = match entry.status {
        // An untracked file has no index side to diff against, so it is
        // compared with an empty file. `--no-index` reports a difference as
        // exit code 1, which is the expected outcome here rather than an error.
        ChangedFileStatus::Untracked => run_git(
            toplevel,
            &["diff", "--no-index", "--", "/dev/null", &entry.path],
        )
        .and_then(|output| match output.status.code() {
            Some(0 | 1) => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
            _ => Err(format!(
                "git diff failed: {}",
                git_error_text(&output.stderr)
            )),
        }),
        _ => run_git(
            toplevel,
            &[
                "diff",
                "HEAD",
                "--",
                entry
                    .previous_relative_path
                    .as_deref()
                    .unwrap_or(&entry.relative_path),
                &entry.relative_path,
            ],
        )
        .and_then(|output| {
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).into_owned())
            } else {
                Err(format!(
                    "git diff failed: {}",
                    git_error_text(&output.stderr)
                ))
            }
        }),
    };

    match text {
        Ok(text) => truncate_diff(entry.path.clone(), text),
        Err(reason) => ChangedFileDiffSnapshot {
            path: entry.path.clone(),
            text: String::new(),
            notice: Some(reason),
        },
    }
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|error| format!("git could not be run: {error}"))
}

fn git_error_text(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr).trim().to_owned();
    if text.is_empty() {
        "no error output".to_owned()
    } else {
        text
    }
}

/// Splits porcelain v1 `-z` output into entries. Records are NUL-terminated
/// and each is `XY<space><path>`, so no quoting or escaping applies and a path
/// containing a space or a newline survives intact.
pub fn parse_status(output: &str, toplevel: &Path) -> Vec<ChangedFileSnapshot> {
    let fields = output.split('\0').collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let record = fields[index];
        index += 1;
        if record.len() <= 3 {
            continue;
        }
        let Some(code) = record.get(0..2) else {
            continue;
        };
        let relative_path = record.get(3..).unwrap_or_default().to_owned();
        let previous_relative_path = if code.contains('R') || code.contains('C') {
            let previous = fields
                .get(index)
                .copied()
                .filter(|path| !path.is_empty())
                .map(str::to_owned);
            if previous.is_some() {
                index += 1;
            }
            previous
        } else {
            None
        };
        entries.push(ChangedFileSnapshot {
            path: toplevel.join(&relative_path).to_string_lossy().into_owned(),
            relative_path,
            previous_relative_path,
            status: ChangedFileStatus::from_porcelain(code),
            added_lines: None,
            removed_lines: None,
        });
    }
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    entries
}

fn truncate_diff(path: String, text: String) -> ChangedFileDiffSnapshot {
    if text.len() <= MAX_DIFF_BYTES {
        return ChangedFileDiffSnapshot {
            path,
            text,
            notice: None,
        };
    }
    // Cut on a character boundary so the retained text stays valid UTF-8.
    let mut end = MAX_DIFF_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    ChangedFileDiffSnapshot {
        path,
        text: text[..end].to_owned(),
        notice: Some(format!(
            "This diff is larger than {} KB and is shown truncated",
            MAX_DIFF_BYTES / 1024
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_records_project_the_presented_statuses() {
        let output = " M src/lib.rs\0A  src/new.rs\0 D src/gone.rs\0?? notes.txt\0UU src/conflict.rs\0R  src/새 이름.rs\0src/old name.rs\0";
        let entries = parse_status(output, Path::new("/checkout"));
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.relative_path.as_str(), entry.status.as_str()))
                .collect::<Vec<_>>(),
            [
                ("notes.txt", "untracked"),
                ("src/conflict.rs", "conflict"),
                ("src/gone.rs", "deleted"),
                ("src/lib.rs", "modified"),
                ("src/new.rs", "added"),
                ("src/새 이름.rs", "renamed"),
            ]
        );
        assert_eq!(entries[0].path, "/checkout/notes.txt");
        let renamed = entries
            .iter()
            .find(|entry| entry.status == ChangedFileStatus::Renamed)
            .unwrap();
        assert_eq!(
            renamed.previous_relative_path.as_deref(),
            Some("src/old name.rs")
        );
    }

    #[test]
    fn a_staged_delete_reads_as_deleted_even_with_a_worktree_column() {
        assert_eq!(
            ChangedFileStatus::from_porcelain("AD"),
            ChangedFileStatus::Deleted
        );
        assert_eq!(
            ChangedFileStatus::from_porcelain("MM"),
            ChangedFileStatus::Modified
        );
        assert_eq!(
            ChangedFileStatus::from_porcelain("R "),
            ChangedFileStatus::Renamed
        );
        assert_eq!(
            ChangedFileStatus::from_porcelain("UU"),
            ChangedFileStatus::Conflict
        );
    }

    #[test]
    fn a_path_with_a_space_survives_the_nul_split() {
        let entries = parse_status(" M a file.rs\0", Path::new("/checkout"));
        assert_eq!(entries[0].relative_path, "a file.rs");
    }

    #[test]
    fn name_status_records_project_the_committed_group() {
        let entries = parse_name_status(
            "M\0src/lib.rs\0A\0src/new.rs\0D\0src/gone.rs\0R100\0src/old name.rs\0src/새 이름.rs\0",
            Path::new("/checkout"),
        );
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.relative_path.as_str(), entry.status.as_str()))
                .collect::<Vec<_>>(),
            [
                ("src/gone.rs", "deleted"),
                ("src/lib.rs", "modified"),
                ("src/new.rs", "added"),
                ("src/새 이름.rs", "renamed"),
            ]
        );
        assert_eq!(entries[0].path, "/checkout/src/gone.rs");
        assert_eq!(
            entries.last().unwrap().previous_relative_path.as_deref(),
            Some("src/old name.rs")
        );
    }

    #[test]
    fn numstat_counts_land_on_the_file_they_describe() {
        let mut entries = parse_name_status("M\0src/lib.rs\0M\0src/other.rs\0", Path::new("/c"));
        apply_line_counts(&mut entries, "12\t3\tsrc/lib.rs\0-\t-\tsrc/other.rs\0");
        let lib = &entries
            .iter()
            .find(|e| e.relative_path == "src/lib.rs")
            .unwrap();
        assert_eq!((lib.added_lines, lib.removed_lines), (Some(12), Some(3)));
        // A binary file counts no lines, so it shows no numbers rather than
        // claiming it changed none.
        let other = &entries
            .iter()
            .find(|e| e.relative_path == "src/other.rs")
            .unwrap();
        assert_eq!((other.added_lines, other.removed_lines), (None, None));
    }

    #[test]
    fn nul_numstat_rename_counts_land_on_the_destination() {
        let mut entries =
            parse_name_status("R100\0src/old name.rs\0src/새 이름.rs\0", Path::new("/c"));
        apply_line_counts(&mut entries, "7\t2\t\0src/old name.rs\0src/새 이름.rs\0");
        assert_eq!(
            (entries[0].added_lines, entries[0].removed_lines),
            (Some(7), Some(2))
        );
    }

    /// A file with no numstat record - an untracked one, which has no index
    /// side to count against - keeps absent counts rather than gaining zeros.
    #[test]
    fn a_file_without_counts_keeps_none() {
        let mut entries = parse_status("?? notes.txt\0", Path::new("/c"));
        apply_line_counts(&mut entries, "");
        assert_eq!(entries[0].added_lines, None);
        assert_eq!(entries[0].removed_lines, None);
    }

    #[test]
    fn an_oversized_diff_states_that_it_was_cut() {
        let projected = truncate_diff("/x".to_owned(), "a".repeat(MAX_DIFF_BYTES + 1));
        assert_eq!(projected.text.len(), MAX_DIFF_BYTES);
        assert!(projected.notice.is_some());
    }

    #[test]
    fn a_closed_view_reads_nothing_and_publishes_an_empty_projection() {
        let mut reader = ChangesReader::new();
        let projected = reader.read_if_due(None).expect("first read is always due");
        assert_eq!(projected, ChangesSnapshot::default());
        assert!(reader.read_if_due(None).is_none());
    }

    #[test]
    fn a_directory_outside_a_repository_states_its_reason() {
        let root = crate::workspace::temp_base_outside_any_repository()
            .join(format!("herdr-core-changes-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("the temporary checkout root is creatable");
        let mut reader = ChangesReader::new();
        let projected = reader
            .read_if_due(Some(ChangesRequest {
                root_path: root.clone(),
                selected_path: None,
                selected_committed: false,
                base_branch: None,
            }))
            .expect("first read is always due");
        let _ = std::fs::remove_dir_all(&root);
        assert!(projected.entries.is_empty());
        assert!(projected.unavailable_reason.is_some());
    }
}
