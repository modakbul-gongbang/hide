use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cap_std::fs::Dir;
use hide_host::document::Document;
use hide_host::protocol::{Call, RevisionNow, RootRef};
use hide_host::save::Saved;
use hide_host::{ErrorCode, RootIdentity};
use hide_platform::fs::identity;
use hide_platform::path::{self, PathError, RelPath};

use crate::host_access::{HostCallError, HostChannel, call_as};
use crate::model::EditorDocumentSnapshot;

/// Opened checkout roots supplied by the daemon after its registration check.
/// Each root's identity pins the folder every host request names; the opened
/// handle is held so that identity cannot be reused by another folder while
/// the daemon runs. A client that supplies none has its requests pin the
/// root when they first open it.
type PinnedIdentity = (PathBuf, Option<RootIdentity>);

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
            let dir = Dir::from_std_file(file);
            let identity = hide_host::root::identity_of(&dir).ok();
            identities.push((path.clone(), identity));
            opened.push((path, Arc::new(dir)));
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
            .and_then(|(root, identity)| identity.map(|identity| (root.clone(), identity)))
    }
}

/// A checkout as document work reaches it: the device it is on, its root
/// path there, and the root's identity when hided pinned it at
/// registration. Without one, the open reads it and the document keeps it.
#[derive(Clone, Debug, PartialEq)]
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
    pub relative: RelPath,
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

/// `absolute` below `root`, both one device's paths in the wire spelling
/// (`hide_platform::path`), as the host protocol names it. By names alone,
/// so it holds for a device on any system; the root itself is not a file.
pub fn relative_under(root: &str, absolute: &str) -> Result<RelPath, String> {
    path::wire_relative(root, absolute)
        .ok()
        .filter(|relative| !relative.is_root())
        .ok_or_else(|| "The file is not inside its checkout".to_owned())
}

/// `relative_under` for a file on this machine, which a shell may spell
/// through a link to the checkout's folder (`/var` for `/private/var`) or
/// in another case on a volume that ignores it: the folder holding the
/// file is resolved, never the file itself, so a link inside the checkout
/// is still opened as the link.
fn local_relative(root: &str, absolute: &str) -> Result<RelPath, String> {
    relative_under(root, absolute).or_else(|refused| {
        let (Ok(root), Ok(absolute)) = (path::from_wire(root), path::from_wire(absolute)) else {
            return Err(refused);
        };
        let resolved = absolute
            .parent()
            .and_then(|folder| identity::canonical(folder).ok())
            .zip(absolute.file_name())
            .map(|(folder, name)| folder.join(name))
            .zip(identity::canonical(&root).ok());
        match resolved {
            Some((absolute, root)) => path::relative(&root, &absolute)
                .ok()
                .filter(|relative| !relative.is_root())
                .ok_or(refused),
            None => Err(refused),
        }
    })
}

/// The host's checkout-relative path rule, including macOS root aliases
/// such as `/var` and `/private/var` for local reads.
pub fn relative_in_root(
    channel: &dyn HostChannel,
    root: &str,
    absolute: &str,
) -> Result<RelPath, String> {
    if channel.in_process() {
        local_relative(root, absolute)
    } else {
        relative_under(root, absolute)
    }
}

