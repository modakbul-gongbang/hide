//! The Sessions of a Project a shell screen names (PRD S8 D-03): its session
//! history and the one session read beside it, for the web shell's Project
//! Sessions screen.
//!
//! The Swift right panel's Sessions follow the focused checkout and share
//! their reads with Project Memory (`memory.rs`): a refresh there cancels
//! another checkout's analysis, writes Memory counts and schedules a due
//! poll. A named Project is kept out of all of that. It reuses the same pure
//! readers (`load_sessions`, `load_session_detail`) and the same fence - a
//! generation per read, one read in flight and at most one waiting - but
//! nothing here reads the focus, and nothing here touches Memory state.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::json;

use super::memory::{load_session_detail, load_sessions};
use super::*;
use crate::model::{
    ArchiveDetailSnapshot, ProjectSessionDetailSnapshot, ProjectSessionsSnapshot,
    SessionRowSnapshot,
};

/// The reason a session the history listed before carries once its file is
/// no longer found (B5, D-04).
const SESSION_GONE: &str =
    "The session file can no longer be found. It may have been moved or deleted.";

/// The reason an open session gives when the history holds no row for it.
const SESSION_NOT_LISTED: &str =
    "This session is no longer in the Project's history. Its file may have been moved or deleted.";

/// The reads behind the named Project, beside its snapshot.
#[derive(Default)]
pub(super) struct ProjectSessionsWork {
    /// The folder the history is read from while the named Project can be
    /// read here; `None` for one on another device or gone from the catalog.
    pub(super) path: Option<String>,
    /// The Project identity (`hide_project`) the last history answered for;
    /// a session is read under it.
    pub(super) project_id: Option<String>,
    pub(super) list_generation: u64,
    pub(super) list_in_flight: bool,
    pub(super) list_waiting: bool,
    pub(super) detail_generation: u64,
    pub(super) detail_in_flight: bool,
    pub(super) detail_waiting: bool,
    /// Every session each Project's history has listed since the daemon
    /// started, by Project. One whose file is no longer found keeps its row,
    /// unavailable, instead of vanishing as if it never existed; only the
    /// Memory store remembers such a session across a restart, and the web
    /// shell's store has no record of any. A session belongs to one Project,
    /// so this holds at most what the catalog can list.
    pub(super) known: HashMap<String, Vec<SessionRowSnapshot>>,
}

impl Runtime {
    /// Names a catalog Project for a screen's Sessions and reads its history,
    /// or reads it again. Naming another Project leaves nothing of the
    /// previous one on screen, and a read still in flight for it can no
    /// longer land: its generation is behind (B6).
    pub(super) fn refresh_project_sessions(
        &mut self,
        device_id: Option<&str>,
        workspace_id: &str,
    ) -> bool {
        let device_id = device_id
            .filter(|device| !device.is_empty())
            .unwrap_or(workspace::LOCAL_DEVICE_ID);
        let named = self
            .snapshot
            .project_sessions
            .as_ref()
            .is_some_and(|sessions| {
                sessions.device_id == device_id && sessions.workspace_id == workspace_id
            });
        if !named {
            self.snapshot.project_sessions = Some(ProjectSessionsSnapshot {
                device_id: device_id.to_owned(),
                workspace_id: workspace_id.to_owned(),
                unavailable_reason: None,
                loading: false,
                failure: None,
                rows: Vec::new(),
                detail: None,
            });
            self.project_sessions_work.project_id = None;
            self.project_sessions_work.detail_generation += 1;
        }
        self.project_sessions_work.list_generation += 1;
        match self.project_sessions_folder(device_id, workspace_id) {
            Err(reason) => {
                self.project_sessions_work.path = None;
                self.project_sessions_work.list_waiting = false;
                if let Some(sessions) = self.snapshot.project_sessions.as_mut() {
                    sessions.unavailable_reason = Some(reason);
                    sessions.loading = false;
                    sessions.failure = None;
                    sessions.rows.clear();
                    sessions.detail = None;
                }
                true
            }
            Ok(path) => {
                self.project_sessions_work.path = Some(path);
                if let Some(sessions) = self.snapshot.project_sessions.as_mut() {
                    sessions.unavailable_reason = None;
                    sessions.loading = true;
                    sessions.failure = None;
                }
                if self.project_sessions_work.list_in_flight {
                    self.project_sessions_work.list_waiting = true;
                } else {
                    self.spawn_project_sessions_read();
                }
                true
            }
        }
    }

