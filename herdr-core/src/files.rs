use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use cap_std::fs::{Dir, OpenOptions as CapOpenOptions};

use crate::model::{DocumentKind, EditorConflictSnapshot, EditorDocumentSnapshot};

/// The largest file the editor reads into a document. Past it the shell
/// offers the OS default handler instead (PRD S3 D-12).
const MAX_EDITABLE_BYTES: u64 = 16 * 1024 * 1024;

/// The first bytes of every PDF, whatever the file is called.
const PDF_SIGNATURE: &[u8] = b"%PDF-";

/// The image kinds the shell decodes with the platform image loader. The core
/// does not decode images, so the extension is the decision; the loader
/// reports a file that is not what its name says.
const IMAGE_EXTENSIONS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "tiff", "heic", "avif"];

/// Opened checkout roots supplied by the daemon after its registration check.
/// The Swift shell uses the ambient path calls below; both shells share the
/// document and explorer logic, while the daemon's paths resolve through
/// these directory capabilities when the actual I/O runs.
#[derive(Clone, Default)]
pub struct FileRoots(Arc<Vec<(PathBuf, Arc<Dir>)>>);

impl FileRoots {
    pub fn from_opened(roots: Vec<(PathBuf, File)>) -> Self {
        Self(Arc::new(
            roots
                .into_iter()
                .map(|(path, file)| (path, Arc::new(Dir::from_std_file(file))))
                .collect(),
        ))
    }

    fn relative<'a>(&'a self, path: &'a Path) -> io::Result<(&'a Dir, &'a Path)> {
        self.0
            .iter()
            .filter_map(|(root, dir)| path.strip_prefix(root).ok().map(|rest| (root, dir, rest)))
            .max_by_key(|(root, _, _)| root.components().count())
            .map(|(_, dir, rest)| (dir.as_ref(), rest))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "path is outside registered checkout",
                )
            })
    }

    pub(crate) fn open(&self, path: &Path, write: bool) -> io::Result<File> {
        let (dir, relative) = self.relative(path)?;
        let mut options = CapOpenOptions::new();
        options.read(true).write(write);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
        }
        dir.open_with(relative, &options)
            .map(|file| file.into_std())
    }

    fn parent(&self, path: &Path) -> io::Result<(Dir, std::ffi::OsString)> {
        let (dir, relative) = self.relative(path)?;
        let name = relative.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "checkout root is not an item")
        })?;
        let parent = relative
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        dir.open_dir(parent).map(|parent| (parent, name.to_owned()))
    }
}

fn open_handle(path: &Path, roots: Option<&FileRoots>, write: bool) -> io::Result<File> {
    match roots {
        Some(roots) => roots.open(path, write),
        None if write => OpenOptions::new().read(true).write(true).open(path),
        None => File::open(path),
    }
}

#[cfg(test)]
pub fn open(path: &Path) -> Result<EditorDocumentSnapshot, String> {
    open_with_roots(path, None)
}

pub fn open_with_roots(
    path: &Path,
    roots: Option<&FileRoots>,
) -> Result<EditorDocumentSnapshot, String> {
    let mut file = open_handle(path, roots, false)
        .map_err(|_| "The selected file could not be read".to_owned())?;
    let metadata = file
        .metadata()
        .map_err(|_| "The selected file could not be read".to_owned())?;
    if !metadata.is_file() {
        return Err("Only existing regular files can be opened".to_owned());
    }
    let modified = modified_milliseconds(&metadata)?;
    let language = language_for(path);
    let document = |document_kind, contents_utf8, readonly_reason| EditorDocumentSnapshot {
        path: path.to_string_lossy().into_owned(),
        language: language.clone(),
        document_kind,
        contents_utf8,
        opened_modified_at_unix_ms: Some(modified),
        dirty: false,
        readonly_reason,
        conflict: None,
    };

    // An image or a PDF is drawn from disk by the shell, so its size is not
    // the editor's concern and its bytes are never carried in the snapshot.
    if has_image_extension(path) {
        return Ok(document(DocumentKind::Image, None, None));
    }
    if starts_with_pdf_signature(&mut file)? {
        return Ok(document(DocumentKind::Pdf, None, None));
    }
    if metadata.len() > MAX_EDITABLE_BYTES {
        return Ok(document(
            DocumentKind::Text,
            None,
            Some("Files larger than 16 MB are preview-only".to_owned()),
        ));
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "The selected file contents could not be read".to_owned())?;
    let mut bytes = Vec::new();
    file.take(MAX_EDITABLE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The selected file contents could not be read".to_owned())?;
    if bytes.len() as u64 > MAX_EDITABLE_BYTES {
        return Ok(document(
            DocumentKind::Text,
            None,
            Some("Files larger than 16 MB are preview-only".to_owned()),
        ));
    }
    let Ok(contents) = String::from_utf8(bytes) else {
        return Ok(document(DocumentKind::Binary, None, None));
    };
    let kind = if language.as_deref() == Some("markdown") {
        DocumentKind::Markdown
    } else {
        DocumentKind::Text
    };
    let reason = metadata
        .permissions()
        .readonly()
        .then(|| "The file is read-only on disk; editing is disabled".to_owned());
    Ok(document(kind, Some(contents), reason))
}

fn has_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            IMAGE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
}

