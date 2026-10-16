//! agy's folder trust for the worktrees Hide makes (PRD antigravity-first-class
//! D-08). agy asks whether to trust a folder the first time it starts there
//! interactively, and a started prompt loses its workspace while it asks, so
//! Hide records the trust before it starts agy in a worktree it made.
//!
//! The trust list is `trustedWorkspaces` in `~/.gemini/antigravity-cli/
//! settings.json`, one exact path per entry. Hide adds only the path it is
//! about to start agy in, and remembers in its own record which entries it
//! added: removing a worktree, or switching agy off, takes out only those,
//! and an entry the operator added, before Hide or after it, stays.
//!
//! Every change holds one account lock and rewrites settings.json through the
//! same lossless read and compare-before-replace as the hook files, so the
//! other keys and the operator's formatting are kept byte for byte.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::install::{HookDocument, InstallFailure, read_document, write_document};

const KEY: &str = "trustedWorkspaces";
const LOCK_WAIT: Duration = Duration::from_secs(2);
/// The most entries Hide's record keeps; a worktree is removed long before.
const RECORD_LIMIT: usize = 4096;

/// agy's settings file under `home`.
pub fn settings_path(home: &Path) -> PathBuf {
    home.join(".gemini")
        .join("antigravity-cli")
        .join("settings.json")
}

fn record_path(home: &Path) -> PathBuf {
    home.join(".hide")
        .join("agent-hooks")
        .join("antigravity-trust.json")
}

fn lock_path(home: &Path) -> PathBuf {
    home.join(".hide")
        .join("agent-hooks")
        .join("antigravity-trust.lock")
}

fn hold(home: &Path) -> Result<hide_platform::fs::lock::Lock, InstallFailure> {
    let failure = |detail: String| InstallFailure::NotWritable {
        path: lock_path(home).display().to_string(),
        detail,
    };
    let path = lock_path(home);
    if let Some(parent) = path.parent() {
        hide_platform::fs::private::create_dir_all(parent)
            .map_err(|error| failure(error.to_string()))?;
    }
    let file = hide_platform::fs::private::open_or_create_file(&path)
        .map_err(|error| failure(error.to_string()))?;
    match hide_platform::fs::lock::lock_file(
        file,
        hide_platform::fs::lock::Mode::Exclusive,
        LOCK_WAIT,
        &|| false,
    )
    .map_err(|error| failure(error.to_string()))?
    {
        hide_platform::fs::lock::Waited::Locked(held) => Ok(held),
        _ => Err(failure("another trust change held the lock".to_owned())),
    }
}

fn read_record(home: &Path) -> Result<Vec<String>, InstallFailure> {
    let path = record_path(home);
    let unreadable = |detail: String| InstallFailure::Unreadable {
        path: path.display().to_string(),
        detail,
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| unreadable(error.to_string())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(unreadable(error.to_string())),
    }
}

fn write_record(home: &Path, record: &[String]) -> Result<(), InstallFailure> {
    let path = record_path(home);
    let failure = |detail: String| InstallFailure::NotWritable {
        path: path.display().to_string(),
        detail,
    };
    if record.len() > RECORD_LIMIT {
        return Err(failure(format!(
            "Hide's record of agy trust holds more than {RECORD_LIMIT} folders"
        )));
    }
    let bytes = serde_json::to_vec(record).map_err(|error| failure(error.to_string()))?;
    hide_platform::fs::atomic::write_file(&path, &bytes, hide_platform::fs::Access::Private)
        .map(drop)
        .map_err(|error| failure(error.to_string()))
}

/// The spelling agy compares a folder by: the absolute path as agy's own
/// trust dialog writes it.
fn spelling(path: &Path) -> Result<String, InstallFailure> {
    path.to_str()
        .filter(|_| path.is_absolute())
        .map(str::to_owned)
        .ok_or_else(|| InstallFailure::UnexpectedShape {
            path: path.display().to_string(),
            detail: "an agy trust folder must be an absolute UTF-8 path".to_owned(),
        })
}

/// The trust list of `document`, made when the file has none.
fn entries<'a>(
    document: &'a mut HookDocument,
    settings: &Path,
) -> Result<&'a mut Vec<Value>, InstallFailure> {
    let shape = |detail: &str| InstallFailure::UnexpectedShape {
        path: settings.display().to_string(),
        detail: detail.to_owned(),
    };
    let object = document
        .as_object_mut()
        .ok_or_else(|| shape("agy's settings are not a JSON object"))?;
    object
        .entry(KEY)
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| shape("trustedWorkspaces is not a list"))
}

