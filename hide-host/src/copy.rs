//! Copies inside a checkout: an Explorer item copied into a folder, and the
//! files of a drop from the operator's file manager written into one.
//!
//! Both are all or nothing. The new items are built in a private folder made
//! beside where they land and moved to their names last, each by the
//! platform layer's rename that refuses to replace
//! (`hide_platform::fs::atomic::rename_no_replace`), so a half-made copy is
//! never at a name the operator can open and a name that appears meanwhile
//! is never replaced. A name that is taken becomes `name copy.ext`, then
//! `name copy 2.ext`, as VS Code names it.
//! Everything runs through handles opened under the checkout root, as the
//! other changes do (`crate::mutate`), and a link or a special file is
//! refused rather than followed or recreated: a link could point out of the
//! checkout, and a copy of it would name that place from inside.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
use cap_std::fs::{Dir, Metadata, OpenOptions};
use hide_node_link::mutate::{
    COPY_MAX_BYTES, COPY_MAX_ITEMS, Copied, IMPORT_MAX_BYTES, IMPORT_MAX_FILE_BYTES,
    IMPORT_MAX_ITEMS, ImportEntry, Imported, valid_name,
};
use hide_platform::fs::atomic;
use hide_platform::path::RelPath;

use crate::error::{ErrorCode, HostError, HostResult};
use crate::mutate::{open_folder, require_item};
use crate::root::open_parent;

/// Names tried for one item before the copy gives up: the wanted one, then
/// `copy`, then `copy 2` to `copy` and this number.
const NAME_TRIES: usize = 1_000;

/// What a copy has used of its caps.
struct Budget {
    items: usize,
    bytes: u64,
}

impl Budget {
    fn new() -> Self {
        Self { items: 0, bytes: 0 }
    }

    fn item(&mut self, name: &OsStr) -> HostResult<()> {
        self.items += 1;
        if self.items > COPY_MAX_ITEMS {
            return Err(HostError::new(
                ErrorCode::TooLarge,
                format!(
                    "{} holds more than {COPY_MAX_ITEMS} items; nothing was copied",
                    name.to_string_lossy()
                ),
            ));
        }
        Ok(())
    }

    fn bytes(&mut self, name: &OsStr, len: u64) -> HostResult<()> {
        self.bytes = self.bytes.saturating_add(len);
        if self.bytes > COPY_MAX_BYTES {
            return Err(HostError::new(
                ErrorCode::TooLarge,
                format!(
                    "{} is larger than {} MiB; nothing was copied",
                    name.to_string_lossy(),
                    COPY_MAX_BYTES / (1024 * 1024)
                ),
            ));
        }
        Ok(())
    }
}

/// The names `name` may take in a folder, in the order they are tried: the
/// name itself, then `stem copy.ext`, then `stem copy 2.ext` onward. The
/// extension is the text after the last dot of a file's name, never a name's
/// leading dot, and a folder has none.
pub fn copy_names(name: &str, directory: bool) -> impl Iterator<Item = String> + use<> {
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if !directory && dot > 0 => (name[..dot].to_owned(), name[dot..].to_owned()),
        _ => (name.to_owned(), String::new()),
    };
    let wanted = name.to_owned();
    std::iter::once(wanted).chain((1..NAME_TRIES).map(move |attempt| {
        if attempt == 1 {
            format!("{stem} copy{extension}")
        } else {
            format!("{stem} copy {attempt}{extension}")
        }
    }))
}

fn refuse_kind(meta: &Metadata, name: &OsStr) -> HostResult<()> {
    if meta.is_symlink() {
        return Err(HostError::new(
            ErrorCode::Unsupported,
            format!(
                "{} is a link; links are not copied, and nothing was copied",
                name.to_string_lossy()
            ),
        ));
    }
    if !meta.is_file() && !meta.is_dir() {
        return Err(HostError::new(
            ErrorCode::Unsupported,
            format!(
                "{} is not a file or a folder; nothing was copied",
                name.to_string_lossy()
            ),
        ));
    }
    Ok(())
}

/// Counts what copying `name` would make, and refuses a link, a special file
/// or a cap crossed, before anything is written.
fn measure(parent: &Dir, name: &OsStr, budget: &mut Budget) -> HostResult<()> {
    let meta = require_item(parent, name)?;
    refuse_kind(&meta, name)?;
    budget.item(name)?;
    if meta.is_file() {
        return budget.bytes(name, meta.len());
    }
    let folder = parent
        .open_dir(name)
        .map_err(|error| HostError::io(&error, "A folder could not be opened"))?;
    for entry in folder
        .entries()
        .map_err(|error| HostError::io(&error, "A folder could not be read"))?
    {
        let entry = entry.map_err(|error| HostError::io(&error, "A folder could not be read"))?;
        measure(&folder, &entry.file_name(), budget)?;
    }
    Ok(())
}

