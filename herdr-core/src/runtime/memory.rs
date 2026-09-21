use super::*;
use crate::model::{
    ArchiveEventSnapshot, MemoryAnalysisSnapshot, MemoryDetailSnapshot, MemoryNoticeSnapshot,
    MemoryRevisionSnapshot, MemoryRowSnapshot, MemorySourceSnapshot, SessionsMode,
    SessionsProviderFilter, SessionsSnapshot,
};
use hide_agent_hooks::{HookEvent, HookStatus};
use hide_ai::{AiError, CancelToken};
use hide_memory::{
    ANALYSIS_INPUT_LIMIT_BYTES, AnalysisBatch, Candidate, CandidateRelation, ConflictChoice,
    HideNativeAnalyzer, Injection, InjectionOutcome, MemoryStore, SessionCursorRecord,
    SessionSourceRecord,
};
use hide_session::{
    Agent, EventKind, SESSION_READ_LIMIT_BYTES, SessionAvailability, SessionCatalog, SessionCursor,
    read_bounded,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;

const MEMORY_QUIESCENCE_MS: u64 = 60_000;
const MEMORY_POLL_INTERVAL_MS: u64 = 5_000;
const RELATION_CONTEXT_INPUT_LIMIT_BYTES: usize = 16 * 1024;

struct SessionsLoad {
    project_id: String,
    checkout_path: String,
    rows: Vec<SessionRowSnapshot>,
    memories: Vec<MemoryRowSnapshot>,
    state: Option<hide_memory::ProjectMemoryState>,
}

fn sessions_load_matches_scope(
    generation: u64,
    current_generation: u64,
    checkout_path: &str,
    focused_path: Option<&str>,
) -> bool {
    generation == current_generation && focused_path == Some(checkout_path)
}

fn archive_load_matches_scope(
    generation: u64,
    current_generation: u64,
    workspace_id: &str,
    checkout_id: &str,
    focused_scope: Option<(&str, &str)>,
) -> bool {
    generation == current_generation && focused_scope == Some((workspace_id, checkout_id))
}

impl Runtime {
    fn memory_database_path(&self) -> PathBuf {
        self.state_path.with_file_name("project-memory.sqlite3")
    }

    fn focused_memory_context(&self) -> Option<(String, String, String)> {
        let (workspace, checkout) = self.focused_local_checkout()?;
        Some((
            workspace.id.clone(),
            checkout.id.clone(),
            checkout.path.clone(),
        ))
    }

    /// Polls only as a coordinator-owned trigger. Session discovery, SQLite
    /// reads and provider work all remain outside the runtime mutex.
    pub(super) fn tick_project_memory(&mut self, now_unix_ms: u64) -> bool {
        if now_unix_ms < self.memory_next_poll_unix_ms
            || self.memory_poll_in_flight
            || self.memory_operation_in_flight
            || self.ai_settings.is_none()
            || self.snapshot.sessions.analysis.failed > 0
            || matches!(
                self.snapshot.sessions.analysis.state.as_str(),
                "paused" | "hooks_need_update"
            )
        {
            return false;
        }
        let Some((_, _, checkout_path)) = self.focused_memory_context() else {
            return false;
        };
        let Some(home) = self.home_path.clone() else {
            return false;
        };
        let Some(context) = self.worker_context.clone() else {
            return false;
        };
        self.memory_next_poll_unix_ms = now_unix_ms.saturating_add(MEMORY_POLL_INTERVAL_MS);
        self.memory_poll_in_flight = true;
        let database = self.memory_database_path();
        let spawn = thread::Builder::new()
            .name("hide-project-memory-poll".to_owned())
            .spawn(move || {
                let due = memory_analysis_due(&database, &home, &checkout_path, now_unix_ms);
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => {
                        guard.memory_poll_in_flight = false;
                        let still_focused = guard
                            .focused_memory_context()
                            .is_some_and(|(_, _, path)| path == checkout_path);
                        if !still_focused {
                            return;
                        }
                        match due {
                            Err(_) => {
                                guard.push_diagnostic(
                                    "memory.poll_failed",
                                    "Project Memory polling failed before analysis could be scheduled",
                                );
                                guard.snapshot.sessions.analysis = MemoryAnalysisSnapshot {
                                    state: "paused".to_owned(),
                                    message: Some("Memory unavailable".to_owned()),
                                    action: Some("retry".to_owned()),
                                    ..MemoryAnalysisSnapshot::default()
                                };
                                true
                            }
                            Ok(due) => {
                                if due && !guard.memory_operation_in_flight {
                                    guard.apply_memory_action(events::MemoryActionPayload {
                                        action: "retry".to_owned(),
                                        item_id: None,
                                        candidate_id: None,
                                        body: None,
                                        batch_id: None,
                                        conflict_choice: None,
                                    })
                                } else {
                                    false
                                }
                            }
                        }
                    }
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawn {
            self.memory_poll_in_flight = false;
            self.push_diagnostic(
                "memory.poll_worker_failed",
                format!("Project Memory poll worker could not start: {error}"),
            );
            return true;
        }
        false
    }

    pub(super) fn request_sessions_refresh(&mut self) -> bool {
        let Some((_, _, checkout_path)) = self.focused_memory_context() else {
            self.snapshot.sessions = SessionsSnapshot {
                unavailable_reason: Some("Choose a local Project to view sessions".to_owned()),
                ..SessionsSnapshot::default()
            };
            return true;
        };
        if self.memory_operation_in_flight
            && self.memory_operation_checkout_path.as_deref() != Some(checkout_path.as_str())
            && let Some(cancel) = &self.memory_cancel
        {
            cancel.cancel();
        }
        let Some(context) = self.worker_context.clone() else {
            self.snapshot.sessions.loading = false;
            self.snapshot.sessions.unavailable_reason =
                Some("Session reader is unavailable".to_owned());
            return true;
        };
        self.snapshot.sessions.loading = true;
        self.snapshot.sessions.unavailable_reason = None;
        self.memory_sessions_load_generation =
            self.memory_sessions_load_generation.saturating_add(1);
        let generation = self.memory_sessions_load_generation;
        if self.memory_sessions_load_in_flight {
            self.memory_sessions_load_pending = true;
            return true;
        }
        self.memory_sessions_load_in_flight = true;
        self.memory_sessions_load_pending = false;
        let home = self.home_path.clone();
        let database = self.memory_database_path();
        thread::Builder::new()
            .name("hide-project-memory-read".to_owned())
            .spawn(move || {
                let result = home
                    .as_deref()
                    .ok_or_else(|| "The home directory is unavailable".to_owned())
                    .and_then(|home| load_sessions(home, &database, &checkout_path));
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_sessions_load(generation, &checkout_path, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            })
            .map(|_| true)
            .unwrap_or_else(|error| {
                self.memory_sessions_load_in_flight = false;
                self.snapshot.sessions.loading = false;
                self.snapshot.sessions.unavailable_reason =
                    Some(format!("Session reader could not start: {error}"));
                true
            })
    }

    pub(super) fn refresh_sessions_after_catalog_change(&mut self, catalog_changed: bool) -> bool {
        if catalog_changed
            && self.snapshot.ui_state.right_panel_visible
            && matches!(
                self.snapshot.ui_state.right_panel_section,
                crate::model::RightPanelSection::Sessions
            )
        {
            self.request_sessions_refresh()
        } else {
            false
        }
    }

    fn ingest_sessions_load(
        &mut self,
        generation: u64,
        checkout_path: &str,
        result: Result<SessionsLoad, String>,
    ) -> bool {
        self.memory_sessions_load_in_flight = false;
        let current_path = self.focused_memory_context().map(|(_, _, path)| path);
        if !sessions_load_matches_scope(
            generation,
            self.memory_sessions_load_generation,
            checkout_path,
            current_path.as_deref(),
        ) {
            let pending = self.memory_sessions_load_pending;
            self.memory_sessions_load_pending = false;
            return if pending {
                self.request_sessions_refresh()
            } else {
                false
            };
        }
        self.memory_sessions_load_pending = false;
        self.snapshot.sessions.loading = false;
        match result {
            Err(message) => {
                self.snapshot.sessions.unavailable_reason = Some(message);
                true
            }
            Ok(load) => {
                let should_request_due_poll = should_request_memory_due_poll_after_load(
                    load.state.as_ref().is_some_and(|state| state.enabled),
                    &self.snapshot.sessions.analysis,
                );
                let mode = self
                    .snapshot
                    .ui_state
                    .sessions_mode_by_project
                    .get(&load.project_id)
                    .copied()
                    .unwrap_or_default();
                self.session_catalog_rows = load.rows;
                self.memory_catalog_rows = load.memories;
                self.snapshot.sessions.project_id = Some(load.project_id);
                self.snapshot.sessions.checkout_path = Some(load.checkout_path);
                self.snapshot.sessions.mode = mode;
                self.snapshot.sessions.total_session_count = self.session_catalog_rows.len();
                self.snapshot.sessions.unavailable_reason = None;
                if let Some(state) = load.state {
                    self.snapshot.sessions.memory_enabled = state.enabled;
                    self.snapshot.sessions.memory_disclosure_accepted =
                        state.disclosure_accepted_at_unix_ms.is_some();
                    self.snapshot.sessions.memory_active_count = state.active_count;
                    self.snapshot.sessions.memory_conflict_count = state.conflict_count;
                    self.snapshot.sessions.memory_capacity_reached = state.capacity_reached;
                } else {
                    self.snapshot.sessions.memory_enabled = false;
                    self.snapshot.sessions.memory_disclosure_accepted = false;
                    self.snapshot.sessions.memory_active_count = 0;
                    self.snapshot.sessions.memory_conflict_count = 0;
                    self.snapshot.sessions.memory_capacity_reached = false;
                }
                self.apply_session_filters();
                if should_request_due_poll {
                    // Session refreshes do not own analysis scheduling. Make
                    // the coordinator poll immediately; its pending-content
                    // check is the single authority for starting provider work.
                    self.memory_next_poll_unix_ms = 0;
                }
                true
            }
        }
    }

    fn apply_session_filters(&mut self) {
        let filter = self.snapshot.sessions.provider_filter;
        let query = self.snapshot.sessions.query.trim().to_lowercase();
        self.snapshot.sessions.rows = self
            .session_catalog_rows
            .iter()
            .filter(|row| {
                let provider_matches = match filter {
                    SessionsProviderFilter::All => true,
                    SessionsProviderFilter::Codex => row.provider == "codex",
                    SessionsProviderFilter::Claude => row.provider == "claude",
                };
                provider_matches
                    && (query.is_empty()
                        || row
                            .first_human_request
                            .as_ref()
                            .is_some_and(|value| value.to_lowercase().contains(&query))
                        || row
                            .title
                            .as_ref()
                            .is_some_and(|value| value.to_lowercase().contains(&query))
                        || row.checkout_path.to_lowercase().contains(&query))
            })
            .cloned()
            .collect();
        self.snapshot.sessions.memories = self
            .memory_catalog_rows
            .iter()
            .filter(|row| {
                (self.snapshot.sessions.this_turn_memory_ids.is_empty()
                    || self
                        .snapshot
                        .sessions
                        .this_turn_memory_ids
                        .contains(&row.id))
                    && (query.is_empty() || row.body.to_lowercase().contains(&query))
            })
            .cloned()
            .collect();
    }

    pub(super) fn open_memory_for_turn(&mut self, mut item_ids: Vec<String>) -> bool {
        item_ids.sort();
        item_ids.dedup();
        item_ids.truncate(hide_memory::PROMPT_ITEM_LIMIT);
        self.snapshot.ui_state.right_panel_visible = true;
        self.snapshot.ui_state.right_panel_section = RightPanelSection::Sessions;
        self.snapshot.sessions.mode = SessionsMode::Memory;
        self.snapshot.sessions.query.clear();
        self.snapshot.sessions.this_turn_memory_ids = item_ids;
        if let Some(project_id) = self.snapshot.sessions.project_id.clone() {
            self.snapshot
                .ui_state
                .sessions_mode_by_project
                .insert(project_id, SessionsMode::Memory);
        }
        self.apply_session_filters();
        self.persist_ui_state();
        true
    }

    pub(super) fn set_sessions_mode(&mut self, value: &str) -> bool {
        let mode = match value {
            "sessions" => SessionsMode::Sessions,
            "memory" => SessionsMode::Memory,
            _ => {
                self.set_error(
                    "sessions.mode_invalid",
                    "Sessions mode must be sessions or memory",
                    false,
                );
                return true;
            }
        };
        if self.snapshot.sessions.mode == mode {
            return false;
        }
        self.snapshot.sessions.mode = mode;
        self.snapshot.sessions.this_turn_memory_ids.clear();
        if let Some(project_id) = self.snapshot.sessions.project_id.clone() {
            self.snapshot
                .ui_state
                .sessions_mode_by_project
                .insert(project_id, mode);
            self.persist_ui_state();
        }
        true
    }

    pub(super) fn set_sessions_filter(&mut self, provider: &str, query: String) -> bool {
        let filter = match provider {
            "all" => SessionsProviderFilter::All,
            "codex" => SessionsProviderFilter::Codex,
            "claude" => SessionsProviderFilter::Claude,
            _ => {
                self.set_error(
                    "sessions.filter_invalid",
                    "Session filter must be all, codex, or claude",
                    false,
                );
                return true;
            }
        };
        if self.snapshot.sessions.provider_filter == filter && self.snapshot.sessions.query == query
        {
            return false;
        }
        self.snapshot.sessions.provider_filter = filter;
        self.snapshot.sessions.query = query;
        self.apply_session_filters();
        true
    }

    pub(super) fn open_archive_detail(&mut self, kind: &str, id: &str, preview: bool) -> bool {
        let Some((workspace_id, checkout_id, _)) = self.focused_memory_context() else {
            self.set_error(
                "archive.project_unavailable",
                "Choose a local Project first",
                false,
            );
            return true;
        };
        let Some(context) = self.worker_context.clone() else {
            self.set_error(
                "archive.worker_unavailable",
                "The archive reader is unavailable",
                true,
            );
            return true;
        };
        let id = id.to_owned();
        let kind = kind.to_owned();
        self.archive_detail_load_generation = self.archive_detail_load_generation.saturating_add(1);
        let generation = self.archive_detail_load_generation;
        let row = self
            .session_catalog_rows
            .iter()
            .find(|row| row.id == id)
            .cloned();
        let available_sources = self
            .session_catalog_rows
            .iter()
            .filter(|row| row.unavailable_reason.is_none())
            .map(|row| (row.provider.clone(), row.id.clone()))
            .collect::<HashSet<_>>();
        let database = self.memory_database_path();
        let project_id = self.snapshot.sessions.project_id.clone();
        thread::Builder::new()
            .name("hide-archive-detail".to_owned())
            .spawn(move || {
                let result = match kind.as_str() {
                    "session" => project_id
                        .ok_or_else(|| "Project Memory is unavailable".to_owned())
                        .and_then(|project_id| {
                            row.ok_or_else(|| "Session is no longer in this Project".to_owned())
                                .and_then(|row| load_session_detail(&database, &project_id, row))
                        }),
                    "memory" => project_id
                        .ok_or_else(|| "Project Memory is unavailable".to_owned())
                        .and_then(|project_id| {
                            load_memory_detail(&database, &project_id, &id, &available_sources)
                        }),
                    _ => Err("Archive detail kind is unsupported".to_owned()),
                };
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard)
                        if archive_load_matches_scope(
                            generation,
                            guard.archive_detail_load_generation,
                            &workspace_id,
                            &checkout_id,
                            guard.focused_memory_context().as_ref().map(
                                |(workspace, checkout, _)| (workspace.as_str(), checkout.as_str()),
                            ),
                        ) =>
                    {
                        match result {
                            Ok(detail) => {
                                guard.show_archive_tab(
                                    &workspace_id,
                                    &checkout_id,
                                    detail,
                                    preview,
                                );
                                true
                            }
                            Err(message) => {
                                guard.set_error("archive.open_failed", message, true);
                                true
                            }
                        }
                    }
                    Ok(_) => false,
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            })
            .map(|_| true)
            .unwrap_or_else(|error| {
                self.set_error(
                    "archive.worker_failed",
                    format!("Archive reader could not start: {error}"),
                    true,
                );
                true
            })
    }

    fn refresh_active_memory_detail(&mut self) {
        let item_id = self
            .snapshot
            .editor
            .archive_detail
            .as_ref()
            .filter(|detail| detail.kind == "memory")
            .map(|detail| detail.id.clone());
        if let Some(item_id) = item_id {
            self.open_archive_detail("memory", &item_id, false);
        }
    }

    pub(super) fn hooks_support_memory(&self) -> bool {
        self.hook_diagnosis.as_ref().is_some_and(|diagnosis| {
            diagnosis.runtimes.iter().any(|row| {
                row.memory_compatibility.supports_injection()
                    && matches!(row.status, HookStatus::Installed { version } if version >= hide_agent_hooks::HOOK_VERSION)
            })
        })
    }

    fn has_memory_compatible_runtime(&self) -> bool {
        self.hook_diagnosis.as_ref().is_some_and(|diagnosis| {
            diagnosis.runtimes.iter().any(|row| {
                !matches!(row.status, HookStatus::RuntimeAbsent)
                    && row.memory_compatibility.supports_injection()
            })
        })
    }

    pub(super) fn apply_memory_action(&mut self, payload: events::MemoryActionPayload) -> bool {
        if payload.action == "update_hooks" {
            let runtimes = self
                .hook_diagnosis
                .as_ref()
                .map(|diagnosis| {
                    diagnosis
                        .runtimes
                        .iter()
                        .filter(|row| {
                            row.memory_compatibility.supports_injection() && row.offers_install()
                        })
                        .map(|row| row.runtime)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if runtimes.is_empty() {
                self.set_error(
                    "memory.hook_update_unavailable",
                    "No supported local agent hook update is available",
                    false,
                );
                return true;
            }
            for runtime in runtimes {
                self.pending_hook_installs.insert(runtime);
            }
            self.memory_enable_after_hook_update = true;
            self.snapshot.sessions.analysis = MemoryAnalysisSnapshot {
                state: "hooks_need_update".to_owned(),
                message: Some("Updating agent hooks".to_owned()),
                action: Some("retry".to_owned()),
                ..MemoryAnalysisSnapshot::default()
            };
            return true;
        }
        if payload.action == "enable" && !self.hooks_support_memory() {
            let compatible_runtime = self.has_memory_compatible_runtime();
            self.snapshot.sessions.analysis = MemoryAnalysisSnapshot {
                state: "hooks_need_update".to_owned(),
                message: Some(if compatible_runtime {
                    "Hooks need update before Memory can turn on".to_owned()
                } else {
                    "Update Claude Code or Codex before Memory can turn on".to_owned()
                }),
                action: compatible_runtime.then(|| "update_hooks".to_owned()),
                ..MemoryAnalysisSnapshot::default()
            };
            return true;
        }
        let Some((_, _, checkout_path)) = self.focused_memory_context() else {
            self.set_error(
                "memory.project_unavailable",
                "Choose a local Project first",
                false,
            );
            return true;
        };
        if self.worker_context.is_none() {
            self.set_error(
                "memory.worker_unavailable",
                "The Project Memory writer is unavailable",
                true,
            );
            return true;
        }
        let interrupts_analysis = matches!(payload.action.as_str(), "disable" | "delete");
        if interrupts_analysis {
            self.memory_enable_after_hook_update = false;
            if let Some(cancel) = &self.memory_cancel {
                cancel.cancel();
            }
            if self.memory_operation_in_flight {
                self.queue_memory_interrupt(checkout_path, payload);
                return true;
            }
        } else if self.memory_operation_in_flight {
            return false;
        }
        self.begin_memory_action(checkout_path, payload)
    }

    fn queue_memory_interrupt(
        &mut self,
        checkout_path: String,
        payload: events::MemoryActionPayload,
    ) {
        let keep_existing_delete = self
            .memory_pending_action
            .as_ref()
            .is_some_and(|(_, pending)| pending.action == "delete" && payload.action != "delete");
        if !keep_existing_delete {
            self.memory_pending_action = Some((checkout_path, payload));
        }
        let action = self
            .memory_pending_action
            .as_ref()
            .map(|(_, pending)| pending.action.as_str())
            .unwrap_or("disable");
        self.snapshot.sessions.analysis = MemoryAnalysisSnapshot {
            state: "analyzing".to_owned(),
            message: Some(if action == "delete" {
                "Finishing the current write before deleting Memory".to_owned()
            } else {
                "Finishing the current write before turning Memory off".to_owned()
            }),
            ..MemoryAnalysisSnapshot::default()
        };
    }

    fn begin_memory_action(
        &mut self,
        checkout_path: String,
        payload: events::MemoryActionPayload,
    ) -> bool {
        let Some(context) = self.worker_context.clone() else {
            self.set_error(
                "memory.worker_unavailable",
                "The Project Memory writer is unavailable",
                true,
            );
            return true;
        };
        self.memory_operation_in_flight = true;
        self.memory_operation_generation = self.memory_operation_generation.saturating_add(1);
        let generation = self.memory_operation_generation;
        self.memory_operation_checkout_path = Some(checkout_path.clone());
        let action = payload.action.clone();
        let analyzes = matches!(action.as_str(), "enable" | "retry");
        let cancel = CancelToken::new();
        self.memory_cancel = analyzes.then(|| cancel.clone());
        if analyzes {
            self.snapshot.sessions.analysis = MemoryAnalysisSnapshot {
                state: "analyzing".to_owned(),
                discovered: self.snapshot.sessions.total_session_count,
                message: Some("Preparing session analysis".to_owned()),
                ..MemoryAnalysisSnapshot::default()
            };
        }
        let database = self.memory_database_path();
        let home = self.home_path.clone();
        let settings = self.ai_settings.clone().unwrap_or_default();
        thread::Builder::new()
            .name("hide-project-memory-write".to_owned())
            .spawn(move || {
                let mut result = mutate_memory(&database, &checkout_path, payload);
                if result.is_ok() && analyzes {
                    result = match home {
                        Some(home) => Ok(analyze_project(
                            &database,
                            &home,
                            &checkout_path,
                            &settings,
                            &cancel,
                            &context,
                            generation,
                        )),
                        None => Err("The home directory is unavailable".to_owned()),
                    };
                }
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => {
                        if guard.memory_operation_generation != generation {
                            return;
                        }
                        guard.memory_operation_in_flight = false;
                        guard.memory_cancel = None;
                        guard.memory_operation_checkout_path = None;
                        let pending = guard.memory_pending_action.take();
                        let still_focused = guard
                            .focused_memory_context()
                            .is_some_and(|(_, _, path)| path == checkout_path);
                        if still_focused && pending.is_none() {
                            match result {
                                Ok(outcome) => {
                                    if let Some((kind, message)) = outcome.diagnostic {
                                        guard.push_diagnostic(kind, message);
                                    }
                                    guard.snapshot.sessions.notice = outcome.notice;
                                    let current =
                                        std::mem::take(&mut guard.snapshot.sessions.analysis);
                                    guard.snapshot.sessions.analysis = settled_analysis_snapshot(
                                        analyzes,
                                        current,
                                        outcome.analysis,
                                    );
                                    guard.request_sessions_refresh();
                                    if action == "delete" {
                                        guard.close_memory_archive_tabs();
                                    } else if matches!(
                                        action.as_str(),
                                        "edit" | "forget" | "undo" | "resolve_conflict"
                                    ) {
                                        guard.refresh_active_memory_detail();
                                    }
                                }
                                Err(message) => {
                                    guard.set_error("memory.action_failed", message, true)
                                }
                            }
                        } else if pending.is_none() {
                            guard.request_sessions_refresh();
                        }
                        if let Some((pending_checkout, pending_payload)) = pending {
                            let pending_still_focused = guard
                                .focused_memory_context()
                                .is_some_and(|(_, _, path)| path == pending_checkout);
                            if pending_still_focused {
                                guard.begin_memory_action(pending_checkout, pending_payload);
                            } else {
                                guard.set_error(
                                    "memory.pending_project_changed",
                                    "The Project changed before the Memory action could run",
                                    false,
                                );
                            }
                        }
                        true
                    }
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            })
            .map(|_| true)
            .unwrap_or_else(|error| {
                self.memory_operation_in_flight = false;
                self.memory_operation_checkout_path = None;
                self.set_error(
                    "memory.worker_failed",
                    format!("The Project Memory writer could not start: {error}"),
                    true,
                );
                true
            })
    }
}

#[cfg(test)]
mod scope_tests {
    use super::{
        archive_load_matches_scope, load_session_detail, project_session_row,
        sessions_load_matches_scope, settled_analysis_snapshot,
        should_request_memory_due_poll_after_load, trusted_memory_receipt, update_hook_projection,
        validate_candidates,
    };
    use crate::model::MemoryAnalysisSnapshot;
    use hide_agent_hooks::{
        memory::{HookMemoryOutcome, HookMemoryResult, database_path, project_memory_output_until},
        runtime::{AgentRuntime, HookEvent},
    };
    use hide_memory::{AnalysisBatch, Candidate, CandidateKind, CandidateRelation, MemoryStore};
    use hide_session::{
        Agent, ProjectSession, SessionAvailability, parse_claude_events, parse_codex_events,
    };
    use serde_json::json;
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    const FUNCTIONAL_HOOK_TEST_TIMEOUT: Duration = Duration::from_secs(5);

    fn functional_hook_output(
        runtime: AgentRuntime,
        event: HookEvent,
        payload: &[u8],
        home: &Path,
    ) -> HookMemoryResult {
        project_memory_output_until(
            runtime,
            event,
            payload,
            false,
            home,
            Instant::now() + FUNCTIONAL_HOOK_TEST_TIMEOUT,
        )
    }

    #[test]
    fn session_detail_opens_before_memory_database_exists() {
        let temp = tempdir().unwrap();
        let session_path = temp.path().join("session.jsonl");
        fs::write(
            &session_path,
            concat!(
                "{\"type\":\"user\",\"timestamp\":\"2026-09-18T00:00:00Z\",",
                "\"origin\":{\"kind\":\"human\"},",
                "\"message\":{\"role\":\"user\",\"content\":\"open me\"}}\n"
            ),
        )
        .unwrap();
        let row = project_session_row(ProjectSession {
            id: "session-1".to_owned(),
            agent: Agent::Claude,
            locator: session_path,
            checkout_path: temp.path().to_path_buf(),
            first_human_request: Some("open me".to_owned()),
            started_at_unix_ms: Some(1),
            updated_at_unix_ms: 1,
            title: None,
            event_count: 1,
            availability: SessionAvailability::Available,
        });

        let detail =
            load_session_detail(&temp.path().join("missing.sqlite3"), "project-1", row).unwrap();

        assert_eq!(detail.events.len(), 1);
        assert_eq!(detail.events[0].text, "open me");
        assert_eq!(detail.events[0].memory_attached_count, None);
    }

    #[test]
    fn reversed_session_load_completion_cannot_replace_the_newer_generation() {
        assert!(!sessions_load_matches_scope(
            7,
            8,
            "/project/main",
            Some("/project/main"),
        ));
        assert!(sessions_load_matches_scope(
            8,
            8,
            "/project/main",
            Some("/project/main"),
        ));
    }

    #[test]
    fn a_session_load_for_the_previous_project_cannot_land_after_focus_moves() {
        assert!(!sessions_load_matches_scope(
            3,
            3,
            "/project/alpha",
            Some("/project/beta"),
        ));
    }

    #[test]
    fn only_the_latest_archive_request_for_the_focused_checkout_can_land() {
        assert!(!archive_load_matches_scope(
            4,
            5,
            "workspace",
            "checkout",
            Some(("workspace", "checkout")),
        ));
        assert!(!archive_load_matches_scope(
            5,
            5,
            "workspace",
            "checkout",
            Some(("workspace", "other")),
        ));
        assert!(archive_load_matches_scope(
            5,
            5,
            "workspace",
            "checkout",
            Some(("workspace", "checkout")),
        ));
    }

    #[test]
    fn successful_analysis_with_no_pending_content_clears_preparing_state() {
        let preparing = MemoryAnalysisSnapshot {
            state: "analyzing".to_owned(),
            discovered: 5,
            message: Some("Analyzing 0 of 5 sessions".to_owned()),
            ..MemoryAnalysisSnapshot::default()
        };

        let settled = settled_analysis_snapshot(true, preparing, None);

        assert!(settled.state.is_empty());
        assert!(settled.message.is_none());
    }

    #[test]
    fn non_analysis_mutations_do_not_hide_an_existing_actionable_failure() {
        let paused = MemoryAnalysisSnapshot {
            state: "paused".to_owned(),
            message: Some("Analysis paused".to_owned()),
            action: Some("retry".to_owned()),
            ..MemoryAnalysisSnapshot::default()
        };

        assert_eq!(
            settled_analysis_snapshot(false, paused.clone(), None),
            paused
        );
    }

    #[test]
    fn enabled_memory_load_requests_a_due_check_without_restarting_active_work() {
        assert!(should_request_memory_due_poll_after_load(
            true,
            &MemoryAnalysisSnapshot::default(),
        ));
        assert!(!should_request_memory_due_poll_after_load(
            true,
            &MemoryAnalysisSnapshot {
                state: "analyzing".to_owned(),
                ..MemoryAnalysisSnapshot::default()
            },
        ));
        assert!(!should_request_memory_due_poll_after_load(
            false,
            &MemoryAnalysisSnapshot::default(),
        ));
    }

    #[test]
    fn assistant_only_supersede_plans_require_human_conflict_resolution() {
        let candidates = validate_candidates(
            vec![Candidate {
                text: "Use the new boundary".to_owned(),
                kind: CandidateKind::Decision,
                confidence: 0.9,
                salience: 0.8,
                source_offsets: vec![7],
                direct_human_source: false,
                relation: CandidateRelation::Supersedes {
                    target_id: "existing".to_owned(),
                },
            }],
            &[json!({"offset": 7, "kind": "assistant"})],
        )
        .unwrap();

        assert_eq!(
            candidates[0].relation,
            CandidateRelation::Conflicts {
                target_id: "existing".to_owned()
            }
        );
        assert!(!candidates[0].direct_human_source);
    }

    #[test]
    fn direct_human_supersede_plans_remain_supersedes() {
        let candidates = validate_candidates(
            vec![Candidate {
                text: "새 경계로 교체한다.".to_owned(),
                kind: CandidateKind::Decision,
                confidence: 0.9,
                salience: 0.8,
                source_offsets: vec![7],
                direct_human_source: false,
                relation: CandidateRelation::Supersedes {
                    target_id: "existing".to_owned(),
                },
            }],
            &[json!({"offset": 7, "kind": "human"})],
        )
        .unwrap();

        assert_eq!(
            candidates[0].relation,
            CandidateRelation::Supersedes {
                target_id: "existing".to_owned()
            }
        );
        assert!(candidates[0].direct_human_source);
    }

    #[test]
    fn only_provider_injected_events_can_assert_memory_receipts() {
        let temp = tempdir().unwrap();
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).unwrap();
        let project = hide_project::resolve(&project_root, "local").unwrap();
        let database = temp.path().join("memory.sqlite3");
        let store = MemoryStore::open(&database).unwrap();
        store
            .ensure_project(&project.id, &project.root, "local")
            .unwrap();
        let item_key = "m1@2";
        let auth = store
            .receipt_auth_tag(
                &project.id,
                "codex",
                "session-1",
                HookEvent::UserPromptSubmit.name(),
                item_key,
            )
            .unwrap();
        let marker = format!(
            "<hide-memory-receipt event=\"UserPromptSubmit\" count=\"1\" items=\"{item_key}\" auth=\"{auth}\" />"
        );
        assert!(
            trusted_memory_receipt(&store, &project.id, "codex", "session-1", false, &marker,)
                .is_none()
        );
        assert!(
            trusted_memory_receipt(
                &store,
                &project.id,
                "codex",
                "session-1",
                true,
                &marker.replace(&auth, &"0".repeat(64)),
            )
            .is_none()
        );
        let receipt =
            trusted_memory_receipt(&store, &project.id, "codex", "session-1", true, &marker)
                .unwrap();
        assert_eq!(receipt.count, 1);
        assert_eq!(receipt.items, vec![("m1".to_owned(), 2)]);

        let user_prefix_record = json!({
            "type": "response_item",
            "timestamp": "2026-09-21T00:00:00Z",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": format!("# AGENTS.md instructions\n{marker}")}],
            },
        })
        .to_string();
        let event = parse_codex_events(&user_prefix_record)
            .events
            .into_iter()
            .next()
            .unwrap();
        assert!(!event.is_provider_injected());
        assert!(
            trusted_memory_receipt(
                &store,
                &project.id,
                "codex",
                "session-1",
                event.is_provider_injected(),
                &event.text,
            )
            .is_none()
        );

        let claude_auth = store
            .receipt_auth_tag(
                &project.id,
                "claude",
                "session-1",
                HookEvent::UserPromptSubmit.name(),
                item_key,
            )
            .unwrap();
        let claude_marker = format!(
            "<hide-memory-receipt event=\"UserPromptSubmit\" count=\"1\" items=\"{item_key}\" auth=\"{claude_auth}\" />"
        );
        let claude_user_record = json!({
            "type": "user",
            "timestamp": "2026-09-21T00:00:00Z",
            "userType": "external",
            "entrypoint": "claude-desktop",
            "promptId": "prompt-1",
            "message": {"role": "user", "content": claude_marker},
        })
        .to_string();
        let event = parse_claude_events(&claude_user_record)
            .events
            .into_iter()
            .next()
            .unwrap();
        assert!(!event.is_provider_injected());
        assert!(
            trusted_memory_receipt(
                &store,
                &project.id,
                "claude",
                "session-1",
                event.is_provider_injected(),
                &event.text,
            )
            .is_none()
        );
    }

    #[test]
    fn empty_session_start_projection_unblocks_later_prompt_memory_for_both_providers() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("home");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project_root).unwrap();
        let project = hide_project::resolve(&project_root, "local").unwrap();
        let database = database_path(&home);
        fs::create_dir_all(database.parent().unwrap()).unwrap();
        let store = MemoryStore::open(&database).unwrap();
        store
            .ensure_project(&project.id, &project.root, "local")
            .unwrap();
        store.set_enabled(&project.id, true, true).unwrap();
        drop(store);

        for (runtime, agent, provider, session_id) in [
            (
                AgentRuntime::ClaudeCode,
                Agent::Claude,
                "claude",
                "claude-empty",
            ),
            (AgentRuntime::Codex, Agent::Codex, "codex", "codex-empty"),
        ] {
            let start_payload = serde_json::to_vec(&json!({
                "cwd": project_root,
                "session_id": session_id,
            }))
            .unwrap();
            let start =
                functional_hook_output(runtime, HookEvent::SessionStart, &start_payload, &home);
            assert_eq!(start.outcome, HookMemoryOutcome::Empty);
            let envelope: serde_json::Value = serde_json::from_str(&start.stdout.unwrap()).unwrap();
            let context = envelope["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap();
            assert!(context.contains("count=\"0\""));
            assert!(context.contains("auth=\""));
            assert!(!context.contains("Project Memory ready 0"));

            let locator = temp.path().join(format!("{provider}-empty.jsonl"));
            let transcript = match agent {
                Agent::Claude => json!({
                    "type": "user",
                    "timestamp": "2026-09-21T00:00:00Z",
                    "origin": {"kind": "hook"},
                    "isMeta": true,
                    "message": {"role": "user", "content": context},
                }),
                Agent::Codex => json!({
                    "type": "response_item",
                    "timestamp": "2026-09-21T00:00:00Z",
                    "payload": {
                        "type": "message",
                        "role": "developer",
                        "content": [{"type": "input_text", "text": context}],
                    },
                }),
            };
            fs::write(&locator, format!("{transcript}\n")).unwrap();
            let session = ProjectSession {
                id: session_id.to_owned(),
                agent,
                locator,
                checkout_path: project_root.clone(),
                first_human_request: None,
                started_at_unix_ms: Some(1),
                updated_at_unix_ms: 1,
                title: None,
                event_count: 1,
                availability: SessionAvailability::Available,
            };
            let mut store = MemoryStore::open(&database).unwrap();
            update_hook_projection(&mut store, &project.id, &session).unwrap();
            assert_eq!(
                store
                    .session_start_receipt_ids(&project.id, provider, session_id)
                    .unwrap(),
                Some(Vec::new())
            );
        }
        let mut store = MemoryStore::open(&database).unwrap();
        store
            .apply_candidates(
                &AnalysisBatch {
                    id: "empty-receipt-batch".to_owned(),
                    project_id: project.id.clone(),
                    provider: "claude".to_owned(),
                    analysis_provider: "claude".to_owned(),
                    session_id: "source-session".to_owned(),
                    content_hash: "empty-receipt-hash".to_owned(),
                    created_at_unix_ms: 2,
                },
                &[Candidate {
                    text: "빈 시작 영수증 뒤에도 durable hook memory를 제공한다.".to_owned(),
                    kind: CandidateKind::Rule,
                    confidence: 0.9,
                    salience: 0.9,
                    source_offsets: vec![1],
                    direct_human_source: true,
                    relation: CandidateRelation::New,
                }],
            )
            .unwrap();
        drop(store);

        for (runtime, session_id) in [
            (AgentRuntime::ClaudeCode, "claude-empty"),
            (AgentRuntime::Codex, "codex-empty"),
        ] {
            let prompt_payload = serde_json::to_vec(&json!({
                "cwd": project_root,
                "session_id": session_id,
                "prompt": "durable hook memory",
            }))
            .unwrap();
            let prompt = functional_hook_output(
                runtime,
                HookEvent::UserPromptSubmit,
                &prompt_payload,
                &home,
            );
            assert_eq!(prompt.outcome, HookMemoryOutcome::Provided { count: 1 });
            assert!(
                prompt
                    .stdout
                    .unwrap()
                    .contains("빈 시작 영수증 뒤에도 durable hook memory를 제공한다.")
            );
        }
    }
}

struct MemoryMutationOutcome {
    notice: Option<MemoryNoticeSnapshot>,
    analysis: Option<MemoryAnalysisSnapshot>,
    diagnostic: Option<(String, String)>,
}

impl MemoryMutationOutcome {
    fn notice(notice: Option<MemoryNoticeSnapshot>) -> Self {
        Self {
            notice,
            analysis: None,
            diagnostic: None,
        }
    }
}

fn settled_analysis_snapshot(
    analyzed_project: bool,
    current: MemoryAnalysisSnapshot,
    outcome: Option<MemoryAnalysisSnapshot>,
) -> MemoryAnalysisSnapshot {
    match (analyzed_project, outcome) {
        (_, Some(snapshot)) => snapshot,
        (true, None) => MemoryAnalysisSnapshot::default(),
        (false, None) => current,
    }
}

fn should_request_memory_due_poll_after_load(
    memory_enabled: bool,
    analysis: &MemoryAnalysisSnapshot,
) -> bool {
    memory_enabled && analysis.state.is_empty()
}

fn report_analysis(
    context: &RuntimeWorkerContext,
    generation: u64,
    checkout_path: &str,
    analysis: MemoryAnalysisSnapshot,
) {
    let Some(runtime) = context.runtime.upgrade() else {
        return;
    };
    let changed = match runtime.lock() {
        Ok(mut guard)
            if guard.memory_operation_generation == generation
                && guard
                    .focused_memory_context()
                    .is_some_and(|(_, _, path)| path == checkout_path) =>
        {
            guard.snapshot.sessions.analysis = analysis;
            true
        }
        _ => false,
    };
    drop(runtime);
    if changed {
        context.notifier.notify();
    }
}

fn analyze_project(
    database: &Path,
    home: &Path,
    checkout_path: &str,
    settings: &hide_ai::AiSettings,
    cancel: &CancelToken,
    context: &RuntimeWorkerContext,
    generation: u64,
) -> MemoryMutationOutcome {
    let result = analyze_project_inner(
        database,
        home,
        checkout_path,
        settings,
        cancel,
        |snapshot| {
            report_analysis(context, generation, checkout_path, snapshot);
        },
    );
    match result {
        Ok(result) => result,
        Err(error) => MemoryMutationOutcome {
            notice: None,
            analysis: Some(MemoryAnalysisSnapshot {
                state: "paused".to_owned(),
                message: Some(error_message(&error).to_owned()),
                action: Some(error_action(&error).to_owned()),
                ..MemoryAnalysisSnapshot::default()
            }),
            diagnostic: None,
        },
    }
}

fn analyze_project_inner(
    database: &Path,
    home: &Path,
    checkout_path: &str,
    settings: &hide_ai::AiSettings,
    cancel: &CancelToken,
    mut progress: impl FnMut(MemoryAnalysisSnapshot),
) -> Result<MemoryMutationOutcome, AnalysisFailure> {
    let identity = hide_project::resolve(Path::new(checkout_path), workspace::LOCAL_DEVICE_ID)
        .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
    let sessions = SessionCatalog::new(home, workspace::LOCAL_DEVICE_ID)
        .project_sessions(&identity)
        .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
    let discovered = sessions.len();
    let mut store =
        MemoryStore::open(database).map_err(|error| AnalysisFailure::Local(error.to_string()))?;
    store
        .ensure_project(&identity.id, &identity.root, workspace::LOCAL_DEVICE_ID)
        .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
    let router = crate::ai::memory_router(settings);
    let analyzer = HideNativeAnalyzer;
    let mut analyzed = 0;
    let mut failed = 0;
    let mut learned = 0;
    let mut undo_batches = Vec::new();
    progress(MemoryAnalysisSnapshot {
        state: "analyzing".to_owned(),
        discovered,
        analyzed,
        failed,
        message: Some(format!("Analyzing {analyzed} of {discovered} sessions")),
        action: None,
    });

    for session in sessions {
        if cancel.is_cancelled() {
            return Err(AnalysisFailure::Cancelled);
        }
        let availability = match &session.availability {
            SessionAvailability::Available => None,
            SessionAvailability::Unavailable { reason } => Some(reason.clone()),
        };
        store
            .upsert_session_source(&SessionSourceRecord {
                id: session.id.clone(),
                project_id: identity.id.clone(),
                provider: session.agent.as_str().to_owned(),
                locator: session.locator.to_string_lossy().into_owned(),
                checkout_path: session.checkout_path.to_string_lossy().into_owned(),
                started_at_unix_ms: session.started_at_unix_ms,
                updated_at_unix_ms: session.updated_at_unix_ms,
                unavailable_reason: availability.clone(),
            })
            .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
        if availability.is_none() {
            update_hook_projection(&mut store, &identity.id, &session)
                .map_err(AnalysisFailure::Local)?;
        }
        if availability.is_some() {
            failed += 1;
            progress(analysis_progress(discovered, analyzed, failed));
            continue;
        }
        if !session_is_quiescent(&session, now_ms()) {
            continue;
        }
        if !session_has_pending_analysis(&store, &identity.id, &session)
            .map_err(AnalysisFailure::Local)?
        {
            continue;
        }

        match analyze_session(
            &mut store,
            &router,
            &analyzer,
            &identity.id,
            &session,
            cancel,
        ) {
            Ok(result) => {
                analyzed += 1;
                learned += result.learned;
                undo_batches.extend(result.batch_ids);
            }
            Err(AnalysisFailure::Provider(error)) => {
                failed += 1;
                progress(MemoryAnalysisSnapshot {
                    state: "paused".to_owned(),
                    discovered,
                    analyzed,
                    failed,
                    message: Some(format!("{analyzed} analyzed · {failed} failed")),
                    action: Some(
                        error_action(&AnalysisFailure::Provider(error.clone())).to_owned(),
                    ),
                });
                return Err(AnalysisFailure::Provider(error));
            }
            Err(AnalysisFailure::Cancelled) => return Err(AnalysisFailure::Cancelled),
            Err(AnalysisFailure::Local(_)) => failed += 1,
        }
        progress(analysis_progress(discovered, analyzed, failed));
    }

    Ok(MemoryMutationOutcome {
        notice: (learned > 0).then(|| MemoryNoticeSnapshot {
            message: format!(
                "Learned {learned} {} · Undo",
                if learned == 1 { "memory" } else { "memories" }
            ),
            undo_batch_id: (!undo_batches.is_empty()).then(|| undo_batches.join(",")),
        }),
        analysis: (failed > 0).then(|| MemoryAnalysisSnapshot {
            state: "complete".to_owned(),
            discovered,
            analyzed,
            failed,
            message: Some(format!("{analyzed} analyzed · {failed} failed")),
            action: Some("retry".to_owned()),
        }),
        diagnostic: None,
    })
}

#[derive(Clone, Debug)]
enum AnalysisFailure {
    Provider(AiError),
    Local(String),
    Cancelled,
}

fn error_action(error: &AnalysisFailure) -> &'static str {
    match error {
        AnalysisFailure::Provider(AiError::NotAuthenticated | AiError::NoProvider(_)) => "sign_in",
        AnalysisFailure::Provider(AiError::UsageLimited { .. } | AiError::OverBudget { .. }) => {
            "open_settings"
        }
        AnalysisFailure::Provider(_) | AnalysisFailure::Local(_) | AnalysisFailure::Cancelled => {
            "retry"
        }
    }
}

fn error_message(error: &AnalysisFailure) -> &'static str {
    match error {
        AnalysisFailure::Local(reason) if !reason.is_empty() => "Memory unavailable",
        AnalysisFailure::Cancelled => "Analysis stopped",
        AnalysisFailure::Provider(_) | AnalysisFailure::Local(_) => "Analysis paused",
    }
}

