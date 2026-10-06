//! What a phone sees of the agents (PRD D-03, D-08, D-17): every agent this
//! Mac lists and every connected SSH device's, grouped the way the desktop
//! sidebar groups them, each row keyed by device id and pane id. It is built
//! from the core's snapshot `rest` section and carries no files, no paths and
//! nothing else of the snapshot.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

pub const LOCAL_DEVICE: &str = herdr_core::workspace::LOCAL_DEVICE_ID;

/// The groups in the order the phone draws them.
pub const GROUP_ORDER: [&str; 4] = ["needs_you", "done", "working", "seen"];

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Line {
    pub text: String,
    /// `error`, `warning` (a question or approval), or `news`.
    pub tone: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PhoneAgent {
    pub device_id: String,
    /// The core's pane id: a Herdr pane id on this Mac, `remote:<device>:pane:<id>` on a device.
    pub pane_id: String,
    /// The pane id of this agent's lineage root on the same device; itself for a root.
    pub root_pane_id: String,
    pub group: String,
    pub symbol: String,
    /// `error`, `warning`, `working`, `success` or `subtle`, as the desktop row colors its mark.
    pub tone: &'static str,
    pub agent_kind: String,
    pub title: String,
    /// `project · branch`, or the project alone for a plain folder.
    pub place: Option<String>,
    /// The SSH device's name; `None` on this Mac.
    pub device_label: Option<String>,
    /// When the core last saw the agent change state; the phone counts the
    /// elapsed time from it, so time passing sends nothing.
    pub changed_at_unix_ms: Option<u64>,
    pub line: Option<Line>,
    pub status_code: String,
    pub demand: String,
}

impl PhoneAgent {
    pub fn key(&self) -> AgentKey {
        AgentKey {
            device_id: self.device_id.clone(),
            pane_id: self.pane_id.clone(),
        }
    }

    pub fn root_key(&self) -> AgentKey {
        AgentKey {
            device_id: self.device_id.clone(),
            pane_id: self.root_pane_id.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct AgentKey {
    pub device_id: String,
    pub pane_id: String,
}

impl AgentKey {
    /// The notification tag: one notification per root agent, replaced in place.
    pub fn tag(&self) -> String {
        format!("{}|{}", self.device_id, self.pane_id)
    }

    /// The pane id Herdr knows on the device that owns it.
    pub fn herdr_pane_id(&self) -> &str {
        let prefix = format!("remote:{}:pane:", self.device_id);
        self.pane_id
            .strip_prefix(prefix.as_str())
            .unwrap_or(&self.pane_id)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Group {
    pub group: String,
    pub agents: Vec<PhoneAgent>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Projection {
    pub groups: Vec<Group>,
    /// The core's explicit interface language (`ui_state.interface_language`)
    /// as stored, or `None` when the operator follows the system. The phone
    /// resolves the value; it is part of the projection so a change republishes
    /// the frame like any other change.
    pub interface_language: Option<String>,
}

impl Projection {
    pub fn agents(&self) -> impl Iterator<Item = &PhoneAgent> {
        self.groups.iter().flat_map(|group| group.agents.iter())
    }

    pub fn find(&self, key: &AgentKey) -> Option<&PhoneAgent> {
        self.agents()
            .find(|agent| agent.device_id == key.device_id && agent.pane_id == key.pane_id)
    }
}

fn str_of<'a>(value: &'a Value, field: &str) -> &'a str {
    value.get(field).and_then(Value::as_str).unwrap_or("")
}

fn tone(agent: &Value) -> &'static str {
    if agent
        .get("waiting_on_descendants")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return "working";
    }
    match str_of(agent, "demand") {
        "error" => "error",
        "question" | "approval" => "warning",
        _ => match str_of(agent, "activity") {
            "working" => "working",
            "stopped"
                if agent
                    .get("emphasized")
                    .and_then(Value::as_bool)
                    .unwrap_or(false) =>
            {
                "success"
            }
            _ => "subtle",
        },
    }
}

/// The desktop sidebar's second-line rule (`sidebarLine`): a request stays,
/// news shows while unread, a quiet sentence is not drawn.
fn line(agent: &Value) -> Option<Line> {
    let text = agent
        .get("detail")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())?;
    let tone = match str_of(agent, "demand") {
        "error" => "error",
        "question" | "approval" => "warning",
        _ if agent.get("unread").and_then(Value::as_bool) == Some(true) => "news",
        _ => return None,
    };
    Some(Line {
        text: text.to_owned(),
        tone,
    })
}

/// Each pane's `project · branch`, from one device's workspaces, the way
/// the desktop's `agentPlaces` walks them.
fn places(workspaces: Option<&Value>) -> HashMap<String, String> {
    let mut places = HashMap::new();
    for workspace in workspaces.and_then(Value::as_array).into_iter().flatten() {
        let label = str_of(workspace, "label");
        let checkouts = workspace
            .get("checkouts")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let folder =
            workspace.get("is_git").and_then(Value::as_bool) != Some(true) && checkouts.len() == 1;
        for checkout in checkouts {
            let place = if folder {
                label.to_owned()
            } else {
                let branch = checkout
                    .get("branch")
                    .and_then(Value::as_str)
                    .filter(|branch| !branch.is_empty())
                    .unwrap_or_else(|| str_of(checkout, "label"));
                format!("{label} · {branch}")
            };
            for tab in checkout
                .get("tabs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                for pane in tab
                    .get("panes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(id) = pane.get("id").and_then(Value::as_str) {
                        places.entry(id.to_owned()).or_insert_with(|| place.clone());
                    }
                }
            }
        }
    }
    places
}

fn roots(agents: &[Value]) -> HashMap<String, String> {
    let parents: HashMap<&str, &str> = agents
        .iter()
        .filter_map(|agent| {
            let pane = agent.get("pane_id").and_then(Value::as_str)?;
            let parent = agent
                .get("lineage_parent_pane_id")
                .and_then(Value::as_str)
                .filter(|parent| !parent.is_empty())?;
            Some((pane, parent))
        })
        .collect();
    agents
        .iter()
        .filter_map(|agent| agent.get("pane_id").and_then(Value::as_str))
        .map(|pane| {
            let mut root = pane;
            // A lineage longer than the rows is a cycle; stop where it began.
            for _ in 0..agents.len() {
                match parents.get(root) {
                    Some(parent) if *parent != pane => root = parent,
                    _ => break,
                }
            }
            (pane.to_owned(), root.to_owned())
        })
        .collect()
}

fn rows(
    agents: &[Value],
    device_id: &str,
    device_label: Option<&str>,
    places: &HashMap<String, String>,
    out: &mut Vec<PhoneAgent>,
) {
    let roots = roots(agents);
    for agent in agents {
        let pane_id = str_of(agent, "pane_id");
        if pane_id.is_empty() {
            continue;
        }
        let title = agent
            .get("identity_label")
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty())
            .unwrap_or(str_of(agent, "agent_kind"));
        out.push(PhoneAgent {
            device_id: device_id.to_owned(),
            pane_id: pane_id.to_owned(),
            root_pane_id: roots
                .get(pane_id)
                .cloned()
                .unwrap_or_else(|| pane_id.to_owned()),
            group: str_of(agent, "group").to_owned(),
            symbol: str_of(agent, "symbol").to_owned(),
            tone: tone(agent),
            agent_kind: str_of(agent, "agent_kind").to_owned(),
            title: title.to_owned(),
            place: places.get(pane_id).cloned(),
            device_label: device_label.map(str::to_owned),
            changed_at_unix_ms: agent.get("changed_at_unix_ms").and_then(Value::as_u64),
            line: line(agent),
            status_code: str_of(agent, "status_code").to_owned(),
            demand: agent
                .get("demand")
                .and_then(Value::as_str)
                .unwrap_or("none")
                .to_owned(),
        });
    }
}

