//! The Explorer's listing of one folder inside a checkout.

use std::cmp::Ordering;
use std::path::Path;

use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, HostError, HostResult};

/// Children a listing carries at most; a folder with more is answered as
/// truncated, never grown (PRD S5.5 B6).
pub const LIST_CAP: usize = 500;

/// The one name the Explorer hides: the repository's own directory.
const GIT_DIR_NAME: &str = ".git";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub is_directory: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// More than `LIST_CAP` children existed; the rest were not read.
    pub truncated: bool,
}

/// The children of `relative` under `dir`: files and directories, hidden
/// names included, `.git` left out, directories first and then the natural
/// order the Swift Explorer uses.
///
/// A child that is neither a file nor a directory, and a symlink whose target
/// leaves `dir` or resolves to nothing, is not a row: the listing never
/// describes a path outside the checkout, not even as a name.
///
/// `real_root` is the canonical path `dir` was opened at. The handle refuses
/// every absolute link target, so one whose resolution lies under
/// `real_root` is followed as that relative path, through the same handle;
/// the resolution only classifies the row and never opens anything.
pub fn list(dir: &Dir, relative: &Path, real_root: &Path) -> HostResult<Listing> {
    let folder = if relative.as_os_str().is_empty() {
        dir.try_clone()
            .map_err(|error| HostError::io(&error, "The folder could not be opened"))?
    } else {
        dir.open_dir(relative)
            .map_err(|error| HostError::io(&error, "The folder could not be opened"))?
    };
    let read = folder
        .entries()
        .map_err(|error| HostError::io(&error, "The folder could not be read"))?;
    let mut entries = Vec::new();
    let mut truncated = false;
    for item in read {
        let Ok(item) = item else { continue };
        let name = item.file_name();
        let Some(name) = name.to_str() else { continue };
        if name == GIT_DIR_NAME {
            continue;
        }
        let Ok(file_type) = item.file_type() else {
            continue;
        };
        let is_directory = if file_type.is_symlink() {
            // cap-std follows the link only while it stays inside `dir`.
            let target = folder.metadata(Path::new(name)).or_else(|error| {
                let link = read_link_text(&folder, name).map_err(|_| error)?;
                if !link.is_absolute() {
                    return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
                }
                let resolved = std::fs::canonicalize(&link)?;
                let inside = resolved
                    .strip_prefix(real_root)
                    .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
                dir.metadata(inside)
            });
            match target {
                Ok(target) if target.is_dir() => true,
                Ok(target) if target.is_file() => false,
                _ => continue,
            }
        } else if file_type.is_dir() {
            true
        } else if file_type.is_file() {
            false
        } else {
            continue;
        };
        if entries.len() == LIST_CAP {
            truncated = true;
            break;
        }
        entries.push(Entry {
            name: name.to_owned(),
            is_directory,
        });
    }
    entries.sort_by(|left, right| {
        right
            .is_directory
            .cmp(&left.is_directory)
            .then_with(|| natural_cmp(&left.name, &right.name))
    });
    Ok(Listing { entries, truncated })
}

/// A link's target as written. cap-std refuses to return an absolute one,
/// which is exactly the case the caller needs to classify.
#[cfg(unix)]
fn read_link_text(folder: &Dir, name: &str) -> std::io::Result<std::path::PathBuf> {
    use std::ffi::{CString, OsStr};
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let name = CString::new(name)?;
    let mut buffer = vec![0u8; libc::PATH_MAX as usize];
    let read = unsafe {
        libc::readlinkat(
            folder.as_raw_fd(),
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    if read < 0 {
        return Err(std::io::Error::last_os_error());
    }
    buffer.truncate(read as usize);
    Ok(OsStr::from_bytes(&buffer).into())
}

#[cfg(not(unix))]
fn read_link_text(_folder: &Dir, _name: &str) -> std::io::Result<std::path::PathBuf> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// Refuses a listing request whose folder is a file, with the code a caller
/// maps to `not_a_directory`.
pub fn require_directory(dir: &Dir, relative: &Path) -> HostResult<()> {
    if relative.as_os_str().is_empty() {
        return Ok(());
    }
    let metadata = dir
        .metadata(relative)
        .map_err(|error| HostError::io(&error, "The folder could not be opened"))?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(HostError::new(
            ErrorCode::NotADirectory,
            "The path is not a folder",
        ))
    }
}

/// The order the Swift Explorer shows names in: case-insensitive, with a run
/// of digits compared as a number, so `file2` sorts before `file10`.
/// `localizedStandardCompare` is the rule the approved line names and this is
/// that rule's comparable part; a tie keeps the order the folder was read in.
pub fn natural_cmp(left: &str, right: &str) -> Ordering {
    let mut left = left.chars().flat_map(char::to_lowercase).peekable();
    let mut right = right.chars().flat_map(char::to_lowercase).peekable();
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(l), Some(r)) if l.is_ascii_digit() && r.is_ascii_digit() => {
                let left_run = take_digits(&mut left);
                let right_run = take_digits(&mut right);
                let order = number_order(&left_run, &right_run);
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(l), Some(r)) => {
                left.next();
                right.next();
                let order = l.cmp(&r);
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

fn take_digits(iter: &mut std::iter::Peekable<impl Iterator<Item = char>>) -> String {
    let mut run = String::new();
    while let Some(digit) = iter.peek().copied() {
        if !digit.is_ascii_digit() {
            break;
        }
        run.push(digit);
        iter.next();
    }
    run
}

/// Two digit runs by their value; leading zeros do not count.
fn number_order(left: &str, right: &str) -> Ordering {
    let left = left.trim_start_matches('0');
    let right = right.trim_start_matches('0');
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_sort_case_insensitively_and_by_the_value_of_a_digit_run() {
        let mut names = vec!["File10.txt", "file2.txt", "beta", "Beta2", "alpha"];
        names.sort_by(|left, right| natural_cmp(left, right));
        assert_eq!(
            names,
            vec!["alpha", "beta", "Beta2", "file2.txt", "File10.txt"]
        );
        assert_eq!(natural_cmp("007", "7"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "a "), Ordering::Less);
    }
}
