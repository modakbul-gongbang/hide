//! Bounded, explicit terminal file ingress. No provider draft or submission state.
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) const MAX_FILES: usize = 8;
pub(crate) const MAX_PATH_BYTES: usize = 4096;
pub(crate) const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
pub(crate) const MAX_REQUEST_BYTES: u64 = 40 * 1024 * 1024;
pub(crate) const MAX_QUEUED_INPUT: usize = 64 * 1024;
pub(crate) const MAX_STAGED_FILES: usize = 128;
pub(crate) const MAX_STAGED_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const STAGING_TTL_SECONDS: u64 = 24 * 60 * 60;

#[derive(Clone, Debug)]
pub(crate) struct AttachmentFile {
    pub path: String,
    pub name: String,
    pub bytes: Vec<u8>,
}

pub(crate) fn valid_request_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

pub(crate) fn clipboard_root(state_path: &Path) -> PathBuf {
    state_path.with_file_name("TerminalClipboard")
}

pub(crate) fn clipboard_path(state_path: &Path, request_id: &str) -> PathBuf {
    clipboard_root(state_path).join(format!("hide-{request_id}.png"))
}

pub(crate) fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        Err("File transfer was cancelled.".to_owned())
    } else {
        Ok(())
    }
}

pub(crate) fn read_sources(
    paths: &[String],
    cancelled: &AtomicBool,
) -> Result<Vec<AttachmentFile>, String> {
    if paths.is_empty() || paths.len() > MAX_FILES {
        return Err("Choose between 1 and 8 regular files.".to_owned());
    }
    let mut files = Vec::with_capacity(paths.len());
    let mut total = 0u64;
    for path in paths {
        check_cancelled(cancelled)?;
        let path = Path::new(path);
        if path.as_os_str().len() > MAX_PATH_BYTES
            || !path.is_absolute()
            || path
                .as_os_str()
                .as_encoded_bytes()
                .iter()
                .any(|byte| byte.is_ascii_control())
        {
            return Err(
                "File paths must be absolute and contain no control characters.".to_owned(),
            );
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| {
                "A selected file is unavailable or is a symbolic link. Choose a regular file."
                    .to_owned()
            })?;
        let before = file
            .metadata()
            .map_err(|_| "Could not inspect a selected file.".to_owned())?;
        if !before.is_file() {
            return Err("Folders, symbolic links and special files cannot be attached. Choose regular files.".to_owned());
        }
        if before.len() > MAX_FILE_BYTES {
            return Err("A file exceeds the 20 MiB attachment limit.".to_owned());
        }
        total = total
            .checked_add(before.len())
            .ok_or("Attachment size overflow.")?;
        if total > MAX_REQUEST_BYTES {
            return Err("The selected files exceed the 40 MiB total attachment limit.".to_owned());
        }
        let mut bytes = Vec::with_capacity(before.len() as usize);
        (&mut file)
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                "Could not read a selected file. Check its permissions and retry.".to_owned()
            })?;
        let after = file
            .metadata()
            .map_err(|_| "Could not recheck a selected file.".to_owned())?;
        if bytes.len() as u64 != before.len()
            || before.len() != after.len()
            || before.mtime() != after.mtime()
            || before.mtime_nsec() != after.mtime_nsec()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
        {
            return Err(
                "A selected file changed while it was being read. Choose it again.".to_owned(),
            );
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("A file name is not valid UTF-8.")?;
        files.push(AttachmentFile {
            path: path.to_string_lossy().into_owned(),
            name: name.to_owned(),
            bytes,
        });
    }
    Ok(files)
}

pub(crate) fn paste_bytes(paths: &[String], bracketed: bool) -> Result<Vec<u8>, String> {
    if paths.is_empty() || paths.len() > MAX_FILES {
        return Err("Choose between 1 and 8 regular files.".to_owned());
    }
    let mut quoted = Vec::with_capacity(paths.len());
    for path in paths {
        if path.len() > MAX_PATH_BYTES
            || !Path::new(path).is_absolute()
            || path.chars().any(char::is_control)
        {
            return Err("The attachment destination is not a safe absolute path.".to_owned());
        }
        let mut value = String::from("\"");
        for character in path.chars() {
            if "\\\"$`".contains(character) {
                value.push('\\');
            }
            value.push(character);
        }
        value.push('"');
        quoted.push(if bracketed {
            format!("\u{1b}[200~{value}\u{1b}[201~")
        } else {
            value
        });
    }
    let mut text = quoted.join(" ");
    if !bracketed {
        text.push(' ');
    }
    Ok(text.into_bytes())
}

pub(crate) fn remove_clipboard(state_path: &Path, request_id: &str) {
    // This exact generated child is the only local file this feature may remove.
    let path = clipboard_path(state_path, request_id);
    if let Err(error) = fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        crate::diagnostic!(
            serde_json::json!({"kind":"terminal.attachment.clipboard_cleanup_failed", "request_id":request_id, "error":error.to_string()})
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    #[test]
    fn paths_are_quoted_in_file_order_without_submission() {
        let paths = vec![
            "/tmp/한글's.png".to_owned(),
            "/tmp/a $HOME `echo` ".to_owned(),
        ];
        assert_eq!(
            String::from_utf8(paste_bytes(&paths, true).unwrap()).unwrap(),
            "\u{1b}[200~\"/tmp/한글's.png\"\u{1b}[201~ \u{1b}[200~\"/tmp/a \\$HOME \\`echo\\` \"\u{1b}[201~"
        );
        assert_eq!(
            paste_bytes(&["/tmp/image.png".to_owned()], false).unwrap(),
            b"\"/tmp/image.png\" "
        );
        assert!(paste_bytes(&["/tmp/a\ncommand".to_owned()], true).is_err());
    }
    #[test]
    fn selected_files_are_read_and_special_files_refused() {
        let root = std::env::temp_dir().join(format!(
            "hide-attachment-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let file = root.join("한글.png");
        fs::write(&file, b"explicit bytes").unwrap();
        let link = root.join("link.png");
        symlink(&file, &link).unwrap();
        let cancelled = AtomicBool::new(false);
        let result = read_sources(&[file.to_string_lossy().into_owned()], &cancelled).unwrap();
        assert_eq!(result[0].bytes, b"explicit bytes");
        assert!(read_sources(&[link.to_string_lossy().into_owned()], &cancelled).is_err());
        assert!(read_sources(&[root.to_string_lossy().into_owned()], &cancelled).is_err());
        let oversized = root.join("large");
        fs::File::create(&oversized)
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        assert!(
            read_sources(&[oversized.to_string_lossy().into_owned()], &cancelled)
                .unwrap_err()
                .contains("20 MiB")
        );
        cancelled.store(true, Ordering::Release);
        assert!(read_sources(&[file.to_string_lossy().into_owned()], &cancelled).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
