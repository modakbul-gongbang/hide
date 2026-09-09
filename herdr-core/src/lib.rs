mod attachments;
mod changes;
pub mod chromux;
pub mod diagnostics;
mod disk;
pub mod domain;
mod environment;
mod ffi;
mod files;
pub mod find;
pub mod fixture;
mod fork;
mod github;
mod herdr_api;
pub mod herdr_contract;
pub mod live;
mod model;
pub mod pane_content;
mod persistence;
pub mod pet;
mod ports;
mod project_context;
mod reader;
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

mod viewport;
