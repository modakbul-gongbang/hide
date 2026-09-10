use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::model::{EditorConflictSnapshot, EditorDocumentSnapshot};

const MAX_EDITABLE_BYTES: u64 = 2 * 1024 * 1024;

pub fn open(path: &Path) -> Result<EditorDocumentSnapshot, String> {
    let metadata =
        fs::metadata(path).map_err(|_| "The selected file could not be read".to_owned())?;
    if !metadata.is_file() {
        return Err("Only existing regular files can be opened".to_owned());
    }
    let modified = modified_milliseconds(&metadata)?;
    let language = language_for(path);
    let (contents_utf8, readonly_reason) = if metadata.len() > MAX_EDITABLE_BYTES {
        (
            None,
            Some("Files larger than 2 MB are preview-only".to_owned()),
        )
    } else {
        match fs::read(path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(contents) => {
                    let reason = metadata
                        .permissions()
                        .readonly()
                        .then(|| "The file is read-only on disk; editing is disabled".to_owned());
                    (Some(contents), reason)
                }
                Err(_) => (None, Some("Binary files are preview-only".to_owned())),
            },
            Err(_) => return Err("The selected file contents could not be read".to_owned()),
        }
    };

    Ok(EditorDocumentSnapshot {
        path: path.to_string_lossy().into_owned(),
        language,
        contents_utf8,
        opened_modified_at_unix_ms: Some(modified),
        dirty: false,
        readonly_reason,
        conflict: None,
    })
}

pub fn update_draft(editor: &mut EditorDocumentSnapshot, contents: String) -> Result<(), String> {
    if editor.readonly_reason.is_some() {
        return Err("The current file is read-only; the draft was not changed".to_owned());
    }
    editor.dirty = editor.contents_utf8.as_deref() != Some(contents.as_str());
    editor.contents_utf8 = Some(contents);
    Ok(())
}

pub fn save(
    editor: &mut EditorDocumentSnapshot,
    path: &Path,
    contents: String,
    expected_modified_at_unix_ms: Option<u64>,
) -> Result<(), String> {
    if editor.path != path.to_string_lossy() {
        return Err("The save target does not match the open document".to_owned());
    }
    if editor.readonly_reason.is_some() {
        return Err("The current file is read-only; the draft was preserved".to_owned());
    }
    let metadata = fs::metadata(path).map_err(|_| {
        "The existing file could not be inspected; the draft was preserved".to_owned()
    })?;
    if !metadata.is_file() {
        return Err(
            "The save target is no longer a regular file; the draft was preserved".to_owned(),
        );
    }
    let disk_modified = modified_milliseconds(&metadata)?;
    let disk_bytes = fs::read(path)
        .map_err(|_| "The existing file could not be read; the draft was preserved".to_owned())?;

    if disk_bytes == contents.as_bytes() {
        editor.contents_utf8 = Some(contents);
        editor.opened_modified_at_unix_ms = Some(disk_modified);
        editor.dirty = false;
        editor.conflict = None;
        return Ok(());
    }

    let opened_modified = expected_modified_at_unix_ms
        .or(editor.opened_modified_at_unix_ms)
        .unwrap_or(disk_modified);
    if disk_modified != opened_modified {
        editor.contents_utf8 = Some(contents);
        editor.dirty = true;
        editor.conflict = Some(EditorConflictSnapshot {
            disk_modified_at_unix_ms: disk_modified,
            opened_modified_at_unix_ms: opened_modified,
        });
        return Err("The file changed on disk; choose Reload or Keep Editing".to_owned());
    }

    let mut output = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|_| {
            "The file could not be opened for writing; the draft was preserved".to_owned()
        })?;
    output
        .write_all(contents.as_bytes())
        .and_then(|_| output.sync_all())
        .map_err(|_| "The file could not be saved; the draft was preserved".to_owned())?;
    let modified = fs::metadata(path)
        .ok()
        .and_then(|metadata| modified_milliseconds(&metadata).ok())
        .unwrap_or(disk_modified);
    editor.contents_utf8 = Some(contents);
    editor.opened_modified_at_unix_ms = Some(modified);
    editor.dirty = false;
    editor.conflict = None;
    Ok(())
}

pub fn reload(editor: &mut EditorDocumentSnapshot) -> Result<(), String> {
    let path = editor.path.clone();
    *editor = open(Path::new(&path))?;
    Ok(())
}

fn modified_milliseconds(metadata: &fs::Metadata) -> Result<u64, String> {
    metadata
        .modified()
        .map_err(|_| "File modification time is unavailable".to_owned())?
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .map_err(|_| "File modification time is invalid".to_owned())
}

fn language_for(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let whole_name = match name.to_ascii_lowercase().as_str() {
        ".gitignore" | ".gitattributes" | ".dockerignore" | ".npmignore" => Some("bash"),
        ".env" | ".editorconfig" => Some("ini"),
        "makefile" | "gnumakefile" => Some("makefile"),
        "dockerfile" => Some("dockerfile"),
        "gemfile" | "rakefile" => Some("ruby"),
        "cmakelists.txt" => Some("cmake"),
        _ => None,
    };
    if let Some(language) = whole_name {
        return Some(language.to_owned());
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())?;
    let language = match extension.as_str() {
        "rs" => "rust",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "json" | "jsonc" | "jsonl" => "json",
        "md" | "markdown" => "markdown",
        "sh" | "bash" | "zsh" | "fish" => "bash",
        "toml" | "ini" | "cfg" => "ini",
        "py" | "pyw" => "python",
        "htm" => "html",
        "scss" | "sass" | "less" => "css",
        _ => extension.as_str(),
    };
    Some(language.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_updates_keep_unsaved_contents_in_memory() {
        let mut editor = EditorDocumentSnapshot {
            path: "/tmp/existing.txt".to_owned(),
            language: Some("txt".to_owned()),
            contents_utf8: Some("old".to_owned()),
            opened_modified_at_unix_ms: Some(1),
            dirty: false,
            readonly_reason: None,
            conflict: None,
        };
        update_draft(&mut editor, "new".to_owned()).unwrap();
        assert!(editor.dirty);
        assert_eq!(editor.contents_utf8.as_deref(), Some("new"));
    }

    #[test]
    fn viewer_languages_cover_extensionless_configuration_and_json() {
        assert_eq!(
            language_for(Path::new(".gitignore")).as_deref(),
            Some("bash")
        );
        assert_eq!(
            language_for(Path::new("Makefile")).as_deref(),
            Some("makefile")
        );
        assert_eq!(
            language_for(Path::new("settings.jsonc")).as_deref(),
            Some("json")
        );
        assert_eq!(
            language_for(Path::new("manifest.json")).as_deref(),
            Some("json")
        );
        assert_eq!(language_for(Path::new("LICENSE")), None);
    }
}
