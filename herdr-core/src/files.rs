use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
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

/// One change the explorer asks the filesystem for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplorerOperationKind {
    FileCreate,
    DirCreate,
    PathRename,
    PathMove,
    /// The item goes to the platform Trash. There is no permanent delete;
    /// a filesystem with no Trash refuses the move and the item stays.
    PathTrash,
}

impl ExplorerOperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FileCreate => "file_create",
            Self::DirCreate => "dir_create",
            Self::PathRename => "path_rename",
            Self::PathMove => "path_move",
            Self::PathTrash => "path_trash",
        }
    }
}

/// An explorer change with both of its paths already decided.
///
/// Deciding is separate from doing because the decision runs under the
/// runtime mutex, where it may read nothing from disk, and the filesystem
/// call runs on a worker with the lock released. Everything the runtime
/// refuses - a path outside the root, a name with a separator, a folder
/// moved into itself - is refused here from the paths alone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplorerOperation {
    pub kind: ExplorerOperationKind,
    /// The item the change starts from: the path a new item takes, or the
    /// current path of the item being renamed or moved.
    pub source: PathBuf,
    /// Where the item is once the change has landed. Equal to `source` for
    /// a creation, and for a trash, whose item has no path here afterwards.
    pub destination: PathBuf,
    /// The row the tree selects once the change has landed: the item itself
    /// for a creation, rename or move, and the tree's chosen neighbour for
    /// a trash, whose item is no longer there to select.
    pub selection: PathBuf,
    /// The inode the operator was shown when a trash was confirmed, when the
    /// shell could read one. The call refuses an item with another inode:
    /// what leaves is what the modal named, not whatever replaced it at that
    /// path while the modal was open. `None` leaves only the existence
    /// check, which is all a rename or move has ever had.
    pub expected_inode: Option<u64>,
}

impl ExplorerOperation {
    pub fn create(
        kind: ExplorerOperationKind,
        root: &Path,
        parent: &Path,
        name: &str,
    ) -> Result<Self, String> {
        if !matches!(
            kind,
            ExplorerOperationKind::FileCreate | ExplorerOperationKind::DirCreate
        ) {
            return Err(format!("{} does not create an item", kind.as_str()));
        }
        let parent = path_inside_root(root, parent, true)?;
        let name = valid_item_name(name)?;
        let path = parent.join(name);
        Ok(Self {
            kind,
            source: path.clone(),
            destination: path.clone(),
            selection: path,
            expected_inode: None,
        })
    }

    pub fn rename(root: &Path, path: &Path, name: &str) -> Result<Self, String> {
        let source = path_inside_root(root, path, false)?;
        let name = valid_item_name(name)?;
        let destination = source
            .parent()
            .ok_or_else(|| "The item has no parent folder".to_owned())?
            .join(name);
        if destination == source {
            return Err("The name is unchanged".to_owned());
        }
        Ok(Self {
            kind: ExplorerOperationKind::PathRename,
            source,
            destination: destination.clone(),
            selection: destination,
            expected_inode: None,
        })
    }

    pub fn move_into(root: &Path, path: &Path, destination_dir: &Path) -> Result<Self, String> {
        let source = path_inside_root(root, path, false)?;
        let destination_dir = path_inside_root(root, destination_dir, true)?;
        let name = source
            .file_name()
            .ok_or_else(|| "The item has no name".to_owned())?;
        if destination_dir == source || destination_dir.starts_with(&source) {
            return Err("A folder cannot be moved into itself".to_owned());
        }
        let destination = destination_dir.join(name);
        if destination == source {
            return Err("The item is already in that folder".to_owned());
        }
        Ok(Self {
            kind: ExplorerOperationKind::PathMove,
            source,
            destination: destination.clone(),
            selection: destination,
            expected_inode: None,
        })
    }

    /// The item leaves the tree for the Trash and `select_after` takes its
    /// place as the selection. Refused from the strings alone like every
    /// other change: the root itself and anything outside it never reach
    /// the call, and a selection that would leave with the item is refused
    /// rather than pointed at nothing.
    pub fn trash(
        root: &Path,
        path: &Path,
        select_after: &Path,
        expected_inode: Option<u64>,
    ) -> Result<Self, String> {
        let source = path_inside_root(root, path, false)?;
        let selection = path_inside_root(root, select_after, true)?;
        if selection == source || selection.starts_with(&source) {
            return Err(
                "The selection cannot move into the item being moved to the Trash".to_owned(),
            );
        }
        Ok(Self {
            kind: ExplorerOperationKind::PathTrash,
            source: source.clone(),
            destination: source,
            selection,
            expected_inode,
        })
    }

