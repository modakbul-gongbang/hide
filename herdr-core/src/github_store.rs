//! The last GitHub answer per project, kept across a restart.
//!
//! The runtime holds the authoritative copy; this file only lets a daemon that
//! has just started draw the sidebar's pull request state before its first
//! `gh` pass finishes. A restored answer is always stale until a read replaces
//! it, so a file that is days old is shown as old rather than as current.
//!
//! The write runs on its own thread, never under `Mutex<Runtime>`: the runtime
//! hands over an owned snapshot and the thread serializes and replaces the
//! file. One thread and one pending slot per store, so a burst of answers
//! writes the newest and skips the rest.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};

use crate::model::{GithubProjectSnapshot, GithubSnapshot};

const SCHEMA_VERSION: u32 = 1;

/// The most a file may hold. The projects (64), their pull requests (200) and
/// issues (200) are capped, so a file past this is not one this store wrote.
const READ_CAP: u64 = 32 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
struct Stored {
    schema_version: u32,
    projects: Vec<StoredProject>,
}

/// A project as the wire carries it, plus the two pull request times the wire
/// leaves out. They sit beside it, one entry per pull request, so a field
/// added to the wire type is stored without a change here.
#[derive(Deserialize, Serialize)]
struct StoredProject {
    #[serde(flatten)]
    project: GithubProjectSnapshot,
    pull_request_times: Vec<PullRequestTimes>,
}

#[derive(Deserialize, Serialize)]
struct PullRequestTimes {
    created_at_unix_ms: Option<u64>,
    closed_at_unix_ms: Option<u64>,
}

fn encode(github: &GithubSnapshot) -> Result<Vec<u8>, String> {
    let stored = Stored {
        schema_version: SCHEMA_VERSION,
        // Only a project that has been read successfully has an answer worth
        // restoring; the others would draw an empty list as if it were one.
        projects: github
            .projects
            .iter()
            .filter(|project| project.status.last_success_at_unix_ms.is_some())
            .map(|project| StoredProject {
                pull_request_times: project
                    .pull_requests
                    .iter()
                    .map(|pull_request| PullRequestTimes {
                        created_at_unix_ms: pull_request.created_at_unix_ms,
                        closed_at_unix_ms: pull_request.closed_at_unix_ms,
                    })
                    .collect(),
                project: project.clone(),
            })
            .collect(),
    };
    serde_json::to_vec(&stored).map_err(|error| format!("could not be encoded: {error}"))
}

/// What a restored project says about itself: read once, not current.
fn decode(bytes: &[u8]) -> Result<GithubSnapshot, String> {
    let stored: Stored =
        serde_json::from_slice(bytes).map_err(|error| format!("could not be decoded: {error}"))?;
    if stored.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "has schema version {}, expected {SCHEMA_VERSION}",
            stored.schema_version
        ));
    }
    let mut projects = Vec::with_capacity(stored.projects.len());
    for stored in stored.projects {
        let StoredProject {
            mut project,
            pull_request_times,
        } = stored;
        if pull_request_times.len() != project.pull_requests.len() {
            return Err(format!(
                "lists {} pull request times for {} pull requests in {}",
                pull_request_times.len(),
                project.pull_requests.len(),
                project.root_path
            ));
        }
        for (pull_request, times) in project.pull_requests.iter_mut().zip(pull_request_times) {
            pull_request.created_at_unix_ms = times.created_at_unix_ms;
            pull_request.closed_at_unix_ms = times.closed_at_unix_ms;
        }
        if project.status.last_success_at_unix_ms.is_none() {
            return Err(format!(
                "holds {} without a successful read",
                project.root_path
            ));
        }
        project.pull_requests_read = true;
        project.issues_read = true;
        project.status.stale = true;
        project.status.loading = false;
        projects.push(project);
    }
    Ok(GithubSnapshot { projects })
}

