//! Worktree actions cross the socket boundary before publishing removal readiness.
use super::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};

const CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);
const CONFIRM_POLL: Duration = Duration::from_millis(100);
/// A host's Git checks answer in seconds; a removal deletes a whole folder.
const HOST_CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const HOST_REMOVE_TIMEOUT: Duration = Duration::from_secs(120);

/// Where a worktree task runs: the Herdr that creates or closes the
/// checkout's panes and the file host that answers its Git checks and runs
/// its removal, both of the device that holds the repository (PRD S5.5
/// B27-B29). This machine is the in-process host and its own Herdr.
#[derive(Clone)]
pub struct WorktreeTarget {
    connector: Arc<dyn ApiConnector>,
    host: Arc<dyn crate::host_access::HostChannel>,
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    /// This machine: a purpose is mirrored into the branch description, and
    /// a provider missing from this PATH is refused before Herdr is asked.
    local: bool,
}

impl WorktreeTarget {
    pub(crate) fn local(
        context: &LiveContext,
        host: Arc<dyn crate::host_access::HostChannel>,
    ) -> Self {
        Self {
            connector: Arc::clone(&context.api_connector),
            host,
            runtime: context.runtime.clone(),
            notifier: context.notifier.clone(),
            local: true,
        }
    }

    pub(crate) fn device(
        context: &RemoteControlContext,
        host: Arc<dyn crate::host_access::HostChannel>,
    ) -> Self {
        Self {
            connector: Arc::clone(&context.api_connector),
            host,
            runtime: context.runtime.clone(),
            notifier: context.notifier.clone(),
            local: false,
        }
    }
}

pub fn spawn_worktree_close(
    target: WorktreeTarget,
    id: u64,
    checkout_path: String,
    pane_ids: Vec<String>,
) -> Result<(), String> {
    let context = target.clone();
    thread::Builder::new()
        .name("herdr-core-worktree-close".into())
        .spawn(move || {
            let result = close_checkout_panes(
                context.connector.as_ref(),
                std::slice::from_ref(&checkout_path),
                &pane_ids,
                CONFIRM_TIMEOUT,
            );
            // This thread is the removal's only executor: it closes the panes,
            // takes the runtime's confirmation, and runs Git itself. No shell
            // reports a completion, so a client cannot claim one.
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let confirmed = match runtime.lock() {
                Ok(mut guard) => {
                    guard.ingest_worktree_close_result(id, &pane_ids, result);
                    guard.confirmed_worktree_removal(id)
                }
                Err(error) => {
                    trace(
                        &checkout_path,
                        &pane_ids,
                        "publish_failed",
                        Some(&error.to_string()),
                    );
                    return;
                }
            };
            context.notifier.notify();
            let Some(request) = confirmed else {
                return;
            };
            trace(&checkout_path, &pane_ids, "remove_started", None);
            let outcome = remove_on_host(context.host.as_ref(), request);
            trace(
                &checkout_path,
                &pane_ids,
                if outcome.is_ok() {
                    "remove_finished"
                } else {
                    "remove_failed"
                },
                outcome.as_ref().err().map(String::as_str),
            );
            match runtime.lock() {
                Ok(mut guard) => {
                    guard.ingest_worktree_removal_result(id, outcome);
                }
                Err(error) => {
                    trace(
                        &checkout_path,
                        &pane_ids,
                        "publish_failed",
                        Some(&error.to_string()),
                    );
                    return;
                }
            }
            context.notifier.notify();
        })
        .map(|_| ())
        .map_err(|error| format!("worktree close worker could not be started: {error}"))
}

/// Runs the confirmed removal on the repository's own host. An answer that
/// never came leaves the removal's effect unknown, which is not a success.
fn remove_on_host(
    host: &dyn crate::host_access::HostChannel,
    removal: hide_host::worktrees::ConfirmedRemoval,
) -> Result<String, String> {
    let path = removal.checkout_path.clone();
    match crate::host_access::call_as::<hide_host::worktrees::RemovalOutcome>(
        host,
        hide_host::protocol::Call::WorktreeRemove { removal },
        HOST_REMOVE_TIMEOUT,
    ) {
        Ok(outcome) if outcome.removed => Ok(outcome.message),
        Ok(outcome) => Err(outcome.message),
        Err(crate::host_access::HostCallError::Unknown(reason)) => Err(format!(
            "The removal of {path} was sent but its result is unknown ({reason}). Review the worktree again before retrying."
        )),
        Err(error) => Err(format!(
            "Worktree removal stopped: {error}. The worktree remains; panes already closed stay closed."
        )),
    }
}

/// Asks the repository's host whether `branch` may be created; nothing is
/// created by the question.
fn check_new_branch(
    host: &dyn crate::host_access::HostChannel,
    repository_root: &str,
    branch: &str,
) -> Result<(), String> {
    crate::host_access::call_as::<()>(
        host,
        hide_host::protocol::Call::BranchCheck {
            path: repository_root.to_owned(),
            branch: branch.to_owned(),
        },
        HOST_CHECK_TIMEOUT,
    )
    .map_err(|error| error.to_string())
}

/// The real path of an existing directory on the repository's host, `None`
/// when there is none there. A host that did not answer is an error, never a
/// missing folder: a real worktree is not rolled back for a helper hiccup.
fn host_directory(
    host: &dyn crate::host_access::HostChannel,
    path: &str,
) -> Result<Option<String>, String> {
    crate::host_access::call_as::<Option<String>>(
        host,
        hide_host::protocol::Call::Directory {
            path: path.to_owned(),
        },
        HOST_CHECK_TIMEOUT,
    )
    .map_err(|error| error.to_string())
}

