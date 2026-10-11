use super::*;
use crate::model::{
    ArchiveEventSnapshot, MemoryDetailSnapshot, MemoryRevisionSnapshot, MemoryRowSnapshot,
    MemorySourceSnapshot, SessionsProviderFilter, SessionsSnapshot,
};
use crate::node::NodeId;
use crate::node_access::{NodeLink, call_as};
use hide_memory::{MemoryStore, SessionCursorRecord, SessionSourceRecord};
use hide_node_link::protocol::Call;
use hide_node_link::sessions::{SessionChunk, SessionStat};
use hide_session::{Agent, ProjectSession, SessionAvailability};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// One session call on the node that holds the Project's sessions; a
/// listing walks at most `hide_session::SESSION_DISCOVERY_LIMIT` files.
pub(super) const SESSION_CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub(super) struct SessionsLoad {
    pub(super) project_id: String,
    pub(super) checkout_path: String,
    pub(super) rows: Vec<SessionRowSnapshot>,
    /// The Project's Memory when the store knows the Project.
    pub(super) memory: Option<ProjectMemoryLoad>,
}

/// A Project's Memory as one read of the store found it.
pub(super) struct ProjectMemoryLoad {
    pub(super) state: hide_memory::ProjectMemoryState,
    pub(super) rows: Vec<MemoryRowSnapshot>,
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
    pub(super) fn memory_database_path(&self) -> PathBuf {
        hide_memory::database_path(self.state_path.parent().unwrap_or(Path::new(".")))
    }

