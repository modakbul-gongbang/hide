//! The Overview's PRs tab (PRD overview-lenses-prs): which of a project's
//! pull requests reach the wire, a pull request linked to an issue, and an
//! agent handed a pull request. Every `gh` call runs on a worker off the
//! lock (`github.rs`); the answers land in `pr_work` or, for an agent's
//! start, in the task-operation slot every start reports through.

use super::*;
use crate::model::{
    CheckoutSnapshot, PrFeedbackSnapshot, PrLinkSnapshot, PullRequestBadge, PullRequestSnapshot,
};

/// How long a merged pull request with no worktree here stays on the tab (D-52).
const RECENT_MERGE_MS: u64 = 14 * 24 * 60 * 60 * 1000;

/// The pull requests a project's PRs tab can draw (D-32, D-52): every open
/// one, and a merged one while a worktree of its branch is recorded here or
/// for 14 days after it merged. A pull request closed without merging never
/// shows.
pub(super) fn shown_pull_requests(
    all: &[PullRequestSnapshot],
    checkouts: &[CheckoutSnapshot],
    now: u64,
) -> Vec<PullRequestSnapshot> {
    let recorded: BTreeSet<&str> = checkouts
        .iter()
        .filter(|checkout| checkout.is_worktree)
        .filter_map(|checkout| checkout.branch.as_deref())
        .collect();
    all.iter()
        .filter(|pull_request| match pull_request.badge {
            PullRequestBadge::Closed => false,
            PullRequestBadge::Merged => {
                recorded.contains(pull_request.head_branch.as_str())
                    || pull_request
                        .merged_at_unix_ms
                        .is_some_and(|at| now.saturating_sub(at) <= RECENT_MERGE_MS)
            }
            PullRequestBadge::Review | PullRequestBadge::Open => true,
        })
        .cloned()
        .collect()
}

/// The worktree checked out on `branch`, the one Hide's issue link lives on.
fn branch_worktree<'a>(
    workspace: &'a WorkspaceSnapshot,
    branch: &str,
) -> Option<&'a CheckoutSnapshot> {
    workspace.checkouts.iter().find(|checkout| {
        checkout.is_worktree && checkout.exists && checkout.branch.as_deref() == Some(branch)
    })
}

impl Runtime {
    /// A local Git project and one of its open pull requests, or why not.
    fn open_pull_request(
        &self,
        workspace_id: &str,
        number: u32,
    ) -> Result<(WorkspaceSnapshot, PullRequestSnapshot), String> {
        let workspace = self
            .local_project_by_id(workspace_id)
            .filter(|workspace| workspace.is_git)
            .ok_or_else(|| "이 Mac의 Git 프로젝트가 아닙니다".to_owned())?;
        let pull_request = workspace
            .pull_requests
            .iter()
            .find(|pull_request| pull_request.number == number && !pull_request.badge.is_settled())
            .cloned()
            .ok_or_else(|| format!("PR #{number}은 열려 있지 않습니다"))?;
        Ok((workspace, pull_request))
    }

    fn fail_pr_link(&mut self, mut link: PrLinkSnapshot, step: &str, message: String) -> bool {
        crate::diagnostic!(serde_json::json!({
            "component": "pull_requests", "kind": "pr_link.failed",
            "request": link.request_id, "pr": link.pr_number, "step": step, "reason": message,
        }));
        link.step = step.into();
        link.phase = "failed".into();
        link.message = Some(message);
        self.snapshot.pr_work.link = Some(link);
        true
    }

