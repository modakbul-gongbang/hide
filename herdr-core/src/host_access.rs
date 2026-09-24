//! Where a device's file work runs (PRD S5.5 D-05).
//!
//! Every document open, save and revision read, on this machine or on an SSH
//! device, is one `hide_host` request answered by the same dispatch: this
//! process runs it in place for the machine hided runs on, and
//! `hide-host-helper` runs it on a device at the other end of an SSH channel.
//! The callers see one interface and one failure vocabulary, so a local path
//! and a remote one cannot drift apart in what they refuse.

use std::fmt;
use std::time::Duration;

use hide_host::list::Listing;
use hide_host::protocol::{Call, RootOpened, RootRef};
use hide_host::{ErrorCode, RootIdentity};
use serde_json::Value;

#[derive(Debug)]
pub enum HostCallError {
    /// No helper connection; nothing was sent.
    NotConnected(String),
    /// Four requests run and thirty-two wait; nothing was sent.
    Busy,
    /// The host answered and refused, or the operation failed there.
    Refused(hide_host::HostError),
    /// The request may have reached the host and its effect is unknown.
    Unknown(String),
}

impl fmt::Display for HostCallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConnected(reason) => write!(formatter, "{reason}"),
            Self::Busy => formatter.write_str(
                "The device is busy with other file work; nothing was sent. Try again in a moment",
            ),
            Self::Refused(error) => write!(formatter, "{}", error.message),
            Self::Unknown(reason) => write!(formatter, "{reason}"),
        }
    }
}

/// One device's answerer for `hide_host` requests.
pub trait HostChannel: Send + Sync {
    /// Sends one request and waits at most `timeout` for its answer. Blocks,
    /// on this machine's disk as on a device; never call it under the
    /// runtime lock.
    fn call(&self, call: Call, timeout: Duration) -> Result<Value, HostCallError>;

    /// Whether the answer is computed in this process, so a path the
    /// operator spelled through a link to the checkout can be resolved on
    /// this machine's filesystem (`files::open_document`).
    fn in_process(&self) -> bool {
        false
    }

    /// Why the connection ended, once it has; `None` while it takes requests.
    fn closed_reason(&self) -> Option<String> {
        None
    }

    /// Ends the connection; requests still waiting become `Unknown`.
    fn close(&self, _reason: &str) {}

    /// Ends the connection once the requests already admitted have answered,
    /// so work in flight settles to its real result (B52). The caller has
    /// already stopped handing the channel out.
    fn close_when_idle(&self, reason: &str) {
        self.close(reason);
    }

    /// The identity this channel pinned for `root`, if any. A device pins a
    /// checkout root the first time a connection touches it, so a folder
    /// swapped in at that path later is refused rather than listed.
    fn pinned(&self, _root: &str) -> Option<RootIdentity> {
        None
    }

    fn pin(&self, _root: &str, _identity: Option<RootIdentity>) {}
}

const LIST_TIMEOUT: Duration = Duration::from_secs(30);

/// `root` as the channel's requests name it: its pinned identity, or the one
/// a fresh `root_open` reports, which is then pinned.
pub fn pinned_root(
    channel: &(impl HostChannel + ?Sized),
    root: &str,
    timeout: Duration,
) -> Result<RootRef, HostCallError> {
    let identity = match channel.pinned(root) {
        Some(identity) => identity,
        None => {
            let opened: RootOpened = call_as(
                channel,
                Call::RootOpen {
                    root: root.to_owned(),
                },
                timeout,
            )?;
            channel.pin(root, Some(opened.identity));
            opened.identity
        }
    };
    Ok(RootRef {
        path: root.to_owned(),
        identity,
    })
}

/// One folder of a checkout, for the Explorer. A root that was replaced is
/// refused and unpinned, so the operator's next explicit read adopts the
/// folder now at that path.
pub fn list_folder(
    channel: &(impl HostChannel + ?Sized),
    root: &str,
    relative: &str,
) -> Result<Listing, HostCallError> {
    let root_ref = pinned_root(channel, root, LIST_TIMEOUT)?;
    let result = call_as(
        channel,
        Call::List {
            root: root_ref,
            path: relative.to_owned(),
        },
        LIST_TIMEOUT,
    );
    if let Err(HostCallError::Refused(error)) = &result
        && error.code == ErrorCode::RootReplaced
    {
        channel.pin(root, None);
    }
    result.map(|mut listing: Listing| {
        // A device's answer is untrusted input: a name that is not one plain
        // path component, or entries past the cap, never reach the page.
        let answered = listing.entries.len();
        listing
            .entries
            .retain(|entry| hide_host::mutate::valid_name(&entry.name).is_ok());
        if listing.entries.len() > hide_host::list::LIST_CAP {
            listing.entries.truncate(hide_host::list::LIST_CAP);
            listing.truncated = true;
        }
        if listing.entries.len() < answered.min(hide_host::list::LIST_CAP) {
            crate::diagnostic!(serde_json::json!({
                "component": "host_access", "kind": "host.listing_names_refused",
                "root": root, "refused": answered - listing.entries.len(),
            }));
        }
        listing
    })
}

/// How long a watch poll waits for its stamps; the next poll asks again.
const STAMPS_TIMEOUT: Duration = Duration::from_secs(5);

