use super::*;
use crate::model::{
    ArchiveEventSnapshot, MemoryAnalysisSnapshot, MemoryDetailSnapshot, MemoryNoticeSnapshot,
    MemoryRevisionSnapshot, MemoryRowSnapshot, MemorySourceSnapshot, SessionsMode,
    SessionsProviderFilter, SessionsSnapshot,
};
use hide_agent_hooks::HookStatus;
use hide_ai::{AiError, CancelToken};
use hide_memory::{
    AnalysisBatch, Candidate, ConflictChoice, Injection, InjectionOutcome, Mem0Adapter,
    MemoryLifecycle, MemoryStore, SessionCursorRecord, SessionSourceRecord,
};
use hide_session::{Agent, EventKind, SessionAvailability, SessionCatalog, SessionCursor};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;

const MEMORY_QUIESCENCE_MS: u64 = 60_000;
const MEMORY_POLL_INTERVAL_MS: u64 = 5_000;

struct SessionsLoad {
    project_id: String,
    checkout_path: String,
    rows: Vec<SessionRowSnapshot>,
    memories: Vec<MemoryRowSnapshot>,
    state: Option<hide_memory::ProjectMemoryState>,
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
                                let still_focused = guard
                                    .focused_memory_context()
                                    .is_some_and(|(_, _, path)| path == checkout_path);
                                if due && still_focused && !guard.memory_operation_in_flight {
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
        let Some(context) = self.worker_context.clone() else {
            self.snapshot.sessions.loading = false;
            self.snapshot.sessions.unavailable_reason =
                Some("Session reader is unavailable".to_owned());
            return true;
        };
        self.snapshot.sessions.loading = true;
        self.snapshot.sessions.unavailable_reason = None;
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
                    Ok(mut guard) => guard.ingest_sessions_load(result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            })
            .map(|_| true)
            .unwrap_or_else(|error| {
                self.snapshot.sessions.loading = false;
                self.snapshot.sessions.unavailable_reason =
                    Some(format!("Session reader could not start: {error}"));
                true
            })
    }

    fn ingest_sessions_load(&mut self, result: Result<SessionsLoad, String>) -> bool {
        self.snapshot.sessions.loading = false;
        match result {
            Err(message) => {
                self.snapshot.sessions.unavailable_reason = Some(message);
                true
            }
            Ok(load) => {
                let should_resume = load.state.as_ref().is_some_and(|state| state.enabled)
                    && self.snapshot.sessions.analysis.state.is_empty();
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
                if should_resume {
                    self.apply_memory_action(events::MemoryActionPayload {
                        action: "retry".to_owned(),
                        item_id: None,
                        candidate_id: None,
                        body: None,
                        batch_id: None,
                        conflict_choice: None,
                    });
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
                let result = if kind == "session" {
                    row.ok_or_else(|| "Session is no longer in this Project".to_owned())
                        .and_then(load_session_detail)
                } else if kind == "memory" {
                    let project_id =
                        project_id.ok_or_else(|| "Project Memory is unavailable".to_owned());
                    project_id.and_then(|project_id| {
                        load_memory_detail(&database, &project_id, &id, &available_sources)
                    })
                } else {
                    Err("Archive detail kind is unsupported".to_owned())
                };
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => match result {
                        Ok(detail) => {
                            guard.show_archive_tab(&workspace_id, &checkout_id, detail, preview);
                            true
                        }
                        Err(message) => {
                            guard.set_error("archive.open_failed", message, true);
                            true
                        }
                    },
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
            diagnosis.runtimes.iter().all(|row| match row.status {
                HookStatus::RuntimeAbsent => true,
                HookStatus::Installed { version } => version >= hide_agent_hooks::HOOK_VERSION,
                _ => false,
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
                        .filter(|row| row.offers_install())
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
            self.snapshot.sessions.analysis = MemoryAnalysisSnapshot {
                state: "hooks_need_update".to_owned(),
                message: Some("Hooks need update before Memory can turn on".to_owned()),
                action: Some("update_hooks".to_owned()),
                ..MemoryAnalysisSnapshot::default()
            };
            return true;
        }
        let interrupts_analysis = matches!(payload.action.as_str(), "disable" | "delete");
        if interrupts_analysis {
            self.memory_enable_after_hook_update = false;
            if let Some(cancel) = self.memory_cancel.take() {
                cancel.cancel();
            }
        } else if self.memory_operation_in_flight {
            return false;
        }
        let Some((_, _, checkout_path)) = self.focused_memory_context() else {
            self.set_error(
                "memory.project_unavailable",
                "Choose a local Project first",
                false,
            );
            return true;
        };
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
                        match result {
                            Ok(outcome) => {
                                guard.snapshot.sessions.notice = outcome.notice;
                                if let Some(analysis) = outcome.analysis {
                                    guard.snapshot.sessions.analysis = analysis;
                                }
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
                            Err(message) => guard.set_error("memory.action_failed", message, true),
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
                self.set_error(
                    "memory.worker_failed",
                    format!("The Project Memory writer could not start: {error}"),
                    true,
                );
                true
            })
    }
}

struct MemoryMutationOutcome {
    notice: Option<MemoryNoticeSnapshot>,
    analysis: Option<MemoryAnalysisSnapshot>,
}

impl MemoryMutationOutcome {
    fn notice(notice: Option<MemoryNoticeSnapshot>) -> Self {
        Self {
            notice,
            analysis: None,
        }
    }
}

fn report_analysis(
    context: &RuntimeWorkerContext,
    generation: u64,
    analysis: MemoryAnalysisSnapshot,
) {
    let Some(runtime) = context.runtime.upgrade() else {
        return;
    };
    let changed = match runtime.lock() {
        Ok(mut guard) if guard.memory_operation_generation == generation => {
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
            report_analysis(context, generation, snapshot);
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
    let adapter = Mem0Adapter;
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
                first_human_request: session.first_human_request.clone(),
                started_at_unix_ms: session.started_at_unix_ms,
                updated_at_unix_ms: session.updated_at_unix_ms,
                unavailable_reason: availability.clone(),
            })
            .map_err(|error| AnalysisFailure::Local(error.to_string()))?;
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
            &adapter,
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
        analysis: Some(MemoryAnalysisSnapshot {
            state: "complete".to_owned(),
            discovered,
            analyzed,
            failed,
            message: Some(if failed == 0 {
                format!("{analyzed} analyzed")
            } else {
                format!("{analyzed} analyzed · {failed} failed")
            }),
            action: (failed > 0).then(|| "retry".to_owned()),
        }),
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
    let store = MemoryStore::open_hook_read_only(database).map_err(|error| error.to_string())?;
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
        if !matches!(&session.availability, SessionAvailability::Available)
            || !session_is_quiescent(&session, now_unix_ms)
        {
            continue;
        }
        if session_has_pending_analysis(&store, &identity.id, &session)? {
            return Ok(true);
        }
    }
    Ok(false)
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
    adapter: &Mem0Adapter,
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
        if let Some((count, items)) = memory_receipt(&event.text) {
            let turn_id = format!("event:{stable_offset}");
            let injection = Injection {
                outcome: if count == 0 {
                    InjectionOutcome::Empty
                } else {
                    InjectionOutcome::Provided
                },
                items: items
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
                    Some(&turn_id),
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
        "text": strip_memory_receipt(&event.text),
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
        let request = adapter
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
            adapter
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
        current.push(event.clone());
        let measured = serde_json::to_vec(&current)
            .map_err(|error| error.to_string())?
            .len();
        if measured > EVENT_BUDGET {
            let event = current.pop().expect("the just-pushed event exists");
            if current.is_empty() {
                return Err(
                    "One normalized session event exceeds the analysis input cap".to_owned(),
                );
            }
            groups.push(std::mem::take(&mut current));
            current.push(event);
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    Ok(groups)
}

fn active_memories_json(store: &MemoryStore, project_id: &str) -> Result<String, String> {
    const COMPARISON_BUDGET: usize = 20 * 1024;
    let mut values = Vec::new();
    for item in store
        .list_memories(project_id, "")
        .map_err(|error| error.to_string())?
    {
        if item.lifecycle != MemoryLifecycle::Active {
            continue;
        }
        values.push(json!({"id": item.id, "text": item.body}));
        if serde_json::to_vec(&values)
            .map_err(|error| error.to_string())?
            .len()
            > COMPARISON_BUDGET
        {
            values.pop();
            break;
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
        first_human_request: source.first_human_request,
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

fn load_session_detail(row: SessionRowSnapshot) -> Result<ArchiveDetailSnapshot, String> {
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
    let contents = fs::read_to_string(&row.locator)
        .map_err(|error| format!("Session unavailable: {error}"))?;
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
            let receipt = memory_receipt(&event.text);
            let attached = receipt.as_ref().map(|(count, _)| *count);
            let item_ids = receipt
                .map(|(_, items)| items.into_iter().map(|(id, _)| id).collect())
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

fn memory_receipt(text: &str) -> Option<(usize, Vec<(String, u64)>)> {
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
    (items.len() == count).then_some((count, items))
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
            store
                .delete_project_data(&identity.id)
                .map_err(|error| error.to_string())?;
            Ok(MemoryMutationOutcome::notice(None))
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
