//! Attachment staging uses the authenticated russh boundary, never shell commands.
use super::*;
use crate::terminal_attachments::{
    AttachmentFile, MAX_STAGED_BYTES, MAX_STAGED_FILES, STAGING_TTL_SECONDS, check_cancelled,
    valid_request_id,
};
use russh_sftp::client::{RawSftpSession, error::Error as SftpError};
use russh_sftp::protocol::{FileAttributes, StatusCode};
use std::time::{SystemTime, UNIX_EPOCH};

const ROOT_NAME: &str = ".hide-terminal-attachments";
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(45);

fn transport_failure(error: impl std::fmt::Display) -> String {
    crate::diagnostic!(
        json!({"kind":"terminal.attachment.transport_failed", "error":error.to_string()})
    );
    "Remote transfer failed. Check the device connection and SFTP access permissions, then retry."
        .to_owned()
}

fn attachment_name(request_id: &str, index: usize, file: &AttachmentFile) -> String {
    let extension = Path::new(&file.name)
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| {
            value.len() <= 12 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
        });
    format!(
        "hide-{request_id}-{index}{}",
        extension
            .map(|value| format!(".{value}"))
            .unwrap_or_default()
    )
}

impl RusshSftpTransport {
    /// Best-effort exact-intent cleanup. No directory recursion or unrelated deletion.
    pub(crate) fn remove_attachments(&self, request_id: &str, files: &[AttachmentFile]) {
        if !valid_request_id(request_id) || files.len() > crate::terminal_attachments::MAX_FILES {
            return;
        }
        let result = self.client.runtime.block_on(async {
            let session = tokio::time::timeout(
                SSH_OPERATION_TIMEOUT,
                self.client
                    .connect(KnownHostHandler::new(&self.client.host, None)),
            )
            .await
            .map_err(|_| "Cleanup connection timed out".to_owned())?
            .map_err(transport_failure)?;
            let result = tokio::time::timeout(Duration::from_secs(20), async {
                let channel = session
                    .channel_open_session()
                    .await
                    .map_err(transport_failure)?;
                channel
                    .request_subsystem(true, "sftp")
                    .await
                    .map_err(transport_failure)?;
                let raw = RawSftpSession::new(channel.into_stream());
                raw.set_timeout(5);
                let removed = async {
                    raw.init().await.map_err(transport_failure)?;
                    let home = raw
                        .realpath(".")
                        .await
                        .map_err(transport_failure)?
                        .files
                        .first()
                        .map(|entry| entry.filename.clone())
                        .ok_or("Missing remote home")?;
                    if !Path::new(&home).is_absolute() || home.chars().any(char::is_control) {
                        return Err("Unsafe remote home".to_owned());
                    }
                    let owner = raw
                        .lstat(&home)
                        .await
                        .map_err(transport_failure)?
                        .attrs
                        .uid
                        .ok_or("Missing remote owner")?;
                    let root = format!("{}/{ROOT_NAME}", home.trim_end_matches('/'));
                    validate_root(
                        &raw.lstat(&root).await.map_err(transport_failure)?.attrs,
                        owner,
                    )?;
                    for (index, file) in files.iter().enumerate() {
                        let path = format!("{root}/{}", attachment_name(request_id, index, file));
                        match raw.lstat(&path).await {
                            Ok(attrs)
                                if attrs.attrs.is_regular() && attrs.attrs.uid == Some(owner) =>
                            {
                                raw.remove(path).await.map_err(transport_failure)?;
                            }
                            Err(SftpError::Status(status))
                                if status.status_code == StatusCode::NoSuchFile => {}
                            _ => return Err("Refused unsafe cleanup entry".to_owned()),
                        }
                    }
                    Ok::<(), String>(())
                }
                .await;
                let _ = raw.close_session();
                removed
            })
            .await
            .map_err(|_| "Cleanup timed out".to_owned())
            .and_then(|result| result);
            let _ = tokio::time::timeout(
                Duration::from_secs(3),
                session.disconnect(Disconnect::ByApplication, "Attachment cleanup", "en"),
            )
            .await;
            result
        });
        if let Err(error) = result {
            crate::diagnostic!(
                json!({"kind":"terminal.attachment.cleanup_deferred", "request_id":request_id, "error":error})
            );
        }
    }

