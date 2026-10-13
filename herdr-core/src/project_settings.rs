//! Private Project choices, shared by every checkout of one machine's
//! canonical main-worktree root. An absent field inherits the old choice;
//! an explicit Auto shadows it. No path resolution or I/O belongs here.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub(crate) const VERSION: u32 = 1;
pub(crate) const LIMIT: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum RepositoryConnection {
    Auto,
    Repository { repository: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSourceChoice {
    Auto,
    Github,
    Local,
}

impl IssueSourceChoice {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Github => "github",
            Self::Local => "local",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NewWorkLocation {
    Checkout,
    Worktree,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum BaseBranchChoice {
    Auto,
    Branch { branch: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProjectSettings {
    pub version: u32,
    pub device_id: String,
    pub root: String,
    #[serde(default)]
    pub repository: Option<RepositoryConnection>,
    #[serde(default)]
    pub issue_source: Option<IssueSourceChoice>,
    #[serde(default)]
    pub new_work: Option<NewWorkLocation>,
    #[serde(default)]
    pub base_branch: Option<BaseBranchChoice>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Patch {
    pub repository: Option<RepositoryConnection>,
    pub issue_source: Option<IssueSourceChoice>,
    pub new_work: Option<NewWorkLocation>,
    pub base_branch: Option<BaseBranchChoice>,
}

impl ProjectSettings {
    pub(crate) fn new(device: &str, root: &str) -> Self {
        Self {
            version: VERSION,
            device_id: device.to_owned(),
            root: root.to_owned(),
            repository: None,
            issue_source: None,
            new_work: None,
            base_branch: None,
        }
    }

    pub(crate) fn patched(&self, patch: Patch) -> Result<Self, &'static str> {
        if self.version != VERSION {
            return Err("project_settings_newer_version");
        }
        let mut next = self.clone();
        if let Some(repository) = patch.repository {
            if let RepositoryConnection::Repository { repository } = &repository
                && (repository.len() > 256 || !crate::issues::is_repository(repository))
            {
                return Err("project_settings_invalid_repository");
            }
            next.repository = Some(repository);
        }
        if let Some(source) = patch.issue_source {
            next.issue_source = Some(source);
        }
        if let Some(location) = patch.new_work {
            next.new_work = Some(location);
        }
        if let Some(base) = patch.base_branch {
            next.base_branch = Some(base);
        }
        Ok(next)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct WriteStatus {
    pub request_id: String,
    pub state: WriteState,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteState {
    Pending,
    Saved,
    Failed,
}

/// Catalog roots already come from the owning node's canonical Project
/// facts. Hash those facts exactly as Memory does, without an fs read under
/// Runtime or interpreting a remote path on this machine.
pub(crate) fn key(device: &str, root: &str) -> String {
    hide_project::project_id(device, Path::new(root))
}

pub(crate) fn get<'a>(
    records: &'a BTreeMap<String, ProjectSettings>,
    device: &str,
    root: &str,
) -> Option<&'a ProjectSettings> {
    records.get(&key(device, root)).filter(|record| {
        record.version == VERSION && record.device_id == device && record.root == root
    })
}

pub(crate) fn issue_choice<'a>(
    records: &'a BTreeMap<String, ProjectSettings>,
    legacy: &'a BTreeMap<String, String>,
    device: &str,
    root: &str,
) -> Option<&'a str> {
    get(records, device, root)
        .and_then(|record| record.issue_source)
        .map(IssueSourceChoice::as_str)
        .or_else(|| legacy.get(root).map(String::as_str))
}

pub(crate) fn base_choice<'a>(
    records: &'a BTreeMap<String, ProjectSettings>,
    legacy: &'a BTreeMap<String, String>,
    device: &str,
    root: &str,
) -> Option<&'a str> {
    match get(records, device, root).and_then(|record| record.base_branch.as_ref()) {
        Some(BaseBranchChoice::Auto) => None,
        Some(BaseBranchChoice::Branch { branch }) => Some(branch),
        None => legacy.get(root).map(String::as_str),
    }
}
