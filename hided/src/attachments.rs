//! Staging for terminal attachments (PRD B14, B15, D-02, D-08).
//!
//! The browser can read a dropped file's bytes but cannot name its path, so
//! hided is the one that stages them: the client uploads one file per stage in
//! binary frames, and a commit turns the staged files into the one
//! `terminal_attachment` event the Swift shell sends for a batch of paths. The
//! staged file's path is what the core pastes, so it lands in the app state
//! dir, where it outlives the paste; only the newest `STAGED_KEEP` files are
//! kept, so the directory cannot grow with use (engineering 15). The caps are
//! the core's own numbers (20 MiB per file, 40 MiB and 8 files per batch);
//! hided refuses past them here rather than making the core read a file it will
//! reject. A clipboard image stages at the exact path the core reads it from.

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Bytes one attachment may hold, the same cap the core enforces.
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// Bytes one batch may hold, the same cap the core enforces.
pub const MAX_BATCH_BYTES: u64 = 40 * 1024 * 1024;
/// Files one batch may carry, the same cap the core enforces.
pub const MAX_FILES: usize = 8;
/// Staged files kept on disk. A pasted token is the staged path, so the bound
/// is generous; older files are removed only as newer ones arrive.
const STAGED_KEEP: usize = 512;

struct Upload {
    path: PathBuf,
    file: Option<File>,
    written: u64,
}

#[derive(Default)]
struct State {
    uploads: HashMap<String, Upload>,
    staged: VecDeque<PathBuf>,
}

pub struct Attachments {
    dir: PathBuf,
    clipboard_root: PathBuf,
    state: Mutex<State>,
}

