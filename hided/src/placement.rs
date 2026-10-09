//! Where this machine's core runs (PRD core-host-node-remote-core D-02,
//! D-03): `<state>/core-placement.json`. Absent, this machine is its own
//! core, as every machine was before this record existed. Present, it names
//! the core's machine and this daemon runs in the node role: it starts no
//! core and dials that machine instead.
//!
//! Only the move (layer 5) and test fixtures write the record. It is the
//! account's own private file; one that is anything else is refused rather
//! than read, and so is one that names this machine as its own core.

use std::io::Read;
use std::path::{Path, PathBuf};

use hide_platform::fs::private;
use serde::Deserialize;

/// The record's largest size.
const RECORD_CAP: u64 = 4096;

/// The core's machine, as this machine reaches it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    /// The SSH alias in this account's `~/.ssh/config`.
    pub alias: String,
    /// The core machine's node id, which its attach role must name.
    pub node: String,
    /// The `hided` the core machine runs as its attach role.
    pub program: String,
    /// The core's state folder on its machine, when it is not that
    /// account's default.
    #[serde(default)]
    pub state_dir: Option<String>,
}

pub fn record_path(state_dir: &Path) -> PathBuf {
    state_dir.join("core-placement.json")
}

/// The placement `state_dir` records; `None` when this machine is its own
/// core.
pub fn read(state_dir: &Path, own_node: &str) -> Result<Option<Placement>, String> {
    let path = record_path(state_dir);
    let file = match private::open_own_file(&path, false) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "the core placement record could not be opened: {error}"
            ));
        }
    };
    if !private::is_private(&path).unwrap_or(false) {
        return Err("the core placement record is readable by other accounts".to_owned());
    }
    let mut bytes = Vec::new();
    file.take(RECORD_CAP + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("the core placement record could not be read: {error}"))?;
    if bytes.len() as u64 > RECORD_CAP {
        return Err("the core placement record is too large".to_owned());
    }
    let placement: Placement = serde_json::from_slice(&bytes).map_err(|error| {
        format!("the core placement record is not one this build reads: {error}")
    })?;
    herdr_core::node::NodeId::parse(&placement.node)
        .map_err(|error| format!("the core placement record names no node: {error}"))?;
    if placement.node == own_node {
        return Err("the core placement record names this machine".to_owned());
    }
    if placement.alias.trim().is_empty() || placement.alias.starts_with('-') {
        return Err("the core placement record names no SSH alias".to_owned());
    }
    if !placement.program.starts_with('/') {
        return Err("the core placement record's program is not an absolute path".to_owned());
    }
    if placement
        .state_dir
        .as_deref()
        .is_some_and(|dir| !dir.starts_with('/'))
    {
        return Err("the core placement record's state folder is not an absolute path".to_owned());
    }
    Ok(Some(placement))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: &str = "own-node";

    fn write(dir: &Path, text: &str) {
        let path = record_path(dir);
        std::fs::write(&path, text).unwrap();
        private::restrict_to_owner(&path).unwrap();
    }

    #[test]
    fn an_absent_record_leaves_this_machine_its_own_core() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path(), OWN), Ok(None));
    }

    #[test]
    fn a_record_names_the_core_machine() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            r#"{"alias":"mini","node":"core-node","program":"/opt/hided","state_dir":"/tmp/c"}"#,
        );
        assert_eq!(
            read(dir.path(), OWN),
            Ok(Some(Placement {
                alias: "mini".to_owned(),
                node: "core-node".to_owned(),
                program: "/opt/hided".to_owned(),
                state_dir: Some("/tmp/c".to_owned()),
            }))
        );
    }

    #[test]
    fn a_record_naming_this_machine_or_shared_with_others_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            r#"{"alias":"mini","node":"own-node","program":"/opt/hided"}"#,
        );
        assert!(read(dir.path(), OWN).is_err());
        write(
            dir.path(),
            r#"{"alias":"mini","node":"core-node","program":"/opt/hided"}"#,
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                record_path(dir.path()),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            assert!(read(dir.path(), OWN).is_err());
        }
    }
}