/// Writes `name` of `parent` as `to_name` in `to`, with what is under it.
/// Bytes are counted again as they are read: a file that grew after it was
/// measured is a refusal, not a larger copy.
fn copy_tree(
    parent: &Dir,
    name: &OsStr,
    to: &Dir,
    to_name: &OsStr,
    budget: &mut Budget,
) -> HostResult<()> {
    let meta = require_item(parent, name)?;
    refuse_kind(&meta, name)?;
    budget.item(name)?;
    if meta.is_dir() {
        to.create_dir(Path::new(to_name))
            .map_err(|error| HostError::io(&error, "A folder could not be made"))?;
        let from = parent
            .open_dir(name)
            .map_err(|error| HostError::io(&error, "A folder could not be opened"))?;
        let made = to
            .open_dir(Path::new(to_name))
            .map_err(|error| HostError::io(&error, "A folder could not be opened"))?;
        for entry in from
            .entries()
            .map_err(|error| HostError::io(&error, "A folder could not be read"))?
        {
            let entry =
                entry.map_err(|error| HostError::io(&error, "A folder could not be read"))?;
            let child = entry.file_name();
            copy_tree(&from, &child, &made, &child, budget)?;
        }
        // The folder's own mode last, so a read-only folder is filled first.
        to.set_permissions(Path::new(to_name), meta.permissions())
            .map_err(|error| HostError::io(&error, "A folder's permissions could not be copied"))?;
        return Ok(());
    }
    budget.bytes(name, meta.len())?;
    let mut input = parent.open(name).map_err(|error| {
        HostError::io(
            &error,
            format!("{} could not be read", name.to_string_lossy()),
        )
    })?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut output = to.open_with(Path::new(to_name), &options).map_err(|error| {
        HostError::io(
            &error,
            format!("{} could not be written", to_name.to_string_lossy()),
        )
    })?;
    let copied = io::copy(&mut (&mut input).take(meta.len() + 1), &mut output).map_err(|error| {
        HostError::io(
            &error,
            format!("{} could not be copied", name.to_string_lossy()),
        )
    })?;
    if copied != meta.len() {
        return Err(HostError::new(
            ErrorCode::Conflict,
            format!(
                "{} changed while it was being copied; nothing was copied",
                name.to_string_lossy()
            ),
        ));
    }
    output.flush().map_err(|error| {
        HostError::io(
            &error,
            format!("{} could not be written", to_name.to_string_lossy()),
        )
    })?;
    output
        .set_permissions(meta.permissions())
        .map_err(|error| HostError::io(&error, "A file's permissions could not be copied"))?;
    Ok(())
}

/// A private folder made in `folder` to build the new items in. Its name
/// starts with a dot and names this process, so two copies in one folder
/// never share one.
fn make_staging(folder: &Dir) -> HostResult<OsString> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    for attempt in 0..16u32 {
        let name = OsString::from(format!(
            ".hide-copy-{}-{stamp}-{attempt}",
            std::process::id()
        ));
        match folder.create_dir(Path::new(&name)) {
            Ok(()) => return Ok(name),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(HostError::io(
                    &error,
                    "A working folder for the copy could not be made",
                ));
            }
        }
    }
    Err(HostError::new(
        ErrorCode::Io,
        "A working folder for the copy could not be made",
    ))
}

/// Removes the working folder after a failure, and says so when it stays.
fn discard(folder: &Dir, staging: &OsStr, mut reason: HostError) -> HostError {
    if let Err(error) = folder.remove_dir_all(Path::new(staging)) {
        reason.message = format!(
            "{}; the unfinished copy {} could not be removed ({error})",
            reason.message,
            staging.to_string_lossy()
        );
    }
    reason
}

/// Moves `from` of `staged` to the first free name `wanted` allows in
/// `folder`, and answers that name.
fn place(
    staged: &Dir,
    from: &OsStr,
    folder: &Dir,
    wanted: &OsStr,
    directory: bool,
) -> HostResult<String> {
    let lossy = wanted.to_string_lossy().into_owned();
    let mut tries = copy_names(&lossy, directory);
    // The exact name first, even when it is not valid text.
    let mut candidate: OsString = wanted.to_owned();
    tries.next();
    loop {
        match atomic::rename_no_replace(staged, from, folder, &candidate) {
            Ok(()) => return Ok(candidate.to_string_lossy().into_owned()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let Some(next) = tries.next() else {
                    return Err(HostError::new(
                        ErrorCode::AlreadyExists,
                        format!("No free name for {lossy} was found; nothing was copied"),
                    ));
                };
                candidate = OsString::from(next);
            }
            Err(error) => {
                return Err(HostError::io(
                    &error,
                    format!("{lossy} could not be placed"),
                ));
            }
        }
    }
}

