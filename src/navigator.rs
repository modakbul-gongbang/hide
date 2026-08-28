use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::domain::{DomainProjection, SurfaceKind};
use crate::presentation::{AgentPresentationStore, phase_label};

pub const NAVIGATOR_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigatorView {
    Workspaces,
    Agents,
    Worktrees,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ViewState {
    pub selected: Option<String>,
    pub expanded: BTreeSet<String>,
    #[serde(default)]
    pub collapsed: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NavigatorPreferences {
    pub schema_version: u32,
    pub active_view: NavigatorView,
    pub views: BTreeMap<NavigatorView, ViewState>,
}

impl Default for NavigatorPreferences {
    fn default() -> Self {
        Self {
            schema_version: NAVIGATOR_SCHEMA_VERSION,
            active_view: NavigatorView::Workspaces,
            views: [
                (NavigatorView::Workspaces, ViewState::default()),
                (NavigatorView::Agents, ViewState::default()),
                (NavigatorView::Worktrees, ViewState::default()),
            ]
            .into_iter()
            .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct NavigatorController {
    path: PathBuf,
    preferences: NavigatorPreferences,
}

impl NavigatorController {
    pub fn in_memory() -> Self {
        Self {
            path: PathBuf::new(),
            preferences: NavigatorPreferences::default(),
        }
    }

    pub fn load_or_default(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let preferences = match std::fs::read(&path) {
            Ok(bytes) => {
                let parsed: NavigatorPreferences = serde_json::from_slice(&bytes)
                    .with_context(|| format!("stage=navigator.decode path={}", path.display()))?;
                if parsed.schema_version != NAVIGATOR_SCHEMA_VERSION {
                    anyhow::bail!(
                        "stage=navigator.decode path={} cause=unsupported-schema expected={} received={}",
                        path.display(),
                        NAVIGATOR_SCHEMA_VERSION,
                        parsed.schema_version
                    );
                }
                parsed
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                NavigatorPreferences::default()
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("stage=navigator.read path={}", path.display()));
            }
        };
        Ok(Self { path, preferences })
    }

    pub fn preferences(&self) -> &NavigatorPreferences {
        &self.preferences
    }

    pub fn select_view(&mut self, view: NavigatorView) {
        self.preferences.active_view = view;
    }

    pub fn select(&mut self, key: impl Into<String>) {
        self.active_state_mut().selected = Some(key.into());
    }

    pub fn set_expanded(&mut self, key: impl Into<String>, expanded: bool) {
        let key = key.into();
        let state = self.active_state_mut();
        if expanded {
            state.expanded.insert(key.clone());
            state.collapsed.remove(&key);
        } else {
            state.expanded.remove(&key);
            state.collapsed.insert(key);
        }
    }

    pub fn save(&self) -> Result<()> {
        if self.path.as_os_str().is_empty() {
            anyhow::bail!("stage=navigator.save cause=no-persistence-path");
        }
        write_json_atomic(&self.path, &self.preferences)
    }

    fn active_state_mut(&mut self) -> &mut ViewState {
        self.preferences
            .views
            .entry(self.preferences.active_view)
            .or_default()
    }
}

/// Build the stable, server-identity keyed sidebar tree used by the native shell.
///
/// The projection is deliberately the only source of labels and IDs. This keeps the
/// Workspaces, Agents, Worktrees and Pet surfaces in sync and prevents a stale UI-only
/// hierarchy from surviving a Herdr snapshot replacement.
pub fn render_navigator(
    view: NavigatorView,
    projection: &DomainProjection,
    agents: &AgentPresentationStore,
    preferences: &NavigatorPreferences,
) -> String {
    let state = preferences.views.get(&view).cloned().unwrap_or_default();
    let mut lines = vec![format!("{}", view_label(view))];
    match view {
        NavigatorView::Workspaces => render_workspaces(&mut lines, projection, &state),
        NavigatorView::Agents => render_agents(&mut lines, agents, &state),
        NavigatorView::Worktrees => render_worktrees(&mut lines, projection, &state),
    }
    lines.join("\n")
}

fn view_label(view: NavigatorView) -> &'static str {
    match view {
        NavigatorView::Workspaces => "WORKSPACES",
        NavigatorView::Agents => "AGENTS",
        NavigatorView::Worktrees => "WORKTREES",
    }
}

fn render_workspaces(lines: &mut Vec<String>, projection: &DomainProjection, state: &ViewState) {
    let mut last_host = None::<String>;
    for workspace in projection.workspaces() {
        let host_key = host_key(&workspace.host.host_id, &workspace.host.session_id);
        if last_host.as_deref() != Some(host_key.as_str()) {
            let host_open = is_open(state, &format!("host:{host_key}"));
            lines.push(row(
                0,
                &format!("host:{host_key}"),
                &format!("{} ({})", workspace.host.host_id, workspace.host.session_id),
                state,
                host_open,
                true,
            ));
            last_host = Some(host_key.clone());
        }
        if !is_open(state, &format!("host:{host_key}")) {
            continue;
        }
        let workspace_key = format!("workspace:{host_key}:{}", workspace.workspace_id);
        let workspace_open = is_open(state, &workspace_key);
        let remote = if workspace.remote { " [remote]" } else { "" };
        lines.push(row(
            1,
            &workspace_key,
            &format!("{}{}", workspace.name, remote),
            state,
            workspace_open,
            true,
        ));
        if !workspace_open {
            continue;
        }
        for tab in &workspace.tabs {
            let tab_key = format!("tab:{host_key}:{}:{}", workspace.workspace_id, tab.tab_id);
            let tab_open = is_open(state, &tab_key);
            let active = if tab.tab_id == workspace.active_tab_id {
                " *"
            } else {
                ""
            };
            lines.push(row(
                2,
                &tab_key,
                &format!("{}{}", tab.name, active),
                state,
                tab_open,
                true,
            ));
            if !tab_open {
                continue;
            }
            for pane in &tab.panes {
                let pane_key = format!(
                    "pane:{host_key}:{}:{}:{}",
                    workspace.workspace_id, tab.tab_id, pane.pane_id
                );
                let focused = pane.pane_id == tab.focused_pane_id;
                let surface = match pane.surface {
                    SurfaceKind::Terminal => "terminal",
                    SurfaceKind::Editor => "editor",
                    SurfaceKind::Browser => "browser",
                };
                let focus_marker = if focused { " *" } else { "" };
                lines.push(row(
                    3,
                    &pane_key,
                    &format!("{} [{}]{}", pane.title, surface, focus_marker),
                    state,
                    false,
                    false,
                ));
            }
        }
    }
    if lines.len() == 1 {
        lines.push("  No Herdr workspaces".to_owned());
    }
}

fn render_agents(lines: &mut Vec<String>, agents: &AgentPresentationStore, state: &ViewState) {
    let mut grouped =
        BTreeMap::<(String, String), Vec<&crate::presentation::AgentPresentation>>::new();
    for agent in agents.iter() {
        grouped
            .entry((agent.host.clone(), agent.workspace_id.clone()))
            .or_default()
            .push(agent);
    }
    for ((host, workspace), mut entries) in grouped {
        entries.sort_by(|left, right| left.stable_id.cmp(&right.stable_id));
        let group_key = format!("agents:{host}:{workspace}");
        let group_open = is_open(state, &group_key);
        lines.push(row(
            0,
            &group_key,
            &format!("{host} / {workspace}"),
            state,
            group_open,
            true,
        ));
        if !group_open {
            continue;
        }
        let ids = entries
            .iter()
            .map(|agent| agent.stable_id.clone())
            .collect::<BTreeSet<_>>();
        let mut rendered = BTreeSet::new();
        for agent in entries.iter().filter(|agent| {
            agent
                .parent_id
                .as_ref()
                .is_none_or(|parent| !ids.contains(parent))
        }) {
            render_agent_branch(lines, agent, &entries, state, 1, &mut rendered);
        }
        for agent in entries {
            if !rendered.contains(&agent.stable_id) {
                render_agent_branch(lines, agent, &[], state, 1, &mut rendered);
            }
        }
    }
    if lines.len() == 1 {
        lines.push("  No authoritative agents".to_owned());
    }
}

fn render_agent_branch(
    lines: &mut Vec<String>,
    agent: &crate::presentation::AgentPresentation,
    entries: &[&crate::presentation::AgentPresentation],
    state: &ViewState,
    depth: usize,
    rendered: &mut BTreeSet<String>,
) {
    if !rendered.insert(agent.stable_id.clone()) {
        return;
    }
    let key = format!("agent:{}:{}", agent.host, agent.stable_id);
    let children = entries
        .iter()
        .filter(|candidate| candidate.parent_id.as_deref() == Some(agent.stable_id.as_str()))
        .copied()
        .collect::<Vec<_>>();
    let open = is_open(state, &key);
    let parent_hint = if agent.parent_id.is_some() {
        " child"
    } else {
        ""
    };
    lines.push(row(
        depth,
        &key,
        &format!(
            "{} {}{} - {} - {}s - {}",
            agent.logo.marker(),
            agent.name,
            parent_hint,
            phase_label(agent.phase),
            agent.elapsed_seconds,
            agent.summary
        ),
        state,
        open,
        !children.is_empty(),
    ));
    if !open {
        return;
    }
    for child in children {
        render_agent_branch(lines, child, entries, state, depth + 1, rendered);
    }
}

fn render_worktrees(lines: &mut Vec<String>, projection: &DomainProjection, state: &ViewState) {
    let mut grouped = BTreeMap::<
        String,
        Vec<(
            &crate::domain::WorkspaceProjection,
            &crate::domain::WorktreeProjection,
        )>,
    >::new();
    for (workspace, worktree) in projection.worktrees() {
        grouped
            .entry(worktree.repo_key.clone())
            .or_default()
            .push((workspace, worktree));
    }
    for (repo_key, mut entries) in grouped {
        entries.sort_by(|left, right| left.1.checkout_path.cmp(&right.1.checkout_path));
        let repo_name = entries
            .first()
            .map(|(_, worktree)| worktree.repo_name.as_str())
            .unwrap_or("Repository");
        let repo_row_key = format!("repo:{repo_key}");
        let repo_open = is_open(state, &repo_row_key);
        lines.push(row(
            0,
            &repo_row_key,
            &format!("{repo_name} ({repo_key})"),
            state,
            repo_open,
            true,
        ));
        if !repo_open {
            continue;
        }
        for (workspace, worktree) in entries {
            let key = format!("worktree:{repo_key}:{}", worktree.checkout_path);
            let linked = if worktree.is_linked_worktree {
                " [linked]"
            } else {
                ""
            };
            let remote = if workspace.remote { " [remote]" } else { "" };
            lines.push(row(
                1,
                &key,
                &format!(
                    "{}{} -> {}{}",
                    worktree.checkout_path, linked, workspace.name, remote
                ),
                state,
                false,
                false,
            ));
        }
    }
    if lines.len() == 1 {
        lines.push("  No worktrees reported".to_owned());
    }
}

fn host_key(host_id: &str, session_id: &str) -> String {
    format!("{host_id}/{session_id}")
}

fn is_open(state: &ViewState, key: &str) -> bool {
    if state.collapsed.contains(key) {
        return false;
    }
    state.expanded.is_empty() || state.expanded.contains(key)
}

fn row(
    depth: usize,
    key: &str,
    label: &str,
    state: &ViewState,
    open: bool,
    has_children: bool,
) -> String {
    let selected = state.selected.as_deref() == Some(key);
    let selection = if selected { "▸" } else { " " };
    let disclosure = if has_children {
        if open { "▾" } else { "▸" }
    } else {
        "·"
    };
    format!(
        "{}{} {} {}",
        "  ".repeat(depth + 1),
        selection,
        disclosure,
        label
    )
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("stage=navigator.mkdir path={}", parent.display()))?;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(value).context("stage=navigator.encode")?;
    std::fs::write(&temporary, bytes)
        .with_context(|| format!("stage=navigator.write path={}", temporary.display()))?;
    std::fs::rename(&temporary, path).with_context(|| {
        format!(
            "stage=navigator.publish from={} to={}",
            temporary.display(),
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "herdr-ide-t4-{name}-{}-navigator.json",
            std::process::id()
        ))
    }

    #[test]
    fn state_persists_independently_per_view() {
        let path = fixture_path("persist");
        let _ = std::fs::remove_file(&path);
        let mut navigator = NavigatorController::load_or_default(&path).unwrap();
        navigator.select("workspace:local");
        navigator.set_expanded("workspace:local", true);
        navigator.select_view(NavigatorView::Agents);
        navigator.select("agent:stable-1");
        navigator.set_expanded("agent:stable-1", true);
        navigator.save().unwrap();

        let restored = NavigatorController::load_or_default(&path).unwrap();
        assert_eq!(restored.preferences().active_view, NavigatorView::Agents);
        assert_eq!(
            restored.preferences().views[&NavigatorView::Workspaces].selected,
            Some("workspace:local".to_owned())
        );
        assert!(
            restored.preferences().views[&NavigatorView::Agents]
                .expanded
                .contains("agent:stable-1")
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn repeated_save_converges_on_one_valid_document() {
        let path = fixture_path("repeat");
        let _ = std::fs::remove_file(&path);
        let mut navigator = NavigatorController::load_or_default(&path).unwrap();
        navigator.select("workspace:one");
        navigator.save().unwrap();
        navigator.save().unwrap();
        assert_eq!(
            NavigatorController::load_or_default(&path)
                .unwrap()
                .preferences()
                .views[&NavigatorView::Workspaces]
                .selected,
            Some("workspace:one".to_owned())
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn workspaces_view_keeps_host_workspace_tab_and_pane_identity() {
        let projection = projection_fixture();
        let mut agents = AgentPresentationStore::default();
        agents.rebuild(&projection);
        let rendered = render_navigator(
            NavigatorView::Workspaces,
            &projection,
            &agents,
            &NavigatorPreferences::default(),
        );
        assert!(rendered.contains("WORKSPACES"));
        assert!(rendered.contains("local (fixture)"));
        assert!(rendered.contains("Native IDE"));
        assert!(rendered.contains("main"));
        assert!(rendered.contains("Terminal [terminal]"));
        assert!(rendered.contains("Editor [editor]"));
    }

    #[test]
    fn agents_view_renders_authoritative_logos_and_parent_before_child() {
        let mut projection = projection_fixture();
        projection
            .apply_snapshot({
                let mut snapshot = crate::domain::fixture_snapshot(2);
                snapshot.agents = vec![
                    crate::domain::AgentProjection {
                        agent_instance_id: "parent".to_owned(),
                        parent_agent_instance_id: None,
                        host: crate::domain::HostScope {
                            host_id: "local".to_owned(),
                            session_id: "fixture".to_owned(),
                        },
                        workspace_id: "workspace-1".to_owned(),
                        tab_id: "tab-1".to_owned(),
                        pane_id: "pane-terminal".to_owned(),
                        name: "Parent".to_owned(),
                        kind: "codex".to_owned(),
                        phase: crate::domain::AgentPhase::Working,
                        summary: Some("parent task".to_owned()),
                        elapsed_seconds: 4,
                    },
                    crate::domain::AgentProjection {
                        agent_instance_id: "child".to_owned(),
                        parent_agent_instance_id: Some("parent".to_owned()),
                        host: crate::domain::HostScope {
                            host_id: "local".to_owned(),
                            session_id: "fixture".to_owned(),
                        },
                        workspace_id: "workspace-1".to_owned(),
                        tab_id: "tab-1".to_owned(),
                        pane_id: "pane-terminal".to_owned(),
                        name: "Child".to_owned(),
                        kind: "unknown-kind".to_owned(),
                        phase: crate::domain::AgentPhase::Ended,
                        summary: None,
                        elapsed_seconds: 2,
                    },
                ];
                snapshot
            })
            .unwrap();
        let mut agents = AgentPresentationStore::default();
        agents.rebuild(&projection);
        let rendered = render_navigator(
            NavigatorView::Agents,
            &projection,
            &agents,
            &NavigatorPreferences::default(),
        );
        assert!(rendered.contains("◈ Parent"));
        assert!(rendered.contains("◇ Child child"));
        assert!(rendered.find("Parent").unwrap() < rendered.find("Child").unwrap());
        assert!(rendered.contains("Summary unavailable"));
    }

    #[test]
    fn worktrees_view_groups_repository_and_honors_remote_badge() {
        let mut projection = projection_fixture();
        let mut snapshot = crate::domain::fixture_snapshot(3);
        snapshot.workspaces[0].remote = true;
        snapshot.workspaces[0].worktree = Some(crate::domain::WorktreeProjection {
            repo_key: "repo".to_owned(),
            repo_name: "Repository".to_owned(),
            repo_root: "/fixtures/repo".to_owned(),
            checkout_path: "/fixtures/repo/worktree".to_owned(),
            is_linked_worktree: true,
        });
        projection.apply_snapshot(snapshot).unwrap();
        let agents = AgentPresentationStore::default();
        let rendered = render_navigator(
            NavigatorView::Worktrees,
            &projection,
            &agents,
            &NavigatorPreferences::default(),
        );
        assert!(rendered.contains("Repository (repo)"));
        assert!(rendered.contains("[linked]"));
        assert!(rendered.contains("[remote]"));
    }

    #[test]
    fn explicit_collapse_hides_children_without_losing_selection() {
        let projection = projection_fixture();
        let agents = AgentPresentationStore::default();
        let mut controller = NavigatorController::in_memory();
        controller.set_expanded("host:local/fixture", false);
        controller.select("host:local/fixture");
        let preferences = controller.preferences().clone();
        let rendered = render_navigator(
            NavigatorView::Workspaces,
            &projection,
            &agents,
            &preferences,
        );
        assert!(rendered.contains("▸ local (fixture)"));
        assert!(!rendered.contains("Native IDE"));
    }

    fn projection_fixture() -> crate::domain::DomainProjection {
        let mut projection = crate::domain::DomainProjection::default();
        projection
            .apply_snapshot(crate::domain::fixture_snapshot(1))
            .unwrap();
        projection
    }
}
