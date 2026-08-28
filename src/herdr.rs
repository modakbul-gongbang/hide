use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::domain::{
    AgentPhase, AgentProjection, DomainSnapshot, EnvironmentContract, HERDR_PROTOCOL_REVISION,
    HostScope, LayoutNode, PaneProjection, SplitAxis, SurfaceKind, TabProjection,
    WorkspaceProjection, WorktreeProjection,
};

const API_TIMEOUT: Duration = Duration::from_secs(2);

pub const PROCESS_ENVIRONMENT: &[EnvironmentContract] = &[
    EnvironmentContract {
        key: "HERDR_SOCKET_PATH",
        value: None,
        requirement: "optional",
        missing_behavior: "the IDE resolves the default or selected session socket",
    },
    EnvironmentContract {
        key: "HOME",
        value: None,
        requirement: "required runtime value",
        missing_behavior: "the default Herdr socket cannot be resolved, so connection fails visibly",
    },
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HerdrConnectionConfig {
    pub socket_path: PathBuf,
    pub herdr_bin: PathBuf,
    pub session_name: Option<String>,
}

impl HerdrConnectionConfig {
    pub fn resolve(
        socket_path: Option<PathBuf>,
        herdr_bin: Option<PathBuf>,
        session_name: Option<String>,
    ) -> Result<Self> {
        let socket_path = match socket_path {
            Some(path) => path,
            None => resolve_socket_path(session_name.as_deref())?,
        };
        Ok(Self {
            socket_path,
            herdr_bin: herdr_bin.unwrap_or_else(|| PathBuf::from("herdr")),
            session_name,
        })
    }
}

fn resolve_socket_path(session_name: Option<&str>) -> Result<PathBuf> {
    if session_name.is_none()
        && let Some(path) = read_registered_environment("HERDR_SOCKET_PATH")?
    {
        return Ok(PathBuf::from(path));
    }
    let home = read_registered_environment("HOME")?
        .ok_or_else(|| anyhow!("stage=herdr.socket target=HOME cause=missing-required-value"))?;
    let base = PathBuf::from(home).join(".config/herdr");
    Ok(match session_name {
        Some(name) => base.join("sessions").join(name).join("herdr.sock"),
        None => base.join("herdr.sock"),
    })
}

fn read_registered_environment(key: &str) -> Result<Option<String>> {
    if !PROCESS_ENVIRONMENT.iter().any(|item| item.key == key) {
        return Err(anyhow!(
            "stage=environment.read target={key} cause=unregistered-key"
        ));
    }
    Ok(std::env::var(key).ok().filter(|value| !value.is_empty()))
}

#[derive(Clone, Debug, PartialEq)]
pub enum TopologyOperation {
    TabCreate {
        workspace_id: String,
        cwd: Option<String>,
    },
    TabFocus {
        tab_id: String,
    },
    TabRename {
        tab_id: String,
        label: String,
    },
    TabMove {
        tab_id: String,
        insert_index: usize,
    },
    TabClose {
        tab_id: String,
    },
    PaneSplit {
        pane_id: String,
        direction: SplitDirection,
        cwd: Option<String>,
    },
    PaneFocus {
        pane_id: String,
    },
    PaneFocusDirection {
        pane_id: String,
        direction: PaneDirection,
    },
    PaneResize {
        pane_id: String,
        direction: PaneDirection,
        amount: f32,
    },
    PaneRename {
        pane_id: String,
        label: Option<String>,
    },
    PaneClose {
        pane_id: String,
    },
}

impl TopologyOperation {
    fn request(&self) -> (&'static str, Value) {
        match self {
            Self::TabCreate { workspace_id, cwd } => (
                "tab.create",
                json!({"workspace_id": workspace_id, "cwd": cwd, "focus": true}),
            ),
            Self::TabFocus { tab_id } => ("tab.focus", json!({"tab_id": tab_id})),
            Self::TabRename { tab_id, label } => {
                ("tab.rename", json!({"tab_id": tab_id, "label": label}))
            }
            Self::TabMove {
                tab_id,
                insert_index,
            } => (
                "tab.move",
                json!({"tab_id": tab_id, "insert_index": insert_index}),
            ),
            Self::TabClose { tab_id } => ("tab.close", json!({"tab_id": tab_id})),
            Self::PaneSplit {
                pane_id,
                direction,
                cwd,
            } => (
                "pane.split",
                json!({
                    "target_pane_id": pane_id,
                    "direction": direction.as_wire(),
                    "cwd": cwd,
                    "focus": true,
                    "right_click": "pane"
                }),
            ),
            Self::PaneFocus { pane_id } => ("pane.focus", json!({"pane_id": pane_id})),
            Self::PaneFocusDirection { pane_id, direction } => (
                "pane.focus_direction",
                json!({"pane_id": pane_id, "direction": direction.as_wire()}),
            ),
            Self::PaneResize {
                pane_id,
                direction,
                amount,
            } => (
                "pane.resize",
                json!({"pane_id": pane_id, "direction": direction.as_wire(), "amount": amount}),
            ),
            Self::PaneRename { pane_id, label } => {
                ("pane.rename", json!({"pane_id": pane_id, "label": label}))
            }
            Self::PaneClose { pane_id } => ("pane.close", json!({"pane_id": pane_id})),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::TabCreate { .. } => "tab.create",
            Self::TabFocus { .. } => "tab.focus",
            Self::TabRename { .. } => "tab.rename",
            Self::TabMove { .. } => "tab.move",
            Self::TabClose { .. } => "tab.close",
            Self::PaneSplit { .. } => "pane.split",
            Self::PaneFocus { .. } => "pane.focus",
            Self::PaneFocusDirection { .. } => "pane.focus_direction",
            Self::PaneResize { .. } => "pane.resize",
            Self::PaneRename { .. } => "pane.rename",
            Self::PaneClose { .. } => "pane.close",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplitDirection {
    Right,
    Down,
}

impl SplitDirection {
    fn as_wire(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Down => "down",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
}

impl PaneDirection {
    fn as_wire(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

#[derive(Debug)]
enum WorkerCommand {
    Refresh,
    Apply(TopologyOperation),
}

#[derive(Debug)]
pub enum HerdrUpdate {
    Snapshot(DomainSnapshot),
    Applied {
        operation: &'static str,
        snapshot: DomainSnapshot,
    },
    Failed {
        operation: &'static str,
        error: String,
    },
}

pub struct HerdrController {
    commands: Sender<WorkerCommand>,
    updates: Receiver<HerdrUpdate>,
    config: HerdrConnectionConfig,
}

impl HerdrController {
    pub fn start(config: HerdrConnectionConfig) -> Result<Self> {
        let (commands, command_rx) = mpsc::channel();
        let (update_tx, updates) = mpsc::channel();
        let worker_config = config.clone();
        thread::Builder::new()
            .name("herdr-ide-api".to_owned())
            .spawn(move || run_worker(worker_config, command_rx, update_tx))
            .context("stage=herdr.worker target=api cause=spawn-failed")?;
        let controller = Self {
            commands,
            updates,
            config,
        };
        controller.refresh()?;
        Ok(controller)
    }

    pub fn config(&self) -> &HerdrConnectionConfig {
        &self.config
    }

    pub fn refresh(&self) -> Result<()> {
        self.commands
            .send(WorkerCommand::Refresh)
            .map_err(|_| anyhow!("stage=herdr.refresh target=worker cause=channel-closed"))
    }

    pub fn submit(&self, operation: TopologyOperation) -> Result<()> {
        self.commands
            .send(WorkerCommand::Apply(operation))
            .map_err(|_| anyhow!("stage=herdr.command target=worker cause=channel-closed"))
    }

    pub fn try_update(&self) -> Result<Option<HerdrUpdate>> {
        match self.updates.try_recv() {
            Ok(update) => Ok(Some(update)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(anyhow!(
                "stage=herdr.update target=worker cause=channel-closed"
            )),
        }
    }

    pub fn execute_sync(
        config: &HerdrConnectionConfig,
        operation: &TopologyOperation,
    ) -> Result<DomainSnapshot> {
        let (method, params) = operation.request();
        request(&config.socket_path, method, params)?;
        fetch_snapshot(&config.socket_path)
    }
}

fn run_worker(
    config: HerdrConnectionConfig,
    commands: Receiver<WorkerCommand>,
    updates: Sender<HerdrUpdate>,
) {
    while let Ok(command) = commands.recv() {
        match command {
            WorkerCommand::Refresh => match fetch_snapshot(&config.socket_path) {
                Ok(snapshot) => {
                    if updates.send(HerdrUpdate::Snapshot(snapshot)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    if updates
                        .send(HerdrUpdate::Failed {
                            operation: "session.snapshot",
                            error: format!("{error:#}"),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            },
            WorkerCommand::Apply(operation) => {
                let label = operation.label();
                let result = HerdrController::execute_sync(&config, &operation);
                let update = match result {
                    Ok(snapshot) => HerdrUpdate::Applied {
                        operation: label,
                        snapshot,
                    },
                    Err(error) => HerdrUpdate::Failed {
                        operation: label,
                        error: format!("{error:#}"),
                    },
                };
                if updates.send(update).is_err() {
                    break;
                }
            }
        }
    }
}

fn request(socket_path: &PathBuf, method: &str, params: Value) -> Result<Value> {
    let mut stream = UnixStream::connect(socket_path).with_context(|| {
        format!(
            "stage=herdr.connect target=api-socket cause=unreachable path={}",
            socket_path.display()
        )
    })?;
    stream
        .set_read_timeout(Some(API_TIMEOUT))
        .context("stage=herdr.timeout target=read")?;
    stream
        .set_write_timeout(Some(API_TIMEOUT))
        .context("stage=herdr.timeout target=write")?;
    let envelope = json!({"id": format!("herdr-ide:{method}"), "method": method, "params": params});
    serde_json::to_writer(&mut stream, &envelope)
        .context("stage=herdr.request target=serialize")?;
    stream
        .write_all(b"\n")
        .context("stage=herdr.request target=socket-write")?;
    stream
        .flush()
        .context("stage=herdr.request target=socket-flush")?;

    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .context("stage=herdr.response target=socket-read")?;
    if line.trim().is_empty() {
        return Err(anyhow!("stage=herdr.response cause=empty-response"));
    }
    let response: Value =
        serde_json::from_str(&line).context("stage=herdr.response target=json")?;
    if let Some(error) = response.get("error") {
        let code = string(error, "code").unwrap_or("unknown");
        let message = string(error, "message").unwrap_or("Herdr request failed");
        return Err(anyhow!(
            "stage=herdr.command target={method} code={code} cause={message}"
        ));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow!("stage=herdr.response target={method} cause=missing-result"))
}

fn fetch_snapshot(socket_path: &PathBuf) -> Result<DomainSnapshot> {
    let result = request(socket_path, "session.snapshot", json!({}))?;
    let snapshot = result
        .get("snapshot")
        .ok_or_else(|| anyhow!("stage=herdr.snapshot cause=missing-snapshot"))?;
    let protocol = number(snapshot, "protocol")? as u32;
    if protocol != HERDR_PROTOCOL_REVISION {
        return Err(anyhow!(
            "stage=herdr.snapshot cause=protocol-mismatch expected={} received={protocol}",
            HERDR_PROTOCOL_REVISION
        ));
    }
    let host_value = required(snapshot, "host")?;
    let host = HostScope {
        host_id: required_string(host_value, "host_id")?.to_owned(),
        session_id: required_string(host_value, "session_id")?.to_owned(),
    };
    let tabs = array(snapshot, "tabs")?;
    let panes = array(snapshot, "panes")?;
    let workspaces_value = array(snapshot, "workspaces")?;
    let active_workspace_id = string(snapshot, "focused_workspace_id")
        .filter(|value| !value.is_empty())
        .or_else(|| {
            workspaces_value
                .iter()
                .find(|workspace| workspace.get("focused").and_then(Value::as_bool) == Some(true))
                .and_then(|workspace| string(workspace, "workspace_id"))
        })
        .ok_or_else(|| anyhow!("stage=herdr.snapshot cause=missing-focused-workspace"))?
        .to_owned();
    let mut layouts = BTreeMap::new();
    for tab in tabs {
        let tab_id = required_string(tab, "tab_id")?;
        let result = request(socket_path, "layout.export", json!({"tab_id": tab_id}))?;
        let root = required(required(&result, "layout")?, "root")?;
        layouts.insert(tab_id.to_owned(), parse_layout(root)?);
    }

    let mut workspaces = Vec::new();
    for workspace in workspaces_value {
        let workspace_id = required_string(workspace, "workspace_id")?;
        let active_tab_id = required_string(workspace, "active_tab_id")?;
        let mut projected_tabs = Vec::new();
        for tab in tabs
            .iter()
            .filter(|tab| string(tab, "workspace_id").is_some_and(|value| value == workspace_id))
        {
            let tab_id = required_string(tab, "tab_id")?;
            let tab_panes = panes
                .iter()
                .filter(|pane| string(pane, "tab_id").is_some_and(|value| value == tab_id))
                .map(parse_pane)
                .collect::<Result<Vec<_>>>()?;
            let focused_pane_id = panes
                .iter()
                .find(|pane| {
                    string(pane, "tab_id").is_some_and(|value| value == tab_id)
                        && pane.get("focused").and_then(Value::as_bool) == Some(true)
                })
                .and_then(|pane| string(pane, "pane_id"))
                .or_else(|| tab_panes.first().map(|pane| pane.pane_id.as_str()))
                .ok_or_else(|| {
                    anyhow!("stage=herdr.snapshot target={tab_id} cause=tab-has-no-pane")
                })?;
            projected_tabs.push(TabProjection {
                tab_id: tab_id.to_owned(),
                name: required_string(tab, "label")?.to_owned(),
                focused_pane_id: focused_pane_id.to_owned(),
                panes: tab_panes,
                layout: layouts.remove(tab_id).ok_or_else(|| {
                    anyhow!("stage=herdr.snapshot target={tab_id} cause=missing-layout")
                })?,
            });
        }
        workspaces.push(WorkspaceProjection {
            host: host.clone(),
            workspace_id: workspace_id.to_owned(),
            name: required_string(workspace, "label")?.to_owned(),
            remote: host.host_id != "local",
            active_tab_id: active_tab_id.to_owned(),
            tabs: projected_tabs,
            worktree: parse_worktree(workspace)?,
        });
    }

    Ok(DomainSnapshot {
        protocol_revision: protocol,
        sequence: number(snapshot, "event_sequence")?,
        active_workspace_id,
        workspaces,
        agents: parse_agents(snapshot, &host)?,
    })
}

fn parse_worktree(workspace: &Value) -> Result<Option<WorktreeProjection>> {
    let Some(value) = workspace.get("worktree") else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(WorktreeProjection {
        repo_key: required_string(value, "repo_key")?.to_owned(),
        repo_name: required_string(value, "repo_name")?.to_owned(),
        repo_root: required_string(value, "repo_root")?.to_owned(),
        checkout_path: required_string(value, "checkout_path")?.to_owned(),
        is_linked_worktree: value
            .get("is_linked_worktree")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                anyhow!("stage=herdr.worktree cause=invalid-boolean field=is_linked_worktree")
            })?,
    }))
}

#[derive(Clone, Debug)]
struct LineageRecord {
    agent_instance_id: String,
    parent_agent_instance_id: Option<String>,
    host: HostScope,
    workspace_id: String,
    tab_id: String,
    pane_id: String,
    name: String,
    kind: String,
    phase: AgentPhase,
}

fn parse_agents(snapshot: &Value, default_host: &HostScope) -> Result<Vec<AgentProjection>> {
    let mut lineage = BTreeMap::new();
    let mut lineage_by_pane = BTreeMap::new();
    for value in array(snapshot, "lineage")? {
        let agent_instance_id = required_string(value, "agent_instance_id")?.to_owned();
        let record = LineageRecord {
            agent_instance_id: agent_instance_id.clone(),
            parent_agent_instance_id: optional_string(value, "parent_agent_instance_id"),
            host: parse_host(value.get("host"), default_host)?,
            workspace_id: required_string(value, "workspace_id")?.to_owned(),
            tab_id: required_string(value, "tab_id")?.to_owned(),
            pane_id: required_string(value, "pane_id")?.to_owned(),
            name: required_string(value, "name")?.to_owned(),
            kind: required_string(value, "kind")?.to_owned(),
            phase: parse_lineage_phase(required_string(value, "state")?),
        };
        lineage_by_pane.insert(record.pane_id.clone(), agent_instance_id.clone());
        lineage.insert(agent_instance_id, record);
    }

    let mut agents = lineage
        .values()
        .map(|record| {
            (
                record.agent_instance_id.clone(),
                AgentProjection {
                    agent_instance_id: record.agent_instance_id.clone(),
                    parent_agent_instance_id: record.parent_agent_instance_id.clone(),
                    host: record.host.clone(),
                    workspace_id: record.workspace_id.clone(),
                    tab_id: record.tab_id.clone(),
                    pane_id: record.pane_id.clone(),
                    name: fallback_name(&record.name, None, None, None),
                    kind: record.kind.clone(),
                    phase: record.phase,
                    summary: None,
                    elapsed_seconds: 0,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    for value in array(snapshot, "agents")? {
        let pane_id = required_string(value, "pane_id")?;
        let agent_instance_id = optional_string(value, "agent_instance_id")
            .or_else(|| lineage_by_pane.get(pane_id).cloned());
        let Some(agent_instance_id) = agent_instance_id else {
            // A pane without a stable agent identity is intentionally not projected as an
            // agent. This keeps the tree keyed by the server-owned identity instead of
            // inventing a pane-derived id.
            continue;
        };
        let lineage_record = lineage.get(&agent_instance_id);
        let host = lineage_record
            .map(|record| record.host.clone())
            .unwrap_or(parse_host(value.get("host"), default_host)?);
        let parent = lineage_record
            .and_then(|record| record.parent_agent_instance_id.clone())
            .or_else(|| optional_string(value, "parent_agent_instance_id"));
        let workspace_id = match lineage_record {
            Some(record) => record.workspace_id.clone(),
            None => required_string(value, "workspace_id")?.to_owned(),
        };
        let tab_id = match lineage_record {
            Some(record) => record.tab_id.clone(),
            None => required_string(value, "tab_id")?.to_owned(),
        };
        let name = fallback_name(
            lineage_record
                .map(|record| record.name.as_str())
                .unwrap_or(""),
            optional_string(value, "name").as_deref(),
            optional_string(value, "display_agent").as_deref(),
            optional_string(value, "terminal_title_stripped").as_deref(),
        );
        let kind = lineage_record
            .map(|record| record.kind.clone())
            .or_else(|| optional_string(value, "agent"))
            .unwrap_or_default();
        let phase = parse_agent_phase(required_string(value, "agent_status")?);
        let summary = parse_summary(value);
        let elapsed_seconds = parse_elapsed_seconds(value);
        agents.insert(
            agent_instance_id.clone(),
            AgentProjection {
                agent_instance_id,
                parent_agent_instance_id: parent,
                host,
                workspace_id,
                tab_id,
                pane_id: pane_id.to_owned(),
                name,
                kind,
                phase,
                summary,
                elapsed_seconds,
            },
        );
    }
    Ok(agents.into_values().collect())
}

fn parse_host(value: Option<&Value>, fallback: &HostScope) -> Result<HostScope> {
    let Some(value) = value else {
        return Ok(fallback.clone());
    };
    Ok(HostScope {
        host_id: required_string(value, "host_id")?.to_owned(),
        session_id: required_string(value, "session_id")?.to_owned(),
    })
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn fallback_name(
    primary: &str,
    secondary: Option<&str>,
    tertiary: Option<&str>,
    last: Option<&str>,
) -> String {
    [Some(primary), secondary, tertiary, last]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or("Unnamed agent")
        .to_owned()
}

fn parse_lineage_phase(state: &str) -> AgentPhase {
    match state {
        "ended" => AgentPhase::Ended,
        "orphaned" => AgentPhase::Attention,
        _ => AgentPhase::Idle,
    }
}

fn parse_agent_phase(status: &str) -> AgentPhase {
    match status {
        "working" => AgentPhase::Working,
        "blocked" | "unknown" => AgentPhase::Attention,
        "done" => AgentPhase::Ended,
        _ => AgentPhase::Idle,
    }
}

fn parse_summary(value: &Value) -> Option<String> {
    let candidates = [
        value
            .get("tokens")
            .and_then(|tokens| string(tokens, "summary")),
        value
            .get("state_labels")
            .and_then(|labels| string(labels, "summary")),
        value
            .get("tokens")
            .and_then(|tokens| string(tokens, "task")),
    ];
    candidates
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|summary| !summary.is_empty())
        .map(|summary| {
            summary
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(120)
                .collect()
        })
}

fn parse_elapsed_seconds(value: &Value) -> u64 {
    let raw = value
        .get("tokens")
        .and_then(|tokens| string(tokens, "elapsed"))
        .or_else(|| {
            value
                .get("state_labels")
                .and_then(|labels| string(labels, "elapsed"))
        });
    let Some(raw) = raw.map(str::trim) else {
        return 0;
    };
    let (number, multiplier) = if let Some(value) = raw.strip_suffix('h') {
        (value, 3_600)
    } else if let Some(value) = raw.strip_suffix('m') {
        (value, 60)
    } else if let Some(value) = raw.strip_suffix('s') {
        (value, 1)
    } else {
        (raw, 1)
    };
    number
        .trim()
        .parse::<u64>()
        .unwrap_or(0)
        .saturating_mul(multiplier)
}

fn parse_pane(pane: &Value) -> Result<PaneProjection> {
    let pane_id = required_string(pane, "pane_id")?.to_owned();
    let surface = required(pane, "surface")?;
    let kind = required_string(surface, "kind")?;
    let (surface_kind, agent_instance_id) = match kind {
        "terminal" => {
            let attach = required(surface, "attach")?;
            let protocol = number(attach, "protocol")? as u32;
            if protocol != HERDR_PROTOCOL_REVISION {
                return Err(anyhow!(
                    "stage=herdr.attach target={pane_id} cause=protocol-mismatch expected={} received={protocol}",
                    HERDR_PROTOCOL_REVISION
                ));
            }
            (
                SurfaceKind::Terminal,
                string(surface, "agent_instance_id").map(str::to_owned),
            )
        }
        "editor" => (SurfaceKind::Editor, None),
        "browser" => (SurfaceKind::Browser, None),
        other => {
            return Err(anyhow!(
                "stage=herdr.snapshot target={pane_id} cause=unknown-surface kind={other}"
            ));
        }
    };
    Ok(PaneProjection {
        pane_id,
        title: string(pane, "label")
            .or_else(|| string(pane, "title"))
            .unwrap_or("Terminal")
            .to_owned(),
        surface: surface_kind,
        agent_instance_id,
    })
}

fn parse_layout(node: &Value) -> Result<LayoutNode> {
    match required_string(node, "type")? {
        "pane" => Ok(LayoutNode::Pane {
            pane_id: required_string(node, "pane_id")?.to_owned(),
        }),
        "split" => {
            let direction = required_string(node, "direction")?;
            Ok(LayoutNode::Split {
                axis: match direction {
                    "horizontal" | "right" => SplitAxis::Horizontal,
                    "vertical" | "down" => SplitAxis::Vertical,
                    other => {
                        return Err(anyhow!(
                            "stage=herdr.layout cause=unknown-direction direction={other}"
                        ));
                    }
                },
                ratio: required(node, "ratio")?
                    .as_f64()
                    .ok_or_else(|| anyhow!("stage=herdr.layout cause=invalid-ratio"))?
                    as f32,
                first: Box::new(parse_layout(required(node, "first")?)?),
                second: Box::new(parse_layout(required(node, "second")?)?),
            })
        }
        other => Err(anyhow!(
            "stage=herdr.layout cause=unknown-node type={other}"
        )),
    }
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .get(key)
        .ok_or_else(|| anyhow!("stage=herdr.decode cause=missing-field field={key}"))
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    string(value, key).ok_or_else(|| anyhow!("stage=herdr.decode cause=invalid-string field={key}"))
}

fn string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn number(value: &Value, key: &str) -> Result<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("stage=herdr.decode cause=invalid-number field={key}"))
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("stage=herdr.decode cause=invalid-array field={key}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topology_operation_uses_the_current_method_and_stable_target() {
        let operations = [
            TopologyOperation::TabCreate {
                workspace_id: "w1".to_owned(),
                cwd: Some("/tmp/fixture".to_owned()),
            },
            TopologyOperation::TabFocus {
                tab_id: "w1:t2".to_owned(),
            },
            TopologyOperation::TabRename {
                tab_id: "w1:t2".to_owned(),
                label: "renamed".to_owned(),
            },
            TopologyOperation::TabMove {
                tab_id: "w1:t2".to_owned(),
                insert_index: 0,
            },
            TopologyOperation::TabClose {
                tab_id: "w1:t2".to_owned(),
            },
            TopologyOperation::PaneSplit {
                pane_id: "w1:p1".to_owned(),
                direction: SplitDirection::Right,
                cwd: None,
            },
            TopologyOperation::PaneFocus {
                pane_id: "w1:p1".to_owned(),
            },
            TopologyOperation::PaneFocusDirection {
                pane_id: "w1:p1".to_owned(),
                direction: PaneDirection::Right,
            },
            TopologyOperation::PaneResize {
                pane_id: "w1:p1".to_owned(),
                direction: PaneDirection::Right,
                amount: 0.1,
            },
            TopologyOperation::PaneRename {
                pane_id: "w1:p1".to_owned(),
                label: Some("renamed".to_owned()),
            },
            TopologyOperation::PaneClose {
                pane_id: "w1:p1".to_owned(),
            },
        ];
        let methods = operations
            .iter()
            .map(|operation| operation.request().0)
            .collect::<Vec<_>>();
        assert_eq!(
            methods,
            [
                "tab.create",
                "tab.focus",
                "tab.rename",
                "tab.move",
                "tab.close",
                "pane.split",
                "pane.focus",
                "pane.focus_direction",
                "pane.resize",
                "pane.rename",
                "pane.close"
            ]
        );
        assert_eq!(operations[5].request().1["target_pane_id"], "w1:p1");
        assert_eq!(operations[3].request().1["insert_index"], 0);
    }

    #[test]
    fn terminal_projection_requires_the_current_attach_endpoint() {
        let valid = json!({
            "pane_id": "w1:p1",
            "surface": {
                "kind": "terminal",
                "agent_instance_id": null,
                "attach": {"protocol": HERDR_PROTOCOL_REVISION}
            }
        });
        assert_eq!(parse_pane(&valid).unwrap().surface, SurfaceKind::Terminal);

        let invalid = json!({
            "pane_id": "w1:p1",
            "surface": {"kind": "terminal"}
        });
        assert!(
            parse_pane(&invalid)
                .unwrap_err()
                .to_string()
                .contains("missing-field")
        );
    }

    #[test]
    fn exported_layout_preserves_stable_leaf_ids_and_ratio() {
        let value = json!({
            "type": "split",
            "direction": "horizontal",
            "ratio": 0.37,
            "first": {"type": "pane", "pane_id": "w1:p1"},
            "second": {"type": "pane", "pane_id": "w1:p2"}
        });
        assert_eq!(
            parse_layout(&value).unwrap(),
            LayoutNode::Split {
                axis: SplitAxis::Horizontal,
                ratio: 0.37,
                first: Box::new(LayoutNode::Pane {
                    pane_id: "w1:p1".to_owned()
                }),
                second: Box::new(LayoutNode::Pane {
                    pane_id: "w1:p2".to_owned()
                })
            }
        );
    }

    #[test]
    fn snapshot_agent_parser_merges_authoritative_info_with_lineage() {
        let host = HostScope {
            host_id: "local".to_owned(),
            session_id: "fixture".to_owned(),
        };
        let snapshot = json!({
            "lineage": [{
                "agent_instance_id": "parent",
                "idempotency_key": "parent-key",
                "name": "Root",
                "kind": "codex",
                "host": {"host_id": "local", "session_id": "fixture"},
                "workspace_id": "workspace-1",
                "tab_id": "tab-1",
                "pane_id": "pane-1",
                "parent_agent_instance_id": null,
                "spawned_from_pane_id": null,
                "state": "ended"
            }, {
                "agent_instance_id": "child",
                "idempotency_key": "child-key",
                "name": "Child",
                "kind": "claude-code",
                "host": {"host_id": "remote", "session_id": "fixture"},
                "workspace_id": "workspace-2",
                "tab_id": "tab-2",
                "pane_id": "pane-2",
                "parent_agent_instance_id": "parent",
                "spawned_from_pane_id": "pane-1",
                "state": "active"
            }],
            "agents": [{
                "agent_instance_id": "child",
                "agent": "claude-code",
                "agent_status": "working",
                "workspace_id": "workspace-2",
                "tab_id": "tab-2",
                "pane_id": "pane-2",
                "terminal_id": "terminal-2",
                "focused": true,
                "revision": 2,
                "parent_agent_instance_id": "parent",
                "name": "Live Child",
                "tokens": {"summary": "  shipping   a fix ", "elapsed": "2m"},
                "state_labels": {}
            }]
        });
        let agents = parse_agents(&snapshot, &host).unwrap();
        assert_eq!(agents.len(), 2);
        let parent = agents
            .iter()
            .find(|agent| agent.agent_instance_id == "parent")
            .unwrap();
        assert_eq!(parent.phase, AgentPhase::Ended);
        let child = agents
            .iter()
            .find(|agent| agent.agent_instance_id == "child")
            .unwrap();
        assert_eq!(child.host.host_id, "remote");
        assert_eq!(child.parent_agent_instance_id.as_deref(), Some("parent"));
        assert_eq!(child.name, "Child");
        assert_eq!(child.kind, "claude-code");
        assert_eq!(child.summary.as_deref(), Some("shipping a fix"));
        assert_eq!(child.elapsed_seconds, 120);
        assert_eq!(child.phase, AgentPhase::Working);
    }

    #[test]
    fn workspace_parser_requires_typed_worktree_fields() {
        let workspace = json!({
            "worktree": {
                "repo_key": "repo",
                "repo_name": "Repo",
                "repo_root": "/fixtures/repo",
                "checkout_path": "/fixtures/repo/checkout",
                "is_linked_worktree": true
            }
        });
        let worktree = parse_worktree(&workspace).unwrap().unwrap();
        assert_eq!(worktree.repo_key, "repo");
        assert!(worktree.is_linked_worktree);
        let invalid = json!({"worktree": {"repo_key": "repo"}});
        assert!(
            parse_worktree(&invalid)
                .unwrap_err()
                .to_string()
                .contains("repo_name")
        );
    }

    #[test]
    fn environment_registry_is_complete_and_contains_no_values() {
        assert_eq!(PROCESS_ENVIRONMENT.len(), 2);
        assert!(PROCESS_ENVIRONMENT.iter().all(|item| {
            !item.key.is_empty()
                && !item.requirement.is_empty()
                && !item.missing_behavior.is_empty()
        }));
    }

    #[test]
    #[ignore = "requires a disposable current-protocol Herdr fixture"]
    fn live_topology_commands_return_exact_post_mutation_snapshots() {
        let config = HerdrConnectionConfig::resolve(None, None, None).unwrap();
        let before = fetch_snapshot(&config.socket_path).unwrap();
        let workspace = before.workspaces.first().expect("fixture workspace");
        assert!(workspace.tabs.len() >= 2, "fixture needs two tabs");
        let first_tab = &workspace.tabs[0];
        assert!(first_tab.panes.len() >= 3, "fixture needs three panes");
        let pane_a = first_tab.panes[0].pane_id.clone();
        let pane_to_close = first_tab.panes[2].pane_id.clone();
        let tab_to_close = workspace.tabs[1].tab_id.clone();

        let focused = HerdrController::execute_sync(
            &config,
            &TopologyOperation::PaneFocus {
                pane_id: pane_a.clone(),
            },
        )
        .unwrap();
        assert_eq!(focused.workspaces[0].active_tab_id, first_tab.tab_id);
        assert_eq!(focused.workspaces[0].tabs[0].focused_pane_id, pane_a);

        let renamed = HerdrController::execute_sync(
            &config,
            &TopologyOperation::PaneRename {
                pane_id: pane_a.clone(),
                label: Some("T5 terminal".to_owned()),
            },
        )
        .unwrap();
        assert_eq!(renamed.workspaces[0].tabs[0].panes[0].title, "T5 terminal");

        let layout_before_resize = renamed.workspaces[0].tabs[0].layout.clone();
        let resized = HerdrController::execute_sync(
            &config,
            &TopologyOperation::PaneResize {
                pane_id: pane_a.clone(),
                direction: PaneDirection::Right,
                amount: 0.1,
            },
        )
        .unwrap();
        assert_ne!(resized.workspaces[0].tabs[0].layout, layout_before_resize);

        let renamed_tab = HerdrController::execute_sync(
            &config,
            &TopologyOperation::TabRename {
                tab_id: tab_to_close.clone(),
                label: "T5 reordered".to_owned(),
            },
        )
        .unwrap();
        assert!(
            renamed_tab.workspaces[0]
                .tabs
                .iter()
                .any(|tab| { tab.tab_id == tab_to_close && tab.name == "T5 reordered" })
        );

        let reordered = HerdrController::execute_sync(
            &config,
            &TopologyOperation::TabMove {
                tab_id: tab_to_close.clone(),
                insert_index: 0,
            },
        )
        .unwrap();
        assert_eq!(reordered.workspaces[0].tabs[0].tab_id, tab_to_close);

        let pane_closed = HerdrController::execute_sync(
            &config,
            &TopologyOperation::PaneClose {
                pane_id: pane_to_close.clone(),
            },
        )
        .unwrap();
        assert!(
            pane_closed.workspaces[0]
                .tabs
                .iter()
                .all(|tab| { tab.panes.iter().all(|pane| pane.pane_id != pane_to_close) })
        );

        let tab_closed = HerdrController::execute_sync(
            &config,
            &TopologyOperation::TabClose {
                tab_id: tab_to_close.clone(),
            },
        )
        .unwrap();
        assert!(
            tab_closed.workspaces[0]
                .tabs
                .iter()
                .all(|tab| tab.tab_id != tab_to_close)
        );
    }
}
