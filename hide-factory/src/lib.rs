//! Software Factory: the engine that takes Tasks with clear requirements,
//! runs them through workers in dependency order, verifies and merges them,
//! and hands a person only what they must act on (PRD software-factory).
//!
//! This crate knows nothing of Hide's runtime or Herdr. It owns the values
//! ([`model`]), the store ([`store`]), the dependency graph ([`dag`]), the
//! roles ([`role`]), the judgment features ([`judgment`]), the command
//! contract ([`command`]), the stage 2 summary ([`summary`]) and the adapter
//! traits the engine is generic over ([`adapters`]). `docs/factory.md` is the
//! owning guide.

pub mod adapters;
pub mod command;
pub mod dag;
pub mod engine;
pub mod judgment;
pub mod model;
pub mod role;
pub mod store;
pub mod summary;

pub use command::{Command, Refusal};
pub use engine::{Engine, Inbound, Ports};
pub use model::{Factory, Task, TaskState};
pub use role::Role;
pub use summary::FactorySummary;