    fn item_name(&self) -> String {
        self.destination
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn folder_name(&self) -> String {
        self.destination
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// Runs the change on disk. Nothing here overwrites: every call is the
/// exclusive form, so a name that appears between the runtime's decision
/// and this call is refused rather than replaced.
pub fn apply_explorer_operation(operation: &ExplorerOperation) -> Result<(), String> {
    let describe = |error: io::Error| describe_explorer_error(operation, error);
    match operation.kind {
        ExplorerOperationKind::FileCreate => OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&operation.destination)
            .map(drop)
            .map_err(describe),
        ExplorerOperationKind::DirCreate => {
            fs::create_dir(&operation.destination).map_err(describe)
        }
        ExplorerOperationKind::PathRename | ExplorerOperationKind::PathMove => {
            require_source(operation, describe)?;
            rename_exclusive(&operation.source, &operation.destination).map_err(describe)
        }
        ExplorerOperationKind::PathTrash => {
            let present = require_source(operation, describe)?;
            if let Some(expected) = operation.expected_inode
                && inode_of(&present) != expected
            {
                return Err(format!(
                    "{} changed while the prompt was open; nothing was moved",
                    operation.item_name()
                ));
            }
            move_to_trash(&operation.source).map_err(|error| {
                format!(
                    "{} could not be moved to the Trash: {error}",
                    operation.item_name()
                )
            })
        }
    }
}

/// A rename, move or trash of an item that is already gone is named as
/// such rather than reported as a failed write. The metadata comes back so
/// a trash can compare the inode without a second read.
fn require_source(
    operation: &ExplorerOperation,
    describe: impl FnOnce(io::Error) -> String,
) -> Result<fs::Metadata, String> {
    match fs::symlink_metadata(&operation.source) {
        Ok(metadata) => Ok(metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(format!(
            "{} no longer exists",
            operation
                .source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        )),
        Err(error) => Err(describe(error)),
    }
}

/// The inode is the identity the shell captured when it built the prompt
/// and the one the core checks before the move; see `expected_inode`.
#[cfg(unix)]
pub(crate) fn inode_of(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.ino()
}

#[cfg(not(unix))]
pub(crate) fn inode_of(_metadata: &fs::Metadata) -> u64 {
    0
}

/// Moves the item to the Trash through `NSFileManager` rather than through
/// Finder, which is the crate's default. The Finder route runs `osascript`
/// and asks macOS for Automation permission on first use; a refusal there
/// would fail every delete after it with a permission prompt the tree
/// cannot explain. The file-manager route needs no permission and no
/// subprocess. What it gives up is Finder's "Put Back" on some systems; the
/// item is still in the Trash and restores by dragging it out.
fn move_to_trash(path: &Path) -> Result<(), String> {
    let mut context = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        context.set_delete_method(DeleteMethod::NsFileManager);
    }
    context.delete(path).map_err(|error| match error {
        trash::Error::CouldNotAccess { .. } => "it is not accessible".to_owned(),
        trash::Error::TargetedRoot => "it is a volume root".to_owned(),
        trash::Error::Unknown { description } | trash::Error::Os { description, .. } => description,
        other => other.to_string(),
    })
}

fn describe_explorer_error(operation: &ExplorerOperation, error: io::Error) -> String {
    let name = operation.item_name();
    let folder = operation.folder_name();
    match error.kind() {
        io::ErrorKind::AlreadyExists => format!("{name} already exists in {folder}"),
        io::ErrorKind::NotFound => format!("The folder {folder} no longer exists"),
        io::ErrorKind::PermissionDenied => format!("{folder} is not writable"),
        io::ErrorKind::CrossesDevices => {
            "Items can only be moved within the same volume".to_owned()
        }
        _ => format!("{name} could not be written: {error}"),
    }
}

/// `rename(2)` replaces an existing destination, which is the one thing a
/// move or rename here must never do. macOS offers the exclusive form
/// directly; elsewhere the destination is checked first, which leaves a
/// window this crate does not otherwise close.
#[cfg(target_os = "macos")]
fn rename_exclusive(source: &Path, destination: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let from = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
    let to = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
    // SAFETY: both strings are NUL-terminated and outlive the call; the
    // call touches no memory of ours beyond reading them.
    let status = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "macos"))]
