//! The explicit missed-work preview and create. Disk, session, provider and
//! GitHub calls run off Runtime's lock; the link worker owns the durable intent.

use super::*;
use crate::links::ledger::{LedgerAnswer, LedgerCall, MissedSource, MissedStatus};
use crate::links::worker::LinkClient;
use crate::model::SessionIssueDraftSnapshot;
use crate::node_access::NodeLink;
use hide_ai::{AiSettings, CancelToken};
use hide_node_link::protocol::Call;

const DRAFT_PLACE_LIMIT: usize = 512;

#[derive(Clone)]
struct Place {
    workspace_id: String,
    project: String,
    checkout: String,
    repository: Option<String>,
    target: Result<crate::github::Where, String>,
    connection_epoch: u64,
}

#[derive(Default)]
pub(super) struct SessionIssueWork {
    generation: u64,
    /// Remains owned until the cancelled worker answers: repeated Cancel /
    /// Draft cannot create an unbounded set of threads or provider processes.
    active: Option<CancelToken>,
    ready: Option<Place>,
    create_active: Option<(String, String, CancelToken)>,
}

struct Draft {
    place: Place,
    title: String,
    body: String,
    message: Option<String>,
}

impl Runtime {
    pub(super) fn invalidate_session_issue_repository(&mut self, device: &str, root: &str) {
        let owns = self
            .snapshot
            .issue_work
            .draft
            .as_ref()
            .is_some_and(|draft| {
                draft.source.session.device_id == device
                    && draft.workspace_id.as_ref().is_some_and(|id| {
                        self.issue_project(id).is_some_and(|(_, workspace)| {
                            workspace.device_id == device && workspace.path == root
                        })
                    })
            });
        if owns {
            if let Some(cancel) = &self.session_issue_work.active {
                cancel.cancel();
            }
            self.session_issue_work.ready = None;
            if let Some(slot) = &mut self.snapshot.issue_work.draft {
                slot.phase = "failed".into();
                slot.message = Some("issue_draft_repository_changed".into());
            }
        }
        if let Some((owner, project, cancel)) = &self.session_issue_work.create_active
            && owner == device
            && project == root
        {
            cancel.cancel();
        }
    }

    fn session_issue_place_current(&self, device: &str, place: &Place) -> bool {
        self.issue_project(&place.workspace_id)
            .is_some_and(|(_, workspace)| {
                workspace.device_id == device
                    && workspace.path == place.project
                    && workspace
                        .checkouts
                        .iter()
                        .any(|checkout| checkout.path == place.checkout)
                    && self.github_place(
                        (device != self.node.as_str()).then_some(device),
                        &place.project,
                    ) == place.target
                    && self
                        .links_work
                        .connection_epochs
                        .get(&hide_project::project_id(device, Path::new(&place.project)))
                        .copied()
                        .unwrap_or(0)
                        == place.connection_epoch
            })
    }

    pub(crate) fn cancel_session_issue_draft(&mut self) -> bool {
        if let Some(cancel) = &self.session_issue_work.active {
            cancel.cancel();
        }
        self.session_issue_work.ready = None;
        self.snapshot.issue_work.draft.take().is_some()
    }

