use std::collections::BTreeSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub kind: FileKind,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileRevision {
    pub token: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileDocument {
    pub path: String,
    pub content: String,
    pub revision: FileRevision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SaveResult {
    pub document: FileDocument,
    pub replaced_revision: FileRevision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchMatch {
    pub path: String,
    pub line: Option<u64>,
    pub preview: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiffResult {
    pub path: String,
    pub changed: bool,
    pub unified: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GitStatusEntry {
    pub code: String,
    pub path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileServiceError {
    InvalidPath {
        path: String,
        reason: String,
    },
    Io {
        operation: String,
        path: String,
        reason: String,
    },
    Encoding {
        path: String,
    },
    RevisionConflict {
        path: String,
        expected: FileRevision,
        actual: FileRevision,
    },
    Remote {
        operation: String,
        target: String,
        reason: String,
    },
    Command {
        operation: String,
        reason: String,
    },
}

impl fmt::Display for FileServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath { path, reason } => {
                write!(formatter, "invalid file path {path:?}: {reason}")
            }
            Self::Io {
                operation,
                path,
                reason,
            } => write!(formatter, "file {operation} failed for {path:?}: {reason}"),
            Self::Encoding { path } => write!(formatter, "file {path:?} is not valid UTF-8"),
            Self::RevisionConflict {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "file save conflict for {path:?}: expected revision {}, actual {}",
                expected.token, actual.token
            ),
            Self::Remote {
                operation,
                target,
                reason,
            } => write!(
                formatter,
                "remote SFTP {operation} failed for {target:?}: {reason}"
            ),
            Self::Command { operation, reason } => {
                write!(formatter, "command {operation} failed: {reason}")
            }
        }
    }
}

impl std::error::Error for FileServiceError {}

pub type FileResult<T> = Result<T, FileServiceError>;

/// The common editor/file contract shared by local filesystem and remote SFTP.
///
/// Mutation intentionally contains only revision-checked content save. There are no
/// create, move, rename, or delete methods, so a caller cannot accidentally broaden the
/// product's file scope through this abstraction.
pub trait FileService {
    fn list(&self, path: &str) -> FileResult<Vec<FileEntry>>;
    fn search_filename(&self, query: &str) -> FileResult<Vec<SearchMatch>>;
    fn search_content(&self, query: &str) -> FileResult<Vec<SearchMatch>>;
    fn open(&self, path: &str) -> FileResult<FileDocument>;
    fn save(&self, path: &str, content: &str, expected: &FileRevision) -> FileResult<SaveResult>;
    fn diff(&self, path: &str, original: &str) -> FileResult<DiffResult>;
    fn git_status(&self) -> FileResult<Vec<GitStatusEntry>>;
}

#[derive(Clone, Debug)]
pub struct LocalFileService {
    root: PathBuf,
}

impl LocalFileService {
    pub fn new(root: impl Into<PathBuf>) -> FileResult<Self> {
        let root = root.into();
        let metadata = std::fs::metadata(&root).map_err(|error| io_error("root", &root, error))?;
        if !metadata.is_dir() {
            return Err(FileServiceError::InvalidPath {
                path: root.display().to_string(),
                reason: "root is not a directory".to_owned(),
            });
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve(&self, path: &str) -> FileResult<PathBuf> {
        let relative = validate_relative_path(path)?;
        Ok(self.root.join(relative))
    }

    fn display_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/")
    }

    fn walk(&self, directory: &Path, output: &mut Vec<FileEntry>) -> FileResult<()> {
        let entries =
            std::fs::read_dir(directory).map_err(|error| io_error("list", directory, error))?;
        for entry in entries {
            let entry = entry.map_err(|error| io_error("list", directory, error))?;
            let path = entry.path();
            let metadata =
                std::fs::symlink_metadata(&path).map_err(|error| io_error("stat", &path, error))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            let kind = if metadata.is_dir() {
                FileKind::Directory
            } else if metadata.is_file() {
                FileKind::File
            } else {
                continue;
            };
            output.push(FileEntry {
                path: self.display_path(&path),
                name: entry.file_name().to_string_lossy().into_owned(),
                kind,
                size_bytes: metadata.len(),
            });
            if kind == FileKind::Directory {
                self.walk(&path, output)?;
            }
        }
        Ok(())
    }

    fn read_bytes(&self, path: &str) -> FileResult<Vec<u8>> {
        let absolute = self.resolve(path)?;
        std::fs::read(&absolute).map_err(|error| io_error("read", &absolute, error))
    }
}

impl FileService for LocalFileService {
    fn list(&self, path: &str) -> FileResult<Vec<FileEntry>> {
        let absolute = self.resolve(path)?;
        let mut entries = Vec::new();
        for entry in
            std::fs::read_dir(&absolute).map_err(|error| io_error("list", &absolute, error))?
        {
            let entry = entry.map_err(|error| io_error("list", &absolute, error))?;
            let path = entry.path();
            let metadata =
                std::fs::symlink_metadata(&path).map_err(|error| io_error("stat", &path, error))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            let kind = if metadata.is_dir() {
                FileKind::Directory
            } else if metadata.is_file() {
                FileKind::File
            } else {
                continue;
            };
            entries.push(FileEntry {
                path: self.display_path(&path),
                name: entry.file_name().to_string_lossy().into_owned(),
                kind,
                size_bytes: metadata.len(),
            });
        }
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(entries)
    }

    fn search_filename(&self, query: &str) -> FileResult<Vec<SearchMatch>> {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        self.walk(&self.root, &mut entries)?;
        Ok(entries
            .into_iter()
            .filter(|entry| entry.name.to_ascii_lowercase().contains(&query))
            .map(|entry| SearchMatch {
                path: entry.path,
                line: None,
                preview: entry.name,
            })
            .collect())
    }

    fn search_content(&self, query: &str) -> FileResult<Vec<SearchMatch>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        self.walk(&self.root, &mut entries)?;
        let mut matches = Vec::new();
        for entry in entries
            .into_iter()
            .filter(|entry| entry.kind == FileKind::File)
        {
            // Search is intentionally bounded so a large binary or generated artifact cannot
            // consume the UI process. Opening a file still has the same UTF-8 contract.
            if entry.size_bytes > 4 * 1024 * 1024 {
                continue;
            }
            let bytes = self.read_bytes(&entry.path)?;
            let Ok(content) = std::str::from_utf8(&bytes) else {
                continue;
            };
            for (index, line) in content.lines().enumerate() {
                if line.contains(query) {
                    matches.push(SearchMatch {
                        path: entry.path.clone(),
                        line: Some(index as u64 + 1),
                        preview: line.chars().take(160).collect(),
                    });
                }
            }
        }
        Ok(matches)
    }

    fn open(&self, path: &str) -> FileResult<FileDocument> {
        let bytes = self.read_bytes(path)?;
        let content = String::from_utf8(bytes.clone()).map_err(|_| FileServiceError::Encoding {
            path: path.to_owned(),
        })?;
        Ok(FileDocument {
            path: path.to_owned(),
            content,
            revision: revision(&bytes),
        })
    }

    fn save(&self, path: &str, content: &str, expected: &FileRevision) -> FileResult<SaveResult> {
        let absolute = self.resolve(path)?;
        let current_bytes =
            std::fs::read(&absolute).map_err(|error| io_error("revision", &absolute, error))?;
        let actual = revision(&current_bytes);
        if &actual != expected {
            return Err(FileServiceError::RevisionConflict {
                path: path.to_owned(),
                expected: expected.clone(),
                actual,
            });
        }
        let temporary = absolute.with_file_name(format!(
            ".herdr-save-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        std::fs::write(&temporary, content.as_bytes())
            .map_err(|error| io_error("write", &temporary, error))?;
        if let Err(error) = std::fs::rename(&temporary, &absolute) {
            let _ = std::fs::remove_file(&temporary);
            return Err(io_error("publish", &absolute, error));
        }
        let document = FileDocument {
            path: path.to_owned(),
            content: content.to_owned(),
            revision: revision(content.as_bytes()),
        };
        Ok(SaveResult {
            document,
            replaced_revision: expected.clone(),
        })
    }

    fn diff(&self, path: &str, original: &str) -> FileResult<DiffResult> {
        let document = self.open(path)?;
        Ok(diff_text(path, original, &document.content))
    }

    fn git_status(&self) -> FileResult<Vec<GitStatusEntry>> {
        let output = Command::new("git")
            .args([
                "-C",
                self.root.to_string_lossy().as_ref(),
                "status",
                "--short",
            ])
            .output()
            .map_err(|error| FileServiceError::Command {
                operation: "git status".to_owned(),
                reason: error.to_string(),
            })?;
        if !output.status.success() {
            return Err(FileServiceError::Command {
                operation: "git status".to_owned(),
                reason: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(parse_git_status(&String::from_utf8_lossy(&output.stdout)))
    }
}

/// Transport boundary for remote SFTP. Tests can supply a deterministic fake without
/// starting an SSH process, while production uses `remote::RusshSftpTransport`.
pub trait SftpTransport {
    fn list(&self, path: &str) -> FileResult<Vec<FileEntry>>;
    fn read(&self, path: &str) -> FileResult<Vec<u8>>;
    fn write(&self, path: &str, bytes: &[u8]) -> FileResult<()>;
    fn git_status(&self, root: &str) -> FileResult<String>;
}

#[derive(Clone, Debug)]
pub struct RemoteFileService<T> {
    root: String,
    transport: T,
}

impl<T> RemoteFileService<T> {
    pub fn new(root: impl Into<String>, transport: T) -> FileResult<Self> {
        let root = root.into();
        if root.trim().is_empty() || !root.starts_with('/') {
            return Err(FileServiceError::InvalidPath {
                path: root,
                reason: "remote root must be an absolute path".to_owned(),
            });
        }
        Ok(Self { root, transport })
    }

    fn resolve(&self, path: &str) -> FileResult<String> {
        let relative = validate_relative_path(path)?;
        let relative = relative
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        if relative.is_empty() {
            Ok(self.root.clone())
        } else {
            Ok(format!("{}/{}", self.root.trim_end_matches('/'), relative))
        }
    }

    fn document_from_bytes(&self, path: &str, bytes: Vec<u8>) -> FileResult<FileDocument> {
        let content = String::from_utf8(bytes.clone()).map_err(|_| FileServiceError::Encoding {
            path: path.to_owned(),
        })?;
        Ok(FileDocument {
            path: path.to_owned(),
            content,
            revision: revision(&bytes),
        })
    }
}

impl<T: SftpTransport> FileService for RemoteFileService<T> {
    fn list(&self, path: &str) -> FileResult<Vec<FileEntry>> {
        let target = self.resolve(path)?;
        self.transport.list(&target)
    }

    fn search_filename(&self, query: &str) -> FileResult<Vec<SearchMatch>> {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let mut pending = vec![self.root.clone()];
        let mut matches = Vec::new();
        while let Some(directory) = pending.pop() {
            for entry in self.transport.list(&directory)? {
                if entry.name.to_ascii_lowercase().contains(&query) {
                    matches.push(SearchMatch {
                        path: entry.path.clone(),
                        line: None,
                        preview: entry.name.clone(),
                    });
                }
                if entry.kind == FileKind::Directory {
                    pending.push(entry.path);
                }
            }
        }
        matches.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(matches)
    }

    fn search_content(&self, query: &str) -> FileResult<Vec<SearchMatch>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let mut pending = vec![self.root.clone()];
        let mut matches = Vec::new();
        while let Some(directory) = pending.pop() {
            for entry in self.transport.list(&directory)? {
                if entry.kind == FileKind::Directory {
                    pending.push(entry.path);
                    continue;
                }
                let bytes = self.transport.read(&entry.path)?;
                if bytes.len() > 4 * 1024 * 1024 {
                    continue;
                }
                let Ok(content) = std::str::from_utf8(&bytes) else {
                    continue;
                };
                for (index, line) in content.lines().enumerate() {
                    if line.contains(query) {
                        matches.push(SearchMatch {
                            path: entry.path.clone(),
                            line: Some(index as u64 + 1),
                            preview: line.chars().take(160).collect(),
                        });
                    }
                }
            }
        }
        matches.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(matches)
    }

    fn open(&self, path: &str) -> FileResult<FileDocument> {
        let target = self.resolve(path)?;
        self.document_from_bytes(path, self.transport.read(&target)?)
    }

    fn save(&self, path: &str, content: &str, expected: &FileRevision) -> FileResult<SaveResult> {
        let target = self.resolve(path)?;
        let current = self.transport.read(&target)?;
        let actual = revision(&current);
        if &actual != expected {
            return Err(FileServiceError::RevisionConflict {
                path: path.to_owned(),
                expected: expected.clone(),
                actual,
            });
        }
        self.transport.write(&target, content.as_bytes())?;
        Ok(SaveResult {
            document: self.document_from_bytes(path, content.as_bytes().to_vec())?,
            replaced_revision: expected.clone(),
        })
    }

    fn diff(&self, path: &str, original: &str) -> FileResult<DiffResult> {
        let document = self.open(path)?;
        Ok(diff_text(path, original, &document.content))
    }

    fn git_status(&self) -> FileResult<Vec<GitStatusEntry>> {
        Ok(parse_git_status(&self.transport.git_status(&self.root)?))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileTreePreferences {
    pub schema_version: u32,
    pub expanded: BTreeSet<String>,
}

impl Default for FileTreePreferences {
    fn default() -> Self {
        Self {
            schema_version: FILE_TREE_SCHEMA_VERSION,
            expanded: BTreeSet::new(),
        }
    }
}

pub const FILE_TREE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct FileTreeState {
    path: PathBuf,
    preferences: FileTreePreferences,
}

impl FileTreeState {
    pub fn load_or_default(path: impl Into<PathBuf>) -> FileResult<Self> {
        let path = path.into();
        let preferences = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| FileServiceError::Io {
                operation: "decode-tree-state".to_owned(),
                path: path.display().to_string(),
                reason: error.to_string(),
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => FileTreePreferences {
                ..FileTreePreferences::default()
            },
            Err(error) => return Err(io_error("read-tree-state", &path, error)),
        };
        if preferences.schema_version != FILE_TREE_SCHEMA_VERSION {
            return Err(FileServiceError::Io {
                operation: "decode-tree-state".to_owned(),
                path: path.display().to_string(),
                reason: format!("unsupported schema {}", preferences.schema_version),
            });
        }
        Ok(Self { path, preferences })
    }

    pub fn preferences(&self) -> &FileTreePreferences {
        &self.preferences
    }

    pub fn set_expanded(&mut self, path: impl Into<String>, expanded: bool) {
        let path = path.into();
        if expanded {
            self.preferences.expanded.insert(path);
        } else {
            self.preferences.expanded.remove(&path);
        }
    }

    pub fn save(&self) -> FileResult<()> {
        let parent = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        if let Some(parent) = parent {
            std::fs::create_dir_all(parent)
                .map_err(|error| io_error("mkdir-tree-state", parent, error))?;
        }
        let temporary = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        let bytes =
            serde_json::to_vec_pretty(&self.preferences).map_err(|error| FileServiceError::Io {
                operation: "encode-tree-state".to_owned(),
                path: self.path.display().to_string(),
                reason: error.to_string(),
            })?;
        std::fs::write(&temporary, bytes)
            .map_err(|error| io_error("write-tree-state", &temporary, error))?;
        if let Err(error) = std::fs::rename(&temporary, &self.path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(io_error("publish-tree-state", &self.path, error));
        }
        Ok(())
    }
}

fn validate_relative_path(path: &str) -> FileResult<PathBuf> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Err(FileServiceError::InvalidPath {
            path: path.display().to_string(),
            reason: "path must be relative to the service root".to_owned(),
        });
    }
    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(FileServiceError::InvalidPath {
                path: path.display().to_string(),
                reason: "path traversal is not allowed".to_owned(),
            });
        }
    }
    Ok(path.to_path_buf())
}

fn revision(bytes: &[u8]) -> FileRevision {
    // FNV-1a is a compact, deterministic content revision token. It is used only for
    // optimistic concurrency, not for security or identity, so no extra crypto crate is
    // needed for the local and SFTP implementations.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    FileRevision {
        token: format!("fnv1a:{hash:016x}"),
        size_bytes: bytes.len() as u64,
    }
}

fn diff_text(path: &str, original: &str, current: &str) -> DiffResult {
    if original == current {
        return DiffResult {
            path: path.to_owned(),
            changed: false,
            unified: String::new(),
        };
    }
    let before = original.lines().collect::<Vec<_>>();
    let after = current.lines().collect::<Vec<_>>();
    let mut unified = format!("--- {path} (original)\n+++ {path} (current)\n");
    let common = before.len().min(after.len());
    for index in 0..common {
        if before[index] == after[index] {
            unified.push_str(&format!(" {}\n", before[index]));
        } else {
            unified.push_str(&format!("-{}\n+{}\n", before[index], after[index]));
        }
    }
    for line in before.iter().skip(common) {
        unified.push_str(&format!("-{}\n", line));
    }
    for line in after.iter().skip(common) {
        unified.push_str(&format!("+{}\n", line));
    }
    DiffResult {
        path: path.to_owned(),
        changed: true,
        unified,
    }
}

fn parse_git_status(output: &str) -> Vec<GitStatusEntry> {
    output
        .lines()
        .filter(|line| !line.starts_with("##") && line.len() >= 3)
        .map(|line| GitStatusEntry {
            code: line[..2].to_owned(),
            path: line[3..].to_owned(),
        })
        .collect()
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn io_error(operation: &str, path: &Path, error: impl fmt::Display) -> FileServiceError {
    FileServiceError::Io {
        operation: operation.to_owned(),
        path: path.display().to_string(),
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn fixture_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("herdr-ide-t7-{name}-{}", std::process::id()))
    }

    fn local_fixture(name: &str) -> LocalFileService {
        let root = fixture_root(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/nested")).unwrap();
        std::fs::write(root.join("README.md"), "hello\nworld\n").unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "fn main() {\n    println!(\"needle\");\n}\n",
        )
        .unwrap();
        std::fs::write(root.join("src/nested/notes.txt"), "needle again\n").unwrap();
        LocalFileService::new(root).unwrap()
    }

    #[test]
    fn local_service_lists_searches_and_opens_utf8_files() {
        let service = local_fixture("read");
        let entries = service.list("src").unwrap();
        assert_eq!(entries[0].name, "main.rs");
        assert_eq!(entries[1].kind, FileKind::Directory);
        assert_eq!(
            service.search_filename("notes").unwrap()[0].path,
            "src/nested/notes.txt"
        );
        let matches = service.search_content("needle").unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line, Some(2));
        assert_eq!(service.open("README.md").unwrap().content, "hello\nworld\n");
        let _ = std::fs::remove_dir_all(service.root());
    }

    #[test]
    fn save_uses_revision_guard_and_atomic_publish() {
        let service = local_fixture("save");
        let document = service.open("README.md").unwrap();
        let saved = service
            .save("README.md", "updated\n", &document.revision)
            .unwrap();
        assert_eq!(saved.document.content, "updated\n");
        let conflict = service
            .save("README.md", "stale\n", &document.revision)
            .unwrap_err();
        assert!(matches!(
            conflict,
            FileServiceError::RevisionConflict { .. }
        ));
        assert_eq!(service.open("README.md").unwrap().content, "updated\n");
        let _ = std::fs::remove_dir_all(service.root());
    }

    #[test]
    fn path_traversal_and_binary_open_are_explicit_failures() {
        let service = local_fixture("guard");
        let traversal = service.open("../secret").unwrap_err();
        assert!(matches!(traversal, FileServiceError::InvalidPath { .. }));
        std::fs::write(service.root().join("binary"), [0xff, 0x00]).unwrap();
        assert!(matches!(
            service.open("binary"),
            Err(FileServiceError::Encoding { .. })
        ));
        let _ = std::fs::remove_dir_all(service.root());
    }

    #[test]
    fn diff_and_git_status_are_deterministic_and_redacted_to_relative_paths() {
        let service = local_fixture("diff");
        let diff = service.diff("README.md", "hello\nchanged\n").unwrap();
        assert!(diff.changed);
        assert!(diff.unified.contains("-changed"));
        assert!(diff.unified.contains("+world"));
        let init = Command::new("git")
            .args(["init", "-q", service.root().to_string_lossy().as_ref()])
            .status()
            .unwrap();
        assert!(init.success());
        let status = service.git_status().unwrap();
        assert!(!status.is_empty());
        assert!(status.iter().all(|entry| !entry.path.starts_with('/')));
        let _ = std::fs::remove_dir_all(service.root());
    }

    #[test]
    fn persisted_tree_expansion_converges_on_repeated_save() {
        let path = fixture_root("tree").join("state.json");
        let _ = std::fs::remove_file(&path);
        let mut state = FileTreeState::load_or_default(&path).unwrap();
        state.set_expanded("src", true);
        state.save().unwrap();
        state.save().unwrap();
        let restored = FileTreeState::load_or_default(&path).unwrap();
        assert!(restored.preferences().expanded.contains("src"));
        let _ = std::fs::remove_file(path);
    }

    #[derive(Clone, Debug, Default)]
    struct FakeSftp {
        files: std::sync::Arc<std::sync::Mutex<BTreeMap<String, Vec<u8>>>>,
    }

    impl SftpTransport for FakeSftp {
        fn list(&self, path: &str) -> FileResult<Vec<FileEntry>> {
            let files = self.files.lock().unwrap();
            let prefix = format!("{}/", path.trim_end_matches('/'));
            let mut names = BTreeSet::new();
            for file in files.keys() {
                if let Some(name) = file
                    .strip_prefix(&prefix)
                    .and_then(|rest| rest.split('/').next())
                {
                    names.insert(name.to_owned());
                }
            }
            Ok(names
                .into_iter()
                .map(|name| {
                    let full = format!("{prefix}{name}");
                    let directory = files
                        .keys()
                        .any(|path| path.starts_with(&format!("{full}/")));
                    FileEntry {
                        path: full,
                        name,
                        kind: if directory {
                            FileKind::Directory
                        } else {
                            FileKind::File
                        },
                        size_bytes: 0,
                    }
                })
                .collect())
        }

        fn read(&self, path: &str) -> FileResult<Vec<u8>> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| FileServiceError::Remote {
                    operation: "read".to_owned(),
                    target: path.to_owned(),
                    reason: "not found".to_owned(),
                })
        }

        fn write(&self, path: &str, bytes: &[u8]) -> FileResult<()> {
            self.files
                .lock()
                .unwrap()
                .insert(path.to_owned(), bytes.to_vec());
            Ok(())
        }

        fn git_status(&self, _root: &str) -> FileResult<String> {
            Ok(" M remote.txt\n".to_owned())
        }
    }

    #[test]
    fn remote_service_has_the_same_revision_and_git_contract() {
        let files = std::sync::Arc::new(std::sync::Mutex::new(BTreeMap::from([(
            "/remote/workspace/remote.txt".to_owned(),
            b"before\n".to_vec(),
        )])));
        let service = RemoteFileService::new("/remote/workspace", FakeSftp { files }).unwrap();
        let document = service.open("remote.txt").unwrap();
        service
            .save("remote.txt", "after\n", &document.revision)
            .unwrap();
        assert_eq!(service.open("remote.txt").unwrap().content, "after\n");
        assert_eq!(service.git_status().unwrap()[0].path, "remote.txt");
    }
}
