//! A Project's Memory as the web shell's Project Sessions screen shows it
//! (PRD project-memory-v2 D-17, B20-B24): the Sessions/Memory switch, the
//! cards of the Project the screen names, and the operator's actions on them.
//!
//! The screen names a Project with `sessions_refresh` (`project_sessions.rs`);
//! its Memory rides the same snapshot section and follows the same naming,
//! never the focused checkout. Every store read and write runs on a worker;
//! the lock only starts it and applies its answer.

use super::memory::{ProjectMemoryLoad, load_project_memory, project_identity};
use super::*;
use crate::model::{
    MemoryAnalysisSnapshot, MemoryNoticeSnapshot, ProjectMemorySnapshot, SessionsMode,
};
use hide_agent_hooks::HookStatus;
use hide_memory::{ConflictChoice, MemoryStore};
use serde_json::json;

/// The reads and the action behind the named Project's Memory.
#[derive(Default)]
pub(super) struct ProjectMemoryReads {
    generation: u64,
    in_flight: bool,
    waiting: bool,
    /// The Project an action runs for, with the action's name.
    pub(super) action: Option<(String, String)>,
    /// The named Project whose Memory turns on once the hook update the
    /// operator asked for lands.
    pub(super) enable_after_hook_update: Option<String>,
}

impl Runtime {
    /// Checks a `hide memory` caller like a Factory command does and takes
    /// what the command needs (B16, D-22): only a caller on this machine is
    /// answered, and an agent pane's own session is the one a `show` is
    /// recorded for.
    pub(crate) fn prepare_memory(
        &self,
        device: &str,
        caller: &str,
        expected: &crate::workspace_control::Context,
        command: crate::memory_command::MemoryCommand,
    ) -> Result<crate::memory_command::PreparedMemory, String> {
        if device != self.node.as_str() {
            return Err("memory_device_unsupported".into());
        }
        // The caller's checkout as it is now; Memory needs no window, so
        // the Views a Workspace query also checks do not matter here.
        let context = self
            .resolve_caller(device, crate::workspace_control::Caller::parse(caller))
            .map_err(|refusal| refusal.reason.to_owned())?;
        if context != *expected {
            return Err("caller_context_changed".into());
        }
        let reader = match crate::workspace_control::Caller::parse(caller) {
            crate::workspace_control::Caller::Pane(pane) => self
                .snapshot
                .navigator
                .agents
                .iter()
                .find(|agent| agent.pane_id == pane)
                .and_then(|agent| {
                    let provider = hide_session::Agent::from_kind(&agent.agent_kind)?;
                    Some((provider.as_str().to_owned(), agent.session_id.clone()?))
                }),
            crate::workspace_control::Caller::Checkout { .. } => None,
        };
        Ok(crate::memory_command::PreparedMemory {
            command,
            node: self.node.clone(),
            sessions_node: self.own_node(),
            database: self.memory_database_path(),
            checkout_path: context.checkout_path,
            reader,
            settings: self.ai_active().then(|| self.ai_settings.clone()).flatten(),
            context: self.worker_context.clone(),
        })
    }

    /// Memory's Project id for the Project the screen names, once its
    /// history read answered for it.
    pub(super) fn named_memory_project(&self) -> Option<&str> {
        self.snapshot.project_sessions.as_ref()?;
        self.project_sessions_work.project_id.as_deref()
    }

    /// Shows the named Project's Sessions or its Memory; the choice is kept
    /// per Project (D-17).
    pub(super) fn set_sessions_mode(&mut self, workspace_id: &str, value: &str) -> bool {
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
        let Some(sessions) = self
            .snapshot
            .project_sessions
            .as_mut()
            .filter(|sessions| sessions.workspace_id == workspace_id)
        else {
            return false;
        };
        if sessions.mode == mode {
            return false;
        }
        sessions.mode = mode;
        self.snapshot
            .ui_state
            .sessions_mode_by_project
            .insert(workspace_id.to_owned(), mode);
        self.persist_ui_state();
        true
    }

    /// The mode the screen last chose for `workspace_id`.
    pub(super) fn sessions_mode_for(&self, workspace_id: &str) -> SessionsMode {
        self.snapshot
            .ui_state
            .sessions_mode_by_project
            .get(workspace_id)
            .copied()
            .unwrap_or_default()
    }