    /// `pr_link_issue` (D-13, D-31, D-34): a GitHub issue is linked in Hide
    /// when a worktree holds the pull request's branch, and `Closes #N` is
    /// written into the pull request's body on a worker; a new GitHub issue
    /// is made first, on the same worker. A Local issue, made in the store
    /// first when it is new, is linked in Hide only and GitHub is not written.
    pub(super) fn link_pr_issue(&mut self, payload: PrLinkIssuePayload) -> bool {
        if self
            .snapshot
            .pr_work
            .link
            .as_ref()
            .is_some_and(|slot| slot.phase == "working")
        {
            self.set_error(
                "pr_link.busy",
                "Another pull request is still being linked",
                true,
            );
            return true;
        }
        let link = PrLinkSnapshot {
            request_id: payload.request_id,
            workspace_id: payload.workspace_id,
            pr_number: payload.pr_number,
            step: "link".into(),
            phase: "working".into(),
            issue_key: None,
            issue_id: None,
            created: false,
            message: None,
        };
        let (workspace, pull_request) =
            match self.open_pull_request(&link.workspace_id, link.pr_number) {
                Ok(target) => target,
                Err(message) => return self.fail_pr_link(link, "link", message),
            };
        crate::diagnostic!(serde_json::json!({
            "component": "pull_requests", "kind": "pr_link.requested",
            "request": link.request_id, "project": workspace.path, "pr": link.pr_number,
            "new_issue": payload.new_issue.is_some(),
        }));
        let local = self.project_source_kind(&workspace) == crate::tasks::SourceKind::Local;
        match (payload.issue_key, payload.new_issue) {
            (Some(key), None) if local => self.link_local_pr(link, &workspace, &pull_request, key),
            (Some(key), None) => self.link_github_pr(link, &workspace, &pull_request, &key),
            (None, Some(new)) => {
                let checked = crate::local_issues::validated_title(&new.title).and_then(|title| {
                    crate::local_issues::validated_body(&new.body).map(|body| (title, body))
                });
                let (title, body) = match checked {
                    Ok(checked) => checked,
                    Err(message) => return self.fail_pr_link(link, "create", message),
                };
                if local {
                    return match self.create_local_issue(&workspace.path, &title, &body) {
                        Err(message) => self.fail_pr_link(link, "create", message),
                        Ok(number) => {
                            let link = PrLinkSnapshot {
                                created: true,
                                ..link
                            };
                            let key = crate::tasks::local_key(&workspace.path, number);
                            self.link_local_pr(link, &workspace, &pull_request, key)
                        }
                    };
                }
                let request_id = link.request_id.clone();
                self.snapshot.pr_work.link = Some(PrLinkSnapshot {
                    step: "create".into(),
                    ..link
                });
                let root = PathBuf::from(&workspace.path);
                let number = pull_request.number;
                let spawned = self.spawn_issue_worker(
                    "pr-issue-create",
                    move || {
                        let created = crate::github::create_issue(&root, &title, &body);
                        let written = created.as_ref().ok().map(|issue| {
                            crate::github::write_closing_line(&root, number, issue.reference.number)
                        });
                        (created, written)
                    },
                    move |runtime, (created, written)| {
                        runtime.ingest_pr_issue_created(&request_id, created, written)
                    },
                );
                match spawned {
                    Ok(()) => true,
                    Err(message) => {
                        let link = self.snapshot.pr_work.link.take().expect("slot set above");
                        self.fail_pr_link(link, "create", message)
                    }
                }
            }
            _ => self.fail_pr_link(
                link,
                "link",
                "잇는 이슈 하나 또는 새 이슈 하나를 보내야 합니다".to_owned(),
            ),
        }
    }

    /// A Local issue linked to a pull request: Hide's link on the worktree of
    /// its branch, nothing on GitHub (D-34). The link write's answer settles
    /// the slot (`settle_pr_link_write`).
    fn link_local_pr(
        &mut self,
        mut link: PrLinkSnapshot,
        workspace: &WorkspaceSnapshot,
        pull_request: &PullRequestSnapshot,
        key: String,
    ) -> bool {
        let Some(number) = crate::tasks::parse_local_key(&key)
            .filter(|(path, _)| *path == workspace.path)
            .map(|(_, number)| number)
        else {
            return self.fail_pr_link(
                link,
                "link",
                format!("{key}는 이 프로젝트의 이슈가 아닙니다"),
            );
        };
        let id = crate::local_issues::display_id(number);
        link.issue_key = Some(key);
        link.issue_id = Some(id.clone());
        let Some(checkout) = branch_worktree(workspace, &pull_request.head_branch) else {
            return self.fail_pr_link(
                link,
                "link",
                "이 PR의 브랜치를 가진 워크트리가 없어 Local 이슈를 이을 곳이 없습니다".to_owned(),
            );
        };
        let checkout_id = checkout.id.clone();
        link.step = "link".into();
        self.snapshot.pr_work.link = Some(link);
        // Set before the write starts: a core without a live connection
        // answers the write at once, inside `link_checkout_issue`.
        self.pr_link_checkout = Some(checkout_id.clone());
        if let Err((_, message)) = self.link_checkout_issue(&checkout_id, &id) {
            self.pr_link_checkout = None;
            let link = self.snapshot.pr_work.link.take().expect("slot set above");
            return self.fail_pr_link(link, "link", message);
        }
        true
    }

    /// The Hide link a Local `pr_link_issue` waited on has been written, or
    /// not; a GitHub link is settled by the body write instead.
    pub(super) fn settle_pr_link_write(&mut self, error: Option<String>) {
        let Some(link) = self
            .snapshot
            .pr_work
            .link
            .take_if(|slot| slot.phase == "working" && slot.step == "link")
        else {
            return;
        };
        match error {
            Some(message) => {
                self.fail_pr_link(link, "link", message);
            }
            None => {
                self.snapshot.pr_work.link = Some(PrLinkSnapshot {
                    phase: "ready".into(),
                    ..link
                })
            }
        }
    }

