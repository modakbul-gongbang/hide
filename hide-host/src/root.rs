use std::path::{Component, Path, PathBuf};

use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, HostError, HostResult};

/// The directory a root named when it was first opened. A later open of the
/// same path that finds another directory there is refused, so a checkout
/// renamed or replaced between two requests cannot redirect the second one.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct RootIdentity {
    pub device: u64,
    pub inode: u64,
}

/// An opened checkout root: the handle every operation is confined to.
#[derive(Debug)]
pub struct Root {
    path: PathBuf,
    /// The canonical spelling of `path` when it was opened; an absolute link
    /// target under it is a place inside the checkout.
    real_path: PathBuf,
    dir: Dir,
    identity: RootIdentity,
}

impl Root {
    /// Opens `path` and records which directory it names.
    pub fn open(path: &Path) -> HostResult<Self> {
        if !path.is_absolute() {
            return Err(HostError::new(
                ErrorCode::InvalidPath,
                "A checkout root must be an absolute path",
            ));
        }
        let dir = Dir::open_ambient_dir(path, cap_std::ambient_authority())
            .map_err(|error| HostError::io(&error, "The checkout folder could not be opened"))?;
        let identity = identity_of(&dir)
            .map_err(|error| HostError::io(&error, "The checkout folder could not be inspected"))?;
        let real_path = std::fs::canonicalize(path)
            .map_err(|error| HostError::io(&error, "The checkout folder could not be resolved"))?;
        Ok(Self {
            path: path.to_path_buf(),
            real_path,
            dir,
            identity,
        })
    }

    /// Opens `path` and refuses it unless it is still the directory `expected`
    /// names.
    pub fn open_pinned(path: &Path, expected: RootIdentity) -> HostResult<Self> {
        let root = Self::open(path)?;
        if root.identity != expected {
            return Err(HostError::new(
                ErrorCode::RootReplaced,
                "The checkout folder was replaced since it was opened; nothing was read or changed",
            ));
        }
        Ok(root)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn dir(&self) -> &Dir {
        &self.dir
    }

    pub fn real_path(&self) -> &Path {
        &self.real_path
    }

    pub fn identity(&self) -> RootIdentity {
        self.identity
    }
}

pub fn identity_of(dir: &Dir) -> std::io::Result<RootIdentity> {
    let metadata = dir.dir_metadata()?;
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        Ok(RootIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "directory identity is unavailable on this platform",
        ))
    }
}

/// A path inside a root, as a caller spells it: `/`-separated components, no
/// leading `/`, no `.` or `..`, no NUL. The empty string is the root itself.
/// The handle is what actually confines the work; this refuses the shapes
/// that could never name a child, before anything is opened.
pub fn relative_path(raw: &str) -> HostResult<PathBuf> {
    let invalid = || {
        HostError::new(
            ErrorCode::InvalidPath,
            "The path is not inside the checkout",
        )
    };
    if raw.contains('\0') || raw.starts_with('/') {
        return Err(invalid());
    }
    let path = Path::new(raw);
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            _ => return Err(invalid()),
        }
    }
    Ok(clean)
}

/// The parent directory handle of `relative` and the final name, opened
/// through `dir`. The root itself has no parent inside the checkout.
pub fn open_parent(dir: &Dir, relative: &Path) -> HostResult<(Dir, std::ffi::OsString)> {
    let name = relative.file_name().ok_or_else(|| {
        HostError::new(ErrorCode::InvalidPath, "The checkout root is not an item")
    })?;
    let parent = relative
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let opened = dir
        .open_dir(parent)
        .map_err(|error| HostError::io(&error, "The folder could not be opened"))?;
    Ok((opened, name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_refuse_every_shape_that_leaves_the_root() {
        for raw in ["/etc/passwd", "../x", "a/../b", "./a", "a\0b"] {
            assert_eq!(
                relative_path(raw).unwrap_err().code,
                ErrorCode::InvalidPath,
                "{raw}"
            );
        }
        assert_eq!(relative_path("").unwrap(), PathBuf::new());
        assert_eq!(relative_path("a/b.txt").unwrap(), PathBuf::from("a/b.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn a_root_replaced_at_its_path_is_refused() {
        let outer = tempfile::tempdir().unwrap();
        let path = outer.path().join("checkout");
        std::fs::create_dir(&path).unwrap();
        let identity = Root::open(&path).unwrap().identity();
        std::fs::rename(&path, outer.path().join("moved")).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert_eq!(
            Root::open_pinned(&path, identity).unwrap_err().code,
            ErrorCode::RootReplaced
        );
    }
}
