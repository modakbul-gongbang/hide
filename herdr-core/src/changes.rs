//! Reads one checkout's Git working-tree state for the right panel's changes
//! view.
//!
//! Every `git` invocation happens here, on the session-sync coordinator
//! thread, never while the runtime mutex is held and never on a per-event
//! path: [`ChangesReader::read_if_due`] recomputes only when the request
//! changes or the refresh window lapses, and it produces nothing at all while
//! neither Changes nor Explorer is visible and no diff tab needs it.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
    /// Opened registered roots from the daemon. The native shell supplies no
    /// handle, so the reader pins its scope before starting its Git reads.
    pub file_roots: Option<crate::files::FileRoots>,
    /// The selected checkout's Git root, independently of a narrower
    /// registered-folder History scope.
    pub checkout_path: PathBuf,
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
    let owned_roots = if request.file_roots.is_none() {
        std::fs::File::open(&request.root_path).ok().map(|file| {
            crate::files::FileRoots::from_opened(vec![(request.root_path.clone(), file)])
        })
    } else {
        None
    };
    let scoped_roots = if request.root_path != request.checkout_path {
        match request
            .file_roots
            .as_ref()
            .and_then(|roots| roots.scoped(&request.root_path).ok())
        {
            Some(roots) => Some(roots),
            None if request.file_roots.is_some() => {
                return ChangesSnapshot {
                    root_path: Some(root_path),
                    unavailable_reason: Some(
                        "The registered History folder could not be opened".to_owned(),
                    ),
                    ..ChangesSnapshot::default()
                };
            }
            None => None,
        }
    } else {
        None
    };
    let file_roots = scoped_roots
        .as_ref()
        .or(request.file_roots.as_ref())
        .or(owned_roots.as_ref());
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
    if toplevel.canonicalize().ok() != request.checkout_path.canonicalize().ok() {
        return ChangesSnapshot {
            root_path: Some(root_path),
            unavailable_reason: Some("This History scope belongs to another checkout".to_owned()),
            ..ChangesSnapshot::default()
        };
    }
    let Some(scope) = repository_scope(&toplevel, &request.checkout_path, &request.root_path)
    else {
        return ChangesSnapshot {
            root_path: Some(root_path),
            unavailable_reason: Some(
                "This History folder no longer matches its registered checkout".to_owned(),
            ),
            ..ChangesSnapshot::default()
        };
    };
    if !handles_match_history_paths(request, file_roots) {
        return ChangesSnapshot {
            root_path: Some(root_path),
            unavailable_reason: Some(
                "This History folder no longer matches its registered checkout".to_owned(),
            ),
            ..ChangesSnapshot::default()
        };
    }

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
    let entries = scope_entries(entries, &request.root_path, &scope);

    // A base the reader could not resolve means there is nothing to compare
    // against, so the committed group is absent rather than empty - an empty
    // group would claim the branch has no commits.
    let (base_branch, committed) = match request.base_branch.as_deref() {
        Some(base) => match read_committed(&toplevel, base) {
            Ok(Some(committed)) => (
                Some(base.to_owned()),
                scope_entries(committed, &request.root_path, &scope),
            ),
            Ok(None) => (None, Vec::new()),
            Err(reason) => {
                return ChangesSnapshot {
                    root_path: Some(root_path),
                    unavailable_reason: Some(reason),
                    ..ChangesSnapshot::default()
                };
            }
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
            read_committed_diff(&toplevel, &scope, entry, base_branch.as_deref())
        } else {
            read_diff(&toplevel, &scope, entry, file_roots)
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

/// A registration may name a folder below the Git root. Keep the existing
/// reader, but project only paths owned by that folder. Git's rename source
/// can cross the boundary, so an inbound rename is an addition and an
/// outbound rename is a deletion from this checkout's perspective.
fn repository_scope(toplevel: &Path, checkout: &Path, root: &Path) -> Option<PathBuf> {
    let repository = toplevel.canonicalize().ok()?;
    // Git may spell the same checkout through a system symlink (for example
    // /tmp versus /private/tmp). The registered root and checkout came from
    // the same registration path, so compare their lexical relationship.
    let registered_relative = root.strip_prefix(checkout).ok()?;
    let resolved = root.canonicalize().ok()?;
    let resolved_relative = resolved.strip_prefix(repository).ok()?;
    // Registration stores the canonical folder. If its path now resolves to
    // a different in-repository folder, that folder is not registered even
    // though Git reports the same checkout root.
    (registered_relative == resolved_relative).then(|| resolved_relative.to_path_buf())
}

fn handles_match_history_paths(
    request: &ChangesRequest,
    scoped_or_registered: Option<&crate::files::FileRoots>,
) -> bool {
    // The daemon retains the original checkout handle. A missing handle is a
    // registration failure, not permission to read the current ambient path.
    let checkout_matches = request
        .file_roots
        .as_ref()
        .is_none_or(|roots| roots.matches_ambient_root(&request.checkout_path));
    checkout_matches
        && scoped_or_registered.is_some_and(|roots| roots.matches_ambient_root(&request.root_path))
}

fn scope_entries(
    entries: Vec<ChangedFileSnapshot>,
    root: &Path,
    scope: &Path,
) -> Vec<ChangedFileSnapshot> {
    let mut scoped = Vec::new();
    for mut entry in entries {
        let current = Path::new(&entry.relative_path)
            .strip_prefix(scope)
            .ok()
            .map(Path::to_path_buf);
        let previous = entry.previous_relative_path.as_deref().and_then(|path| {
            Path::new(path)
                .strip_prefix(scope)
                .ok()
                .map(Path::to_path_buf)
        });
        match (current, previous) {
            (Some(current), previous) => {
                let crosses_boundary =
                    entry.status == ChangedFileStatus::Renamed && previous.is_none();
                entry.relative_path = current.to_string_lossy().into_owned();
                entry.path = root.join(current).to_string_lossy().into_owned();
                entry.previous_relative_path =
                    previous.map(|path| path.to_string_lossy().into_owned());
                if crosses_boundary {
                    entry.status = ChangedFileStatus::Added;
                    // Repository-wide rename counts are not the scoped
                    // addition's counts. Leave them unknown instead.
                    entry.added_lines = None;
                    entry.removed_lines = None;
                }
                scoped.push(entry);
            }
            (None, Some(previous)) if entry.status == ChangedFileStatus::Renamed => {
                entry.relative_path = previous.to_string_lossy().into_owned();
                entry.path = root.join(previous).to_string_lossy().into_owned();
                entry.previous_relative_path = None;
                entry.status = ChangedFileStatus::Deleted;
                entry.added_lines = None;
                entry.removed_lines = None;
                scoped.push(entry);
            }
            _ => {}
        }
    }
    scoped.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    scoped
}

/// The files this branch's commits changed since `base`, with their line
/// counts. An unresolved base omits the group; a failed read is unavailable.
fn read_committed(toplevel: &Path, base: &str) -> Result<Option<Vec<ChangedFileSnapshot>>, String> {
    let Some(base_ref) = resolvable_base(toplevel, base) else {
        return Ok(None);
    };
    let range = format!("{base_ref}...HEAD");
    let statuses = git_text(
        toplevel,
        &["diff", "--name-status", "-z", "--find-renames", &range],
    )?;
    let mut entries = parse_name_status(&statuses, toplevel);
    if let Ok(numstat) = git_numstat(toplevel, &["diff", "--numstat", "-z", &range]) {
        apply_line_counts(&mut entries, &numstat);
    }
    Ok(Some(entries))
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
    scope: &Path,
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
    let current = scope
        .join(&entry.relative_path)
        .to_string_lossy()
        .into_owned();
    let previous = entry
        .previous_relative_path
        .as_ref()
        .map(|path| scope.join(path).to_string_lossy().into_owned())
        .unwrap_or_else(|| current.clone());
    match git_diff_text(
        toplevel,
        &["diff", &format!("{base}...HEAD"), "--", &previous, &current],
        false,
        None,
    ) {
        Ok((text, truncated)) => bounded_diff(entry.path.clone(), text, truncated),
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

fn read_diff(
    toplevel: &Path,
    scope: &Path,
    entry: &ChangedFileSnapshot,
    file_roots: Option<&crate::files::FileRoots>,
) -> ChangedFileDiffSnapshot {
    let current = scope
        .join(&entry.relative_path)
        .to_string_lossy()
        .into_owned();
    let previous = entry
        .previous_relative_path
        .as_ref()
        .map(|path| scope.join(path).to_string_lossy().into_owned())
        .unwrap_or_else(|| current.clone());
    let text = match entry.status {
        // An untracked file has no index side to diff against, so it is
        // compared with an empty file. `--no-index` reports a difference as
        // exit code 1, which is the expected outcome here rather than an error.
        ChangedFileStatus::Untracked => file_roots
            .ok_or_else(|| "The selected file could not be read".to_owned())
            .and_then(|roots| {
                let file = roots
                    .open(Path::new(&entry.path), false)
                    .map_err(|_| "The selected file could not be read".to_owned())?;
                if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
                    return Err("Only existing regular files can be opened".to_owned());
                }
                git_diff_text(
                    toplevel,
                    &["diff", "--no-index", "--", "/dev/null", "-"],
                    true,
                    Some(file),
                )
            }),
        _ => git_diff_text(
            toplevel,
            &["diff", "HEAD", "--", &previous, &current],
            false,
            None,
        ),
    };

    match text {
        Ok((text, truncated)) => bounded_diff(entry.path.clone(), text, truncated),
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

/// Capture at most one patch's wire budget plus a UTF-8 boundary. Kill a Git
/// diff that exceeds it, rather than first buffering an arbitrarily large
/// file in `Command::output`. Stderr is drained concurrently so it cannot
/// block a child that is reporting a failure.
fn git_diff_text(
    cwd: &Path,
    args: &[&str],
    no_index: bool,
    input: Option<std::fs::File>,
) -> Result<(String, bool), String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(cwd).args(args);
    if let Some(input) = input {
        command.stdin(Stdio::from(input));
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("git could not be run: {error}"))?;
    let mut stdout = child.stdout.take().expect("piped Git stdout");
    let mut stderr = child.stderr.take().expect("piped Git stderr");
    let stderr_reader = std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0; 8192];
        loop {
            let count = stderr.read(&mut chunk)?;
            if count == 0 {
                break;
            }
            kept.extend_from_slice(&chunk[..count.min(8192usize.saturating_sub(kept.len()))]);
        }
        Ok::<_, std::io::Error>(kept)
    });
    let mut bytes = Vec::with_capacity(MAX_DIFF_BYTES + 4);
    let mut chunk = [0; 8192];
    let mut truncated = false;
    let mut read_error = None;
    loop {
        match stdout.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                let remaining = (MAX_DIFF_BYTES + 4).saturating_sub(bytes.len());
                bytes.extend_from_slice(&chunk[..count.min(remaining)]);
                if count > remaining {
                    truncated = true;
                    let _ = child.kill();
                    break;
                }
            }
            Err(error) => {
                read_error = Some(error);
                let _ = child.kill();
                break;
            }
        }
    }
    drop(stdout);
    let status = child.wait();
    let stderr = stderr_reader
        .join()
        .map_err(|_| "git diff error output could not be read".to_owned())?
        .map_err(|error| format!("git diff error output could not be read: {error}"))?;
    let status = status.map_err(|error| format!("git diff could not finish: {error}"))?;
    if let Some(error) = read_error {
        return Err(format!("git diff output could not be read: {error}"));
    }
    if !truncated && !status.success() && !(no_index && status.code() == Some(1)) {
        return Err(format!("git diff failed: {}", git_error_text(&stderr)));
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
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

fn bounded_diff(path: String, text: String, truncated: bool) -> ChangedFileDiffSnapshot {
    let mut diff = truncate_diff(path, text);
    if truncated && diff.notice.is_none() {
        diff.notice = Some(format!(
            "This diff is larger than {} KB and is shown truncated",
            MAX_DIFF_BYTES / 1024
        ));
    }
    diff
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
    fn selected_large_untracked_patch_is_captured_within_the_wire_budget() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let output = run_git(root, &["init", "-q"]).unwrap();
        assert!(output.status.success());
        let file = root.join("large.txt");
        std::fs::write(&file, "한글".repeat(100_000)).unwrap();
        let snapshot = read(&ChangesRequest {
            root_path: root.to_path_buf(),
            file_roots: None,
            checkout_path: root.to_path_buf(),
            selected_path: Some(file.to_string_lossy().into_owned()),
            selected_committed: false,
            base_branch: None,
        });
        let diff = snapshot.diff.expect("untracked patch");
        assert!(diff.text.len() <= MAX_DIFF_BYTES);
        assert!(diff.notice.as_deref().unwrap().contains("truncated"));
        assert!(diff.text.is_char_boundary(diff.text.len()));
    }

    #[cfg(unix)]
    #[test]
    fn untracked_diff_uses_the_registered_handle_after_a_parent_swap() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let repository = temporary.path().join("repository");
        let outside = repository.join("outside");
        std::fs::create_dir(&repository).unwrap();
        std::fs::create_dir(&outside).unwrap();
        assert!(
            run_git(&repository, &["init", "-q"])
                .unwrap()
                .status
                .success()
        );
        let registered = repository.join("registered");
        let nested = registered.join("nested");
        std::fs::create_dir(&registered).unwrap();
        std::fs::create_dir(&nested).unwrap();
        let inside = nested.join("file.txt");
        std::fs::write(&inside, "INSIDE_CONTENT\n").unwrap();
        std::fs::write(outside.join("file.txt"), "OUTSIDE_SENTINEL_CONTENT\n").unwrap();
        let roots = crate::files::FileRoots::from_opened(vec![(
            repository.clone(),
            std::fs::File::open(&repository).unwrap(),
        )]);
        let request = ChangesRequest {
            root_path: registered.clone(),
            file_roots: Some(roots.clone()),
            checkout_path: repository.clone(),
            selected_path: Some(inside.to_string_lossy().into_owned()),
            selected_committed: false,
            base_branch: None,
        };
        let listed = read(&request);
        assert!(listed.entries.iter().all(|entry| {
            entry
                .path
                .starts_with(&registered.to_string_lossy().to_string())
        }));
        let entry = listed
            .entries
            .iter()
            .find(|entry| entry.path == inside.to_string_lossy())
            .unwrap();
        assert!(
            listed
                .diff
                .as_ref()
                .unwrap()
                .text
                .contains("INSIDE_CONTENT"),
            "diff: {:?}",
            listed.diff
        );

        let scoped_roots = roots.scoped(&registered).unwrap();
        std::fs::rename(&nested, registered.join("moved")).unwrap();
        symlink(&outside, &nested).unwrap();
        let after_swap = read_diff(
            &request.checkout_path,
            Path::new("registered"),
            entry,
            Some(&scoped_roots),
        );
        assert!(!after_swap.text.contains("OUTSIDE_SENTINEL_CONTENT"));
        assert!(after_swap.notice.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn registered_root_cannot_retarget_history_to_a_sibling() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let repository = temporary.path().canonicalize().unwrap();
        let registered = repository.join("registered");
        let sibling = repository.join("sibling");
        std::fs::create_dir(&registered).unwrap();
        std::fs::create_dir(&sibling).unwrap();
        assert!(
            run_git(&repository, &["init", "-q"])
                .unwrap()
                .status
                .success()
        );
        std::fs::write(registered.join("inside.txt"), "INSIDE_CONTENT\n").unwrap();
        std::fs::write(sibling.join("outside.txt"), "OUTSIDE_SENTINEL_CONTENT\n").unwrap();
        let roots = crate::files::FileRoots::from_opened(vec![(
            repository.clone(),
            std::fs::File::open(&repository).unwrap(),
        )]);
        let request = ChangesRequest {
            root_path: registered.clone(),
            file_roots: Some(roots),
            checkout_path: repository,
            selected_path: Some(registered.join("inside.txt").to_string_lossy().into_owned()),
            selected_committed: false,
            base_branch: None,
        };
        let inside = read(&request);
        assert!(inside.unavailable_reason.is_none());
        assert_eq!(inside.entries.len(), 1);
        assert!(inside.diff.unwrap().text.contains("INSIDE_CONTENT"));

        std::fs::rename(&registered, request.checkout_path.join("moved")).unwrap();
        symlink("sibling", &registered).unwrap();
        let retargeted = read(&request);
        assert!(retargeted.unavailable_reason.is_some());
        assert!(retargeted.entries.is_empty());
        assert!(retargeted.committed.is_empty());
        assert!(retargeted.diff.is_none());
        assert!(!format!("{retargeted:?}").contains("OUTSIDE_SENTINEL_CONTENT"));
    }

    #[cfg(unix)]
    #[test]
    fn replaced_checkout_root_cannot_read_another_repository() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let registered = temporary.path().join("registered");
        let replacement = temporary.path().join("replacement");
        std::fs::create_dir(&registered).unwrap();
        std::fs::create_dir(&replacement).unwrap();
        for repository in [&registered, &replacement] {
            assert!(
                run_git(repository, &["init", "-q"])
                    .unwrap()
                    .status
                    .success()
            );
        }
        std::fs::write(registered.join("inside.txt"), "INSIDE_CONTENT\n").unwrap();
        std::fs::write(replacement.join("secret.txt"), "OUTSIDE_SENTINEL_CONTENT\n").unwrap();
        let roots = crate::files::FileRoots::from_opened(vec![(
            registered.clone(),
            std::fs::File::open(&registered).unwrap(),
        )]);
        let request = ChangesRequest {
            root_path: registered.clone(),
            file_roots: Some(roots),
            checkout_path: registered.clone(),
            selected_path: Some(registered.join("inside.txt").to_string_lossy().into_owned()),
            selected_committed: false,
            base_branch: None,
        };
        let inside = read(&request);
        assert!(inside.unavailable_reason.is_none());
        assert!(inside.diff.unwrap().text.contains("INSIDE_CONTENT"));

        std::fs::rename(&registered, temporary.path().join("moved")).unwrap();
        symlink(&replacement, &registered).unwrap();
        let swapped = read(&request);
        assert!(swapped.unavailable_reason.is_some());
        assert!(swapped.entries.is_empty());
        assert!(swapped.diff.is_none());
        assert!(!format!("{swapped:?}").contains("OUTSIDE_SENTINEL_CONTENT"));
    }

    #[cfg(unix)]
    #[test]
    fn scoped_handle_rejects_a_transient_sibling_swap() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let repository = temporary.path().join("repository");
        let registered = repository.join("registered");
        let sibling = repository.join("sibling");
        std::fs::create_dir(&repository).unwrap();
        std::fs::create_dir(&registered).unwrap();
        std::fs::create_dir(&sibling).unwrap();
        assert!(
            run_git(&repository, &["init", "-q"])
                .unwrap()
                .status
                .success()
        );
        std::fs::write(registered.join("inside.txt"), "INSIDE_CONTENT\n").unwrap();
        std::fs::write(sibling.join("inside.txt"), "OUTSIDE_SENTINEL_CONTENT\n").unwrap();
        let roots = crate::files::FileRoots::from_opened(vec![(
            repository.clone(),
            std::fs::File::open(&repository).unwrap(),
        )]);
        let request = ChangesRequest {
            root_path: registered.clone(),
            file_roots: Some(roots.clone()),
            checkout_path: repository,
            selected_path: Some(registered.join("inside.txt").to_string_lossy().into_owned()),
            selected_committed: false,
            base_branch: None,
        };
        let inside = read(&request);
        assert!(inside.unavailable_reason.is_none());
        assert!(inside.diff.unwrap().text.contains("INSIDE_CONTENT"));

        std::fs::rename(&registered, request.checkout_path.join("moved")).unwrap();
        symlink("sibling", &registered).unwrap();
        let stale_scope = roots.scoped(&registered).unwrap();
        std::fs::remove_file(&registered).unwrap();
        std::fs::rename(request.checkout_path.join("moved"), &registered).unwrap();
        assert!(!handles_match_history_paths(&request, Some(&stale_scope)));
        let restored = read(&request);
        assert!(restored.unavailable_reason.is_none());
        assert!(
            restored
                .diff
                .as_ref()
                .unwrap()
                .text
                .contains("INSIDE_CONTENT")
        );
        assert!(!format!("{restored:?}").contains("OUTSIDE_SENTINEL_CONTENT"));
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
                file_roots: None,
                checkout_path: root.clone(),
                selected_path: None,
                selected_committed: false,
                base_branch: None,
            }))
            .expect("first read is always due");
        let _ = std::fs::remove_dir_all(&root);
        assert!(projected.entries.is_empty());
        assert!(projected.unavailable_reason.is_some());
    }

    #[test]
    fn a_registered_subfolder_keeps_its_history_without_exposing_siblings() {
        let temporary = tempfile::tempdir().expect("fixture root");
        let repository = temporary.path();
        let registered = repository.join("registered");
        std::fs::create_dir(&registered).unwrap();
        let git = |arguments: &[&str]| {
            let output = run_git(repository, arguments).expect("git runs");
            assert!(
                output.status.success(),
                "git {arguments:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "Fixture"]);
        git(&["config", "user.email", "fixture@example.invalid"]);
        for (path, content) in [
            ("registered/inside.txt", "inside base\n"),
            ("registered/delete.txt", "delete me\n"),
            ("registered/rename-old.txt", "rename within\n"),
            ("registered/outgoing.txt", "move out\n"),
            ("registered/work-out.txt", "working outbound only\n"),
            ("outside.txt", "outside base\n"),
            ("outside-source.txt", "move in\n"),
            ("work-in.txt", "working inbound only\n"),
        ] {
            std::fs::write(repository.join(path), content).unwrap();
        }
        git(&["add", "."]);
        git(&["commit", "-m", "base"]);
        git(&["checkout", "-b", "feature"]);
        git(&[
            "mv",
            "registered/rename-old.txt",
            "registered/rename-new.txt",
        ]);
        git(&["mv", "outside-source.txt", "registered/incoming.txt"]);
        git(&["mv", "registered/outgoing.txt", "outside-outgoing.txt"]);
        std::fs::write(registered.join("inside.txt"), "inside committed\n").unwrap();
        std::fs::write(repository.join("outside.txt"), "outside committed\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "feature"]);
        git(&["mv", "work-in.txt", "registered/work-in.txt"]);
        git(&["mv", "registered/work-out.txt", "work-out.txt"]);
        std::fs::write(registered.join("inside.txt"), "inside working\n").unwrap();
        std::fs::remove_file(registered.join("delete.txt")).unwrap();
        std::fs::write(registered.join("new.txt"), "untracked inside\n").unwrap();
        std::fs::write(repository.join("outside.txt"), "outside working\n").unwrap();
        std::fs::write(repository.join("outside-new.txt"), "untracked outside\n").unwrap();

        let request = |path: Option<&str>, committed| ChangesRequest {
            root_path: registered.clone(),
            file_roots: None,
            checkout_path: repository.to_path_buf(),
            selected_path: path.map(|path| registered.join(path).to_string_lossy().into_owned()),
            selected_committed: committed,
            base_branch: Some("main".to_owned()),
        };
        let mut wrong_checkout = request(None, false);
        wrong_checkout.checkout_path = registered.clone();
        assert!(read(&wrong_checkout).unavailable_reason.is_some());
        let listed = read(&request(Some("inside.txt"), false));
        assert!(listed.unavailable_reason.is_none());
        assert_eq!(
            listed
                .entries
                .iter()
                .map(|entry| entry.relative_path.as_str())
                .collect::<Vec<_>>(),
            [
                "delete.txt",
                "inside.txt",
                "new.txt",
                "work-in.txt",
                "work-out.txt"
            ]
        );
        assert!(listed.entries.iter().all(|entry| {
            entry
                .path
                .starts_with(&registered.to_string_lossy().to_string())
        }));
        assert!(
            listed
                .diff
                .as_ref()
                .unwrap()
                .text
                .contains("inside working")
        );
        assert!(!listed.diff.as_ref().unwrap().text.contains("outside"));
        let working_incoming = listed
            .entries
            .iter()
            .find(|entry| entry.relative_path == "work-in.txt")
            .unwrap();
        assert_eq!(working_incoming.status, ChangedFileStatus::Added);
        assert_eq!(working_incoming.previous_relative_path, None);
        assert_eq!(
            (working_incoming.added_lines, working_incoming.removed_lines),
            (None, None)
        );
        let working_outgoing = listed
            .entries
            .iter()
            .find(|entry| entry.relative_path == "work-out.txt")
            .unwrap();
        assert_eq!(working_outgoing.status, ChangedFileStatus::Deleted);
        assert_eq!(working_outgoing.previous_relative_path, None);
        for (name, outside) in [
            ("work-in.txt", "a/work-in.txt"),
            ("work-out.txt", "b/work-out.txt"),
        ] {
            let selected = read(&request(Some(name), false));
            let diff = selected.diff.unwrap();
            assert!(!diff.text.is_empty(), "{name} has a working diff");
            assert!(
                !diff.text.contains(outside),
                "outside working rename path is hidden"
            );
        }
        assert_eq!(
            listed
                .committed
                .iter()
                .map(|entry| entry.relative_path.as_str())
                .collect::<Vec<_>>(),
            [
                "incoming.txt",
                "inside.txt",
                "outgoing.txt",
                "rename-new.txt"
            ]
        );
        let renamed = listed
            .committed
            .iter()
            .find(|entry| entry.relative_path == "rename-new.txt")
            .unwrap();
        assert_eq!(renamed.status, ChangedFileStatus::Renamed);
        assert_eq!(
            renamed.previous_relative_path.as_deref(),
            Some("rename-old.txt")
        );
        let incoming = listed
            .committed
            .iter()
            .find(|entry| entry.relative_path == "incoming.txt")
            .unwrap();
        assert_eq!(incoming.status, ChangedFileStatus::Added);
        assert_eq!(incoming.previous_relative_path, None);
        assert_eq!((incoming.added_lines, incoming.removed_lines), (None, None));
        let outgoing = listed
            .committed
            .iter()
            .find(|entry| entry.relative_path == "outgoing.txt")
            .unwrap();
        assert_eq!(outgoing.status, ChangedFileStatus::Deleted);
        assert_eq!(outgoing.previous_relative_path, None);
        for name in ["incoming.txt", "outgoing.txt", "rename-new.txt"] {
            let selected = read(&request(Some(name), true));
            let diff = selected.diff.unwrap();
            assert!(!diff.text.is_empty(), "{name} has a diff");
            assert!(
                !diff.text.contains("outside-source.txt"),
                "outside rename source is hidden"
            );
            assert!(
                !diff.text.contains("outside-outgoing.txt"),
                "outside rename destination is hidden"
            );
        }
        let deleted = read(&request(Some("delete.txt"), false));
        assert!(deleted.diff.unwrap().text.contains("delete me"));
    }
}
