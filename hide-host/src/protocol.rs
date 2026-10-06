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
/// 13: a helper running from the default root `~/.hide/host-helper` takes
/// the old layout's helper root and bridge folder off the device in its
/// `kit` apply, and its report carries `legacy_retirement` (PRD
/// hide-home-layout D-13); a helper on 12 would leave them.
/// 14: the kit carries `codex_per_pane` with its `off` state, and a
/// `reinstall` names the parts the operator turned off (PRD
/// overview-request-view D-21, D-24); a helper on 13 would not know them.
/// 15: `session_activity` answers only a proven session's modification time
/// and size, for the parent-owned inactivity watcher.
/// 16: worktree facts carry lock reasons and measured ignored repositories;
/// `worktree_removal_check` measures the exact accepted deletion before any
/// pane closes. A helper without this preflight must never remove instead.
/// 17: Hello carries native machine identity for lineage, and the kit reports
/// the one-release coordination retirement instead of installing it.
/// 18: the kit has seven agents, retires the other thirteen once, and no
/// longer carries `codex_per_pane`; `reinstall` has no `turn_off` and a report
/// names the Codex daemon capability itself (PRD settings-cleanup D-06, D-14).
/// A helper on 17 would still turn the Codex daemon off and know none of it.
pub const PROTOCOL_VERSION: u32 = 18;

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
    /// Non-mutating authoritative check before any pane is closed.
    WorktreeRemovalCheck {
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
        /// Known registered checkout roots on this helper's device only.
        #[serde(default)]
        retirement_projects: Vec<String>,
    },
    /// One bounded read of a pane's conversation for its label, from the
    /// checkpoint the caller kept (`hide_session::label_transcript::read`).
    /// The helper keeps nothing between reads; it answers events and the
    /// next checkpoint, never a path.
    LabelTranscript {
        request: hide_session::label_transcript::LabelTranscriptRequest,
    },
    /// Metadata-only activity using the same native ownership proof as labels.
    SessionActivity {
        request: hide_session::session_activity::SessionActivityRequest,
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
    /// The operator's choice on the machine's row: Reinstall of these
    /// parts, or an agent switched on (`agents_on`, which is also Reinstall
    /// of an agent that is on) or off (`agents_off`).
    Reinstall {
        components: Vec<hide_kit::ComponentId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        agents_on: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        agents_off: Vec<String>,
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
    /// Read once when this connection starts; a failure leaves lineage
    /// unresolved without making the file helper unavailable.
    pub machine_identity: MachineIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MachineIdentity {
    Available { id: String },
    Unavailable { reason: String },
}

impl MachineIdentity {
    pub fn into_result(self) -> Result<String, String> {
        match self {
            Self::Available { id }
                if !id.is_empty()
                    && id.len() <= 256
                    && id == id.trim()
                    && !id.bytes().any(|byte| byte.is_ascii_control()) =>
            {
                Ok(id)
            }
            Self::Available { .. } => {
                Err("The device helper reported an invalid machine identity".to_owned())
            }
            Self::Unavailable { reason } => Err(reason),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RootOpened {
    pub identity: RootIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RevisionNow {
    pub revision: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_identity_reports_unavailability_and_refuses_invalid_ids() {
        let decode = |value| serde_json::from_value::<MachineIdentity>(value).unwrap();
        assert_eq!(
            decode(serde_json::json!({"state":"available", "id":"machine-device"})).into_result(),
            Ok("machine-device".to_owned())
        );
        assert_eq!(
            decode(
                serde_json::json!({"state":"unavailable", "reason":"native identity unavailable"})
            )
            .into_result(),
            Err("native identity unavailable".to_owned())
        );
        let longest_id = "x".repeat(256);
        assert_eq!(
            MachineIdentity::Available {
                id: longest_id.clone()
            }
            .into_result(),
            Ok(longest_id)
        );
        for id in [
            String::new(),
            "device\n".to_owned(),
            "device\0suffix".to_owned(),
            " device".to_owned(),
            "x".repeat(257),
        ] {
            assert!(MachineIdentity::Available { id }.into_result().is_err());
        }
        assert!(
            serde_json::from_value::<MachineIdentity>(serde_json::json!({"state":"available"}))
                .is_err()
        );
    }
}