    /// Lays a history read's Memory on the named Project's snapshot.
    pub(super) fn land_project_memory(&mut self, memory: Option<ProjectMemoryLoad>) {
        let Some(sessions) = self.snapshot.project_sessions.as_mut() else {
            return;
        };
        let current = sessions.memory.take().unwrap_or_default();
        sessions.memory = Some(match memory {
            Some(load) => ProjectMemorySnapshot {
                enabled: load.state.enabled,
                capacity_reached: load.state.capacity_reached,
                rows: if *current.rows == load.rows {
                    current.rows
                } else {
                    Arc::new(load.rows)
                },
                ..current
            },
            None => ProjectMemorySnapshot {
                enabled: false,
                capacity_reached: false,
                rows: Arc::new(Vec::new()),
                ..current
            },
        });
        self.sync_project_memory();
    }

    /// Copies the named Project's analysis state, notice and running action
    /// onto its snapshot.
    pub(super) fn sync_project_memory(&mut self) {
        let project_id = self.project_sessions_work.project_id.clone();
        let work = project_id
            .as_deref()
            .and_then(|id| self.memory_work.projects.get(id))
            .cloned()
            .unwrap_or_default();
        let action = self
            .project_memory_reads
            .action
            .as_ref()
            .filter(|(project, _)| Some(project) == project_id.as_ref())
            .map(|(_, action)| action.clone());
        let hooks = self.memory_hooks_analysis();
        let Some(memory) = self
            .snapshot
            .project_sessions
            .as_mut()
            .and_then(|sessions| sessions.memory.as_mut())
        else {
            return;
        };
        memory.analysis = match hooks {
            Some(analysis) if memory.enabled || work.analysis.state.is_empty() => analysis,
            _ => work.analysis,
        };
        memory.notice = work.notice;
        memory.pending_action = action;
    }

    /// What Memory needs before it can give anything to an agent: an agent
    /// whose installed hooks carry Memory.
    fn memory_hooks_analysis(&self) -> Option<MemoryAnalysisSnapshot> {
        if self.hooks_support_memory() {
            return None;
        }
        let diagnosis = self.hook_diagnosis.as_ref()?;
        let compatible = diagnosis.runtimes.iter().any(|row| {
            !matches!(row.status, HookStatus::RuntimeAbsent)
                && row.memory_compatibility.supports_injection()
        });
        Some(MemoryAnalysisSnapshot {
            state: "needs_action".to_owned(),
            message: Some(if compatible {
                "Agent hooks need an update before Memory can reach agents".to_owned()
            } else {
                "Install Claude Code or Codex for Memory to reach agents".to_owned()
            }),
            action: compatible.then(|| "update_hooks".to_owned()),
        })
    }

    pub(super) fn hooks_support_memory(&self) -> bool {
        self.hook_diagnosis.as_ref().is_some_and(|diagnosis| {
            diagnosis.runtimes.iter().any(|row| {
                row.memory_compatibility.supports_injection()
                    && matches!(row.status, HookStatus::Installed { version } if version >= hide_agent_hooks::HOOK_VERSION)
            })
        })
    }

