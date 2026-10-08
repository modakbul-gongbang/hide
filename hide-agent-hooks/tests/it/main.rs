//! hide-agent-hooks' integration tests, built as one binary. Each module here was
//! a `tests/*.rs` target of its own once, and every target linked the
//! crate and all its dependencies again; docs/BUILD.md, "One integration
//! test binary per crate", owns the rule.

mod adapter_contract;
mod bell_intake;
mod codex_trust;
mod guidance_hook;
mod hook_deadline;
mod letter_origin;
mod lossless_install;
mod opencode_helper;
mod opencode_plugin;
mod spawn_guard;
mod windows_hook_command;