pub(crate) struct GithubStore {
    path: PathBuf,
    slot: Arc<Mutex<Slot>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Default)]
struct Slot {
    latest: Option<GithubSnapshot>,
    active: bool,
}

impl GithubStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            slot: Arc::default(),
            worker: Mutex::new(None),
        }
    }

    /// What the last run saved. A missing file is the first run and says
    /// nothing; any other reason a file is not used is stated, then the file is
    /// ignored and the next save replaces it.
    pub(crate) fn restore(&self) -> Option<GithubSnapshot> {
        let discard = |reason: String| {
            crate::diagnostic!(serde_json::json!({
                "component": "github",
                "kind": "snapshot.discarded",
                "path": self.path.to_string_lossy(),
                "message": reason,
            }));
            None
        };
        let bytes = match read_capped(&self.path) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return None,
            Err(reason) => return discard(reason),
        };
        match decode(&bytes) {
            Ok(github) => {
                crate::diagnostic!(serde_json::json!({
                    "component": "github",
                    "kind": "snapshot.restored",
                    "projects": github.projects.len(),
                }));
                Some(github)
            }
            Err(reason) => discard(reason),
        }
    }

    /// Queues `github` to replace the file. A snapshot still waiting when a
    /// newer one arrives is dropped: only the latest answer is worth writing.
    pub(crate) fn save(&self, github: GithubSnapshot) {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        slot.latest = Some(github);
        if slot.active {
            return;
        }
        let mut worker = self.worker.lock().unwrap_or_else(PoisonError::into_inner);
        // The previous worker has cleared `active` and is returning.
        if let Some(finished) = worker.take() {
            let _ = finished.join();
        }
        let shared = Arc::clone(&self.slot);
        let path = self.path.clone();
        match std::thread::Builder::new()
            .name("hide-github-save".to_owned())
            .spawn(move || write_latest(&shared, &path))
        {
            Ok(handle) => {
                slot.active = true;
                *worker = Some(handle);
            }
            Err(error) => crate::diagnostic!(serde_json::json!({
                "component": "github",
                "kind": "snapshot.save_failed",
                "message": format!("worker could not start: {error}"),
            })),
        }
    }
}

impl Drop for GithubStore {
    /// A save queued just before quitting reaches the disk first.
    fn drop(&mut self) {
        let worker = self
            .worker
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(worker) = worker
            && worker.join().is_err()
        {
            crate::diagnostic!(
                serde_json::json!({"component":"github","kind":"snapshot.save_join_failed"})
            );
        }
    }
}

fn write_latest(slot: &Mutex<Slot>, path: &Path) {
    loop {
        let github = {
            let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
            match slot.latest.take() {
                Some(github) => github,
                None => {
                    slot.active = false;
                    return;
                }
            }
        };
        if let Err(reason) = write(path, &github) {
            crate::diagnostic!(serde_json::json!({
                "component": "github",
                "kind": "snapshot.save_failed",
                "path": path.to_string_lossy(),
                "message": reason,
            }));
        }
    }
}

fn write(path: &Path, github: &GithubSnapshot) -> Result<(), String> {
    let bytes = encode(github)?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        hide_platform::fs::private::create_dir_all(parent)
            .map_err(|error| format!("folder could not be prepared: {error}"))?;
    }
    hide_platform::fs::atomic::write_file(path, &bytes, hide_platform::fs::Access::Private)
        .map(|_| ())
        .map_err(|error| format!("could not be replaced: {error}"))
}