/// `relative` copied into the folder `destination`, whole, taking its name or
/// the first free `copy` name; the answer is the name it took. A folder is
/// not copied into itself or below itself, and a copy past
/// [`COPY_MAX_ITEMS`] items or [`COPY_MAX_BYTES`] bytes, or holding a link or
/// a special file, is refused before the first byte is written.
pub fn copy_into(dir: &Dir, relative: &Path, destination: &Path) -> HostResult<Copied> {
    if destination.starts_with(relative) {
        return Err(HostError::new(
            ErrorCode::InvalidPath,
            "A folder cannot be copied into itself",
        ));
    }
    let (parent, name) = open_parent(dir, relative)?;
    let folder = open_folder(dir, destination)?;
    measure(&parent, &name, &mut Budget::new())?;
    let directory = require_item(&parent, &name)?.is_dir();
    let staging = make_staging(&folder)?;
    let result = folder
        .open_dir(Path::new(&staging))
        .map_err(|error| HostError::io(&error, "The working folder could not be opened"))
        .and_then(|staged| {
            copy_tree(&parent, &name, &staged, &name, &mut Budget::new())?;
            place(&staged, &name, &folder, &name, directory)
        });
    match result {
        Ok(name) => {
            // Only the emptied working folder is left.
            let _ = folder.remove_dir(Path::new(&staging));
            Ok(Copied { name })
        }
        Err(error) => Err(discard(&folder, &staging, error)),
    }
}

/// One entry of a drop, checked: its path below the destination, native.
struct Checked<'a> {
    native: PathBuf,
    entry: &'a ImportEntry,
}

/// Checks a drop's entries against the caps and the shape the page promises:
/// every path a name below the destination, every file under a folder the
/// drop lists first, no path twice, and the bytes of every file within the
/// per-file and per-drop caps. Returns the entries with their native paths and
/// the names of the top-level items in order.
fn check_drop(entries: &[ImportEntry]) -> HostResult<(Vec<Checked<'_>>, Vec<String>)> {
    let refuse = |code, message: String| Err(HostError::new(code, message));
    if entries.is_empty() {
        return refuse(ErrorCode::InvalidRequest, "A drop holds nothing".to_owned());
    }
    if entries.len() > IMPORT_MAX_ITEMS {
        return refuse(
            ErrorCode::TooLarge,
            format!("A drop may hold {IMPORT_MAX_ITEMS} items; nothing was added"),
        );
    }
    let mut checked = Vec::with_capacity(entries.len());
    let mut top = Vec::new();
    let mut folders = std::collections::HashSet::new();
    let mut seen = std::collections::HashSet::new();
    let mut total = 0u64;
    for entry in entries {
        let path = RelPath::parse(&entry.path).map_err(|_| {
            HostError::new(
                ErrorCode::InvalidPath,
                format!("{} is not a path below the folder", entry.path),
            )
        })?;
        if path.is_root() {
            return refuse(ErrorCode::InvalidPath, "A drop item has no name".to_owned());
        }
        for name in path.names() {
            valid_name(name)?;
        }
        if !seen.insert(entry.path.clone()) {
            return refuse(
                ErrorCode::InvalidPath,
                format!("{} is in the drop twice", entry.path),
            );
        }
        match path.parent() {
            Some(parent) if parent.is_root() => top.push(entry.path.clone()),
            Some(parent) if folders.contains(parent.as_str()) => {}
            _ => {
                return refuse(
                    ErrorCode::InvalidPath,
                    format!("{} is not inside a folder of the drop", entry.path),
                );
            }
        }
        if entry.directory {
            if !entry.data.is_empty() {
                return refuse(
                    ErrorCode::InvalidRequest,
                    format!("{} is a folder and carries bytes", entry.path),
                );
            }
            folders.insert(entry.path.clone());
        } else {
            // Base64 carries 3 bytes in 4 characters; the exact count is
            // checked after the decode.
            let bound = (entry.data.len() as u64 / 4) * 3;
            if bound > IMPORT_MAX_FILE_BYTES + 3 {
                return refuse(
                    ErrorCode::TooLarge,
                    format!(
                        "{} is larger than {} MiB; nothing was added",
                        entry.path,
                        IMPORT_MAX_FILE_BYTES / (1024 * 1024)
                    ),
                );
            }
            total = total.saturating_add(bound);
            if total > IMPORT_MAX_BYTES + 3 * IMPORT_MAX_ITEMS as u64 {
                return refuse(
                    ErrorCode::TooLarge,
                    format!(
                        "A drop may hold {} MiB; nothing was added",
                        IMPORT_MAX_BYTES / (1024 * 1024)
                    ),
                );
            }
        }
        checked.push(Checked {
            native: path
                .to_native()
                .map_err(|error| HostError::new(ErrorCode::InvalidPath, error.to_string()))?,
            entry,
        });
    }
    Ok((checked, top))
}

