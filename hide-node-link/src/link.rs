//! The one interface the core reaches a node through.

use std::fmt;
use std::time::Duration;

use serde_json::Value;

use crate::RootIdentity;
use crate::error::HostError;
use crate::protocol::Call;

#[derive(Debug)]
pub enum LinkError {
    /// No connection to the node; nothing was sent.
    NotConnected(String),
    /// Four requests run and thirty-two wait; nothing was sent.
    Busy,
    /// The node answered and refused, or the operation failed there.
    Refused(HostError),
    /// The request may have reached the node and its effect is unknown.
    Unknown(String),
}

impl fmt::Display for LinkError {
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

/// A node's answer before it is decoded into the type the call expects.
/// Another machine's answer stays the raw JSON text it sent: materializing an
/// untrusted line as a generic `Value` costs tens of times its size, so it is
/// decoded once, straight into the typed answer (`call_as`).
#[derive(Debug)]
pub enum LinkAnswer {
    Parsed(Value),
    Raw(Box<serde_json::value::RawValue>),
}

impl From<Value> for LinkAnswer {
    fn from(value: Value) -> Self {
        LinkAnswer::Parsed(value)
    }
}

/// One node's answerer, addressed by the core through that node's id.
pub trait NodeLink: Send + Sync {
    /// Sends one request and waits at most `timeout` for its answer. Blocks,
    /// on this machine's disk as on another; never call it under the
    /// runtime lock.
    fn call(&self, call: Call, timeout: Duration) -> Result<LinkAnswer, LinkError>;

    /// A call whose node reports progress before it answers: `progress`
    /// hears each report and answers whether the call should go on, and
    /// answering `false` asks the node to stop the work, which it answers
    /// as stopped. A link that carries no reports answers as
    /// [`NodeLink::call`] does.
    fn call_with_progress(
        &self,
        call: Call,
        timeout: Duration,
        progress: &mut dyn FnMut(Value) -> bool,
    ) -> Result<LinkAnswer, LinkError> {
        let _ = progress;
        self.call(call, timeout)
    }

    /// Whether the answer is computed in this process, so a path the
    /// operator spelled through a link to the checkout can be resolved on
    /// this machine's filesystem.
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
    /// already stopped handing the link out.
    fn close_when_idle(&self, reason: &str) {
        self.close(reason);
    }

    /// The identity this link pinned for `root`, if any. A node pins a
    /// checkout root the first time a connection touches it, so a folder
    /// swapped in at that path later is refused rather than listed.
    fn pinned(&self, _root: &str) -> Option<RootIdentity> {
        None
    }

    fn pin(&self, _root: &str, _identity: Option<RootIdentity>) {}
}

/// [`call_as`] for a call that reports progress: each report is decoded as
/// `P`, and one that does not decode stops the call.
pub fn call_as_with_progress<T: serde::de::DeserializeOwned, P: serde::de::DeserializeOwned>(
    link: &(impl NodeLink + ?Sized),
    call: Call,
    timeout: Duration,
    mut progress: impl FnMut(P) -> bool,
) -> Result<T, LinkError> {
    let mut undecoded = None;
    let answer = link.call_with_progress(
        call,
        timeout,
        &mut |report| match serde_json::from_value(report) {
            Ok(report) => progress(report),
            Err(error) => {
                undecoded = Some(error);
                false
            }
        },
    );
    if let Some(error) = undecoded {
        return Err(LinkError::Unknown(format!(
            "The node reported progress in an unexpected shape: {error}"
        )));
    }
    decode(answer?)
}

pub fn call_as<T: serde::de::DeserializeOwned>(
    link: &(impl NodeLink + ?Sized),
    call: Call,
    timeout: Duration,
) -> Result<T, LinkError> {
    decode(link.call(call, timeout)?)
}

fn decode<T: serde::de::DeserializeOwned>(answer: LinkAnswer) -> Result<T, LinkError> {
    let decoded = match answer {
        LinkAnswer::Parsed(value) => serde_json::from_value(value),
        LinkAnswer::Raw(raw) => serde_json::from_str(raw.get()),
    };
    decoded.map_err(|error| {
        LinkError::Unknown(format!(
            "The device helper answered in an unexpected shape: {error}"
        ))
    })
}
