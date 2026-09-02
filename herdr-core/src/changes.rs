//! Reads one checkout's Git working-tree state for the right panel's changes
//! view.
//!
//! Every `git` invocation happens here, on the session-sync coordinator
//! thread, never while the runtime mutex is held and never on a per-event
//! path: [`ChangesReader::read_if_due`] recomputes only when the request
//! changes or the refresh window lapses, and it produces nothing at all while
//! the changes view is not the visible section.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::model::{ChangedFileDiffSnapshot, ChangedFileSnapshot, ChangedFileStatus, ChangesSnapshot};

/// How stale the list may be while the view is open. Short enough that an edit
/// made in a terminal pane shows up by the time the operator looks over, long
/// enough that it is nowhere near a per-tick fork.
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// The diff text is carried whole on the snapshot wire, so it is bounded. A
/// diff past this is truncated with its reason stated rather than either
/// silently cut or allowed to dominate the wire.
const MAX_DIFF_BYTES: usize = 256 * 1024;

/// What the runtime wants read: the checkout to describe and the file whose
/// diff to fetch. Absent while the changes view is not showing, which is what
/// keeps the reader from forking anything in the common case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangesRequest {
    pub root_path: PathBuf,
    pub selected_path: Option<String>,
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

    let entries = match git_status(&toplevel) {
        Ok(status) => parse_status(&status, &toplevel),
        Err(reason) => {
            return ChangesSnapshot {
                root_path: Some(root_path),
                unavailable_reason: Some(reason),
                ..ChangesSnapshot::default()
            };
        }
    };

    // A selection that is no longer changed is dropped rather than kept
    // pointing at a diff that no longer exists.
    let selected = request
        .selected_path
        .as_ref()
        .and_then(|path| entries.iter().find(|entry| &entry.path == path));
    let diff = selected.map(|entry| read_diff(&toplevel, entry));

    ChangesSnapshot {
        root_path: Some(root_path),
        selected_path: selected.map(|entry| entry.path.clone()),
        entries,
        diff,
        unavailable_reason: None,
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
            "--no-renames",
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
        _ => run_git(toplevel, &["diff", "HEAD", "--", &entry.relative_path]).and_then(|output| {
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
            truncated_reason: Some(reason),
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
    let mut entries: Vec<ChangedFileSnapshot> = output
        .split('\0')
        .filter(|record| record.len() > 3)
        .map(|record| {
            let (code, rest) = record.split_at(2);
            let relative_path = rest.trim_start_matches(' ').to_owned();
            ChangedFileSnapshot {
                path: toplevel.join(&relative_path).to_string_lossy().into_owned(),
                relative_path,
                status: ChangedFileStatus::from_porcelain(code),
            }
        })
        .collect();
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    entries
}

fn truncate_diff(path: String, text: String) -> ChangedFileDiffSnapshot {
    if text.len() <= MAX_DIFF_BYTES {
        return ChangedFileDiffSnapshot {
            path,
            text,
            truncated_reason: None,
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
        truncated_reason: Some(format!(
            "This diff is larger than {} KB and is shown truncated",
            MAX_DIFF_BYTES / 1024
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_records_project_the_four_presented_statuses() {
        let output = " M src/lib.rs\0A  src/new.rs\0 D src/gone.rs\0?? notes.txt\0";
        let entries = parse_status(output, Path::new("/checkout"));
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.relative_path.as_str(), entry.status.as_str()))
                .collect::<Vec<_>>(),
            [
                ("notes.txt", "untracked"),
                ("src/gone.rs", "deleted"),
                ("src/lib.rs", "modified"),
                ("src/new.rs", "added"),
            ]
        );
        assert_eq!(entries[0].path, "/checkout/notes.txt");
    }

    #[test]
    fn a_staged_delete_reads_as_deleted_even_with_a_worktree_column() {
        assert_eq!(ChangedFileStatus::from_porcelain("AD"), ChangedFileStatus::Deleted);
        assert_eq!(ChangedFileStatus::from_porcelain("MM"), ChangedFileStatus::Modified);
        assert_eq!(ChangedFileStatus::from_porcelain("R "), ChangedFileStatus::Modified);
    }

    #[test]
    fn a_path_with_a_space_survives_the_nul_split() {
        let entries = parse_status(" M a file.rs\0", Path::new("/checkout"));
        assert_eq!(entries[0].relative_path, "a file.rs");
    }

    #[test]
    fn an_oversized_diff_states_that_it_was_cut() {
        let projected = truncate_diff("/x".to_owned(), "a".repeat(MAX_DIFF_BYTES + 1));
        assert_eq!(projected.text.len(), MAX_DIFF_BYTES);
        assert!(projected.truncated_reason.is_some());
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
        let root = std::env::temp_dir().join(format!(
            "herdr-core-changes-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("the temporary checkout root is creatable");
        let mut reader = ChangesReader::new();
        let projected = reader
            .read_if_due(Some(ChangesRequest {
                root_path: root.clone(),
                selected_path: None,
            }))
            .expect("first read is always due");
        let _ = std::fs::remove_dir_all(&root);
        assert!(projected.entries.is_empty());
        assert!(projected.unavailable_reason.is_some());
    }
}
