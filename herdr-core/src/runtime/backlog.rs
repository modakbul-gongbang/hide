//! The backlog's one add, its undo and an item's origin (PRD
//! overview-device-tasks D-10, D-13, D-21, D-34). A memo is made in the
//! store at once; a GitHub issue is a title-only `gh issue create` on a
//! worker, one at a time per device with `WAITING_LIMIT` waiting, through a
//! durable attempt in the link ledger, so a retry, even after a restart,
//! names the issue the first try made instead of making a second one.

use super::*;
use crate::links::ledger::{
    AttemptKey, BacklogIntent, LedgerAnswer, LedgerCall, MissedSource, PendingBacklog,
};
use crate::links::worker::LinkClient;
use crate::model::{BacklogFailure, BacklogItemSnapshot, BacklogPhase};
use crate::node_access::NodeLink;
use hide_node_link::protocol::Call;
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};

/// Items waiting behind the one a device is making (D-34).
pub(super) const WAITING_LIMIT: usize = 8;
/// Failed items kept for `다시 시도`; past it a new item is refused.
const FAILED_LIMIT: usize = 32;
/// Made items kept so the quick add and the missed line can name them.
const CREATED_KEPT: usize = 16;
/// The intent key the shell makes: short, plain, never a path.
const INTENT_LIMIT: usize = 64;

/// What a GitHub item needs to be made again on a retry.
#[derive(Clone)]
struct Job {
    workspace_id: String,
    device: String,
    node: Option<String>,
    project: String,
    title: String,
    missed: Option<MissedSource>,
}

#[derive(Default)]
pub(super) struct BacklogWork {
    jobs: HashMap<String, Job>,
    /// The device each running create is for.
    running: HashSet<String>,
    waiting: HashMap<String, VecDeque<String>>,
    undo_generation: u64,
    origin_generation: u64,
}

/// `backlog_add`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BacklogAddPayload {
    pub(super) intent: String,
    pub(super) workspace_id: String,
    pub(super) title: String,
    #[serde(default)]
    pub(super) missed: Option<MissedSource>,
}

/// `backlog_retry` and `backlog_dismiss`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BacklogIntentPayload {
    pub(super) intent: String,
}

/// `backlog_undo` and `backlog_origin`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BacklogItemPayload {
    pub(super) workspace_id: String,
    pub(super) task_key: String,
}

fn summaries_on(settings: Option<&hide_ai::AiSettings>) -> bool {
    settings.is_some_and(|settings| settings.enabled && settings.chosen && settings.agent_summary)
}

impl Runtime {
    fn backlog_row(&mut self, intent: &str) -> Option<&mut BacklogItemSnapshot> {
        self.snapshot
            .issue_work
            .backlog
            .iter_mut()
            .find(|row| row.intent == intent)
    }

    fn set_backlog_row(&mut self, row: BacklogItemSnapshot) {
        let rows = &mut self.snapshot.issue_work.backlog;
        rows.retain(|known| known.intent != row.intent);
        rows.push(row);
        let created: Vec<_> = rows
            .iter()
            .filter(|row| row.phase == BacklogPhase::Created)
            .map(|row| row.intent.clone())
            .collect();
        let excess = created.len().saturating_sub(CREATED_KEPT);
        for intent in created.into_iter().take(excess) {
            rows.retain(|row| row.intent != intent);
            self.backlog_work.jobs.remove(&intent);
        }
    }

    fn refuse_backlog(&mut self, code: &str, message: &str) -> bool {
        crate::diagnostic!(serde_json::json!({"component":"backlog","kind":code}));
        self.set_error(code, message, false);
        true
    }

