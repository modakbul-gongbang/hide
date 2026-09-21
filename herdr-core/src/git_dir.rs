//! Shared repository discovery.
//!
//! The durable project resolver, session catalog, hook helper, and core must
//! agree about linked worktrees. The implementation therefore lives in
//! `hide-project`; this module keeps the existing core call sites stable.

pub use hide_project::git::{Repository, discover};
