//! What a phone sees of the agents (PRD D-03, D-08, D-17): every agent this
//! Mac lists and every connected SSH device's, grouped the way the desktop
//! sidebar groups them, each row keyed by device id and pane id. It is built
//! from the core's snapshot `rest` section and carries no files, no paths and
//! nothing else of the snapshot.

pub use herdr_core::agent_state::phone::{AgentKey, PhoneAgent, Projection, project};
use serde_json::Value;

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
    use herdr_core::agent_state::phone::Line;
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
        let projection = project(&rest(), "local");
        let order: Vec<_> = projection
            .groups
            .iter()
            .map(|group| (group.group.as_str(), group.count))
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

    #[test]
    fn read_questions_and_waiting_roots_keep_their_current_phone_presentation() {
        let input = json!({"navigator": {"agents": [
            agent("root", "working", json!({"waiting_on_descendants": true,
                "activity": "stopped", "emphasized": true, "unread": true, "detail": "waiting"})),
            agent("read-question", "seen", json!({"activity": "stopped", "demand": "question",
                "emphasized": false, "unread": false, "detail": "  answer me  "})),
            agent("child", "seen", json!({"lineage_parent_pane_id": "root", "delegated": true,
                "activity": "stopped", "demand": "error", "emphasized": false, "detail": "failed"})),
            agent("unknown", "seen", json!({"activity": "unknown", "detail": "quiet"})),
        ]}});
        let projection = project(&input, "local");
        let rows: Vec<_> = projection
            .agents()
            .map(|row| {
                (
                    row.pane_id.as_str(),
                    row.group.as_str(),
                    row.root_pane_id.as_str(),
                    row.tone,
                    row.line
                        .as_ref()
                        .map(|line| (line.text.as_str(), line.tone)),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "root",
                    "working",
                    "root",
                    "working",
                    Some(("waiting", "news"))
                ),
                (
                    "read-question",
                    "seen",
                    "read-question",
                    "warning",
                    Some(("answer me", "warning"))
                ),
                ("child", "seen", "root", "error", Some(("failed", "error"))),
                ("unknown", "seen", "unknown", "subtle", None),
            ]
        );
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
        let projection = project(&rest, "local");
        let sent = serde_json::to_string(&projection.groups).unwrap();
        assert!(!sent.contains("secret"), "{sent}");
    }

    #[test]
    fn rows_carry_place_line_and_lineage_root() {
        let projection = project(&rest(), "local");
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
        assert_eq!(project(&rest, "local").interface_language, None, "absent");
        rest["ui_state"] = json!({"interface_language": null});
        let unset = project(&rest, "local");
        assert_eq!(unset.interface_language, None, "null");
        rest["ui_state"] = json!({"interface_language": 7});
        assert_eq!(
            project(&rest, "local").interface_language,
            None,
            "not a string"
        );
        rest["ui_state"] = json!({"interface_language": "ko"});
        let korean = project(&rest, "local");
        assert_eq!(korean.interface_language.as_deref(), Some("ko"));
        assert_ne!(korean, unset, "the watch channel must republish");
        rest["ui_state"] = json!({"interface_language": "zh-CN"});
        assert_eq!(
            project(&rest, "local").interface_language.as_deref(),
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
        assert_eq!(project(&kept, "local").groups[0].group, "done");
    }
}
