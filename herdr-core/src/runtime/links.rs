//! What the runtime hands the link worker, and what it publishes back
//! (PRD link-graph D-27, D-45).
//!
//! Facts leave the lock as owned copies, and only when a cheap fingerprint
//! of them moved: this runs inside `apply_pull_requests`, which every Herdr
//! session ingest reaches. Answers come back under a short lock and change
//! the snapshot only when they differ, each on its own delta revision.

use super::*;
use crate::links::worker::{
    LinkClient, LinkWorker, PanelAnswer, PanelRequest, PanelTarget, Paths, Sink,
};
use crate::links::{
    IssueSource, LinkPanelSnapshot, LinkSummariesSnapshot, LinkTarget, LinkedPr, PaneFact,
    ParentFact, PrFact, ProjectFacts, ProjectLinkSummary, WorktreeFact,
};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

#[derive(Default)]
pub(super) struct LinksWork {
    client: Option<LinkClient>,
    projects_print: Option<u64>,
    panes: Arc<Vec<PaneFact>>,
    parents: Arc<Vec<ParentFact>>,
    generation: u64,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LinksOpenPayload {
    pub workspace_id: String,
    pub target: LinkTarget,
}

struct RuntimeSink {
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
}

impl RuntimeSink {
    fn apply(&self, change: impl FnOnce(&mut Runtime) -> bool) {
        let Some(runtime) = self.runtime.upgrade() else {
            return;
        };
        let changed = match runtime.lock() {
            Ok(mut guard) => change(&mut guard),
            Err(_) => return,
        };
        drop(runtime);
        if changed {
            self.notifier.notify();
        }
    }
}

impl Sink for RuntimeSink {
    fn summaries(&self, summaries: BTreeMap<String, ProjectLinkSummary>) {
        self.apply(move |runtime| runtime.ingest_link_summaries(summaries));
    }

    fn panel(&self, generation: u64, answer: Result<PanelAnswer, String>) {
        self.apply(move |runtime| runtime.ingest_link_panel(generation, answer));
    }

    fn filling(&self, filling: bool) {
        self.apply(move |runtime| runtime.ingest_link_filling(filling));
    }

    fn devices(&self) -> Vec<(String, Arc<dyn crate::node_access::NodeLink>)> {
        self.runtime
            .upgrade()
            .and_then(|runtime| {
                runtime
                    .lock()
                    .ok()
                    .map(|runtime| runtime.ready_device_channels())
            })
            .unwrap_or_default()
    }
}

/// Starts the worker beside the runtime that owns its answers.
pub(crate) fn spawn_worker(
    runtime: &Arc<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    home: Option<PathBuf>,
) -> Result<LinkWorker, String> {
    let (state_path, node) = {
        let runtime = runtime.lock().map_err(|_| "runtime lock poisoned")?;
        (runtime.state_path.clone(), runtime.node.to_string())
    };
    let directory = state_path
        .parent()
        .filter(|directory| !directory.as_os_str().is_empty())
        .ok_or("the state folder is unknown")?;
    let paths = Paths {
        store: hide_kit::layout::links_store(directory),
        search: state_path.with_file_name("session-search.sqlite3"),
        home,
        local_device: node,
    };
    let worker = LinkWorker::spawn(
        paths,
        RuntimeSink {
            runtime: Arc::downgrade(runtime),
            notifier,
        },
    )?;
    let mut guard = runtime.lock().map_err(|_| "runtime lock poisoned")?;
    guard.links_work.client = Some(worker.client());
    guard.feed_links();
    guard.feed_link_parents();
    Ok(worker)
}

/// A session file's agent name for a Hide agent kind.
fn session_agent(kind: &str) -> Option<&'static str> {
    match kind.to_ascii_lowercase().as_str() {
        "claude" | "claude-code" => Some("claude"),
        "codex" => Some("codex"),
        "opencode" => Some("opencode"),
        _ => None,
    }
}

impl Runtime {
    /// Hands the worker the projects and the panes' sessions when either
    /// moved since the last time.
    pub(super) fn feed_links(&mut self) {
        let Some(client) = self.links_work.client.clone() else {
            return;
        };
        let print = self.link_projects_print();
        if self.links_work.projects_print != Some(print) {
            self.links_work.projects_print = Some(print);
            client.projects(Arc::new(self.link_projects()));
        }
        let panes = self.link_panes();
        if *self.links_work.panes != panes {
            self.links_work.panes = Arc::new(panes);
            client.panes(Arc::clone(&self.links_work.panes));
        }
    }