    /// A GitHub issue the project has, linked to a pull request: Hide's link
    /// on the worktree of its branch at once, and `Closes #N` in the body on
    /// a worker. A failed body write keeps the link (D-53).
    fn link_github_pr(
        &mut self,
        mut link: PrLinkSnapshot,
        workspace: &WorkspaceSnapshot,
        pull_request: &PullRequestSnapshot,
        key: &str,
    ) -> bool {
        let reference = crate::tasks::parse_github_key(key).filter(|reference| {
            workspace.home_issues.repository.as_deref() == Some(reference.repository.as_str())
        });
        let Some(reference) = reference else {
            return self.fail_pr_link(link, "link", format!("{key}는 이 저장소의 이슈가 아닙니다"));
        };
        link.issue_key = Some(key.to_owned());
        link.issue_id = Some(format!("#{}", reference.number));
        link.step = "body".into();
        let request_id = link.request_id.clone();
        self.snapshot.pr_work.link = Some(link);
        self.link_pr_branch(workspace, pull_request, &reference);
        let root = PathBuf::from(&workspace.path);
        let number = pull_request.number;
        let issue = reference.number;
        let spawned = self.spawn_issue_worker(
            "pr-body-write",
            move || crate::github::write_closing_line(&root, number, issue),
            move |runtime, written| runtime.finish_pr_body(&request_id, &reference, written),
        );
        if let Err(message) = spawned {
            let link = self.snapshot.pr_work.link.take().expect("slot set above");
            return self.fail_pr_link(link, "body", message);
        }
        true
    }

    /// Hide's link for a GitHub issue on the worktree of the pull request's
    /// branch, when there is one. Its failure is the diagnostic log's: the
    /// body's `Closes` line links the two for GitHub and Hide alike.
    fn link_pr_branch(
        &mut self,
        workspace: &WorkspaceSnapshot,
        pull_request: &PullRequestSnapshot,
        reference: &crate::issues::IssueReference,
    ) {
        let Some(checkout) = branch_worktree(workspace, &pull_request.head_branch) else {
            return;
        };
        let checkout_id = checkout.id.clone();
        if let Err((kind, message)) = self.link_checkout_issue(&checkout_id, &reference.token()) {
            self.push_diagnostic(kind, message);
        }
    }

    pub(crate) fn ingest_pr_issue_created(
        &mut self,
        request_id: &str,
        created: Result<crate::issues::IssueSnapshot, String>,
        written: Option<Result<bool, String>>,
    ) -> bool {
        let Some(link) = self
            .snapshot
            .pr_work
            .link
            .take_if(|slot| slot.request_id == request_id && slot.phase == "working")
        else {
            return false;
        };
        let issue = match created {
            Ok(issue) => issue,
            Err(message) => return self.fail_pr_link(link, "create", message),
        };
        let reference = issue.reference.clone();
        let target = self.open_pull_request(&link.workspace_id, link.pr_number);
        if let Some(workspace) = self.local_project_by_id(&link.workspace_id) {
            self.remember_created_issue(&workspace.path, issue);
        }
        self.snapshot.pr_work.link = Some(PrLinkSnapshot {
            step: "body".into(),
            issue_key: Some(crate::tasks::github_key(&reference)),
            issue_id: Some(format!("#{}", reference.number)),
            created: true,
            ..link
        });
        if let Ok((workspace, pull_request)) = &target {
            self.link_pr_branch(workspace, pull_request, &reference);
        }
        let written = written.unwrap_or_else(|| Err("PR 본문을 쓰지 않았습니다".to_owned()));
        self.finish_pr_body(request_id, &reference, written);
        true
    }

    /// The body write's answer (D-31, D-53): the pull request closes the
    /// issue from now on, shown at once and confirmed by the next read; a
    /// failure keeps the issue and the link and says why, for `본문 다시 쓰기`.
    pub(crate) fn finish_pr_body(
        &mut self,
        request_id: &str,
        reference: &crate::issues::IssueReference,
        written: Result<bool, String>,
    ) -> bool {
        let Some(link) = self
            .snapshot
            .pr_work
            .link
            .take_if(|slot| slot.request_id == request_id && slot.phase == "working")
        else {
            return false;
        };
        let wrote = match written {
            Ok(wrote) => wrote,
            Err(message) => return self.fail_pr_link(link, "body", message),
        };
        crate::diagnostic!(serde_json::json!({
            "component": "pull_requests", "kind": "pr_link.body_written",
            "request": link.request_id, "pr": link.pr_number, "issue": reference.number,
            "already_there": !wrote,
        }));
        if let Some(workspace) = self.local_project_by_id(&link.workspace_id)
            && let Some(project) = self
                .github
                .projects
                .iter_mut()
                .find(|project| project.root_path == workspace.path)
        {
            for pull_request in &mut project.pull_requests {
                if pull_request.number == link.pr_number
                    && !pull_request.closing_issues.contains(reference)
                {
                    pull_request.closing_issues.push(reference.clone());
                }
            }
            self.refresh_pull_requests(&workspace.path);
        }
        self.snapshot.pr_work.link = Some(PrLinkSnapshot {
            phase: "ready".into(),
            ..link
        });
        self.apply_pull_requests();
        true
    }

