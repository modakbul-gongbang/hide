//! `~/.hide/kit/installed.json`: which parts the kit has put on this machine.
//!
//! It is the only thing that tells "the operator took it away" from "never
//! installed" (D-26). A part enters it the first time the kit finds it in
//! place and leaves it only when the machine is removed from Hide.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ComponentId;

const FORMAT: u32 = 1;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Record {
    format: u32,
    installed: BTreeSet<String>,
}

impl Record {
    pub(crate) fn contains(&self, id: ComponentId) -> bool {
        self.installed.contains(id.code())
    }

    pub(crate) fn insert(&mut self, id: ComponentId) {
        self.installed.insert(id.code().to_owned());
    }
}

/// The folders under HOME that hold the kit's state and its copy of
/// hcoord.
pub(crate) const STATE_PARTS: [&str; 2] = [".hide", "kit"];

pub fn kit_state_dir(home: &Path) -> PathBuf {
    home.join(STATE_PARTS[0]).join(STATE_PARTS[1])
}

/// The kit's state folder, made private when it is missing, or why the kit
/// keeps nothing there (`crate::private_dirs`).
pub(crate) fn private_state_dir(home: &Path, create: bool) -> Result<PathBuf, String> {
    crate::private_dirs(home, &STATE_PARTS, create)
}

fn path(home: &Path) -> PathBuf {
    kit_state_dir(home).join("installed.json")
}

/// The record, an empty one when the machine has none, or why it could not
/// be read. An unreadable record is not treated as empty: that would read
/// every part the operator removed as never installed and put it back.
pub(crate) fn load(home: &Path) -> Result<Record, String> {
    let path = path(home);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Record {
                format: FORMAT,
                ..Record::default()
            });
        }
        Err(error) => return Err(format!("{} could not be read: {error}", path.display())),
    };
    let record: Record = serde_json::from_slice(&bytes)
        .map_err(|error| format!("{} is not a record Hide can read: {error}", path.display()))?;
    if record.format != FORMAT {
        return Err(format!(
            "{} has format {}, which this build does not read",
            path.display(),
            record.format
        ));
    }
    Ok(record)
}

pub(crate) fn save(home: &Path, record: &Record) -> Result<(), String> {
    let record = Record {
        format: FORMAT,
        installed: record.installed.clone(),
    };
    let mut bytes = serde_json::to_vec_pretty(&record).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    private_state_dir(home, true)?;
    crate::write_atomically(&path(home), &bytes, 0o600)
}

pub(crate) fn forget(home: &Path) -> Result<(), String> {
    let path = path(home);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{} could not be removed: {error}", path.display())),
    }
}