    /// What the project facts are built from, without building them.
    fn link_projects_print(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for workspace in &self.snapshot.navigator.workspaces {
            (&workspace.id, &workspace.path, &workspace.device_id).hash(&mut hasher);
            for checkout in &workspace.checkouts {
                (
                    &checkout.path,
                    &checkout.branch,
                    &checkout.task_key,
                    checkout
                        .worktree
                        .as_ref()
                        .and_then(|w| w.created_at_unix_ms),
                )
                    .hash(&mut hasher);
            }
            if let Some(project) = self.github.project(&workspace.path) {
                (
                    project.pull_requests_read,
                    project.status.last_success_at_unix_ms,
                    project.pull_requests.len(),
                    &project.issues.repository,
                    &project.issues.repository_id,
                )
                    .hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    pub(super) fn link_projects(&self) -> Vec<ProjectFacts> {
        self.snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.is_git && !workspace.is_home && !workspace.temporary)
            .map(|workspace| {
                let project = self.github.project(&workspace.path);
                let prs = project
                    .map(|project| {
                        project
                            .pull_requests
                            .iter()
                            .filter_map(|pr| link_pr(pr, workspace))
                            .collect()
                    })
                    .unwrap_or_default();
                ProjectFacts {
                    key: hide_project::project_id(&workspace.device_id, Path::new(&workspace.path)),
                    device_id: workspace.device_id.clone(),
                    workspace_id: workspace.id.clone(),
                    root: workspace.path.clone(),
                    repository: project
                        .and_then(|project| project.issues.repository.as_deref())
                        .map(str::to_ascii_lowercase),
                    repository_id: project.and_then(|project| project.issues.repository_id.clone()),
                    worktrees: workspace
                        .checkouts
                        .iter()
                        .map(|checkout| WorktreeFact {
                            path: checkout.path.clone(),
                            branch: checkout.branch.clone(),
                            created_at_unix_ms: checkout
                                .worktree
                                .as_ref()
                                .and_then(|worktree| worktree.created_at_unix_ms),
                        })
                        .collect(),
                    prs,
                    prs_read: project.is_some_and(|project| project.pull_requests_read),
                }
            })
            .collect()
    }

    /// The agent sessions Hide panes carry now, with each pane's folder and
    /// its checkout's branch.
    pub(super) fn link_panes(&self) -> Vec<PaneFact> {
        let agents = self
            .snapshot
            .navigator
            .agents
            .iter()
            .filter_map(|agent| {
                let session = agent.session_id.as_deref()?;
                let kind = session_agent(&agent.agent_kind)?;
                Some((agent.pane_id.as_str(), (kind, session)))
            })
            .collect::<HashMap<_, _>>();
        let mut panes = Vec::new();
        for workspace in &self.snapshot.navigator.workspaces {
            for checkout in &workspace.checkouts {
                for pane in checkout.tabs.iter().flat_map(|tab| tab.panes.iter()) {
                    let Some((kind, session)) = agents.get(pane.id.as_str()) else {
                        continue;
                    };
                    panes.push(PaneFact {
                        device_id: workspace.device_id.clone(),
                        agent: (*kind).to_owned(),
                        session_id: (*session).to_owned(),
                        cwd: if pane.cwd.is_empty() {
                            checkout.path.clone()
                        } else {
                            pane.cwd.clone()
                        },
                        branch: checkout
                            .branch
                            .clone()
                            .filter(|branch| hide_session::links::is_branch(branch)),
                    });
                }
            }
        }
        panes.sort_by(|left, right| {
            (&left.device_id, &left.session_id).cmp(&(&right.device_id, &right.session_id))
        });
        panes.dedup_by(|left, right| {
            left.device_id == right.device_id && left.session_id == right.session_id
        });
        panes
    }

    /// Which registered session `hide agent spawn` started for which, from
    /// the delivery ledger (D-27).
    pub(super) fn feed_link_parents(&mut self) {
        let Some(client) = self.links_work.client.clone() else {
            return;
        };
        let Ok(ledger) = self.delivery_ledger.as_ref() else {
            return;
        };
        let records = ledger
            .agents
            .iter()
            .map(|record| (record.id.as_str(), record))
            .collect::<HashMap<_, _>>();
        let mut parents = ledger
            .agents
            .iter()
            .filter_map(|record| {
                let parent = records.get(record.parent.as_deref()?)?;
                if record.session.is_empty() || parent.session.is_empty() {
                    return None;
                }
                Some(ParentFact {
                    device_id: record.actor.device_id.clone(),
                    agent: session_agent(&record.actor.kind)?.to_owned(),
                    session_id: record.session.clone(),
                    parent_agent: session_agent(&parent.actor.kind)?.to_owned(),
                    parent_session_id: parent.session.clone(),
                    parent_name: parent.name.clone(),
                })
            })
            .collect::<Vec<_>>();
        parents.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        if *self.links_work.parents != parents {
            self.links_work.parents = Arc::new(parents);
            client.parents(Arc::clone(&self.links_work.parents));
        }
    }

    /// The Projects `hide links` may read, and which one holds the caller's
    /// checkout; none when the record has no home.
    pub(crate) fn links_scope(
        &self,
        context: &crate::workspace_control::Context,
    ) -> Option<crate::links::query::Scope> {
        let directory = self
            .state_path
            .parent()
            .filter(|directory| !directory.as_os_str().is_empty())?;
        let projects = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .filter(|workspace| workspace.is_git && !workspace.is_home && !workspace.temporary)
            .map(|workspace| crate::links::query::ScopeProject {
                key: hide_project::project_id(&workspace.device_id, Path::new(&workspace.path)),
                workspace_id: workspace.id.clone(),
                label: workspace.label.clone(),
                root: workspace.path.clone(),
                device_id: workspace.device_id.clone(),
                checkouts: workspace
                    .checkouts
                    .iter()
                    .map(|checkout| checkout.path.clone())
                    .collect(),
            })
            .collect::<Vec<_>>();
        let caller = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| {
                workspace.device_id == context.device_id
                    && (workspace.id == context.workspace_id
                        || workspace
                            .checkouts
                            .iter()
                            .any(|checkout| checkout.path == context.checkout_path))
            })
            .map(|workspace| {
                hide_project::project_id(&workspace.device_id, Path::new(&workspace.path))
            })
            .unwrap_or_default();
        Some(crate::links::query::Scope {
            store: hide_kit::layout::links_store(directory),
            local_device: self.node.to_string(),
            caller,
            projects,
        })
    }

    /// `links_open`: reads the record for one pull request or issue of a
    /// project into the `link_panel` section; reading the same one again is
    /// the failure line's Retry (B25).
    pub(super) fn open_links(&mut self, payload: LinksOpenPayload) -> bool {
        let Some(workspace) = self
            .snapshot
            .navigator
            .workspaces
            .iter()
            .find(|workspace| workspace.id == payload.workspace_id)
        else {
            return false;
        };
        let key = hide_project::project_id(&workspace.device_id, Path::new(&workspace.path));
        self.links_work.generation += 1;
        let generation = self.links_work.generation;
        let same = self.snapshot.link_panel.as_ref().is_some_and(|panel| {
            panel.workspace_id == payload.workspace_id && panel.target == payload.target
        });
        let mut panel = match self.snapshot.link_panel.take() {
            // What the same target showed stays while it is read again (B24).
            Some(panel) if same => panel,
            _ => LinkPanelSnapshot {
                workspace_id: payload.workspace_id.clone(),
                target: payload.target.clone(),
                ..LinkPanelSnapshot::default()
            },
        };
        panel.loading = true;
        panel.failure = None;
        let Some(client) = self.links_work.client.as_ref() else {
            panel.loading = false;
            panel.failure = Some("links_worker_unavailable".to_owned());
            self.snapshot.link_panel = Some(panel);
            return true;
        };
        client.read(PanelRequest {
            generation,
            project: key,
            target: match &payload.target {
                LinkTarget::Pr { number } => PanelTarget::Pr(*number),
                LinkTarget::Issue { key } => PanelTarget::Issue(key.clone()),
            },
        });
        self.snapshot.link_panel = Some(panel);
        true
    }

    #[cfg(test)]
    pub(super) fn links_generation(&self) -> u64 {
        self.links_work.generation
    }

    /// `links_close`: the panel closed, so its read stops following writes.
    pub(super) fn close_links(&mut self) -> bool {
        self.links_work.generation += 1;
        if let Some(client) = self.links_work.client.as_ref() {
            client.close_panel();
        }
        false
    }

    pub(super) fn ingest_link_panel(
        &mut self,
        generation: u64,
        answer: Result<PanelAnswer, String>,
    ) -> bool {
        if generation != self.links_work.generation {
            return false;
        }
        let Some(held) = self.snapshot.link_panel.as_ref() else {
            return false;
        };
        let mut panel = LinkPanelSnapshot {
            workspace_id: held.workspace_id.clone(),
            target: held.target.clone(),
            ..LinkPanelSnapshot::default()
        };
        match answer {
            Err(code) => {
                // The lines read before stay under the failure line.
                panel.pr = held.pr.clone();
                panel.prs = held.prs.clone();
                panel.sessions = held.sessions.clone();
                panel.total = held.total;
                panel.failure = Some(code);
            }
            Ok(PanelAnswer::Pr(links)) => {
                if let Some(links) = links {
                    panel.pr = Some(LinkedPr {
                        number: links.pr.number,
                        branch: links.pr.branch,
                        title: links.pr.title,
                        url: links.pr.url,
                        created_at_unix_ms: links.pr.created_at,
                        closed_at_unix_ms: links.pr.closed_at,
                        merged_at_unix_ms: links.pr.merged_at,
                        issues: links.issues,
                        worktrees: links.worktrees,
                    });
                    panel.sessions = links.sessions;
                    panel.total = links.total;
                }
            }
            Ok(PanelAnswer::Issue(links)) => {
                panel.prs = links.prs;
                panel.sessions = links.sessions;
                panel.total = links.total;
            }
        }
        if self.snapshot.link_panel.as_ref() == Some(&panel) {
            return false;
        }
        self.snapshot.link_panel = Some(panel);
        true
    }

    pub(super) fn ingest_link_summaries(
        &mut self,
        projects: BTreeMap<String, ProjectLinkSummary>,
    ) -> bool {
        let summaries = self
            .snapshot
            .link_summaries
            .get_or_insert_with(LinkSummariesSnapshot::default);
        if summaries.projects == projects {
            return false;
        }
        for workspace in &mut self.snapshot.navigator.workspaces {
            associate_landed(workspace, projects.get(&workspace.id));
        }
        summaries.projects = projects;
        self.refresh_inactive_groups();
        true
    }

    pub(super) fn ingest_link_filling(&mut self, filling: bool) -> bool {
        let summaries = self
            .snapshot
            .link_summaries
            .get_or_insert_with(LinkSummariesSnapshot::default);
        if summaries.filling == filling {
            return false;
        }
        summaries.filling = filling;
        true
    }
}

/// Decides each checkout's `landed` from Git and the project's record: its
/// HEAD is in the base, and the record names it among the checkouts whose
/// work landed. With no record (still filling, Copied history Off, a device
/// with no pull requests read) nothing is landed, so a checkout with no
/// commits of its own is never drawn as merged. Runs on every catalog
/// projection, so it compares before it writes.
pub(super) fn associate_landed(
    workspace: &mut WorkspaceSnapshot,
    summary: Option<&ProjectLinkSummary>,
) -> bool {
    let mut changed = false;
    for checkout in &mut workspace.checkouts {
        let landed = checkout
            .worktree
            .as_ref()
            .is_some_and(|worktree| worktree.merged == Some(true))
            && summary.is_some_and(|summary| summary.landed.contains(&checkout.path));
        if checkout.landed != landed {
            checkout.landed = landed;
            changed = true;
        }
    }
    changed
}

/// A pull request GitHub answered, with the issues it is linked to now.
fn link_pr(
    pr: &crate::model::PullRequestSnapshot,
    workspace: &WorkspaceSnapshot,
) -> Option<PrFact> {
    let repository = crate::links::repository_of(&pr.url)?;
    if pr.cross_repository || !hide_session::links::is_branch(&pr.head_branch) {
        return None;
    }
    let mut issues = pr
        .closing_issues
        .iter()
        .map(|reference| (crate::tasks::github_key(reference), IssueSource::Closes))
        .collect::<Vec<_>>();
    let checkout = workspace
        .checkouts
        .iter()
        .find(|checkout| checkout.branch.as_deref() == Some(pr.head_branch.as_str()));
    if let Some(key) = checkout.and_then(|checkout| checkout.task_key.clone())
        && !issues.iter().any(|(held, _)| *held == key)
    {
        issues.push((key, IssueSource::Hide));
    }
    Some(PrFact {
        repository,
        number: u64::from(pr.number),
        branch: pr.head_branch.clone(),
        title: pr.title.clone(),
        url: pr.url.clone(),
        created_at: pr.created_at_unix_ms,
        closed_at: pr.closed_at_unix_ms,
        merged_at: pr.merged_at_unix_ms,
        issues,
        hide_issue_known: checkout.is_some(),
    })
}
