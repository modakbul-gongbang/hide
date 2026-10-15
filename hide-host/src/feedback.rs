//! A node's View feedback files (PRD view-opener-feedback D-16, D-23): the
//! screenshots a screen stages when the operator leaves a comment, and the
//! attachments a Send places for the recipient agent.
//!
//! Both live in the account's own state folder, in owner-only folders whose
//! names `hide_kit::layout` keeps, and every name a call carries is one Hide
//! generated (`hide_node_link::feedback`), so a call can never name its way
//! out of them. Each folder has a cap; a write past it is refused whole.

use std::io::Read;
use std::path::{Path, PathBuf};

use hide_node_link::feedback::{
    FEEDBACK_FILE, FeedbackCall, File, MAX_CALL_BYTES, MAX_FILE_BYTES, MAX_KEPT_LETTERS,
    MAX_LETTER_FILES, MAX_PLACED_BYTES, MAX_STAGED_SHOT_BYTES, MAX_STAGED_SHOTS, PLACE_FULL,
    Placed, SHOT_GONE, valid_letter_file_name, valid_letter_folder, valid_shot_name,
};
use hide_platform::fs::private;
use serde_json::Value;

/// Answers one feedback call for the account whose state folder is `state`.
pub fn handle(call: FeedbackCall, state: Option<&Path>) -> Result<Value, String> {
    let state = state.ok_or("This node has no state folder for View feedback.")?;
    let shots = hide_kit::layout::view_feedback_shots(state);
    let letters = hide_kit::layout::letter_attachments(state);
    let value = match call {
        FeedbackCall::ReadShots { names } => serde_json::to_value(read_shots(&shots, &names)?),
        FeedbackCall::RemoveShots { names } => {
            remove_named(&shots, &names, valid_shot_name)?;
            Ok(Value::Null)
        }
        FeedbackCall::SweepShots { keep } => {
            sweep(&shots, &keep, valid_shot_name, false)?;
            Ok(Value::Null)
        }
        FeedbackCall::Place { letter, files } => {
            serde_json::to_value(place(&letters, &letter, &files)?)
        }
        FeedbackCall::Remove { letter } => {
            remove_letter(&letters, &letter)?;
            Ok(Value::Null)
        }
        FeedbackCall::Sweep { keep } => {
            if keep.len() > MAX_KEPT_LETTERS {
                return Err("Too many letters to keep in one sweep.".to_owned());
            }
            sweep(&letters, &keep, valid_letter_folder, true)?;
            Ok(Value::Null)
        }
    };
    value.map_err(|error| error.to_string())
}

/// Stages one screenshot named `name` (`valid_shot_name`) in `state`'s
/// staging folder. The screen's own process calls this when a comment is
/// left; the core learns only the name. A name already staged is refused,
/// so a screenshot never replaces another.
pub fn stage_shot(state: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    if !valid_shot_name(name) {
        return Err("The screenshot name is not one Hide makes.".to_owned());
    }
    if bytes.is_empty() || bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("The screenshot is empty or larger than 16 MiB.".to_owned());
    }
    let folder = hide_kit::layout::view_feedback_shots(state);
    private::create_dir_all(&folder)
        .map_err(|error| format!("The screenshot folder could not be made: {error}"))?;
    let (count, total) = usage(&folder, valid_shot_name)?;
    if count >= MAX_STAGED_SHOTS || total + bytes.len() as u64 > MAX_STAGED_SHOT_BYTES {
        return Err(
            "Screenshots of unsent comments fill their 256 MiB. Send or delete some comments."
                .to_owned(),
        );
    }
    let mut file = private::create_new_file(&folder.join(name))
        .map_err(|error| format!("The screenshot could not be saved: {error}"))?;
    std::io::Write::write_all(&mut file, bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("The screenshot could not be saved: {error}"))?;
    Ok(())
}

fn read_shots(folder: &Path, names: &[String]) -> Result<Vec<File>, String> {
    let mut total = 0u64;
    let mut files = Vec::with_capacity(names.len());
    for name in names {
        if !valid_shot_name(name) {
            return Err("A screenshot name is not one Hide staged.".to_owned());
        }
        let bytes = read_capped(&folder.join(name))?;
        total += bytes.len() as u64;
        if total > MAX_CALL_BYTES {
            return Err("The screenshots asked for at once are too large.".to_owned());
        }
        files.push(File::new(name.clone(), &bytes));
    }
    Ok(files)
}

fn read_capped(path: &Path) -> Result<Vec<u8>, String> {
    let file = hide_platform::fs::open_regular(path).map_err(|_| SHOT_GONE.to_owned())?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("A screenshot could not be read: {error}"))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("A screenshot is larger than 16 MiB.".to_owned());
    }
    Ok(bytes)
}

