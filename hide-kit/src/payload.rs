//! A kit-owned copy of a folder the build ships.
//!
//! Herdr stores a linked plugin by its resolved path, and Node resolves a
//! script's own path through links, so neither can be pointed at a build
//! folder that the next update deletes. The kit copies such a folder to a
//! fixed place under `~/.hide/kit/` and keeps that copy equal to the build's
//! (D-11). The copy carries the digest of what it was copied from, so
//! judging it reads the build's folder and one small file.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const DIGEST_FILE: &str = ".hide-kit-digest";

/// Folders deeper than this are refused rather than walked; nothing the kit
/// ships is nested this far (engineering rule 15).
const MAX_DEPTH: usize = 8;

/// A digest of every file's relative path, executable bit and bytes.
pub(crate) fn digest(source: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    walk(source, source, 0, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for relative in files {
        let path = source.join(&relative);
        let metadata = std::fs::metadata(&path)
            .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
        hasher.update(relative.to_string_lossy().as_bytes());
        let executable = hide_platform::fs::permissions::is_executable(&path)
            .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
        hasher.update([0, u8::from(executable)]);
        hasher.update(metadata.len().to_le_bytes());
        let mut file = std::fs::File::open(&path)
            .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn walk(root: &Path, dir: &Path, depth: usize, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("{} is nested too deeply to copy", dir.display()));
    }
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("{} could not be read: {error}", dir.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("{} could not be read: {error}", dir.display()))?;
        let path = entry.path();
        if entry.file_name() == DIGEST_FILE {
            continue;
        }
        // Followed, not copied as links: the build's folder may itself be
        // reached through `current`.
        let metadata = std::fs::metadata(&path)
            .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
        if metadata.is_dir() {
            walk(root, &path, depth + 1, files)?;
        } else if metadata.is_file() {
            files.push(
                path.strip_prefix(root)
                    .expect("walked paths stay under the root")
                    .to_path_buf(),
            );
        }
    }
    Ok(())
}

/// Whether `copy` holds what `source` holds now.
pub(crate) fn is_current(source: &Path, copy: &Path) -> Result<bool, String> {
    let wanted = digest(source)?;
    Ok(std::fs::read_to_string(copy.join(DIGEST_FILE)).is_ok_and(|found| found.trim() == wanted))
}

/// Makes `copy` hold what `source` holds, replacing any older copy whole: a
/// new folder is filled beside it and swapped in, so a reader never sees a
/// half-written plugin.
pub(crate) fn sync(source: &Path, copy: &Path) -> Result<(), String> {
    if is_current(source, copy)? {
        return Ok(());
    }
    let wanted = digest(source)?;
    let parent = copy
        .parent()
        .ok_or_else(|| format!("{} has no parent folder", copy.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("{} could not be created: {error}", parent.display()))?;
    let name = copy
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("copy");
    let fresh = parent.join(format!(".{name}.new-{}", std::process::id()));
    let old = parent.join(format!(".{name}.old-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&fresh);
    let _ = std::fs::remove_dir_all(&old);
    let filled = (|| {
        let mut files = Vec::new();
        walk(source, source, 0, &mut files)?;
        for relative in files {
            let from = source.join(&relative);
            let to = fresh.join(&relative);
            if let Some(folder) = to.parent() {
                std::fs::create_dir_all(folder).map_err(|error| {
                    format!("{} could not be created: {error}", folder.display())
                })?;
            }
            std::fs::copy(&from, &to)
                .map_err(|error| format!("{} could not be copied: {error}", from.display()))?;
        }
        std::fs::write(fresh.join(DIGEST_FILE), format!("{wanted}\n"))
            .map_err(|error| format!("{} could not be written: {error}", fresh.display()))
    })();
    if let Err(reason) = filled {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(reason);
    }
    if copy.exists() {
        std::fs::rename(copy, &old).map_err(|error| {
            let _ = std::fs::remove_dir_all(&fresh);
            format!("{} could not be replaced: {error}", copy.display())
        })?;
    }
    let swapped = std::fs::rename(&fresh, copy);
    if let Err(error) = swapped {
        // Put the old copy back rather than leave nothing in its place.
        let _ = std::fs::rename(&old, copy);
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(format!("{} could not be replaced: {error}", copy.display()));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}
