//! Writing files to another machine over SFTP, shared by the device kit's
//! install (`host`) and the core move's copy (`upstream`).
//!
//! Each write names its own offset, so the order they land in does not
//! matter; every one's answer is checked. One request at a time made each
//! 32 KiB wait a round trip, so a 41 MB helper took about two minutes at a
//! 78 ms link; sixteen in flight is 512 KiB per round trip.

use std::path::Path;

use futures_util::{TryStreamExt, stream};
use russh_sftp::client::RawSftpSession;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{FileAttributes, OpenFlags, StatusCode};
use tokio::io::AsyncReadExt;

/// How much of a file one SFTP request carries, and how many requests an
/// upload or a read-back keeps in flight.
pub(super) const TRANSFER_CHUNK: usize = 32 * 1024;
pub(super) const TRANSFERS_IN_FLIGHT: usize = 16;

/// Writes `chunks`, each with its offset, to the open `handle`.
pub(super) async fn write_chunks<E>(
    raw: &RawSftpSession,
    handle: &str,
    chunks: impl futures_util::Stream<Item = Result<(u64, Vec<u8>), E>>,
    failed: impl Fn(SftpError) -> E + Copy,
) -> Result<(), E> {
    chunks
        .map_ok(|(offset, chunk)| {
            let handle = handle.to_owned();
            async move {
                raw.write(handle, offset, chunk)
                    .await
                    .map(|_| ())
                    .map_err(failed)
            }
        })
        .try_buffer_unordered(TRANSFERS_IN_FLIGHT)
        .try_collect::<()>()
        .await
}

/// Writes `bytes` from memory.
pub(super) async fn write_bytes<E>(
    raw: &RawSftpSession,
    handle: &str,
    bytes: &[u8],
    failed: impl Fn(SftpError) -> E + Copy,
) -> Result<(), E> {
    let chunks = stream::iter(
        bytes
            .chunks(TRANSFER_CHUNK)
            .enumerate()
            .map(|(index, chunk)| Ok(((index * TRANSFER_CHUNK) as u64, chunk.to_vec()))),
    );
    write_chunks(raw, handle, chunks, failed).await
}

/// One file of a copy that does not fit in memory: where it is here and
/// where it goes there.
#[derive(Clone, Debug)]
pub struct FileUpload {
    pub local: std::path::PathBuf,
    /// An absolute path on the other machine, `/`-separated.
    pub remote: String,
}

/// Why a copy stopped.
#[derive(Debug)]
pub enum UploadError {
    /// A file here could not be read.
    Local(String),
    /// The other machine refused a request or the channel failed.
    Remote(String),
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(reason) => write!(formatter, "local: {reason}"),
            Self::Remote(reason) => write!(formatter, "remote: {reason}"),
        }
    }
}

/// Streams `file` to `<remote>.part` beside its final name, at most
/// [`TRANSFERS_IN_FLIGHT`] chunks read ahead, and renames it into place, so
/// the final name only ever holds a whole copy. Each folder on the way is
/// made private (0700) when it is missing. `sent` hears each chunk's size
/// as it is answered.
pub(super) async fn upload_file(
    raw: &RawSftpSession,
    file: &FileUpload,
    sent: &(dyn Fn(u64) + Sync),
) -> Result<(), UploadError> {
    let remote = |what: &str, error: SftpError| {
        UploadError::Remote(format!("{what} {}: {error}", file.remote))
    };
    if !file.remote.starts_with('/') || file.remote.split('/').any(|part| part == "..") {
        return Err(UploadError::Remote(format!(
            "{} is not a plain absolute path",
            file.remote
        )));
    }
    if let Some((folder, _)) = file.remote.rsplit_once('/') {
        make_folders(raw, folder).await?;
    }
    let part = format!("{}.part", file.remote);
    let _ = raw.remove(&part).await;
    let handle = raw
        .open(
            &part,
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
            FileAttributes {
                permissions: Some(0o600),
                ..FileAttributes::empty()
            },
        )
        .await
        .map_err(|error| remote("could not open", error))?
        .handle;
    let local = tokio::fs::File::open(&file.local)
        .await
        .map_err(|error| UploadError::Local(format!("{}: {error}", file.local.display())))?;
    let chunks = stream::try_unfold((local, 0_u64), |(mut local, offset)| async move {
        let mut chunk = vec![0_u8; TRANSFER_CHUNK];
        let mut filled = 0;
        while filled < TRANSFER_CHUNK {
            match local.read(&mut chunk[filled..]).await {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(error) => return Err(UploadError::Local(error.to_string())),
            }
        }
        if filled == 0 {
            return Ok(None);
        }
        chunk.truncate(filled);
        Ok(Some(((offset, chunk), (local, offset + filled as u64))))
    })
    .inspect_ok(|(_, chunk)| sent(chunk.len() as u64));
    let written = write_chunks(raw, &handle, chunks, |error| {
        UploadError::Remote(format!("write {}: {error}", file.remote))
    })
    .await;
    let closed = raw.close(handle).await;
    if let Err(error) = written {
        let _ = raw.remove(&part).await;
        return Err(error);
    }
    closed.map_err(|error| remote("could not finish", error))?;
    let _ = raw.remove(&file.remote).await;
    raw.rename(&part, &file.remote)
        .await
        .map_err(|error| remote("could not be put in place", error))?;
    Ok(())
}

/// Makes each missing folder of `folder` private, parents first.
async fn make_folders(raw: &RawSftpSession, folder: &str) -> Result<(), UploadError> {
    let mut path = String::new();
    for name in folder.split('/').filter(|name| !name.is_empty()) {
        path.push('/');
        path.push_str(name);
        match raw.lstat(&path).await {
            Ok(found) if found.attrs.is_dir() => continue,
            Ok(_) => {
                return Err(UploadError::Remote(format!("{path} is not a folder")));
            }
            Err(SftpError::Status(status)) if status.status_code == StatusCode::NoSuchFile => {}
            Err(error) => return Err(UploadError::Remote(format!("{path}: {error}"))),
        }
        raw.mkdir(
            &path,
            FileAttributes {
                permissions: Some(0o700),
                ..FileAttributes::empty()
            },
        )
        .await
        .map_err(|error| UploadError::Remote(format!("could not make {path}: {error}")))?;
    }
    Ok(())
}

/// The size of a local file, for the copy's progress total.
pub fn local_size(path: &Path) -> Result<u64, UploadError> {
    std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| UploadError::Local(format!("{}: {error}", path.display())))
}