    /// Reads the named Project's Memory again, after a change to it.
    pub(super) fn refresh_project_memory(&mut self) {
        let Some(project_id) = self.named_memory_project().map(str::to_owned) else {
            return;
        };
        self.project_memory_reads.generation += 1;
        if self.project_memory_reads.in_flight {
            self.project_memory_reads.waiting = true;
            return;
        }
        let Some(context) = self.worker_context.clone() else {
            return;
        };
        let generation = self.project_memory_reads.generation;
        let database = self.memory_database_path();
        self.project_memory_reads.in_flight = true;
        let spawn = thread::Builder::new()
            .name("hide-project-memory-read".to_owned())
            .spawn(move || {
                let result = if MemoryStore::exists(&database) {
                    MemoryStore::open_read_only(&database)
                        .map_err(|error| error.to_string())
                        .and_then(|store| load_project_memory(&store, &project_id))
                } else {
                    Ok(None)
                };
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_project_memory(generation, &project_id, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawn {
            self.project_memory_reads.in_flight = false;
            self.push_diagnostic(
                "memory.read_not_started",
                format!("Project Memory reader could not start: {error}"),
            );
        }
    }

    fn ingest_project_memory(
        &mut self,
        generation: u64,
        project_id: &str,
        result: Result<Option<ProjectMemoryLoad>, String>,
    ) -> bool {
        self.project_memory_reads.in_flight = false;
        if std::mem::take(&mut self.project_memory_reads.waiting)
            || generation != self.project_memory_reads.generation
        {
            self.refresh_project_memory();
            return false;
        }
        if self.named_memory_project() != Some(project_id) {
            return false;
        }
        match result {
            Ok(memory) => {
                self.land_project_memory(memory);
                true
            }
            Err(message) => {
                crate::diagnostic!(json!({
                    "component": "memory",
                    "kind": "memory.read_failed",
                    "project_id": project_id,
                    "message": message,
                }));
                false
            }
        }
    }

    /// One operator action on the named Project's Memory (B20, B22, B24).
    /// One action runs at a time; another is refused with the reason.
    pub(super) fn apply_memory_action(&mut self, payload: events::MemoryActionPayload) -> bool {
        let named = self
            .snapshot
            .project_sessions
            .as_ref()
            .is_some_and(|sessions| sessions.workspace_id == payload.workspace_id);
        if !named {
            // The screen moved to another Project before this arrived.
            return false;
        }
        let (Some(path), Some(project_id)) = (
            self.project_sessions_work.path.clone(),
            self.project_sessions_work.project_id.clone(),
        ) else {
            self.set_error(
                "memory.project_unavailable",
                "Memory is kept only for a Project on this Mac",
                false,
            );
            return true;
        };
        if payload.action == "update_hooks" {
            // Memory's "update hooks" is the kit's Reinstall of this Mac's
            // hook parts; the kit owns every hook write (PRD device-parity
            // D-27).
            let parts = self.local_hook_parts_to_repair();
            if parts.is_empty() {
                self.set_error(
                    "memory.hook_update_unavailable",
                    "No supported local agent hook update is available",
                    false,
                );
                return true;
            }
            let node = self.node.clone();
            self.queue_kit_reinstall(node.as_str(), parts);
            self.project_memory_reads.enable_after_hook_update = Some(payload.workspace_id);
            return true;
        }
        if payload.action == "enable" && !self.hooks_support_memory() {
            // Memory on without hooks that carry it would analyze for agents
            // that never receive a card; the screen shows the hook update.
            if self.hook_diagnosis.is_none() {
                self.set_error(
                    "memory.hooks_unread",
                    "Hide is still reading the agent hooks; try again in a moment",
                    false,
                );
            }
            self.sync_project_memory();
            return true;
        }
        if payload.action == "retry" {
            self.wake_project_memory(&project_id);
            self.sync_project_memory();
            return true;
        }
        if let Some((_, running)) = &self.project_memory_reads.action {
            self.set_error(
                "memory.action_busy",
                format!("Wait for the Memory change \"{running}\" to finish"),
                false,
            );
            return true;
        }
        let Some(context) = self.worker_context.clone() else {
            self.set_error(
                "memory.worker_unavailable",
                "The Project Memory writer is unavailable",
                true,
            );
            return true;
        };
        if matches!(payload.action.as_str(), "disable" | "delete") {
            self.quiet_project_memory(&project_id);
        }
        let action = payload.action.clone();
        self.project_memory_reads.action = Some((project_id.clone(), action.clone()));
        self.sync_project_memory();
        let database = self.memory_database_path();
        let node = self.node.clone();
        let sessions_node = self.own_node();
        let spawn = thread::Builder::new()
            .name("hide-project-memory-write".to_owned())
            .spawn(move || {
                let result =
                    mutate_memory(&node, sessions_node.as_ref(), &database, &path, payload);
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.finish_memory_action(&project_id, &action, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawn {
            self.project_memory_reads.action = None;
            self.set_error(
                "memory.worker_failed",
                format!("The Project Memory writer could not start: {error}"),
                true,
            );
        }
        true
    }

    fn finish_memory_action(
        &mut self,
        project_id: &str,
        action: &str,
        result: Result<MemoryActionOutcome, String>,
    ) -> bool {
        self.project_memory_reads.action = None;
        match result {
            Ok(outcome) => {
                if let Some((kind, message)) = outcome.diagnostic {
                    self.push_diagnostic(kind, message);
                }
                let project = self
                    .memory_work
                    .projects
                    .entry(project_id.to_owned())
                    .or_default();
                project.notice = outcome.notice;
                if action == "enable" {
                    self.wake_project_memory(project_id);
                } else if action == "delete" {
                    self.close_memory_archive_tabs();
                }
            }
            Err(message) => self.set_error("memory.action_failed", message, true),
        }
        self.sync_project_memory();
        self.refresh_project_memory();
        true
    }

    /// Turns Memory on for the Project whose hook update the operator asked
    /// for, once the hooks carry Memory.
    pub(super) fn enable_memory_after_hook_update(&mut self) {
        if !self.hooks_support_memory() {
            return;
        }
        let Some(workspace_id) = self.project_memory_reads.enable_after_hook_update.take() else {
            return;
        };
        self.apply_memory_action(events::MemoryActionPayload {
            workspace_id,
            action: "enable".to_owned(),
            item_id: None,
            candidate_id: None,
            batch_id: None,
            conflict_choice: None,
        });
    }
}

struct MemoryActionOutcome {
    notice: Option<MemoryNoticeSnapshot>,
    diagnostic: Option<(String, String)>,
}

impl MemoryActionOutcome {
    fn notice(message: impl Into<String>, undo_batch_id: Option<String>) -> Self {
        Self {
            notice: Some(MemoryNoticeSnapshot {
                message: message.into(),
                undo_batch_id,
            }),
            diagnostic: None,
        }
    }

    fn quiet() -> Self {
        Self {
            notice: None,
            diagnostic: None,
        }
    }
}

fn mutate_memory(
    node: &crate::node::NodeId,
    sessions_node: &dyn crate::node_access::NodeLink,
    database: &Path,
    checkout_path: &str,
    payload: events::MemoryActionPayload,
) -> Result<MemoryActionOutcome, String> {
    let identity = project_identity(sessions_node, node, checkout_path)?;
    let mut store = MemoryStore::open(database).map_err(|error| error.to_string())?;
    let required = |value: Option<String>, what: &str| {
        value.ok_or_else(|| format!("The Memory action names no {what}"))
    };
    match payload.action.as_str() {
        "enable" => {
            // Turning Memory on is accepting the disclosure the switch shows.
            store
                .ensure_project(&identity.id, &identity.root, node.as_str())
                .map_err(|error| error.to_string())?;
            store
                .set_enabled(&identity.id, true, true)
                .map_err(|error| error.to_string())?;
            Ok(MemoryActionOutcome::quiet())
        }
        "disable" => {
            store
                .set_enabled(&identity.id, false, false)
                .map_err(|error| error.to_string())?;
            Ok(MemoryActionOutcome::quiet())
        }
        "delete" => {
            let deletion = store
                .delete_project_data(&identity.id)
                .map_err(|error| error.to_string())?;
            Ok(MemoryActionOutcome {
                notice: None,
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
        "forget" => {
            let id = required(payload.item_id, "card")?;
            let batch = store
                .forget(&identity.id, &id)
                .map_err(|error| error.to_string())?;
            Ok(MemoryActionOutcome::notice("Forgotten", Some(batch)))
        }
        "undo" => {
            let batch = required(payload.batch_id, "change")?;
            let mut count = 0;
            for batch_id in batch.split(',').rev().filter(|value| !value.is_empty()) {
                count += store
                    .undo_batch(&identity.id, batch_id)
                    .map_err(|error| error.to_string())?;
            }
            Ok(MemoryActionOutcome::notice(
                format!(
                    "Restored {count} {}",
                    if count == 1 { "card" } else { "cards" }
                ),
                None,
            ))
        }
        "resolve_conflict" => {
            let existing = required(payload.item_id, "card")?;
            let candidate = required(payload.candidate_id, "new card")?;
            let choice = match payload.conflict_choice.as_deref() {
                Some("keep_existing") => ConflictChoice::KeepExisting,
                Some("replace_with_new") => ConflictChoice::ReplaceWithNew,
                Some("forget_both") => ConflictChoice::ForgetBoth,
                _ => return Err("The review choice is not one Memory knows".to_owned()),
            };
            let batch = store
                .resolve_conflict(&identity.id, &existing, &candidate, choice)
                .map_err(|error| error.to_string())?;
            Ok(MemoryActionOutcome::notice("Reviewed", Some(batch)))
        }
        _ => Err(format!("Unknown Memory action: {}", payload.action)),
    }
}
