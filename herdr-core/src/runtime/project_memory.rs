//! Project Memory as a shell shows it (PRD project-memory-v2 B13,
//! B20-B24): the Project a shell opened Memory for with `memory_open`, the
//! operator's actions on a Project's Memory, one card in full, a search,
//! and the cards each live agent session on this Mac was given.
//!
//! Memory is its own snapshot section, keyed by the Project a shell opens;
//! nothing here follows the focused checkout or the Overview's Sessions.
//! Every store read and write runs on a worker; the lock only starts it and
//! applies its answer.

use super::memory::project_identity;
use super::*;
use crate::labels::worker::MemorySession;
use crate::model::{
    MemoryAction, MemoryAnalysisSnapshot, MemoryAnalysisState, MemoryCardKind, MemoryCardSnapshot,
    MemoryCardState, MemoryFix, MemoryNoticeSnapshot, MemoryReceivedCardSnapshot,
    MemoryReceivedSnapshot, MemoryReviewChoice, MemoryRowSnapshot, MemorySearchSnapshot,
    MemorySnapshot, MemorySourceSnapshot, ProjectMemorySnapshot,
};
use hide_agent_hooks::HookStatus;
use hide_memory::{ConflictChoice, MemoryLifecycle, MemoryStore};
use serde_json::json;
use std::collections::HashSet;

/// The most bytes of a screen search, as `hide memory search` takes.
const SEARCH_LIMIT_BYTES: usize = crate::memory_command::SEARCH_LIMIT_BYTES;

/// The reads and the action behind Project Memory on screen.
#[derive(Default)]
pub(super) struct ProjectMemoryWork {
    /// The opened Project's folder while it can be read here.
    path: Option<String>,
    /// Memory's id for the opened Project, once a read resolved it.
    pub(super) project_id: Option<String>,
    /// The card shown in full and the screen's search words.
    expanded: Option<u64>,
    search: Option<String>,
    reads: ReadFence,
    /// The catalog Project an action runs for, with the action.
    pub(super) action: Option<(String, MemoryAction)>,
    /// The Project whose Memory turns on once the hook update the operator
    /// asked for lands, by catalog Project.
    pub(super) enable_after_hook_update: Option<String>,
    received_reads: ReadFence,
    /// The cards each live session was given in full, as the last read found.
    pub(super) received: HashMap<MemorySession, Vec<MemoryReceivedCardSnapshot>>,
}

/// One read in flight and at most one waiting; a read whose generation is
/// behind never lands.
#[derive(Default)]
struct ReadFence {
    generation: u64,
    in_flight: bool,
    waiting: bool,
}

impl ReadFence {
    /// Starts a read now, or leaves it waiting behind the one in flight;
    /// answers the generation to start.
    fn begin(&mut self) -> Option<u64> {
        self.generation += 1;
        if self.in_flight {
            self.waiting = true;
            return None;
        }
        self.in_flight = true;
        Some(self.generation)
    }

    /// A read ended: whether its answer may land, and whether the waiting
    /// one has to start.
    fn end(&mut self, generation: u64) -> (bool, bool) {
        self.in_flight = false;
        let waiting = std::mem::take(&mut self.waiting);
        (!waiting && generation == self.generation, waiting)
    }
}

/// What one read of the opened Project found.
pub(super) struct OpenedMemoryLoad {
    project_id: String,
    /// `None` when the store has never held the Project.
    state: Option<hide_memory::ProjectMemoryState>,
    rows: Vec<MemoryRowSnapshot>,
    expanded: Option<MemoryCardSnapshot>,
    search: Option<MemorySearchSnapshot>,
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

    /// Memory's id for the Project a shell opened, once a read resolved it.
    pub(super) fn named_memory_project(&self) -> Option<&str> {
        self.snapshot
            .memory
            .as_ref()
            .and_then(|memory| memory.project.as_ref())?;
        self.project_memory.project_id.as_deref()
    }

    fn memory_section(&mut self) -> &mut MemorySnapshot {
        self.snapshot
            .memory
            .get_or_insert_with(MemorySnapshot::default)
    }

