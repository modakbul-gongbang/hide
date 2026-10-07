//! The TCP listeners of a node's machine.

use serde::{Deserialize, Serialize};

/// One listening address and the working directory of the process behind it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListeningPort {
    pub host: String,
    pub port: u16,
    pub cwd: String,
}

/// Every listener the node could read, or why it could read none.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListeningPorts {
    pub entries: Vec<ListeningPort>,
    pub unavailable_reason: Option<String>,
}