    fn focused_memory_context(&self) -> Option<(String, String, String)> {
        let (workspace, checkout) = self.focused_local_checkout()?;
        Some((
            workspace.id.clone(),
            checkout.id.clone(),
            checkout.path.clone(),
        ))
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
        self.memory_sessions_load_generation =
            self.memory_sessions_load_generation.saturating_add(1);
        let generation = self.memory_sessions_load_generation;
        if self.memory_sessions_load_in_flight {
            self.memory_sessions_load_pending = true;
            return true;
        }
        self.memory_sessions_load_in_flight = true;
        self.memory_sessions_load_pending = false;
        let sessions_node = self.own_node();
        let database = self.memory_database_path();
        let node = self.node.clone();
        thread::Builder::new()
            .name("hide-project-memory-read".to_owned())
            .spawn(move || {
                let result =
                    load_sessions(&node, sessions_node.as_ref(), &database, &checkout_path);
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
                self.session_catalog_rows = load.rows;
                self.snapshot.sessions.project_id = Some(load.project_id);
                self.snapshot.sessions.checkout_path = Some(load.checkout_path);
                self.snapshot.sessions.total_session_count = self.session_catalog_rows.len();
                self.snapshot.sessions.unavailable_reason = None;
                self.apply_session_filters();
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
                    SessionsProviderFilter::Codex => {
                        Agent::from_kind(&row.provider) == Some(Agent::Codex)
                    }
                    SessionsProviderFilter::Claude => {
                        Agent::from_kind(&row.provider) == Some(Agent::Claude)
                    }
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
    }

    pub(super) fn set_sessions_filter(&mut self, provider: &str, query: String) -> bool {
        let filter = match (provider, Agent::from_kind(provider)) {
            ("all", _) => SessionsProviderFilter::All,
            (_, Some(Agent::Codex)) => SessionsProviderFilter::Codex,
            (_, Some(Agent::Claude)) => SessionsProviderFilter::Claude,
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
        let sessions_node = self.own_node();
        thread::Builder::new()
            .name("hide-archive-detail".to_owned())
            .spawn(move || {
                let result = match kind.as_str() {
                    "session" => project_id
                        .ok_or_else(|| "Project Memory is unavailable".to_owned())
                        .and_then(|project_id| {
                            row.ok_or_else(|| "Session is no longer in this Project".to_owned())
                                .and_then(|row| {
                                    load_session_detail(
                                        sessions_node.as_ref(),
                                        &database,
                                        &project_id,
                                        row,
                                    )
                                })
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
}

/// The Project that holds `checkout_path`, as the node that holds it reads
/// it; the id names the node.
pub(crate) fn project_identity(
    sessions_node: &dyn NodeLink,
    node: &NodeId,
    checkout_path: &str,
) -> Result<hide_project::ProjectIdentity, String> {
    let facts: hide_project::ProjectFacts = call_as(
        sessions_node,
        Call::Project {
            path: checkout_path.to_owned(),
        },
        SESSION_CALL_TIMEOUT,
    )
    .map_err(|error| error.to_string())?;
    Ok(hide_project::ProjectIdentity {
        id: hide_project::project_id(node.as_str(), &facts.root),
        root: facts.root,
        checkout_root: facts.checkout_root,
        device_id: node.as_str().to_owned(),
        kind: facts.kind,
    })
}

pub(super) fn project_sessions(
    sessions_node: &dyn NodeLink,
    identity: &hide_project::ProjectIdentity,
) -> Result<Vec<ProjectSession>, String> {
    let page = hide_node_link::readers::project_sessions(
        sessions_node,
        identity.clone(),
        SESSION_CALL_TIMEOUT,
    )
    .map_err(|error| error.to_string())?;
    if page.refused > 0 {
        crate::diagnostic!(serde_json::json!({
            "component": "memory", "kind": "sessions.reader_refused",
            "project_id": identity.id, "sessions": page.refused,
        }));
    }
    for refusal in &page.store_refusals {
        crate::diagnostic!(serde_json::json!({
            "component": "memory", "kind": "sessions.store_unreadable",
            "project_id": identity.id, "agent": refusal.agent, "reason": refusal.reason,
        }));
    }
    Ok(page.rows)
}

pub(super) fn session_stat(
    sessions_node: &dyn NodeLink,
    session: &ProjectSession,
) -> Result<SessionStat, String> {
    hide_node_link::link::call_as_reader(
        sessions_node,
        session.agent,
        hide_node_link::sessions::ReaderFeature::Memory,
        Call::SessionStat {
            scope: Some(hide_session::SessionReadScope {
                id: session.id.clone(),
                cwd: session.checkout_path.to_string_lossy().into_owned(),
            }),
            path: session.locator.to_string_lossy().into_owned(),
        },
        SESSION_CALL_TIMEOUT,
    )
    .map_err(|error| error.to_string())
}

/// One bounded read of `session` past the saved checkpoint, on its node.
pub(super) fn session_chunk(
    sessions_node: &dyn NodeLink,
    session: &ProjectSession,
    saved: Option<&SessionCursorRecord>,
) -> Result<SessionChunk, String> {
    hide_node_link::link::check_reader_support(
        sessions_node,
        session.agent,
        hide_node_link::sessions::ReaderFeature::Memory,
    )
    .map_err(|error| error.to_string())?;
    let checkpoint = saved
        .filter(|record| !record.checkpoint.is_empty())
        .map(|record| serde_json::from_slice(&record.checkpoint))
        .transpose()
        .map_err(|error| format!("session_checkpoint:{error}"))?;
    hide_node_link::link::call_as_reader(
        sessions_node,
        session.agent,
        hide_node_link::sessions::ReaderFeature::Memory,
        Call::SessionChunk {
            scope: Some(hide_session::SessionReadScope {
                id: session.id.clone(),
                cwd: session.checkout_path.to_string_lossy().into_owned(),
            }),
            path: session.locator.to_string_lossy().into_owned(),
            checkpoint,
        },
        SESSION_CALL_TIMEOUT,
    )
    .map_err(|error| error.to_string())
}

pub(super) fn digest(parts: &[&str]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    format!("{:x}", hash.finalize())
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn load_sessions(
    node: &NodeId,
    sessions_node: &dyn NodeLink,
    database: &Path,
    checkout_path: &str,
) -> Result<SessionsLoad, String> {
    let identity = project_identity(sessions_node, node, checkout_path)?;
    let sessions = project_sessions(sessions_node, &identity)?;
    let mut rows = sessions
        .into_iter()
        .map(project_session_row)
        .collect::<Vec<_>>();
    let memory = if MemoryStore::exists(database) {
        let store = MemoryStore::open_read_only(database).map_err(|error| error.to_string())?;
        match load_project_memory(&store, &identity.id)? {
            Some(memory) => {
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
                Some(memory)
            }
            None => None,
        }
    } else {
        None
    };
    Ok(SessionsLoad {
        project_id: identity.id,
        checkout_path: checkout_path.to_owned(),
        rows,
        memory,
    })
}

/// The Project's Memory switch and cards, or `None` when the store has never
/// held the Project.
pub(super) fn load_project_memory(
    store: &MemoryStore,
    project_id: &str,
) -> Result<Option<ProjectMemoryLoad>, String> {
    let state = match store.project_state(project_id) {
        Ok(state) => state,
        Err(hide_memory::MemoryError::ProjectMissing) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let items = store
        .list_memories(project_id)
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
            let compared = paired.contains(&item.number)
                || item.lifecycle == hide_memory::MemoryLifecycle::Conflicting;
            memory_row(item, compared)
        })
        .collect();
    Ok(Some(ProjectMemoryLoad { state, rows }))
}

fn persisted_unavailable_session_row(source: SessionSourceRecord) -> SessionRowSnapshot {
    SessionRowSnapshot {
        id: source.id,
        provider_label: hide_agent_adapter::adapter(&source.provider)
            .map_or(source.provider.as_str(), |row| row.label)
            .to_owned(),
        provider: source.provider,
        locator: source.locator,
        checkout_path: hide_platform::path::to_wire_lossy(Path::new(&source.checkout_path)),
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

fn project_session_row(session: ProjectSession) -> SessionRowSnapshot {
    let unavailable_reason = match session.availability {
        SessionAvailability::Available => None,
        SessionAvailability::Unavailable { reason } => Some(reason),
    };
    SessionRowSnapshot {
        id: session.id,
        provider: session.agent.as_str().to_owned(),
        provider_label: session.agent.format().adapter().label.to_owned(),
        locator: session.locator.to_string_lossy().into_owned(),
        checkout_path: hide_platform::path::to_wire_lossy(&session.checkout_path),
        first_human_request: session.first_human_request,
        started_at_unix_ms: session.started_at_unix_ms,
        updated_at_unix_ms: session.updated_at_unix_ms,
        title: session.title,
        unavailable_reason,
    }
}

fn memory_row(item: hide_memory::MemoryItem, with_body: bool) -> MemoryRowSnapshot {
    MemoryRowSnapshot {
        id: item.id,
        number: item.number,
        title: item.title,
        kind: item.kind.as_str().to_owned(),
        lifecycle: lifecycle_name(item.lifecycle),
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

fn lifecycle_name(value: hide_memory::MemoryLifecycle) -> String {
    format!("{value:?}").to_ascii_lowercase()
}

pub(super) fn load_session_detail(
    sessions_node: &dyn NodeLink,
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
    if Agent::from_kind(&row.provider) == Some(Agent::OpenCode) {
        return load_opencode_detail(sessions_node, database, project_id, row);
    }
    let agent = Agent::from_kind(&row.provider)
        .filter(|agent| agent.has_session_file())
        .ok_or_else(|| format!("Unsupported session provider: {}", row.provider))?;
    let scope = hide_session::SessionReadScope {
        id: row.id.clone(),
        cwd: row.checkout_path.clone(),
    };
    let mut parsed = if agent == Agent::Cursor {
        let mut parsed = hide_session::ParsedSession::default();
        let mut checkpoint = None;
        let mut read_bytes = 0u64;
        let mut text_bytes = 0u64;
        let mut pages = 0;
        let mut previous_offset = None;
        loop {
            pages += 1;
            if pages > hide_session::cursor::ARCHIVE_PAGE_LIMIT {
                return Err("session_conversation_capacity".to_owned());
            }
            let page: hide_session::ConversationRead = hide_node_link::link::call_as_reader(
                sessions_node,
                agent,
                hide_node_link::sessions::ReaderFeature::Conversation,
                Call::SessionConversation {
                    agent,
                    path: row.locator.clone(),
                    scope: scope.clone(),
                    checkpoint,
                },
                SESSION_CALL_TIMEOUT,
            )
            .map_err(|_| "session_conversation_unavailable".to_owned())?;
            if page.rescanned || page.events.len() != page.event_offsets.len() {
                return Err("session_conversation_changed".to_owned());
            }
            if page.read_bytes > hide_session::SESSION_INCREMENT_READ_LIMIT_BYTES
                || (page.has_more
                    && previous_offset.is_some_and(|offset| page.checkpoint.offset() <= offset))
                || page.event_offsets.windows(2).any(|pair| pair[0] >= pair[1])
                || page.event_offsets.iter().any(|offset| {
                    *offset >= page.checkpoint.offset()
                        || previous_offset.is_some_and(|previous| *offset < previous)
                })
            {
                return Err("session_conversation_no_progress".to_owned());
            }
            read_bytes = read_bytes.saturating_add(page.read_bytes);
            text_bytes = text_bytes.saturating_add(
                page.events
                    .iter()
                    .map(|event| event.text.len() as u64)
                    .sum::<u64>(),
            );
            if read_bytes > hide_session::SESSION_READ_LIMIT_BYTES
                || text_bytes > hide_session::SESSION_READ_LIMIT_BYTES
                || parsed.events.len() + page.events.len() > hide_session::SESSION_DISCOVERY_LIMIT
            {
                return Err("session_conversation_capacity".to_owned());
            }
            if page.has_more && page.read_bytes == 0 {
                return Err("session_conversation_no_progress".to_owned());
            }
            parsed.events.extend(page.events);
            parsed.event_offsets.extend(page.event_offsets);
            if !page.has_more {
                break;
            }
            previous_offset = Some(page.checkpoint.offset());
            checkpoint = Some(page.checkpoint);
        }
        parsed
    } else {
        let contents: String = hide_node_link::link::call_as_reader(
            sessions_node,
            agent,
            hide_node_link::sessions::ReaderFeature::Conversation,
            Call::SessionText {
                scope: Some(scope),
                path: row.locator.clone(),
            },
            SESSION_CALL_TIMEOUT,
        )
        .map_err(|error| format!("Session unavailable: {error}"))?;
        hide_session::parse_events(agent, &contents)
    };
    let store = MemoryStore::exists(database)
        .then(|| MemoryStore::open_read_only(database).map_err(|error| error.to_string()))
        .transpose()?;
    // A message a format splits into records is one message to read.
    parsed.coalesce();
    let events = parsed
        .events
        .into_iter()
        .zip(parsed.event_offsets)
        .map(|(event, source_offset)| {
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
                source_offset: Some(source_offset),
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

/// The most bounded reads one OpenCode archive detail takes: a session read
/// a budget at a time, as large as one session file the detail parses.
const OPENCODE_DETAIL_READS: usize = (hide_session::SESSION_READ_LIMIT_BYTES
    / hide_session::SESSION_INCREMENT_READ_LIMIT_BYTES)
    as usize;

/// An OpenCode session's archive detail, read from OpenCode's database in
/// the bounded reads its label reader makes, each between the root and
/// checkout proof of the catalog row. Hide's Memory receipt rides in a part
/// only Hide's plugin writes, so it is checked as provider-injected text.
fn load_opencode_detail(
    sessions_node: &dyn NodeLink,
    database: &Path,
    project_id: &str,
    row: SessionRowSnapshot,
) -> Result<ArchiveDetailSnapshot, String> {
    let mut request = hide_session::label_transcript::LabelTranscriptRequest {
        agent: Agent::OpenCode,
        reference_kind: "id".into(),
        reference_value: row.id.clone(),
        cwd: Some(row.checkout_path.clone()),
        checkpoint: None,
        subagents: Default::default(),
        turns: None,
    };
    let mut events = Vec::new();
    let mut receipts = Vec::new();
    let mut owner = None;
    for read in 0.. {
        if read == OPENCODE_DETAIL_READS {
            return Err("Session unavailable: session_too_large".to_owned());
        }
        let transcript: hide_session::label_transcript::LabelTranscript =
            hide_node_link::link::call_as_reader(
                sessions_node,
                Agent::OpenCode,
                hide_node_link::sessions::ReaderFeature::Conversation,
                Call::LabelTranscript {
                    request: request.clone(),
                },
                SESSION_CALL_TIMEOUT,
            )
            .map_err(|error| format!("Session unavailable: {error}"))?;
        // A session rewound or replaced between reads is not one history.
        if transcript.rescanned.is_some()
            || owner.get_or_insert_with(|| transcript.confirmed.incarnation.clone())
                != &transcript.confirmed.incarnation
        {
            return Err("Session unavailable: label_session_read_changed".to_owned());
        }
        events.extend(transcript.events);
        receipts.extend(transcript.memory_receipts);
        if !transcript.has_more {
            break;
        }
        request.checkpoint = Some(transcript.checkpoint);
    }
    let store = MemoryStore::exists(database)
        .then(|| MemoryStore::open_read_only(database).map_err(|error| error.to_string()))
        .transpose()?;
    let events = events
        .into_iter()
        .map(|event| {
            let receipt = store.as_ref().and_then(|store| {
                receipts
                    .iter()
                    .filter(|receipt| receipt.offset == event.offset)
                    .find_map(|receipt| {
                        trusted_memory_receipt(
                            store,
                            project_id,
                            &row.provider,
                            &row.id,
                            true,
                            &receipt.text,
                        )
                    })
            });
            ArchiveEventSnapshot {
                source_offset: Some(event.offset),
                role: event.kind.role().to_owned(),
                kind: match event.kind {
                    hide_session::label_transcript::LabelEventKind::Human => "human",
                    hide_session::label_transcript::LabelEventKind::Assistant => "assistant",
                    hide_session::label_transcript::LabelEventKind::Interrupted => "interrupted",
                }
                .to_owned(),
                at_unix_ms: event.at_unix_ms,
                memory_attached_count: receipt.as_ref().map(|receipt| receipt.count),
                memory_attached_item_ids: receipt
                    .map(|receipt| receipt.items.into_iter().map(|(id, _)| id).collect())
                    .unwrap_or_default(),
                text: event.text,
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

pub(super) struct MemoryReceipt {
    event: String,
    count: usize,
    item_key: String,
    items: Vec<(String, u64)>,
    auth: String,
}

impl MemoryReceipt {
    /// The turn a receipt belongs to: the offset of the event that carried
    /// it, which every reader of the same file finds at the same place.
    pub(super) fn turn_id(offset: u64) -> String {
        format!("event:{offset}")
    }

    /// The cards it names as `m<number>@<revision>`, comma-separated.
    pub(super) fn item_key(&self) -> &str {
        &self.item_key
    }
}

pub(super) fn trusted_memory_receipt(
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
