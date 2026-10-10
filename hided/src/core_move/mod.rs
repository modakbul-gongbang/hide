//! Moving the core to another machine and back (PRD core-host-node-move).
//!
//! The machine giving the core drives a forward move: it stops its core,
//! copies its brain state into a staging folder, changes the copy's owner
//! (`node_migration::reown`), sends it over SSH, and has the other machine
//! verify, place and start it (`target`); its own hided then links to that
//! core as a node, and the first link that carries the move's intent is
//! the commit point (`handover`). Every step before it is undone by
//! restarting the old core from its untouched folder. A move back is its
//! own sequence, driven by the node (`back`).

pub mod answer;
pub mod back;
pub mod control;
pub mod driver;
pub mod gate;
pub mod handover;
pub mod journal;
pub mod preflight;
pub mod screen;
pub mod starter;
pub mod target;

use std::io::Read;
use std::path::Path;

/// The longest intent id.
const MAX_INTENT: usize = 64;

/// A move's id: lowercase letters, digits and `-`, at most 64.
pub fn checked_intent(intent: &str) -> Result<&str, String> {
    if intent.is_empty()
        || intent.len() > MAX_INTENT
        || !intent
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(format!("{intent:?} is not a move id"));
    }
    Ok(intent)
}

/// A fresh move id.
pub fn new_intent() -> String {
    let mut bytes = [0_u8; 8];
    getrandom::getrandom(&mut bytes).expect("getrandom");
    format!("move-{}", hex::encode(bytes))
}

/// Reads a private record of at most `cap` bytes; a record another account
/// can read or one past the cap is refused.
pub(crate) fn read_private(path: &Path, cap: u64) -> std::io::Result<Vec<u8>> {
    let file = hide_platform::fs::private::open_own_file(path, false)?;
    if !hide_platform::fs::private::is_private(path)? {
        return Err(std::io::Error::other(
            "the record is readable by other accounts",
        ));
    }
    let mut bytes = Vec::new();
    file.take(cap + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(std::io::Error::other("the record is too large"));
    }
    Ok(bytes)
}