/// `Remove project…`: closes every pane in the project's checkouts and waits
/// for Herdr to confirm, the same handshake a worktree deletion uses, then
/// hands the answer to the runtime, which alone removes the registration.
pub fn spawn_workspace_close(
    context: WorktreeTarget,
    workspace_id: String,
    checkout_paths: Vec<String>,
    pane_ids: Vec<String>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-workspace-close".into())
        .spawn(move || {
            let result = close_checkout_panes(
                context.connector.as_ref(),
                &checkout_paths,
                &pane_ids,
                CONFIRM_TIMEOUT,
            );
            if let Some(runtime) = context.runtime.upgrade() {
                match runtime.lock() {
                    Ok(mut guard) => {
                        guard.ingest_workspace_close_result(&workspace_id, result);
                    }
                    Err(error) => {
                        trace(
                            &checkout_paths.join(","),
                            &pane_ids,
                            "publish_failed",
                            Some(&error.to_string()),
                        );
                        return;
                    }
                }
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("workspace close worker could not be started: {error}"))
}

pub fn spawn_worktree_open(
    context: LiveContext,
    checkout_path: String,
    repository_root: String,
    pane_id: Option<String>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-worktree-open".into())
        .spawn(move || {
            let result = open_worktree(
                context.api_connector.as_ref(),
                &checkout_path,
                &repository_root,
                pane_id.as_deref(),
            );
            if let Some(runtime) = context.runtime.upgrade() {
                match runtime.lock() {
                    Ok(mut guard) => {
                        guard.ingest_worktree_open_result(checkout_path.clone(), result);
                    }
                    Err(error) => {
                        trace(
                            &checkout_path,
                            &[],
                            "publish_failed",
                            Some(&error.to_string()),
                        );
                        return;
                    }
                }
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("worktree open worker could not be started: {error}"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeTaskRequest {
    pub id: u64,
    pub repository_root: String,
    pub branch: String,
    pub base_branch: Option<String>,
    pub agent_kind: Option<String>,
    pub focus: bool,
    pub purpose: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeTaskOutcome {
    pub path: String,
    pub pane_id: String,
    /// Worktree creation succeeded even when its optional purpose did not.
    /// The runtime records this as a diagnostic without turning the finished
    /// creation into a failed operation.
    pub purpose_error: Option<String>,
    /// A token-first creation save whose Git write and compensating token
    /// clear both failed. The runtime hides this unconfirmed value so the
    /// completed row follows the creation contract and shows its fallback.
    pub unconfirmed_purpose_token: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurposeTaskRequest {
    pub id: u64,
    pub checkout_id: String,
    pub repository_root: String,
    pub branch: Option<String>,
    pub session_workspace_id: Option<String>,
    pub purpose: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PurposeTaskOutcome {
    Saved {
        purpose: String,
        token_written: bool,
    },
    GitFailed {
        purpose: String,
        token_written: bool,
        detail: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PurposeMirrorRequest {
    workspace_id: String,
    repository_root: String,
    branch: String,
    purpose: Option<String>,
}

enum PurposeMirrorMessage {
    Write(PurposeMirrorRequest),
    RemoveIssue {
        repository_root: String,
        branch: String,
        workspace_ids: Vec<String>,
    },
    Stop,
}

/// Mirrors new live workspace purpose values into Git without delaying the
/// session coordinator or taking the runtime lock. Each observed token change
/// is queued once; a failed write is diagnosed and is not retried until a
/// later token change makes a new intent.
pub struct PurposeMirror {
    sender: SyncSender<PurposeMirrorMessage>,
    worker: Option<thread::JoinHandle<()>>,
    observed: BTreeMap<(String, String, String), Option<String>>,
    issue_worktrees: BTreeSet<(String, String, String)>,
}

impl PurposeMirror {
    const QUEUE_CAPACITY: usize = 64;

    pub fn new(connector: Arc<dyn ApiConnector>) -> Result<Self, String> {
        let (sender, receiver) = sync_channel(Self::QUEUE_CAPACITY);
        let worker = thread::Builder::new()
            .name("herdr-core-purpose-mirror".to_owned())
            .spawn(move || {
                let git = SystemGit;
                while let Ok(message) = receiver.recv() {
                    let request = match message {
                        PurposeMirrorMessage::Write(request) => request,
                        PurposeMirrorMessage::Stop => break,
                        PurposeMirrorMessage::RemoveIssue { repository_root, branch, workspace_ids } => {
                            for id in workspace_ids {
                                let result = wire::workspace_issue_params(&id, None).and_then(|params| control_request(connector.as_ref(), "workspace.report_metadata", params));
                                if let Err(error) = result {
                                    crate::diagnostic!(serde_json::json!({"component":"checkout_issue","kind":"removed_worktree.token_cleanup_failed","workspace_id":id,"message":error}));
                                }
                            }
                            if let Err(error) = git.unset(&repository_root, &format!("branch.{branch}.issue")) {
                                crate::diagnostic!(serde_json::json!({"component":"checkout_issue","kind":"removed_worktree.cleanup_failed","message":error}));
                            }
                            continue;
                        }
                    };
                    let key = format!("branch.{}.description", request.branch);
                    let result = match request.purpose.as_deref() {
                        Some(purpose) => git
                            .run(&request.repository_root, &["config", &key, purpose])
                            .map(|_| ()),
                        None => git.unset(&request.repository_root, &key),
                    };
                    match result {
                        Ok(()) => crate::diagnostic!(serde_json::json!({
                            "component": "checkout_purpose",
                            "kind": "mirror.saved",
                            "workspace_id": request.workspace_id,
                            "branch": request.branch,
                        })),
                        Err(message) => crate::diagnostic!(serde_json::json!({
                            "component": "checkout_purpose",
                            "kind": "mirror.failed",
                            "workspace_id": request.workspace_id,
                            "branch": request.branch,
                            "message": message,
                        })),
                    }
                }
            })
            .map_err(|error| format!("purpose mirror worker could not be started: {error}"))?;
        Ok(Self {
            sender,
            worker: Some(worker),
            observed: BTreeMap::new(),
            issue_worktrees: BTreeSet::new(),
        })
    }

    #[cfg(test)]
    fn recording() -> (Self, std::sync::mpsc::Receiver<PurposeMirrorMessage>) {
        let (sender, receiver) = sync_channel(Self::QUEUE_CAPACITY);
        (
            Self {
                sender,
                worker: None,
                observed: BTreeMap::new(),
                issue_worktrees: BTreeSet::new(),
            },
            receiver,
        )
    }

    pub fn sync(
        &mut self,
        spaces: &[workspace::SessionSpace],
        workspaces: &[WorkspaceSnapshot],
        unconfirmed_created_purposes: &HashMap<String, String>,
    ) {
        let current_issues: BTreeSet<_> = workspaces
            .iter()
            .filter(|workspace| workspace.remote_target_id.is_none())
            .flat_map(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .filter(|checkout| checkout.is_worktree)
                    .filter_map(|checkout| {
                        checkout.branch.as_ref().map(|branch| {
                            (
                                workspace.path.clone(),
                                checkout.path.clone(),
                                branch.clone(),
                            )
                        })
                    })
            })
            .collect();
        let mut retained = current_issues.clone();
        for (root, path, branch) in self.issue_worktrees.difference(&current_issues) {
            let project = workspaces.iter().find(|workspace| &workspace.path == root);
            if project.is_some_and(|project| {
                project
                    .checkouts
                    .iter()
                    .any(|checkout| &checkout.path == path && checkout.branch.is_none())
            }) {
                // Detaching keeps the last branch's cleanup ownership until removal.
                retained.insert((root.clone(), path.clone(), branch.clone()));
                continue;
            }
            // A branch switch or detached HEAD is not a removed worktree.
            if project.is_some_and(|project| {
                !project
                    .checkouts
                    .iter()
                    .any(|checkout| &checkout.path == path)
            }) && let Err(error) = self.sender.try_send(PurposeMirrorMessage::RemoveIssue {
                repository_root: root.clone(),
                branch: branch.clone(),
                workspace_ids: spaces
                    .iter()
                    .filter(|space| {
                        space
                            .cwds
                            .iter()
                            .any(|cwd| Path::new(cwd).starts_with(path))
                    })
                    .map(|space| space.id.clone())
                    .collect(),
            }) {
                retained.insert((root.clone(), path.clone(), branch.clone()));
                crate::diagnostic!(
                    serde_json::json!({"component":"checkout_issue","kind":"cleanup.queue_unavailable","message":error.to_string()})
                );
            }
        }
        self.issue_worktrees = retained;
        let mut current = BTreeSet::new();
        for workspace in workspaces {
            for checkout in &workspace.checkouts {
                let Some(branch) = checkout.branch.as_deref() else {
                    continue;
                };
                let Some(effective) =
                    workspace::effective_checkout_purpose(spaces, workspace, &checkout.path)
                else {
                    continue;
                };
                let key = (
                    workspace::normalized_for_comparison(Path::new(&workspace.path)),
                    workspace::normalized_for_comparison(Path::new(&checkout.path)),
                    branch.to_owned(),
                );
                current.insert(key.clone());
                let purpose = effective.purpose.map(str::to_owned);
                if purpose.as_deref().is_some_and(|purpose| {
                    unconfirmed_created_purposes
                        .get(&key.1)
                        .is_some_and(|unconfirmed| unconfirmed == purpose)
                }) {
                    // Do not remember the suppressed value as mirrored. Once
                    // Herdr confirms a clear or replacement, the next sync
                    // must treat that visible value as new work.
                    self.observed.remove(&key);
                    continue;
                }
                let previous = self.observed.get(&key);
                let first_live_value =
                    previous.is_none() && (purpose.is_some() || effective.has_shadowed_purpose);
                let changed_value = previous.is_some_and(|value| value != &purpose);
                self.observed.insert(key, purpose.clone());
                if !first_live_value && !changed_value {
                    continue;
                }
                let request = PurposeMirrorRequest {
                    workspace_id: effective.workspace_id.to_owned(),
                    repository_root: workspace.path.clone(),
                    branch: branch.to_owned(),
                    purpose,
                };
                if let Err(error) = self.sender.try_send(PurposeMirrorMessage::Write(request)) {
                    let kind = match error {
                        TrySendError::Full(_) => "mirror.queue_full",
                        TrySendError::Disconnected(_) => "mirror.worker_closed",
                    };
                    crate::diagnostic!(serde_json::json!({
                        "component": "checkout_purpose",
                        "kind": kind,
                        "workspace_id": effective.workspace_id,
                    }));
                }
            }
        }
        self.observed.retain(|key, _| current.contains(key));
    }
}

impl Drop for PurposeMirror {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = self.sender.send(PurposeMirrorMessage::Stop);
            if worker.join().is_err() {
                crate::diagnostic!(serde_json::json!({
                    "component": "checkout_purpose",
                    "kind": "mirror.join_failed",
                }));
            }
        }
    }
}

pub fn spawn_worktree_create(
    context: WorktreeTarget,
    request: WorktreeTaskRequest,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-worktree-create".into())
        .spawn(move || {
            let result = check_new_branch(
                context.host.as_ref(),
                &request.repository_root,
                &request.branch,
            )
            .and_then(|()| {
                create_worktree_observing_purpose(
                    context.connector.as_ref(),
                    context.host.as_ref(),
                    context.local,
                    &request,
                    |path, purpose| {
                        if let Some(runtime) = context.runtime.upgrade()
                            && let Ok(mut guard) = runtime.lock()
                        {
                            guard.begin_created_purpose_write(path, purpose);
                        }
                    },
                )
            });
            if let Some(runtime) = context.runtime.upgrade() {
                if let Ok(mut guard) = runtime.lock() {
                    guard.ingest_task_operation_result(request.id, result);
                } else {
                    return;
                }
                context.notifier.notify();
            }
            start_task_agent(
                context.connector.as_ref(),
                &context.runtime,
                &context.notifier,
                context.local,
                request.id,
            );
        })
        .map(|_| ())
        .map_err(|error| format!("worktree create worker could not be started: {error}"))
}

pub fn spawn_purpose_write(
    context: LiveContext,
    request: PurposeTaskRequest,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-purpose-write".into())
        .spawn(move || {
            let git = SystemGit;
            let result = write_purpose(context.api_connector.as_ref(), &git, &request);
            if let Some(runtime) = context.runtime.upgrade() {
                if let Ok(mut guard) = runtime.lock() {
                    guard.ingest_purpose_operation_result(request.id, result);
                } else {
                    return;
                }
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("purpose worker could not be started: {error}"))
}

#[derive(Clone, Debug)]
pub struct IssueWriteFailure {
    pub detail: String,
    pub unconfirmed_token: bool,
}
impl IssueWriteFailure {
    pub fn unchanged(detail: String) -> Self {
        Self {
            detail,
            unconfirmed_token: false,
        }
    }
}

fn write_issue_metadata(
    connector: &dyn ApiConnector,
    git: &dyn GitCommands,
    request: &PurposeTaskRequest,
    previous: Option<&str>,
) -> Result<(), IssueWriteFailure> {
    let (detail, token_may_have_changed) =
        match write_workspace_metadata(connector, git, request, "issue", "issue") {
            Ok(PurposeTaskOutcome::Saved { .. }) => return Ok(()),
            Ok(PurposeTaskOutcome::GitFailed {
                detail,
                token_written,
                ..
            }) => (detail, token_written),
            // A transport failure can arrive after the server accepted the token.
            Err(detail) => (detail, request.session_workspace_id.is_some()),
        };
    if token_may_have_changed && let Some(workspace_id) = request.session_workspace_id.as_deref() {
        let rollback = wire::workspace_issue_params(workspace_id, previous).and_then(|params| {
            control_request(connector, "workspace.report_metadata", params).map(|_| ())
        });
        if let Err(error) = rollback {
            return Err(IssueWriteFailure {
                detail: format!("{detail}; issue rollback failed: {error}"),
                unconfirmed_token: true,
            });
        }
    }
    Err(IssueWriteFailure::unchanged(detail))
}

pub fn spawn_issue_write(
    context: LiveContext,
    request: PurposeTaskRequest,
    previous: Option<String>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-issue-write".into())
        .spawn(move || {
            let result = (|| {
                let issue = if request.purpose.is_empty() {
                    None
                } else {
                    let reference = crate::issues::IssueReference::parse(&request.purpose, None)
                        .map_err(IssueWriteFailure::unchanged)?;
                    Some(
                        crate::github::read_linked_issue(
                            Path::new(&request.repository_root),
                            &reference,
                        )
                        .map_err(IssueWriteFailure::unchanged)?,
                    )
                };
                write_issue_metadata(
                    context.api_connector.as_ref(),
                    &SystemGit,
                    &request,
                    previous.as_deref(),
                )?;
                Ok(issue)
            })();
            if let Some(runtime) = context.runtime.upgrade() {
                if let Ok(mut guard) = runtime.lock() {
                    guard.ingest_issue_operation_result(&request, result);
                }
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("issue worker could not be started: {error}"))
}

pub fn spawn_remote_purpose_write(
    context: RemoteControlContext,
    request: PurposeTaskRequest,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-remote-purpose-write".into())
        .spawn(move || {
            let git = SystemGit;
            let result = write_purpose(context.api_connector.as_ref(), &git, &request);
            if let Some(runtime) = context.runtime.upgrade() {
                if let Ok(mut guard) = runtime.lock() {
                    guard.ingest_purpose_operation_result(request.id, result);
                } else {
                    return;
                }
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("remote purpose worker could not be started: {error}"))
}

/// `New agent here`: one new tab whose cwd is the checkout, in the Herdr
/// workspace that already holds the checkout's panes, or a new workspace on
/// the checkout when Herdr holds none (Herdr drops a workspace with its last
/// pane, so a listed checkout can have no workspace behind it). The result
/// lands in the same task operation slot the worktree sheet uses, and the
/// shell starts the provider in the returned pane exactly as it does there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckoutTabRequest {
    pub id: u64,
    pub checkout_path: String,
    pub label: String,
    /// The Herdr workspace to open the tab in; `None` creates a workspace.
    pub session_workspace_id: Option<String>,
}

/// How starting the chosen agent in a task's created pane ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskAgentOutcome {
    Started,
    /// Herdr refused, or the agent is not installed; nothing started.
    Failed(String),
    /// Herdr did not answer; the agent may be running. The pane says which.
    Unknown(String),
}

/// `agent.start` waits up to this long for the pane's shell; the read waits a
/// little longer so a slow start is not reported as an unknown one.
const AGENT_START_TIMEOUT_MS: u64 = 120_000;

/// Starts the agent the task chose, in the pane the task created, and hands
/// the answer back on its own axis. The creation was already published, so a
/// failure here never hides the worktree or the pane.
fn start_task_agent(
    connector: &dyn ApiConnector,
    runtime: &Weak<Mutex<Runtime>>,
    notifier: &ChangeNotifier,
    local: bool,
    id: u64,
) {
    let Some(runtime) = runtime.upgrade() else {
        return;
    };
    let pending = match runtime.lock() {
        Ok(guard) => guard.pending_task_agent_start(id),
        Err(_) => return,
    };
    let Some((pane_id, kind)) = pending else {
        return;
    };
    let outcome = launch_agent(connector, local, id, &pane_id, &kind);
    if let Ok(mut guard) = runtime.lock() {
        guard.ingest_task_agent_result(id, outcome);
    } else {
        return;
    }
    notifier.notify();
}

pub fn spawn_task_agent_start(context: WorktreeTarget, id: u64) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-task-agent-start".into())
        .spawn(move || {
            start_task_agent(
                context.connector.as_ref(),
                &context.runtime,
                &context.notifier,
                context.local,
                id,
            )
        })
        .map(|_| ())
        .map_err(|error| format!("agent start worker could not be started: {error}"))
}

fn launch_agent(
    connector: &dyn ApiConnector,
    local: bool,
    id: u64,
    pane_id: &str,
    kind: &str,
) -> TaskAgentOutcome {
    // Herdr would type the command into the pane's shell and wait for an
    // agent that can never appear; say so before asking it. A device's PATH
    // is not this machine's, so there its Herdr answers for it.
    if local && hide_ai::resolve_binary(Path::new(kind)).is_none() {
        return TaskAgentOutcome::Failed(format!(
            "{kind} is not installed on the daemon's PATH. Install it, then retry."
        ));
    }
    let params = match wire::agent_start_params(pane_id, &format!("hide-{kind}"), kind, Vec::new())
    {
        Ok(params) => params,
        Err(message) => return TaskAgentOutcome::Failed(message),
    };
    match request_with_correlation_id(
        connector,
        &format!("herdr-core:task:{id}:agent"),
        "agent.start",
        params,
        Duration::from_millis(AGENT_START_TIMEOUT_MS + 5_000),
    ) {
        Ok(value) => match wire::started_agent(value) {
            Ok(_) => TaskAgentOutcome::Started,
            Err(message) => TaskAgentOutcome::Unknown(format!(
                "Herdr answered the agent start in an unexpected shape ({message}). Check the pane before retrying."
            )),
        },
        Err(ApiError::Remote { code, message }) => {
            TaskAgentOutcome::Failed(format!("Agent could not start: {code}: {message}"))
        }
        Err(error) => TaskAgentOutcome::Unknown(format!(
            "Herdr did not confirm the agent start ({error}). Check the pane before retrying."
        )),
    }
}

pub fn spawn_checkout_tab_create(
    context: LiveContext,
    request: CheckoutTabRequest,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-checkout-tab-create".into())
        .spawn(move || {
            let result = create_checkout_tab(context.api_connector.as_ref(), &request);
            if let Some(runtime) = context.runtime.upgrade() {
                if let Ok(mut guard) = runtime.lock() {
                    guard.ingest_task_operation_result(request.id, result);
                } else {
                    return;
                }
                context.notifier.notify();
            }
            start_task_agent(
                context.api_connector.as_ref(),
                &context.runtime,
                &context.notifier,
                true,
                request.id,
            );
        })
        .map(|_| ())
        .map_err(|error| format!("checkout tab worker could not be started: {error}"))
}

fn create_checkout_tab(
    connector: &dyn ApiConnector,
    request: &CheckoutTabRequest,
) -> Result<WorktreeTaskOutcome, String> {
    let pane_id = match &request.session_workspace_id {
        Some(workspace_id) => {
            let result = control_request(
                connector,
                "tab.create",
                wire::tab_create_params(workspace_id, &request.checkout_path, &request.label)?,
            )?;
            wire::created_tab(result)?.1
        }
        None => {
            let result = control_request(
                connector,
                "workspace.create",
                wire::workspace_create_params(&request.checkout_path, &request.label)?,
            )?;
            wire::created_workspace_pane(result)?
        }
    };
    Ok(WorktreeTaskOutcome {
        path: request.checkout_path.clone(),
        pane_id,
        purpose_error: None,
        unconfirmed_purpose_token: None,
    })
}

pub fn spawn_branch_migration(
    context: LiveContext,
    request: WorktreeTaskRequest,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-branch-migrate".into())
        .spawn(move || {
            let runner = SystemGit;
            let result = migrate_branch(context.api_connector.as_ref(), &runner, &request);
            if let Some(runtime) = context.runtime.upgrade() {
                if let Ok(mut guard) = runtime.lock() {
                    guard.ingest_task_operation_result(request.id, result);
                } else {
                    return;
                }
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("branch migration worker could not be started: {error}"))
}

fn create_worktree(
    connector: &dyn ApiConnector,
    host: &dyn crate::host_access::HostChannel,
    local: bool,
    request: &WorktreeTaskRequest,
) -> Result<WorktreeTaskOutcome, String> {
    create_worktree_observing_purpose(connector, host, local, request, |_, _| {})
}

fn create_worktree_observing_purpose(
    connector: &dyn ApiConnector,
    host: &dyn crate::host_access::HostChannel,
    local: bool,
    request: &WorktreeTaskRequest,
    on_purpose_write: impl FnOnce(&str, &str),
) -> Result<WorktreeTaskOutcome, String> {
    let params = wire::worktree_create_params(
        &request.repository_root,
        &request.branch,
        request.base_branch.as_deref(),
        request.focus,
    )?;
    let result = control_request(connector, "worktree.create", params)
        .map_err(|error| format!("create worktree: {error}"))?;
    let created = wire::created_worktree(result)?;
    // The folder is on the repository's host, which answers for it. A host
    // that cannot answer leaves the worktree as created and says so, rather
    // than rolling back what may be a good worktree (B27).
    let unverified = |error: String| {
        format!(
            "The worktree was created at {} but its host could not confirm it ({error}); it was kept and no agent was started in it. Once its host answers it is listed under its project, where you can start the agent in its pane or delete the worktree; creating it again is refused because its branch is in use",
            created.path
        )
    };
    let created_real = host_directory(host, &created.path).map_err(unverified)?;
    let path_exists = created_real.is_some();
    let listed = control_request(
        connector,
        "worktree.list",
        wire::worktree_list_params(&request.repository_root)?,
    )
    .and_then(|result| wire::listed_worktree_path(result, &request.branch));
    let identity_matches = created.branch.as_deref() == Some(request.branch.as_str())
        && path_exists
        && listed
            .as_ref()
            .ok()
            .and_then(|path| path.as_deref())
            .map(|path| host_directory(host, path).map(|real| (path, real)))
            .transpose()
            .map_err(unverified)?
            .is_some_and(|(_, real)| created_real.is_some() && real == created_real);
    if identity_matches {
        let purpose_failure = request.purpose.as_ref().and_then(|purpose| {
            on_purpose_write(&created.path, purpose);
            let git = SystemGit;
            let purpose_request = PurposeTaskRequest {
                id: request.id,
                checkout_id: String::new(),
                repository_root: request.repository_root.clone(),
                // A device's purpose lives in its Herdr metadata only; the
                // branch description mirror is this machine's (B30).
                branch: local.then(|| request.branch.clone()),
                session_workspace_id: Some(created.workspace_id.clone()),
                purpose: purpose.clone(),
            };
            write_created_purpose(connector, &git, &purpose_request)
        });
        return Ok(WorktreeTaskOutcome {
            path: created.path,
            pane_id: created.pane_id,
            purpose_error: purpose_failure
                .as_ref()
                .map(|failure| failure.detail.clone()),
            unconfirmed_purpose_token: purpose_failure
                .and_then(|failure| failure.unconfirmed_token),
        });
    }

    let reason = if created.branch.as_deref() != Some(request.branch.as_str()) {
        format!(
            "created branch mismatch: requested {}, Herdr returned {}",
            request.branch,
            created.branch.as_deref().unwrap_or("detached")
        )
    } else if !path_exists {
        format!("created path is missing on disk: {}", created.path)
    } else {
        let detail = match &listed {
            Ok(Some(path)) => format!("Herdr listed the branch at {path}"),
            Ok(None) => "Herdr did not list the created branch".to_owned(),
            Err(error) => error.clone(),
        };
        format!(
            "created worktree is absent from Herdr's worktree list: {}",
            detail
        )
    };
    let rollback = control_request(
        connector,
        "worktree.remove",
        wire::worktree_remove_params(&created.workspace_id)?,
    );
    if let Err(error) = rollback {
        return Err(format!("{reason}; rollback failed: {error}"));
    }
    let still_listed = control_request(
        connector,
        "worktree.list",
        wire::worktree_list_params(&request.repository_root)?,
    )
    .and_then(|result| wire::listed_worktree_path(result, &request.branch))
    .map(|path| path.is_some())
    .unwrap_or(true);
    let residue = host_directory(host, &created.path);
    if let Err(error) = &residue {
        return Err(format!(
            "{reason}; the rollback could not be confirmed on the host ({error})"
        ));
    }
    if residue.is_ok_and(|real| real.is_some()) || still_listed {
        return Err(format!(
            "{reason}; rollback left residue at {} or in Herdr's worktree list",
            created.path
        ));
    }
    Err(format!("{reason}; the created worktree was rolled back"))
}

trait GitCommands {
    fn run(&self, cwd: &str, args: &[&str]) -> Result<String, String>;

    fn unset(&self, cwd: &str, key: &str) -> Result<(), String> {
        self.run(cwd, &["config", "--unset-all", key]).map(|_| ())
    }
}

struct SystemGit;
impl GitCommands for SystemGit {
    fn run(&self, cwd: &str, args: &[&str]) -> Result<String, String> {
        let output = Command::new("git")
            .arg("--no-optional-locks")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .map_err(|error| format!("git could not be run: {error}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            Err(if detail.is_empty() {
                format!("git {} exited with {}", args[0], output.status)
            } else {
                format!("git {}: {detail}", args[0])
            })
        }
    }

    fn unset(&self, cwd: &str, key: &str) -> Result<(), String> {
        let output = Command::new("git")
            .arg("--no-optional-locks")
            .arg("-C")
            .arg(cwd)
            .args(["config", "--unset-all", key])
            .output()
            .map_err(|error| format!("git could not be run: {error}"))?;
        if output.status.success() || output.status.code() == Some(5) {
            Ok(())
        } else {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            Err(if detail.is_empty() {
                format!("git config exited with {}", output.status)
            } else {
                format!("git config: {detail}")
            })
        }
    }
}

fn write_purpose(
    connector: &dyn ApiConnector,
    git: &dyn GitCommands,
    request: &PurposeTaskRequest,
) -> Result<PurposeTaskOutcome, String> {
    write_workspace_metadata(connector, git, request, "purpose", "description")
}

fn write_workspace_metadata(
    connector: &dyn ApiConnector,
    git: &dyn GitCommands,
    request: &PurposeTaskRequest,
    token: &str,
    config_key: &str,
) -> Result<PurposeTaskOutcome, String> {
    let purpose = request.purpose.trim().to_owned();
    if purpose.chars().count() > 80 {
        return Err("Purpose must be 80 characters or fewer".to_owned());
    }
    if purpose.contains(['\n', '\r']) {
        return Err("Purpose must be one line".to_owned());
    }
    let token_written = request.session_workspace_id.is_some();
    if let Some(workspace_id) = request.session_workspace_id.as_deref() {
        control_request(
            connector,
            "workspace.report_metadata",
            if token == "issue" {
                wire::workspace_issue_params(
                    workspace_id,
                    (!purpose.is_empty()).then_some(purpose.as_str()),
                )?
            } else {
                wire::workspace_purpose_params(
                    workspace_id,
                    (!purpose.is_empty()).then_some(purpose.as_str()),
                )?
            },
        )
        .map_err(|error| format!("workspace purpose: {error}"))?;
    }
    let Some(branch) = request.branch.as_deref() else {
        return Ok(PurposeTaskOutcome::Saved {
            purpose,
            token_written,
        });
    };
    let key = format!("branch.{branch}.{config_key}");
    let git_result = if purpose.is_empty() {
        git.unset(&request.repository_root, &key)
    } else {
        git.run(
            &request.repository_root,
            &["config", &key, purpose.as_str()],
        )
        .map(|_| ())
    };
    match git_result {
        Ok(()) => Ok(PurposeTaskOutcome::Saved {
            purpose,
            token_written,
        }),
        Err(detail) => Ok(PurposeTaskOutcome::GitFailed {
            purpose,
            token_written,
            detail,
        }),
    }
}

/// Creation closes its sheet after the worktree exists, even when purpose
/// persistence fails. Compensate a token-first partial save so the new row
/// shows its fallback instead of claiming a purpose Git did not preserve.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CreatedPurposeFailure {
    detail: String,
    unconfirmed_token: Option<String>,
}

fn write_created_purpose(
    connector: &dyn ApiConnector,
    git: &dyn GitCommands,
    request: &PurposeTaskRequest,
) -> Option<CreatedPurposeFailure> {
    match write_purpose(connector, git, request) {
        Ok(PurposeTaskOutcome::Saved { .. }) => None,
        Ok(PurposeTaskOutcome::GitFailed {
            purpose,
            token_written: true,
            detail,
            ..
        }) => {
            let cleanup = request.session_workspace_id.as_deref().map(|workspace_id| {
                control_request(
                    connector,
                    "workspace.report_metadata",
                    wire::workspace_purpose_params(workspace_id, None)
                        .map_err(|error| error.to_string())?,
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
            });
            match cleanup {
                Some(Ok(())) | None => Some(CreatedPurposeFailure {
                    detail,
                    unconfirmed_token: None,
                }),
                Some(Err(cleanup_error)) => Some(CreatedPurposeFailure {
                    detail: format!("{detail}; Herdr token cleanup failed: {cleanup_error}"),
                    unconfirmed_token: Some(purpose),
                }),
            }
        }
        Ok(PurposeTaskOutcome::GitFailed { detail, .. }) | Err(detail) => {
            Some(CreatedPurposeFailure {
                detail,
                unconfirmed_token: None,
            })
        }
    }
}

fn migrate_branch(
    connector: &dyn ApiConnector,
    git: &dyn GitCommands,
    request: &WorktreeTaskRequest,
) -> Result<WorktreeTaskOutcome, String> {
    let status = git.run(
        &request.repository_root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !status.is_empty() {
        return Err(
            "preflight: commit or discard uncommitted changes before moving the branch".into(),
        );
    }
    let original = git.run(
        &request.repository_root,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )?;
    let Some(base) = request.base_branch.as_deref() else {
        return Err("preflight: the project base branch is unknown".into());
    };
    if original == base {
        return Err(format!("preflight: the main worktree is already on {base}"));
    }
    git.run(
        &request.repository_root,
        &["checkout", "--no-overwrite-ignore", base],
    )
    .map_err(|error| format!("checkout base branch: {error}"))?;

    let create_request = WorktreeTaskRequest {
        branch: original.clone(),
        focus: false,
        purpose: None,
        ..request.clone()
    };
    match create_worktree(
        connector,
        &crate::host_access::InProcessHost,
        true,
        &create_request,
    ) {
        Ok(outcome) => Ok(outcome),
        Err(create_error) => match git.run(
            &request.repository_root,
            &["checkout", "--no-overwrite-ignore", &original],
        ) {
            Ok(_) => Err(format!(
                "create worktree: {create_error}; restored the main worktree to {original}"
            )),
            Err(restore_error) => {
                let current = git
                    .run(
                        &request.repository_root,
                        &["symbolic-ref", "--quiet", "--short", "HEAD"],
                    )
                    .unwrap_or_else(|_| "an unknown branch".into());
                Err(format!(
                    "restore original branch: {restore_error}. The main worktree is now on {current}. After resolving the Git error, open the repository at {} and check out {original}.",
                    request.repository_root
                ))
            }
        },
    }
}

fn trace(path: &str, pane_ids: &[String], stage: &str, error: Option<&str>) {
    eprintln!(
        "{}",
        json!({"event":"worktree.control", "path":path, "pane_ids":pane_ids, "stage":stage, "error":error})
    );
}

/// Closes `pane_ids` and waits until Herdr's snapshot lists none of them and
/// no pane at any of `paths`, so the caller's next step cannot run beside a
/// pane that is still there.
fn close_checkout_panes(
    connector: &dyn ApiConnector,
    paths: &[String],
    pane_ids: &[String],
    timeout: Duration,
) -> Result<(), String> {
    let path = paths.join(",");
    let path = path.as_str();
    let result = (|| {
        for pane_id in pane_ids {
            trace(path, std::slice::from_ref(pane_id), "close_requested", None);
            control_request(connector, "pane.close", json!({"pane_id":pane_id}))?;
        }
        if pane_ids.is_empty() {
            return Ok(());
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| {
                    format!(
                        "Timed out waiting for Herdr to confirm closed panes: {}",
                        pane_ids.join(", ")
                    )
                })?;
            let result =
                request_with_connector(connector, "session.snapshot", json!({}), remaining)
                    .map_err(|error| {
                        format!("session.snapshot close confirmation failed: {error}")
                    })?;
            // No serde defaults here: missing topology is never proof of absence.
            if result["type"] != "session_snapshot" {
                return Err("Herdr close confirmation is not a session_snapshot".into());
            }
            let panes = result
                .pointer("/snapshot/panes")
                .and_then(Value::as_array)
                .ok_or("Herdr close confirmation is missing snapshot.panes")?;
            let (present, pane_at_checkout) = confirmation_state(panes, paths)?;
            if pane_ids.iter().all(|id| !present.contains(id)) && !pane_at_checkout {
                return Ok(());
            }
            thread::sleep(CONFIRM_POLL.min(deadline.saturating_duration_since(Instant::now())));
        }
    })();
    trace(
        path,
        pane_ids,
        if result.is_ok() {
            "close_confirmed"
        } else {
            "close_failed"
        },
        result.as_ref().err().map(String::as_str),
    );
    result
}

fn confirmation_state(panes: &[Value], paths: &[String]) -> Result<(Vec<String>, bool), String> {
    let present = panes
        .iter()
        .map(|pane| {
            pane.get("pane_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| "Herdr close confirmation has an invalid pane_id".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let pane_at_checkout = panes.iter().any(|pane| {
        ["cwd", "foreground_cwd"].into_iter().any(|key| {
            pane.get(key)
                .and_then(Value::as_str)
                .is_some_and(|cwd| paths.iter().any(|path| path == cwd))
        })
    });
    Ok((present, pane_at_checkout))
}

fn open_worktree(
    connector: &dyn ApiConnector,
    path: &str,
    repository_root: &str,
    pane_id: Option<&str>,
) -> Result<(), String> {
    let panes = pane_id.into_iter().map(str::to_owned).collect::<Vec<_>>();
    trace(path, &panes, "open_requested", None);
    let result = match pane_id {
        Some(id) => control_request(connector, "pane.focus", json!({"pane_id":id})),
        None => control_request(
            connector,
            "worktree.open",
            json!({"cwd":repository_root,"path":path,"focus":true}),
        ),
    }
    .map(|_| ());
    trace(
        path,
        &panes,
        if result.is_ok() {
            "open_completed"
        } else {
            "open_failed"
        },
        result.as_ref().err().map(String::as_str),
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use hide_herdr_client::{ApiError, ApiStream};
    use std::collections::VecDeque;
    use std::os::unix::net::UnixStream;

    // The only fake is the external server's newline-delimited protocol.
    struct Server {
        replies: Mutex<VecDeque<Value>>,
        requests: Arc<Mutex<Vec<Value>>>,
    }
    impl ApiConnector for Server {
        fn connect(&self) -> Result<Box<dyn ApiStream>, ApiError> {
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected request");
            let requests = self.requests.clone();
            let (client, mut server) = UnixStream::pair().unwrap();
            thread::spawn(move || {
                let mut line = String::new();
                BufReader::new(server.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                let mut response = reply;
                response["id"] = request["id"].clone();
                requests.lock().unwrap().push(request);
                writeln!(server, "{response}").unwrap();
            });
            Ok(Box::new(client))
        }
    }
    fn server(replies: Vec<Value>) -> Server {
        Server {
            replies: Mutex::new(replies.into()),
            requests: Arc::new(Mutex::new(vec![])),
        }
    }
    fn snapshot(ids: &[&str]) -> Value {
        json!({"result":{"type":"session_snapshot","snapshot":{"panes":ids.iter().map(|id|json!({"pane_id":id})).collect::<Vec<_>>()}}})
    }
    fn workspace_created(path: &str, workspace_id: &str, pane_id: &str) -> Value {
        json!({"result":{
            "type":"workspace_created",
            "workspace":{"workspace_id":workspace_id,"label":"hide","number":1,"focused":true,"pane_count":1,"tab_count":1,"active_tab_id":format!("{workspace_id}:t1"),"agent_status":"unknown"},
            "tab":{"workspace_id":workspace_id,"tab_id":format!("{workspace_id}:t1"),"label":"hide codex","number":1,"focused":true,"pane_count":1,"agent_status":"unknown"},
            "root_pane":{"workspace_id":workspace_id,"tab_id":format!("{workspace_id}:t1"),"pane_id":pane_id, "terminal_id": "fixture-terminal","cwd":path,"foreground_cwd":path,"focused":true,"agent_status":"unknown","revision":0,"scroll":{"max_offset_from_bottom":0,"offset_from_bottom":0,"viewport_rows":40}}
        }})
    }
    fn worktree_created(path: &str, branch: &str) -> Value {
        let mut value = workspace_created(path, "w2", "w2:p1");
        let result = value["result"].as_object_mut().unwrap();
        result.insert("type".into(), json!("worktree_created"));
        result.insert(
            "worktree".into(),
            json!({
                "branch":branch,"is_bare":false,"is_detached":false,
                "is_linked_worktree":true,"is_prunable":false,"label":"repo",
                "open_workspace_id":"w2","path":path
            }),
        );
        value
    }
    fn worktree_list(path: &str, branch: &str) -> Value {
        json!({"result":{"type":"worktree_list","source":{
            "repo_key":"/fixture/repo/.git","repo_name":"repo","repo_root":"/fixture/repo",
            "source_checkout_path":"/fixture/repo","source_workspace_id":"w1"
        },"worktrees":[{
            "branch":branch,"is_bare":false,"is_detached":false,
            "is_linked_worktree":true,"is_prunable":false,"label":"repo",
            "open_workspace_id":"w2","path":path
        }]}})
    }
    struct ScriptedGit {
        replies: Mutex<VecDeque<Result<String, String>>>,
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl ScriptedGit {
        fn new(replies: Vec<Result<&str, &str>>) -> Self {
            Self {
                replies: Mutex::new(
                    replies
                        .into_iter()
                        .map(|result| result.map(str::to_owned).map_err(str::to_owned))
                        .collect(),
                ),
                calls: Mutex::new(Vec::new()),
            }
        }
    }
    impl GitCommands for ScriptedGit {
        fn run(&self, _cwd: &str, args: &[&str]) -> Result<String, String> {
            self.calls
                .lock()
                .unwrap()
                .push(args.iter().map(|arg| (*arg).to_owned()).collect());
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected git call")
        }
    }
    fn task(branch: &str) -> WorktreeTaskRequest {
        WorktreeTaskRequest {
            id: 1,
            repository_root: "/fixture/repo".into(),
            branch: branch.into(),
            base_branch: Some("main".into()),
            agent_kind: None,
            focus: true,
            purpose: None,
        }
    }

    fn purpose_request(purpose: &str) -> PurposeTaskRequest {
        PurposeTaskRequest {
            id: 7,
            checkout_id: "checkout:feature".into(),
            repository_root: "/fixture/repo".into(),
            branch: Some("feature".into()),
            session_workspace_id: Some("w7".into()),
            purpose: purpose.into(),
        }
    }

    #[test]
    fn issue_writer_uses_the_same_token_first_boundary_and_clears_both_values() {
        let server = server(vec![
            json!({"result":{"type":"ok"}}),
            json!({"result":{"type":"ok"}}),
        ]);
        let git = ScriptedGit::new(vec![Ok(""), Ok("")]);
        write_workspace_metadata(
            &server,
            &git,
            &purpose_request("acme/project#42"),
            "issue",
            "issue",
        )
        .unwrap();
        write_workspace_metadata(&server, &git, &purpose_request(""), "issue", "issue").unwrap();
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests[0]["params"]["tokens"]["issue"], "acme/project#42");
        assert!(requests[1]["params"]["tokens"]["issue"].is_null());
        let calls = git.calls.lock().unwrap();
        assert_eq!(
            calls[0],
            ["config", "branch.feature.issue", "acme/project#42"]
        );
        assert_eq!(calls[1], ["config", "--unset-all", "branch.feature.issue"]);
    }

    #[test]
    fn purpose_writes_the_workspace_token_before_the_branch_description() {
        let server = server(vec![json!({"result":{"type":"ok"}})]);
        let git = ScriptedGit::new(vec![Ok("")]);

        assert_eq!(
            write_purpose(&server, &git, &purpose_request("Ship checkout row D")).unwrap(),
            PurposeTaskOutcome::Saved {
                purpose: "Ship checkout row D".into(),
                token_written: true,
            }
        );

        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["method"], "workspace.report_metadata");
        assert_eq!(requests[0]["params"]["workspace_id"], "w7");
        assert_eq!(requests[0]["params"]["source"], "hide");
        assert_eq!(
            requests[0]["params"]["tokens"]["purpose"],
            "Ship checkout row D"
        );
        assert_eq!(
            git.calls.lock().unwrap().as_slice(),
            [vec![
                "config".to_owned(),
                "branch.feature.description".to_owned(),
                "Ship checkout row D".to_owned(),
            ]]
        );
    }

    #[test]
    fn issue_cleanup_distinguishes_branch_switch_detach_removal_and_unregister() {
        let (mut mirror, receiver) = PurposeMirror::recording();
        let mut project = WorkspaceSnapshot {
            home_issues: Default::default(),
            id: "project".to_owned(),
            label: "Fixture".to_owned(),
            path: "/fixture/repo".to_owned(),
            remote_target_id: None,
            expanded: true,
            device_id: "local".to_owned(),
            repo_name: "repo".to_owned(),
            is_git: true,
            default_branch: Some("main".to_owned()),
            branches: vec!["main".to_owned(), "topic/quoted".to_owned()],
            registered: true,
            temporary: false,
            session_workspace_ids: vec!["w-purpose".to_owned()],
            last_activity_unix_ms: None,
            pinned: false,
            checkouts: vec![crate::model::CheckoutSnapshot {
                id: "checkout".to_owned(),
                workspace_id: "project".to_owned(),
                label: "topic/quoted".to_owned(),
                path: "/fixture/repo/worktrees/topic".to_owned(),
                branch: Some("topic/quoted".to_owned()),
                exists: true,
                is_worktree: true,
                ..Default::default()
            }],
            inactive_checkouts: Default::default(),
            removal: Default::default(),
        };
        mirror.sync(&[], std::slice::from_ref(&project), &HashMap::new());
        project.checkouts[0].branch = Some("other".into());
        mirror.sync(&[], std::slice::from_ref(&project), &HashMap::new());
        project.checkouts[0].branch = None;
        mirror.sync(&[], std::slice::from_ref(&project), &HashMap::new());
        assert!(receiver.try_recv().is_err());
        project.checkouts[0].branch = Some("other".into());
        mirror.sync(&[], std::slice::from_ref(&project), &HashMap::new());
        mirror.sync(&[], &[], &HashMap::new());
        assert!(receiver.try_recv().is_err(), "unregister is not deletion");
        mirror.sync(&[], std::slice::from_ref(&project), &HashMap::new());
        project.checkouts[0].branch = None;
        mirror.sync(&[], std::slice::from_ref(&project), &HashMap::new());
        mirror.sync(&[], std::slice::from_ref(&project), &HashMap::new());
        assert!(receiver.try_recv().is_err(), "detaching is not deletion");
        project.checkouts.clear();
        let space = workspace::SessionSpace {
            id: "w-purpose".into(),
            label: "Fixture".into(),
            cwds: vec!["/fixture/repo/worktrees/topic".into()],
            purpose: None,
        };
        mirror.sync(&[space], &[project], &HashMap::new());
        let PurposeMirrorMessage::RemoveIssue {
            branch,
            workspace_ids,
            ..
        } = receiver
            .try_recv()
            .expect("detached removal still owns cleanup")
        else {
            panic!("expected cleanup")
        };
        assert_eq!(branch, "other");
        assert_eq!(workspace_ids, vec!["w-purpose"]);
        assert!(receiver.try_recv().is_err());
    }
    #[test]
    fn issue_rollback_reports_only_an_uncertain_token_as_unconfirmed() {
        for failed_rollback in [false, true] {
            let rollback = if failed_rollback {
                json!({"error":{"code":"unavailable","message":"rollback refused"}})
            } else {
                json!({"result":{"type":"ok"}})
            };
            let server = server(vec![json!({"result":{"type":"ok"}}), rollback]);
            let git = ScriptedGit::new(vec![Err("git locked")]);
            let error = write_issue_metadata(
                &server,
                &git,
                &purpose_request("acme/project#3"),
                Some("acme/project#1"),
            )
            .unwrap_err();
            assert_eq!(error.unconfirmed_token, failed_rollback);
            assert_eq!(
                server.requests.lock().unwrap()[1]["params"]["tokens"]["issue"],
                "acme/project#1"
            );
        }
    }

    #[test]
    fn rejected_workspace_token_never_writes_git() {
        let server = server(vec![json!({
            "error":{"code":"unavailable","message":"injected token refusal"}
        })]);
        let git = ScriptedGit::new(vec![]);

        let error = write_purpose(&server, &git, &purpose_request("Keep input")).unwrap_err();

        assert!(error.contains("injected token refusal"));
        assert!(git.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn git_failure_after_a_workspace_token_is_a_partial_save() {
        let server = server(vec![json!({"result":{"type":"ok"}})]);
        let git = ScriptedGit::new(vec![Err("injected git lock")]);

        assert_eq!(
            write_purpose(&server, &git, &purpose_request("Visible live")).unwrap(),
            PurposeTaskOutcome::GitFailed {
                purpose: "Visible live".into(),
                token_written: true,
                detail: "injected git lock".into(),
            }
        );
    }

    #[test]
    fn git_failure_without_a_live_workspace_reports_no_partial_save() {
        let server = server(vec![]);
        let git = ScriptedGit::new(vec![Err("injected git lock")]);
        let mut request = purpose_request("Keep input");
        request.session_workspace_id = None;

        assert_eq!(
            write_purpose(&server, &git, &request).unwrap(),
            PurposeTaskOutcome::GitFailed {
                purpose: "Keep input".into(),
                token_written: false,
                detail: "injected git lock".into(),
            }
        );
        assert!(server.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn clearing_a_purpose_clears_the_token_and_branch_description() {
        let server = server(vec![json!({"result":{"type":"ok"}})]);
        let git = ScriptedGit::new(vec![Ok("")]);

        assert!(matches!(
            write_purpose(&server, &git, &purpose_request("")),
            Ok(PurposeTaskOutcome::Saved { purpose, .. }) if purpose.is_empty()
        ));

        let requests = server.requests.lock().unwrap();
        assert!(requests[0]["params"]["tokens"]["purpose"].is_null());
        assert_eq!(
            git.calls.lock().unwrap().as_slice(),
            [vec![
                "config".to_owned(),
                "--unset-all".to_owned(),
                "branch.feature.description".to_owned(),
            ]]
        );
    }

    #[test]
    fn creation_compensates_a_token_when_the_git_mirror_fails() {
        let server = server(vec![
            json!({"result":{"type":"ok"}}),
            json!({"result":{"type":"ok"}}),
        ]);
        let git = ScriptedGit::new(vec![Err("injected git lock")]);

        assert_eq!(
            write_created_purpose(&server, &git, &purpose_request("Use the fallback")),
            Some(CreatedPurposeFailure {
                detail: "injected git lock".to_owned(),
                unconfirmed_token: None,
            })
        );

        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0]["params"]["tokens"]["purpose"],
            "Use the fallback"
        );
        assert!(requests[1]["params"]["tokens"]["purpose"].is_null());
    }

    #[test]
    fn creation_marks_a_token_unconfirmed_when_git_and_cleanup_both_fail() {
        let server = server(vec![
            json!({"result":{"type":"ok"}}),
            json!({"error":{"code":"unavailable","message":"injected cleanup refusal"}}),
        ]);
        let git = ScriptedGit::new(vec![Err("injected git lock")]);

        assert_eq!(
            write_created_purpose(&server, &git, &purpose_request("Use the fallback")),
            Some(CreatedPurposeFailure {
                detail: "injected git lock; Herdr token cleanup failed: workspace.report_metadata failed: unavailable: injected cleanup refusal".to_owned(),
                unconfirmed_token: Some("Use the fallback".to_owned()),
            })
        );
        assert_eq!(server.requests.lock().unwrap().len(), 2);
    }

    #[test]
    fn purpose_mirror_emits_initial_changes_and_clear_for_the_exact_branch() {
        let (mut mirror, receiver) = PurposeMirror::recording();
        let mut space = workspace::SessionSpace {
            id: "w-purpose".to_owned(),
            label: "Fixture".to_owned(),
            cwds: vec!["/fixture/repo/worktrees/topic".to_owned()],
            purpose: Some("Initial purpose".to_owned()),
        };
        let checkout = WorkspaceSnapshot {
            home_issues: Default::default(),
            id: "project".to_owned(),
            label: "Fixture".to_owned(),
            path: "/fixture/repo".to_owned(),
            remote_target_id: None,
            expanded: true,
            device_id: "local".to_owned(),
            repo_name: "repo".to_owned(),
            is_git: true,
            default_branch: Some("main".to_owned()),
            branches: vec!["main".to_owned(), "topic/quoted".to_owned()],
            registered: true,
            temporary: false,
            session_workspace_ids: vec!["w-purpose".to_owned()],
            last_activity_unix_ms: None,
            pinned: false,
            checkouts: vec![crate::model::CheckoutSnapshot {
                id: "checkout".to_owned(),
                workspace_id: "project".to_owned(),
                label: "topic/quoted".to_owned(),
                path: "/fixture/repo/worktrees/topic".to_owned(),
                branch: Some("topic/quoted".to_owned()),
                exists: true,
                is_worktree: true,
                ..Default::default()
            }],
            inactive_checkouts: Default::default(),
            removal: Default::default(),
        };

        let suppressed = HashMap::from([(
            workspace::normalized_for_comparison(Path::new("/fixture/repo/worktrees/topic")),
            "Initial purpose".to_owned(),
        )]);
        mirror.sync(
            std::slice::from_ref(&space),
            std::slice::from_ref(&checkout),
            &suppressed,
        );
        assert!(
            receiver.try_recv().is_err(),
            "an unconfirmed creation token is never mirrored back into Git"
        );

        mirror.sync(
            std::slice::from_ref(&space),
            std::slice::from_ref(&checkout),
            &HashMap::new(),
        );
        let PurposeMirrorMessage::Write(initial) = receiver.recv().unwrap() else {
            panic!("initial purpose is a write")
        };
        assert_eq!(initial.workspace_id, "w-purpose");
        assert_eq!(initial.repository_root, "/fixture/repo");
        assert_eq!(initial.branch, "topic/quoted");
        assert_eq!(initial.purpose.as_deref(), Some("Initial purpose"));

        mirror.sync(
            std::slice::from_ref(&space),
            std::slice::from_ref(&checkout),
            &HashMap::new(),
        );
        assert!(
            receiver.try_recv().is_err(),
            "an unchanged token is not rewritten"
        );

        space.purpose = Some("Changed purpose".to_owned());
        mirror.sync(
            std::slice::from_ref(&space),
            std::slice::from_ref(&checkout),
            &HashMap::new(),
        );
        let PurposeMirrorMessage::Write(changed) = receiver.recv().unwrap() else {
            panic!("changed purpose is a write")
        };
        assert_eq!(changed.purpose.as_deref(), Some("Changed purpose"));

        space.purpose = None;
        mirror.sync(
            std::slice::from_ref(&space),
            std::slice::from_ref(&checkout),
            &HashMap::new(),
        );
        let PurposeMirrorMessage::Write(cleared) = receiver.recv().unwrap() else {
            panic!("cleared purpose is a write")
        };
        assert_eq!(cleared.branch, "topic/quoted");
        assert!(cleared.purpose.is_none());
    }

    #[test]
    fn purpose_mirror_uses_the_projects_authoritative_workspace_only() {
        let (mut mirror, receiver) = PurposeMirror::recording();
        let spaces = vec![
            workspace::SessionSpace {
                id: "w-outer".to_owned(),
                label: "Outer".to_owned(),
                cwds: vec!["/fixture/repo/worktrees/topic".to_owned()],
                purpose: Some("Outer purpose".to_owned()),
            },
            workspace::SessionSpace {
                id: "w-nested".to_owned(),
                label: "Nested".to_owned(),
                cwds: vec!["/fixture/repo/worktrees/topic/nested".to_owned()],
                purpose: Some("Nested purpose".to_owned()),
            },
            workspace::SessionSpace {
                id: "w-authority".to_owned(),
                label: "Outer second".to_owned(),
                cwds: vec!["/fixture/repo/worktrees/topic".to_owned()],
                purpose: Some("Authoritative purpose".to_owned()),
            },
        ];
        let project = WorkspaceSnapshot {
            home_issues: Default::default(),
            id: "outer".to_owned(),
            label: "Outer".to_owned(),
            path: "/fixture/repo".to_owned(),
            remote_target_id: None,
            expanded: true,
            device_id: "local".to_owned(),
            repo_name: "repo".to_owned(),
            is_git: true,
            default_branch: Some("main".to_owned()),
            branches: vec!["topic".to_owned()],
            registered: true,
            temporary: false,
            session_workspace_ids: vec!["w-outer".to_owned(), "w-authority".to_owned()],
            last_activity_unix_ms: None,
            pinned: false,
            checkouts: vec![crate::model::CheckoutSnapshot {
                id: "checkout".to_owned(),
                workspace_id: "outer".to_owned(),
                label: "topic".to_owned(),
                path: "/fixture/repo/worktrees/topic".to_owned(),
                branch: Some("topic".to_owned()),
                exists: true,
                is_worktree: true,
                ..Default::default()
            }],
            inactive_checkouts: Default::default(),
            removal: Default::default(),
        };

        mirror.sync(&spaces, std::slice::from_ref(&project), &HashMap::new());

        let PurposeMirrorMessage::Write(write) = receiver.recv().unwrap() else {
            panic!("the authoritative purpose is a write")
        };
        assert_eq!(write.workspace_id, "w-authority");
        assert_eq!(write.purpose.as_deref(), Some("Authoritative purpose"));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn purpose_mirror_tracks_source_switches_by_checkout_and_restores_the_last_value() {
        let (mut mirror, receiver) = PurposeMirror::recording();
        let first = workspace::SessionSpace {
            id: "w-first".to_owned(),
            label: "First".to_owned(),
            cwds: vec!["/fixture/repo/worktrees/topic".to_owned()],
            purpose: Some("First purpose".to_owned()),
        };
        let later_without_token = workspace::SessionSpace {
            id: "w-later".to_owned(),
            label: "Later".to_owned(),
            cwds: vec!["/fixture/repo/worktrees/topic".to_owned()],
            purpose: None,
        };
        let mut project = WorkspaceSnapshot {
            home_issues: Default::default(),
            id: "project".to_owned(),
            label: "Fixture".to_owned(),
            path: "/fixture/repo".to_owned(),
            remote_target_id: None,
            expanded: true,
            device_id: "local".to_owned(),
            repo_name: "repo".to_owned(),
            is_git: true,
            default_branch: Some("main".to_owned()),
            branches: vec!["topic".to_owned()],
            registered: true,
            temporary: false,
            session_workspace_ids: vec!["w-first".to_owned()],
            last_activity_unix_ms: None,
            pinned: false,
            checkouts: vec![crate::model::CheckoutSnapshot {
                id: "checkout".to_owned(),
                workspace_id: "project".to_owned(),
                label: "topic".to_owned(),
                path: "/fixture/repo/worktrees/topic".to_owned(),
                branch: Some("topic".to_owned()),
                exists: true,
                is_worktree: true,
                ..Default::default()
            }],
            inactive_checkouts: Default::default(),
            removal: Default::default(),
        };

        mirror.sync(
            std::slice::from_ref(&first),
            std::slice::from_ref(&project),
            &HashMap::new(),
        );
        let PurposeMirrorMessage::Write(initial) = receiver.recv().unwrap() else {
            panic!("initial purpose is written")
        };
        assert_eq!(initial.workspace_id, "w-first");
        assert_eq!(initial.purpose.as_deref(), Some("First purpose"));

        project.session_workspace_ids.push("w-later".to_owned());
        mirror.sync(
            &[first.clone(), later_without_token],
            std::slice::from_ref(&project),
            &HashMap::new(),
        );
        let PurposeMirrorMessage::Write(cleared) = receiver.recv().unwrap() else {
            panic!("later explicit absence clears the mirror")
        };
        assert_eq!(cleared.workspace_id, "w-later");
        assert!(cleared.purpose.is_none());

        project.session_workspace_ids.pop();
        mirror.sync(
            std::slice::from_ref(&first),
            std::slice::from_ref(&project),
            &HashMap::new(),
        );
        let PurposeMirrorMessage::Write(restored) = receiver.recv().unwrap() else {
            panic!("removing the later workspace restores the earlier value")
        };
        assert_eq!(restored.workspace_id, "w-first");
        assert_eq!(restored.purpose.as_deref(), Some("First purpose"));
    }

    #[test]
    fn refusal_never_authorizes_removal_or_closes_the_next_pane() {
        let server = server(vec![
            json!({"error":{"code":"confirmation_required","message":"close refused"}}),
        ]);
        let result = close_checkout_panes(
            &server,
            &["/fixture/topic".into()],
            &["w1:p1".into(), "w1:p2".into()],
            CONFIRM_TIMEOUT,
        );
        assert!(result.unwrap_err().contains("close refused"));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    }
    #[test]
    fn every_requested_pane_must_disappear_before_removal_is_ready() {
        let server = server(vec![
            json!({"result":{"type":"ok"}}),
            json!({"result":{"type":"ok"}}),
            snapshot(&["w1:p2", "w2:p1"]),
            snapshot(&["w2:p1"]),
        ]);
        close_checkout_panes(
            &server,
            &["/fixture/topic".into()],
            &["w1:p1".into(), "w1:p2".into()],
            CONFIRM_TIMEOUT,
        )
        .unwrap();
        assert!(server.replies.lock().unwrap().is_empty());
        assert_eq!(
            server.requests.lock().unwrap()[1]["params"],
            json!({"pane_id":"w1:p2"})
        );
    }
    #[test]
    fn a_new_pane_at_the_checkout_blocks_removal_authorization() {
        let panes = vec![json!({
            "pane_id":"w2:p1",
            "cwd":"/fixture/topic",
            "foreground_cwd":"/fixture/topic"
        })];
        let (ids, at_checkout) = confirmation_state(&panes, &["/fixture/topic".into()]).unwrap();
        assert_eq!(ids, vec!["w2:p1"]);
        assert!(at_checkout);
    }
    #[test]
    fn malformed_confirmation_cannot_authorize_removal() {
        for invalid in [
            json!({}),
            json!({"type":"session_snapshot","snapshot":{}}),
            json!({"type":"session_snapshot","snapshot":{"panes":[{}]}}),
        ] {
            let server = server(vec![
                json!({"result":{"type":"ok"}}),
                json!({"result":invalid}),
            ]);
            assert!(
                close_checkout_panes(
                    &server,
                    &["/fixture/topic".into()],
                    &["w1:p1".into()],
                    CONFIRM_TIMEOUT
                )
                .is_err()
            );
        }
    }
    #[test]
    fn confirmation_timeout_cannot_authorize_removal() {
        let server = server(vec![json!({"result":{"type":"ok"}}), snapshot(&["w1:p1"])]);
        assert!(
            close_checkout_panes(
                &server,
                &["/fixture/topic".into()],
                &["w1:p1".into()],
                Duration::from_millis(20)
            )
            .unwrap_err()
            .contains("Timed out")
        );
    }
    #[test]
    fn open_uses_repository_context_and_existing_pane_focus_is_exact() {
        let server = server(vec![
            json!({"result":{"type":"ok"}}),
            json!({"result":{"type":"ok"}}),
        ]);
        open_worktree(&server, "/fixture/topic", "/fixture/main", None).unwrap();
        open_worktree(&server, "/fixture/topic", "/fixture/main", Some("w1:p2")).unwrap();
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests[0]["method"], "worktree.open");
        assert_eq!(
            requests[0]["params"],
            json!({"cwd":"/fixture/main","path":"/fixture/topic","focus":true})
        );
        assert_eq!(requests[1]["method"], "pane.focus");
        assert_eq!(requests[1]["params"], json!({"pane_id":"w1:p2"}));
    }
    #[test]
    fn open_preserves_the_server_rejection() {
        let server = server(vec![
            json!({"error":{"code":"not_found","message":"checkout no longer exists"}}),
        ]);
        assert!(
            open_worktree(&server, "/fixture/topic", "/fixture/main", None)
                .unwrap_err()
                .contains("checkout no longer exists")
        );
    }

    #[test]
    fn created_path_mismatch_is_rolled_back() {
        let missing = "/private/tmp/hide-created-path-missing";
        let server = server(vec![
            worktree_created(missing, "feature"),
            worktree_list(missing, "other"),
            json!({"result":{"type":"ok"}}),
            worktree_list(missing, "other"),
        ]);
        let error = create_worktree(
            &server,
            &crate::host_access::InProcessHost,
            true,
            &task("feature"),
        )
        .unwrap_err();
        assert!(error.contains("rolled back"));
        assert!(!Path::new(missing).exists());
        let methods = server
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request["method"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            methods,
            [
                "worktree.create",
                "worktree.list",
                "worktree.remove",
                "worktree.list"
            ]
        );
    }

    /// B27: a host that cannot answer whether the created folder exists
    /// leaves the worktree as created; nothing is rolled back for it.
    #[test]
    fn an_unanswered_folder_check_keeps_the_created_worktree() {
        struct Busy;
        impl crate::host_access::HostChannel for Busy {
            fn call(
                &self,
                _call: hide_host::protocol::Call,
                _timeout: std::time::Duration,
            ) -> Result<crate::host_access::HostAnswer, crate::host_access::HostCallError>
            {
                Err(crate::host_access::HostCallError::Unknown(
                    "The device did not answer in time".to_owned(),
                ))
            }
        }
        let path = "/private/tmp/hide-created-unanswered";
        let server = server(vec![
            worktree_created(path, "feature"),
            worktree_list(path, "feature"),
        ]);
        let error = create_worktree(&server, &Busy, false, &task("feature")).unwrap_err();
        assert!(error.contains("it was kept"), "{error}");
        // No retry of the creation can succeed, so the next step is named.
        assert!(error.contains("start the agent in its pane"), "{error}");
        let methods = server
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request["method"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert!(
            !methods.iter().any(|method| method == "worktree.remove"),
            "{methods:?}"
        );
    }

    #[test]
    fn created_path_alias_is_not_a_mismatch() {
        let root =
            Path::new("/tmp").join(format!("hide-created-path-alias-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let response_path = root.to_string_lossy().into_owned();
        let listed_path = std::fs::canonicalize(&root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let server = server(vec![
            worktree_created(&response_path, "feature"),
            worktree_list(&listed_path, "feature"),
        ]);

        let outcome = create_worktree(
            &server,
            &crate::host_access::InProcessHost,
            true,
            &task("feature"),
        )
        .unwrap();
        assert_eq!(outcome.path, response_path);
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| request["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["worktree.create", "worktree.list"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migrate_branch_blocked_when_dirty() {
        let git = ScriptedGit::new(vec![Ok(" M file")]);
        let server = server(vec![]);
        let error = migrate_branch(&server, &git, &task("feature")).unwrap_err();
        assert!(error.contains("uncommitted changes"));
        assert!(server.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn migrate_branch_moves_branch_to_worktree() {
        let root =
            std::env::temp_dir().join(format!("hide-migrate-success-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.to_string_lossy().into_owned();
        let server = server(vec![
            worktree_created(&path, "feature"),
            worktree_list(&path, "feature"),
        ]);
        let git = ScriptedGit::new(vec![Ok(""), Ok("feature"), Ok("")]);
        let outcome = migrate_branch(&server, &git, &task("feature")).unwrap();
        assert_eq!(outcome.path, path);
        assert_eq!(
            git.calls.lock().unwrap()[2],
            ["checkout", "--no-overwrite-ignore", "main"]
        );
        let requests = server.requests.lock().unwrap();
        let create = requests
            .iter()
            .find(|request| request["method"] == "worktree.create")
            .unwrap();
        assert_eq!(
            create["params"]["focus"], false,
            "migration must not move Herdr focus"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migrate_branch_restores_on_failure() {
        let server = server(vec![
            json!({"error":{"code":"injected","message":"pane creation failed"}}),
        ]);
        let git = ScriptedGit::new(vec![Ok(""), Ok("feature"), Ok(""), Ok("")]);
        let error = migrate_branch(&server, &git, &task("feature")).unwrap_err();
        assert!(error.contains("restored the main worktree to feature"));
        assert_eq!(
            git.calls.lock().unwrap()[3],
            ["checkout", "--no-overwrite-ignore", "feature"]
        );
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| request["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["worktree.create"]
        );
    }

    #[test]
    fn migrate_branch_leaves_panes_and_focus() {
        let git = ScriptedGit::new(vec![Ok(""), Ok("feature"), Ok(""), Ok("")]);
        let server = server(vec![
            json!({"error":{"code":"injected","message":"create failed"}}),
        ]);
        let _ = migrate_branch(&server, &git, &task("feature"));
        assert!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|request| request["method"] != "pane.focus"
                    && request["method"] != "pane.close")
        );
    }

    #[test]
    fn migrate_branch_does_not_remove_an_unowned_worktree_after_create_failure() {
        let server = server(vec![
            json!({"error":{"code":"injected","message":"create failed after another client used the branch"}}),
        ]);
        let git = ScriptedGit::new(vec![Ok(""), Ok("feature"), Ok(""), Ok("")]);

        let error = migrate_branch(&server, &git, &task("feature")).unwrap_err();

        assert!(error.contains("restored the main worktree to feature"));
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| request["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["worktree.create"],
            "an ambiguous create failure must not remove a worktree it cannot prove it owns"
        );
    }

    #[test]
    fn migrate_branch_checkout_failure_leaves_repo_untouched() {
        let git = ScriptedGit::new(vec![
            Ok(""),
            Ok("feature"),
            Err("injected checkout failure"),
        ]);
        let server = server(vec![]);
        let error = migrate_branch(&server, &git, &task("feature")).unwrap_err();
        assert!(error.contains("checkout base branch"));
        assert_eq!(git.calls.lock().unwrap().len(), 3);
        assert!(server.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn migrate_branch_refuses_before_overwriting_an_ignored_file() {
        let root = std::env::temp_dir().join(format!(
            "hide-migrate-ignored-collision-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.name", "Fixture"]);
        run(&["config", "user.email", "fixture@example.invalid"]);
        run(&["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("ignored.txt"), "base bytes").unwrap();
        run(&["add", "ignored.txt"]);
        run(&["commit", "-m", "base"]);
        run(&["checkout", "-b", "feature"]);
        run(&["rm", "ignored.txt"]);
        std::fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        run(&["add", ".gitignore"]);
        run(&["commit", "-m", "ignore path"]);
        std::fs::write(root.join("ignored.txt"), "private ignored bytes").unwrap();

        let server = server(vec![]);
        let request = WorktreeTaskRequest {
            repository_root: root.to_string_lossy().into_owned(),
            branch: "feature".into(),
            base_branch: Some("main".into()),
            ..task("feature")
        };
        let error = migrate_branch(&server, &SystemGit, &request).unwrap_err();

        assert!(error.contains("checkout base branch"));
        assert_eq!(
            std::fs::read_to_string(root.join("ignored.txt")).unwrap(),
            "private ignored bytes"
        );
        let branch = SystemGit
            .run(
                root.to_str().unwrap(),
                &["symbolic-ref", "--quiet", "--short", "HEAD"],
            )
            .unwrap();
        assert_eq!(branch, "feature");
        assert!(server.requests.lock().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migrate_branch_restore_failure_reports_repo_state() {
        let server = server(vec![
            json!({"error":{"code":"injected","message":"create failed"}}),
        ]);
        let git = ScriptedGit::new(vec![
            Ok(""),
            Ok("feature"),
            Ok(""),
            Err("injected restore failure"),
            Ok("main"),
        ]);
        let error = migrate_branch(&server, &git, &task("feature")).unwrap_err();
        assert!(error.contains("main worktree is now on main"));
        assert!(error.contains("open the repository at /fixture/repo and check out feature"));
    }
}
