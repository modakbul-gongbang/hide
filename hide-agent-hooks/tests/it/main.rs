//! hide-agent-hooks' integration tests, built as one binary. Each module here was
//! a `tests/*.rs` target of its own once, and every target linked the
//! crate and all its dependencies again; docs/BUILD.md, "One integration
//! test binary per crate", owns the rule.

mod adapter_contract;
mod bell_intake;
mod codex_trust;
mod grok_cursor_hooks;
mod guidance_hook;
mod hook_deadline;
mod letter_origin;
mod lossless_install;
mod programs;
mod spawn_guard;
#[path = "../../../hide-platform/tests/it/stand_ins.rs"]
mod stand_ins;
mod windows_hook_command;