    /// Where the named Project's history is read from, or why it is not read
    /// here: sessions are read only from this machine's provider folders, so
    /// a Project on an SSH device says so rather than showing local sessions
    /// in its place (B7).
    fn project_sessions_folder(
        &self,
        device_id: &str,
        workspace_id: &str,
    ) -> Result<String, String> {
        if device_id != workspace::LOCAL_DEVICE_ID {
            return Err(self.device_sessions_reason(device_id));
        }
        let workspace = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .ok_or_else(|| "This Project is no longer registered.".to_owned())?;
        if workspace.remote_target_id.is_some() || workspace.device_id != workspace::LOCAL_DEVICE_ID
        {
            return Err(self.device_sessions_reason(&workspace.device_id));
        }
        Ok(workspace.path.clone())
    }

    fn device_sessions_reason(&self, device_id: &str) -> String {
        let label = |id: &str| {
            self.snapshot
                .navigator
                .devices
                .iter()
                .find(|device| device.id == id)
                .map(|device| device.label.clone())
        };
        let device = label(device_id).unwrap_or_else(|| device_id.to_owned());
        let here =
            label(workspace::LOCAL_DEVICE_ID).unwrap_or_else(|| workspace::local_device().label);
        format!(
            "Sessions on {device} are not available here. Hide reads Codex and Claude Code sessions only on {here}."
        )
    }

    /// Starts the history read; true when it could not start and the
    /// snapshot now says so.
    fn spawn_project_sessions_read(&mut self) -> bool {
        let Some(path) = self.project_sessions_work.path.clone() else {
            return false;
        };
        let generation = self.project_sessions_work.list_generation;
        let started = match (self.worker_context.clone(), self.home_path.clone()) {
            (Some(context), Some(home)) => {
                let database = self.memory_database_path();
                thread::Builder::new()
                    .name("hide-project-sessions-read".to_owned())
                    .spawn(move || {
                        let result = load_sessions(&home, &database, &path);
                        let Some(runtime) = context.runtime.upgrade() else {
                            return;
                        };
                        let changed = match runtime.lock() {
                            Ok(mut guard) => guard.ingest_project_sessions(generation, result),
                            Err(_) => return,
                        };
                        drop(runtime);
                        if changed {
                            context.notifier.notify();
                        }
                    })
                    .map_err(|error| format!("The session reader could not start: {error}"))
            }
            (None, _) => Err("The session reader is unavailable.".to_owned()),
            (_, None) => Err(
                "The home directory is unavailable, so no session folder can be read.".to_owned(),
            ),
        };
        match started {
            Ok(_) => {
                self.project_sessions_work.list_in_flight = true;
                false
            }
            Err(message) => {
                self.push_diagnostic("project_sessions.read_not_started", message.clone());
                if let Some(sessions) = self.snapshot.project_sessions.as_mut() {
                    sessions.loading = false;
                    sessions.failure = Some(message);
                }
                true
            }
        }
    }