    /// `backlog_add`: one item in the project's backlog, with no form.
    pub(super) fn add_backlog(&mut self, payload: BacklogAddPayload) -> bool {
        if payload.intent.is_empty()
            || payload.intent.len() > INTENT_LIMIT
            || !payload
                .intent
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return self.refuse_backlog("backlog.intent_invalid", "The item has no valid key");
        }
        if self
            .snapshot
            .issue_work
            .backlog
            .iter()
            .any(|row| row.intent == payload.intent)
        {
            // A repeated send of the same item: the row already answers it.
            return false;
        }
        let title = match crate::local_issues::validated_title(&payload.title) {
            Ok(title) => title,
            Err(message) => return self.refuse_backlog("backlog.title_invalid", &message),
        };
        // A device the core dials keeps its issues there, so it has no
        // backlog here and is not found (D-33).
        let Some((node, workspace)) = self.issue_project(&payload.workspace_id) else {
            return self.refuse_backlog(
                "backlog.unknown_project",
                "The backlog needs a project whose issues Hide keeps",
            );
        };
        if let Some(missed) = &payload.missed {
            if !summaries_on(self.ai_settings.as_ref()) {
                return false;
            }
            if missed.session.device_id != workspace.device_id {
                return self.refuse_backlog(
                    "backlog.missed_elsewhere",
                    "Missed work goes to a project on its session's device",
                );
            }
        }
        crate::diagnostic!(serde_json::json!({
            "component": "backlog", "kind": "backlog.add",
            "intent": payload.intent, "project": workspace.path, "missed": payload.missed.is_some(),
        }));
        let job = Job {
            workspace_id: workspace.id.clone(),
            device: workspace.device_id.clone(),
            node,
            project: workspace.path.clone(),
            title,
            missed: payload.missed,
        };
        match self.project_source_kind(&workspace) {
            crate::tasks::SourceKind::Local => self.add_memo(payload.intent, job),
            crate::tasks::SourceKind::Github => self.enqueue_issue(payload.intent, job),
        }
    }

    fn add_memo(&mut self, intent: String, job: Job) -> bool {
        let mut row = BacklogItemSnapshot {
            intent,
            workspace_id: job.workspace_id.clone(),
            title: job.title.clone(),
            phase: BacklogPhase::Created,
            task_key: None,
            failure: None,
            missed: job.missed.clone(),
            at_unix_ms: unix_milliseconds(),
        };
        match self.create_local_issue(job.node.as_deref(), &job.project, &job.title, "") {
            Ok(number) => {
                let key = crate::tasks::local_key(&job.project, number);
                row.task_key = Some(key.clone());
                if let Some(source) = job.missed {
                    self.mark_missed_memo(row.intent.clone(), source, key);
                }
            }
            Err(reason) => {
                crate::diagnostic!(
                    serde_json::json!({"component":"backlog","kind":"backlog.memo_failed","reason":reason})
                );
                row.phase = BacklogPhase::Failed;
                row.failure = Some(BacklogFailure::Refused);
            }
        }
        self.set_backlog_row(row);
        true
    }

    /// The missed line that made a memo turns `· 백로그 L-N`.
    fn mark_missed_memo(&mut self, intent: String, source: MissedSource, key: String) {
        let Some(client) = self.links_work.client.clone() else {
            return;
        };
        let spawned = self.spawn_issue_worker(
            "backlog-memo",
            move || {
                client.call(LedgerCall::BacklogLocal {
                    source,
                    backlog_key: key,
                })
            },
            move |runtime, result| {
                if let Err(code) = result {
                    crate::diagnostic!(serde_json::json!({"component":"backlog","kind":"backlog.missed_mark_failed","intent":intent,"code":code}));
                }
                runtime.feed_links();
                true
            },
        );
        if let Err(code) = spawned {
            crate::diagnostic!(
                serde_json::json!({"component":"backlog","kind":"backlog.missed_mark_failed","code":code})
            );
        }
    }

    fn enqueue_issue(&mut self, intent: String, job: Job) -> bool {
        let failed = self
            .snapshot
            .issue_work
            .backlog
            .iter()
            .filter(|row| row.phase == BacklogPhase::Failed)
            .count();
        let device = job.device.clone();
        let waiting = self
            .backlog_work
            .waiting
            .get(&device)
            .map_or(0, VecDeque::len);
        let busy = self.backlog_work.running.contains(&device);
        let mut row = BacklogItemSnapshot {
            intent: intent.clone(),
            workspace_id: job.workspace_id.clone(),
            title: job.title.clone(),
            phase: BacklogPhase::Waiting,
            task_key: None,
            failure: None,
            missed: job.missed.clone(),
            at_unix_ms: unix_milliseconds(),
        };
        if (busy && waiting >= WAITING_LIMIT) || failed >= FAILED_LIMIT {
            crate::diagnostic!(
                serde_json::json!({"component":"backlog","kind":"backlog.queue_full","intent":intent,"device":device,"waiting":waiting,"failed":failed})
            );
            row.phase = BacklogPhase::Failed;
            row.failure = Some(BacklogFailure::QueueFull);
            self.backlog_work.jobs.insert(intent, job);
            self.set_backlog_row(row);
            return true;
        }
        self.backlog_work.jobs.insert(intent.clone(), job);
        self.set_backlog_row(row);
        if busy {
            self.backlog_work
                .waiting
                .entry(device)
                .or_default()
                .push_back(intent);
        } else {
            self.start_issue(intent);
        }
        true
    }

    /// `backlog_retry`: a failed item goes back in line, under the same
    /// intent, so the attempt it already made is found rather than repeated.
    pub(super) fn retry_backlog(&mut self, payload: BacklogIntentPayload) -> bool {
        let Some(row) = self.backlog_row(&payload.intent) else {
            return false;
        };
        if row.phase != BacklogPhase::Failed {
            return false;
        }
        let Some(job) = self.backlog_work.jobs.get(&payload.intent).cloned() else {
            return false;
        };
        self.snapshot
            .issue_work
            .backlog
            .retain(|row| row.intent != payload.intent);
        self.backlog_work.jobs.remove(&payload.intent);
        self.enqueue_issue(payload.intent, job)
    }

    /// `backlog_dismiss` (`지우기`): a failed item is put away; its attempt
    /// stops being offered after a restart.
    pub(super) fn dismiss_backlog(&mut self, payload: BacklogIntentPayload) -> bool {
        let Some(index) = self
            .snapshot
            .issue_work
            .backlog
            .iter()
            .position(|row| row.intent == payload.intent && row.phase == BacklogPhase::Failed)
        else {
            return false;
        };
        let row = self.snapshot.issue_work.backlog.remove(index);
        let job = self.backlog_work.jobs.remove(&row.intent);
        if job.is_some_and(|job| job.missed.is_none())
            && let Some(client) = self.links_work.client.clone()
        {
            let intent = row.intent.clone();
            let _ = self.spawn_issue_worker(
                "backlog-dismiss",
                move || client.call(LedgerCall::DismissBacklog { intent }),
                |_, result| {
                    if let Err(code) = result {
                        crate::diagnostic!(serde_json::json!({"component":"backlog","kind":"backlog.dismiss_failed","code":code}));
                    }
                    false
                },
            );
        }
        true
    }

    fn start_issue(&mut self, intent: String) {
        let Some(job) = self.backlog_work.jobs.get(&intent).cloned() else {
            return;
        };
        let fail = |runtime: &mut Runtime, failure: BacklogFailure| {
            if let Some(row) = runtime.backlog_row(&intent) {
                row.phase = BacklogPhase::Failed;
                row.failure = Some(failure);
            }
        };
        let Some(client) = self.links_work.client.clone() else {
            crate::diagnostic!(
                serde_json::json!({"component":"backlog","kind":"backlog.ledger_unavailable","intent":intent})
            );
            return fail(self, BacklogFailure::Refused);
        };
        let place = match self.github_place(job.node.as_deref(), &job.project) {
            Ok(place) => place,
            Err(reason) => {
                crate::diagnostic!(
                    serde_json::json!({"component":"backlog","kind":"backlog.place_unknown","intent":intent,"reason":reason})
                );
                return fail(self, BacklogFailure::NoGithubRemote);
            }
        };
        if let Some(row) = self.backlog_row(&intent) {
            row.phase = BacklogPhase::Creating;
            row.failure = None;
        }
        self.backlog_work.running.insert(job.device.clone());
        let key = match &job.missed {
            Some(source) => AttemptKey::Missed(source.clone()),
            None => AttemptKey::Intent(BacklogIntent {
                intent: intent.clone(),
                device: job.device.clone(),
                project: job.project.clone(),
                workspace_id: job.workspace_id.clone(),
            }),
        };
        let own = self.own_node();
        let title = job.title.clone();
        let intent_for_refusal = intent.clone();
        let spawned = self.spawn_issue_worker(
            "backlog-issue",
            move || create_once(&client, &key, own.as_ref(), &place, &title),
            move |runtime, result| runtime.ingest_backlog_issue(&intent, result),
        );
        if let Err(code) = spawned {
            crate::diagnostic!(
                serde_json::json!({"component":"backlog","kind":"backlog.worker_unavailable","code":code})
            );
            self.backlog_work.running.remove(&job.device);
            if let Some(row) = self.backlog_row(&intent_for_refusal) {
                row.phase = BacklogPhase::Failed;
                row.failure = Some(BacklogFailure::Refused);
            }
        }
    }

    fn ingest_backlog_issue(
        &mut self,
        intent: &str,
        result: Result<crate::issues::IssueSnapshot, BacklogFailure>,
    ) -> bool {
        let Some(job) = self.backlog_work.jobs.get(intent).cloned() else {
            return false;
        };
        self.backlog_work.running.remove(&job.device);
        match result {
            Ok(issue) => {
                let key = crate::tasks::github_key(&issue.reference);
                self.remember_created_issue(job.node.as_deref(), &job.project, issue);
                if let Some(row) = self.backlog_row(intent) {
                    row.phase = BacklogPhase::Created;
                    row.task_key = Some(key);
                }
                if job.missed.is_some() {
                    self.feed_links();
                }
            }
            Err(failure) => {
                crate::diagnostic!(
                    serde_json::json!({"component":"backlog","kind":"backlog.issue_failed","intent":intent,"failure":failure})
                );
                if let Some(row) = self.backlog_row(intent) {
                    row.phase = BacklogPhase::Failed;
                    row.failure = Some(failure);
                }
            }
        }
        let next = self
            .backlog_work
            .waiting
            .get_mut(&job.device)
            .and_then(VecDeque::pop_front);
        if let Some(next) = next {
            self.start_issue(next);
        }
        true
    }

    /// Typed items whose create was never confirmed come back after a
    /// restart as failed rows, so `다시 시도` still finds what they made.
    pub(super) fn restore_pending_backlog(&mut self) {
        let Some(client) = self.links_work.client.clone() else {
            return;
        };
        let spawned = self.spawn_issue_worker(
            "backlog-restore",
            move || client.call(LedgerCall::PendingBacklog),
            |runtime, answer| match answer {
                Ok(LedgerAnswer::Pending(rows)) => runtime.ingest_pending_backlog(rows),
                Ok(_) => false,
                Err(code) => {
                    crate::diagnostic!(serde_json::json!({"component":"backlog","kind":"backlog.restore_failed","code":code}));
                    false
                }
            },
        );
        if let Err(code) = spawned {
            crate::diagnostic!(
                serde_json::json!({"component":"backlog","kind":"backlog.restore_failed","code":code})
            );
        }
    }

    fn ingest_pending_backlog(&mut self, rows: Vec<PendingBacklog>) -> bool {
        let mut changed = false;
        for pending in rows.into_iter().take(FAILED_LIMIT) {
            let intent = pending.intent.intent.clone();
            if self.backlog_row(&intent).is_some() {
                continue;
            }
            let node = (pending.intent.device != self.node.as_str())
                .then(|| pending.intent.device.clone());
            self.backlog_work.jobs.insert(
                intent.clone(),
                Job {
                    workspace_id: pending.intent.workspace_id.clone(),
                    device: pending.intent.device.clone(),
                    node,
                    project: pending.intent.project.clone(),
                    title: pending.title.clone(),
                    missed: None,
                },
            );
            self.snapshot.issue_work.backlog.push(BacklogItemSnapshot {
                intent,
                workspace_id: pending.intent.workspace_id,
                title: pending.title,
                phase: BacklogPhase::Failed,
                task_key: None,
                failure: Some(BacklogFailure::Unconfirmed),
                missed: None,
                at_unix_ms: pending.started_at_unix_ms,
            });
            changed = true;
        }
        changed
    }

    /// `backlog_undo` (`되돌리기`): a memo is deleted and a GitHub issue
    /// closed; the missed line it came from opens again (B32).
    pub(super) fn undo_backlog(&mut self, payload: BacklogItemPayload) -> bool {
        let Some((node, workspace)) = self.issue_project(&payload.workspace_id) else {
            return self
                .refuse_backlog("backlog.unknown_project", "The item's project is not known");
        };
        let started = workspace
            .checkouts
            .iter()
            .any(|checkout| checkout.task_key.as_deref() == Some(payload.task_key.as_str()));
        if started {
            return self.refuse_backlog("backlog.undo_started", "Started work cannot be undone");
        }
        self.backlog_work.undo_generation += 1;
        let generation = self.backlog_work.undo_generation;
        let task_key = payload.task_key.clone();
        let mut snapshot = crate::model::BacklogUndoSnapshot {
            task_key: task_key.clone(),
            phase: "working".into(),
            failure: None,
        };
        let client = self.links_work.client.clone();
        if let Some((path, number)) = crate::tasks::parse_local_key(&task_key) {
            if path != workspace.path {
                return self
                    .refuse_backlog("backlog.undo_elsewhere", "The memo is not this project's");
            }
            let deleted = match self.local_issues.as_mut() {
                Ok(store) => store.delete(node.as_deref(), path, number),
                Err(reason) => Err(reason.clone()),
            };
            match deleted {
                Ok(()) => {
                    self.persist_local_issues();
                    self.refresh_issue_rows(node.as_deref());
                    snapshot.phase = "done".into();
                    self.release_backlog(client, task_key);
                }
                Err(reason) => {
                    crate::diagnostic!(
                        serde_json::json!({"component":"backlog","kind":"backlog.undo_failed","reason":reason})
                    );
                    snapshot.phase = "failed".into();
                    snapshot.failure = Some(BacklogFailure::Refused);
                }
            }
            self.snapshot.issue_work.undo = Some(snapshot);
            return true;
        }
        let Some(reference) = crate::tasks::parse_github_key(&task_key) else {
            return self
                .refuse_backlog("backlog.undo_invalid", "The item is not a memo or an issue");
        };
        let place = match self.github_place(node.as_deref(), &workspace.path) {
            Ok(place) => place,
            Err(_) => {
                snapshot.phase = "failed".into();
                snapshot.failure = Some(BacklogFailure::NoGithubRemote);
                self.snapshot.issue_work.undo = Some(snapshot);
                return true;
            }
        };
        self.snapshot.issue_work.undo = Some(snapshot);
        let own = self.own_node();
        let project = workspace.path.clone();
        let spawned = self.spawn_issue_worker(
            "backlog-undo",
            move || {
                crate::github::close_issue(own.as_ref(), &place, &reference.repository, reference.number)
                    .map(|()| reference)
                    .map_err(|failure| BacklogFailure::from(failure.category))
            },
            move |runtime, result| {
                if runtime.backlog_work.undo_generation != generation {
                    return false;
                }
                let Some(slot) = runtime.snapshot.issue_work.undo.as_mut() else {
                    return false;
                };
                match result {
                    Ok(reference) => {
                        slot.phase = "done".into();
                        runtime.forget_created_issue(node.as_deref(), &project, &reference);
                        let client = runtime.links_work.client.clone();
                        runtime.release_backlog(client, task_key);
                    }
                    Err(failure) => {
                        crate::diagnostic!(serde_json::json!({"component":"backlog","kind":"backlog.close_failed","failure":failure}));
                        slot.phase = "failed".into();
                        slot.failure = Some(failure);
                    }
                }
                true
            },
        );
        if let Err(code) = spawned {
            crate::diagnostic!(
                serde_json::json!({"component":"backlog","kind":"backlog.worker_unavailable","code":code})
            );
            if let Some(slot) = self.snapshot.issue_work.undo.as_mut() {
                slot.phase = "failed".into();
                slot.failure = Some(BacklogFailure::Refused);
            }
        }
        true
    }

    fn release_backlog(&mut self, client: Option<LinkClient>, backlog_key: String) {
        self.snapshot
            .issue_work
            .backlog
            .retain(|row| row.task_key.as_deref() != Some(backlog_key.as_str()));
        let Some(client) = client else {
            return;
        };
        let spawned = self.spawn_issue_worker(
            "backlog-release",
            move || client.call(LedgerCall::ReleaseBacklog { backlog_key }),
            |runtime, result| {
                if let Err(code) = result {
                    crate::diagnostic!(serde_json::json!({"component":"backlog","kind":"backlog.release_failed","code":code}));
                }
                runtime.feed_links();
                true
            },
        );
        if let Err(code) = spawned {
            crate::diagnostic!(
                serde_json::json!({"component":"backlog","kind":"backlog.release_failed","code":code})
            );
        }
    }

    /// `backlog_origin`: where an item came from, for its panel's `어디서
    /// 왔나` and the Start prompt (B31, B33). The turn's words are read on the
    /// session's device and stay in this app.
    pub(super) fn read_backlog_origin(&mut self, payload: BacklogItemPayload) -> bool {
        self.backlog_work.origin_generation += 1;
        let generation = self.backlog_work.origin_generation;
        let mut slot = crate::model::BacklogOriginSnapshot {
            task_key: payload.task_key.clone(),
            phase: "reading".into(),
            session: None,
            session_title: None,
            missed_text: None,
            task_name: None,
            request: None,
            answer: None,
        };
        let Some(client) = self.links_work.client.clone() else {
            slot.phase = "failed".into();
            self.snapshot.issue_work.origin = Some(slot);
            return true;
        };
        self.snapshot.issue_work.origin = Some(slot);
        let nodes: HashMap<String, Arc<dyn NodeLink>> = self
            .snapshot
            .navigator
            .devices
            .iter()
            .map(|device| device.id.clone())
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|id| self.node_link(&id).ok().map(|node| (id, node)))
            .collect();
        let task_key = payload.task_key;
        let spawned = self.spawn_issue_worker(
            "backlog-origin",
            move || read_origin(&client, &nodes, &task_key),
            move |runtime, result| {
                if runtime.backlog_work.origin_generation != generation {
                    return false;
                }
                let Some(slot) = runtime.snapshot.issue_work.origin.as_mut() else {
                    return false;
                };
                match result {
                    Ok(read) => *slot = read,
                    Err(code) => {
                        crate::diagnostic!(serde_json::json!({"component":"backlog","kind":"backlog.origin_failed","code":code}));
                        slot.phase = "failed".into();
                    }
                }
                true
            },
        );
        if let Err(code) = spawned
            && let Some(slot) = self.snapshot.issue_work.origin.as_mut()
        {
            crate::diagnostic!(
                serde_json::json!({"component":"backlog","kind":"backlog.worker_unavailable","code":code})
            );
            slot.phase = "failed".into();
        }
        true
    }

    /// Hide AI turned off: a missed line can no longer be put in the backlog.
    pub(super) fn drop_missed_backlog(&mut self) {
        let waiting: Vec<_> = self
            .snapshot
            .issue_work
            .backlog
            .iter()
            .filter(|row| row.missed.is_some() && row.phase == BacklogPhase::Waiting)
            .map(|row| row.intent.clone())
            .collect();
        for intent in waiting {
            self.snapshot
                .issue_work
                .backlog
                .retain(|row| row.intent != intent);
            self.backlog_work.jobs.remove(&intent);
            for queue in self.backlog_work.waiting.values_mut() {
                queue.retain(|queued| *queued != intent);
            }
        }
    }
}

