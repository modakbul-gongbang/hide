pub mod agent_hooks;
mod ai;
mod changes;
mod device_catalog;
pub mod diagnostics;
mod disk;
pub mod domain;
mod environment;
#[cfg(test)]
mod fake_herdr;
mod ffi;
mod files;
pub use files::FileRoots;
pub mod find;
pub mod fixture;
mod fork;
mod git_dir;
mod github;
pub mod herdr_contract;
pub mod host_access;
pub mod issues;
pub mod live;
mod model;
mod persistence;
pub mod pet;
mod ports;
mod project_context;
mod reader;
mod recent_closed;
pub mod remote;
mod runtime;
pub mod schema;
mod session_sync;
mod sidebar;
mod terminal_attachments;
mod terminal_recovery;
mod usage;
mod view_layout;
mod wire;
pub mod workspace;
pub mod workspace_views;
mod worktrees;
mod zoneinfo;

pub use ffi::{
    Core, HerdrBytes, HerdrCore, herdr_core_create, herdr_core_destroy, herdr_core_dispatch,
    herdr_core_free_bytes, herdr_core_on_change, herdr_core_snapshot,
};
pub use model::{CoreOptions, SCHEMA_VERSION, Snapshot, SnapshotDeltaPayload};
pub use runtime::serialize_snapshot_delta;