    pub(crate) fn stage_attachments(
        &self,
        request_id: &str,
        files: &[AttachmentFile],
        cancelled: &AtomicBool,
    ) -> Result<Vec<String>, String> {
        if !valid_request_id(request_id) {
            return Err("Invalid attachment request identity.".to_owned());
        }
        check_cancelled(cancelled)?;
        self.client.runtime.block_on(async {
            let mut session = tokio::time::timeout(SSH_OPERATION_TIMEOUT, self.client.connect(KnownHostHandler::new(&self.client.host, None)))
                .await.map_err(|_| "Attachment connection timed out. Check the device and retry.".to_owned())?
                .map_err(transport_failure)?;
            let result = self.stage_on_session(&mut session, request_id, files, cancelled).await;
            let disconnected = tokio::time::timeout(Duration::from_secs(3), session.disconnect(Disconnect::ByApplication, "Attachment transfer complete", "en")).await;
            if !matches!(disconnected, Ok(Ok(()))) {
                crate::diagnostic!(json!({"kind":"terminal.attachment.disconnect_failed", "request_id":request_id, "host_id":self.client.host.host_id}));
            }
            result
        })
    }

    async fn stage_on_session(
        &self,
        session: &mut Handle<KnownHostHandler>,
        request_id: &str,
        files: &[AttachmentFile],
        cancelled: &AtomicBool,
    ) -> Result<Vec<String>, String> {
        let channel = tokio::time::timeout(SSH_OPERATION_TIMEOUT, session.channel_open_session())
            .await
            .map_err(|_| "Attachment SFTP channel timed out.".to_owned())?
            .map_err(transport_failure)?;
        tokio::time::timeout(
            SSH_OPERATION_TIMEOUT,
            channel.request_subsystem(true, "sftp"),
        )
        .await
        .map_err(|_| "Attachment SFTP request timed out. Check the device and retry.".to_owned())?
        .map_err(transport_failure)?;
        let raw = RawSftpSession::new(channel.into_stream());
        raw.set_timeout(10);
        let mut created = Vec::new();
        let mut destinations = Vec::new();
        let transfer = tokio::time::timeout(TRANSFER_TIMEOUT, async {
            raw.init().await.map_err(transport_failure)?;
            let home = raw.realpath(".").await.map_err(transport_failure)?
                .files.first().map(|entry| entry.filename.clone()).ok_or("SFTP did not identify the remote home directory.")?;
            if !Path::new(&home).is_absolute() || home.chars().any(char::is_control) {
                return Err("SFTP returned an unsafe home directory.".to_owned());
            }
            let owner = raw.lstat(&home).await.map_err(transport_failure)?.attrs.uid.ok_or("SFTP did not report the directory owner.")?;
            let root = format!("{}/{ROOT_NAME}", home.trim_end_matches('/'));
            match raw.lstat(&root).await {
                Ok(attrs) => validate_root(&attrs.attrs, owner)?,
                Err(SftpError::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
                    raw.mkdir(&root, FileAttributes { permissions: Some(0o700), ..FileAttributes::empty() }).await.map_err(transport_failure)?;
                    let attrs = raw.lstat(&root).await.map_err(transport_failure)?;
                    validate_root(&attrs.attrs, owner)?;
                }
                Err(error) => return Err(transport_failure(error)),
            }
            check_cancelled(cancelled)?;
            let directory = raw.opendir(&root).await.map_err(transport_failure)?.handle;
            let budget = inspect_staging(&raw, &directory, &root, owner).await;
            let closed = raw.close(directory).await.map_err(transport_failure);
            let existing = budget?;
            closed?;
            let incoming = files.iter().enumerate().filter(|(index, file)| !existing.contains_key(&attachment_name(request_id, *index, file)));
            let (new_count, new_bytes) = incoming.fold((0, 0u64), |(count, bytes), (_, file)| (count + 1, bytes + file.bytes.len() as u64));
            if existing.len() + new_count > MAX_STAGED_FILES || existing.values().sum::<u64>().saturating_add(new_bytes) > MAX_STAGED_BYTES {
                return Err("Remote attachment storage is full (128 files or 256 MiB). Remove old files from .hide-terminal-attachments and retry; files expire after 24 hours.".to_owned());
            }
            for (index, file) in files.iter().enumerate() {
                check_cancelled(cancelled)?;
                let path = format!("{root}/{}", attachment_name(request_id, index, file));
                let handle = match raw.open(&path, OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE, FileAttributes { permissions: Some(0o600), ..FileAttributes::empty() }).await {
                    Ok(handle) => handle.handle,
                    Err(error) => {
                        // A retry may own a completed upload whose input was not accepted.
                        // Adopt only identical bytes at this intent's exact private filename.
                        if raw.lstat(&path).await.is_err() { return Err(transport_failure(error)); }
                        verify_existing(&raw, &path, file, owner, cancelled).await?;
                        destinations.push(path);
                        continue;
                    }
                };
                destinations.push(path.clone());
                created.push(path);
                let written = async {
                    for (chunk, bytes) in file.bytes.chunks(32 * 1024).enumerate() {
                        check_cancelled(cancelled)?;
                        raw.write(&handle, (chunk * 32 * 1024) as u64, bytes.to_vec()).await.map_err(transport_failure)?;
                    }
                    Ok::<(), String>(())
                }.await;
                let closed = raw.close(handle).await.map_err(transport_failure);
                written?;
                closed?;
            }
            check_cancelled(cancelled)?;
            Ok(destinations.clone())
        }).await.unwrap_or_else(|_| Err("Attachment transfer timed out. Retry when the device is available.".to_owned()));
        if transfer.is_err() {
            for path in &created {
                if !matches!(
                    tokio::time::timeout(Duration::from_secs(2), raw.remove(path)).await,
                    Ok(Ok(_))
                ) {
                    crate::diagnostic!(
                        json!({"kind":"terminal.attachment.cleanup_deferred", "request_id":request_id, "host_id":self.client.host.host_id})
                    );
                }
            }
        }
        if let Err(error) = raw.close_session() {
            crate::diagnostic!(
                json!({"kind":"terminal.attachment.sftp_close_failed", "request_id":request_id, "error":error.to_string()})
            );
        }
        transfer
    }
}

