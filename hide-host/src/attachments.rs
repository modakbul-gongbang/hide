//! The node's read of files the operator picked to attach to a terminal:
//! bounded, regular files only, each read whole and checked unchanged.

use std::io::Read;
use std::path::Path;

use hide_node_link::attachments::{
    MAX_FILE_BYTES, MAX_FILES, MAX_PATH_BYTES, MAX_REQUEST_BYTES, ReadFile,
};
use hide_platform::fs::identity::stamp_of;

/// Reads each of `paths` in order; `go_on` is asked before each file, and a
/// false answer ends the read as cancelled.
pub fn read_sources(
    paths: &[String],
    go_on: &mut dyn FnMut(usize) -> bool,
) -> Result<Vec<ReadFile>, String> {
    if paths.is_empty() || paths.len() > MAX_FILES {
        return Err("Choose between 1 and 8 regular files.".to_owned());
    }
    let mut files = Vec::with_capacity(paths.len());
    let mut total = 0u64;
    for (index, path) in paths.iter().enumerate() {
        if !go_on(index) {
            return Err("File transfer was cancelled.".to_owned());
        }
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
        let mut file = hide_platform::fs::open_regular(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::InvalidInput {
                "Folders, symbolic links and special files cannot be attached. Choose regular files."
                    .to_owned()
            } else {
                "A selected file is unavailable or is a symbolic link. Choose a regular file."
                    .to_owned()
            }
        })?;
        let before = file
            .metadata()
            .map_err(|_| "Could not inspect a selected file.".to_owned())?;
        let stamp = stamp_of(&file).map_err(|_| "Could not inspect a selected file.".to_owned())?;
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
        let after = stamp_of(&file).map_err(|_| "Could not recheck a selected file.".to_owned())?;
        if bytes.len() as u64 != before.len() || stamp != after {
            return Err(
                "A selected file changed while it was being read. Choose it again.".to_owned(),
            );
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("A file name is not valid UTF-8.")?;
        files.push(ReadFile::new(
            path.to_string_lossy().into_owned(),
            name.to_owned(),
            &bytes,
        ));
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn selected_files_are_read_and_special_files_refused() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().to_path_buf();
        let file = root.join("한글.png");
        fs::write(&file, b"explicit bytes").unwrap();
        let link = root.join("link.png");
        // A Windows account without the privilege cannot make the link.
        let linked = match hide_platform::fs::link::create_link(&file, &link) {
            Err(error) if hide_platform::fs::link::needs_privilege(&error) => false,
            made => made.map(|()| true).unwrap(),
        };
        let result = read_sources(&[file.to_string_lossy().into_owned()], &mut |_| true).unwrap();
        assert_eq!(result[0].bytes().unwrap(), b"explicit bytes");
        assert!(
            !linked || read_sources(&[link.to_string_lossy().into_owned()], &mut |_| true).is_err()
        );
        assert!(read_sources(&[root.to_string_lossy().into_owned()], &mut |_| true).is_err());
        let oversized = root.join("large");
        fs::File::create(&oversized)
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        assert!(
            read_sources(&[oversized.to_string_lossy().into_owned()], &mut |_| true)
                .unwrap_err()
                .contains("20 MiB")
        );
        assert!(read_sources(&[file.to_string_lossy().into_owned()], &mut |_| false).is_err());
    }
}
