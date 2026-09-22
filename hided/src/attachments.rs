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
use std::time::{Duration, Instant};

/// Bytes one attachment may hold, the same cap the core enforces.
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// Bytes one batch may hold, the same cap the core enforces.
pub const MAX_BATCH_BYTES: u64 = 40 * 1024 * 1024;
/// Files one batch may carry, the same cap the core enforces.
pub const MAX_FILES: usize = 8;
/// Staged files kept on disk. A pasted token is the staged path, so the bound
/// is generous; older files are removed only as newer ones arrive.
const STAGED_KEEP: usize = 512;

/// Bytes the staged files may hold together. Past it the oldest evictable
/// file goes, and nothing committed within the grace window is evicted: the
/// core may still be reading the file a just-pasted token names.
const MAX_STAGED_BYTES: u64 = 256 * 1024 * 1024;

/// How long a committed file is safe from eviction.
const COMMIT_GRACE: Duration = Duration::from_secs(60);

/// Stages one connection may have open at once; past it the oldest open stage
/// is dropped, so an abandoned upload cannot hold file descriptors forever.
const MAX_OPEN_UPLOADS: usize = 64;

/// One client-supplied name, flattened to a single relative component: a
/// separator, a control character, or a parent segment would let the client
/// name a path hided did not choose. Returns `None` for a shape no stage may
/// carry.
fn safe_component(value: &str) -> Option<String> {
    if value.is_empty() || value.len() > 128 {
        return None;
    }
    if value == "."
        || value == ".."
        || value
            .chars()
            .any(|c| c == '/' || c == '\\' || c.is_control())
    {
        return None;
    }
    Some(value.to_owned())
}

struct Upload {
    path: PathBuf,
    file: Option<File>,
    written: u64,
}

/// One staged file on disk, with what eviction needs to judge it.
struct Staged {
    request_id: String,
    path: PathBuf,
    bytes: u64,
    /// When the core was told about it; until the grace window passes, the
    /// core may still be reading it, so eviction leaves it alone.
    committed_at: Option<Instant>,
}

#[derive(Default)]
struct State {
    uploads: HashMap<String, Upload>,
    /// The open stages in arrival order, oldest first, so the cap can drop one.
    open: VecDeque<String>,
    staged: VecDeque<Staged>,
    staged_bytes: u64,
}

/// Removes one stage everywhere the state names it: the file, the staged
/// entry (with its byte count), the open upload and its queue slot.
fn drop_stage(state: &mut State, request_id: &str) {
    let index = state
        .staged
        .iter()
        .position(|entry| entry.request_id == request_id);
    if let Some(entry) = index.and_then(|index| state.staged.remove(index)) {
        state.staged_bytes = state.staged_bytes.saturating_sub(entry.bytes);
        let _ = std::fs::remove_file(&entry.path);
    }
    state.uploads.remove(request_id);
    state.open.retain(|id| id != request_id);
}