async fn verify_existing(
    raw: &RawSftpSession,
    path: &str,
    file: &AttachmentFile,
    owner: u32,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let attrs = raw.lstat(path).await.map_err(transport_failure)?.attrs;
    if !attrs.is_regular()
        || attrs.uid != Some(owner)
        || attrs.permissions.is_none_or(|mode| mode & 0o777 != 0o600)
        || attrs.size != Some(file.bytes.len() as u64)
    {
        return Err(
            "An earlier attachment at this request's destination differs. Cancel and paste again."
                .to_owned(),
        );
    }
    let handle = raw
        .open(path, OpenFlags::READ, FileAttributes::empty())
        .await
        .map_err(transport_failure)?
        .handle;
    let compared = async {
        let mut offset = 0;
        while offset < file.bytes.len() {
            check_cancelled(cancelled)?;
            let read = raw
                .read(
                    &handle,
                    offset as u64,
                    (file.bytes.len() - offset).min(32 * 1024) as u32,
                )
                .await
                .map_err(transport_failure)?
                .data;
            if read.is_empty()
                || read.len() > file.bytes.len() - offset
                || read != file.bytes[offset..offset + read.len()]
            {
                return Err("An earlier attachment changed. Cancel and paste again.".to_owned());
            }
            offset += read.len();
        }
        Ok(())
    }
    .await;
    let closed = raw.close(handle).await.map_err(transport_failure);
    compared?;
    closed?;
    Ok(())
}

fn validate_root(attrs: &FileAttributes, owner: u32) -> Result<(), String> {
    if !attrs.is_dir()
        || attrs.uid != Some(owner)
        || attrs
            .permissions
            .is_none_or(|permissions| permissions & 0o777 != 0o700)
    {
        return Err("Remote attachment directory must be a private, owned directory (0700), not a symbolic link. Fix .hide-terminal-attachments and retry.".to_owned());
    }
    Ok(())
}

fn owned_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("hide-") else {
        return false;
    };
    rest.len() > 37
        && rest.is_char_boundary(36)
        && valid_request_id(&rest[..36])
        && rest.as_bytes()[36] == b'-'
        && rest[37..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
}