    pub(super) fn draft_missed_issue(&mut self, source: MissedSource) -> bool {
        if !self
            .ai_settings
            .as_ref()
            .is_some_and(|settings| settings.agent_summary && settings.enabled && settings.chosen)
        {
            return false;
        }
        if self.session_issue_work.active.is_some() {
            self.set_error(
                "session_issue_draft.busy",
                "A session issue preview is still running",
                true,
            );
            return true;
        }
        let Some(client) = self.links_work.client.clone() else {
            self.set_error(
                "session_issue_draft.unavailable",
                "The session record is unavailable",
                true,
            );
            return true;
        };
        let node = match self.node_link(&source.session.device_id) {
            Ok(node) => node,
            Err(_) => {
                self.set_error(
                    "session_issue_draft.offline",
                    "The session's device is not connected",
                    true,
                );
                return true;
            }
        };
        let mut places = Vec::new();
        let mut overflow = false;
        let mut add = |workspace: &WorkspaceSnapshot| {
            if workspace.device_id != source.session.device_id {
                return;
            }
            for checkout in &workspace.checkouts {
                if places.len() >= DRAFT_PLACE_LIMIT {
                    overflow = true;
                    break;
                }
                places.push(Place {
                    workspace_id: workspace.id.clone(),
                    project: workspace.path.clone(),
                    checkout: checkout.path.clone(),
                    repository: workspace.home_issues.repository.clone(),
                    target: self.github_place(
                        (workspace.device_id != self.node.as_str())
                            .then_some(workspace.device_id.as_str()),
                        &workspace.path,
                    ),
                    connection_epoch: self
                        .links_work
                        .connection_epochs
                        .get(&hide_project::project_id(
                            &workspace.device_id,
                            Path::new(&workspace.path),
                        ))
                        .copied()
                        .unwrap_or(0),
                });
            }
        };
        for workspace in &self.snapshot.navigator.workspaces {
            add(workspace);
        }
        for target in &self.snapshot.status.remote {
            if let Some(session) = &target.session {
                for workspace in &session.workspaces {
                    add(workspace);
                }
            }
        }
        if overflow {
            crate::diagnostic!(
                serde_json::json!({"component":"session_work","kind":"issue_draft.place_capacity","limit":DRAFT_PLACE_LIMIT})
            );
            self.set_error(
                "session_issue_draft.capacity",
                "The session checkout lookup exceeded its capacity",
                true,
            );
            return true;
        }
        let settings = self.ai_settings.clone().unwrap_or_default();
        let ai_usable = self.ai_active() && !self.usage_paused();
        let provider = self.own_node();
        self.session_issue_work.generation += 1;
        let generation = self.session_issue_work.generation;
        let cancel = CancelToken::new();
        self.session_issue_work.active = Some(cancel.clone());
        self.session_issue_work.ready = None;
        // A connection retry keeps the same form mounted, so edited title
        // and body remain the operator's until an explicit Create.
        let workspace_id = self
            .snapshot
            .issue_work
            .draft
            .as_ref()
            .filter(|draft| draft.source == source)
            .and_then(|draft| draft.workspace_id.clone());
        self.snapshot.issue_work.draft = Some(SessionIssueDraftSnapshot {
            source: source.clone(),
            phase: "working".into(),
            workspace_id,
            repository: None,
            title: String::new(),
            body: String::new(),
            message: None,
        });
        let requested = source.clone();
        let admission = cancel.clone();
        let spawned = self.spawn_issue_worker("session-issue-preview", move || {
            read_draft(&client, &requested, &places, &node, &provider, ai_usable.then_some(&settings), &cancel)
        }, move |runtime, result| {
            if runtime.session_issue_work.generation != generation { return false; }
            runtime.session_issue_work.active = None;
            if !runtime.ai_settings.as_ref().is_some_and(|settings| settings.enabled && settings.chosen && settings.agent_summary) { return runtime.cancel_session_issue_draft(); }
            if runtime.snapshot.issue_work.draft.as_ref().is_none_or(|slot| slot.source != source) { return false; }
            let invalid = admission.is_cancelled() || result.as_ref().is_ok_and(|draft| !runtime.session_issue_place_current(&source.session.device_id, &draft.place));
            let slot = runtime.snapshot.issue_work.draft.as_mut().expect("draft owner checked");
            if invalid { slot.phase = "failed".into(); slot.message = Some("issue_draft_repository_changed".into()); return true; }
            match result {
                Ok(draft) => {
                    slot.phase = "ready".into(); slot.workspace_id = Some(draft.place.workspace_id.clone());
                    slot.repository = draft.place.repository.clone(); slot.title = draft.title;
                    slot.body = draft.body; slot.message = draft.message;
                    runtime.session_issue_work.ready = Some(draft.place);
                }
                Err(code) => {
                    crate::diagnostic!(serde_json::json!({"component":"session_work","kind":"issue_draft.failed","generation":generation,"code":code}));
                    slot.phase = "failed".into(); slot.message = Some(if matches!(code.as_str(), "issue_draft_repository_unknown" | "issue_draft_repository_changed") {code} else {"issue_draft_unavailable".into()});
                }
            }
            true
        });
        if let Err(code) = spawned {
            self.session_issue_work.active = None;
            if let Some(slot) = &mut self.snapshot.issue_work.draft {
                slot.phase = "failed".into();
                slot.message = Some(code);
            }
        }
        true
    }