fn failure_of(failure: crate::github::CreateFailure) -> BacklogFailure {
    match failure {
        crate::github::CreateFailure::Gh(failure) => failure.category.into(),
        crate::github::CreateFailure::Unreadable(_) => BacklogFailure::Unconfirmed,
    }
}

/// One title-only issue under a durable attempt: an attempt that already
/// names its issue returns it, one left uncertain is looked for first, and
/// only then is `gh issue create` run.
fn create_once(
    client: &LinkClient,
    key: &AttemptKey,
    node: &dyn NodeLink,
    place: &crate::github::Where,
    title: &str,
) -> Result<crate::issues::IssueSnapshot, BacklogFailure> {
    let repository = crate::github::issue_repository(node, place).map_err(|code| {
        crate::diagnostic!(serde_json::json!({"component":"backlog","kind":"backlog.repository_unread","code":code}));
        BacklogFailure::NetworkOrRateLimit
    })?;
    let ledger = |call: LedgerCall| {
        client.call(call).map_err(|code| {
            crate::diagnostic!(serde_json::json!({"component":"backlog","kind":"backlog.ledger_refused","code":code}));
            BacklogFailure::Refused
        })
    };
    let LedgerAnswer::Attempt(attempt) = ledger(LedgerCall::ReserveIssue {
        key: key.clone(),
        repository: repository.clone(),
        title: title.to_owned(),
        body: String::new(),
        at_unix_ms: unix_milliseconds(),
    })?
    else {
        return Err(BacklogFailure::Refused);
    };
    if let (Some(number), Some(url)) = (attempt.number, &attempt.url) {
        let reference = crate::issues::IssueReference::parse(url, None)
            .map_err(|_| BacklogFailure::Unconfirmed)?;
        if u64::from(reference.number) != number {
            return Err(BacklogFailure::Unconfirmed);
        }
        return Ok(issue_named(reference, attempt.title.clone(), url.clone()));
    }
    let made = (|| {
        if attempt.reconcile {
            match crate::github::reconcile_issue_attempt(
                node,
                place,
                &attempt.repository,
                &attempt.title,
                attempt.started_at_unix_ms,
            ) {
                Ok(Some(issue)) => return Ok(issue),
                Ok(None) => {}
                Err(code) => {
                    crate::diagnostic!(
                        serde_json::json!({"component":"backlog","kind":"backlog.reconcile_failed","code":code})
                    );
                    return Err(BacklogFailure::Unconfirmed);
                }
            }
            ledger(LedgerCall::StartIssue {
                key: key.clone(),
                title: title.to_owned(),
                body: String::new(),
                at_unix_ms: unix_milliseconds(),
            })?;
        }
        let approved = crate::github::Where::Repository(attempt.repository.clone());
        let place = match place {
            crate::github::Where::Checkout(_) => place,
            _ => &approved,
        };
        crate::github::create_issue_failing(node, place, title, "").map_err(failure_of)
    })();
    let issue = match made {
        Ok(issue) => issue,
        Err(failure) => {
            ledger(LedgerCall::UnknownIssue(key.clone()))?;
            return Err(failure);
        }
    };
    ledger(LedgerCall::FinishIssue {
        key: key.clone(),
        number: u64::from(issue.reference.number),
        url: issue.url.clone(),
    })?;
    Ok(issue)
}