/// The phone projection of a merged `rest` section.
pub fn project(rest: &Value) -> Projection {
    let mut agents = Vec::new();
    let local = rest
        .pointer("/navigator/agents")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let local_places = places(rest.pointer("/navigator/workspaces"));
    rows(local, LOCAL_DEVICE, None, &local_places, &mut agents);
    let devices = rest
        .pointer("/navigator/devices")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for status in rest
        .pointer("/status/remote")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if str_of(status, "state") != "connected" {
            continue;
        }
        let target = str_of(status, "target_id");
        let label = devices
            .iter()
            .find(|device| str_of(device, "id") == target)
            .map(|device| str_of(device, "label"))
            .filter(|label| !label.is_empty())
            .unwrap_or(target);
        let session = status.get("session");
        let remote_agents = session
            .and_then(|session| session.get("agents"))
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let remote_places = places(session.and_then(|session| session.get("workspaces")));
        rows(
            remote_agents,
            target,
            Some(label),
            &remote_places,
            &mut agents,
        );
    }
    let mut groups: Vec<Group> = GROUP_ORDER
        .iter()
        .map(|group| Group {
            group: (*group).to_owned(),
            agents: Vec::new(),
        })
        .collect();
    for agent in agents {
        match groups.iter_mut().find(|group| group.group == agent.group) {
            Some(group) => group.agents.push(agent),
            None => groups.push(Group {
                group: agent.group.clone(),
                agents: vec![agent],
            }),
        }
    }
    groups.retain(|group| !group.agents.is_empty());
    let interface_language = rest
        .pointer("/ui_state/interface_language")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Projection {
        groups,
        interface_language,
    }
}