/// Opens the document at `absolute` in `root`, as a snapshot and the place
/// its saves go to. Blocks on the channel.
pub fn open_document(
    channel: &dyn HostChannel,
    root: &DocumentRoot,
    absolute: &str,
) -> Result<(EditorDocumentSnapshot, DocumentPlace), OpenFailure> {
    let relative = relative_in_root(channel, &root.path, absolute).map_err(OpenFailure::Failed)?;
    let place = DocumentPlace {
        device_id: root.device_id.clone(),
        root: root_ref(channel, root).map_err(open_failure)?,
        relative,
    };
    let document: Result<Document, _> = call_as(
        channel,
        Call::OpenDocument {
            root: place.root.clone(),
            path: place.relative.to_string(),
        },
        OPEN_TIMEOUT,
    );
    // An open is an explicit read, like a listing: a replaced root is
    // refused once and unpinned, so the operator's next open adopts the
    // folder now at that path.
    if let Err(HostCallError::Refused(error)) = &document
        && error.code == ErrorCode::RootReplaced
        && root.identity.is_none()
    {
        channel.pin(&root.path, None);
    }
    let document = document.map_err(open_failure)?;
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
            path: place.relative.to_string(),
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
            path: place.relative.to_string(),
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
/// moved into itself - is refused here from the paths alone. The paths are
/// the checkout device's, in the wire spelling, and are related by their
/// names alone, so a device on any system is judged the same way.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplorerOperation {
    pub kind: ExplorerOperationKind,
    /// The item the change starts from: the path a new item takes, or the
    /// current path of the item being renamed or moved.
    pub source: String,
    /// Where the item is once the change has landed. Equal to `source` for
    /// a creation, and for a trash, whose item has no path here afterwards.
    pub destination: String,
    /// The row the tree selects once the change has landed: the item itself
    /// for a creation, rename or move, and the tree's chosen neighbour for
    /// a trash, whose item is no longer there to select.
    pub selection: String,
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
        root: &str,
        parent: &str,
        name: &str,
    ) -> Result<Self, String> {
        if !matches!(
            kind,
            ExplorerOperationKind::FileCreate | ExplorerOperationKind::DirCreate
        ) {
            return Err(format!("{} does not create an item", kind.as_str()));
        }
        let parent = path_inside_root(root, parent, true)?;
        let path = path::wire_join(root, &item_below(&parent, name)?);
        Ok(Self {
            kind,
            source: path.clone(),
            destination: path.clone(),
            selection: path,
            expected_inode: None,
        })
    }

    pub fn rename(root: &str, path: &str, name: &str) -> Result<Self, String> {
        let source = path_inside_root(root, path, false)?;
        let parent = source
            .parent()
            .ok_or_else(|| "The item has no parent folder".to_owned())?;
        let destination = item_below(&parent, name)?;
        if destination == source {
            return Err("The name is unchanged".to_owned());
        }
        let destination = path::wire_join(root, &destination);
        Ok(Self {
            kind: ExplorerOperationKind::PathRename,
            source: path::wire_join(root, &source),
            destination: destination.clone(),
            selection: destination,
            expected_inode: None,
        })
    }

    pub fn move_into(root: &str, path: &str, destination_dir: &str) -> Result<Self, String> {
        let source = path_inside_root(root, path, false)?;
        let destination_dir = path_inside_root(root, destination_dir, true)?;
        let name = source
            .file_name()
            .ok_or_else(|| "The item has no name".to_owned())?;
        if destination_dir.starts_with(&source) {
            return Err("A folder cannot be moved into itself".to_owned());
        }
        let destination = item_below(&destination_dir, name)?;
        if destination == source {
            return Err("The item is already in that folder".to_owned());
        }
        let destination = path::wire_join(root, &destination);
        Ok(Self {
            kind: ExplorerOperationKind::PathMove,
            source: path::wire_join(root, &source),
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
        root: &str,
        path: &str,
        select_after: &str,
        expected_inode: Option<u64>,
    ) -> Result<Self, String> {
        let source = path_inside_root(root, path, false)?;
        let selection = path_inside_root(root, select_after, true)?;
        if selection.starts_with(&source) {
            return Err(
                "The selection cannot move into the item being moved to the Trash".to_owned(),
            );
        }
        let source = path::wire_join(root, &source);
        Ok(Self {
            kind: ExplorerOperationKind::PathTrash,
            source: source.clone(),
            destination: source,
            selection: path::wire_join(root, &selection),
            expected_inode,
        })
    }
}

const CHANGE_TIMEOUT: Duration = Duration::from_secs(60);

/// `absolute` below `root` as the host protocol spells it, the root itself
/// as the empty path.
fn relative_or_root(root: &str, absolute: &str) -> Result<String, String> {
    path::wire_relative(root, absolute)
        .map(RelPath::into_string)
        .map_err(|_| "The file is not inside its checkout".to_owned())
}

/// The root as the checkout's host requests name it: the identity hided
/// pinned, or the one the channel pinned when it first touched the root, so
/// a device folder replaced after it was listed is refused rather than
/// opened or changed in its place.
pub(crate) fn root_ref(
    channel: &dyn HostChannel,
    root: &DocumentRoot,
) -> Result<RootRef, HostCallError> {
    match root.identity {
        Some(identity) => Ok(RootRef {
            path: root.path.clone(),
            identity,
        }),
        None => crate::host_access::pinned_root(channel, &root.path, OPEN_TIMEOUT),
    }
}

/// Runs the change on the machine that holds the checkout, through the same
/// `hide_host` request on this machine as on a device: the host confines it
/// to the opened root, never replaces an existing item, and moves an item to
/// its own Trash or leaves it (PRD S5.5 B16-B18). Blocks on the channel; a
/// request whose answer was lost is reported as an unknown result, which the
/// tree settles by reading the folder again.
pub fn apply_explorer_operation(
    channel: &dyn HostChannel,
    root: &DocumentRoot,
    operation: &ExplorerOperation,
) -> Result<(), String> {
    let root_ref = root_ref(channel, root).map_err(|error| change_failure(operation, error))?;
    let source = relative_or_root(&root.path, &operation.source)?;
    let destination = RelPath::parse(&relative_or_root(&root.path, &operation.destination)?)
        .map_err(|error| error.to_string())?;
    let call = match operation.kind {
        ExplorerOperationKind::FileCreate | ExplorerOperationKind::DirCreate => {
            let created = RelPath::parse(&source).map_err(|error| error.to_string())?;
            Call::Create {
                root: root_ref,
                parent: created
                    .parent()
                    .ok_or_else(|| "The item has no parent folder".to_owned())?
                    .into_string(),
                name: file_name(&created)?,
                directory: operation.kind == ExplorerOperationKind::DirCreate,
            }
        }
        ExplorerOperationKind::PathRename => Call::Rename {
            root: root_ref,
            path: source,
            name: file_name(&destination)?,
        },
        ExplorerOperationKind::PathMove => Call::Move {
            root: root_ref,
            path: source,
            destination: destination
                .parent()
                .ok_or_else(|| "The destination has no folder".to_owned())?
                .into_string(),
        },
        ExplorerOperationKind::PathTrash => Call::Trash {
            root: root_ref,
            path: source,
            inode: operation.expected_inode,
        },
    };
    call_as::<hide_host::mutate::Changed>(channel, call, CHANGE_TIMEOUT)
        .map(drop)
        .map_err(|error| change_failure(operation, error))
}

fn file_name(path: &RelPath) -> Result<String, String> {
    path.file_name()
        .map(str::to_owned)
        .ok_or_else(|| "The checkout root has no name".to_owned())
}

fn change_failure(operation: &ExplorerOperation, error: HostCallError) -> String {
    match error {
        HostCallError::Refused(error) => error.message,
        HostCallError::NotConnected(reason) => format!("{reason}; nothing was changed"),
        error @ HostCallError::Busy => error.to_string(),
        HostCallError::Unknown(reason) => format!(
            "{reason}; whether {} changed is unknown, so the folder is read again",
            operation.source
        ),
    }
}

/// `path` below `root`, both wire spellings of the checkout device's paths,
/// when it is absolute, normal, and inside `root`. By names alone, so
/// `/repo-other` is outside `/repo`, and lexical, so a symlink that escapes
/// the root is not followed here and cannot be created here either.
pub(crate) fn path_inside_root(
    root: &str,
    path: &str,
    allow_root: bool,
) -> Result<RelPath, String> {
    if !path::is_wire_absolute(root) {
        return Err("The workspace root is not an absolute path".to_owned());
    }
    if !path::is_wire_absolute(path) {
        return Err(format!("{path} is not an absolute path"));
    }
    let relative = path::wire_relative(root, path).map_err(|error| match error {
        PathError::NotNormal => format!("{path} is not a normal path"),
        _ => format!("{path} is outside the workspace {root}"),
    })?;
    if !allow_root && relative.is_root() {
        return Err("The workspace root itself cannot be changed".to_owned());
    }
    Ok(relative)
}

/// `name` as one item below `parent`. The wire's own rule: one name, no
/// `/`, no NUL, not `.` or `..`; whether the device's system can hold the
/// name is the device's host's to judge when it makes the item.
fn item_below(parent: &RelPath, name: &str) -> Result<RelPath, String> {
    if name.is_empty() {
        return Err("A name is required".to_owned());
    }
    if name.contains('/') {
        return Err("A name cannot contain /".to_owned());
    }
    if name.contains('\0') {
        return Err("A name cannot contain NUL".to_owned());
    }
    parent
        .join(name)
        .map_err(|_| format!("{name} is not a valid name"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::DocumentKind;
    use std::fs;
    use std::time::UNIX_EPOCH;

    #[test]
    fn draft_updates_keep_unsaved_contents_in_memory() {
        let mut editor = EditorDocumentSnapshot {
            path: "/tmp/existing.txt".to_owned(),
            language: Some("txt".to_owned()),
            document_kind: DocumentKind::Text,
            contents_utf8: Some("old".to_owned()),
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
    /// moved original, and an explorer change is refused the same way.
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
        let wire_root = root.to_string_lossy();
        let create = ExplorerOperation::create(
            ExplorerOperationKind::FileCreate,
            &wire_root,
            &wire_root,
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
        let refused = apply_explorer_operation(&channel, &document_root, &create).unwrap_err();
        assert!(refused.contains("replaced"), "{refused}");
        assert_eq!(
            fs::read_to_string(sandbox.path().join("moved/note.txt")).unwrap(),
            "inside"
        );
        assert!(!sandbox.path().join("moved/created.txt").exists());
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
            relative_under("/repo/", "/repo/src/a.rs").unwrap().as_str(),
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
        assert_eq!(place.relative.as_str(), "src/a.txt");
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

    /// Records each request and answers every one, so a test reads exactly
    /// what a checkout's host is asked to do.
    #[derive(Default)]
    struct RecordingHost {
        calls: std::sync::Mutex<Vec<Call>>,
    }

    impl HostChannel for RecordingHost {
        fn call(
            &self,
            call: Call,
            _timeout: Duration,
        ) -> Result<crate::host_access::HostAnswer, HostCallError> {
            self.calls.lock().unwrap().push(call);
            Ok(serde_json::json!({}).into())
        }
    }

    #[test]
    fn a_change_asks_the_checkout_host_with_paths_under_its_pinned_root() {
        let root = "/repo";
        let identity = RootIdentity {
            device: 1,
            inode: 2,
        };
        let document_root = DocumentRoot {
            device_id: "macbook".to_owned(),
            path: "/repo".to_owned(),
            identity: Some(identity),
        };
        let pinned = RootRef {
            path: "/repo".to_owned(),
            identity,
        };
        let host = RecordingHost::default();
        let operations = [
            ExplorerOperation::create(ExplorerOperationKind::DirCreate, root, root, "new").unwrap(),
            ExplorerOperation::create(ExplorerOperationKind::FileCreate, root, "/repo/src", "a.rs")
                .unwrap(),
            ExplorerOperation::rename(root, "/repo/src/a.rs", "b.rs").unwrap(),
            ExplorerOperation::move_into(root, "/repo/src/b.rs", root).unwrap(),
            ExplorerOperation::trash(root, "/repo/src", root, Some(7)).unwrap(),
        ];
        for operation in &operations {
            apply_explorer_operation(&host, &document_root, operation).unwrap();
        }
        assert_eq!(
            *host.calls.lock().unwrap(),
            vec![
                Call::Create {
                    root: pinned.clone(),
                    parent: String::new(),
                    name: "new".to_owned(),
                    directory: true
                },
                Call::Create {
                    root: pinned.clone(),
                    parent: "src".to_owned(),
                    name: "a.rs".to_owned(),
                    directory: false
                },
                Call::Rename {
                    root: pinned.clone(),
                    path: "src/a.rs".to_owned(),
                    name: "b.rs".to_owned()
                },
                Call::Move {
                    root: pinned.clone(),
                    path: "src/b.rs".to_owned(),
                    destination: String::new()
                },
                Call::Trash {
                    root: pinned,
                    path: "src".to_owned(),
                    inode: Some(7)
                },
            ]
        );
    }

    #[test]
    fn explorer_refuses_to_trash_outside_the_root_or_the_root_itself() {
        let root = "/repo";
        let outside = ExplorerOperation::trash(root, "/repo-other/a", root, None);
        assert!(outside.unwrap_err().contains("outside the workspace"));
        let escaping = ExplorerOperation::trash(root, "/repo/../etc/passwd", root, None);
        assert!(escaping.unwrap_err().contains("not a normal path"));
        let root_itself = ExplorerOperation::trash(root, root, root, None);
        assert!(root_itself.unwrap_err().contains("root itself"));
        let relative = ExplorerOperation::trash(root, "src/a", root, None);
        assert!(relative.unwrap_err().contains("not an absolute path"));

        let selection_outside = ExplorerOperation::trash(root, "/repo/src", "/repo-other", None);
        assert!(
            selection_outside
                .unwrap_err()
                .contains("outside the workspace")
        );
        let selection_inside = ExplorerOperation::trash(root, "/repo/src", "/repo/src/a.rs", None);
        assert!(
            selection_inside
                .unwrap_err()
                .contains("cannot move into the item")
        );
        let selection_itself = ExplorerOperation::trash(root, "/repo/src", "/repo/src", None);
        assert!(
            selection_itself
                .unwrap_err()
                .contains("cannot move into the item")
        );
    }

    #[test]
    fn explorer_refuses_paths_outside_the_root_and_invalid_names() {
        let root = "/repo";
        let outside =
            ExplorerOperation::create(ExplorerOperationKind::FileCreate, root, "/repo-other", "a");
        assert!(outside.unwrap_err().contains("outside the workspace"));
        let escaping = ExplorerOperation::rename(root, "/repo/../etc/passwd", "x");
        assert!(escaping.unwrap_err().contains("not a normal path"));
        let root_itself = ExplorerOperation::rename(root, root, "x");
        assert!(root_itself.unwrap_err().contains("root itself"));
        let relative = ExplorerOperation::move_into(root, "src/a", root);
        assert!(relative.unwrap_err().contains("not an absolute path"));

        let parent = "/repo/src";
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
        let root = "/repo";
        let same_parent = ExplorerOperation::move_into(root, "/repo/src/a.rs", "/repo/src");
        assert!(same_parent.unwrap_err().contains("already in that folder"));
        let into_self = ExplorerOperation::move_into(root, "/repo/src", "/repo/src");
        assert!(into_self.unwrap_err().contains("into itself"));
        let into_child = ExplorerOperation::move_into(root, "/repo/src", "/repo/src/nested");
        assert!(into_child.unwrap_err().contains("into itself"));
        let unchanged = ExplorerOperation::rename(root, "/repo/src/a.rs", "a.rs");
        assert!(unchanged.unwrap_err().contains("unchanged"));
    }
}