fn issue_named(
    reference: crate::issues::IssueReference,
    title: String,
    url: String,
) -> crate::issues::IssueSnapshot {
    crate::issues::IssueSnapshot {
        reference,
        title,
        url,
        state: "OPEN".into(),
        project_status: None,
        updated_at_unix_ms: None,
        created_at_unix_ms: None,
        closed_at_unix_ms: None,
        blocked_by: vec![],
        sub_issues: Default::default(),
        labels: vec![],
    }
}

fn read_origin(
    client: &LinkClient,
    nodes: &HashMap<String, Arc<dyn NodeLink>>,
    task_key: &str,
) -> Result<crate::model::BacklogOriginSnapshot, String> {
    let mut slot = crate::model::BacklogOriginSnapshot {
        task_key: task_key.to_owned(),
        phase: "ready".into(),
        session: None,
        session_title: None,
        missed_text: None,
        task_name: None,
        request: None,
        answer: None,
    };
    let LedgerAnswer::Origin(origin) = client.call(LedgerCall::BacklogOrigin {
        backlog_key: task_key.to_owned(),
    })?
    else {
        return Err("backlog_origin_invalid".into());
    };
    let Some(origin) = origin else {
        return Ok(slot);
    };
    slot.session = Some(origin.session.clone());
    slot.missed_text = Some(origin.missed.text.clone());
    let LedgerAnswer::Record(record) = client.call(LedgerCall::Record(origin.session.clone()))?
    else {
        return Err("backlog_origin_invalid".into());
    };
    slot.session_title = Some(
        record
            .manual_title
            .clone()
            .unwrap_or_else(|| record.title.clone()),
    );
    let request = record
        .requests
        .iter()
        .find(|request| request.offset == origin.missed.turn_offset);
    slot.task_name = request
        .and_then(|request| request.task_ids.first())
        .and_then(|id| record.tasks.iter().find(|task| &task.id == id))
        .map(|task| task.name.clone());
    slot.request = request.map(|request| request.text.clone());
    // The answer is the session's own words, read where the session runs.
    let (Some(witness), Some(cwd), Some(node)) = (
        origin.missed.turn_witness.clone(),
        record.cwd.clone(),
        nodes.get(&origin.session.device_id),
    ) else {
        return Ok(slot);
    };
    let Some(agent) = hide_session::Agent::from_kind(&record.session.agent) else {
        return Ok(slot);
    };
    let turn = crate::node_access::call_as::<hide_session::label_transcript::WorkTurn>(
        node.as_ref(),
        Call::SessionWorkTurn {
            request: hide_session::label_transcript::WorkTurnRequest {
                agent,
                session_id: record.session.session_id.clone(),
                path: record.path.clone(),
                cwd,
                offset: origin.missed.turn_offset,
                witness,
            },
        },
        std::time::Duration::from_secs(20),
    );
    match turn {
        Ok(turn) => {
            slot.request = Some(turn.request);
            slot.answer = Some(turn.assistant);
        }
        Err(error) => {
            crate::diagnostic!(
                serde_json::json!({"component":"backlog","kind":"backlog.origin_turn_unread","error":error.to_string()})
            );
        }
    }
    Ok(slot)
}