/// Records that agy may trust `folder` without asking. A folder agy already
/// trusts is left as it is and stays the operator's.
pub fn trust(home: &Path, folder: &Path) -> Result<(), InstallFailure> {
    let folder = spelling(folder)?;
    let settings = settings_path(home);
    let _lock = hold(home)?;
    let mut document =
        read_document(&settings)?.unwrap_or_else(|| HookDocument::new(Value::Object(Default::default())));
    let list = entries(&mut document, &settings)?;
    if list.iter().any(|entry| entry.as_str() == Some(folder.as_str())) {
        return Ok(());
    }
    // The record is written first: a settings write that then fails leaves a
    // recorded folder agy does not trust, which a removal skips harmlessly.
    let mut record = read_record(home)?;
    if !record.contains(&folder) {
        record.push(folder.clone());
        write_record(home, &record)?;
    }
    list.push(Value::String(folder));
    write_document(&settings, &document)
}

/// Takes out the trust Hide added for `folder`, as when Hide removes the
/// worktree; a folder Hide did not add is left alone. Answers how many
/// entries left agy's settings.
pub fn forget(home: &Path, folder: &Path) -> Result<usize, InstallFailure> {
    let folder = spelling(folder)?;
    forget_where(home, |entry| entry == folder)
}

/// Takes out every trust Hide added, as when agy is switched off.
pub fn forget_all(home: &Path) -> Result<usize, InstallFailure> {
    forget_where(home, |_| true)
}

fn forget_where(home: &Path, chosen: impl Fn(&str) -> bool) -> Result<usize, InstallFailure> {
    // A machine where Hide never trusted a folder has no record and needs
    // no lock, so a removal there writes nothing of agy's or Hide's.
    if !record_path(home)
        .try_exists()
        .map_err(|error| InstallFailure::Unreadable {
            path: record_path(home).display().to_string(),
            detail: error.to_string(),
        })?
    {
        return Ok(0);
    }
    let settings = settings_path(home);
    let _lock = hold(home)?;
    let record = read_record(home)?;
    let (gone, kept): (Vec<_>, Vec<_>) = record.into_iter().partition(|entry| chosen(entry));
    if gone.is_empty() {
        return Ok(0);
    }
    let mut removed = 0;
    if let Some(mut document) = read_document(&settings)? {
        let list = entries(&mut document, &settings)?;
        let before = list.len();
        list.retain(|entry| entry.as_str().is_none_or(|entry| !gone.iter().any(|g| g == entry)));
        removed = before - list.len();
        if removed > 0 {
            write_document(&settings, &document)?;
        }
    }
    write_record(home, &kept)?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".gemini/antigravity-cli")).unwrap();
        home
    }

    #[test]
    fn hide_takes_out_only_the_trust_it_added_and_keeps_the_rest_byte_for_byte() {
        let home = home();
        let settings = settings_path(home.path());
        let original = "{\n  \"theme\": \"dark\",\n  \"trustedWorkspaces\": [\"/Users/me\"]\n}\n";
        std::fs::write(&settings, original).unwrap();
        trust(home.path(), Path::new("/work/tree-1")).unwrap();
        trust(home.path(), Path::new("/work/tree-1")).unwrap();
        trust(home.path(), Path::new("/Users/me")).unwrap();
        let written: Value = serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
        assert_eq!(written[KEY], serde_json::json!(["/Users/me", "/work/tree-1"]));
        // The operator's own folder is never Hide's to take out.
        forget(home.path(), Path::new("/Users/me")).unwrap();
        forget(home.path(), Path::new("/work/tree-1")).unwrap();
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
    }

    #[test]
    fn switching_agy_off_takes_out_every_folder_hide_added() {
        let home = home();
        let settings = settings_path(home.path());
        trust(home.path(), Path::new("/work/a")).unwrap();
        trust(home.path(), Path::new("/work/b")).unwrap();
        assert_eq!(forget_all(home.path()).unwrap(), 2);
        let written: Value = serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
        assert_eq!(written[KEY], serde_json::json!([]));
        assert!(read_record(home.path()).unwrap().is_empty());
    }

    #[test]
    fn removing_where_hide_never_trusted_a_folder_writes_nothing() {
        let home = home();
        assert_eq!(forget(home.path(), Path::new("/work/a")).unwrap(), 0);
        assert_eq!(forget_all(home.path()).unwrap(), 0);
        assert!(!home.path().join(".hide").exists());
        assert!(!settings_path(home.path()).exists());
    }

    #[test]
    fn settings_hide_cannot_read_stay_untouched_and_the_trust_fails() {
        let home = home();
        let settings = settings_path(home.path());
        std::fs::write(&settings, "{ not json").unwrap();
        assert!(matches!(
            trust(home.path(), Path::new("/work/a")),
            Err(InstallFailure::Unparsable { .. })
        ));
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{ not json");
        assert!(trust(home.path(), Path::new("relative")).is_err());
    }
}
