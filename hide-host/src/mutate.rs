//! The Explorer's changes to a checkout: a new file or folder, a rename, a
//! move into another folder, and a move to the Trash (PRD S5.5 B16-B18).
//!
//! Every change runs through handles opened under the checkout root, so a
//! path that leaves it, a link that points out of it, or a folder replaced at
//! a checked spelling cannot redirect the change. Nothing here overwrites: a
//! creation uses the exclusive open, and a rename or move uses the kernel's
//! exclusive rename (`RENAME_EXCL` on macOS, `RENAME_NOREPLACE` on Linux), so
//! a name that appears between the caller's decision and the call is refused
//! rather than replaced. There is no permanent delete: a target whose Trash
//! cannot take the item leaves it where it was (B17).

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, HostError, HostResult};
use crate::root::{Root, identity_of, open_parent};

/// What a change did, as the caller's result carries it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Changed {}

/// A name a creation or a rename may carry: one component, and not one of the
/// shapes that would name a different path.
pub fn valid_name(name: &str) -> HostResult<&str> {
    let refuse = |message: String| Err(HostError::new(ErrorCode::InvalidPath, message));
    if name.is_empty() {
        return refuse("A name is required".to_owned());
    }
    if name.contains('/') {
        return refuse("A name cannot contain /".to_owned());
    }
    if name.contains('\0') {
        return refuse("A name cannot contain NUL".to_owned());
    }
    if name == "." || name == ".." {
        return refuse(format!("{name} is not a valid name"));
    }
    Ok(name)
}

/// A new empty file, or a new folder, named `name` in `parent`.
pub fn create(dir: &Dir, parent: &Path, name: &str, directory: bool) -> HostResult<Changed> {
    let name = valid_name(name)?;
    let folder = open_folder(dir, parent)?;
    let folder_name = display_name(parent);
    let result = if directory {
        folder.create_dir(Path::new(name))
    } else {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        folder.open_with(Path::new(name), &options).map(drop)
    };
    result.map_err(|error| describe(&error, name, &folder_name))?;
    Ok(Changed {})
}

/// `relative` takes the name `name` in the folder it is in.
pub fn rename(dir: &Dir, relative: &Path, name: &str) -> HostResult<Changed> {
    let name = valid_name(name)?;
    let (parent, current) = open_parent(dir, relative)?;
    if current == OsStr::new(name) {
        return Err(HostError::new(
            ErrorCode::InvalidPath,
            "The name is unchanged",
        ));
    }
    require_item(&parent, &current)?;
    let folder_name = display_name(relative.parent().unwrap_or(Path::new("")));
    rename_exclusive(&parent, &current, &parent, OsStr::new(name))
        .map_err(|error| describe(&error, name, &folder_name))?;
    Ok(Changed {})
}

/// `relative` moves into the folder `destination`, keeping its name.
pub fn move_into(dir: &Dir, relative: &Path, destination: &Path) -> HostResult<Changed> {
    if destination == relative || destination.starts_with(relative) {
        return Err(HostError::new(
            ErrorCode::InvalidPath,
            "A folder cannot be moved into itself",
        ));
    }
    if relative.parent().unwrap_or(Path::new("")) == destination {
        return Err(HostError::new(
            ErrorCode::InvalidPath,
            "The item is already in that folder",
        ));
    }
    let (parent, name) = open_parent(dir, relative)?;
    require_item(&parent, &name)?;
    let target = open_folder(dir, destination)?;
    rename_exclusive(&parent, &name, &target, &name)
        .map_err(|error| describe(&error, &name.to_string_lossy(), &display_name(destination)))?;
    Ok(Changed {})
}

/// The identity of an item as a listing shows it and a trash checks it: the
/// inode of the entry itself, a link's own and not its target's.
pub fn item_inode(metadata: &cap_std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        metadata.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0
    }
}