/// Reads only the signature's worth of bytes: a PDF is recognised by its
/// content so that a file with no extension opens as one, and a large PDF is
/// not read whole to find that out.
fn starts_with_pdf_signature(file: &mut File) -> Result<bool, String> {
    let mut header = [0u8; PDF_SIGNATURE.len()];
    let mut filled = 0;
    while filled < header.len() {
        match file.read(&mut header[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Err("The selected file contents could not be read".to_owned()),
        }
    }
    Ok(&header[..filled] == PDF_SIGNATURE)
}

pub fn update_draft(editor: &mut EditorDocumentSnapshot, contents: String) -> Result<(), String> {
    if !editor.document_kind.is_editable() {
        return Err(
            "The current file is not a text document; the draft was not changed".to_owned(),
        );
    }
    if editor.readonly_reason.is_some() {
        return Err("The current file is read-only; the draft was not changed".to_owned());
    }
    editor.dirty = editor.contents_utf8.as_deref() != Some(contents.as_str());
    editor.contents_utf8 = Some(contents);
    Ok(())
}

#[cfg(test)]
pub fn save(
    editor: &mut EditorDocumentSnapshot,
    path: &Path,
    contents: String,
    expected_modified_at_unix_ms: Option<u64>,
) -> Result<(), String> {
    save_with_roots(editor, path, contents, expected_modified_at_unix_ms, None)
}

pub fn save_with_roots(
    editor: &mut EditorDocumentSnapshot,
    path: &Path,
    contents: String,
    expected_modified_at_unix_ms: Option<u64>,
    roots: Option<&FileRoots>,
) -> Result<(), String> {
    if editor.path != path.to_string_lossy() {
        return Err("The save target does not match the open document".to_owned());
    }
    if !editor.document_kind.is_editable() {
        return Err("The current file is not a text document; the draft was preserved".to_owned());
    }
    if editor.readonly_reason.is_some() {
        return Err("The current file is read-only; the draft was preserved".to_owned());
    }
    let mut output = open_handle(path, roots, true).map_err(|_| {
        "The existing file could not be inspected; the draft was preserved".to_owned()
    })?;
    let metadata = output.metadata().map_err(|_| {
        "The existing file could not be inspected; the draft was preserved".to_owned()
    })?;
    if !metadata.is_file() {
        return Err(
            "The save target is no longer a regular file; the draft was preserved".to_owned(),
        );
    }
    let disk_modified = modified_milliseconds(&metadata)?;
    let mut disk_bytes = Vec::new();
    (&mut output)
        .take(MAX_EDITABLE_BYTES + 1)
        .read_to_end(&mut disk_bytes)
        .map_err(|_| "The existing file could not be read; the draft was preserved".to_owned())?;
    if disk_bytes.len() as u64 > MAX_EDITABLE_BYTES {
        return Err("The file grew beyond the editable size; the draft was preserved".to_owned());
    }

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

    output
        .seek(SeekFrom::Start(0))
        .and_then(|_| output.set_len(0))
        .map_err(|_| {
            "The file could not be opened for writing; the draft was preserved".to_owned()
        })?;
    output
        .write_all(contents.as_bytes())
        .and_then(|_| output.sync_all())
        .map_err(|_| "The file could not be saved; the draft was preserved".to_owned())?;
    let modified = output
        .metadata()
        .ok()
        .and_then(|metadata| modified_milliseconds(&metadata).ok())
        .unwrap_or(disk_modified);
    editor.contents_utf8 = Some(contents);
    editor.opened_modified_at_unix_ms = Some(modified);
    editor.dirty = false;
    editor.conflict = None;
    Ok(())
}

pub fn reload_with_roots(
    editor: &mut EditorDocumentSnapshot,
    roots: Option<&FileRoots>,
) -> Result<(), String> {
    let path = editor.path.clone();
    *editor = open_with_roots(Path::new(&path), roots)?;
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
#[cfg(test)]
pub fn apply_explorer_operation(operation: &ExplorerOperation) -> Result<(), String> {
    apply_explorer_operation_with_roots(operation, None)
}

pub fn apply_explorer_operation_with_roots(
    operation: &ExplorerOperation,
    roots: Option<&FileRoots>,
) -> Result<(), String> {
    if let Some(roots) = roots {
        return apply_rooted_explorer_operation(operation, roots);
    }
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

/// The daemon's mutation path uses opened parent directories. Relative
/// operations remain inside the registered root even if a checked pathname
/// is replaced before the worker runs.
fn apply_rooted_explorer_operation(
    operation: &ExplorerOperation,
    roots: &FileRoots,
) -> Result<(), String> {
    let describe = |error: io::Error| describe_explorer_error(operation, error);
    let (source_parent, source_name) = roots.parent(&operation.source).map_err(describe)?;
    match operation.kind {
        ExplorerOperationKind::FileCreate => {
            let mut options = CapOpenOptions::new();
            options.write(true).create_new(true);
            source_parent
                .open_with(Path::new(&source_name), &options)
                .map(drop)
                .map_err(describe)
        }
        ExplorerOperationKind::DirCreate => source_parent
            .create_dir(Path::new(&source_name))
            .map_err(describe),
        ExplorerOperationKind::PathRename | ExplorerOperationKind::PathMove => {
            rooted_source(&source_parent, &source_name, operation, describe)?;
            let (destination_parent, destination_name) =
                roots.parent(&operation.destination).map_err(describe)?;
            rename_rooted_exclusive(
                &source_parent,
                &source_name,
                &destination_parent,
                &destination_name,
            )
            .map_err(describe)
        }
        ExplorerOperationKind::PathTrash => {
            let present = rooted_source(&source_parent, &source_name, operation, describe)?;
            if let Some(expected) = operation.expected_inode
                && rooted_inode_of(&present) != expected
            {
                return Err(format!(
                    "{} changed while the prompt was open; nothing was moved",
                    operation.item_name()
                ));
            }
            move_rooted_to_trash(&source_parent, &source_name, operation, roots)
        }
    }
}

fn rooted_source(
    parent: &Dir,
    name: &std::ffi::OsStr,
    operation: &ExplorerOperation,
    describe: impl FnOnce(io::Error) -> String,
) -> Result<cap_std::fs::Metadata, String> {
    match parent.symlink_metadata(Path::new(name)) {
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

#[cfg(unix)]
fn rooted_inode_of(metadata: &cap_std::fs::Metadata) -> u64 {
    use cap_std::fs::MetadataExt;
    metadata.ino()
}

#[cfg(not(unix))]
fn rooted_inode_of(_metadata: &cap_std::fs::Metadata) -> u64 {
    0
}

#[cfg(target_os = "macos")]
fn rename_rooted_exclusive(
    from_dir: &Dir,
    from: &std::ffi::OsStr,
    to_dir: &Dir,
    to: &std::ffi::OsStr,
) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let from = CString::new(from.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid source name"))?;
    let to = CString::new(to.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid destination name"))?;
    let result = unsafe {
        libc::renameatx_np(
            from_dir.as_raw_fd(),
            from.as_ptr(),
            to_dir.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn rename_rooted_exclusive(
    from_dir: &Dir,
    from: &std::ffi::OsStr,
    to_dir: &Dir,
    to: &std::ffi::OsStr,
) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let from = CString::new(from.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid source name"))?;
    let to = CString::new(to.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid destination name"))?;
    let result = unsafe {
        libc::renameat2(
            from_dir.as_raw_fd(),
            from.as_ptr(),
            to_dir.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn rename_rooted_exclusive(
    from_dir: &Dir,
    from: &std::ffi::OsStr,
    to_dir: &Dir,
    to: &std::ffi::OsStr,
) -> io::Result<()> {
    if to_dir.symlink_metadata(Path::new(to)).is_ok() {
        return Err(io::Error::from(io::ErrorKind::AlreadyExists));
    }
    from_dir.rename(Path::new(from), to_dir, Path::new(to))
}

fn move_rooted_to_trash(
    parent: &Dir,
    name: &std::ffi::OsStr,
    operation: &ExplorerOperation,
    roots: &FileRoots,
) -> Result<(), String> {
    // The platform Trash API takes a pathname. Move the selected item through
    // its checked parent handle into a private directory outside the mutable
    // checkout spelling, then hand that independent path to the OS.
    let stage = tempfile::Builder::new()
        .prefix("hide-trash-")
        .tempdir()
        .map_err(|error| format!("Trash staging could not be created: {error}"))?;
    if stage_is_inside_root(stage.path(), roots)
        .map_err(|error| format!("Trash staging could not be inspected: {error}"))?
    {
        return Err("Trash staging must be outside registered checkouts".to_owned());
    }
    let staged = Dir::open_ambient_dir(stage.path(), cap_std::ambient_authority())
        .map_err(|error| format!("Trash staging could not be opened: {error}"))?;
    let item_path = stage.path().join(name);
    let mut preserve_stage = false;
    let result = (|| {
        rename_rooted_exclusive(parent, name, &staged, name).map_err(|error| {
            if error.kind() == io::ErrorKind::CrossesDevices {
                "Trash staging is on another volume; the item was left in place".to_owned()
            } else {
                describe_explorer_error(operation, error)
            }
        })?;
        let before_handoff = (|| {
            if let Some(expected) = operation.expected_inode {
                let actual = staged.symlink_metadata(Path::new(name)).map_err(|error| {
                    format!(
                        "{} could not be inspected after staging: {error}",
                        operation.item_name()
                    )
                })?;
                if rooted_inode_of(&actual) != expected {
                    return Err(format!(
                        "{} changed while the prompt was open; nothing was moved",
                        operation.item_name()
                    ));
                }
            }
            Ok(())
        })();
        if let Err(error) = before_handoff {
            rename_rooted_exclusive(&staged, name, parent, name).map_err(|restore_error| {
                preserve_stage = true;
                format!(
                    "{} could not be restored after staging: {restore_error}",
                    operation.item_name()
                )
            })?;
            return Err(error);
        }
        let outcome = move_to_trash(&item_path).map_err(|error| {
            format!(
                "{} could not be moved to the Trash: {error}",
                operation.item_name()
            )
        });
        if outcome.is_err() {
            rename_rooted_exclusive(&staged, name, parent, name).map_err(|error| {
                preserve_stage = true;
                format!(
                    "Trash failed and {} could not be restored: {error}",
                    operation.item_name()
                )
            })?;
        }
        outcome
    })();
    if preserve_stage {
        let recovery = stage.keep();
        return Err(format!(
            "{}; inspect recovery staging at {}",
            result.unwrap_err(),
            recovery.display()
        ));
    }
    result
}

/// Compare the temporary path's ancestors with the *opened* checkout roots,
/// not their mutable names. This also covers an environment temp directory
/// placed inside a checkout whose original spelling has since been replaced.
fn stage_is_inside_root(stage: &Path, roots: &FileRoots) -> io::Result<bool> {
    let real = stage.canonicalize()?;
    for (_, root) in roots.0.iter() {
        let root_metadata = root.dir_metadata()?;
        for ancestor in real.ancestors() {
            if same_directory_identity(&root_metadata, &fs::metadata(ancestor)?) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(unix)]
fn same_directory_identity(root: &cap_std::fs::Metadata, other: &fs::Metadata) -> bool {
    use cap_std::fs::MetadataExt as _;
    use std::os::unix::fs::MetadataExt as _;
    root.dev() == other.dev() && root.ino() == other.ino()
}

#[cfg(windows)]
fn same_directory_identity(root: &cap_std::fs::Metadata, other: &fs::Metadata) -> bool {
    use cap_std::fs::MetadataExt as _;
    use std::os::windows::fs::MetadataExt as _;
    root.volume_serial_number()
        .zip(root.file_index())
        .is_some_and(|identity| {
            Some(identity) == other.volume_serial_number().zip(other.file_index())
        })
}

#[cfg(not(any(unix, windows)))]
fn same_directory_identity(_root: &cap_std::fs::Metadata, _other: &fs::Metadata) -> bool {
    true
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
        "md" | "markdown" | "mdown" => "markdown",
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
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_EXPLORER_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn draft_updates_keep_unsaved_contents_in_memory() {
        let mut editor = EditorDocumentSnapshot {
            path: "/tmp/existing.txt".to_owned(),
            language: Some("txt".to_owned()),
            document_kind: DocumentKind::Text,
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

    fn kind_fixture(name: &str, bytes: &[u8]) -> PathBuf {
        let root = explorer_fixture();
        let path = root.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    /// D-02: a PDF is its signature, not its name, so it opens as one with
    /// any extension or none; the snapshot carries no bytes for it and no
    /// read-only reason, because the kind already says it takes no edits.
    #[test]
    fn a_pdf_is_recognised_by_its_signature_whatever_it_is_called() {
        for name in ["report.pdf", "report", "report.txt"] {
            let path = kind_fixture(name, b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n");
            let document = open(&path).unwrap();
            assert_eq!(document.document_kind, DocumentKind::Pdf, "{name}");
            assert_eq!(document.contents_utf8, None, "{name}");
            assert_eq!(document.readonly_reason, None, "{name}");
            fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    /// B3 has a file called `.pdf` that is not one: the kind follows the
    /// bytes, so the shell draws it as what it is rather than failing a
    /// PDF decode it was never going to pass.
    #[test]
    fn a_file_named_pdf_without_the_signature_is_not_a_pdf() {
        let text = kind_fixture("notes.pdf", b"just text");
        assert_eq!(open(&text).unwrap().document_kind, DocumentKind::Text);
        let binary = kind_fixture("blob.pdf", &[0xFF, 0xFE, 0x00, 0x80]);
        let document = open(&binary).unwrap();
        assert_eq!(document.document_kind, DocumentKind::Binary);
        assert_eq!(document.contents_utf8, None);
        assert_eq!(document.readonly_reason, None);
        for path in [text, binary] {
            fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    /// D-02: images keep the extension decision the shell used to make, and
    /// the empty-signature case (a zero-byte file) is plain text.
    #[test]
    fn images_markdown_text_and_empty_files_take_their_kinds() {
        let cases: [(&str, &[u8], DocumentKind); 6] = [
            ("shot.PNG", &[0x89, b'P', b'N', b'G'], DocumentKind::Image),
            ("photo.heic", b"", DocumentKind::Image),
            ("README.md", b"# hi", DocumentKind::Markdown),
            ("notes.mdown", b"# hi", DocumentKind::Markdown),
            ("main.rs", b"fn main() {}", DocumentKind::Text),
            ("empty", b"", DocumentKind::Text),
        ];
        for (name, bytes, expected) in cases {
            let path = kind_fixture(name, bytes);
            let document = open(&path).unwrap();
            assert_eq!(document.document_kind, expected, "{name}");
            assert_eq!(
                document.contents_utf8.is_some(),
                expected.is_editable(),
                "{name}"
            );
            fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    /// D-01: the size reason stays on a text document; a non-text kind
    /// refuses a draft on its own, before the read-only reason is asked.
    #[test]
    fn non_text_kinds_refuse_drafts_and_saves() {
        let path = kind_fixture("report.pdf", b"%PDF-1.4");
        let mut document = open(&path).unwrap();
        let refused = update_draft(&mut document, "edited".to_owned()).unwrap_err();
        assert!(refused.contains("not a text document"), "{refused}");
        assert_eq!(document.contents_utf8, None);
        assert!(!document.dirty);
        let refused = save(&mut document, &path, "edited".to_owned(), None).unwrap_err();
        assert!(refused.contains("not a text document"), "{refused}");
        assert_eq!(fs::read(&path).unwrap(), b"%PDF-1.4");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
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

    #[test]
    fn a_document_past_the_editable_cap_opens_as_a_preview() {
        let root = explorer_fixture();
        let big = root.join("big.txt");
        // Sparse: the file reports the size without holding the bytes.
        File::create(&big)
            .unwrap()
            .set_len(MAX_EDITABLE_BYTES + 1)
            .unwrap();
        let document = open(&big).unwrap();
        assert_eq!(document.document_kind, DocumentKind::Text);
        assert_eq!(document.contents_utf8, None);
        assert_eq!(
            document.readonly_reason.as_deref(),
            Some("Files larger than 16 MB are preview-only")
        );
        let at_cap = root.join("at-cap.txt");
        File::create(&at_cap)
            .unwrap()
            .set_len(MAX_EDITABLE_BYTES)
            .unwrap();
        let document = open(&at_cap).unwrap();
        assert!(
            document.contents_utf8.is_some(),
            "the cap itself is editable"
        );
        fs::remove_dir_all(&root).ok();
    }

    fn explorer_fixture() -> PathBuf {
        let sequence = NEXT_EXPLORER_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "hide-explorer-{}-{}-{sequence}",
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

    #[cfg(unix)]
    #[test]
    fn opened_checkout_root_keeps_reads_and_saves_inside_after_path_swap() {
        use std::os::unix::fs::symlink;
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("checkout");
        let outside = sandbox.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(root.join("note.txt"), "inside").unwrap();
        fs::write(outside.join("note.txt"), "outside").unwrap();
        let roots = FileRoots::from_opened(vec![(root.clone(), File::open(&root).unwrap())]);
        let mut document = open_with_roots(&root.join("note.txt"), Some(&roots)).unwrap();
        assert_eq!(document.contents_utf8.as_deref(), Some("inside"));
        let create = ExplorerOperation::create(
            ExplorerOperationKind::FileCreate,
            &root,
            &root,
            "created.txt",
        )
        .unwrap();
        fs::rename(&root, sandbox.path().join("moved")).unwrap();
        symlink(&outside, &root).unwrap();
        save_with_roots(
            &mut document,
            &root.join("note.txt"),
            "edited".to_owned(),
            None,
            Some(&roots),
        )
        .unwrap();
        apply_explorer_operation_with_roots(&create, Some(&roots)).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.path().join("moved/note.txt")).unwrap(),
            "edited"
        );
        assert!(sandbox.path().join("moved/created.txt").is_file());
        assert!(!outside.join("created.txt").exists());
        assert_eq!(
            fs::read_to_string(outside.join("note.txt")).unwrap(),
            "outside"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rooted_mutations_refuse_outside_symlink() {
        use std::os::unix::fs::symlink;
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("checkout");
        let outside = sandbox.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        let roots = FileRoots::from_opened(vec![(root.clone(), File::open(&root).unwrap())]);
        fs::create_dir(root.join("escape")).unwrap();
        let escaped = ExplorerOperation::create(
            ExplorerOperationKind::FileCreate,
            &root,
            &root.join("escape"),
            "bad.txt",
        )
        .unwrap();
        fs::rename(root.join("escape"), root.join("former_escape")).unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        assert!(apply_explorer_operation_with_roots(&escaped, Some(&roots)).is_err());
        assert!(!outside.join("bad.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn rooted_trash_hands_off_the_selected_item_after_checkout_path_replacement() {
        use std::os::unix::fs::symlink;
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("checkout");
        let moved = sandbox.path().join("moved");
        let outside = sandbox.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        let (name, _) = unique_trash_names();
        fs::write(root.join(&name), "selected").unwrap();
        fs::write(outside.join(&name), "outside").unwrap();
        let roots = FileRoots::from_opened(vec![(root.clone(), File::open(&root).unwrap())]);
        let shown = inode_of(&fs::symlink_metadata(root.join(&name)).unwrap());
        let operation =
            ExplorerOperation::trash(&root, &root.join(&name), &root, Some(shown)).unwrap();

        fs::rename(&root, &moved).unwrap();
        symlink(&outside, &root).unwrap();
        apply_explorer_operation_with_roots(&operation, Some(&roots)).unwrap();
        assert!(!moved.join(&name).exists());
        assert_eq!(fs::read_to_string(outside.join(&name)).unwrap(), "outside");
        fs::remove_file(&root).unwrap();
        remove_from_trash(&[&name]);
    }

    #[cfg(unix)]
    #[test]
    fn trash_stage_checks_the_opened_root_even_after_its_name_moves() {
        use std::os::unix::fs::symlink;
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("checkout");
        let outside = sandbox.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        let roots = FileRoots::from_opened(vec![(root.clone(), File::open(&root).unwrap())]);
        let inside_stage = tempfile::tempdir_in(&root).unwrap();
        let outside_stage = tempfile::tempdir_in(&outside).unwrap();
        let moved = sandbox.path().join("moved");
        fs::rename(&root, &moved).unwrap();
        symlink(&outside, &root).unwrap();
        let moved_stage = moved.join(inside_stage.path().file_name().unwrap());
        assert!(stage_is_inside_root(&moved_stage, &roots).unwrap());
        assert!(!stage_is_inside_root(outside_stage.path(), &roots).unwrap());
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