    pub(super) fn ingest_project_sessions(
        &mut self,
        generation: u64,
        result: Result<memory::SessionsLoad, String>,
    ) -> bool {
        self.project_sessions_work.list_in_flight = false;
        if generation != self.project_sessions_work.list_generation {
            // A newer request superseded this read: another Project was named
            // or the same one was read again. Only the newest may land.
            return std::mem::take(&mut self.project_sessions_work.list_waiting)
                && self.spawn_project_sessions_read();
        }
        self.project_sessions_work.list_waiting = false;
        let Some(sessions) = self.snapshot.project_sessions.as_mut() else {
            return false;
        };
        sessions.loading = false;
        match result {
            Err(message) => {
                crate::diagnostic!(json!({
                    "kind": "project_sessions.read_failed",
                    "workspace_id": sessions.workspace_id,
                    "generation": generation,
                    "message": message,
                }));
                let failure = history_failure(&message);
                sessions.rows.clear();
                // The open session would be read against rows this read did
                // not produce: a read of it still waiting is dropped, and one
                // that never showed its conversation says why (B5).
                if let Some(detail) = sessions.detail.as_mut() {
                    self.project_sessions_work.detail_generation += 1;
                    self.project_sessions_work.detail_waiting = false;
                    if detail.loading {
                        detail.loading = false;
                        if detail.archive.is_none() {
                            detail.failure = Some(failure.clone());
                        }
                    }
                }
                sessions.failure = Some(failure);
                true
            }
            Ok(load) => {
                sessions.failure = None;
                let mut rows = load
                    .rows
                    .into_iter()
                    .map(|mut row| {
                        row.unavailable_reason =
                            row.unavailable_reason.as_deref().map(session_reason);
                        row
                    })
                    .collect::<Vec<_>>();
                // A session listed before and not found now stays listed as
                // unavailable with its last location, so a moved or deleted
                // file reads differently from one that never existed (B5).
                let listed = rows
                    .iter()
                    .map(|row| (row.provider.clone(), row.id.clone()))
                    .collect::<HashSet<_>>();
                let known = self
                    .project_sessions_work
                    .known
                    .entry(sessions.workspace_id.clone())
                    .or_default();
                let gone = known
                    .iter()
                    .filter(|row| !listed.contains(&(row.provider.clone(), row.id.clone())))
                    .map(|row| SessionRowSnapshot {
                        unavailable_reason: Some(SESSION_GONE.to_owned()),
                        ..row.clone()
                    })
                    .collect::<Vec<_>>();
                if !gone.is_empty() {
                    rows.extend(gone);
                    rows.sort_by(|left, right| {
                        right
                            .updated_at_unix_ms
                            .cmp(&left.updated_at_unix_ms)
                            .then_with(|| left.id.cmp(&right.id))
                    });
                }
                *known = rows.clone();
                sessions.rows = rows;
                self.project_sessions_work.project_id = Some(load.project_id);
                // A Retry reads the history and then the open session against
                // its fresh row, so a source that came back opens and one that
                // went away says so: one action, one event (A6).
                self.reread_open_project_session();
                true
            }
        }
    }

    /// Opens one session of the named Project beside its history. A request
    /// for a Project that is no longer named is stale and changes nothing.
    pub(super) fn open_project_session(
        &mut self,
        workspace_id: &str,
        kind: &str,
        id: &str,
    ) -> bool {
        let Some(sessions) = self.snapshot.project_sessions.as_mut() else {
            return false;
        };
        if sessions.workspace_id != workspace_id {
            return false;
        }
        if kind != "session" {
            self.set_error(
                "archive.kind_unsupported",
                "Only a session opens beside a Project's Sessions.",
                false,
            );
            return true;
        }
        let previous = sessions
            .detail
            .take()
            .filter(|detail| detail.session_id == id);
        sessions.detail = Some(ProjectSessionDetailSnapshot {
            session_id: id.to_owned(),
            locator: String::new(),
            loading: false,
            failure: None,
            // The same session read again keeps its conversation on screen
            // until the new read lands.
            archive: previous.and_then(|detail| detail.archive),
        });
        self.reread_open_project_session();
        true
    }

    /// Reads the open session against its current row, or says why it cannot.
    fn reread_open_project_session(&mut self) {
        let Some(sessions) = self.snapshot.project_sessions.as_mut() else {
            return;
        };
        let Some(detail) = sessions.detail.as_mut() else {
            return;
        };
        self.project_sessions_work.detail_generation += 1;
        match sessions.rows.iter().find(|row| row.id == detail.session_id) {
            None => {
                detail.loading = false;
                detail.locator.clear();
                detail.archive = None;
                detail.failure = Some(SESSION_NOT_LISTED.to_owned());
            }
            Some(row) => {
                detail.locator = row.locator.clone();
                detail.loading = true;
                detail.failure = None;
                if self.project_sessions_work.detail_in_flight {
                    self.project_sessions_work.detail_waiting = true;
                } else {
                    self.spawn_project_session_read();
                }
            }
        }
    }