fn place(root: &Path, letter: &str, files: &[File]) -> Result<Placed, String> {
    if !valid_letter_folder(letter) {
        return Err("The letter's folder name is not one Hide made.".to_owned());
    }
    if files.len() > MAX_LETTER_FILES {
        return Err("A letter carries at most 64 files.".to_owned());
    }
    let mut incoming = 0u64;
    for file in files {
        if !valid_letter_file_name(&file.name) {
            return Err("A letter file's name is not one Hide made.".to_owned());
        }
        incoming += file.len();
    }
    if incoming > MAX_CALL_BYTES {
        return Err("The letter's files are too large for one call.".to_owned());
    }
    private::create_dir_all(root)
        .map_err(|error| format!("The letter attachment folder could not be made: {error}"))?;
    let folder = root.join(letter);
    // What a retry already placed counts once: it is kept, not written again.
    let (_, total) = folder_usage(root)?;
    let already = folder_usage(&folder).map(|(_, bytes)| bytes).unwrap_or(0);
    if total.saturating_sub(already) + incoming > MAX_PLACED_BYTES {
        return Err(PLACE_FULL.to_owned());
    }
    match private::create_dir(&folder) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !private::is_private(&folder).unwrap_or(false)
                || !private::owned_by_current_user(&folder).unwrap_or(false)
            {
                return Err("The letter's folder is not this account's own.".to_owned());
            }
        }
        Err(error) => return Err(format!("The letter's folder could not be made: {error}")),
    }
    let mut paths = Vec::with_capacity(files.len());
    for file in files {
        let bytes = file.bytes()?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err("A letter file is larger than 16 MiB.".to_owned());
        }
        let path = folder.join(&file.name);
        let same = read_capped(&path).is_ok_and(|held| held == bytes);
        if !same {
            hide_platform::fs::atomic::write_file(
                &path,
                &bytes,
                hide_platform::fs::Access::Private,
            )
            .map_err(|error| format!("A letter file could not be written: {error}"))?;
        }
        paths.push(wire(&path)?);
    }
    Ok(Placed { paths })
}

fn wire(path: &Path) -> Result<String, String> {
    hide_platform::path::to_wire(path).map_err(|error| error.to_string())
}

fn remove_letter(root: &Path, letter: &str) -> Result<(), String> {
    if !valid_letter_folder(letter) {
        return Err("The letter's folder name is not one Hide made.".to_owned());
    }
    remove_folder(&root.join(letter))
}

/// Removes a letter folder: only the files Hide could have written there,
/// then the folder; anything else in it keeps it.
fn remove_folder(folder: &Path) -> Result<(), String> {
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("The letter's folder could not be read: {error}")),
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_str().is_some_and(valid_letter_file_name)
            && entry.file_type().is_ok_and(|kind| kind.is_file())
        {
            remove_file(&entry.path())?;
        }
    }
    match std::fs::remove_dir(folder) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("The letter's folder could not be removed: {error}")),
    }
}

fn remove_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("A feedback file could not be removed: {error}")),
    }
}

fn remove_named(folder: &Path, names: &[String], valid: fn(&str) -> bool) -> Result<(), String> {
    for name in names {
        if !valid(name) {
            return Err("A name is not one Hide made.".to_owned());
        }
        remove_file(&folder.join(name))?;
    }
    Ok(())
}

/// Removes every entry of `folder` that Hide made and `keep` does not name:
/// files, or letter folders when `folders`.
fn sweep(
    folder: &Path,
    keep: &[String],
    valid: fn(&str) -> bool,
    folders: bool,
) -> Result<(), String> {
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("A feedback folder could not be read: {error}")),
    };
    let keep: std::collections::HashSet<&str> = keep.iter().map(String::as_str).collect();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !valid(&name) || keep.contains(name.as_str()) {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if folders && kind.is_dir() {
            remove_folder(&entry.path())?;
        } else if !folders && kind.is_file() {
            remove_file(&entry.path())?;
        }
    }
    Ok(())
}

fn usage(folder: &Path, valid: fn(&str) -> bool) -> Result<(usize, u64), String> {
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(error) => return Err(format!("A feedback folder could not be read: {error}")),
    };
    let mut count = 0;
    let mut bytes = 0;
    for entry in entries.flatten() {
        if entry.file_name().to_str().is_some_and(valid)
            && let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
            count += 1;
            bytes += meta.len();
        }
    }
    Ok((count, bytes))
}

