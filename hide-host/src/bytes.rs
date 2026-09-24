//! One range of a regular file's bytes under an opened root, for a device
//! file's image, PDF and video viewers and its download (PRD S5.5 B8, B39).
//!
//! A range is at most `MAX_RANGE`, the size of one binary frame on the
//! daemon's socket, so a device read is as bounded as a local one. Each range
//! names the file it was read from (`FileStamp`); the daemon stops a read whose
//! file changed between ranges rather than joining two files' bytes.

use base64::Engine;
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::{ErrorCode, HostError, HostResult};

/// The most one range carries: one binary frame on the daemon's socket.
pub const MAX_RANGE: u64 = 4 * 1024 * 1024;

/// Which file a range came from: the same device, inode, size and
/// modification time across ranges is the same unchanged file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileStamp {
    pub device: u64,
    pub inode: u64,
    pub modified_ns: i128,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Range {
    /// The file's whole size when this range was read.
    pub total: u64,
    pub offset: u64,
    /// The bytes, base64 encoded for the JSON line.
    pub data: String,
    pub file: FileStamp,
}

impl Range {
    pub fn bytes(&self) -> HostResult<Vec<u8>> {
        base64::engine::general_purpose::STANDARD
            .decode(&self.data)
            .map_err(|error| {
                HostError::new(
                    ErrorCode::Io,
                    format!("The file's bytes arrived damaged: {error}"),
                )
            })
    }
}

/// Reads at most `length` bytes (and never more than `MAX_RANGE`) of the
/// regular file `relative` from `offset`. An offset past the end reads none.
pub fn read(dir: &Dir, relative: &Path, offset: u64, length: u64) -> HostResult<Range> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        // A FIFO must not stall the reader waiting for a writer.
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
    }
    let failed =
        |error: &std::io::Error| HostError::io(error, "The selected file could not be read");
    let mut file = dir
        .open_with(relative, &options)
        .map_err(|error| failed(&error))?
        .into_std();
    let metadata = file.metadata().map_err(|error| failed(&error))?;
    if !metadata.is_file() {
        return Err(HostError::new(
            ErrorCode::NotAFile,
            "Only existing regular files can be read",
        ));
    }
    let total = metadata.len();
    let start = offset.min(total);
    let wanted = length.min(MAX_RANGE).min(total - start);
    file.seek(SeekFrom::Start(start))
        .map_err(|error| failed(&error))?;
    let mut bytes = Vec::with_capacity(wanted as usize);
    file.take(wanted)
        .read_to_end(&mut bytes)
        .map_err(|error| failed(&error))?;
    Ok(Range {
        total,
        offset: start,
        data: base64::engine::general_purpose::STANDARD.encode(&bytes),
        file: stamp(&metadata),
    })
}

#[cfg(unix)]
fn stamp(metadata: &std::fs::Metadata) -> FileStamp {
    use std::os::unix::fs::MetadataExt;
    FileStamp {
        device: metadata.dev(),
        inode: metadata.ino(),
        modified_ns: i128::from(metadata.mtime()) * 1_000_000_000
            + i128::from(metadata.mtime_nsec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_is_bounded_and_names_its_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.bin"), b"0123456789").unwrap();
        let root = Dir::open_ambient_dir(dir.path(), cap_std::ambient_authority()).unwrap();
        let range = read(&root, Path::new("a.bin"), 3, 4).unwrap();
        assert_eq!(range.total, 10);
        assert_eq!(range.offset, 3);
        assert_eq!(range.bytes().unwrap(), b"3456");
        let past = read(&root, Path::new("a.bin"), 50, 4).unwrap();
        assert_eq!((past.offset, past.bytes().unwrap().len()), (10, 0));
        std::fs::write(dir.path().join("b.bin"), b"x").unwrap();
        std::fs::rename(dir.path().join("b.bin"), dir.path().join("a.bin")).unwrap();
        let replaced = read(&root, Path::new("a.bin"), 0, 4).unwrap();
        assert_ne!(replaced.file, range.file, "another file is another stamp");
    }

    #[test]
    fn a_folder_is_not_read_as_bytes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let root = Dir::open_ambient_dir(dir.path(), cap_std::ambient_authority()).unwrap();
        let error = read(&root, Path::new("sub"), 0, 4).unwrap_err();
        assert_eq!(error.code, ErrorCode::NotAFile);
    }
}
