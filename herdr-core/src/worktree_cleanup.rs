//! Reviewed, non-force removal of clean linked worktrees merged into local main.
//! All inspection and filesystem effects run on the existing action-worker path.
use super::{LiveContext, control_request};
use crate::{disk, github, model::DiskUsageSnapshot, worktrees::git};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

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
) -> Result<(), String> {
    let range = format!("{main}..{head}");
    let count = git(root, &["rev-list", "--count", &range, "--"])?;
    match count.trim().parse::<u64>() {
        Ok(0) => return Ok(()),
        Err(_) => return Err("Merge status could not be verified".into()),
        Ok(_) => {}
    }

    let proofs = merged_proofs(branch)?;
    let Some(proof) = proofs.iter().find(|proof| proof.head_oid == head) else {
        return Err(
            "Not merged into local main, and no merged pull request exactly matches this worktree HEAD"
                .into(),
        );
    };
    let merge_oid = proof
        .merge_oid
        .as_deref()
        .ok_or("The matching merged pull request has no merge commit identity")?;
    git(root, &["merge-base", "--is-ancestor", merge_oid, main]).map_err(|_| {
        String::from(
            "The matching pull request is merged on GitHub but its merge commit is not in local main. Update main and review again.",
        )
    })?;
    Ok(())
}

fn inspect(
    root: &Path,
    current: &Path,
    usage: Result<Vec<PathBuf>, String>,
) -> Result<CleanupSnapshot, String> {
    inspect_with_merge_proofs(root, current, usage, |branch| {
        github::merged_pull_request_proofs(root, branch)
    })
}

