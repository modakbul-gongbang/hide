//! The explicit missed-work preview and create. Disk, session, provider and
//! GitHub calls run off Runtime's lock; the link worker owns the durable intent.

use super::*;
use crate::links::ledger::{LedgerAnswer, LedgerCall, MissedSource, MissedStatus};
use crate::links::worker::LinkClient;
use crate::model::SessionIssueDraftSnapshot;
use crate::node_access::NodeLink;
use hide_ai::{AiSettings, CancelToken};
use hide_node_link::protocol::Call;

#[derive(Clone)]
struct Place {
    workspace_id: String,
    project: String,
    checkout: String,
    repository: Option<String>,
}

#[derive(Default)]
pub(super) struct SessionIssueWork {
    generation: u64,
    /// Remains owned until the cancelled worker answers: repeated Cancel /
    /// Draft cannot create an unbounded set of threads or provider processes.
    active: Option<CancelToken>,
    ready: Option<Place>,
}

struct Draft {
    place: Place,
    title: String,
    body: String,
    message: Option<String>,
}

impl Runtime {
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
            .is_some_and(|settings| settings.agent_summary && self.ai_active())
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
        let mut add = |workspace: &WorkspaceSnapshot| {
            if workspace.device_id != source.session.device_id {
                return;
            }
            for checkout in &workspace.checkouts {
                places.push(Place {
                    workspace_id: workspace.id.clone(),
                    project: workspace.path.clone(),
                    checkout: checkout.path.clone(),
                    repository: workspace.home_issues.repository.clone(),
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
        let settings = self.ai_settings.clone().unwrap_or_default();
        let provider = self.own_node();
        self.session_issue_work.generation += 1;
        let generation = self.session_issue_work.generation;
        let cancel = CancelToken::new();
        self.session_issue_work.active = Some(cancel.clone());
        self.session_issue_work.ready = None;
        self.snapshot.issue_work.draft = Some(SessionIssueDraftSnapshot {
            source: source.clone(),
            phase: "working".into(),
            workspace_id: None,
            repository: None,
            title: String::new(),
            body: String::new(),
            message: None,
        });
        let requested = source.clone();
        let spawned = self.spawn_issue_worker("session-issue-preview", move || {
            read_draft(&client, &requested, &places, &node, &provider, &settings, &cancel)
        }, move |runtime, result| {
            if runtime.session_issue_work.generation != generation { return false; }
            runtime.session_issue_work.active = None;
            let Some(slot) = runtime.snapshot.issue_work.draft.as_mut().filter(|slot|slot.source == source) else { return false; };
            match result {
                Ok(draft) => {
                    slot.phase = "ready".into(); slot.workspace_id = Some(draft.place.workspace_id.clone());
                    slot.repository = draft.place.repository.clone(); slot.title = draft.title;
                    slot.body = draft.body; slot.message = draft.message;
                    runtime.session_issue_work.ready = Some(draft.place);
                }
                Err(code) => {
                    crate::diagnostic!(serde_json::json!({"component":"session_work","kind":"issue_draft.failed","generation":generation,"code":code}));
                    slot.phase = "failed".into(); slot.message = Some("issue_draft_unavailable".into());
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
        let spawned = self.spawn_issue_worker("session-issue-create", move || {
            create_once(&client, &source, &node, &place, &payload.title, &payload.body)
        }, move |runtime, result| {
            match result {
                Ok(issue) => {
                    let key = crate::tasks::github_key(&issue.reference);
                    let node = (device != runtime.node.as_str()).then_some(device.as_str());
                    runtime.remember_created_issue(node, &project, issue);
                    runtime.settle_issue_create(id, Ok(key))
                }
                Err(code) => {
                    crate::diagnostic!(serde_json::json!({"component":"session_work","kind":"issue_create.failed","id":id,"code":code}));
                    runtime.settle_issue_create(id, Err("The issue could not be confirmed. Retry to check the previous attempt before creating it.".into()))
                }
            }
        });
        if let Err(code) = spawned {
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
    settings: &AiSettings,
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
        .ok_or("issue_draft_checkout_missing")?;
    let gh_place = crate::github::Where::DeviceCheckout(place.checkout.clone());
    let repository = crate::github::issue_repository(node.as_ref(), &gh_place);
    if let Ok(repository) = &repository {
        place.repository = Some(repository.clone());
    }
    if cancel.is_cancelled() {
        return Err("cancelled".into());
    }
    let request = hide_session::label_transcript::WorkTurnRequest {
        agent: hide_session::Agent::from_kind(&record.session.agent)
            .ok_or("issue_draft_agent_unsupported")?,
        session_id: record.session.session_id.clone(),
        path: record.path.clone(),
        cwd: cwd.into(),
        offset: missed.turn_offset,
    };
    let context = crate::node_access::call_as::<hide_host::session_work::Context>(
        node.as_ref(),
        Call::SessionWorkContext {
            request,
            checkout_path: place.checkout.clone(),
        },
        std::time::Duration::from_secs(20),
    );
    let mut draft = Draft {
        place,
        title: missed.text.clone(),
        body: String::new(),
        message: None,
    };
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
                draft.message = Some("issue_draft_ai_unavailable".into());
            }
        },
        Err(error) => {
            crate::diagnostic!(
                serde_json::json!({"component":"session_work","kind":"issue_draft.context_failed","code":error.to_string()})
            );
            draft.message = Some("issue_draft_context_unavailable".into());
        }
    }
    if repository.is_err() {
        draft.message = Some("issue_draft_github_unavailable".into());
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
) -> Result<crate::issues::IssueSnapshot, String> {
    let title = crate::local_issues::validated_title(title)?;
    let body = crate::local_issues::validated_body(body)?;
    let gh_place = crate::github::Where::DeviceCheckout(place.checkout.clone());
    let repository = crate::github::issue_repository(node.as_ref(), &gh_place)?;
    if place
        .repository
        .as_ref()
        .is_some_and(|known| !known.eq_ignore_ascii_case(&repository))
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
                &gh_place,
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
        crate::github::create_issue(node.as_ref(), &gh_place, &title, &body)
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