/// Moves `relative` to the Trash of the machine that holds it.
///
/// `expected_inode` is the item the operator confirmed: an item replaced at
/// that path while the prompt was open is refused and stays. The platform
/// Trash call takes a pathname, which a change to the checkout's spelling
/// could redirect, so the item is first moved through its opened parent into
/// a private folder outside the checkout, checked again there, and only then
/// handed to the Trash from that independent path. A Trash that refuses it
/// puts it back where it was; an item that cannot be put back stays in the
/// private folder, which the refusal names (B17, B18).
pub fn trash(root: &Root, relative: &Path, expected_inode: Option<u64>) -> HostResult<Changed> {
    let (parent, name) = open_parent(root.dir(), relative)?;
    let item = name.to_string_lossy().into_owned();
    let present = require_item(&parent, &name)?;
    if expected_inode.is_some_and(|expected| item_inode(&present) != expected) {
        return Err(HostError::new(
            ErrorCode::Conflict,
            format!("{item} changed while the prompt was open; nothing was moved"),
        ));
    }
    let stage = tempfile::Builder::new()
        .prefix("hide-trash-")
        .tempdir()
        .map_err(|error| HostError::io(&error, "Trash staging could not be created"))?;
    if stage_is_inside(stage.path(), root.dir())
        .map_err(|error| HostError::io(&error, "Trash staging could not be inspected"))?
    {
        return Err(HostError::new(
            ErrorCode::Unsupported,
            "Trash staging must be outside the checkout; nothing was moved",
        ));
    }
    let staged = Dir::open_ambient_dir(stage.path(), cap_std::ambient_authority())
        .map_err(|error| HostError::io(&error, "Trash staging could not be opened"))?;
    rename_exclusive(&parent, &name, &staged, &name).map_err(|error| {
        if error.kind() == io::ErrorKind::CrossesDevices {
            HostError::new(
                ErrorCode::Unsupported,
                format!("{item} is on another volume than the Trash staging; it was left in place"),
            )
        } else {
            describe(
                &error,
                &item,
                &display_name(relative.parent().unwrap_or(Path::new(""))),
            )
        }
    })?;
    // The item is only in the staging folder now: put it back on any
    // refusal, and keep the folder for the operator if that fails too.
    let put_back = |reason: HostError| -> HostError {
        match rename_exclusive(&staged, &name, &parent, &name) {
            Ok(()) => reason,
            Err(error) => {
                let kept = stage.path().join(&name);
                HostError::new(
                    ErrorCode::Io,
                    format!(
                        "{}; {item} could not be put back ({error}) and is kept at {}",
                        reason.message,
                        kept.display()
                    ),
                )
            }
        }
    };
    let refusal = if let Some(expected) = expected_inode
        && staged
            .symlink_metadata(Path::new(&name))
            .map(|metadata| item_inode(&metadata))
            .ok()
            != Some(expected)
    {
        put_back(HostError::new(
            ErrorCode::Conflict,
            format!("{item} changed while the prompt was open; nothing was moved"),
        ))
    } else {
        match move_to_trash(&stage.path().join(&name)) {
            Ok(()) => return Ok(Changed {}),
            Err(reason) => put_back(HostError::new(
                ErrorCode::Unsupported,
                format!("{item} could not be moved to the Trash: {reason}; it was left in place"),
            )),
        }
    };
    if staged.symlink_metadata(Path::new(&name)).is_ok() {
        let _ = stage.keep();
    }
    Err(refusal)
}

fn open_folder(dir: &Dir, relative: &Path) -> HostResult<Dir> {
    let opened = if relative.as_os_str().is_empty() {
        dir.try_clone()
    } else {
        dir.open_dir(relative)
    };
    opened.map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => HostError::new(
            ErrorCode::NotFound,
            format!("The folder {} no longer exists", display_name(relative)),
        ),
        _ => HostError::io(
            &error,
            format!("The folder {} could not be opened", display_name(relative)),
        ),
    })
}