fn rename_exclusive(source: &Path, destination: &Path) -> io::Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(io::Error::from(io::ErrorKind::AlreadyExists));
    }
    fs::rename(source, destination)
}

/// The path as given when it is absolute, normal, and inside `root`.
/// Component-wise, so `/repo-other` is outside `/repo`; lexical, so a
/// symlink that escapes the root is not followed here and cannot be
/// created here either.
fn path_inside_root(root: &Path, path: &Path, allow_root: bool) -> Result<PathBuf, String> {
    if !root.is_absolute() {
        return Err("The workspace root is not an absolute path".to_owned());
    }
    if !path.is_absolute() {
        return Err(format!("{} is not an absolute path", path.display()));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(format!("{} is not a normal path", path.display()));
    }
    if !path.starts_with(root) {
        return Err(format!(
            "{} is outside the workspace {}",
            path.display(),
            root.display()
        ));
    }
    if !allow_root && path == root {
        return Err("The workspace root itself cannot be changed".to_owned());
    }
    Ok(path.to_path_buf())
}

fn valid_item_name(name: &str) -> Result<&str, String> {
    if name.is_empty() {
        return Err("A name is required".to_owned());
    }
    if name.contains('/') {
        return Err("A name cannot contain /".to_owned());
    }
    if name.contains('\0') {
        return Err("A name cannot contain NUL".to_owned());
    }
    if name == "." || name == ".." {
        return Err(format!("{name} is not a valid name"));
    }
    Ok(name)
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
pub(crate) mod tests {
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

    fn explorer_fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hide-explorer-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("README.md"), "readme").unwrap();
        fs::write(root.join("src/lib.rs"), "lib").unwrap();
        root
    }

    /// Names no other Trash entry can carry, so a trashed fixture can be
    /// found and removed by exact name without touching anything else.
    pub(crate) fn unique_trash_names() -> (String, String) {
        let stamp = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        (
            format!("hide-test-trash-file-{stamp}.txt"),
            format!("hide-test-trash-folder-{stamp}"),
        )
    }

    /// Removes the named fixtures from `~/.Trash`; a name that is not there
    /// is left alone. `HOME` is read because that is where macOS keeps the
    /// account Trash; the move itself ignores the variable, so a test cannot
    /// redirect it and can only clean up after it.
    pub(crate) fn remove_from_trash(names: &[&str]) {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let trash = Path::new(&home).join(".Trash");
        for name in names {
            let entry = trash.join(name);
            match fs::symlink_metadata(&entry) {
                Ok(metadata) if metadata.is_dir() => {
                    fs::remove_dir_all(&entry).ok();
                }
                Ok(_) => {
                    fs::remove_file(&entry).ok();
                }
                Err(_) => {}
            }
        }
    }

    #[test]
    fn explorer_creates_a_file_and_a_folder_inside_the_root() {
        let root = explorer_fixture();
        let file = ExplorerOperation::create(
            ExplorerOperationKind::FileCreate,
            &root,
            &root.join("src"),
            "new.rs",
        )
        .unwrap();
        apply_explorer_operation(&file).unwrap();
        assert!(root.join("src/new.rs").is_file());
        assert_eq!(file.destination, root.join("src/new.rs"));

        let dir = ExplorerOperation::create(ExplorerOperationKind::DirCreate, &root, &root, "docs")
            .unwrap();
        apply_explorer_operation(&dir).unwrap();
        assert!(root.join("docs").is_dir());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn explorer_renames_and_moves_without_touching_siblings() {
        let root = explorer_fixture();
        let rename = ExplorerOperation::rename(&root, &root.join("README.md"), "GUIDE.md").unwrap();
        apply_explorer_operation(&rename).unwrap();
        assert!(!root.join("README.md").exists());
        assert_eq!(fs::read_to_string(root.join("GUIDE.md")).unwrap(), "readme");

        let moved =
            ExplorerOperation::move_into(&root, &root.join("src/lib.rs"), &root.join("src/nested"))
                .unwrap();
        apply_explorer_operation(&moved).unwrap();
        assert_eq!(moved.destination, root.join("src/nested/lib.rs"));
        assert!(!root.join("src/lib.rs").exists());
        assert_eq!(fs::read_to_string(&moved.destination).unwrap(), "lib");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn explorer_refuses_to_overwrite_an_existing_item() {
        let root = explorer_fixture();
        let create =
            ExplorerOperation::create(ExplorerOperationKind::FileCreate, &root, &root, "README.md")
                .unwrap();
        let error = apply_explorer_operation(&create).unwrap_err();
        assert!(error.contains("already exists"), "{error}");
        assert_eq!(
            fs::read_to_string(root.join("README.md")).unwrap(),
            "readme"
        );

        fs::write(root.join("src/nested/README.md"), "nested").unwrap();
        let moved =
            ExplorerOperation::move_into(&root, &root.join("README.md"), &root.join("src/nested"))
                .unwrap();
        let error = apply_explorer_operation(&moved).unwrap_err();
        assert!(error.contains("already exists"), "{error}");
        assert_eq!(
            fs::read_to_string(root.join("README.md")).unwrap(),
            "readme"
        );
        assert_eq!(
            fs::read_to_string(root.join("src/nested/README.md")).unwrap(),
            "nested"
        );

        let rename = ExplorerOperation::rename(&root, &root.join("src/lib.rs"), "nested").unwrap();
        let error = apply_explorer_operation(&rename).unwrap_err();
        assert!(error.contains("already exists"), "{error}");
        assert!(root.join("src/lib.rs").is_file());
        assert!(root.join("src/nested").is_dir());
        fs::remove_dir_all(&root).unwrap();
    }

    /// The moved items really land in the account's Trash: the crate has no
    /// fixture Trash and `NSFileManager` ignores `HOME`. So they carry names
    /// nothing else in the Trash has, and the test removes them from
    /// `~/.Trash` afterwards. On a volume whose Trash is elsewhere the
    /// removal finds nothing and the item stays there, which is the one
    /// side effect this test cannot avoid.
    #[test]
    fn explorer_moves_a_file_and_a_folder_to_the_trash() {
        let root = explorer_fixture();
        let (file_name, folder_name) = unique_trash_names();
        let file_path = root.join("src").join(&file_name);
        let folder_path = root.join(&folder_name);
        fs::write(&file_path, "trashed").unwrap();
        fs::create_dir(&folder_path).unwrap();
        fs::write(folder_path.join("inside.txt"), "inside").unwrap();

        let file =
            ExplorerOperation::trash(&root, &file_path, &root.join("src/nested"), None).unwrap();
        assert_eq!(file.kind, ExplorerOperationKind::PathTrash);
        assert_eq!(file.destination, file_path);
        assert_eq!(file.selection, root.join("src/nested"));
        apply_explorer_operation(&file).unwrap();
        assert!(!file_path.exists());
        assert!(root.join("src/lib.rs").is_file());
        assert!(root.join("src/nested").is_dir());

        let folder = ExplorerOperation::trash(&root, &folder_path, &root, None).unwrap();
        assert_eq!(folder.selection, root);
        apply_explorer_operation(&folder).unwrap();
        assert!(!folder_path.exists());
        assert!(root.join("README.md").is_file());
        fs::remove_dir_all(&root).unwrap();
        remove_from_trash(&[&file_name, &folder_name]);
    }

    /// D-03: the item the modal named is the item that moves. A different
    /// inode at the same path is refused and left where it is.
    #[test]
    fn explorer_refuses_to_trash_an_item_whose_inode_changed_under_the_prompt() {
        let root = explorer_fixture();
        let lib = root.join("src/lib.rs");
        let shown = inode_of(&fs::symlink_metadata(&lib).unwrap());
        // Keep the shown inode alive so the filesystem cannot hand its
        // number to the replacement.
        fs::rename(&lib, root.join("src/old.rs")).unwrap();
        fs::write(&lib, "rewritten").unwrap();
        assert_ne!(inode_of(&fs::symlink_metadata(&lib).unwrap()), shown);

        let stale = ExplorerOperation::trash(&root, &lib, &root.join("src"), Some(shown)).unwrap();
        let error = apply_explorer_operation(&stale).unwrap_err();
        assert_eq!(
            error,
            "lib.rs changed while the prompt was open; nothing was moved"
        );
        assert_eq!(fs::read_to_string(&lib).unwrap(), "rewritten");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn explorer_refuses_to_trash_outside_the_root_or_the_root_itself() {
        let root = Path::new("/repo");
        let outside = ExplorerOperation::trash(root, Path::new("/repo-other/a"), root, None);
        assert!(outside.unwrap_err().contains("outside the workspace"));
        let escaping = ExplorerOperation::trash(root, Path::new("/repo/../etc/passwd"), root, None);
        assert!(escaping.unwrap_err().contains("not a normal path"));
        let root_itself = ExplorerOperation::trash(root, root, root, None);
        assert!(root_itself.unwrap_err().contains("root itself"));
        let relative = ExplorerOperation::trash(root, Path::new("src/a"), root, None);
        assert!(relative.unwrap_err().contains("not an absolute path"));

        let selection_outside =
            ExplorerOperation::trash(root, Path::new("/repo/src"), Path::new("/repo-other"), None);
        assert!(
            selection_outside
                .unwrap_err()
                .contains("outside the workspace")
        );
        let selection_inside = ExplorerOperation::trash(
            root,
            Path::new("/repo/src"),
            Path::new("/repo/src/a.rs"),
            None,
        );
        assert!(
            selection_inside
                .unwrap_err()
                .contains("cannot move into the item")
        );
        let selection_itself =
            ExplorerOperation::trash(root, Path::new("/repo/src"), Path::new("/repo/src"), None);
        assert!(
            selection_itself
                .unwrap_err()
                .contains("cannot move into the item")
        );
    }

    #[test]
    fn explorer_reports_a_missing_item_instead_of_trashing_it() {
        let root = explorer_fixture();
        let missing =
            ExplorerOperation::trash(&root, &root.join("src/gone.rs"), &root.join("src"), None)
                .unwrap();
        let error = apply_explorer_operation(&missing).unwrap_err();
        assert_eq!(error, "gone.rs no longer exists");
        assert!(root.join("src/lib.rs").is_file());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn explorer_refuses_paths_outside_the_root_and_invalid_names() {
        let root = Path::new("/repo");
        let outside = ExplorerOperation::create(
            ExplorerOperationKind::FileCreate,
            root,
            Path::new("/repo-other"),
            "a",
        );
        assert!(outside.unwrap_err().contains("outside the workspace"));
        let escaping = ExplorerOperation::rename(root, Path::new("/repo/../etc/passwd"), "x");
        assert!(escaping.unwrap_err().contains("not a normal path"));
        let root_itself = ExplorerOperation::rename(root, root, "x");
        assert!(root_itself.unwrap_err().contains("root itself"));
        let relative = ExplorerOperation::move_into(root, Path::new("src/a"), root);
        assert!(relative.unwrap_err().contains("not an absolute path"));

        let parent = Path::new("/repo/src");
        for (name, expected) in [
            ("", "A name is required"),
            ("a/b", "cannot contain /"),
            ("..", "not a valid name"),
        ] {
            let error =
                ExplorerOperation::create(ExplorerOperationKind::DirCreate, root, parent, name)
                    .unwrap_err();
            assert!(error.contains(expected), "{name:?}: {error}");
        }
    }

    #[test]
    fn explorer_refuses_a_move_that_changes_nothing_or_nests_a_folder_in_itself() {
        let root = Path::new("/repo");
        let same_parent =
            ExplorerOperation::move_into(root, Path::new("/repo/src/a.rs"), Path::new("/repo/src"));
        assert!(same_parent.unwrap_err().contains("already in that folder"));
        let into_self =
            ExplorerOperation::move_into(root, Path::new("/repo/src"), Path::new("/repo/src"));
        assert!(into_self.unwrap_err().contains("into itself"));
        let into_child = ExplorerOperation::move_into(
            root,
            Path::new("/repo/src"),
            Path::new("/repo/src/nested"),
        );
        assert!(into_child.unwrap_err().contains("into itself"));
        let unchanged = ExplorerOperation::rename(root, Path::new("/repo/src/a.rs"), "a.rs");
        assert!(unchanged.unwrap_err().contains("unchanged"));
    }
}
