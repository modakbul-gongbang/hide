//! The contract between the core and a node (PRD core-host-node D-21).
//!
//! The core decides and keeps state; a node does the work on its machine:
//! files, git, processes, its Herdr server. The two speak only through
//! [`NodeLink`], whatever carries it: a call inside one process for the
//! core's own machine, or a channel to another machine. This crate holds
//! that contract and the shapes that cross it, and nothing that touches a
//! machine, so the core can depend on it without reaching one. A few request
//! shapes are still those of `hide-session` and `hide-kit`, named here for
//! their types only.

pub mod bytes;
pub mod cleanup;
pub mod clone;
pub mod disk;
pub mod document;
pub mod error;
pub mod gh;
pub mod git;
pub mod home;
pub mod index;
pub mod link;
pub mod list;
pub mod mutate;
pub mod ports;
pub mod process;
pub mod protocol;
pub mod register;
pub mod save;
pub mod worktrees;

pub use error::{ErrorCode, HostError, HostResult};
pub use link::{LinkAnswer, LinkError, NodeLink, call_as, call_as_with_progress};

use serde::{Deserialize, Serialize};

/// The directory a root named when it was first opened. A later open of the
/// same path that finds another directory there is refused, so a checkout
/// renamed or replaced between two requests cannot redirect the second one.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct RootIdentity {
    pub device: u64,
    pub inode: u64,
}
