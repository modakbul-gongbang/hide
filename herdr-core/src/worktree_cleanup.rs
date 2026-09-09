//! Reviewed, non-force removal of clean linked worktrees merged into local main.
//! All inspection and filesystem effects run on the existing action-worker path.
use super::{LiveContext, control_request};
use crate::{disk, model::DiskUsageSnapshot, worktrees::git};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CleanupSnapshot {
    pub id: u64,
    pub repository_root: String,
    pub phase: String,
    pub main_head: Option<String>,
    pub rows: Vec<CleanupRow>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CleanupRow {
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub exclusion: Option<String>,
    pub disk: DiskUsageSnapshot,
    pub result: Option<String>,
    pub message: Option<String>,
}

/// Git's NUL porcelain avoids interpreting newlines or quoting in folder names.
fn listed(root: &Path) -> Result<Vec<CleanupRow>, String> {
    let text = git(root, &["worktree", "list", "--porcelain", "-z"])?;
    let mut rows = Vec::new();
    for record in text.split("\0\0").filter(|v| !v.is_empty()) {
        let fields: Vec<_> = record.split('\0').collect();
        let path = fields
            .iter()
            .find_map(|v| v.strip_prefix("worktree "))
            .ok_or("Git returned an invalid worktree record. Refresh the review.")?;
        let mut row = CleanupRow {
            path: path.into(),
            ..Default::default()
        };
        row.branch = fields
            .iter()
            .find_map(|v| v.strip_prefix("branch refs/heads/"))
            .map(str::to_owned);
        row.head = fields
            .iter()
            .find_map(|v| v.strip_prefix("HEAD "))
            .map(str::to_owned);
        row.exclusion = if rows.is_empty() {
            Some("Main checkout".into())
        } else if fields.iter().any(|v| v.starts_with("locked")) {
            Some("Worktree is locked. Unlock it separately before reviewing again.".into())
        } else if fields
            .iter()
            .any(|v| v.starts_with("prunable") || *v == "bare")
        {
            Some("Worktree is unavailable".into())
        } else {
            None
        };
        rows.push(row);
    }
    if rows.is_empty() {
        return Err("Git returned no main checkout. Refresh the review.".into());
    }
    Ok(rows)
}

fn pane_paths(context: &LiveContext) -> Result<Vec<PathBuf>, String> {
    let response = control_request(
        context.api_connector.as_ref(),
        "session.snapshot",
        serde_json::json!({}),
    )?;
    crate::wire::cleanup_usage_paths(response).map_err(|e| e.message().to_owned())?
        .iter().map(|cwd| cwd.as_deref())
        .map(|cwd| {
            let path = cwd.ok_or("A live pane's folder is unknown. Resolve it before cleanup.")?;
            std::fs::canonicalize(path).map_err(|_| {
                "A live pane's folder cannot be verified. Resolve it before cleanup.".into()
            })
        })
        .collect()
}

fn inspect(
    root: &Path,
    current: &Path,
    usage: Result<Vec<PathBuf>, String>,
) -> Result<CleanupSnapshot, String> {
    let mut rows = listed(root)?;
    let main_head = git(root, &["rev-parse", "--verify", "refs/heads/main^{commit}"])
        .map(|v| v.trim().to_owned());
    let canonical_current =
        std::fs::canonicalize(current).map_err(|_| "The current checkout cannot be verified.")?;
    let paths: Vec<PathBuf> = rows.iter().map(|r| PathBuf::from(&r.path)).collect();
    for row in &mut rows {
        if row.exclusion.is_some() {
            continue;
        }
        let checked = (|| -> Result<(), String> {
            let path = Path::new(&row.path);
            let canonical =
                std::fs::canonicalize(path).map_err(|_| "Folder is missing or unreadable")?;
            if canonical != path {
                return Err("Folder is an alias. Refresh authoritative worktree paths.".into());
            }
            if canonical == canonical_current {
                return Err("Current checkout".into());
            }
            // Removing an ancestor would also remove another checkout's files.
            if paths
                .iter()
                .any(|other| other != path && other.starts_with(path))
            {
                return Err("Contains another registered worktree".into());
            }
            let usage = usage.as_ref().map_err(Clone::clone)?;
            if usage.iter().any(|cwd| cwd.starts_with(&canonical)) {
                return Err(
                    "In use by a live pane or agent. Close or move it, then review again.".into(),
                );
            }
            let main = main_head.as_ref().map_err(|_| "Local main is unavailable. Fetching or another base cannot establish eligibility.")?;
            let head = row.head.as_ref().ok_or("Worktree HEAD is unknown")?;
            if row.branch.is_none() {
                return Err("Detached HEAD. Review this worktree separately.".into());
            }
            if !git(path, &["status", "--porcelain=v1", "--untracked-files=all"])?
                .trim()
                .is_empty()
            {
                return Err(
                    "Uncommitted or untracked files. Commit or move them, then review again."
                        .into(),
                );
            }
            let range = format!("{main}..{head}");
            let count = git(root, &["rev-list", "--count", &range, "--"])?;
            match count.trim().parse::<u64>() {
                Ok(0) => Ok(()),
                Ok(_) => Err("Not merged into local main".into()),
                Err(_) => Err("Merge status could not be verified".into()),
            }
        })();
        row.exclusion = checked.err();
    }
    Ok(CleanupSnapshot {
        repository_root: root.to_string_lossy().into_owned(),
        main_head: main_head.ok(),
        rows,
        phase: "review".into(),
        ..Default::default()
    })
}

fn confirm(
    review: &CleanupSnapshot,
    selected: &[String],
    mut refresh: impl FnMut() -> Result<CleanupSnapshot, String>,
    mut remove: impl FnMut(&str) -> Result<(), String>,
) -> CleanupSnapshot {
    let mut result = review.clone();
    result.phase = "complete".into();
    for row in &mut result.rows {
        if !selected.contains(&row.path) || row.result.as_deref() == Some("removed") {
            continue;
        }
        let validation = (|| -> Result<bool, String> {
            if let Some(reason) = &row.exclusion {
                return Err(format!("Excluded: {reason}"));
            }
            let fresh = refresh()?;
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
        let removed = validation.and_then(|needed| if needed { remove(&row.path) } else { Ok(()) });
        match removed {
            Ok(()) => {
                row.result = Some("removed".into());
                row.message = Some("Worktree folder removed. Branch and Git history kept.".into());
            }
            Err(message) => {
                row.result = Some("refused".into());
                row.message = Some(message);
            }
        }
    }
    result
}

pub fn spawn(
    context: LiveContext,
    review: CleanupSnapshot,
    selected: Option<Vec<String>>,
    _current: String,
) -> Result<(), String> {
    std::thread::Builder::new().name("hide-worktree-cleanup".into()).spawn(move || {
        let root = PathBuf::from(&review.repository_root);
        let refresh = || {
            // Copy focus only. Git and filesystem reads begin after this guard drops.
            let focused = context.runtime.upgrade().and_then(|runtime| runtime.lock().ok()
                .and_then(|guard| guard.cleanup_current_path())).ok_or("Current checkout is unavailable. Reopen Overview and review again.")?;
            inspect(&root, Path::new(&focused), pane_paths(&context))
        };
        let mut answer = if let Some(selected) = selected {
            confirm(&review, &selected, refresh, |path| {
                git(&root, &["worktree", "remove", "--", path])?;
                if listed(&root)?.iter().any(|r| r.path == path) || Path::new(path).try_exists().map_err(|e| e.to_string())? {
                    return Err("Git acknowledged removal but the folder or registration remains. Review again.".into());
                }
                Ok(())
            })
        } else {
            match refresh() {
                Ok(mut value) => {
                    let shared = git(&root, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).ok().map(|v| PathBuf::from(v.trim()));
                    let mut paths: Vec<_> = value.rows.iter().map(|r| PathBuf::from(&r.path)).collect();
                    paths.extend(shared);
                    let measured = disk::read(&disk::DiskRequest { paths, generation: review.id });
                    for row in &mut value.rows {
                        row.disk = measured.iter().find(|d| d.path.as_deref() == Some(&row.path)).cloned().unwrap_or_default();
                    }
                    value
                }
                Err(message) => CleanupSnapshot { phase: "failed".into(), message: Some(message), ..review.clone() },
            }
        };
        answer.id = review.id;
        if let Some(runtime) = context.runtime.upgrade() {
            if let Ok(mut guard) = runtime.lock() { guard.ingest_cleanup(answer); }
            context.notifier.notify();
        }
    }).map(|_| ()).map_err(|e| format!("Cleanup worker could not start: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        main: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "hide-cleanup-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(root.join("main")).unwrap();
            let root = std::fs::canonicalize(root).unwrap();
            let main = root.join("main");
            git(&main, &["init", "-b", "main"]).unwrap();
            std::fs::write(main.join("tracked"), "keep").unwrap();
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
            inspect(&self.main, current, usage).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
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
                git(&f.main, &["worktree", "remove", "--", path]).map(|_| ())
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
                git(&f.main, &["worktree", "remove", "--", path]).map(|_| ())
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
}
