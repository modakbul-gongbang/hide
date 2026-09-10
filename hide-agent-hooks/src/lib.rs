//! Hide's ownership of the agent runtimes' global hook configuration.
//!
//! This crate is the only code path in the product that writes to a file the
//! operator owns outside the app's own state. Everything it knows lives here:
//! where each runtime keeps its hook file, what an entry looks like, how to
//! append one without touching anybody else's, how to judge what is currently
//! installed, and how to report that judgement.
//!
//! It is a separate crate rather than a module of `herdr-core` because the
//! risk it carries is a file-system risk, and folding it into the crate that
//! owns `Mutex<Runtime>` would put a `settings.json` write behind the same
//! lock as the render snapshot and drag the FFI boundary into the tests that
//! matter most here (PRD D-47).
//!
//! Nothing in this crate talks to Herdr's socket. The hook helper reports a
//! pane's subagent counts through the `herdr` CLI, which Herdr defines as
//! display-only metadata, and the core reads them back out of the pane tokens
//! its ordinary snapshot already carries (PRD D-08, D-27, D-33).

pub mod counters;
pub mod diagnosis;
pub mod install;
pub mod report;
pub mod runtime;

pub use counters::PaneCounters;
pub use diagnosis::{Diagnosis, PaneInstrumentation, RuntimeDiagnosis, UninstrumentedReason};
pub use install::{HookStatus, InstallFailure, InstallOutcome, RemoveOutcome, install, remove};
pub use runtime::{
    AgentRuntime, HELPER_BINARY_NAME, HOOK_SOURCE_NAME, HOOK_VERSION, HookEvent, hook_source_id,
};