/// The files and folders of a drop written into the folder `destination`.
/// Each top-level item takes its name or the first free `copy` name, and the
/// answer lists the names in order. The items are built in a working folder
/// first, so a drop that fails midway leaves nothing at all; one that fails
/// while its items are being moved to their names says which landed and which
/// did not.
pub fn import(dir: &Dir, destination: &Path, entries: &[ImportEntry]) -> HostResult<Imported> {
    let (checked, top) = check_drop(entries)?;
    let folder = open_folder(dir, destination)?;
    let staging = make_staging(&folder)?;
    let staged = folder
        .open_dir(Path::new(&staging))
        .map_err(|error| HostError::io(&error, "The working folder could not be opened"));
    let staged = match staged {
        Ok(staged) => staged,
        Err(error) => return Err(discard(&folder, &staging, error)),
    };
    let written = write_drop(&staged, &checked);
    if let Err(error) = written {
        return Err(discard(&folder, &staging, error));
    }
    let mut names = Vec::new();
    for item in &top {
        let directory = checked
            .iter()
            .any(|checked| checked.entry.path == *item && checked.entry.directory);
        match place(&staged, OsStr::new(item), &folder, OsStr::new(item), directory) {
            Ok(name) => names.push(name),
            Err(mut error) => {
                if !names.is_empty() {
                    error.message = format!(
                        "{}; {} of {} items were added ({}), the rest were not",
                        error.message,
                        names.len(),
                        top.len(),
                        names.join(", ")
                    );
                }
                return Err(discard(&folder, &staging, error));
            }
        }
    }
    let _ = folder.remove_dir(Path::new(&staging));
    Ok(Imported { names })
}

fn write_drop(staged: &Dir, checked: &[Checked<'_>]) -> HostResult<()> {
    let mut written = 0u64;
    for item in checked {
        if item.entry.directory {
            staged.create_dir(&item.native).map_err(|error| {
                HostError::io(&error, format!("{} could not be made", item.entry.path))
            })?;
            continue;
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&item.entry.data)
            .map_err(|error| {
                HostError::new(
                    ErrorCode::InvalidRequest,
                    format!("{} arrived damaged: {error}", item.entry.path),
                )
            })?;
        written = written.saturating_add(bytes.len() as u64);
        if bytes.len() as u64 > IMPORT_MAX_FILE_BYTES || written > IMPORT_MAX_BYTES {
            return Err(HostError::new(
                ErrorCode::TooLarge,
                format!(
                    "The drop is larger than {} MiB; nothing was added",
                    IMPORT_MAX_BYTES / (1024 * 1024)
                ),
            ));
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = staged.open_with(&item.native, &options).map_err(|error| {
            HostError::io(&error, format!("{} could not be written", item.entry.path))
        })?;
        file.write_all(&bytes).map_err(|error| {
            HostError::io(&error, format!("{} could not be written", item.entry.path))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(name: &str, directory: bool, count: usize) -> Vec<String> {
        copy_names(name, directory).take(count).collect()
    }

    #[test]
    fn a_taken_name_becomes_copy_then_copy_two() {
        assert_eq!(
            names("a.txt", false, 4),
            ["a.txt", "a copy.txt", "a copy 2.txt", "a copy 3.txt"]
        );
        assert_eq!(names("src", true, 3), ["src", "src copy", "src copy 2"]);
    }

    #[test]
    fn only_a_files_last_dot_makes_an_extension() {
        assert_eq!(names("a.tar.gz", false, 2), ["a.tar.gz", "a.tar copy.gz"]);
        assert_eq!(names(".env", false, 2), [".env", ".env copy"]);
        assert_eq!(names("v1.2", true, 2), ["v1.2", "v1.2 copy"]);
        assert_eq!(names("Makefile", false, 2), ["Makefile", "Makefile copy"]);
    }

    #[test]
    fn the_name_tries_are_bounded() {
        assert_eq!(copy_names("a", false).count(), NAME_TRIES);
    }
}
