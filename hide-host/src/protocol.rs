//! The requests `hide-host-helper` answers, one JSON object per line.
//!
//! A request is `{"id": n, "op": "...", ...}` and its answer is
//! `{"id": n, "ok": ...}` or `{"id": n, "error": {"code", "message"}}`.
//! Answers may arrive out of order; the id pairs them. Every root-bearing
//! request names the root's path and the identity the first `root_open`
//! reported, so the helper refuses a checkout replaced between requests.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::HostError;
use crate::root::RootIdentity;

/// Bumped when a request or an answer changes shape. The core refuses a
/// helper that reports another version and installs the one it carries.
/// 9: `changes` takes the View displays' `diffs` and answers each (PRD S7
/// A5); a helper on 8 would ignore them and answer none.
/// 10: `kit` installs, judges and removes the device's install kit (PRD
/// device-parity); a helper on 9 would refuse it as an unknown operation.
/// 11: `home_sync` manages Hide's Home folder and its project links.
/// 12: `label_transcript` reads a pane's conversation for its label (PRD
/// labels-in-hided D-03); a helper on 11 would refuse it as unknown.
pub const PROTOCOL_VERSION: u32 = 12;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    #[serde(flatten)]
    pub call: Call,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RootRef {
    pub path: String,
    pub identity: RootIdentity,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Call {
    Hello,
    RootOpen {
        root: String,
    },
    List {
        root: RootRef,
        path: String,
    },
    /// A stamp per folder (`hide_host::list::stamps`), at most
    /// `MAX_STAMPED_FOLDERS`, for a device Explorer's watch.
    Stamps {
        root: RootRef,
        folders: Vec<String>,
    },
    /// At most `hide_host::bytes::MAX_RANGE` bytes of a regular file from
    /// `offset` (`hide_host::bytes::read`).
    Bytes {
        root: RootRef,
        path: String,
        offset: u64,
        length: u64,
    },
    /// Every file under the root the ignore files admit, capped
    /// (`hide_host::index::walk`), for the ⌘P palette.
    Index {
        root: RootRef,
    },
    OpenDocument {
        root: RootRef,
        path: String,
    },
    /// The content revision of a file now, for settling a save whose answer
    /// was lost with the connection.
    Revision {
        root: RootRef,
        path: String,
    },
    Save {
        root: RootRef,
        path: String,
        contents: String,
        expected_revision: String,
    },
    /// A new empty file, or a folder when `directory`, named `name` in the
    /// folder `parent`. Never replaces an existing item.
    Create {
        root: RootRef,
        parent: String,
        name: String,
        directory: bool,
    },
    /// `path` takes the name `name` in its own folder.
    Rename {
        root: RootRef,
        path: String,
        name: String,
    },
    /// `path` moves into the folder `destination`, keeping its name.
    Move {
        root: RootRef,
        path: String,
        destination: String,
    },
    /// `path` goes to this machine's Trash; `inode` is the item the operator
    /// confirmed, and another item found at the path is refused.
    Trash {
        root: RootRef,
        path: String,
        inode: Option<u64>,
    },
    /// The checkout's Git changes under `scope`, with the diff of `selected`
    /// from the working or the committed group, and of each of `diffs`
    /// (`hide_host::git`).
    Changes {
        root: RootRef,
        scope: String,
        selected: Option<String>,
        committed: bool,
        base: Option<String>,
        #[serde(default)]
        diffs: Vec<crate::git::DiffTarget>,
    },
    /// The project facts of a folder on this machine (`hide_project::facts`):
    /// a Herdr pane's directory or a registered project's path, which the
    /// daemon's catalog groups into projects with this device's id. Only
    /// facts are answered, never contents.
    Project {
        path: String,
    },
    /// The worktrees of the repository that holds `path`
    /// (`hide_host::worktrees::read`), measured against `bases` or the
    /// operator's `base_override`. `null` for a folder that is not a
    /// repository.
    Worktrees {
        path: String,
        bases: std::collections::BTreeMap<String, String>,
        base_override: Option<String>,
    },
    /// Whether `branch` may be created in the repository at `path`
    /// (`hide_host::worktrees::check_new_branch`); nothing is created.
    BranchCheck {
        path: String,
        branch: String,
    },
    /// The real path of an existing directory, or `null`.
    Directory {
        path: String,
    },
    /// The project a folder would be registered as, judged against the
    /// host's own home folder (`hide_host::register::check`).
    Registrable {
        path: String,
    },
    /// Makes this host's Hide Home folder (`~/hide`) hold one link per
    /// project in `projects` and nothing Hide did not make
    /// (`hide_host::home::sync`).
    HomeSync {
        projects: Vec<String>,
    },
    /// Rechecks and removes one operator-confirmed linked worktree, forced
    /// only as far as the operator accepted
    /// (`hide_host::worktrees::remove_confirmed`).
    WorktreeRemove {
        removal: crate::worktrees::ConfirmedRemoval,
    },
    /// The device's install kit (`hide_host::kit`): the helper acts on the
    /// helper root it runs from, never on a folder the request names.
    /// `cli_dir` is where the consent allows the `hide` link, and
    /// `herdr_socket` the registration's Herdr socket; both may start with
    /// `~/`.
    Kit {
        action: KitAction,
        cli_dir: String,
        herdr_socket: Option<String>,
    },
    /// One bounded read of a pane's conversation for its label, from the
    /// checkpoint the caller kept (`hide_session::label_transcript::read`).
    /// The helper keeps nothing between reads; it answers events and the
    /// next checkpoint, never a path.
    LabelTranscript {
        request: hide_session::label_transcript::LabelTranscriptRequest,
    },
}

/// What a `kit` request does. `apply` and `reinstall` answer a
/// `hide_kit::KitReport`, `status` answers one without changing anything, and
/// `remove` answers a [`KitRemoved`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KitAction {
    /// The connection pass: install what was never installed and replace
    /// what is outdated.
    Apply,
    /// The operator's Reinstall of these parts.
    Reinstall {
        components: Vec<hide_kit::ComponentId>,
    },
    Status,
    /// The device is being removed from Hide: Hide's parts come off, then
    /// the helper root.
    Remove,
}

/// What removing the kit from a device did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KitRemoved {
    pub kit: hide_kit::RemoveReport,
    /// The helper root and every build under it.
    pub helper_root: hide_kit::RemoveOutcome,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok(Value),
    Error(HostError),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub version: String,
    pub os: String,
    pub arch: String,
    /// The home directory of the account the helper runs as, the boundary
    /// the device's own registration listing applies (PRD S5.5 B24).
    pub home: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RootOpened {
    pub identity: RootIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RevisionNow {
    pub revision: String,
}
