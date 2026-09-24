use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cap_std::fs::{Dir, OpenOptions as CapOpenOptions};
use hide_host::document::Document;
use hide_host::protocol::{Call, RevisionNow, RootOpened, RootRef};
use hide_host::save::Saved;
use hide_host::{ErrorCode, RootIdentity};

use crate::host_access::{HostCallError, HostChannel, call_as};
use crate::model::EditorDocumentSnapshot;

/// Opened checkout roots supplied by the daemon after its registration check.
/// The Swift shell uses the ambient path calls below; both shells share the
/// document and explorer logic, while the daemon's paths resolve through
/// these directory capabilities when the actual I/O runs.
type PinnedIdentity = (PathBuf, Option<(u64, u64)>);

#[derive(Clone, Debug, Default)]
pub struct FileRoots {
    roots: Arc<Vec<(PathBuf, Arc<Dir>)>>,
    identities: Arc<Vec<PinnedIdentity>>,
}

impl PartialEq for FileRoots {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.roots, &other.roots)
            || (self.identities.len() == other.identities.len()
                && self.identities.iter().zip(other.identities.iter()).all(
                    |((left_path, left_id), (right_path, right_id))| {
                        left_path == right_path && left_id.is_some() && left_id == right_id
                    },
                ))
    }
}

impl Eq for FileRoots {}

impl FileRoots {
    pub fn from_opened(roots: Vec<(PathBuf, File)>) -> Self {
        let mut opened = Vec::with_capacity(roots.len());
        let mut identities = Vec::with_capacity(roots.len());
        for (path, file) in roots {
            #[cfg(unix)]
            let identity = {
                use std::os::unix::fs::MetadataExt;
                file.metadata()
                    .ok()
                    .map(|metadata| (metadata.dev(), metadata.ino()))
            };
            #[cfg(not(unix))]
            let identity = None;
            identities.push((path.clone(), identity));
            opened.push((path, Arc::new(Dir::from_std_file(file))));
        }
        Self {
            roots: Arc::new(opened),
            identities: Arc::new(identities),
        }
    }

    /// The registered root that holds `path`, with the identity hided
    /// pinned when it opened it; the deepest one when roots nest.
    pub(crate) fn pinned_root(&self, path: &Path) -> Option<(PathBuf, RootIdentity)> {
        self.identities
            .iter()
            .filter(|(root, _)| path.starts_with(root))
            .max_by_key(|(root, _)| root.components().count())
            .and_then(|(root, identity)| {
                identity.map(|(device, inode)| (root.clone(), RootIdentity { device, inode }))
            })
    }

    fn relative<'a>(&'a self, path: &'a Path) -> io::Result<(&'a Dir, &'a Path)> {
        self.roots
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

    /// Narrow an already opened checkout to one registered subfolder. This
    /// open is resolved through the existing capability, not an ambient path.
    pub(crate) fn scoped(&self, path: &Path) -> io::Result<Self> {
        let file = self.open(path, false)?;
        if !file.metadata()?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "History scope is not a directory",
            ));
        }
        Ok(Self::from_opened(vec![(path.to_path_buf(), file)]))
    }

    /// A pathname may have been replaced after its directory capability was
    /// opened. Git still uses pathnames, so History must reject a different
    /// ambient directory before combining Git output with handle-based reads.
    pub(crate) fn matches_ambient_root(&self, path: &Path) -> bool {
        self.roots
            .iter()
            .find(|(root, _)| root == path)
            .and_then(|(_, dir)| dir.dir_metadata().ok())
            .zip(fs::metadata(path).ok())
            .is_some_and(|(opened, ambient)| same_directory_identity(&opened, &ambient))
    }

    #[cfg(unix)]
    pub(crate) fn opened_root_fd(&self, path: &Path) -> Option<std::os::fd::RawFd> {
        use std::os::fd::AsRawFd;
        self.roots
            .iter()
            .find(|(root, _)| root == path)
            .map(|(_, dir)| dir.as_raw_fd())
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

/// A checkout as document work reaches it: the device it is on, its root
/// path there, and the root's identity when hided pinned it at
/// registration. Without one, the open reads it and the document keeps it.
#[derive(Clone, Debug)]
pub struct DocumentRoot {
    pub device_id: String,
    pub path: String,
    pub identity: Option<RootIdentity>,
}

/// Where an open document lives. Every save and settle read names this
/// exact root, so a checkout replaced after the open refuses them instead
/// of writing into whatever took its path (PRD S5.5 B7, B13). The channel is
/// looked up by device each time, so a save after a reconnect goes to the
/// new helper connection.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentPlace {
    pub device_id: String,
    pub root: RootRef,
    pub relative: String,
}

