//! Project Memory as a shell shows it in its Tools column's Memory tab (PRD
//! project-memory-v2 B16, B19-B24): the Project a shell opened Memory for
//! with `memory_open`, the operator's actions on it, one card in full and a
//! search.
//!
//! Memory is its own snapshot section, keyed by the Project a shell opens;
//! nothing here follows the focused checkout or the Overview's Sessions.
//! The Project's folder is read by the node of the machine that holds it:
//! this machine's or a node that dials in (PRD core-host-node-move B14).
//! Every store read and write runs on a worker; the lock only starts it and
//! applies its answer.

use super::memory::project_identity;
use super::memory_analysis::MemoryChange;
use super::*;
use crate::model::{
    MemoryAction, MemoryAnalysisState, MemoryCardKind, MemoryCardSnapshot, MemoryPreviousSnapshot,
    MemoryQuestionSnapshot, MemoryRowSnapshot, MemorySearchSnapshot, MemorySnapshot,
    MemorySourceSnapshot, MemoryUnavailableSnapshot, ProjectMemorySnapshot,
};
use hide_agent_hooks::HookStatus;
use hide_memory::{MemoryLifecycle, MemoryStore};
use serde_json::json;

/// The most bytes of a screen search, as `hide memory search` takes.
const SEARCH_LIMIT_BYTES: usize = crate::memory_command::SEARCH_LIMIT_BYTES;

/// The reads and the action behind Project Memory on screen.
#[derive(Default)]
pub(super) struct ProjectMemoryWork {
    /// The opened Project's folder, with the machine that holds it, while
    /// Memory keeps it.
    path: Option<(crate::node::NodeId, String)>,
    /// Memory's id for the opened Project, once a read resolved it.
    pub(super) project_id: Option<String>,
    /// The card shown in full and the screen's search words.
    expanded: Option<u64>,
    search: Option<String>,
    reads: ReadFence,
    /// The catalog Project an action runs for, with the action.
    pub(super) action: Option<(String, MemoryAction)>,
    /// The Project whose Memory turns on once the hook update its switch
    /// started lands, by catalog Project.
    enable_after_hook_update: Option<String>,
    /// The tracker of the cards on their way into the repository.
    pub(super) promotions: super::memory_promotion::PromotionTracking,
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
    /// Each card's promotion, drawn on its row once the ledger names its
    /// agent under the lock.
    promotions: Vec<(u64, hide_memory::Promotion)>,
    suggestions: Vec<hide_memory::PromotionSuggestion>,
    expanded: Option<MemoryCardSnapshot>,
    search: Option<MemorySearchSnapshot>,
}