fn analysis_progress(discovered: usize, analyzed: usize, failed: usize) -> MemoryAnalysisSnapshot {
    MemoryAnalysisSnapshot {
        state: "analyzing".to_owned(),
        discovered,
        analyzed,
        failed,
        message: Some(format!(
            "Analyzing {} of {discovered} sessions",
            analyzed + failed
        )),
        action: None,
    }
}

fn session_is_quiescent(session: &hide_session::ProjectSession, now_unix_ms: u64) -> bool {
    fs::metadata(&session.locator)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| {
            now_unix_ms.saturating_sub(duration.as_millis() as u64) >= MEMORY_QUIESCENCE_MS
        })
        .unwrap_or(false)
}

fn memory_analysis_due(
    database: &Path,
    home: &Path,
    checkout_path: &str,
    now_unix_ms: u64,
) -> Result<bool, String> {
    if !database.is_file() {
        return Ok(false);
    }
    let identity = hide_project::resolve(Path::new(checkout_path), workspace::LOCAL_DEVICE_ID)
        .map_err(|error| error.to_string())?;
    let store = MemoryStore::open_read_only(database).map_err(|error| error.to_string())?;
    let state = match store.project_state(&identity.id) {
        Ok(state) => state,
        Err(hide_memory::MemoryError::ProjectMissing) => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    if !state.enabled {
        return Ok(false);
    }
    let sessions = SessionCatalog::new(home, workspace::LOCAL_DEVICE_ID)
        .project_sessions(&identity)
        .map_err(|error| error.to_string())?;
    for session in sessions {
        if !matches!(&session.availability, SessionAvailability::Available) {
            continue;
        }
        if !session_is_quiescent(&session, now_unix_ms) {
            continue;
        }
        if session_has_pending_analysis(&store, &identity.id, &session)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn update_hook_projection(
    store: &mut MemoryStore,
    project_id: &str,
    session: &hide_session::ProjectSession,
) -> Result<(), String> {
    let provider = session.agent.as_str();
    let saved = store
        .load_hook_projection_cursor(project_id, provider, &session.id)
        .map_err(|error| error.to_string())?;
    let mut cursor = match saved.as_ref() {
        Some(record) if !record.checkpoint.is_empty() => {
            SessionCursor::restore_checkpoint(&record.checkpoint)
                .map_err(|error| error.to_string())?
        }
        _ => SessionCursor::new(),
    };
    let chunk = cursor
        .read(&session.locator)
        .map_err(|error| error.to_string())?;
    let checkpoint = cursor
        .encode_checkpoint()
        .map_err(|error| error.to_string())?;
    let parsed = hide_session::parse_events_at(session.agent, &chunk.contents, chunk.start_offset);
    for (event, stable_offset) in parsed.events.into_iter().zip(parsed.event_offsets) {
        if let Some(receipt) = trusted_memory_receipt(
            store,
            project_id,
            provider,
            &session.id,
            event.is_provider_injected(),
            &event.text,
        ) {
            let turn_id = (receipt.event != HookEvent::SessionStart.name())
                .then(|| format!("event:{stable_offset}"));
            store
                .record_injection(
                    project_id,
                    provider,
                    &session.id,
                    turn_id.as_deref(),
                    &Injection {
                        outcome: if receipt.count == 0 {
                            InjectionOutcome::Empty
                        } else {
                            InjectionOutcome::Provided
                        },
                        items: receipt
                            .items
                            .into_iter()
                            .map(|(id, revision)| (id, revision, String::new()))
                            .collect(),
                        token_count: 0,
                    },
                )
                .map_err(|error| error.to_string())?;
        }
        if event.kind == EventKind::Human {
            let topic = normalized_topic_terms(&event.text);
            store
                .record_session_topic(project_id, provider, &session.id, stable_offset, &topic)
                .map_err(|error| error.to_string())?;
        }
    }
    store
        .save_hook_projection_cursor(&SessionCursorRecord {
            project_id: project_id.to_owned(),
            provider: provider.to_owned(),
            session_id: session.id.clone(),
            byte_offset: cursor.offset(),
            checkpoint,
            last_content_hash: (!chunk.contents.is_empty()).then(|| digest(&[&chunk.contents])),
            updated_at_unix_ms: now_ms(),
        })
        .map_err(|error| error.to_string())
}

fn normalized_topic_terms(text: &str) -> String {
    hide_memory::redact(text)
        .text
        .to_lowercase()
        .split(|character: char| {
            !character.is_alphanumeric() && character != '_' && character != '-'
        })
        .filter(|term| term.chars().count() >= 2)
        .take(24)
        .map(|term| term.chars().take(64).collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

fn session_has_pending_analysis(
    store: &MemoryStore,
    project_id: &str,
    session: &hide_session::ProjectSession,
) -> Result<bool, String> {
    let metadata = fs::metadata(&session.locator).map_err(|error| error.to_string())?;
    let cursor = store
        .load_cursor(project_id, session.agent.as_str(), &session.id)
        .map_err(|error| error.to_string())?;
    Ok(match cursor {
        None => metadata.len() > 0,
        Some(cursor) => {
            let modified_ms = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as u64)
                .unwrap_or_default();
            metadata.len() != cursor.byte_offset || modified_ms > cursor.updated_at_unix_ms
        }
    })
}

struct SessionAnalysis {
    learned: usize,
    batch_ids: Vec<String>,
}

fn analyze_session(
    store: &mut MemoryStore,
    router: &hide_ai::AiRouter,
    analyzer: &HideNativeAnalyzer,
    project_id: &str,
    session: &hide_session::ProjectSession,
    cancel: &CancelToken,
) -> Result<SessionAnalysis, AnalysisFailure> {
    let provider = session.agent.as_str();
    let saved = store
        .load_cursor(project_id, provider, &session.id)
        .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
    let mut cursor = match saved.as_ref() {
        Some(record) if !record.checkpoint.is_empty() => {
            SessionCursor::restore_checkpoint(&record.checkpoint)
                .map_err(|error| AnalysisFailure::Local(error.to_string()))?
        }
        _ => SessionCursor::new(),
    };
    let chunk = cursor
        .read(&session.locator)
        .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
    let checkpoint = cursor
        .encode_checkpoint()
        .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
    if chunk.contents.is_empty() {
        store
            .save_cursor(&SessionCursorRecord {
                project_id: project_id.to_owned(),
                provider: provider.to_owned(),
                session_id: session.id.clone(),
                byte_offset: cursor.offset(),
                checkpoint,
                last_content_hash: saved.and_then(|record| record.last_content_hash),
                updated_at_unix_ms: now_ms(),
            })
            .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
        return Ok(SessionAnalysis {
            learned: 0,
            batch_ids: Vec::new(),
        });
    }
    let parsed = hide_session::parse_events_at(session.agent, &chunk.contents, chunk.start_offset);
    let mut events = Vec::with_capacity(parsed.events.len());
    for (event, stable_offset) in parsed.events.into_iter().zip(parsed.event_offsets) {
        if let Some(receipt) = trusted_memory_receipt(
            store,
            project_id,
            provider,
            &session.id,
            event.is_provider_injected(),
            &event.text,
        ) {
            let turn_id = (receipt.event != HookEvent::SessionStart.name())
                .then(|| format!("event:{stable_offset}"));
            let injection = Injection {
                outcome: if receipt.count == 0 {
                    InjectionOutcome::Empty
                } else {
                    InjectionOutcome::Provided
                },
                items: receipt
                    .items
                    .into_iter()
                    .map(|(id, revision)| (id, revision, String::new()))
                    .collect(),
                token_count: 0,
            };
            store
                .record_injection(
                    project_id,
                    provider,
                    &session.id,
                    turn_id.as_deref(),
                    &injection,
                )
                .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
        }
        if !matches!(event.kind, EventKind::Human | EventKind::Assistant) {
            continue;
        }
        events.push(json!({
        "offset": stable_offset,
        "role": event.role,
        "kind": event.kind.as_str(),
        "at_unix_ms": event.at_unix_ms,
        "text": event.text,
        }));
    }
    let groups = event_groups(&events).map_err(AnalysisFailure::Local)?;
    let mut learned = 0;
    let mut batch_ids = Vec::new();
    let mut last_hash = None;
    for group in groups {
        if cancel.is_cancelled() {
            return Err(AnalysisFailure::Cancelled);
        }
        let serialized = serde_json::to_string(&group)
            .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
        let content_hash = digest(&[project_id, provider, &session.id, &serialized]);
        last_hash = Some(content_hash.clone());
        let active = active_memories_json(store, project_id).map_err(AnalysisFailure::Local)?;
        let request_id = digest(&[
            "memory-request",
            project_id,
            provider,
            &session.id,
            &content_hash,
        ]);
        let request = analyzer
            .request(
                request_id.clone(),
                project_id,
                &session.id,
                &serialized,
                &active,
            )
            .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
        let answer = router
            .execute(&request, cancel)
            .map_err(AnalysisFailure::Provider)?;
        let candidates = validate_candidates(
            analyzer
                .parse(answer.value)
                .map_err(|error| AnalysisFailure::Local(error.to_string()))?,
            &group,
        )?;
        let batch_id = digest(&[
            "memory-batch",
            project_id,
            provider,
            &session.id,
            &content_hash,
        ]);
        let summary = store
            .apply_candidates(
                &AnalysisBatch {
                    id: batch_id.clone(),
                    project_id: project_id.to_owned(),
                    provider: provider.to_owned(),
                    analysis_provider: answer.provider.as_str().to_owned(),
                    session_id: session.id.clone(),
                    content_hash,
                    created_at_unix_ms: now_ms(),
                },
                &candidates,
            )
            .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
        if !summary.duplicate_batch {
            learned += summary.created + summary.superseded;
            if summary.created + summary.superseded + summary.conflicts > 0 {
                batch_ids.push(batch_id);
            }
        }
    }
    store
        .save_cursor(&SessionCursorRecord {
            project_id: project_id.to_owned(),
            provider: provider.to_owned(),
            session_id: session.id.clone(),
            byte_offset: cursor.offset(),
            checkpoint,
            last_content_hash: last_hash,
            updated_at_unix_ms: now_ms(),
        })
        .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
    Ok(SessionAnalysis { learned, batch_ids })
}

pub(super) fn event_groups(events: &[Value]) -> Result<Vec<Vec<Value>>, String> {
    const EVENT_BUDGET: usize = 36 * 1024;
    let mut groups = Vec::new();
    let mut current = Vec::new();
    for event in events {
        for event in bounded_event_chunks(event, EVENT_BUDGET)? {
            current.push(event);
            let measured = serde_json::to_vec(&current)
                .map_err(|error| error.to_string())?
                .len();
            if measured > EVENT_BUDGET {
                let event = current.pop().expect("the just-pushed event exists");
                if !current.is_empty() {
                    groups.push(std::mem::take(&mut current));
                }
                current.push(event);
            }
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    Ok(groups)
}

fn bounded_event_chunks(event: &Value, budget: usize) -> Result<Vec<Value>, String> {
    let serialized = serde_json::to_vec(event).map_err(|error| error.to_string())?;
    if serialized.len() <= budget {
        return Ok(vec![event.clone()]);
    }
    if serialized.len() > ANALYSIS_INPUT_LIMIT_BYTES {
        return Err(format!(
            "Memory event exceeds the {ANALYSIS_INPUT_LIMIT_BYTES}-byte analysis limit"
        ));
    }
    let object = event
        .as_object()
        .ok_or_else(|| "Memory event must be a JSON object".to_owned())?;
    let text = object
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| "Oversized Memory event has no text to split".to_owned())?;
    let mut base = object.clone();
    base.remove("text");
    let base_size = serde_json::to_vec(&base)
        .map_err(|error| error.to_string())?
        .len();
    let maximum_text_bytes = budget
        .checked_sub(base_size.saturating_add(256))
        .map(|remaining| remaining / 6)
        .filter(|remaining| *remaining > 0)
        .ok_or_else(|| "Memory event metadata exceeds the analysis request limit".to_owned())?;
    let pieces = split_utf8(text, maximum_text_bytes);
    let count = pieces.len();
    let mut chunks = Vec::with_capacity(count);
    for (index, piece) in pieces.into_iter().enumerate() {
        let mut chunk = base.clone();
        chunk.insert("text".to_owned(), Value::String(piece.to_owned()));
        chunk.insert("chunk_index".to_owned(), json!(index));
        chunk.insert("chunk_count".to_owned(), json!(count));
        let chunk = Value::Object(chunk);
        if serde_json::to_vec(&chunk)
            .map_err(|error| error.to_string())?
            .len()
            > budget
        {
            return Err("Memory event chunk exceeds the analysis request limit".to_owned());
        }
        chunks.push(chunk);
    }
    Ok(chunks)
}

fn split_utf8(text: &str, maximum_bytes: usize) -> Vec<&str> {
    if text.is_empty() {
        return vec![text];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + maximum_bytes).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            end = text[start..]
                .char_indices()
                .nth(1)
                .map(|(offset, _)| start + offset)
                .unwrap_or(text.len());
        }
        chunks.push(&text[start..end]);
        start = end;
    }
    chunks
}

pub(super) fn active_memories_json(
    store: &MemoryStore,
    project_id: &str,
) -> Result<String, String> {
    let memories = store
        .relation_context(project_id, 60, 5_000)
        .map_err(|error| error.to_string())?;
    let mut values = Vec::with_capacity(memories.len());
    for (id, body) in memories {
        let redacted = hide_memory::redact(&body);
        if redacted.contains_secret_candidate {
            return Err("Stored Memory failed the outbound secret check".to_owned());
        }
        values.push(json!({"id": id, "text": redacted.text}));
        if serde_json::to_vec(&values)
            .map_err(|error| error.to_string())?
            .len()
            > RELATION_CONTEXT_INPUT_LIMIT_BYTES
        {
            values.pop();
        }
    }
    serde_json::to_string(&values).map_err(|error| error.to_string())
}

fn validate_candidates(
    mut candidates: Vec<Candidate>,
    events: &[Value],
) -> Result<Vec<Candidate>, AnalysisFailure> {
    let offsets = events
        .iter()
        .filter_map(|event| event.get("offset")?.as_u64())
        .collect::<HashSet<_>>();
    let human = events
        .iter()
        .filter_map(|event| {
            (event.get("kind")?.as_str()? == "human")
                .then(|| event.get("offset")?.as_u64())
                .flatten()
        })
        .collect::<HashSet<_>>();
    for candidate in &mut candidates {
        if candidate
            .source_offsets
            .iter()
            .any(|offset| !offsets.contains(offset))
        {
            return Err(AnalysisFailure::Local(
                "Memory output referenced an event outside the request".to_owned(),
            ));
        }
        candidate.direct_human_source = candidate
            .source_offsets
            .iter()
            .any(|offset| human.contains(offset));
        if !candidate.direct_human_source
            && let CandidateRelation::Supersedes { target_id } = &candidate.relation
        {
            candidate.relation = CandidateRelation::Conflicts {
                target_id: target_id.clone(),
            };
        }
    }
    Ok(candidates)
}

fn digest(parts: &[&str]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    format!("{:x}", hash.finalize())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn load_sessions(
    home: &Path,
    database: &Path,
    checkout_path: &str,
) -> Result<SessionsLoad, String> {
    let identity = hide_project::resolve(Path::new(checkout_path), workspace::LOCAL_DEVICE_ID)
        .map_err(|error| error.to_string())?;
    let sessions = SessionCatalog::new(home, workspace::LOCAL_DEVICE_ID)
        .project_sessions(&identity)
        .map_err(|error| error.to_string())?;
    let mut rows = sessions
        .into_iter()
        .map(project_session_row)
        .collect::<Vec<_>>();
    let (state, memories) = if database.is_file() {
        let store = MemoryStore::open_read_only(database).map_err(|error| error.to_string())?;
        match store.project_state(&identity.id) {
            Ok(state) => {
                let current = rows
                    .iter()
                    .map(|row| (row.provider.clone(), row.id.clone()))
                    .collect::<HashSet<_>>();
                for source in store
                    .list_session_sources(&identity.id)
                    .map_err(|error| error.to_string())?
                {
                    if !current.contains(&(source.provider.clone(), source.id.clone())) {
                        rows.push(persisted_unavailable_session_row(source));
                    }
                }
                rows.sort_by(|left, right| {
                    right
                        .updated_at_unix_ms
                        .cmp(&left.updated_at_unix_ms)
                        .then_with(|| left.id.cmp(&right.id))
                });
                let memories = store
                    .list_memories(&identity.id, "")
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .map(memory_row)
                    .collect();
                (Some(state), memories)
            }
            Err(hide_memory::MemoryError::ProjectMissing) => (None, Vec::new()),
            Err(error) => return Err(error.to_string()),
        }
    } else {
        (None, Vec::new())
    };
    Ok(SessionsLoad {
        project_id: identity.id,
        checkout_path: checkout_path.to_owned(),
        rows,
        memories,
        state,
    })
}

fn persisted_unavailable_session_row(source: SessionSourceRecord) -> SessionRowSnapshot {
    SessionRowSnapshot {
        id: source.id,
        provider_label: if source.provider == "codex" {
            "Codex"
        } else {
            "Claude Code"
        }
        .to_owned(),
        provider: source.provider,
        locator: source.locator,
        checkout_path: source.checkout_path,
        first_human_request: None,
        started_at_unix_ms: source.started_at_unix_ms,
        updated_at_unix_ms: source.updated_at_unix_ms,
        title: None,
        unavailable_reason: Some(
            source
                .unavailable_reason
                .unwrap_or_else(|| "Session source is no longer available".to_owned()),
        ),
    }
}

fn project_session_row(session: hide_session::ProjectSession) -> SessionRowSnapshot {
    let unavailable_reason = match session.availability {
        SessionAvailability::Available => None,
        SessionAvailability::Unavailable { reason } => Some(reason),
    };
    SessionRowSnapshot {
        id: session.id,
        provider: session.agent.as_str().to_owned(),
        provider_label: match session.agent {
            Agent::Codex => "Codex",
            Agent::Claude => "Claude Code",
        }
        .to_owned(),
        locator: session.locator.to_string_lossy().into_owned(),
        checkout_path: session.checkout_path.to_string_lossy().into_owned(),
        first_human_request: session.first_human_request,
        started_at_unix_ms: session.started_at_unix_ms,
        updated_at_unix_ms: session.updated_at_unix_ms,
        title: session.title,
        unavailable_reason,
    }
}

fn memory_row(item: hide_memory::MemoryItem) -> MemoryRowSnapshot {
    MemoryRowSnapshot {
        id: item.id,
        body: item.body,
        lifecycle: lifecycle_name(item.lifecycle),
        revision: item.revision,
        source_count: item.source_count,
        provided_session_count: item.provided_session_count,
        updated_at_unix_ms: item.updated_at_unix_ms,
    }
}

fn lifecycle_name(value: hide_memory::MemoryLifecycle) -> String {
    format!("{value:?}").to_ascii_lowercase()
}

fn load_session_detail(
    database: &Path,
    project_id: &str,
    row: SessionRowSnapshot,
) -> Result<ArchiveDetailSnapshot, String> {
    if let Some(reason) = row.unavailable_reason.clone() {
        return Ok(ArchiveDetailSnapshot {
            id: row.id,
            kind: "session".to_owned(),
            title: row
                .title
                .or(row.first_human_request)
                .unwrap_or_else(|| "Session unavailable".to_owned()),
            provider: Some(row.provider_label),
            unavailable_reason: Some(reason),
            events: Vec::new(),
            memory: None,
        });
    }
    let contents = read_bounded(Path::new(&row.locator), SESSION_READ_LIMIT_BYTES)
        .map_err(|error| format!("Session unavailable: {error}"))?;
    let store = database
        .is_file()
        .then(|| MemoryStore::open_read_only(database).map_err(|error| error.to_string()))
        .transpose()?;
    let agent = if row.provider == "codex" {
        Agent::Codex
    } else {
        Agent::Claude
    };
    let parsed = hide_session::parse_events(agent, &contents);
    let events = parsed
        .events
        .into_iter()
        .map(|event| {
            let receipt = store.as_ref().and_then(|store| {
                trusted_memory_receipt(
                    store,
                    project_id,
                    &row.provider,
                    &row.id,
                    event.is_provider_injected(),
                    &event.text,
                )
            });
            let attached = receipt.as_ref().map(|receipt| receipt.count);
            let item_ids = receipt
                .map(|receipt| receipt.items.into_iter().map(|(id, _)| id).collect())
                .unwrap_or_default();
            ArchiveEventSnapshot {
                role: event.role.to_owned(),
                kind: event.kind.as_str().to_owned(),
                at_unix_ms: event.at_unix_ms,
                text: if attached.is_some() {
                    strip_memory_receipt(&event.text)
                } else {
                    event.text
                },
                memory_attached_count: attached,
                memory_attached_item_ids: item_ids,
            }
        })
        .collect();
    Ok(ArchiveDetailSnapshot {
        id: row.id,
        kind: "session".to_owned(),
        title: row
            .title
            .or(row.first_human_request)
            .unwrap_or_else(|| "Session".to_owned()),
        provider: Some(row.provider_label),
        unavailable_reason: None,
        events,
        memory: None,
    })
}

struct MemoryReceipt {
    event: String,
    count: usize,
    item_key: String,
    items: Vec<(String, u64)>,
    auth: String,
}

fn trusted_memory_receipt(
    store: &MemoryStore,
    project_id: &str,
    provider: &str,
    session_id: &str,
    provider_injected: bool,
    text: &str,
) -> Option<MemoryReceipt> {
    if !provider_injected {
        return None;
    }
    let receipt = memory_receipt(text)?;
    store
        .verify_receipt_auth(
            project_id,
            provider,
            session_id,
            &receipt.event,
            &receipt.item_key,
            &receipt.auth,
        )
        .ok()
        .filter(|valid| *valid)
        .map(|_| receipt)
}

fn memory_receipt(text: &str) -> Option<MemoryReceipt> {
    let marker = text
        .lines()
        .find(|line| line.trim_start().starts_with("<hide-memory-receipt "))?;
    let count_start = marker.find("count=\"")? + "count=\"".len();
    let count = marker[count_start..]
        .split_once('"')?
        .0
        .parse::<usize>()
        .ok()?;
    let items_start = marker.find("items=\"")? + "items=\"".len();
    let raw_items = marker[items_start..].split_once('"')?.0;
    let items = if raw_items.is_empty() {
        Vec::new()
    } else {
        raw_items
            .split(',')
            .map(|value| {
                let (id, revision) = value.rsplit_once('@')?;
                (!id.is_empty()).then_some((id.to_owned(), revision.parse::<u64>().ok()?))
            })
            .collect::<Option<Vec<_>>>()?
    };
    let event_start = marker.find("event=\"")? + "event=\"".len();
    let event = marker[event_start..].split_once('"')?.0.to_owned();
    let auth_start = marker.find("auth=\"")? + "auth=\"".len();
    let auth = marker[auth_start..].split_once('"')?.0.to_owned();
    (items.len() == count).then_some(MemoryReceipt {
        event,
        count,
        item_key: raw_items.to_owned(),
        items,
        auth,
    })
}

fn strip_memory_receipt(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_start().starts_with("<hide-memory-receipt "))
        .collect::<Vec<_>>()
        .join("\n")
}

fn load_memory_detail(
    database: &Path,
    project_id: &str,
    item_id: &str,
    available_sources: &HashSet<(String, String)>,
) -> Result<ArchiveDetailSnapshot, String> {
    let store = MemoryStore::open_read_only(database).map_err(|error| error.to_string())?;
    let detail = store
        .detail(project_id, item_id)
        .map_err(|error| error.to_string())?;
    let (conflict_existing_id, conflict_candidate_id) = detail
        .conflict_pair
        .clone()
        .map(|(existing, candidate)| (Some(existing), Some(candidate)))
        .unwrap_or((None, None));
    let title = detail.item.body.chars().take(48).collect();
    Ok(ArchiveDetailSnapshot {
        id: detail.item.id.clone(),
        kind: "memory".to_owned(),
        title,
        provider: None,
        unavailable_reason: None,
        events: Vec::new(),
        memory: Some(MemoryDetailSnapshot {
            id: detail.item.id,
            body: detail.item.body,
            lifecycle: lifecycle_name(detail.item.lifecycle),
            revision: detail.item.revision,
            source_count: detail.item.source_count,
            provided_session_count: detail.item.provided_session_count,
            learned_at_unix_ms: detail.item.learned_at_unix_ms,
            conflict_existing_id,
            conflict_candidate_id,
            sources: detail
                .sources
                .into_iter()
                .map(|source| {
                    let available = source.available
                        && available_sources
                            .contains(&(source.provider.clone(), source.session_id.clone()));
                    MemorySourceSnapshot {
                        provider: source.provider,
                        session_id: source.session_id,
                        event_offset: source.event_offset,
                        available,
                    }
                })
                .collect(),
            revisions: detail
                .revisions
                .into_iter()
                .map(
                    |(revision, body, lifecycle, created_at_unix_ms)| MemoryRevisionSnapshot {
                        revision,
                        body,
                        lifecycle: lifecycle_name(lifecycle),
                        created_at_unix_ms,
                    },
                )
                .collect(),
        }),
    })
}

fn mutate_memory(
    database: &Path,
    checkout_path: &str,
    payload: events::MemoryActionPayload,
) -> Result<MemoryMutationOutcome, String> {
    let identity = hide_project::resolve(Path::new(checkout_path), workspace::LOCAL_DEVICE_ID)
        .map_err(|error| error.to_string())?;
    let mut store = MemoryStore::open(database).map_err(|error| error.to_string())?;
    store
        .ensure_project(&identity.id, &identity.root, workspace::LOCAL_DEVICE_ID)
        .map_err(|error| error.to_string())?;
    match payload.action.as_str() {
        "enable" => {
            store
                .set_enabled(&identity.id, true, true)
                .map_err(|error| error.to_string())?;
            Ok(MemoryMutationOutcome::notice(None))
        }
        "retry" => {
            let state = store
                .project_state(&identity.id)
                .map_err(|error| error.to_string())?;
            if !state.enabled {
                return Err("Project Memory is off".to_owned());
            }
            Ok(MemoryMutationOutcome::notice(None))
        }
        "disable" => {
            store
                .set_enabled(&identity.id, false, false)
                .map_err(|error| error.to_string())?;
            Ok(MemoryMutationOutcome::notice(None))
        }
        "delete" => {
            let deletion = store
                .delete_project_data(&identity.id)
                .map_err(|error| error.to_string())?;
            Ok(MemoryMutationOutcome {
                notice: None,
                analysis: None,
                diagnostic: deletion.cleanup_warning.map(|warning| {
                    (
                        "memory.delete_cleanup_deferred".to_owned(),
                        format!(
                            "Project Memory data was deleted; page cleanup was deferred: {warning}"
                        ),
                    )
                }),
            })
        }
        "edit" => {
            let id = payload
                .item_id
                .ok_or_else(|| "Memory edit has no item".to_owned())?;
            let body = payload
                .body
                .ok_or_else(|| "Memory edit has no body".to_owned())?;
            store
                .edit(&identity.id, &id, &body)
                .map_err(|error| error.to_string())?;
            Ok(MemoryMutationOutcome::notice(Some(MemoryNoticeSnapshot {
                message: "Memory updated".to_owned(),
                undo_batch_id: None,
            })))
        }
        "forget" => {
            let id = payload
                .item_id
                .ok_or_else(|| "Forget has no Memory item".to_owned())?;
            let batch = store
                .forget(&identity.id, &id)
                .map_err(|error| error.to_string())?;
            Ok(MemoryMutationOutcome::notice(Some(MemoryNoticeSnapshot {
                message: "Memory forgotten · Undo".to_owned(),
                undo_batch_id: Some(batch),
            })))
        }
        "undo" => {
            let batch = payload
                .batch_id
                .ok_or_else(|| "Undo has no batch".to_owned())?;
            let mut count = 0;
            for batch_id in batch.split(',').rev().filter(|value| !value.is_empty()) {
                count += store
                    .undo_batch(&identity.id, batch_id)
                    .map_err(|error| error.to_string())?;
            }
            Ok(MemoryMutationOutcome::notice(Some(MemoryNoticeSnapshot {
                message: format!("Restored {count} memory revisions"),
                undo_batch_id: None,
            })))
        }
        "resolve_conflict" => {
            let existing = payload
                .item_id
                .ok_or_else(|| "Conflict has no existing item".to_owned())?;
            let candidate = payload
                .candidate_id
                .ok_or_else(|| "Conflict has no candidate item".to_owned())?;
            let choice = match payload.conflict_choice.as_deref() {
                Some("keep_existing") => ConflictChoice::KeepExisting,
                Some("replace_with_new") => ConflictChoice::ReplaceWithNew,
                Some("forget_both") => ConflictChoice::ForgetBoth,
                _ => return Err("Conflict choice is invalid".to_owned()),
            };
            store
                .resolve_conflict(&identity.id, &existing, &candidate, choice)
                .map_err(|error| error.to_string())?;
            Ok(MemoryMutationOutcome::notice(None))
        }
        _ => Err(format!("Unknown Memory action: {}", payload.action)),
    }
}