#[derive(Debug)]
pub enum OpenFailure {
    /// The file is not there.
    Missing,
    Failed(String),
}

impl OpenFailure {
    pub fn message(&self) -> String {
        match self {
            Self::Missing => "The file no longer exists".to_owned(),
            Self::Failed(message) => message.clone(),
        }
    }
}

/// What one save came to. `Unknown` is the only answer that does not say
/// what is on disk: the caller settles it by reading the revision again and
/// never by sending the write a second time (B14).
#[derive(Debug)]
pub enum SaveOutcome {
    Saved(Saved),
    Conflict {
        disk_revision: Option<String>,
        message: String,
    },
    Refused(String),
    Unknown(String),
}

const OPEN_TIMEOUT: Duration = Duration::from_secs(30);
const SAVE_TIMEOUT: Duration = Duration::from_secs(60);
const REVISION_TIMEOUT: Duration = Duration::from_secs(30);

/// `absolute` spelled under `root`, as the host protocol names a path.
pub fn relative_under(root: &str, absolute: &str) -> Result<String, String> {
    let root = root.trim_end_matches('/');
    let rest = absolute
        .strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .filter(|rest| !rest.is_empty())
        .ok_or_else(|| "The file is not inside its checkout".to_owned())?;
    hide_host::relative_path(rest).map_err(|error| error.message)?;
    Ok(rest.to_owned())
}

/// `relative_under` for a file on this machine, which a shell may spell
/// through a link to the checkout's folder (`/var` for `/private/var`): the
/// folder holding the file is resolved, never the file itself, so a link
/// inside the checkout is still opened as the link.
fn local_relative(root: &str, absolute: &str) -> Result<String, String> {
    relative_under(root, absolute).or_else(|refused| {
        let absolute = Path::new(absolute);
        let resolved = absolute
            .parent()
            .and_then(|folder| folder.canonicalize().ok())
            .zip(absolute.file_name())
            .map(|(folder, name)| folder.join(name))
            .zip(Path::new(root).canonicalize().ok());
        match resolved {
            Some((absolute, root)) => {
                relative_under(&root.to_string_lossy(), &absolute.to_string_lossy())
            }
            None => Err(refused),
        }
    })
}

/// Opens the document at `absolute` in `root`, as a snapshot and the place
/// its saves go to. Blocks on the channel.
pub fn open_document(
    channel: &dyn HostChannel,
    root: &DocumentRoot,
    absolute: &str,
) -> Result<(EditorDocumentSnapshot, DocumentPlace), OpenFailure> {
    let relative = if channel.in_process() {
        local_relative(&root.path, absolute)
    } else {
        relative_under(&root.path, absolute)
    }
    .map_err(OpenFailure::Failed)?;
    let identity = match root.identity {
        Some(identity) => identity,
        None => {
            call_as::<RootOpened>(
                channel,
                Call::RootOpen {
                    root: root.path.clone(),
                },
                OPEN_TIMEOUT,
            )
            .map_err(open_failure)?
            .identity
        }
    };
    let place = DocumentPlace {
        device_id: root.device_id.clone(),
        root: RootRef {
            path: root.path.clone(),
            identity,
        },
        relative,
    };
    let document: Document = call_as(
        channel,
        Call::OpenDocument {
            root: place.root.clone(),
            path: place.relative.clone(),
        },
        OPEN_TIMEOUT,
    )
    .map_err(open_failure)?;
    Ok((document_snapshot(absolute, document), place))
}

fn open_failure(error: HostCallError) -> OpenFailure {
    match error {
        HostCallError::Refused(error) if error.code == ErrorCode::NotFound => OpenFailure::Missing,
        HostCallError::Refused(error) => {
            OpenFailure::Failed(format!("The file could not be opened: {}", error.message))
        }
        other => OpenFailure::Failed(format!("The file could not be opened: {other}")),
    }
}

