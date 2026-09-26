//! Pane-scoped, daemon-only Workspace command contract. The core remains the
//! sole owner of membership and View state; the transport proves the caller.

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Query {
    Info,
    ViewList,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Context {
    pub device_id: String,
    pub workspace_id: String,
    pub checkout_id: String,
    pub checkout_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct View {
    pub area_id: String,
    pub view_id: String,
    pub kind: &'static str,
    pub target: String,
    pub selected: bool,
    pub active_area: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct QueryResult {
    pub context: Context,
    pub capabilities: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub views: Option<Vec<View>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Refusal {
    pub reason: &'static str,
    pub next_action: &'static str,
}
