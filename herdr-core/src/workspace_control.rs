//! Pane-scoped, daemon-only Workspace command contract. The core remains the
//! sole owner of membership and View state; the transport proves the caller.

use serde::{Deserialize, Serialize};

pub use crate::view_layout::Edge;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Query {
    Info,
    ViewList,
}

/// One pane request is one core transition. The transport supplies the pane
/// identity; no Workspace selector is accepted from the caller.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Select {
        view_id: String,
    },
    Split {
        view_id: String,
        area_id: String,
        edge: Edge,
    },
    Move {
        view_id: String,
        area_id: String,
        index: usize,
    },
    Close {
        view_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ActionResult {
    pub context: Context,
    pub request_id: String,
    pub changed: bool,
    pub view_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area_id: Option<String>,
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