    /// `pr_feedback_read`: what the pull request's failed checks and change
    /// requests say, for the first prompt of the agent it is handed to (D-46).
    pub(super) fn read_pr_feedback(&mut self, payload: PrFeedbackReadPayload) -> bool {
        let mut slot = PrFeedbackSnapshot {
            request_id: payload.request_id,
            pr_number: payload.pr_number,
            phase: "reading".into(),
            body: None,
            failed_checks: Vec::new(),
            change_requests: Vec::new(),
            message: None,
        };
        let workspace = match self.open_pull_request(&payload.workspace_id, payload.pr_number) {
            Ok((workspace, _)) => workspace,
            Err(message) => {
                slot.phase = "failed".into();
                slot.message = Some(message);
                self.snapshot.pr_work.feedback = Some(slot);
                return true;
            }
        };
        let request_id = slot.request_id.clone();
        self.snapshot.pr_work.feedback = Some(slot);
        let root = PathBuf::from(&workspace.path);
        let number = payload.pr_number;
        let answered = request_id.clone();
        if let Err(message) = self.spawn_issue_worker(
            "pr-feedback",
            move || crate::github::pr_feedback(&root, number),
            move |runtime, answer| runtime.ingest_pr_feedback(&answered, answer),
        ) {
            self.ingest_pr_feedback(&request_id, Err(message));
        }
        true
    }

    pub(crate) fn ingest_pr_feedback(
        &mut self,
        request_id: &str,
        answer: Result<crate::github::PrFeedback, String>,
    ) -> bool {
        let Some(slot) = self
            .snapshot
            .pr_work
            .feedback
            .as_mut()
            .filter(|slot| slot.request_id == request_id && slot.phase == "reading")
        else {
            return false;
        };
        match answer {
            Ok(feedback) => {
                slot.phase = "ready".into();
                slot.body = Some(feedback.body);
                slot.failed_checks = feedback.failed_checks;
                slot.change_requests = feedback.change_requests;
            }
            Err(message) => {
                crate::diagnostic!(serde_json::json!({
                    "component": "pull_requests", "kind": "pr_feedback.failed",
                    "request": request_id, "pr": slot.pr_number, "reason": message,
                }));
                slot.phase = "failed".into();
                slot.message = Some(message);
            }
        }
        true
    }

    /// `pr_delegate` (D-12, D-46): the agent starts in the checkout of the
    /// pull request's branch with the operator's first prompt, or, with no
    /// checkout here, in a new worktree of that existing branch. Both report
    /// through the task-operation slot, as every start does.
    pub(super) fn delegate_pr(&mut self, payload: PrDelegatePayload) -> bool {
        if !matches!(payload.provider.as_str(), "claude" | "codex") {
            self.set_error(
                "pr_delegate.unknown_provider",
                format!("No agent provider named {}", payload.provider),
                false,
            );
            return true;
        }
        let (workspace, pull_request) =
            match self.open_pull_request(&payload.workspace_id, payload.pr_number) {
                Ok(target) => target,
                Err(message) => {
                    self.set_error("pr_delegate.unknown_pull_request", message, false);
                    return true;
                }
            };
        let prompt = Some(payload.prompt.trim().to_owned()).filter(|prompt| !prompt.is_empty());
        let checkout = workspace.checkouts.iter().find(|checkout| {
            checkout.exists && checkout.branch.as_deref() == Some(pull_request.head_branch.as_str())
        });
        crate::diagnostic!(serde_json::json!({
            "component": "pull_requests", "kind": "pr_delegate.requested",
            "project": workspace.path, "pr": pull_request.number,
            "checkout": checkout.map(|checkout| checkout.id.clone()),
        }));
        match checkout {
            Some(checkout) => self.agent_start_in_checkout(AgentStartInCheckoutPayload {
                checkout_path: checkout.path.clone(),
                provider: payload.provider,
                prompt,
                model: payload.model,
                request_id: None,
            }),
            None => self.start_worktree_task(
                CreateWorktreePayload {
                    device_id: None,
                    repository_root: workspace.path.clone(),
                    branch: pull_request.head_branch.clone(),
                    base_branch: None,
                    agent_kind: Some(payload.provider),
                    purpose: None,
                    task_key: None,
                    prompt,
                    model: payload.model,
                },
                true,
            ),
        }
    }
}