/// The item a rename, move or trash starts from, as it is now. One that is
/// already gone is named as such rather than reported as a failed write.
fn require_item(parent: &Dir, name: &OsStr) -> HostResult<cap_std::fs::Metadata> {
    parent.symlink_metadata(Path::new(name)).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            HostError::new(
                ErrorCode::NotFound,
                format!("{} no longer exists", name.to_string_lossy()),
            )
        } else {
            HostError::io(
                &error,
                format!("{} could not be inspected", name.to_string_lossy()),
            )
        }
    })
}

fn display_name(relative: &Path) -> String {
    relative
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "the checkout".to_owned())
}

/// The sentence the operator reads for a failed change, with the code the
/// caller branches on.
fn describe(error: &io::Error, name: &str, folder: &str) -> HostError {
    let mut described = HostError::io(error, String::new());
    described.message = match error.kind() {
        io::ErrorKind::AlreadyExists => format!("{name} already exists in {folder}"),
        io::ErrorKind::NotFound => format!("The folder {folder} no longer exists"),
        io::ErrorKind::PermissionDenied if described.code == ErrorCode::OutsideRoot => {
            format!("{name} is outside the checkout")
        }
        io::ErrorKind::PermissionDenied => format!("{folder} is not writable"),
        io::ErrorKind::CrossesDevices => {
            "Items can only be moved within the same volume".to_owned()
        }
        _ => format!("{name} could not be written: {error}"),
    };
    described
}

/// Whether the private staging folder lies inside the checkout, compared by
/// the opened root's identity and not by its mutable spelling.
fn stage_is_inside(stage: &Path, root: &Dir) -> io::Result<bool> {
    let root = identity_of(root)?;
    let real = stage.canonicalize()?;
    for ancestor in real.ancestors() {
        let metadata = std::fs::metadata(ancestor)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.dev() == root.device && metadata.ino() == root.inode {
                return Ok(true);
            }
        }
        #[cfg(not(unix))]
        let _ = metadata;
    }
    Ok(false)
}

#[cfg(target_os = "macos")]
fn rename_exclusive(from_dir: &Dir, from: &OsStr, to_dir: &Dir, to: &OsStr) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let from = CString::new(from.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid source name"))?;
    let to = CString::new(to.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid destination name"))?;
    // SAFETY: both names are NUL-terminated and outlive the call, and both
    // descriptors are open directories borrowed for its duration.
    let result = unsafe {
        libc::renameatx_np(
            from_dir.as_raw_fd(),
            from.as_ptr(),
            to_dir.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn rename_exclusive(from_dir: &Dir, from: &OsStr, to_dir: &Dir, to: &OsStr) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let from = CString::new(from.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid source name"))?;
    let to = CString::new(to.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid destination name"))?;
    // SAFETY: as on macOS.
    let result = unsafe {
        libc::renameat2(
            from_dir.as_raw_fd(),
            from.as_ptr(),
            to_dir.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn rename_exclusive(from_dir: &Dir, from: &OsStr, to_dir: &Dir, to: &OsStr) -> io::Result<()> {
    if to_dir.symlink_metadata(Path::new(to)).is_ok() {
        return Err(io::Error::from(io::ErrorKind::AlreadyExists));
    }
    from_dir.rename(Path::new(from), to_dir, Path::new(to))
}

/// Moves the item to the Trash through `NSFileManager` rather than through
/// Finder, which is the crate's default. The Finder route runs `osascript`
/// and asks macOS for Automation permission on first use, which an SSH
/// session cannot answer; the file-manager route needs no permission and no
/// subprocess. What it gives up is Finder's "Put Back" on some systems; the
/// item is still in the Trash and restores by dragging it out.
fn move_to_trash(path: &Path) -> Result<(), String> {
    let mut context = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        context.set_delete_method(DeleteMethod::NsFileManager);
    }
    context.delete(path).map_err(|error| match error {
        trash::Error::CouldNotAccess { .. } => "it is not accessible".to_owned(),
        trash::Error::TargetedRoot => "it is a volume root".to_owned(),
        trash::Error::Unknown { description } | trash::Error::Os { description, .. } => description,
        other => other.to_string(),
    })
}