/// Makes room for one more staged file of `size` bytes: the oldest entry that
/// is not a freshly committed file goes first, and a fully fresh set is
/// refused rather than unlinked (the core may still be reading it).
fn make_room(state: &mut State, size: u64) -> Result<(), &'static str> {
    while state.staged.len() >= STAGED_KEEP
        || state.staged_bytes.saturating_add(size) > MAX_STAGED_BYTES
    {
        let Some(evictable) = state.staged.iter().find_map(|entry| {
            entry
                .committed_at
                .is_none_or(|at| at.elapsed() >= COMMIT_GRACE)
                .then(|| entry.request_id.clone())
        }) else {
            return Err("staging_full");
        };
        drop_stage(state, &evictable);
    }
    Ok(())
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
        // Both components are client input; the id is flattened the same way
        // the name is, so the joined path is always one file directly in the
        // staging directory and can never name another path.
        let Some(request_id) = safe_component(request_id) else {
            return Err("invalid_request_id");
        };
        let Some(name) = safe_component(name) else {
            return Err("invalid_request_id");
        };
        let root = if clipboard {
            &self.clipboard_root
        } else {
            &self.dir
        };
        let file_name = if clipboard {
            format!("hide-{request_id}.png")
        } else {
            format!("{request_id}-{name}")
        };
        let path = root.join(&file_name);
        if path.parent() != Some(root.as_path()) {
            return Err("invalid_request_id");
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        make_room(&mut state, size)?;
        let file = File::create(&path).map_err(|_| "stage_failed")?;
        // An abandoned stage would otherwise hold its file descriptor for the
        // daemon's life; the oldest open one goes instead.
        while state.open.len() >= MAX_OPEN_UPLOADS {
            let Some(oldest) = state.open.front().cloned() else {
                break;
            };
            drop_stage(&mut state, &oldest);
        }
        state.open.push_back(request_id.clone());
        state.staged.push_back(Staged {
            request_id: request_id.clone(),
            path: path.clone(),
            bytes: size,
            committed_at: None,
        });
        state.staged_bytes = state.staged_bytes.saturating_add(size);
        state.uploads.insert(
            request_id,
            Upload {
                path,
                file: Some(file),
                written: 0,
            },
        );
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
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match state.uploads.get(stage) {
                    Some(upload) if upload.file.is_none() => {}
                    _ => return Err("unknown_stage"),
                }
                // Committed: the file stays for the core to read, the open
                // stage entry does not.
                if let Some(entry) = state
                    .staged
                    .iter_mut()
                    .find(|entry| entry.request_id == *stage)
                {
                    entry.committed_at = Some(Instant::now());
                }
                state.uploads.remove(stage);
                state.open.retain(|id| id != stage);
            }
            return Ok(Vec::new());
        }
        if stages.len() > MAX_FILES || stages.is_empty() {
            return Err("too_many_files");
        }
        let mut state = self
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
        // Committed: the files stay for the core to read, the open stage
        // entries do not, and the files are safe from eviction for a window.
        let committed_at = Instant::now();
        for stage in stages {
            if let Some(entry) = state
                .staged
                .iter_mut()
                .find(|entry| entry.request_id == *stage)
            {
                entry.committed_at = Some(committed_at);
            }
            state.uploads.remove(stage);
        }
        state.open.retain(|id| !stages.contains(id));
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
        drop_stage(&mut state, request_id);
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
    fn a_stage_id_that_names_a_path_is_refused() {
        let (_dir, service) = attachments();
        for id in ["../escape", "/tmp/absolute", "a/b", "", ".", ".."] {
            assert_eq!(
                service.begin(id, "shot.png", 1, false),
                Err("invalid_request_id"),
                "{id:?}"
            );
        }
        // The accepted id lands directly in the staging directory.
        service.begin("ok-id", "shot.png", 1, false).unwrap();
        assert_eq!(service.write("ok-id", &[1], true), Ok(true));
        let path = service.commit(&["ok-id".to_owned()], false).unwrap();
        assert!(path[0].contains("/attachments/ok-id-shot.png"), "{path:?}");
    }

    #[test]
    fn abandoned_stages_are_bounded_and_their_files_go_with_them() {
        let (_dir, service) = attachments();
        for index in 0..(MAX_OPEN_UPLOADS + 2) {
            service.begin(&format!("s{index}"), "x", 1, false).unwrap();
        }
        assert_eq!(service.write("s0", &[1], true), Err("unknown_stage"));
        assert_eq!(service.write("s1", &[1], true), Err("unknown_stage"));
        assert_eq!(
            service.write(&format!("s{}", MAX_OPEN_UPLOADS + 1), &[1], true),
            Ok(true)
        );
    }

    #[test]
    fn a_committed_stage_is_released_but_its_file_stays() {
        let (_dir, service) = attachments();
        service.begin("r1", "shot.png", 1, false).unwrap();
        service.write("r1", &[1], true).unwrap();
        let paths = service.commit(&["r1".to_owned()], false).unwrap();
        assert!(std::path::Path::new(&paths[0]).is_file());
        assert_eq!(service.write("r1", &[1], true), Err("unknown_stage"));
    }

    #[test]
    fn staged_bytes_are_bounded_and_a_fresh_commit_is_spared() {
        let (_dir, service) = attachments();
        // A committed file inside the grace window survives eviction pressure.
        service.begin("keep", "keep.png", 1, false).unwrap();
        service.write("keep", &[1], true).unwrap();
        let kept = service.commit(&["keep".to_owned()], false).unwrap();
        let big = 20 * 1024 * 1024;
        for index in 0..14 {
            service
                .begin(&format!("big{index}"), "x", big, false)
                .unwrap();
        }
        assert!(
            std::path::Path::new(&kept[0]).is_file(),
            "a fresh commit stays"
        );
        // Everything else past the byte budget was evicted.
        assert_eq!(service.write("big0", &[1], true), Err("unknown_stage"));
    }

    #[test]
    fn a_directory_full_of_fresh_commits_refuses_rather_than_unlinks() {
        let (_dir, service) = attachments();
        let big = 20 * 1024 * 1024;
        let mut committed = Vec::new();
        // Twelve files stay under the 256 MiB budget even though each commit
        // is fresh, so the budget is what the next stage trips.
        for index in 0..12 {
            let id = format!("c{index}");
            service.begin(&id, "x", big, false).unwrap();
            service.write(&id, &[1], true).unwrap();
            // One batch each: the batch cap is 40 MiB, the staged budget 256.
            committed.extend(service.commit(&[id], false).unwrap());
        }
        assert_eq!(committed.len(), 12);
        // Every staged file is a fresh commit, so the budget is over but
        // nothing may be evicted; the next stage is refused instead.
        assert_eq!(service.begin("late", "x", big, false), Err("staging_full"));
        assert!(std::path::Path::new(&committed[0]).is_file());
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