impl Attachments {
    pub fn new(state_dir: &Path) -> Self {
        let dir = state_dir.join("attachments");
        // The core reads a clipboard image from its own fixed path, so hided
        // stages it exactly there (`herdr-core/src/terminal_attachments.rs`).
        let clipboard_root = state_dir.join("TerminalClipboard");
        for folder in [&dir, &clipboard_root] {
            if let Err(error) = std::fs::create_dir_all(folder) {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "hided",
                        "kind": "attachment.stage_dir_failed",
                        "message": error.to_string(),
                    })
                );
            }
        }
        Self {
            dir,
            clipboard_root,
            state: Mutex::new(State::default()),
        }
    }

    /// Opens one stage: a file of `size` bytes the client will upload. The
    /// name is flattened to one component so it can never name another path.
    /// A clipboard image takes the path the core reads it from.
    pub fn begin(
        &self,
        request_id: &str,
        name: &str,
        size: u64,
        clipboard: bool,
    ) -> Result<(), &'static str> {
        if size > MAX_FILE_BYTES {
            return Err("too_large");
        }
        let safe: String = name
            .chars()
            .map(|c| {
                if c == '/' || c == '\\' || c == '\0' {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let path = if clipboard {
            self.clipboard_root.join(format!("hide-{request_id}.png"))
        } else {
            self.dir.join(format!("{request_id}-{safe}"))
        };
        let file = File::create(&path).map_err(|_| "stage_failed")?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Keep the newest few staged files; the core has already copied the
        // older ones by the time a new batch arrives.
        while state.staged.len() >= STAGED_KEEP {
            if let Some(old) = state.staged.pop_front() {
                let _ = std::fs::remove_file(old);
            }
        }
        state.uploads.insert(
            request_id.to_owned(),
            Upload {
                path: path.clone(),
                file: Some(file),
                written: 0,
            },
        );
        state.staged.push_back(path);
        Ok(())
    }

    /// Appends one chunk; `eof` closes the stage. Returns whether it closed.
    pub fn write(&self, request_id: &str, bytes: &[u8], eof: bool) -> Result<bool, &'static str> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(upload) = state.uploads.get_mut(request_id) else {
            return Err("unknown_stage");
        };
        if upload.written + bytes.len() as u64 > MAX_FILE_BYTES {
            return Err("too_large");
        }
        if let Some(file) = upload.file.as_mut() {
            file.write_all(bytes).map_err(|_| "stage_failed")?;
        }
        upload.written += bytes.len() as u64;
        if eof {
            if let Some(mut file) = upload.file.take() {
                let _ = file.flush();
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// The staged paths for a batch, in the order the client names them, with
    /// the batch caps applied. A clipboard batch names no paths: the core reads
    /// the image it staged at its own path. A stage that was never opened is an
    /// error, so a commit cannot name its way to a path hided did not write.
    pub fn commit(&self, stages: &[String], clipboard: bool) -> Result<Vec<String>, &'static str> {
        if clipboard {
            if stages.len() > 1 {
                return Err("too_many_files");
            }
            if let Some(stage) = stages.first() {
                let state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match state.uploads.get(stage) {
                    Some(upload) if upload.file.is_none() => return Ok(Vec::new()),
                    _ => return Err("unknown_stage"),
                }
            }
            return Ok(Vec::new());
        }
        if stages.len() > MAX_FILES || stages.is_empty() {
            return Err("too_many_files");
        }
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut total = 0u64;
        let mut paths = Vec::new();
        for stage in stages {
            let Some(upload) = state.uploads.get(stage) else {
                return Err("unknown_stage");
            };
            if upload.file.is_some() {
                return Err("stage_incomplete");
            }
            total += upload.written;
            if total > MAX_BATCH_BYTES {
                return Err("batch_too_large");
            }
            paths.push(upload.path.display().to_string());
        }
        Ok(paths)
    }

    /// Parses one binary upload frame (a 4-byte big-endian header length, the
    /// header JSON, then bytes) and writes it. A refusal names the request and
    /// the reason so the caller can answer it.
    pub fn receive(&self, data: &[u8]) -> Option<(String, &'static str)> {
        if data.len() < 4 {
            return None;
        }
        let header_len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if 4 + header_len > data.len() {
            return None;
        }
        let header: serde_json::Value = serde_json::from_slice(&data[4..4 + header_len]).ok()?;
        let request_id = header.get("request_id")?.as_str()?.to_owned();
        let eof = header
            .get("eof")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        match self.write(&request_id, &data[4 + header_len..], eof) {
            Ok(_) => None,
            Err(reason) => Some((request_id, reason)),
        }
    }

    /// Drops a stage that was refused or abandoned, so the file goes with it.
    pub fn discard(&self, request_id: &str) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(upload) = state.uploads.remove(request_id) {
            let _ = std::fs::remove_file(&upload.path);
            state.staged.retain(|path| path != &upload.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachments() -> (tempfile::TempDir, Attachments) {
        let dir = tempfile::tempdir().unwrap();
        let service = Attachments::new(dir.path());
        (dir, service)
    }

    #[test]
    fn a_staged_file_is_written_and_committed_as_a_path() {
        let (_dir, service) = attachments();
        service.begin("r1", "shot.png", 5, false).unwrap();
        assert_eq!(service.write("r1", &[1, 2, 3], false), Ok(false));
        assert_eq!(service.write("r1", &[4, 5], true), Ok(true));
        let paths = service.commit(&["r1".to_owned()], false).unwrap();
        assert_eq!(paths.len(), 1);
        assert!(std::fs::read(&paths[0]).unwrap() == vec![1, 2, 3, 4, 5]);
        assert!(paths[0].contains("r1-shot.png"));
    }

    #[test]
    fn the_caps_and_shape_are_enforced() {
        let (_dir, service) = attachments();
        assert_eq!(
            service.begin("r1", "big.bin", MAX_FILE_BYTES + 1, false),
            Err("too_large")
        );
        assert_eq!(service.write("missing", &[1], true), Err("unknown_stage"));
        service.begin("r1", "a", 1, false).unwrap();
        assert_eq!(service.commit(&[], false), Err("too_many_files"));
        assert_eq!(
            service.commit(&["r1".to_owned()], false),
            Err("stage_incomplete")
        );
        service.write("r1", &[1], true).unwrap();
        assert_eq!(
            service.commit(&["nope".to_owned()], false),
            Err("unknown_stage")
        );
        let too_many: Vec<String> = (0..(MAX_FILES + 1)).map(|i| format!("r{i}")).collect();
        assert_eq!(service.commit(&too_many, false), Err("too_many_files"));
    }

    #[test]
    fn a_batch_past_the_total_cap_is_refused() {
        let (_dir, service) = attachments();
        for i in 0..3 {
            let id = format!("r{i}");
            service
                .begin(&id, "chunk.bin", MAX_FILE_BYTES, false)
                .unwrap();
            // Write the cap's worth without allocating it all at once.
            let chunk = vec![0u8; 1024 * 1024];
            let mut written = 0u64;
            while written < MAX_FILE_BYTES {
                let remaining = MAX_FILE_BYTES - written;
                let take = remaining.min(chunk.len() as u64) as usize;
                written += take as u64;
                service
                    .write(&id, &chunk[..take], written >= MAX_FILE_BYTES)
                    .unwrap();
            }
        }
        assert_eq!(
            service.commit(&["r0".to_owned(), "r1".to_owned(), "r2".to_owned()], false),
            Err("batch_too_large")
        );
    }
}
