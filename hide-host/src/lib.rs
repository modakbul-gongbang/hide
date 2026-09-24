//! The file host contract: what Hide does to files on the machine that owns
//! them, always through an opened checkout root.
//!
//! Every operation takes a `cap_std::fs::Dir` for the checkout (or a folder
//! under it) and a path relative to it, so a `..`, an absolute path or a
//! symlink that leaves the directory is refused by the handle rather than by a
//! string check, and a checkout renamed or replaced after it was opened cannot
//! redirect the work. The daemon and the core call these functions in process
//! for this machine; `hide-host-helper` serves the same functions over one SSH
//! exec channel on a registered device, so a local and a remote checkout obey
//! one contract (PRD S5.5 D-05, D-06).

pub mod bytes;
pub mod document;
pub mod error;
pub mod git;
pub mod index;
pub mod list;
pub mod mutate;
pub mod protocol;
pub mod register;
pub mod root;
pub mod save;
pub mod serve;
pub mod worktrees;

pub use error::{ErrorCode, HostError, HostResult};
pub use root::{Root, RootIdentity, relative_path};