/// A stamp per watched folder of a checkout (`hide_host::list::stamps`), for a
/// device Explorer's watch. A replaced root is refused and stays pinned: only
/// an explicit read by the operator adopts the folder now at that path.
pub fn folder_stamps(
    channel: &(impl HostChannel + ?Sized),
    root: &str,
    folders: &[String],
) -> Result<Vec<Option<String>>, HostCallError> {
    let root_ref = pinned_root(channel, root, STAMPS_TIMEOUT)?;
    call_as(
        channel,
        Call::Stamps {
            root: root_ref,
            folders: folders.to_vec(),
        },
        STAMPS_TIMEOUT,
    )
}

/// One range of a device file's bytes (`hide_host::bytes::read`).
pub fn read_bytes(
    channel: &(impl HostChannel + ?Sized),
    root: &str,
    relative: &str,
    offset: u64,
    length: u64,
) -> Result<hide_host::bytes::Range, HostCallError> {
    let root_ref = pinned_root(channel, root, LIST_TIMEOUT)?;
    let result = call_as(
        channel,
        Call::Bytes {
            root: root_ref,
            path: relative.to_owned(),
            offset,
            length,
        },
        LIST_TIMEOUT,
    );
    if let Err(HostCallError::Refused(error)) = &result
        && error.code == ErrorCode::RootReplaced
    {
        channel.pin(root, None);
    }
    result
}

/// A whole-root walk can take a while on a wide checkout far away.
const INDEX_TIMEOUT: Duration = Duration::from_secs(120);

/// Every file of a pinned root the ignore files admit, capped, from the host
/// that holds it (`hide_host::index::walk`).
pub fn index_root(
    channel: &(impl HostChannel + ?Sized),
    root: &str,
) -> Result<hide_host::index::Walked, HostCallError> {
    let root_ref = pinned_root(channel, root, LIST_TIMEOUT)?;
    let result = call_as(channel, Call::Index { root: root_ref }, INDEX_TIMEOUT);
    if let Err(HostCallError::Refused(error)) = &result
        && error.code == ErrorCode::RootReplaced
    {
        channel.pin(root, None);
    }
    result.map(|mut walked: hide_host::index::Walked| {
        // As for a listing: only relative paths inside the root, and no more
        // of them than the walk cap.
        walked
            .paths
            .retain(|path| hide_host::relative_path(path).is_ok());
        if walked.paths.len() > hide_host::index::INDEX_CAP {
            walked.paths.truncate(hide_host::index::INDEX_CAP);
            walked.truncated = true;
        }
        walked
    })
}

pub fn call_as<T: serde::de::DeserializeOwned>(
    channel: &(impl HostChannel + ?Sized),
    call: Call,
    timeout: Duration,
) -> Result<T, HostCallError> {
    let value = channel.call(call, timeout)?;
    serde_json::from_value(value).map_err(|error| {
        HostCallError::Unknown(format!(
            "The device helper answered in an unexpected shape: {error}"
        ))
    })
}

/// The machine hided runs on: the helper's own dispatch, called in place.
#[derive(Debug, Default)]
pub struct InProcessHost;

impl HostChannel for InProcessHost {
    fn call(&self, call: Call, _timeout: Duration) -> Result<Value, HostCallError> {
        hide_host::serve::handle(call).map_err(HostCallError::Refused)
    }

    fn in_process(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A device helper that answers whatever it likes.
    struct Hostile;

    impl HostChannel for Hostile {
        fn call(&self, call: Call, _timeout: Duration) -> Result<Value, HostCallError> {
            let entry =
                |name: &str| serde_json::json!({"name": name, "is_directory": false, "inode": 1});
            Ok(match call {
                Call::List { .. } => {
                    let mut entries = vec![
                        entry("ok.txt"),
                        entry("../../etc"),
                        entry("a/b"),
                        entry(".."),
                    ];
                    entries.extend((0..600).map(|n| entry(&format!("f{n}"))));
                    serde_json::json!({"entries": entries, "truncated": false})
                }
                Call::Index { .. } => {
                    let mut paths = vec![
                        "src/a.rs".to_owned(),
                        "../outside".to_owned(),
                        "/etc/passwd".to_owned(),
                    ];
                    paths.extend((0..hide_host::index::INDEX_CAP + 5).map(|n| format!("f{n}")));
                    serde_json::json!({"paths": paths, "truncated": false})
                }
                _ => serde_json::json!(null),
            })
        }

        fn pinned(&self, _root: &str) -> Option<RootIdentity> {
            Some(RootIdentity {
                device: 1,
                inode: 1,
            })
        }
    }

    /// A device's listing and walk are untrusted input: names that are not
    /// one plain component and paths that leave the root are dropped, and
    /// neither passes its cap (S5.5 B6, B7).
    #[test]
    fn a_hostile_listing_or_walk_is_confined_and_capped() {
        let listing = list_folder(&Hostile, "/r", "").unwrap();
        assert_eq!(listing.entries.len(), hide_host::list::LIST_CAP);
        assert!(listing.truncated);
        assert_eq!(listing.entries[0].name, "ok.txt");
        assert!(
            listing
                .entries
                .iter()
                .all(|entry| !entry.name.contains('/') && entry.name != "..")
        );

        let walked = index_root(&Hostile, "/r").unwrap();
        assert_eq!(walked.paths.len(), hide_host::index::INDEX_CAP);
        assert!(walked.truncated);
        assert_eq!(walked.paths[0], "src/a.rs");
        assert!(
            walked
                .paths
                .iter()
                .all(|path| !path.starts_with("..") && !path.starts_with('/'))
        );
    }
}