fn document_snapshot(absolute: &str, document: Document) -> EditorDocumentSnapshot {
    EditorDocumentSnapshot {
        path: absolute.to_owned(),
        language: document.language,
        document_kind: document.kind,
        contents_utf8: document.contents,
        opened_modified_at_unix_ms: Some(document.modified_at_unix_ms),
        revision: document.revision,
        dirty: false,
        readonly_reason: document.readonly_reason,
        conflict: None,
        save: None,
    }
}

pub fn update_draft(editor: &mut EditorDocumentSnapshot, contents: String) -> Result<(), String> {
    check_editable(editor, "the draft was not changed")?;
    editor.dirty = editor.contents_utf8.as_deref() != Some(contents.as_str());
    editor.contents_utf8 = Some(contents);
    Ok(())
}

/// Refuses a document that takes no draft; `consequence` finishes the
/// sentence the operator reads.
pub fn check_editable(editor: &EditorDocumentSnapshot, consequence: &str) -> Result<(), String> {
    if !editor.document_kind.is_editable() {
        return Err(format!(
            "The current file is not a text document; {consequence}"
        ));
    }
    if editor.readonly_reason.is_some() {
        return Err(format!("The current file is read-only; {consequence}"));
    }
    if editor.revision.is_none() {
        return Err(format!(
            "The file's revision was never read, so a save cannot be checked; {consequence}"
        ));
    }
    Ok(())
}

/// Saves `contents` at `place` if the file there still holds `expected`.
/// Blocks on the channel.
pub fn save_document(
    channel: &dyn HostChannel,
    place: &DocumentPlace,
    contents: &str,
    expected: &str,
) -> SaveOutcome {
    match call_as::<Saved>(
        channel,
        Call::Save {
            root: place.root.clone(),
            path: place.relative.clone(),
            contents: contents.to_owned(),
            expected_revision: expected.to_owned(),
        },
        SAVE_TIMEOUT,
    ) {
        Ok(saved) => SaveOutcome::Saved(saved),
        Err(HostCallError::Refused(error)) if error.code == ErrorCode::Conflict => {
            SaveOutcome::Conflict {
                disk_revision: error.actual_revision,
                message: error.message,
            }
        }
        Err(HostCallError::Refused(error)) => SaveOutcome::Refused(error.message),
        Err(HostCallError::NotConnected(reason)) => SaveOutcome::Refused(format!(
            "{reason}; nothing was sent and the draft was preserved"
        )),
        Err(error @ HostCallError::Busy) => SaveOutcome::Refused(error.to_string()),
        Err(HostCallError::Unknown(reason)) => SaveOutcome::Unknown(reason),
    }
}