/// Files and bytes under a letter root or one letter's folder, one level of
/// folders deep.
fn folder_usage(folder: &Path) -> Result<(usize, u64), String> {
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(error) => return Err(format!("A feedback folder could not be read: {error}")),
    };
    let mut count = 0;
    let mut bytes = 0;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_file() {
            count += 1;
            bytes += meta.len();
        } else if meta.is_dir() {
            let (inner_count, inner_bytes) = usage(&entry.path(), valid_letter_file_name)?;
            count += inner_count;
            bytes += inner_bytes;
        }
    }
    Ok((count, bytes))
}

/// Where the feedback file of a letter sits beside its screenshots.
pub fn feedback_file(state: &Path, letter: &str) -> PathBuf {
    hide_kit::layout::letter_attachments(state)
        .join(letter)
        .join(FEEDBACK_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: &str = "view-feedback-0f8fad5b-d9cb-469f-a165-70867728950e";

    #[test]
    fn a_letters_files_are_placed_owner_only_and_a_retry_places_nothing_twice() {
        let state = tempfile::tempdir().unwrap();
        let shot = "shot-6f9619ff-8b86-4011-b42d-00c04fc964ff.png".to_owned();
        stage_shot(state.path(), &shot, b"png bytes").unwrap();
        let read = handle(
            FeedbackCall::ReadShots {
                names: vec![shot.clone()],
            },
            Some(state.path()),
        )
        .unwrap();
        let files: Vec<File> = serde_json::from_value(read).unwrap();
        let mut files = files;
        files.push(File::new(FEEDBACK_FILE, b"all of it"));
        let call = || FeedbackCall::Place {
            letter: LETTER.into(),
            files: files.clone(),
        };
        let placed: Placed =
            serde_json::from_value(handle(call(), Some(state.path())).unwrap()).unwrap();
        let again: Placed =
            serde_json::from_value(handle(call(), Some(state.path())).unwrap()).unwrap();
        assert_eq!(placed, again);
        assert_eq!(placed.paths.len(), 2);
        let folder = hide_kit::layout::letter_attachments(state.path()).join(LETTER);
        assert!(private::is_private(&folder).unwrap());
        assert!(private::is_private(&folder.join(&shot)).unwrap());
        assert_eq!(
            std::fs::read(feedback_file(state.path(), LETTER)).unwrap(),
            b"all of it"
        );
    }

    #[test]
    fn a_name_hide_did_not_make_reaches_no_file() {
        let state = tempfile::tempdir().unwrap();
        let refused = handle(
            FeedbackCall::Place {
                letter: "../escape".into(),
                files: vec![],
            },
            Some(state.path()),
        );
        assert!(refused.is_err());
        let refused = handle(
            FeedbackCall::ReadShots {
                names: vec!["../../etc/passwd".into()],
            },
            Some(state.path()),
        );
        assert!(refused.is_err());
    }

    #[test]
    fn removing_and_sweeping_leave_only_what_is_kept() {
        let state = tempfile::tempdir().unwrap();
        let kept = "shot-6f9619ff-8b86-4011-b42d-00c04fc964ff.png".to_owned();
        let gone = "shot-7f9619ff-8b86-4011-b42d-00c04fc964ff.png".to_owned();
        stage_shot(state.path(), &kept, b"a").unwrap();
        stage_shot(state.path(), &gone, b"b").unwrap();
        handle(
            FeedbackCall::SweepShots {
                keep: vec![kept.clone()],
            },
            Some(state.path()),
        )
        .unwrap();
        let shots = hide_kit::layout::view_feedback_shots(state.path());
        assert!(shots.join(&kept).exists());
        assert!(!shots.join(&gone).exists());
        handle(
            FeedbackCall::Place {
                letter: LETTER.into(),
                files: vec![File::new(FEEDBACK_FILE, b"x")],
            },
            Some(state.path()),
        )
        .unwrap();
        handle(FeedbackCall::Sweep { keep: vec![] }, Some(state.path())).unwrap();
        assert!(
            !hide_kit::layout::letter_attachments(state.path())
                .join(LETTER)
                .exists()
        );
    }

    #[test]
    fn placing_past_the_cap_writes_nothing() {
        let state = tempfile::tempdir().unwrap();
        let root = hide_kit::layout::letter_attachments(state.path());
        std::fs::create_dir_all(root.join("other")).unwrap();
        // A sparse file stands for attachments already placed.
        let held = std::fs::File::create(root.join("other").join(FEEDBACK_FILE)).unwrap();
        held.set_len(MAX_PLACED_BYTES).unwrap();
        let refused = handle(
            FeedbackCall::Place {
                letter: LETTER.into(),
                files: vec![File::new(FEEDBACK_FILE, b"more")],
            },
            Some(state.path()),
        );
        assert_eq!(refused, Err(PLACE_FULL.to_owned()));
        assert!(!root.join(LETTER).exists());
    }
}