    /// Starts reading the open session; true when it could not start and
    /// the snapshot now says so.
    fn spawn_project_session_read(&mut self) -> bool {
        let generation = self.project_sessions_work.detail_generation;
        let project_id = self.project_sessions_work.project_id.clone();
        let Some(sessions) = self.snapshot.project_sessions.as_ref() else {
            return false;
        };
        let Some(detail) = sessions.detail.as_ref() else {
            return false;
        };
        // The conversation on screen goes with the read, so an unchanged one
        // keeps its shared copy and the delta keeps comparing it by pointer;
        // the comparison runs on the worker, not under the lock.
        let shown = detail.archive.clone();
        let Some(row) = sessions
            .rows
            .iter()
            .find(|row| row.id == detail.session_id)
            .cloned()
        else {
            if let Some(detail) = self
                .snapshot
                .project_sessions
                .as_mut()
                .and_then(|sessions| sessions.detail.as_mut())
            {
                detail.loading = false;
                detail.archive = None;
                detail.failure = Some(SESSION_NOT_LISTED.to_owned());
            }
            return true;
        };
        let started = match (self.worker_context.clone(), project_id) {
            (Some(context), Some(project_id)) => {
                let database = self.memory_database_path();
                thread::Builder::new()
                    .name("hide-project-session-read".to_owned())
                    .spawn(move || {
                        let result =
                            load_session_detail(&database, &project_id, row).map(|archive| {
                                match shown {
                                    Some(shown) if *shown == archive => shown,
                                    _ => Arc::new(archive),
                                }
                            });
                        let Some(runtime) = context.runtime.upgrade() else {
                            return;
                        };
                        let changed = match runtime.lock() {
                            Ok(mut guard) => guard.ingest_project_session(generation, result),
                            Err(_) => return,
                        };
                        drop(runtime);
                        if changed {
                            context.notifier.notify();
                        }
                    })
                    .map_err(|error| format!("The session reader could not start: {error}"))
            }
            (None, _) => Err("The session reader is unavailable.".to_owned()),
            (_, None) => Err("The Project's history has not been read yet.".to_owned()),
        };
        match started {
            Ok(_) => {
                self.project_sessions_work.detail_in_flight = true;
                false
            }
            Err(message) => {
                self.push_diagnostic("project_sessions.detail_not_started", message.clone());
                if let Some(detail) = self
                    .snapshot
                    .project_sessions
                    .as_mut()
                    .and_then(|sessions| sessions.detail.as_mut())
                {
                    detail.loading = false;
                    detail.failure = Some(message);
                }
                true
            }
        }
    }

    fn ingest_project_session(
        &mut self,
        generation: u64,
        result: Result<Arc<ArchiveDetailSnapshot>, String>,
    ) -> bool {
        self.project_sessions_work.detail_in_flight = false;
        if generation != self.project_sessions_work.detail_generation {
            return std::mem::take(&mut self.project_sessions_work.detail_waiting)
                && self.spawn_project_session_read();
        }
        self.project_sessions_work.detail_waiting = false;
        let Some(detail) = self
            .snapshot
            .project_sessions
            .as_mut()
            .and_then(|sessions| sessions.detail.as_mut())
        else {
            return false;
        };
        detail.loading = false;
        match result {
            // A row the history lists as unavailable is answered with its
            // reason rather than read (`load_session_detail`).
            Ok(archive) => match archive.unavailable_reason.as_deref() {
                Some(reason) => {
                    detail.archive = None;
                    detail.failure = Some(session_reason(reason));
                }
                None => {
                    detail.failure = None;
                    detail.archive = Some(archive);
                }
            },
            Err(message) => {
                crate::diagnostic!(json!({
                    "kind": "project_sessions.detail_failed",
                    "session_id": detail.session_id,
                    "generation": generation,
                    "message": message,
                }));
                detail.archive = None;
                detail.failure = Some(session_reason(&message));
            }
        }
        true
    }
}

/// Why one session cannot be read, in words. The catalog reports codes that
/// the Swift panel keeps showing as they are; this screen says what they mean.
fn session_reason(reason: &str) -> String {
    match reason {
        "session_missing" => "The session file is missing.".to_owned(),
        "session_malformed" => "The session file could not be parsed.".to_owned(),
        "session_too_large" => too_large(),
        _ => match reason
            .strip_prefix("session_unreadable:")
            .or_else(|| reason.strip_prefix("Session unavailable: "))
        {
            Some(error) => read_failure(error),
            None => reason.to_owned(),
        },
    }
}

