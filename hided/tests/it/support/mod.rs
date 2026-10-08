//! The fixtures the integration modules share: the device link's, which
//! `remote_delivery` and `node_contract` use, and the private pinned Herdr that
//! `real_herdr` and `held_letter_notice` start. Declared once, so the binary
//! compiles each once.

pub mod private_herdr;
pub mod remote_delivery;
pub mod ssh_server;