    pub(super) fn create_missed_issue(&mut self, payload: IssueCreatePayload) -> bool {
        if !self
            .ai_settings
            .as_ref()
            .is_some_and(|settings| settings.enabled && settings.chosen && settings.agent_summary)
        {
            return false;
        }
        let Some(source) = payload.missed else {
            return false;
        };
        let Some(place) = self
            .session_issue_work
            .ready
            .clone()
            .filter(|place| place.workspace_id == payload.workspace_id)
        else {
            self.set_error(
                "issue_create.preview_required",
                "Open the missed work's issue preview first",
                true,
            );
            return true;
        };
        if !self.session_issue_place_current(&source.session.device_id, &place) {
            self.session_issue_work.ready = None;
            self.set_error(
                "issue_create.repository_changed",
                "The issue preview's repository has changed. Open the preview again.",
                true,
            );
            return true;
        }
        if !self
            .snapshot
            .issue_work
            .draft
            .as_ref()
            .is_some_and(|slot| slot.source == source && slot.phase == "ready")
        {
            self.set_error(
                "issue_create.preview_changed",
                "The issue preview has changed",
                true,
            );
            return true;
        }
        if self
            .snapshot
            .issue_work
            .create
            .as_ref()
            .is_some_and(|slot| slot.phase == "working")
        {
            self.set_error(
                "issue_create.busy",
                "Another issue is still being created",
                true,
            );
            return true;
        }
        let id = self.next_issue_work_id();
        self.snapshot.issue_work.create = Some(crate::model::IssueCreateSnapshot {
            id,
            workspace_id: place.workspace_id.clone(),
            phase: "working".into(),
            task_key: None,
            message: None,
        });
        let Some(client) = self.links_work.client.clone() else {
            return self.settle_issue_create(id, Err("The session record is unavailable".into()));
        };
        let node = match self.node_link(&source.session.device_id) {
            Ok(node) => node,
            Err(_) => {
                return self
                    .settle_issue_create(id, Err("The session's device is not connected".into()));
            }
        };
        let device = source.session.device_id.clone();
        let project = place.project.clone();
        let observed_place = place.clone();
        let cancel = CancelToken::new();
        self.session_issue_work.create_active =
            Some((device.clone(), project.clone(), cancel.clone()));
        let admission = cancel.clone();
        let spawned = self.spawn_issue_worker("session-issue-create", move || {
            create_once(&client, &source, &node, &place, &payload.title, &payload.body, &cancel)
        }, move |runtime, result| {
            runtime.session_issue_work.create_active = None;
            if admission.is_cancelled() || !runtime.session_issue_place_current(&device, &observed_place) {
                return runtime.settle_issue_create(id, Err("The issue preview's repository changed. Reopen it to check the original attempt.".into()));
            }
            match result {
                Ok(issue) => {
                    let key = crate::tasks::github_key(&issue.reference);
                    let node = (device != runtime.node.as_str()).then_some(device.as_str());
                    runtime.remember_created_issue(node, &project, issue);
                    runtime.settle_issue_create(id, Ok(key))
                }
                Err(_) => {
                    crate::diagnostic!(serde_json::json!({"component":"session_work","kind":"issue_create.failed","id":id,"code":"issue_create_unconfirmed"}));
                    runtime.settle_issue_create(id, Err("The issue could not be confirmed. Retry to check the previous attempt before creating it.".into()))
                }
            }
        });
        if let Err(code) = spawned {
            self.session_issue_work.create_active = None;
            return self.settle_issue_create(id, Err(code));
        }
        true
    }
}