/// A session reader error (`hide_session::SessionError`'s text) in words;
/// an I/O failure keeps the operating system's own words after its code.
fn read_failure(error: &str) -> String {
    if error == "session_file_missing" {
        return "The session file is missing.".to_owned();
    }
    if error.starts_with("session_capacity:") {
        return too_large();
    }
    // `session_<operation>: <io error>`
    let detail = error
        .strip_prefix("session_")
        .and_then(|rest| rest.split_once(": "))
        .map_or(error, |(_, detail)| detail);
    format!("The session file could not be read: {detail}")
}

fn too_large() -> String {
    let limit_mib = hide_session::SESSION_READ_LIMIT_BYTES / (1024 * 1024);
    format!("The session file is larger than {limit_mib} MiB.")
}

/// Why a Project's history could not be read, in words: the catalog's and
/// the Project resolver's codes (`hide_project::ResolveError`) are not shown.
fn history_failure(message: &str) -> String {
    if let Some(path) = message.strip_prefix("project_path_missing:") {
        return format!("The Project folder {path} is missing.");
    }
    if let Some((path, error)) = message
        .strip_prefix("project_path_unreadable:")
        .and_then(|rest| rest.rsplit_once(':'))
    {
        return format!("The Project folder {path} could not be read: {error}");
    }
    // The link's reason is a resolver code, which the diagnostic keeps.
    if let Some((path, _)) = message
        .strip_prefix("project_git_link_invalid:")
        .and_then(|rest| rest.split_once(':'))
    {
        return format!("The Project's Git link at {path} is not valid.");
    }
    if let Some(limit) = message.strip_prefix("session_catalog_capacity:") {
        return format!(
            "The session folders hold more than {limit} entries, so Hide stopped reading them."
        );
    }
    if let Some(rest) = message.strip_prefix("session_catalog_") {
        // `<operation>:<path>:<io error>`
        let mut parts = rest.splitn(3, ':');
        if let (Some(_), Some(path), Some(error)) = (parts.next(), parts.next(), parts.next()) {
            return format!("The session folder {path} could not be read: {error}");
        }
    }
    format!("The Project's sessions could not be read: {message}")
}

#[cfg(test)]
mod tests {
    use super::{history_failure, session_reason};

    #[test]
    fn project_folder_failures_read_as_words_without_their_codes() {
        assert_eq!(
            history_failure("project_path_missing:/Volumes/ext/app"),
            "The Project folder /Volumes/ext/app is missing."
        );
        assert_eq!(
            history_failure("project_path_unreadable:/p/app:Permission denied (os error 13)"),
            "The Project folder /p/app could not be read: Permission denied (os error 13)"
        );
        assert_eq!(
            history_failure(
                "project_git_link_invalid:/p/app/.git:git_directory:No such file or directory (os error 2)"
            ),
            "The Project's Git link at /p/app/.git is not valid."
        );
        assert_eq!(
            history_failure(
                "session_catalog_read_directory:/h/.claude/projects:Permission denied (os error 13)"
            ),
            "The session folder /h/.claude/projects could not be read: Permission denied (os error 13)"
        );
    }

    #[test]
    fn session_reader_errors_read_as_words_without_their_codes() {
        assert_eq!(
            session_reason(
                "Session unavailable: session_open: No such file or directory (os error 2)"
            ),
            "The session file could not be read: No such file or directory (os error 2)"
        );
        assert_eq!(
            session_reason("Session unavailable: session_file_missing"),
            "The session file is missing."
        );
        assert_eq!(
            session_reason("Session unavailable: session_capacity:bytes:67108864"),
            "The session file is larger than 64 MiB."
        );
        assert_eq!(
            session_reason("session_unreadable:Permission denied (os error 13)"),
            "The session file could not be read: Permission denied (os error 13)"
        );
        assert_eq!(
            session_reason("Session source is no longer available"),
            "Session source is no longer available"
        );
    }
}