/// Lays a snapshot frame's `rest` over the one kept so far: a snapshot
/// replaces it, a delta replaces only the top-level sections it carries,
/// the same merge the web store does.
pub fn merge_rest(kept: &mut Value, frame: &Value, full: bool) -> bool {
    let Some(rest) = frame.get("rest").filter(|rest| rest.is_object()) else {
        return false;
    };
    if full || !kept.is_object() {
        *kept = rest.clone();
        return true;
    }
    let (Some(kept), Some(incoming)) = (kept.as_object_mut(), rest.as_object()) else {
        return false;
    };
    for (key, value) in incoming {
        kept.insert(key.clone(), value.clone());
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn agent(pane: &str, group: &str, extra: Value) -> Value {
        let mut value = json!({
            "pane_id": pane, "group": group, "symbol": "●", "identity_label": format!("task {pane}"),
            "agent_kind": "claude", "changed_at_unix_ms": 1_790_000_000_000_u64, "status_code": "idle", "demand": "none",
        });
        for (key, field) in extra.as_object().unwrap() {
            value[key] = field.clone();
        }
        value
    }

    fn rest() -> Value {
        json!({
            "navigator": {
                "agents": [
                    agent("w1:p1", "working", json!({"activity": "working"})),
                    agent("w1:p2", "needs_you", json!({"demand": "approval", "detail": "Bash(cargo test) 실행을 허용할까요?", "symbol": "!"})),
                    agent("w1:p3", "working", json!({"lineage_parent_pane_id": "w1:p1", "delegated": true, "demand": "question"})),
                    agent("w1:p4", "seen", json!({"detail": "quiet sentence"})),
                ],
                "workspaces": [
                    {"label": "herdr-ide", "is_git": true, "checkouts": [
                        {"label": "main", "branch": "prd/mobile-companion", "tabs": [{"panes": [{"id": "w1:p1"}, {"id": "w1:p2"}]}]},
                    ]},
                    {"label": "notes", "is_git": false, "checkouts": [
                        {"label": "notes", "branch": null, "tabs": [{"panes": [{"id": "w1:p4"}]}]},
                    ]},
                ],
                "devices": [{"id": "local", "label": "This Mac"}, {"id": "mini", "label": "mini"}],
            },
            "status": {"remote": [
                {"target_id": "mini", "state": "connected", "session": {
                    "agents": [agent("remote:mini:pane:w2:p1", "done", json!({"activity": "stopped", "emphasized": true, "unread": true, "detail": "PR #186"}))],
                    "workspaces": [{"label": "contong", "is_git": true, "checkouts": [{"label": "main", "branch": "main", "tabs": [{"panes": [{"id": "remote:mini:pane:w2:p1"}]}]}]}],
                }},
                {"target_id": "gone", "state": "unreachable", "session": {"agents": [agent("remote:gone:pane:w1:p1", "needs_you", json!({}))]}},
            ]},
        })
    }

    #[test]
    fn groups_come_in_the_sidebar_order_with_devices_included() {
        let projection = project(&rest());
        let order: Vec<_> = projection
            .groups
            .iter()
            .map(|group| (group.group.as_str(), group.agents.len()))
            .collect();
        assert_eq!(
            order,
            [("needs_you", 1), ("done", 1), ("working", 2), ("seen", 1)]
        );
        let done = &projection.groups[1].agents[0];
        assert_eq!(done.device_id, "mini");
        assert_eq!(done.device_label.as_deref(), Some("mini"));
        assert_eq!(done.place.as_deref(), Some("contong · main"));
        assert_eq!(done.tone, "success");
        assert_eq!(
            done.line,
            Some(Line {
                text: "PR #186".into(),
                tone: "news"
            })
        );
        assert_eq!(done.key().herdr_pane_id(), "w2:p1");
        // The unreachable device lists nothing.
        assert!(projection.agents().all(|agent| agent.device_id != "gone"));
    }

    /// The request view's words (the operator's request, the agent's reply)
    /// stay on the desktop: the phone's rows carry none of them (PRD
    /// overview-request-view Risks).
    #[test]
    fn the_request_views_text_never_reaches_a_phone_row() {
        let mut rest = rest();
        rest["navigator"]["agents"][0]["request"] = json!({
            "verb": "working", "verb_since_unix_ms": 1,
            "request": {"text": "secret request words", "cut": false, "images": 0, "at_unix_ms": 1, "sender": {"kind": "operator"}},
            "reply": {"text": "secret reply words", "cut": false, "at_unix_ms": 2},
            "later_by": null, "pull_requests": [],
        });
        let projection = project(&rest);
        let sent = serde_json::to_string(&projection.groups).unwrap();
        assert!(!sent.contains("secret"), "{sent}");
    }

    #[test]
    fn rows_carry_place_line_and_lineage_root() {
        let projection = project(&rest());
        let asking = &projection.groups[0].agents[0];
        assert_eq!(
            asking.place.as_deref(),
            Some("herdr-ide · prd/mobile-companion")
        );
        assert_eq!(asking.tone, "warning");
        assert_eq!(asking.line.as_ref().unwrap().tone, "warning");
        assert_eq!(asking.key().herdr_pane_id(), "w1:p2");
        let child = projection
            .agents()
            .find(|agent| agent.pane_id == "w1:p3")
            .unwrap();
        assert_eq!(child.root_pane_id, "w1:p1");
        let seen = &projection.groups[3].agents[0];
        assert_eq!(seen.place.as_deref(), Some("notes"));
        assert_eq!(seen.line, None, "a quiet sentence is not drawn");
        assert!(
            serde_json::to_string(&projection)
                .unwrap()
                .find("path")
                .is_none()
        );
    }

    #[test]
    fn the_interface_language_is_read_as_stored_and_a_change_makes_the_projection_differ() {
        let mut rest = rest();
        assert_eq!(project(&rest).interface_language, None, "absent");
        rest["ui_state"] = json!({"interface_language": null});
        let unset = project(&rest);
        assert_eq!(unset.interface_language, None, "null");
        rest["ui_state"] = json!({"interface_language": 7});
        assert_eq!(project(&rest).interface_language, None, "not a string");
        rest["ui_state"] = json!({"interface_language": "ko"});
        let korean = project(&rest);
        assert_eq!(korean.interface_language.as_deref(), Some("ko"));
        assert_ne!(korean, unset, "the watch channel must republish");
        rest["ui_state"] = json!({"interface_language": "zh-CN"});
        assert_eq!(
            project(&rest).interface_language.as_deref(),
            Some("zh-CN"),
            "the string is not validated here"
        );
    }

    #[test]
    fn a_delta_replaces_only_the_sections_it_carries() {
        let mut kept = json!(null);
        assert!(merge_rest(
            &mut kept,
            &json!({"rest": {"navigator": {"agents": []}, "status": {"remote": []}}}),
            true
        ));
        assert!(!merge_rest(&mut kept, &json!({"revision": 3}), false));
        assert!(merge_rest(
            &mut kept,
            &json!({"rest": {"navigator": {"agents": [agent("w1:p1", "done", json!({}))]}}}),
            false
        ));
        assert_eq!(kept["status"], json!({"remote": []}));
        assert_eq!(project(&kept).groups[0].group, "done");
    }
}
