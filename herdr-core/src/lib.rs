pub mod agent_hooks;
mod ai;
mod changes;
pub mod chromux;
pub mod diagnostics;
mod disk;
pub mod domain;
mod environment;
#[cfg(test)]
mod fake_herdr;
mod ffi;
mod files;
pub mod find;
pub mod fixture;
mod fork;
mod git_dir;
mod github;
pub mod herdr_contract;
pub mod live;
mod model;
pub mod pane_content;
mod persistence;
pub mod pet;
mod ports;
mod project_context;
mod reader;
mod recent_closed;
pub mod remote;
pub mod remote_files;
mod runtime;
pub mod scratch;
mod session_sync;
mod sidebar;
mod terminal_recovery;
mod usage;
mod wire;
pub mod workspace;
mod worktrees;

pub use ffi::{
    HerdrBytes, HerdrCore, herdr_core_create, herdr_core_destroy, herdr_core_dispatch,
    herdr_core_free_bytes, herdr_core_on_change, herdr_core_snapshot,
};
pub use model::{CoreOptions, SCHEMA_VERSION, Snapshot};