async fn inspect_staging(
    raw: &RawSftpSession,
    directory: &str,
    root: &str,
    owner: u32,
) -> Result<BTreeMap<String, u64>, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Clock is before the Unix epoch.")?
        .as_secs();
    let mut visited = 0usize;
    let mut retained = BTreeMap::new();
    loop {
        let entries = match raw.readdir(directory).await {
            Ok(entries) => entries.files,
            Err(SftpError::Status(status)) if status.status_code == StatusCode::Eof => break,
            Err(error) => return Err(transport_failure(error)),
        };
        if entries.is_empty() {
            return Err("SFTP returned an incomplete attachment directory listing.".to_owned());
        }
        for entry in entries {
            if entry.filename == "." || entry.filename == ".." {
                continue;
            }
            visited += 1;
            if visited > MAX_STAGED_FILES {
                return Err("Remote attachment directory exceeds its 128-file inspection limit. Remove old attachments and retry.".to_owned());
            }
            if !owned_name(&entry.filename)
                || !entry.attrs.is_regular()
                || entry.attrs.uid != Some(owner)
                || entry
                    .attrs
                    .permissions
                    .is_none_or(|permissions| permissions & 0o777 != 0o600)
            {
                return Err("Remote attachment directory contains an unrecognized or unsafe entry. Inspect it before retrying.".to_owned());
            }
            let modified = entry
                .attrs
                .mtime
                .ok_or("SFTP did not report attachment modification time.")?
                as u64;
            if now.saturating_sub(modified) >= STAGING_TTL_SECONDS {
                raw.remove(format!("{root}/{}", entry.filename))
                    .await
                    .map_err(transport_failure)?;
            } else {
                let size = entry
                    .attrs
                    .size
                    .ok_or("SFTP did not report attachment size.")?;
                if size > MAX_STAGED_BYTES {
                    return Err("Remote attachment storage exceeds 256 MiB. Remove old attachments and retry.".to_owned());
                }
                retained.insert(entry.filename, size);
            }
        }
    }
    Ok(retained)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires HERDR_TEST_SSH_ALIAS; creates and removes only UUID attachment fixtures"]
    fn remote_attachment_sftp_roundtrip_probe() {
        let alias_name = std::env::var("HERDR_TEST_SSH_ALIAS").expect("configured SSH alias");
        let home = std::env::var_os("HOME").expect("HOME");
        let alias =
            SshAlias::from_config_file(&PathBuf::from(home).join(".ssh/config"), &alias_name)
                .expect("SSH alias resolves");
        let transport =
            RusshSftpTransport::new(Arc::new(RusshRemoteClient::new(alias).expect("client")));
        let unique = format!(
            "{:032x}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let request_id = format!(
            "{}-{}-{}-{}-{}",
            &unique[..8],
            &unique[8..12],
            &unique[12..16],
            &unique[16..20],
            &unique[20..]
        );
        let files = vec![AttachmentFile {
            path: "/never-paste-this-local-path.png".to_owned(),
            name: "한글 image.png".to_owned(),
            bytes: b"explicit attachment bytes\0\xff\r\n".to_vec(),
        }];
        struct Cleanup<'a>(&'a RusshSftpTransport, &'a str, &'a [AttachmentFile]);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                self.0.remove_attachments(self.1, self.2);
            }
        }
        let cleanup = Cleanup(&transport, &request_id, &files);
        let paths = transport
            .stage_attachments(&request_id, &files, &AtomicBool::new(false))
            .expect("upload");
        assert_eq!(paths.len(), 1);
        assert_ne!(paths[0], files[0].path);
        assert!(paths[0].contains("/.hide-terminal-attachments/hide-"));
        assert_eq!(
            transport.read(&paths[0]).expect("remote read"),
            files[0].bytes
        );
        assert_eq!(
            transport
                .stage_attachments(&request_id, &files, &AtomicBool::new(false))
                .expect("idempotent same-byte retry"),
            paths
        );
        assert!(
            transport
                .stage_attachments(&request_id, &files, &AtomicBool::new(true))
                .is_err()
        );
        drop(cleanup);
        assert!(
            transport.read(&paths[0]).is_err(),
            "exact generated remote file was removed"
        );
    }

    #[test]
    fn staging_rejects_links_shared_permissions_and_foreign_names() {
        let mut attrs = FileAttributes {
            uid: Some(42),
            permissions: Some(0o40700),
            ..FileAttributes::empty()
        };
        assert!(validate_root(&attrs, 42).is_ok());
        attrs.permissions = Some(0o120700);
        assert!(validate_root(&attrs, 42).is_err());
        attrs.permissions = Some(0o40755);
        assert!(validate_root(&attrs, 42).is_err());
        assert!(!owned_name("../victim"));
        assert!(!owned_name("hide-unrelated"));
        assert!(owned_name(
            "hide-01234567-0123-0123-0123-0123456789ab-0.png"
        ));
    }
}