    /// Opens a catalog Project's Memory for a screen, or reads it again; no
    /// `workspace_id` closes it (`memory_open`). Opening another Project
    /// leaves nothing of the previous one, and its reads can no longer land.
    pub(super) fn open_project_memory(&mut self, workspace_id: Option<String>) -> bool {
        let Some(workspace_id) = workspace_id else {
            let open = self
                .snapshot
                .memory
                .as_ref()
                .is_some_and(|memory| memory.project.is_some());
            if open {
                self.project_memory = ProjectMemoryWork {
                    received_reads: std::mem::take(&mut self.project_memory.received_reads),
                    received: std::mem::take(&mut self.project_memory.received),
                    action: self.project_memory.action.take(),
                    enable_after_hook_update: self.project_memory.enable_after_hook_update.take(),
                    ..ProjectMemoryWork::default()
                };
                self.project_memory.reads.generation += 1;
                self.memory_section().project = None;
            }
            return open;
        };
        let same = self
            .snapshot
            .memory
            .as_ref()
            .and_then(|memory| memory.project.as_ref())
            .is_some_and(|project| project.workspace_id == workspace_id);
        if !same {
            self.project_memory.project_id = None;
            self.project_memory.expanded = None;
            self.project_memory.search = None;
            self.memory_section().project = Some(ProjectMemorySnapshot {
                workspace_id: workspace_id.clone(),
                loading: true,
                ..ProjectMemorySnapshot::default()
            });
        }
        match self.memory_folder(&workspace_id) {
            Err(reason) => {
                self.project_memory.path = None;
                self.project_memory.reads.generation += 1;
                if let Some(project) = self.memory_section().project.as_mut() {
                    *project = ProjectMemorySnapshot {
                        workspace_id,
                        unavailable_reason: Some(reason),
                        ..ProjectMemorySnapshot::default()
                    };
                }
            }
            Ok(path) => {
                self.project_memory.path = Some(path);
                self.refresh_project_memory();
            }
        }
        true
    }

    /// Where a catalog Project's Memory is read from, or why it is not kept
    /// here: Memory is kept for the Projects on this machine.
    fn memory_folder(&self, workspace_id: &str) -> Result<String, String> {
        let workspace = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .ok_or_else(|| "This Project is no longer registered.".to_owned())?;
        if workspace.remote_target_id.is_some() || workspace.device_id != self.node.as_str() {
            return Err("Project Memory is kept only for Projects on this Mac.".to_owned());
        }
        Ok(workspace.path.clone())
    }

    /// Shows one card of the opened Project in full, or none.
    pub(super) fn expand_memory_card(&mut self, workspace_id: &str, number: Option<u64>) -> bool {
        if !self.memory_opened(workspace_id) || self.project_memory.expanded == number {
            return false;
        }
        self.project_memory.expanded = number;
        if number.is_none()
            && let Some(project) = self.memory_section().project.as_mut()
        {
            project.expanded = None;
        }
        self.refresh_project_memory();
        true
    }

    /// Runs the matcher over the opened Project; empty words clear it.
    pub(super) fn search_project_memory(&mut self, workspace_id: &str, query: String) -> bool {
        if !self.memory_opened(workspace_id) {
            return false;
        }
        if query.len() > SEARCH_LIMIT_BYTES {
            self.set_error("memory.search_too_long", "Search with fewer words", false);
            return true;
        }
        let query = Some(query.trim().to_owned()).filter(|query| !query.is_empty());
        if self.project_memory.search == query {
            return false;
        }
        self.project_memory.search = query;
        if self.project_memory.search.is_none()
            && let Some(project) = self.memory_section().project.as_mut()
        {
            project.search = None;
        }
        self.refresh_project_memory();
        true
    }

    fn memory_opened(&self, workspace_id: &str) -> bool {
        self.snapshot
            .memory
            .as_ref()
            .and_then(|memory| memory.project.as_ref())
            .is_some_and(|project| project.workspace_id == workspace_id)
    }

