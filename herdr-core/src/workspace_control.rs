//! Pane-scoped, daemon-only Workspace command contract. The core remains the
//! sole owner of membership and View state; the transport proves the caller.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::changes::{self, ChangesRequest, ChannelRef};
use crate::files::{self, DocumentPlace, DocumentRoot, OpenFailure};
use crate::host_access::HostChannel;
use crate::model::EditorDocumentSnapshot;

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
    OpenFile {
        path: String,
        beside: bool,
        reveal: bool,
    },
    OpenDiff {
        path: String,
        beside: bool,
        reveal: bool,
    },
    Select {
        view_id: String,
        reveal: bool,
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

/// The owner thread supplies a confined host source and releases its lock.
/// The daemon reads it on a request worker before asking the owner to commit.
pub struct ActionSource {
    root: DocumentRoot,
    channel: Arc<dyn HostChannel>,
    path: String,
    file: bool,
    already_open: bool,
}

pub struct ActionMaterial {
    pub(crate) file: Option<(EditorDocumentSnapshot, DocumentPlace)>,
    pub(crate) path: String,
}

pub enum ActionPreparation {
    Cached(Result<ActionResult, Refusal>),
    Ready,
    Read(ActionSource),
}

impl ActionSource {
    pub(crate) fn file(
        root: DocumentRoot,
        channel: Arc<dyn HostChannel>,
        path: String,
        already_open: bool,
    ) -> Self {
        Self {
            root,
            channel,
            path,
            file: true,
            already_open,
        }
    }

    pub(crate) fn diff(root: DocumentRoot, channel: Arc<dyn HostChannel>, path: String) -> Self {
        Self {
            root,
            channel,
            path,
            file: false,
            already_open: false,
        }
    }

    pub fn read(self) -> Result<ActionMaterial, Refusal> {
        let relative = files::relative_in_root(self.channel.as_ref(), &self.root.path, &self.path)
            .map_err(|_| Refusal {
                reason: "path_outside_checkout",
                next_action: "Choose a path inside the calling pane's checkout",
            })?;
        let path = std::path::Path::new(&self.root.path)
            .join(relative)
            .to_string_lossy()
            .into_owned();
        if self.file {
            if self.already_open {
                return Ok(ActionMaterial { file: None, path });
            }
            let opened = files::open_document(self.channel.as_ref(), &self.root, &path).map_err(
                |failure| match failure {
                    OpenFailure::Missing => Refusal {
                        reason: "file_missing",
                        next_action: "Check the path and retry",
                    },
                    OpenFailure::Failed(_) => Refusal {
                        reason: "file_unavailable",
                        next_action: "Check checkout access and file permissions, then retry",
                    },
                },
            )?;
            return Ok(ActionMaterial {
                file: Some(opened),
                path,
            });
        }
        let changes = changes::read(&ChangesRequest {
            root_path: PathBuf::from(&self.root.path),
            root: self.root,
            channel: Ok(ChannelRef(self.channel)),
            selected_path: Some(path.clone()),
            selected_committed: false,
            base_branch: None,
            diffs: Vec::new(),
        });
        if changes.unavailable_reason.is_some() {
            return Err(Refusal {
                reason: "diff_unavailable",
                next_action: "Check Git access on this device and retry",
            });
        }
        if !changes.entries.iter().any(|entry| entry.path == path) {
            return Err(Refusal {
                reason: "diff_unchanged",
                next_action: "Choose a file with a working-tree change and retry",
            });
        }
        if changes.diff.is_none() {
            return Err(Refusal {
                reason: "diff_unavailable",
                next_action: "Refresh the changed file and retry",
            });
        }
        Ok(ActionMaterial { file: None, path })
    }
}