fn read_draft(
    client: &LinkClient,
    source: &MissedSource,
    places: &[Place],
    node: &Arc<dyn NodeLink>,
    provider: &Arc<dyn NodeLink>,
    settings: Option<&AiSettings>,
    cancel: &CancelToken,
) -> Result<Draft, String> {
    let LedgerAnswer::Record(record) = client.call(LedgerCall::Record(source.session.clone()))?
    else {
        return Err("issue_draft_record_invalid".into());
    };
    let missed = record
        .missed
        .iter()
        .find(|missed| missed.id == source.id && missed.status == MissedStatus::Open)
        .ok_or("issue_draft_missed_closed")?;
    let cwd = record
        .cwd
        .as_deref()
        .ok_or("issue_draft_checkout_missing")?;
    let mut place = places
        .iter()
        .filter(|place| hide_platform::path::wire_relative(&place.checkout, cwd).is_ok())
        .max_by_key(|place| place.checkout.len())
        .cloned()
        .or_else(|| {
            places
                .iter()
                .find(|place| {
                    record.project.as_deref() == Some(place.project.as_str())
                        || record.repository.as_deref().is_some_and(|repo| {
                            place
                                .repository
                                .as_deref()
                                .is_some_and(|known| known.eq_ignore_ascii_case(repo))
                        })
                })
                .cloned()
        })
        .ok_or("issue_draft_checkout_missing")?;
    let original = record
        .repository
        .as_deref()
        .ok_or("issue_draft_repository_unknown")?;
    // Provider failure may retain an editable draft, but never grants the
    // current checkout authority over this session's immutable original.
    place.repository = Some(original.into());
    let gh_place = place.target.as_ref().map_err(Clone::clone)?;
    let repository = crate::github::issue_repository(node.as_ref(), gh_place);
    if let Ok(repository) = &repository {
        if !original.eq_ignore_ascii_case(repository) {
            return Err("issue_draft_repository_changed".into());
        }
        place.repository = Some(repository.clone());
    }
    if cancel.is_cancelled() {
        return Err("cancelled".into());
    }
    let mut draft = Draft {
        place,
        title: missed.text.clone(),
        body: String::new(),
        message: repository
            .is_err()
            .then(|| "issue_draft_github_unavailable".into()),
    };
    let Some(settings) = settings else {
        draft
            .message
            .get_or_insert_with(|| "issue_draft_ai_unavailable".into());
        return Ok(draft);
    };
    let Some(witness) = missed.turn_witness.clone() else {
        draft
            .message
            .get_or_insert_with(|| "issue_draft_context_unavailable".into());
        return Ok(draft);
    };
    let request = hide_session::label_transcript::WorkTurnRequest {
        agent: hide_session::Agent::from_kind(&record.session.agent)
            .ok_or("issue_draft_agent_unsupported")?,
        session_id: record.session.session_id.clone(),
        path: record.path.clone(),
        cwd: cwd.into(),
        offset: missed.turn_offset,
        witness,
    };
    let context = crate::node_access::call_as::<hide_node_link::sessions::WorkContext>(
        node.as_ref(),
        Call::SessionWorkContext {
            request,
            checkout_path: draft.place.checkout.clone(),
        },
        std::time::Duration::from_secs(20),
    );
    match context {
        Ok(context) => match crate::ai::draft_session_issue(
            provider,
            settings,
            &source.id,
            &missed.text,
            &context,
            cancel,
        ) {
            Ok((title, body)) => {
                draft.title = title;
                draft.body = body;
            }
            Err(code) => {
                crate::diagnostic!(
                    serde_json::json!({"component":"session_work","kind":"issue_draft.fallback","code":code})
                );
                draft
                    .message
                    .get_or_insert_with(|| "issue_draft_ai_unavailable".into());
            }
        },
        Err(_) => {
            crate::diagnostic!(
                serde_json::json!({"component":"session_work","kind":"issue_draft.context_failed","code":"issue_draft_context_unavailable"})
            );
            draft
                .message
                .get_or_insert_with(|| "issue_draft_context_unavailable".into());
        }
    }
    if cancel.is_cancelled() {
        return Err("cancelled".into());
    }
    Ok(draft)
}

