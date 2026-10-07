//! The background AI as the core asks a node for it. The core keeps the
//! router (provider choice, retries, budgets, cooldowns); each of its
//! backends is a `BackendSpec` the node answers for with a real backend of
//! its own, so the provider processes run on the node's machine with its
//! logins. A backend's log events come back with each answer.

use serde::{Deserialize, Serialize};

pub use hide_ai::{
    AiError, AiLogEvent, AiRequest, AiResponse, Availability, ModelCatalog, ProcessMeasurement,
    ProviderId,
};

/// One backend of one core router. `instance` is the core's own number for
/// it: the node keeps one real backend per instance, so two routers never
/// share a resident provider process, and drops it when the core releases
/// the instance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BackendSpec {
    pub instance: u64,
    pub provider: ProviderId,
    pub model: String,
}

/// An answer and the log events the backend wrote since the last answer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Logged<T> {
    pub value: T,
    pub log: Vec<AiLogEvent>,
}