fn inspect_with_merge_proofs(
    root: &Path,
    current: &Path,
    usage: Result<Vec<PathBuf>, String>,
    mut merged_proofs: impl FnMut(&str) -> Result<Vec<github::MergedPullRequestProof>, String>,
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
            verified_merge(
                root,
                main,
                row.branch.as_deref().expect("branch checked above"),
                head,
                &mut merged_proofs,
            )
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum RemovalOutcome {
    Removed(String),
    Partial(String),
}

fn confirm(
    review: &CleanupSnapshot,
    selected: &[String],
    mut refresh: impl FnMut() -> Result<CleanupSnapshot, String>,
    mut remove: impl FnMut(&str) -> Result<RemovalOutcome, String>,
) -> CleanupSnapshot {
    let mut result = review.clone();
    result.phase = "complete".into();
    for row in &mut result.rows {
        if !selected.contains(&row.path)
            || matches!(row.result.as_deref(), Some("removed" | "partial"))
        {
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
        let removed = validation.and_then(|needed| {
            if needed {
                remove(&row.path)
            } else {
                Ok(RemovalOutcome::Removed(
                    "Worktree was already removed. Branch and Git history stay.".into(),
                ))
            }
        });
        match removed {
            Ok(RemovalOutcome::Removed(message)) => {
                row.result = Some("removed".into());
                row.message = Some(message);
            }
            Ok(RemovalOutcome::Partial(message)) => {
                row.result = Some("partial".into());
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

fn owned_scratch_root(repository_root: &Path, checkout: &Path) -> Result<Option<PathBuf>, String> {
    let script = repository_root.join("scripts/build-scratch.sh");
    if !script.is_file() {
        return Ok(None);
    }
    let output = Command::new("bash")
        .args([
            "-c",
            "set -e; . \"$1\"; printf '%s' \"$HIDE_SCRATCH_ROOT\"",
            "hide-scratch-root",
        ])
        .arg(&script)
        .current_dir(checkout)
        .output()
        .map_err(|error| format!("Owned test cache path could not be computed: {error}"))?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if reason.is_empty() {
            format!(
                "Owned test cache path could not be computed: {}",
                output.status
            )
        } else {
            format!("Owned test cache path could not be computed: {reason}")
        });
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).as_ref());
    // SAFETY: geteuid reads process identity and has no pointer or lifetime preconditions.
    let effective_uid = unsafe { libc::geteuid() };
    let expected_parent = PathBuf::from(format!("/tmp/hide-verify-{effective_uid}"));
    let key = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if path.parent() != Some(expected_parent.as_path())
        || key.len() != 64
        || !key.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(format!(
            "Owned test cache path was outside the expected checkout cache root: {}",
            path.display()
        ));
    }
    Ok(Some(path))
}

fn remove_worktree_and_owned_scratch(
    repository_root: &Path,
    checkout: &Path,
) -> Result<RemovalOutcome, String> {
    let scratch = owned_scratch_root(repository_root, checkout)?;
    git(
        repository_root,
        &["worktree", "remove", "--", &checkout.to_string_lossy()],
    )?;
    if listed(repository_root)?
        .iter()
        .any(|row| Path::new(&row.path) == checkout)
        || checkout.try_exists().map_err(|error| error.to_string())?
    {
        return Err(
            "Git acknowledged removal but the folder or registration remains. Review again.".into(),
        );
    }
    let Some(scratch) = scratch else {
        return Ok(RemovalOutcome::Removed(
            "Worktree folder removed. This repository defines no owned external test cache. Branch and Git history kept."
                .into(),
        ));
    };
    if !scratch.try_exists().map_err(|error| error.to_string())? {
        return Ok(RemovalOutcome::Removed(
            "Worktree folder removed. No owned external test cache remained. Branch and Git history kept."
                .into(),
        ));
    }
    let metadata = match std::fs::symlink_metadata(&scratch) {
        Ok(metadata) => metadata,
        Err(error) => {
            return Ok(RemovalOutcome::Partial(format!(
                "Worktree folder removed, but its owned test cache could not be inspected: {error}"
            )));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(RemovalOutcome::Partial(format!(
            "Worktree folder removed, but the owned test cache was not a normal directory and was kept: {}",
            scratch.display()
        )));
    }
    if let Err(error) = std::fs::remove_dir_all(&scratch) {
        return Ok(RemovalOutcome::Partial(format!(
            "Worktree folder removed, but its owned test cache could not be removed: {error}"
        )));
    }
    Ok(RemovalOutcome::Removed(
        "Worktree folder and owned external test cache removed. Branch and Git history kept."
            .into(),
    ))
}

pub fn spawn(
    context: LiveContext,
    review: CleanupSnapshot,
    selected: Option<Vec<String>>,
    _current: String,
) -> Result<(), String> {
    std::thread::Builder::new()
        .name("hide-worktree-cleanup".into())
        .spawn(move || {
            let root = PathBuf::from(&review.repository_root);
            let refresh = || {
                // Copy focus only. Git and filesystem reads begin after this guard drops.
                let focused = context
                    .runtime
                    .upgrade()
                    .and_then(|runtime| {
                        runtime
                            .lock()
                            .ok()
                            .and_then(|guard| guard.cleanup_current_path())
                    })
                    .ok_or("Current checkout is unavailable. Reopen Overview and review again.")?;
                inspect(&root, Path::new(&focused), pane_paths(&context))
            };
            let mut answer = if let Some(selected) = selected {
                confirm(&review, &selected, refresh, |path| {
                    remove_worktree_and_owned_scratch(&root, Path::new(path))
                })
            } else {
                match refresh() {
                    Ok(mut value) => {
                        let shared = git(
                            &root,
                            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                        )
                        .ok()
                        .map(|v| PathBuf::from(v.trim()));
                        let mut paths: Vec<_> =
                            value.rows.iter().map(|r| PathBuf::from(&r.path)).collect();
                        paths.extend(shared);
                        let measured = disk::read(&disk::DiskRequest {
                            paths,
                            generation: review.id,
                        });
                        for row in &mut value.rows {
                            row.disk = measured
                                .iter()
                                .find(|d| d.path.as_deref() == Some(&row.path))
                                .cloned()
                                .unwrap_or_default();
                        }
                        value
                    }
                    Err(message) => CleanupSnapshot {
                        phase: "failed".into(),
                        message: Some(message),
                        ..review.clone()
                    },
                }
            };
            answer.id = review.id;
            if let Some(runtime) = context.runtime.upgrade() {
                if let Ok(mut guard) = runtime.lock() {
                    guard.ingest_cleanup(answer);
                }
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|e| format!("Cleanup worker could not start: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
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
            inspect_with_merge_proofs(&self.main, current, usage, |_| Ok(Vec::new())).unwrap()
        }
        fn review_with_proofs(
            &self,
            current: &Path,
            proofs: Vec<github::MergedPullRequestProof>,
        ) -> CleanupSnapshot {
            inspect_with_merge_proofs(&self.main, current, Ok(Vec::new()), |_| Ok(proofs.clone()))
                .unwrap()
        }
        fn install_scratch_script(&self) {
            let scripts = self.main.join("scripts");
            std::fs::create_dir_all(&scripts).unwrap();
            std::fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap()
                    .join("scripts/build-scratch.sh"),
                scripts.join("build-scratch.sh"),
            )
            .unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
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

        let unavailable = inspect_with_merge_proofs(&f.main, &f.main, Ok(Vec::new()), |_| {
            Err("GitHub merge proof is unavailable".into())
        })
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
    fn removal_deletes_only_the_selected_worktree_and_its_owned_test_cache() {
        let f = Fixture::new();
        f.install_scratch_script();
        let clean = f.add("with-cache");
        let scratch = owned_scratch_root(&f.main, &clean).unwrap().unwrap();
        std::fs::create_dir_all(scratch.join("cargo")).unwrap();
        std::fs::write(scratch.join("cargo/artifact"), "regenerable").unwrap();
        let review = f.review(&f.main, Ok(Vec::new()));
        let selected = vec![clean.to_string_lossy().into_owned()];

        let result = confirm(
            &review,
            &selected,
            || Ok(f.review(&f.main, Ok(Vec::new()))),
            |path| remove_worktree_and_owned_scratch(&f.main, Path::new(path)),
        );

        assert!(!clean.exists());
        assert!(!scratch.exists());
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
    fn repository_without_scratch_convention_keeps_ordinary_removal_behavior() {
        let f = Fixture::new();
        let clean = f.add("without-cache-convention");
        let outcome = remove_worktree_and_owned_scratch(&f.main, &clean).unwrap();
        assert!(matches!(outcome, RemovalOutcome::Removed(_)));
        assert!(!clean.exists());
        assert!(
            git(
                &f.main,
                &[
                    "show-ref",
                    "--verify",
                    "refs/heads/without-cache-convention"
                ]
            )
            .is_ok()
        );
    }

    #[test]
    fn cache_failure_is_partial_success_and_is_not_repeated() {
        let f = Fixture::new();
        let clean = f.add("partial");
        let review = f.review(&f.main, Ok(Vec::new()));
        let selected = vec![clean.to_string_lossy().into_owned()];
        let result = confirm(
            &review,
            &selected,
            || Ok(f.review(&f.main, Ok(Vec::new()))),
            |path| {
                git(&f.main, &["worktree", "remove", "--", path])?;
                Ok(RemovalOutcome::Partial(
                    "Worktree folder removed, but its owned test cache was kept".into(),
                ))
            },
        );
        assert_eq!(
            result
                .rows
                .iter()
                .find(|row| row.path == selected[0])
                .and_then(|row| row.result.as_deref()),
            Some("partial")
        );
        let retry = confirm(
            &result,
            &selected,
            || panic!("partial success must not re-read a removed worktree"),
            |_| panic!("partial success must not repeat removal"),
        );
        assert_eq!(retry, result);
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
                    .map(|_| RemovalOutcome::Removed("Worktree removed".into()))
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
                    .map(|_| RemovalOutcome::Removed("Worktree removed".into()))
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