fn create_once(
    client: &LinkClient,
    source: &MissedSource,
    node: &Arc<dyn NodeLink>,
    place: &Place,
    title: &str,
    body: &str,
    cancel: &CancelToken,
) -> Result<crate::issues::IssueSnapshot, String> {
    let title = crate::local_issues::validated_title(title)?;
    let body = crate::local_issues::validated_body(body)?;
    if cancel.is_cancelled() {
        return Err("issue_repository_changed".into());
    }
    let gh_place = place.target.as_ref().map_err(Clone::clone)?;
    let repository = crate::github::issue_repository(node.as_ref(), gh_place)?;
    if place
        .repository
        .as_ref()
        .is_none_or(|known| !known.eq_ignore_ascii_case(&repository))
    {
        return Err("issue_repository_changed".into());
    }
    let LedgerAnswer::Attempt(attempt) = client.call(LedgerCall::ReserveIssue {
        source: source.clone(),
        repository: repository.clone(),
        title: title.clone(),
        body: body.clone(),
        at_unix_ms: unix_milliseconds(),
    })?
    else {
        return Err("issue_attempt_invalid".into());
    };
    let result = (|| {
        if cancel.is_cancelled() {
            return Err("issue_repository_changed".into());
        }
        if let (Some(number), Some(url)) = (attempt.number, &attempt.url) {
            let reference = crate::issues::IssueReference::parse(url, None)?;
            if u64::from(reference.number) != number {
                return Err("issue_attempt_invalid".into());
            }
            return Ok(crate::issues::IssueSnapshot {
                reference,
                title: attempt.title.clone(),
                url: url.clone(),
                state: "OPEN".into(),
                project_status: None,
                updated_at_unix_ms: None,
                created_at_unix_ms: None,
                closed_at_unix_ms: None,
                blocked_by: vec![],
                sub_issues: Default::default(),
                labels: vec![],
            });
        }
        if attempt.reconcile {
            if let Some(issue) = crate::github::reconcile_issue_attempt(
                node.as_ref(),
                gh_place,
                &attempt.repository,
                &attempt.title,
                attempt.started_at_unix_ms,
            )? {
                return Ok(issue);
            }
            client.call(LedgerCall::StartIssue {
                source: source.clone(),
                title: title.clone(),
                body: body.clone(),
                at_unix_ms: unix_milliseconds(),
            })?;
        }
        let approved = crate::github::Where::DeviceRepository {
            checkout: place.checkout.clone(),
            repository: attempt.repository.clone(),
        };
        if cancel.is_cancelled() {
            return Err("issue_repository_changed".into());
        }
        crate::github::create_issue(node.as_ref(), &approved, &title, &body)
            .map_err(|_| "issue_create_unconfirmed".to_owned())
    })();
    let issue = match result {
        Ok(issue) => issue,
        Err(error) => {
            client.call(LedgerCall::UnknownIssue(source.clone()))?;
            return Err(error);
        }
    };
    client.call(LedgerCall::FinishIssue {
        source: source.clone(),
        number: u64::from(issue.reference.number),
        url: issue.url.clone(),
    })?;
    Ok(issue)
}
