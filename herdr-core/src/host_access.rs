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
    result
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