    /// Reads the opened Project's Memory again, after a change to it or to
    /// what the screen asks of it.
    pub(super) fn refresh_project_memory(&mut self) {
        let Some(path) = self.project_memory.path.clone() else {
            return;
        };
        let Some(context) = self.worker_context.clone() else {
            return;
        };
        let Some(generation) = self.project_memory.reads.begin() else {
            return;
        };
        let database = self.memory_database_path();
        let node = self.node.clone();
        let sessions_node = self.own_node();
        let expanded = self.project_memory.expanded;
        let search = self.project_memory.search.clone();
        let spawn = thread::Builder::new()
            .name("hide-project-memory-read".to_owned())
            .spawn(move || {
                let result = load_opened_memory(
                    &node,
                    sessions_node.as_ref(),
                    &database,
                    &path,
                    expanded,
                    search,
                );
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_project_memory(generation, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawn {
            self.project_memory.reads.in_flight = false;
            self.push_diagnostic(
                "memory.read_not_started",
                format!("Project Memory reader could not start: {error}"),
            );
        }
    }

    fn ingest_project_memory(
        &mut self,
        generation: u64,
        result: Result<OpenedMemoryLoad, String>,
    ) -> bool {
        let (lands, waiting) = self.project_memory.reads.end(generation);
        if waiting {
            self.refresh_project_memory();
        }
        if !lands {
            return false;
        }
        let load = match result {
            Ok(load) => load,
            Err(message) => {
                crate::diagnostic!(json!({
                    "component": "memory",
                    "kind": "memory.read_failed",
                    "message": message,
                }));
                if let Some(project) = self.memory_section().project.as_mut() {
                    project.loading = false;
                    project.unavailable_reason =
                        Some("Project Memory could not be read.".to_owned());
                }
                return true;
            }
        };
        self.project_memory.project_id = Some(load.project_id);
        let Some(project) = self.memory_section().project.as_mut() else {
            return false;
        };
        project.loading = false;
        project.unavailable_reason = None;
        project.enabled = load.state.as_ref().is_some_and(|state| state.enabled);
        project.capacity_reached = load
            .state
            .as_ref()
            .is_some_and(|state| state.capacity_reached);
        if *project.rows != load.rows {
            project.rows = Arc::new(load.rows);
        }
        project.expanded = load.expanded;
        project.search = load.search;
        self.sync_project_memory();
        true
    }

    /// Copies the opened Project's analysis state, notice and running action
    /// onto its snapshot.
    pub(super) fn sync_project_memory(&mut self) {
        let work = self
            .project_memory
            .project_id
            .as_deref()
            .and_then(|id| self.memory_work.projects.get(id))
            .cloned()
            .unwrap_or_default();
        let action = self
            .project_memory
            .action
            .as_ref()
            .filter(|(workspace_id, _)| self.memory_opened(workspace_id))
            .map(|(_, action)| *action);
        let hooks = self.memory_hooks_analysis();
        let Some(project) = self
            .snapshot
            .memory
            .as_mut()
            .and_then(|memory| memory.project.as_mut())
        else {
            return;
        };
        project.analysis = shown_analysis(hooks, project.enabled, work.analysis);
        project.notice = work.notice;
        project.pending_action = action;
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
            state: MemoryAnalysisState::NeedsAction,
            message: Some(if compatible {
                "Agent hooks need an update before Memory can reach agents".to_owned()
            } else {
                "Install Claude Code or Codex for Memory to reach agents".to_owned()
            }),
            action: compatible.then_some(MemoryFix::UpdateHooks),
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

    /// One operator action on a catalog Project's Memory (B20, B22, B24),
    /// from the screen that opened it or from a pane's menu. One action runs
    /// at a time; another is refused with the reason.
    pub(super) fn apply_memory_action(&mut self, payload: events::MemoryActionPayload) -> bool {
        let path = match self.memory_folder(&payload.workspace_id) {
            Ok(path) => path,
            Err(reason) => {
                self.set_error("memory.project_unavailable", reason, false);
                return true;
            }
        };
        match payload.action {
            MemoryAction::UpdateHooks => {
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
                self.project_memory.enable_after_hook_update = Some(payload.workspace_id);
                return true;
            }
            MemoryAction::Enable if !self.hooks_support_memory() => {
                // Memory on without hooks that carry it would analyze for
                // agents that never receive a card; the screen shows the hook
                // update.
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
            MemoryAction::Retry => {
                if let Some(project_id) = self
                    .memory_opened(&payload.workspace_id)
                    .then(|| self.project_memory.project_id.clone())
                    .flatten()
                {
                    self.wake_project_memory(&project_id);
                    self.sync_project_memory();
                }
                return true;
            }
            _ => {}
        }
        if let Some((_, running)) = &self.project_memory.action {
            self.set_error(
                "memory.action_busy",
                format!(
                    "Wait for the Memory change \"{}\" to finish",
                    running.as_str()
                ),
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
        // The Project's id is only known once the worker read the folder; a
        // pass is stopped now and again when the switch is written.
        if matches!(payload.action, MemoryAction::Disable | MemoryAction::Delete) {
            self.cancel_memory_pass();
        }
        let action = payload.action;
        self.project_memory.action = Some((payload.workspace_id.clone(), action));
        self.sync_project_memory();
        let database = self.memory_database_path();
        let node = self.node.clone();
        let sessions_node = self.own_node();
        let spawn = thread::Builder::new()
            .name("hide-project-memory-write".to_owned())
            .spawn(move || {
                let result =
                    mutate_memory(&node, sessions_node.as_ref(), &database, &path, &payload);
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.finish_memory_action(action, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawn {
            self.project_memory.action = None;
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
        action: MemoryAction,
        result: Result<MemoryActionOutcome, String>,
    ) -> bool {
        self.project_memory.action = None;
        match result {
            Ok(outcome) => {
                if let Some((kind, message)) = outcome.diagnostic {
                    self.push_diagnostic(kind, message);
                }
                let project_id = outcome.project_id;
                match action {
                    MemoryAction::Enable => self.wake_project_memory(&project_id),
                    // A pass that started between the request and the store
                    // write still saw the Project on; the store refuses its
                    // cards, and this stops it asking the provider.
                    MemoryAction::Disable | MemoryAction::Delete => {
                        self.quiet_project_memory(&project_id)
                    }
                    _ => {}
                }
                self.memory_work
                    .projects
                    .entry(project_id)
                    .or_default()
                    .notice = outcome.notice;
            }
            Err(message) => self.set_error("memory.action_failed", message, true),
        }
        self.sync_project_memory();
        self.refresh_project_memory();
        self.refresh_memory_received();
        true
    }

    /// Turns Memory on for the Project whose hook update the operator asked
    /// for, once the hooks carry Memory.
    pub(super) fn enable_memory_after_hook_update(&mut self) {
        if !self.hooks_support_memory() {
            return;
        }
        let Some(workspace_id) = self.project_memory.enable_after_hook_update.take() else {
            return;
        };
        self.apply_memory_action(events::MemoryActionPayload {
            workspace_id,
            action: MemoryAction::Enable,
            number: None,
            other_number: None,
            batch_id: None,
            choice: None,
        });
    }

    /// Reads again what every live agent session on this Mac was given, for
    /// its pane's menu: after a receipt, a pane change or a Memory change.
    pub(super) fn refresh_memory_received(&mut self) {
        let Some(context) = self.worker_context.clone() else {
            return;
        };
        let database = self.memory_database_path();
        let Some(generation) = self.project_memory.received_reads.begin() else {
            return;
        };
        let sessions = self
            .live_memory_sessions()
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let spawn = thread::Builder::new()
            .name("hide-memory-received-read".to_owned())
            .spawn(move || {
                let result = load_received(&database, sessions);
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_memory_received(generation, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawn {
            self.project_memory.received_reads.in_flight = false;
            self.push_diagnostic(
                "memory.received_read_not_started",
                format!("Project Memory reader could not start: {error}"),
            );
        }
    }

    fn ingest_memory_received(
        &mut self,
        generation: u64,
        result: Result<HashMap<MemorySession, Vec<MemoryReceivedCardSnapshot>>, String>,
    ) -> bool {
        let (lands, waiting) = self.project_memory.received_reads.end(generation);
        if waiting {
            self.refresh_memory_received();
        }
        if !lands {
            return false;
        }
        match result {
            Ok(received) => self.project_memory.received = received,
            Err(message) => {
                crate::diagnostic!(json!({
                    "component": "memory",
                    "kind": "memory.received_read_failed",
                    "message": message,
                }));
                return false;
            }
        }
        self.publish_memory_received()
    }

    /// Joins the received cards with the panes that hold each session now.
    pub(super) fn publish_memory_received(&mut self) -> bool {
        let received = self
            .snapshot
            .navigator
            .agents
            .iter()
            .filter_map(|agent| {
                let runtime = hide_session::Agent::from_kind(&agent.agent_kind)?.as_str();
                let cards = self
                    .project_memory
                    .received
                    .get(&(runtime, agent.session_id.clone()?))?;
                Some(MemoryReceivedSnapshot {
                    pane_id: agent.pane_id.clone(),
                    cards: cards.clone(),
                })
            })
            .collect::<Vec<_>>();
        if self
            .snapshot
            .memory
            .as_ref()
            .map_or(received.is_empty(), |memory| *memory.received == received)
        {
            return false;
        }
        self.memory_section().received = Arc::new(received);
        true
    }
}

/// Which of the hooks' need and analysis's own state the operator acts on
/// (B23): missing hooks block everything while Memory is on; while it is
/// off they show only when analysis has nothing of its own to say.
fn shown_analysis(
    hooks: Option<MemoryAnalysisSnapshot>,
    enabled: bool,
    analysis: MemoryAnalysisSnapshot,
) -> MemoryAnalysisSnapshot {
    match hooks {
        Some(hooks) if enabled || analysis.state == MemoryAnalysisState::Idle => hooks,
        _ => analysis,
    }
}

struct MemoryActionOutcome {
    project_id: String,
    notice: Option<MemoryNoticeSnapshot>,
    diagnostic: Option<(String, String)>,
}

fn mutate_memory(
    node: &crate::node::NodeId,
    sessions_node: &dyn crate::node_access::NodeLink,
    database: &Path,
    checkout_path: &str,
    payload: &events::MemoryActionPayload,
) -> Result<MemoryActionOutcome, String> {
    let identity = project_identity(sessions_node, node, checkout_path)?;
    let mut store = MemoryStore::open(database).map_err(|error| error.to_string())?;
    let outcome = |notice: Option<(String, Option<String>)>| MemoryActionOutcome {
        project_id: identity.id.clone(),
        notice: notice.map(|(message, undo_batch_id)| MemoryNoticeSnapshot {
            message,
            undo_batch_id,
        }),
        diagnostic: None,
    };
    let card = |store: &MemoryStore, number: Option<u64>| {
        let number = number.ok_or("The Memory action names no card")?;
        store
            .item_by_number(&identity.id, number)
            .map_err(|error| error.to_string())?
            .filter(|item| item.lifecycle != MemoryLifecycle::Tombstoned)
            .ok_or_else(|| format!("m{number} is no longer in this Project's Memory"))
    };
    match payload.action {
        MemoryAction::Enable => {
            // Turning Memory on is accepting the disclosure the switch shows.
            store
                .ensure_project(&identity.id, &identity.root, node.as_str())
                .map_err(|error| error.to_string())?;
            store
                .set_enabled(&identity.id, true, true)
                .map_err(|error| error.to_string())?;
            Ok(outcome(None))
        }
        MemoryAction::Disable => {
            store
                .set_enabled(&identity.id, false, false)
                .map_err(|error| error.to_string())?;
            Ok(outcome(None))
        }
        MemoryAction::Delete => {
            let deletion = store
                .delete_project_data(&identity.id)
                .map_err(|error| error.to_string())?;
            Ok(MemoryActionOutcome {
                diagnostic: deletion.cleanup_warning.map(|warning| {
                    (
                        "memory.delete_cleanup_deferred".to_owned(),
                        format!(
                            "Project Memory data was deleted; page cleanup was deferred: {warning}"
                        ),
                    )
                }),
                ..outcome(None)
            })
        }
        MemoryAction::Forget => {
            let item = card(&store, payload.number)?;
            let batch = store
                .forget(&identity.id, &item.id)
                .map_err(|error| error.to_string())?;
            Ok(outcome(Some((
                format!("Forgot m{}", item.number),
                Some(batch),
            ))))
        }
        MemoryAction::Undo => {
            let batch = payload
                .batch_id
                .as_deref()
                .ok_or("The Memory action names no change")?;
            let mut count = 0;
            for batch_id in batch.split(',').rev().filter(|value| !value.is_empty()) {
                count += store
                    .undo_batch(&identity.id, batch_id)
                    .map_err(|error| error.to_string())?;
            }
            Ok(outcome(Some((
                format!(
                    "Restored {count} {}",
                    if count == 1 { "card" } else { "cards" }
                ),
                None,
            ))))
        }
        MemoryAction::ResolveConflict => {
            let existing = card(&store, payload.number)?;
            let candidate = card(&store, payload.other_number)?;
            let choice = match payload.choice.ok_or("The review names no choice")? {
                MemoryReviewChoice::KeepExisting => ConflictChoice::KeepExisting,
                MemoryReviewChoice::ReplaceWithNew => ConflictChoice::ReplaceWithNew,
                MemoryReviewChoice::ForgetBoth => ConflictChoice::ForgetBoth,
            };
            let batch = store
                .resolve_conflict(&identity.id, &existing.id, &candidate.id, choice)
                .map_err(|error| error.to_string())?;
            Ok(outcome(Some(("Reviewed".to_owned(), Some(batch)))))
        }
        MemoryAction::UpdateHooks | MemoryAction::Retry => {
            Err(format!("{} is not a store write", payload.action.as_str()))
        }
    }
}

/// The opened Project's switch, cards, the card shown in full and the
/// search's answer, read once.
fn load_opened_memory(
    node: &crate::node::NodeId,
    sessions_node: &dyn crate::node_access::NodeLink,
    database: &Path,
    checkout_path: &str,
    expanded: Option<u64>,
    search: Option<String>,
) -> Result<OpenedMemoryLoad, String> {
    let identity = project_identity(sessions_node, node, checkout_path)?;
    let empty = |project_id: String| OpenedMemoryLoad {
        project_id,
        state: None,
        rows: Vec::new(),
        expanded: None,
        search: None,
    };
    if !MemoryStore::exists(database) {
        return Ok(empty(identity.id));
    }
    let store = MemoryStore::open_read_only(database).map_err(|error| error.to_string())?;
    let state = match store.project_state(&identity.id) {
        Ok(state) => state,
        Err(hide_memory::MemoryError::ProjectMissing) => return Ok(empty(identity.id)),
        Err(error) => return Err(error.to_string()),
    };
    let items = store
        .list_memories(&identity.id)
        .map_err(|error| error.to_string())?;
    // A card in a needs-review pair carries its text, and so does the card
    // it contradicts, so the two read side by side (B20).
    let paired = items
        .iter()
        .filter_map(|item| item.conflict_number)
        .collect::<HashSet<_>>();
    let rows = items
        .into_iter()
        .map(|item| {
            let compared =
                paired.contains(&item.number) || item.lifecycle == MemoryLifecycle::Conflicting;
            memory_row(item, compared)
        })
        .collect();
    let expanded = match expanded {
        Some(number) => match store
            .item_by_number(&identity.id, number)
            .map_err(|error| error.to_string())?
            .filter(|item| item.lifecycle != MemoryLifecycle::Tombstoned)
        {
            Some(item) => {
                let detail = store
                    .detail(&identity.id, &item.id)
                    .map_err(|error| error.to_string())?;
                Some(MemoryCardSnapshot {
                    number,
                    body: item.body,
                    cited_paths: item.cited_paths,
                    sources: detail
                        .sources
                        .into_iter()
                        .map(|source| MemorySourceSnapshot {
                            provider: source.provider,
                            session_id: source.session_id,
                            event_offset: source.event_offset,
                            human: source.human,
                        })
                        .collect(),
                })
            }
            None => None,
        },
        None => None,
    };
    let search = match search {
        Some(query) => Some(MemorySearchSnapshot {
            numbers: store
                .search(&identity.id, &query, hide_memory::SEARCH_RESULT_LIMIT)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|item| item.number)
                .collect(),
            query,
        }),
        None => None,
    };
    Ok(OpenedMemoryLoad {
        project_id: identity.id,
        state: Some(state),
        rows,
        expanded,
        search,
    })
}

fn memory_row(item: hide_memory::MemoryItem, with_body: bool) -> MemoryRowSnapshot {
    MemoryRowSnapshot {
        number: item.number,
        title: item.title,
        kind: card_kind(item.kind),
        state: card_state(item.lifecycle),
        source_count: item.source_count,
        human_source_count: item.human_source_count,
        provided_session_count: item.provided_session_count,
        pull_count: item.pull_count,
        last_used_at_unix_ms: item.last_used_at_unix_ms,
        updated_at_unix_ms: item.updated_at_unix_ms,
        conflict_number: item.conflict_number,
        body: with_body.then_some(item.body),
    }
}

pub(crate) fn card_kind(kind: hide_memory::CandidateKind) -> MemoryCardKind {
    match kind {
        hide_memory::CandidateKind::Rule => MemoryCardKind::Rule,
        hide_memory::CandidateKind::Decision => MemoryCardKind::Decision,
        hide_memory::CandidateKind::Lesson => MemoryCardKind::Lesson,
        hide_memory::CandidateKind::Fact => MemoryCardKind::Fact,
    }
}

/// A live card's state; only active and needs-review cards are listed.
pub(crate) fn card_state(lifecycle: MemoryLifecycle) -> MemoryCardState {
    match lifecycle {
        MemoryLifecycle::Conflicting => MemoryCardState::NeedsReview,
        _ => MemoryCardState::Active,
    }
}

/// The live cards each session was given in full.
fn load_received(
    database: &Path,
    sessions: Vec<MemorySession>,
) -> Result<HashMap<MemorySession, Vec<MemoryReceivedCardSnapshot>>, String> {
    let mut received = HashMap::new();
    if sessions.is_empty() || !MemoryStore::exists(database) {
        return Ok(received);
    }
    let store = MemoryStore::open_read_only(database).map_err(|error| error.to_string())?;
    for session in sessions {
        let cards = store
            .received_cards(session.0, &session.1)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|(number, title)| MemoryReceivedCardSnapshot { number, title })
            .collect::<Vec<_>>();
        if !cards.is_empty() {
            received.insert(session, cards);
        }
    }
    Ok(received)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hide_memory::{AnalysisBatch, Candidate, CandidateKind, CandidateRelation};

    #[test]
    fn a_read_shows_the_expanded_card_with_its_sources_and_the_search_answer() {
        // P1-2: the row expanded in place and the screen's search.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let root = std::fs::canonicalize(root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let node: Arc<dyn crate::node_access::NodeLink> =
            Arc::new(hide_node::Local::new(Some(temp.path().join("home"))));
        let identity = project_identity(node.as_ref(), &crate::node::test_node(), &root).unwrap();
        let database = temp.path().join("memory.sqlite3");
        let mut store = MemoryStore::open(&database).unwrap();
        store
            .ensure_project(&identity.id, &identity.root, crate::node::TEST_NODE)
            .unwrap();
        store.set_enabled(&identity.id, true, true).unwrap();
        store
            .apply_candidates(
                &AnalysisBatch {
                    id: "b".to_owned(),
                    project_id: identity.id.clone(),
                    provider: "codex".to_owned(),
                    analysis_provider: "codex".to_owned(),
                    session_id: "s-1".to_owned(),
                    content_hash: "h".to_owned(),
                    created_at_unix_ms: 1,
                },
                &[Candidate {
                    text: "Run tests with nextest before a merge".to_owned(),
                    title: "Tests run with nextest".to_owned(),
                    aliases: vec!["테스트".to_owned()],
                    kind: CandidateKind::Rule,
                    confidence: 0.9,
                    salience: 0.9,
                    source_offsets: vec![4],
                    human_offsets: vec![4],
                    cited_paths: vec!["docs/TESTING.md".to_owned()],
                    relation: CandidateRelation::New,
                }],
            )
            .unwrap();
        drop(store);
        let read = |expanded, search: Option<&str>| {
            load_opened_memory(
                &crate::node::test_node(),
                node.as_ref(),
                &database,
                &root,
                expanded,
                search.map(str::to_owned),
            )
            .unwrap()
        };

        let load = read(Some(1), Some("테스트"));
        assert!(load.state.unwrap().enabled);
        assert_eq!(load.rows.len(), 1);
        assert_eq!(load.rows[0].kind, MemoryCardKind::Rule);
        assert_eq!(
            load.rows[0].body, None,
            "a row carries no text until expanded"
        );
        assert_eq!(
            load.expanded,
            Some(MemoryCardSnapshot {
                number: 1,
                body: "Run tests with nextest before a merge".to_owned(),
                cited_paths: vec!["docs/TESTING.md".to_owned()],
                sources: vec![MemorySourceSnapshot {
                    provider: "codex".to_owned(),
                    session_id: "s-1".to_owned(),
                    event_offset: 4,
                    human: true,
                }],
            })
        );
        assert_eq!(
            load.search,
            Some(MemorySearchSnapshot {
                query: "테스트".to_owned(),
                numbers: vec![1],
            })
        );

        let load = read(Some(9), None);
        assert_eq!(
            load.expanded, None,
            "a card that is not there shows nothing"
        );
        assert_eq!(load.search, None);
    }
}
