//! Bounded, explicit terminal file ingress. No provider draft or submission state.
//! The picked files are read by the node that holds them
//! (`hide_host::attachments`); the clipboard image is the core's own file in
//! its state folder.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) use hide_node_link::attachments::{
    AttachmentFile, MAX_FILES, MAX_PATH_BYTES, check_cancelled, valid_request_id,
};
pub(crate) const MAX_QUEUED_INPUT: usize = 64 * 1024;

pub(crate) fn clipboard_root(state_path: &Path) -> PathBuf {
    state_path.with_file_name("TerminalClipboard")
}

pub(crate) fn clipboard_path(state_path: &Path, request_id: &str) -> PathBuf {
    clipboard_root(state_path).join(format!("hide-{request_id}.png"))
}

/// Reads the picked files on `node`, which asks after each file whether to
/// go on, so a cancel stops the read between two files.
pub(crate) fn read_sources(
    node: &dyn crate::node_access::NodeLink,
    paths: &[String],
    cancelled: &AtomicBool,
) -> Result<Vec<AttachmentFile>, String> {
    check_cancelled(cancelled)?;
    let call = hide_node_link::protocol::Call::ReadAttachments {
        paths: paths.to_vec(),
    };
    let read: Vec<hide_node_link::attachments::ReadFile> =
        crate::node_access::call_as_with_progress(
            node,
            call,
            READ_TIMEOUT,
            |_: serde_json::Value| !cancelled.load(Ordering::Acquire),
        )
        .map_err(|error| match error {
            crate::node_access::LinkError::Refused(refusal) => refusal.message,
            other => format!("The selected files could not be read: {other}"),
        })?;
    check_cancelled(cancelled)?;
    read.into_iter()
        .map(|file| {
            Ok(AttachmentFile {
                bytes: file.bytes()?,
                path: file.path,
                name: file.name,
            })
        })
        .collect()
}

/// Reading 40 MiB from a local disk takes well under a minute.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

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
    fn picked_files_are_read_on_the_node_with_its_refusal_in_the_operators_words() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("한글.png");
        fs::write(&file, b"explicit bytes").unwrap();
        let node = hide_node::Local::of_process();
        let paths = vec![file.to_string_lossy().into_owned()];
        let read = read_sources(&node, &paths, &AtomicBool::new(false)).unwrap();
        assert_eq!(read[0].bytes, b"explicit bytes");
        assert_eq!(read[0].name, "한글.png");
        let refused = read_sources(
            &node,
            &[folder.path().to_string_lossy().into_owned()],
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(refused.starts_with("Folders, symbolic links"), "{refused}");
        assert!(read_sources(&node, &paths, &AtomicBool::new(true)).is_err());
    }
}