fn read_capped(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not be opened: {error}")),
    };
    let length = file
        .metadata()
        .map_err(|error| format!("could not be measured: {error}"))?
        .len();
    if length > READ_CAP {
        return Err(format!("is {length} bytes, past the {READ_CAP} byte cap"));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    std::io::Read::read_to_end(&mut std::io::Read::take(file, READ_CAP), &mut bytes)
        .map_err(|error| format!("could not be read: {error}"))?;
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        GithubStatusSnapshot, PullRequestBadge, PullRequestChecks, PullRequestSnapshot,
    };

    fn answer(root: &str, last_success: Option<u64>) -> GithubProjectSnapshot {
        GithubProjectSnapshot {
            root_path: root.to_owned(),
            status: GithubStatusSnapshot {
                available: true,
                last_success_at_unix_ms: last_success,
                ..Default::default()
            },
            pull_requests: vec![PullRequestSnapshot {
                closing_issues: Vec::new(),
                title: "Fix".to_owned(),
                checks: PullRequestChecks::Failed,
                number: 12,
                head_branch: "fix".to_owned(),
                base_branch: "main".to_owned(),
                url: "https://example.invalid/pull/12".to_owned(),
                badge: PullRequestBadge::Closed,
                review: None,
                is_draft: false,
                merged_at_unix_ms: None,
                updated_at_unix_ms: Some(9),
                created_at_unix_ms: Some(4),
                closed_at_unix_ms: Some(8),
            }],
            pull_requests_read: true,
            issues_read: true,
            ..Default::default()
        }
    }

    fn saved_then_restored(github: GithubSnapshot) -> Option<GithubSnapshot> {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("github-snapshot.json");
        let store = GithubStore::new(path.clone());
        store.save(github);
        drop(store);
        GithubStore::new(path).restore()
    }

    #[test]
    fn a_saved_answer_comes_back_stale_with_every_pull_request_time() {
        let sent = GithubSnapshot {
            projects: vec![answer("/repo", Some(100))],
        };
        let restored = saved_then_restored(sent.clone()).expect("the file is restored");
        let project = &restored.projects[0];
        assert!(project.status.stale && !project.status.loading);
        assert_eq!(project.status.last_success_at_unix_ms, Some(100));
        assert_eq!(project.pull_requests, sent.projects[0].pull_requests);
        assert!(project.pull_requests_read && project.issues_read);
    }

    #[test]
    fn a_project_never_read_successfully_is_not_saved() {
        let restored = saved_then_restored(GithubSnapshot {
            projects: vec![answer("/never", None), answer("/repo", Some(1))],
        })
        .unwrap();
        assert_eq!(
            restored
                .projects
                .iter()
                .map(|project| project.root_path.as_str())
                .collect::<Vec<_>>(),
            vec!["/repo"]
        );
    }

    #[test]
    fn the_newest_of_several_saves_is_the_one_on_disk() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("github-snapshot.json");
        let store = GithubStore::new(path.clone());
        for at in 1..=20 {
            store.save(GithubSnapshot {
                projects: vec![answer("/repo", Some(at))],
            });
        }
        drop(store);
        let restored = GithubStore::new(path).restore().unwrap();
        assert_eq!(
            restored.projects[0].status.last_success_at_unix_ms,
            Some(20)
        );
    }

    #[test]
    fn a_missing_file_restores_nothing() {
        let folder = tempfile::tempdir().unwrap();
        assert!(
            GithubStore::new(folder.path().join("github-snapshot.json"))
                .restore()
                .is_none()
        );
    }

    #[test]
    fn a_damaged_or_other_version_file_is_not_used() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("github-snapshot.json");
        let store = GithubStore::new(path.clone());
        let good = encode(&GithubSnapshot {
            projects: vec![answer("/repo", Some(1))],
        })
        .unwrap();
        let other_version = String::from_utf8(good.clone()).unwrap().replacen(
            "\"schema_version\":1",
            "\"schema_version\":2",
            1,
        );
        let torn = &good[..good.len() / 2];
        let mismatched_times = String::from_utf8(good.clone()).unwrap().replacen(
            "\"pull_request_times\":[{",
            "\"pull_request_times\":[{},{",
            1,
        );
        for bytes in [
            b"not json".as_slice(),
            other_version.as_bytes(),
            torn,
            mismatched_times.as_bytes(),
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(store.restore().is_none());
        }
    }
}
