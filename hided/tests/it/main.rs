//! hided's integration tests, built as one binary. Each module here was
//! a `tests/*.rs` target of its own once, and every target linked the
//! crate and all its dependencies again; docs/BUILD.md, "One integration
//! test binary per crate", owns the rule.

mod browser_control;
mod connect_build;
mod connect_stop;
mod handshake;
mod held_letter_notice;
mod mobile;
mod node_contract;
mod node_home;
mod node_link_facts;
mod node_owner;
mod node_session_activity;
mod opener_lifecycle;
mod real_herdr;
mod remote_delivery;
#[cfg(unix)]
mod support;
