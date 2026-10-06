//! The machine side of Hide (PRD core-host-node D-21).
//!
//! A node does the work on the machine it runs on and answers the core
//! through [`NodeLink`]. [`Local`] is the core's own machine, answered in the
//! same process.

use std::time::Duration;

use hide_node_link::protocol::Call;
use hide_node_link::{LinkAnswer, LinkError, NodeLink};

/// The machine this process runs on, answered in place.
#[derive(Debug, Default)]
pub struct Local;

impl NodeLink for Local {
    fn call(&self, call: Call, _timeout: Duration) -> Result<LinkAnswer, LinkError> {
        hide_host::serve::handle(call)
            .map(LinkAnswer::Parsed)
            .map_err(LinkError::Refused)
    }

    fn in_process(&self) -> bool {
        true
    }
}