impl Runtime {
    /// Checks a `hide memory` caller like a Factory command does and takes
    /// what the command needs (B16, D-22): a caller on this machine or on a
    /// node that dials in is answered, its Project read by its own node (PRD
    /// core-host-node-move B14), and an agent pane's own session is the one a
    /// `show` is recorded for.
    pub(crate) fn prepare_memory(
        &self,
        device: &str,
        caller: &str,
        expected: &crate::workspace_control::Context,
        command: crate::memory_command::MemoryCommand,
    ) -> Result<crate::memory_command::PreparedMemory, String> {
        let (node, sessions_node) = self
            .linked_node(device)
            .map_err(|_| "memory_device_unsupported".to_owned())?;
        // The caller's checkout as it is now; Memory needs no window, so
        // the Views a Workspace query also checks do not matter here.
        let context = self
            .resolve_caller(device, crate::workspace_control::Caller::parse(caller))
            .map_err(|refusal| refusal.reason.to_owned())?;
        if context != *expected {
            return Err("caller_context_changed".into());
        }
        // The pane's agent on the caller's own machine: a node's pane is
        // found among that node's agents, never this machine's.
        let reader = match crate::workspace_control::Caller::parse(caller) {
            crate::workspace_control::Caller::Pane(pane) => self
                .machine_agents(device)
                .iter()
                .find(|agent| agent.pane_id == pane)
                .and_then(|agent| {
                    let provider = hide_session::Agent::from_kind(&agent.agent_kind)?;
                    Some((provider.as_str().to_owned(), agent.session_id.clone()?))
                }),
            crate::workspace_control::Caller::Checkout { .. } => None,
        };
        let caller_agents = match crate::workspace_control::Caller::parse(caller) {
            crate::workspace_control::Caller::Pane(pane) => self
                .delivery_state()
                .map(|ledger| {
                    ledger
                        .agents
                        .iter()
                        .filter(|record| {
                            !record.ended && record.pane == pane && record.machine == device
                        })
                        .map(|record| record.id.clone())
                        .collect()
                })
                .unwrap_or_default(),
            crate::workspace_control::Caller::Checkout { .. } => Vec::new(),
        };
        Ok(crate::memory_command::PreparedMemory {
            command,
            node,
            sessions_node,
            ai_node: self.own_node(),
            database: self.memory_database_path(),
            checkout_path: context.checkout_path,
            reader,
            settings: self.ai_active().then(|| self.ai_settings.clone()).flatten(),
            caller_agents,
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
                    action: self.project_memory.action.take(),
                    enable_after_hook_update: self.project_memory.enable_after_hook_update.take(),
                    // The tracker follows every Project's promotions and may
                    // be mid-check; closing a read must not drop its flight.
                    promotions: std::mem::take(&mut self.project_memory.promotions),
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
            Err(unavailable) => {
                self.project_memory.path = None;
                self.project_memory.reads.generation += 1;
                if let Some(project) = self.memory_section().project.as_mut() {
                    *project = ProjectMemorySnapshot {
                        workspace_id,
                        unavailable: Some(unavailable),
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

    /// Where a catalog Project's Memory is read from, with the machine that
    /// holds it, or why it is not kept: Memory is kept for the Projects of
    /// this machine and of a node that dials in, never of a device the core
    /// dials (B16; PRD core-host-node-move B14, Q20). A node whose link is
    /// down keeps its Projects' Memory; reading it waits for the link.
    fn memory_folder(
        &self,
        workspace_id: &str,
    ) -> Result<(crate::node::NodeId, String), MemoryUnavailableSnapshot> {
        let workspace = self
            .catalog_workspaces()
            .find(|workspace| workspace.id == workspace_id)
            .ok_or(MemoryUnavailableSnapshot::Unregistered)?;
        let node = match self.linked_node(&workspace.device_id) {
            Ok((node, _)) => Some(node),
            Err("link_down") => crate::node::NodeId::parse(&workspace.device_id).ok(),
            Err(_) => None,
        };
        let Some(node) = node else {
            let device_name = self
                .snapshot
                .navigator
                .devices
                .iter()
                .find(|device| device.id == workspace.device_id)
                .map_or_else(
                    || workspace.device_id.clone(),
                    |device| device.label.clone(),
                );
            return Err(MemoryUnavailableSnapshot::RemoteDevice { device_name });
        };
        Ok((node, workspace.path.clone()))
    }

    /// Shows one card of the opened Project in full, or none.
    pub(super) fn expand_memory_card(&mut self, workspace_id: &str, number: Option<u64>) -> bool {
        if !self.memory_opened(workspace_id) {
            return false;
        }
        // A card opened from elsewhere (a conversation's card line, B33) that
        // the standing search does not show for certain clears the search, so
        // it is shown: the search's answer may not have landed yet.
        let shown = |project: &crate::model::ProjectMemorySnapshot, query: &str, number: u64| {
            project
                .search
                .as_ref()
                .is_some_and(|search| search.query == query && search.numbers.contains(&number))
        };
        let mut cleared = false;
        if let Some(number) = number
            && let Some(query) = self.project_memory.search.clone()
            && !self
                .memory_section()
                .project
                .as_ref()
                .is_some_and(|project| shown(project, &query, number))
        {
            self.project_memory.search = None;
            if let Some(project) = self.memory_section().project.as_mut() {
                project.search = None;
            }
            cleared = true;
        }
        if self.project_memory.expanded == number && !cleared {
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
        let Some((node, path)) = self.project_memory.path.clone() else {
            return;
        };
        let Some(context) = self.worker_context.clone() else {
            return;
        };
        let sessions_node = match self.linked_node(node.as_str()) {
            Ok((_, link)) => link,
            Err(reason) => {
                crate::diagnostic!(json!({
                    "component": "memory",
                    "kind": "memory.read_unreachable",
                    "device": node.as_str(),
                    "reason": reason,
                }));
                if let Some(project) = self.memory_section().project.as_mut() {
                    project.loading = false;
                    project.unavailable = Some(MemoryUnavailableSnapshot::Unreadable);
                }
                return;
            }
        };
        let Some(generation) = self.project_memory.reads.begin() else {
            return;
        };
        let database = self.memory_database_path();
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
                    project.unavailable = Some(MemoryUnavailableSnapshot::Unreadable);
                }
                return true;
            }
        };
        self.project_memory.project_id = Some(load.project_id);
        let Some(project) = self.memory_section().project.as_mut() else {
            return false;
        };
        project.loading = false;
        project.unavailable = None;
        project.enabled = load.state.as_ref().is_some_and(|state| state.enabled);
        project.capacity_reached = load
            .state
            .as_ref()
            .is_some_and(|state| state.capacity_reached);
        let project_id = self.project_memory.project_id.clone().unwrap_or_default();
        let mut rows = load.rows;
        let mut questions = Vec::new();
        let mut open = false;
        for (number, promotion) in &load.promotions {
            open |= matches!(
                promotion.state,
                hide_memory::PromotionState::Running | hide_memory::PromotionState::PrOpen
            );
            let snapshot = self.promotion_snapshot(&project_id, *number, promotion);
            let Some(row) = rows.iter_mut().find(|row| row.number == *number) else {
                continue;
            };
            if promotion.state == hide_memory::PromotionState::Failed {
                questions.push(MemoryQuestionSnapshot::Failed {
                    number: *number,
                    title: row.title.clone(),
                });
            }
            row.promotion = snapshot;
        }
        questions.extend(load.suggestions.iter().filter_map(|suggestion| {
            let row = rows.iter().find(|row| row.number == suggestion.number)?;
            Some(MemoryQuestionSnapshot::Promote {
                number: suggestion.number,
                title: row.title.clone(),
                human_source_count: suggestion.human_source_count,
                pulled_session_count: suggestion.pulled_session_count,
            })
        }));
        if open {
            self.project_memory.promotions.mark_open();
        }
        let Some(project) = self.memory_section().project.as_mut() else {
            return false;
        };
        if *project.rows != rows {
            project.rows = Arc::new(rows);
        }
        project.questions = questions;
        project.expanded = load.expanded;
        project.search = load.search;
        self.sync_project_memory();
        true
    }

    /// Copies the opened Project's analysis state, forgotten card and running
    /// action onto its snapshot.
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
        let ai_ready = self.ai_active() && self.ai_settings.is_some();
        let Some(project) = self
            .snapshot
            .memory
            .as_mut()
            .and_then(|memory| memory.project.as_mut())
        else {
            return;
        };
        // Hide AI that is off is "not ready" at once, not after a pass
        // fails (D-27); while Memory is off nothing is analyzed.
        project.analysis = match (project.enabled, ai_ready) {
            (false, _) => MemoryAnalysisState::Idle,
            (true, false) => MemoryAnalysisState::AiNotReady,
            (true, true) => work.analysis,
        };
        project.forgotten = work.forgotten;
        project.pending_action = action;
    }

    pub(super) fn hooks_support_memory(&self) -> bool {
        self.hook_diagnosis.as_ref().is_some_and(|diagnosis| {
            diagnosis.runtimes.iter().any(|row| {
                row.memory_compatibility.supports_injection()
                    && matches!(row.status, HookStatus::Installed { version } if version >= hide_agent_hooks::HOOK_VERSION)
            })
        })
    }

    /// One operator action on a catalog Project's Memory (B19, B22, B24),
    /// from its Memory tab. One action runs at a time; another is refused
    /// with the reason.
    pub(super) fn apply_memory_action(&mut self, payload: events::MemoryActionPayload) -> bool {
        let (node, path) = match self.memory_folder(&payload.workspace_id) {
            Ok(folder) => folder,
            Err(_) => {
                self.set_error(
                    "memory.project_unavailable",
                    "Project Memory is kept only for the Projects of this computer and of computers that connect to it",
                    false,
                );
                return true;
            }
        };
        let sessions_node = match self.linked_node(node.as_str()) {
            Ok((_, link)) => link,
            Err(_) => {
                self.set_error(
                    "memory.project_unreachable",
                    "The computer that holds this Project is not connected",
                    true,
                );
                return true;
            }
        };
        if payload.action == MemoryAction::Enable && !self.hooks_support_memory() {
            // Turning Memory on is the operator's word to bring the agent
            // hooks that carry it up to date: the kit's Reinstall of this
            // Mac's hook parts, which owns every hook write (PRD device-parity
            // D-27). Memory turns on once they land.
            if self.hook_diagnosis.is_none() {
                self.set_error(
                    "memory.hooks_unread",
                    "Hide is still reading the agent hooks; try again in a moment",
                    false,
                );
                return true;
            }
            let parts = self.local_hook_parts_to_repair();
            if !parts.is_empty() {
                let node = self.node.clone();
                self.queue_kit_reinstall(node.as_str(), parts);
                self.project_memory.enable_after_hook_update = Some(payload.workspace_id);
                return true;
            }
            // No agent here takes Memory's hooks; Memory is on for the ones
            // installed later, which the kit sets up with Memory.
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
        let start = match payload.action {
            MemoryAction::Promote => match self.promotion_start(&node, &path) {
                Ok(start) => Some(start),
                Err(reason) => {
                    crate::diagnostic!(json!({
                        "component": "memory",
                        "kind": "promotion.start_unavailable",
                        "workspace_id": payload.workspace_id,
                        "reason": reason,
                    }));
                    self.set_error(
                        "memory.promotion_unavailable",
                        "Hide cannot start an agent for this Project right now",
                        true,
                    );
                    return true;
                }
            },
            _ => None,
        };
        let action = payload.action;
        let number = payload.number;
        self.project_memory.action = Some((payload.workspace_id.clone(), action));
        self.sync_project_memory();
        let database = self.memory_database_path();
        let spawn = thread::Builder::new()
            .name("hide-project-memory-write".to_owned())
            .spawn(move || {
                let result = mutate_memory(
                    &node,
                    sessions_node.as_ref(),
                    &database,
                    &path,
                    &payload,
                    start,
                );
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.finish_memory_action(action, number, result),
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
        number: Option<u64>,
        result: Result<MemoryActionOutcome, String>,
    ) -> bool {
        self.project_memory.action = None;
        match result {
            Ok(outcome) => {
                if let Some((kind, message)) = outcome.diagnostic {
                    self.push_diagnostic(kind, message);
                }
                let project_id = outcome.project_id;
                match (action, number) {
                    (MemoryAction::Enable, _) => self.wake_project_memory(&project_id),
                    // A pass that started between the request and the store
                    // write still saw the Project on; the store refuses its
                    // cards, and this stops it asking the provider.
                    (MemoryAction::Disable | MemoryAction::Delete, _) => {
                        self.quiet_project_memory(&project_id)
                    }
                    (MemoryAction::Forget, Some(number)) => {
                        self.note_memory_change(&project_id, MemoryChange::Forgot(number))
                    }
                    (MemoryAction::Undo, Some(number)) => {
                        self.note_memory_change(&project_id, MemoryChange::Undid(number))
                    }
                    (MemoryAction::Promote, _) => {
                        if let Some((number, attempt, start)) = outcome.unseen {
                            self.project_memory
                                .promotions
                                .add_unseen((project_id.clone(), number, attempt), start);
                        }
                        self.promotions_moved()
                    }
                    _ => {}
                }
            }
            Err(message) => self.set_error("memory.action_failed", message, true),
        }
        self.sync_project_memory();
        self.refresh_project_memory();
        true
    }

    /// Turns Memory on for the Project whose switch started a hook update,
    /// once the hooks carry Memory.
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
        });
    }
}

struct MemoryActionOutcome {
    project_id: String,
    diagnostic: Option<(String, String)>,
    /// A promotion whose agent was started but not seen yet: the card, the
    /// attempt and its start, for the tracker to ask again.
    unseen: Option<(u64, u32, super::memory_promotion::PromotionStart)>,
}

fn mutate_memory(
    node: &crate::node::NodeId,
    sessions_node: &dyn crate::node_access::NodeLink,
    database: &Path,
    checkout_path: &str,
    payload: &events::MemoryActionPayload,
    start: Option<super::memory_promotion::PromotionStart>,
) -> Result<MemoryActionOutcome, String> {
    let identity = project_identity(sessions_node, node, checkout_path)?;
    let mut store = MemoryStore::open(database).map_err(|error| error.to_string())?;
    let outcome = || MemoryActionOutcome {
        project_id: identity.id.clone(),
        diagnostic: None,
        unseen: None,
    };
    let number = payload.number;
    let named = || number.ok_or("The Memory action names no card");
    match payload.action {
        MemoryAction::Enable => {
            // Turning Memory on is accepting the disclosure the switch shows.
            store
                .ensure_project(&identity.id, &identity.root, node.as_str())
                .map_err(|error| error.to_string())?;
            store
                .set_enabled(&identity.id, true, true)
                .map_err(|error| error.to_string())?;
            Ok(outcome())
        }
        MemoryAction::Disable => {
            store
                .set_enabled(&identity.id, false, false)
                .map_err(|error| error.to_string())?;
            Ok(outcome())
        }
        MemoryAction::Delete => {
            let deletion = store
                .delete_project_data(&identity.id)
                .map_err(|error| error.to_string())?;
            Ok(MemoryActionOutcome {
                unseen: None,
                diagnostic: deletion.cleanup_warning.map(|warning| {
                    (
                        "memory.delete_cleanup_deferred".to_owned(),
                        format!(
                            "Project Memory data was deleted; page cleanup was deferred: {warning}"
                        ),
                    )
                }),
                ..outcome()
            })
        }
        MemoryAction::Forget => {
            let number = named()?;
            let item = store
                .item_by_number(&identity.id, number)
                .map_err(|error| error.to_string())?
                .filter(|item| item.lifecycle != MemoryLifecycle::Tombstoned)
                .ok_or_else(|| format!("m{number} is no longer in this Project's Memory"))?;
            store
                .forget(&identity.id, &item.id)
                .map_err(|error| error.to_string())?;
            Ok(outcome())
        }
        // Steps the card back one change: a forgotten card returns, a
        // replaced text gives way to the one before it (B19, B24).
        MemoryAction::Undo => {
            let number = named()?;
            store
                .undo_card(&identity.id, number)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("m{number} has no change to undo"))?;
            Ok(outcome())
        }
        MemoryAction::Promote => {
            let start = start.ok_or("The promotion has no agent to start")?;
            let number = named()?;
            let unseen =
                super::memory_promotion::start_promotion(&mut store, &identity.id, number, start)?;
            Ok(MemoryActionOutcome {
                unseen: unseen.map(|(attempt, start)| (number, attempt, start)),
                ..outcome()
            })
        }
        MemoryAction::Dismiss => {
            super::memory_promotion::dismiss_promotion(&mut store, &identity.id, named()?)?;
            Ok(outcome())
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
        promotions: Vec::new(),
        suggestions: Vec::new(),
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
    let rows = store
        .list_memories(&identity.id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(memory_row)
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
                            provider_label: provider_label(&source.provider),
                            at_unix_ms: source.at_unix_ms,
                            human: source.human,
                        })
                        .collect(),
                    previous: detail.previous.map(|previous| MemoryPreviousSnapshot {
                        body: previous.body,
                        changed_at_unix_ms: previous.changed_at_unix_ms,
                    }),
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
    let promotions = store
        .promotions(&identity.id)
        .map_err(|error| error.to_string())?;
    let suggestions = store
        .promotion_suggestions(
            &identity.id,
            super::memory::now_ms(),
            hide_memory::PROMOTION_OPEN_LIMIT,
        )
        .map_err(|error| error.to_string())?;
    Ok(OpenedMemoryLoad {
        project_id: identity.id,
        state: Some(state),
        rows,
        promotions,
        suggestions,
        expanded,
        search,
    })
}

fn memory_row(item: hide_memory::MemoryItem) -> MemoryRowSnapshot {
    MemoryRowSnapshot {
        number: item.number,
        title: item.title,
        kind: card_kind(item.kind),
        human_source_count: item.human_source_count,
        provided_session_count: item.provided_session_count,
        pull_count: item.pull_count,
        updated_at_unix_ms: item.updated_at_unix_ms,
        promotion: None,
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

/// A source's runtime as the screens name it; a card `hide memory add`
/// wrote names the agent that added it only by Hide.
fn provider_label(provider: &str) -> String {
    match hide_agent_adapter::adapter(provider) {
        Some(_) => crate::sidebar::provider_name(Some(provider)),
        None => "Hide".to_owned(),
    }
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
                    source_times: [(4, 77)].into(),
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
            load.expanded,
            Some(MemoryCardSnapshot {
                number: 1,
                body: "Run tests with nextest before a merge".to_owned(),
                cited_paths: vec!["docs/TESTING.md".to_owned()],
                sources: vec![MemorySourceSnapshot {
                    provider_label: "Codex".to_owned(),
                    at_unix_ms: Some(77),
                    human: true,
                }],
                previous: None,
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

    #[test]
    fn a_read_puts_each_promotion_on_its_row_and_asks_only_about_suggestions_and_failures() {
        // B28-B32.
        use crate::model::{MemoryPromotionSnapshot, MemoryPromotionState};
        use hide_memory::{Promotion, PromotionState};
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
        let card = |text: &str, kind, human: Vec<u64>| Candidate {
            text: text.to_owned(),
            title: text.to_owned(),
            aliases: Vec::new(),
            kind,
            confidence: 0.9,
            salience: 0.9,
            source_offsets: human.clone(),
            human_offsets: human,
            source_times: Default::default(),
            cited_paths: Vec::new(),
            relation: CandidateRelation::New,
        };
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
                &[
                    card("Lint runs before tests", CandidateKind::Rule, vec![1, 2]),
                    card(
                        "Releases are tagged on main",
                        CandidateKind::Decision,
                        vec![3],
                    ),
                    card(
                        "Migrations never rewrite a column",
                        CandidateKind::Lesson,
                        vec![4],
                    ),
                    card(
                        "The service listens on port 8080",
                        CandidateKind::Fact,
                        vec![5, 6],
                    ),
                ],
            )
            .unwrap();
        let promotion = |state, pr_number| Promotion {
            state,
            revision: 1,
            attempt: 1,
            agent_id: Some("agent-1".to_owned()),
            branch: Some("hide-memory-m1".to_owned()),
            pr_number,
            target_path: None,
            started_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        };
        store
            .set_promotion(&identity.id, 2, &promotion(PromotionState::Failed, None))
            .unwrap();
        store
            .set_promotion(
                &identity.id,
                3,
                &promotion(PromotionState::PrOpen, Some(924)),
            )
            .unwrap();
        drop(store);
        let load = load_opened_memory(
            &crate::node::test_node(),
            node.as_ref(),
            &database,
            &root,
            None,
            None,
        )
        .unwrap();

        let mut runtime = crate::runtime::tests::runtime();
        runtime.ingest_session(Ok(crate::runtime::tests::context_payload()));
        let workspace = runtime.snapshot.navigator.workspaces[0].id.clone();
        runtime.open_project_memory(Some(workspace));
        let generation = runtime.project_memory.reads.begin().unwrap();
        runtime.project_memory.promotions = Default::default();
        assert!(runtime.ingest_project_memory(generation, Ok(load)));

        let project = runtime
            .snapshot
            .memory
            .as_ref()
            .unwrap()
            .project
            .as_ref()
            .unwrap();
        let mut promotions = project
            .rows
            .iter()
            .map(|row| {
                (
                    row.number,
                    row.promotion.as_ref().map(|promotion| promotion.state),
                )
            })
            .collect::<Vec<_>>();
        promotions.sort_by_key(|(number, _)| *number);
        assert_eq!(
            promotions,
            vec![
                (1, None),
                (2, Some(MemoryPromotionState::Failed)),
                (3, Some(MemoryPromotionState::PrOpen)),
                (4, None),
            ]
        );
        assert_eq!(
            project
                .rows
                .iter()
                .find(|row| row.number == 3)
                .unwrap()
                .promotion,
            Some(MemoryPromotionSnapshot {
                state: MemoryPromotionState::PrOpen,
                agent: None,
                pr_number: Some(924),
                target_path: None,
                github_failure: None,
            })
        );
        assert_eq!(
            project.questions,
            vec![
                MemoryQuestionSnapshot::Failed {
                    number: 2,
                    title: "Releases are tagged on main".to_owned(),
                },
                MemoryQuestionSnapshot::Promote {
                    number: 1,
                    title: "Lint runs before tests".to_owned(),
                    human_source_count: 2,
                    pulled_session_count: 0,
                },
            ],
            "a fact is never suggested, an open pull request asks nothing"
        );
    }
}