/// The file's revision now, or `None` when it is gone; read after any save
/// in its folder has finished. Blocks on the channel.
pub fn revision_now(
    channel: &dyn HostChannel,
    place: &DocumentPlace,
) -> Result<Option<String>, HostCallError> {
    match call_as::<RevisionNow>(
        channel,
        Call::Revision {
            root: place.root.clone(),
            path: place.relative.clone(),
        },
        REVISION_TIMEOUT,
    ) {
        Ok(now) => Ok(Some(now.revision)),
        Err(HostCallError::Refused(error)) if error.code == ErrorCode::NotFound => Ok(None),
        Err(error) => Err(error),
    }
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
    for (_, root) in roots.roots.iter() {
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::DocumentKind;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::UNIX_EPOCH;

    static NEXT_EXPLORER_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn draft_updates_keep_unsaved_contents_in_memory() {
        let mut editor = EditorDocumentSnapshot {
            path: "/tmp/existing.txt".to_owned(),
            language: Some("txt".to_owned()),
            document_kind: DocumentKind::Text,
            contents_utf8: Some("old".to_owned()),
            opened_modified_at_unix_ms: Some(1),
            revision: Some(hide_host::document::revision_of(b"old")),
            dirty: false,
            readonly_reason: None,
            conflict: None,
            save: None,
        };
        update_draft(&mut editor, "new".to_owned()).unwrap();
        assert!(editor.dirty);
        assert_eq!(editor.contents_utf8.as_deref(), Some("new"));
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

    /// Opens `path` on this machine the way the runtime does, with the
    /// parent folder as the checkout root.
    pub(crate) fn open_local(path: &Path) -> (EditorDocumentSnapshot, DocumentPlace) {
        let root = DocumentRoot {
            device_id: crate::workspace::LOCAL_DEVICE_ID.to_owned(),
            path: path.parent().unwrap().to_string_lossy().into_owned(),
            identity: None,
        };
        open_document(
            &crate::host_access::InProcessHost,
            &root,
            &path.to_string_lossy(),
        )
        .expect("fixture document")
    }

    /// A checkout whose path is replaced after a document opened refuses
    /// the save: it lands neither in the impostor nor, silently, in the
    /// moved original, and explorer work keeps using the pinned handle.
    #[cfg(unix)]
    #[test]
    fn a_checkout_replaced_after_the_open_refuses_the_save_and_writes_nowhere() {
        use std::os::unix::fs::symlink;
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("checkout");
        let outside = sandbox.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(root.join("note.txt"), "inside").unwrap();
        fs::write(outside.join("note.txt"), "outside").unwrap();
        let roots = FileRoots::from_opened(vec![(root.clone(), File::open(&root).unwrap())]);
        let (pinned_path, identity) = roots.pinned_root(&root.join("note.txt")).unwrap();
        let document_root = DocumentRoot {
            device_id: "local".to_owned(),
            path: pinned_path.to_string_lossy().into_owned(),
            identity: Some(identity),
        };
        let channel = crate::host_access::InProcessHost;
        let (document, place) = open_document(
            &channel,
            &document_root,
            &root.join("note.txt").to_string_lossy(),
        )
        .unwrap();
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
        match save_document(
            &channel,
            &place,
            "edited",
            document.revision.as_deref().unwrap(),
        ) {
            SaveOutcome::Refused(message) => assert!(message.contains("replaced"), "{message}"),
            other => panic!("a replaced checkout must refuse the save: {other:?}"),
        }
        apply_explorer_operation_with_roots(&create, Some(&roots)).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.path().join("moved/note.txt")).unwrap(),
            "inside"
        );
        assert!(sandbox.path().join("moved/created.txt").is_file());
        assert!(!outside.join("created.txt").exists());
        assert_eq!(
            fs::read_to_string(outside.join("note.txt")).unwrap(),
            "outside"
        );
    }

    #[test]
    fn a_path_outside_the_checkout_is_refused_before_anything_is_sent() {
        for (root, path) in [
            ("/repo", "/repository/a.txt"),
            ("/repo", "/repo"),
            ("/repo", "/repo/../etc/passwd"),
            ("/repo/", "/other/a.txt"),
        ] {
            assert!(relative_under(root, path).is_err(), "{root} {path}");
        }
        assert_eq!(
            relative_under("/repo/", "/repo/src/a.rs").unwrap(),
            "src/a.rs"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_local_file_spelled_through_a_link_to_its_checkout_opens_under_that_spelling() {
        use std::os::unix::fs::symlink;
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().canonicalize().unwrap().join("checkout");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.txt"), "a").unwrap();
        symlink(&root, sandbox.path().join("alias")).unwrap();
        let spelled = sandbox.path().join("alias/src/a.txt");
        let root_path = DocumentRoot {
            device_id: crate::workspace::LOCAL_DEVICE_ID.to_owned(),
            path: root.to_string_lossy().into_owned(),
            identity: None,
        };
        let (document, place) = open_document(
            &crate::host_access::InProcessHost,
            &root_path,
            &spelled.to_string_lossy(),
        )
        .unwrap();
        assert_eq!(document.path, spelled.to_string_lossy());
        assert_eq!(place.relative, "src/a.txt");
        let outside = sandbox.path().join("alias/../elsewhere.txt");
        fs::write(sandbox.path().join("elsewhere.txt"), "b").unwrap();
        assert!(
            open_document(
                &crate::host_access::InProcessHost,
                &root_path,
                &outside.to_string_lossy(),
            )
            .is_err()
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
