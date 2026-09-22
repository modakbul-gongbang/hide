//! The filesystem boundary the web shell reads and writes behind (PRD
//! web-shell-pivot-s2 D-09 and B10, web-shell-pivot-s3 D-01 and B11).
//!
//! Two lines live here, and every path a client sends is checked against one
//! of them before the core sees it.
//!
//! The registration flow (`remote_file_list` for the `local` target and
//! `create_workspace`) lives behind `$HOME`, as it did in S2: a path
//! written outside home is refused as `outside_home` before anything is
//! read, and a path under home is resolved one component at a time from home,
//! where a symlink's target is tested as written the same way before it is
//! followed. So the reason a client reads never says whether a path outside
//! home exists, not even through a symlink it planted under home. `..` and
//! encoded segments are refused on shape or resolve like any other name. The
//! boundary root is read from `HOME` at boot and is not configurable
//! (`practices/env.md`); an allowed-roots setting is an S5 candidate.
//!
//! The Explorer's file work (open, reveal, save, create, rename, move,
//! trash, and later the index and the attachment bytes) lives behind the
//! registered checkout roots the core snapshot carries, which is the same
//! line the core draws for a path outside every checkout. The roots come from
//! the snapshot frame hided already decodes for its clients
//! (`rest.navigator.workspaces`, a local workspace whose checkout exists),
//! so the daemon keeps no second copy of the registration contract. The walk
//! below is the one the home line uses with the root set in place of home, so
//! a symlink out of a checkout is refused as `outside_checkout` before its
//! target is read, whether or not that target exists. Roots are kept most
//! specific first, so a checkout nested inside another owns the paths under
//! it.
//!
//! The Explorer's listing (`file_list`) is that same line: the children of a
//! folder under a root, files and hidden names included and `.git` dropped,
//! ordered as the Swift Explorer orders them, with a symlink that leaves the
//! root left out rather than followed. The registration listing above keeps
//! its own policy - directories only, hidden names dropped - because the
//! directory autocomplete asks a different question of the same tree.
//!
//! Nothing here reaches the core: a refused path is answered to the client and
//! logged, and an accepted path is forwarded as the canonical path that was
//! checked, so the core acts on exactly what the boundary saw.

use std::cmp::Ordering;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Why a path was not answered or forwarded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The real path is not under the home directory.
    OutsideHome,
    /// The real path is not under a registered checkout root.
    OutsideCheckout,
    /// The path is the home directory itself; a workspace is a subdirectory.
    HomeRoot,
    /// Nothing exists at the path.
    NotFound,
    /// The path exists but is not a directory.
    NotADirectory,
    /// The path exists but is not a regular file.
    NotAFile,
    /// Empty, relative, or otherwise not a path this daemon reads.
    InvalidPath,
}

impl Refusal {
    /// The reason code the contract names (`contracts/hided-ws.schema.json`).
    pub fn code(self) -> &'static str {
        match self {
            Self::OutsideHome => "outside_home",
            Self::OutsideCheckout => "outside_checkout",
            Self::HomeRoot => "home_root",
            Self::NotFound => "not_found",
            Self::NotADirectory => "not_a_directory",
            Self::NotAFile => "not_a_file",
            Self::InvalidPath => "invalid_path",
        }
    }
}

/// One child a listing shows: its name, the path a client sends back, and
/// whether it is a directory, which the Explorer draws and opens differently.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
}

/// A listing answer: the directory that was listed and its visible children.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct Listing {
    pub root_path: String,
    pub entries: Vec<Entry>,
    /// More than `LIST_CAP` children existed; the rest were not read.
    pub truncated: bool,
}

/// Children a listing carries at most; a home directory with more subfolders
/// than this is answered as truncated, not grown (engineering principle 15).
pub const LIST_CAP: usize = 500;

/// Symlinks one path may pass through before it is refused as a loop; the
/// kernel's own limit for one lookup is the same order.
const SYMLINK_HOPS: usize = 40;

/// Bytes one `file_bytes` request may name, for the whole file or one range.
/// A viewer reads a document into memory, so the daemon refuses a read past
/// this rather than growing an unbounded allocation (engineering 15).
pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// A name a create or a rename may carry: one component, and not one of the
/// shapes that would name a different path. The core refuses the same names
/// from the paths alone, so this is the boundary's own answer to the client
/// rather than the last word on the name.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0')
}

/// Lexical normalization of an absolute path: `.` dropped, `..` applied to
/// the component before it. Used only on a symlink target joined to a path
/// that holds no symlink, where the lexical parent is the real one.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// One checkout the Explorer's file work is confined to: the pair a client
/// names when it opens or reveals, and the path the core's snapshot carries for
/// that checkout, kept exactly as the snapshot spells it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Root {
    pub workspace_id: String,
    pub checkout_id: String,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct Boundary {
    /// The real home directory every accepted path resolves under.
    home: PathBuf,
    /// Home as `HOME` names it; a client writes paths under this spelling
    /// when home itself sits behind a symlink (`/var` for `/private/var`).
    home_as_given: PathBuf,
    /// The registered checkout roots, most specific first. Empty until the
    /// core's first snapshot arrives, so Explorer work that arrives before it
    /// is refused as `outside_checkout` rather than acted on.
    roots: RwLock<Vec<Root>>,
}

impl Boundary {
    /// Reads the boundary root once. Fails when `$HOME` does not resolve to a
    /// directory: a daemon without a boundary must not serve the registration
    /// flow at all.
    pub fn new(home: &Path) -> Result<Self, String> {
        let real = home
            .canonicalize()
            .map_err(|error| format!("HOME {} does not resolve: {error}", home.display()))?;
        if !real.is_dir() {
            return Err(format!("HOME {} is not a directory", home.display()));
        }
        Ok(Self {
            home: real,
            home_as_given: home.to_path_buf(),
            roots: RwLock::new(Vec::new()),
        })
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Replaces the root set wholesale with the checkouts the core's latest
    /// snapshot carries: a registration whose path is not an absolute existing
    /// directory is dropped, and the rest are kept most specific first, so a
    /// checkout nested inside another owns the paths under it.
    ///
    /// The paths are kept exactly as the snapshot spells them. The core
    /// compares the root it is handed with the focused checkout's path byte for
    /// byte, so re-spelling one here would refuse every operation on it.
    pub fn set_roots(&self, roots: Vec<Root>) {
        let mut accepted: Vec<Root> = roots
            .into_iter()
            .filter(|root| root.path.is_absolute() && root.path.is_dir())
            .collect();
        accepted.sort_by_key(|root| std::cmp::Reverse(root.path.components().count()));
        {
            let current = self.roots_for_read();
            if *current == accepted {
                return;
            }
        }
        let count = accepted.len();
        *self.roots_for_write() = accepted;
        eprintln!(
            "{}",
            serde_json::json!({"component": "hided", "kind": "boundary.roots", "roots": count})
        );
    }

    fn roots_for_read(&self) -> RwLockReadGuard<'_, Vec<Root>> {
        self.roots
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn roots_for_write(&self) -> RwLockWriteGuard<'_, Vec<Root>> {
        self.roots
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The registered root `raw` names, as the snapshot spells it, or `None`
    /// when no registration carries that path.
    pub fn known_root(&self, raw: &str) -> Option<PathBuf> {
        let wanted = Path::new(raw);
        self.roots_for_read()
            .iter()
            .find(|candidate| candidate.path == wanted)
            .map(|candidate| candidate.path.clone())
    }

    /// The path of `raw` when it is written under `root`, which has to be one
    /// of the registered roots (`known_root`).
    ///
    /// The answer keeps the root's own spelling instead of the filesystem's,
    /// because the core compares the root it receives with the focused
    /// checkout's path byte for byte.
    pub fn resolve_below(&self, root: &Path, raw: &str) -> Result<PathBuf, Refusal> {
        let raw = self.expand(raw)?;
        let path = Path::new(&raw);
        let Ok(rest) = path.strip_prefix(root) else {
            return Err(Refusal::OutsideCheckout);
        };
        self.walk(root, rest, Refusal::OutsideCheckout)
    }

    /// The path of `raw` when it is written under a registered checkout root,
    /// whether it names a file or a directory.
    ///
    /// A path under home that is not under a root is not this line's to answer
    /// and is refused as `outside_checkout`, so the two lines never mix: the
    /// registration flow reads home and this one reads the checkouts.
    pub fn resolve_target(&self, raw: &str) -> Result<PathBuf, Refusal> {
        let raw = self.expand(raw)?;
        let path = Path::new(&raw);
        let Some((root, rest)) = self.strip_root(path) else {
            return Err(Refusal::OutsideCheckout);
        };
        self.walk(&root, rest, Refusal::OutsideCheckout)
    }

    /// A regular file under a registered checkout root, with its size. The
    /// file-bytes read is the only caller: a viewer asks for bytes, so a
    /// directory, a symlink to one, and anything else that is not a file is
    /// refused as `not_a_file` rather than read.
    pub fn resolve_file(&self, raw: &str) -> Result<(PathBuf, u64), Refusal> {
        let path = self.resolve_target(raw)?;
        let metadata = fs::metadata(&path).map_err(|_| Refusal::NotFound)?;
        if !metadata.is_file() {
            return Err(Refusal::NotAFile);
        }
        Ok((path, metadata.len()))
    }

    /// The path of `raw` when it is written under `root`, the root the event
    /// itself named.
    pub fn resolve_in_root(&self, root: &str, raw: &str) -> Result<PathBuf, Refusal> {
        let Some(known) = self.known_root(root) else {
            return Err(Refusal::OutsideCheckout);
        };
        self.resolve_below(&known, raw)
    }

    /// The path of `raw` for an open or a reveal that names a checkout: it has
    /// to sit under that checkout's root. A pair this daemon holds no root for
    /// is answered against any root, because the core names that case itself
    /// (`reveal.unknown_checkout`) and the client reads one reason either way.
    pub fn resolve_checkout(
        &self,
        workspace_id: &str,
        checkout_id: &str,
        raw: &str,
    ) -> Result<PathBuf, Refusal> {
        let named = self
            .roots_for_read()
            .iter()
            .find(|root| root.workspace_id == workspace_id && root.checkout_id == checkout_id)
            .map(|root| root.path.clone());
        match named {
            Some(root) => self.resolve_below(&root, raw),
            None => self.resolve_target(raw),
        }
    }

    /// Whether `raw` is written under some root, judged from the path alone.
    /// A save is checked this way and never rewritten: the core compares the
    /// path it stored with the one it is handed.
    pub fn is_under_root(&self, raw: &str) -> bool {
        let Ok(raw) = self.expand(raw) else {
            return false;
        };
        self.strip_root(Path::new(&raw)).is_some()
    }

    /// The most specific root `path` is written under, with what follows it.
    /// The comparison is component-wise, so a sibling whose name merely starts
    /// with a root's path (`/repo-other` beside `/repo`) is not inside it.
    fn strip_root<'a>(&self, path: &'a Path) -> Option<(PathBuf, &'a Path)> {
        self.roots_for_read().iter().find_map(|root| {
            path.strip_prefix(&root.path)
                .ok()
                .map(|rest| (root.path.clone(), rest))
        })
    }

    /// The canonical directory for `raw` when it is home or under home.
    /// `~` and `~/...` name the home directory, so a client can start its
    /// listing without knowing the path; the answer carries the real one.
    /// A path written outside home, or reached through a symlink whose
    /// target is written outside home, is refused before the filesystem is
    /// read there, so the reason never says whether such a path exists.
    pub fn resolve_dir(&self, raw: &str) -> Result<PathBuf, Refusal> {
        let raw = self.expand(raw)?;
        let path = Path::new(&raw);
        let Some(rest) = self.strip_home(path) else {
            return Err(Refusal::OutsideHome);
        };
        let resolved = self.walk(&self.home, rest, Refusal::OutsideHome)?;
        // The walk left no symlink in `resolved`, so this only settles the
        // spelling the OS keeps (letter case on a case-insensitive volume)
        // and cannot fail for a reason the walk has not already answered.
        let real = resolved.canonicalize().map_err(|_| Refusal::InvalidPath)?;
        if !real.starts_with(&self.home) {
            return Err(Refusal::OutsideHome);
        }
        let metadata = fs::metadata(&real).map_err(|_| Refusal::NotFound)?;
        if !metadata.is_dir() {
            return Err(Refusal::NotADirectory);
        }
        Ok(real)
    }

    /// `~` and `~/...` name the home directory, so a client can start without
    /// knowing the path; any other spelling is used as written. Every shape
    /// this daemon does not read is refused here: empty, a NUL, a relative
    /// path, and a `..` component, which would otherwise name its way out of
    /// the directory it is joined to. A `.` component is normalized away by
    /// the path parser and only survives as the first component of a relative
    /// path, which the absolute check above already refuses; the core reads
    /// the same rule, so the two agree on what a normal path is.
    fn expand(&self, raw: &str) -> Result<String, Refusal> {
        let expanded;
        let raw = if raw == "~" || raw.starts_with("~/") {
            expanded = self
                .home
                .join(raw.trim_start_matches('~').trim_start_matches('/'));
            expanded.to_string_lossy().into_owned()
        } else {
            raw.to_owned()
        };
        let path = Path::new(&raw);
        if raw.is_empty() || raw.contains('\0') || !path.is_absolute() {
            return Err(Refusal::InvalidPath);
        }
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
        {
            return Err(Refusal::InvalidPath);
        }
        Ok(raw)
    }

    /// The part of `path` below home, when `path` is written under home in
    /// either spelling.
    fn strip_home<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        path.strip_prefix(&self.home)
            .or_else(|_| path.strip_prefix(&self.home_as_given))
            .ok()
    }

    /// The part of `path` below `anchor`, accepting the second spelling of
    /// home when the anchor is home (`/var` for `/private/var`), which is the
    /// spelling `HOME` uses.
    fn strip_anchor<'a>(&self, anchor: &Path, path: &'a Path) -> Option<&'a Path> {
        if let Ok(rest) = path.strip_prefix(anchor) {
            return Some(rest);
        }
        if anchor == self.home {
            self.strip_home(path)
        } else {
            None
        }
    }

    /// Resolves `rest` below `anchor` one component at a time. Every name is
    /// read where it sits, so the filesystem is only ever asked about paths
    /// under the anchor; a symlink is replaced by its target as written, and a
    /// target not written under the anchor is refused as `outside` - the
    /// reason the line that asked for the walk names - before anything about it
    /// is read, whether or not it exists.
    fn walk(&self, anchor: &Path, rest: &Path, outside: Refusal) -> Result<PathBuf, Refusal> {
        let mut current = anchor.to_path_buf();
        let mut pending: VecDeque<OsString> = rest
            .components()
            .filter(|component| matches!(component, Component::Normal(_)))
            .map(|component| component.as_os_str().to_owned())
            .collect();
        let mut hops = 0;
        while let Some(name) = pending.pop_front() {
            let next = current.join(&name);
            let metadata = match fs::symlink_metadata(&next) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(Refusal::NotFound);
                }
                Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
                    return Err(Refusal::NotADirectory);
                }
                // A loop, a denied directory, or another OS refusal under
                // home: none of them describes anything outside it.
                Err(_) => return Err(Refusal::InvalidPath),
            };
            if metadata.file_type().is_symlink() {
                hops += 1;
                if hops > SYMLINK_HOPS {
                    return Err(Refusal::InvalidPath);
                }
                let target = fs::read_link(&next).map_err(|_| Refusal::InvalidPath)?;
                let target = normalize(&current.join(target));
                let Some(below) = self.strip_anchor(anchor, &target) else {
                    return Err(outside);
                };
                // Start over from the anchor with the target's components in
                // front of what is still pending; `current` holds no symlink,
                // so a `..` in the target was resolved on real directories.
                let mut replaced: VecDeque<OsString> = below
                    .components()
                    .filter(|component| matches!(component, Component::Normal(_)))
                    .map(|component| component.as_os_str().to_owned())
                    .collect();
                replaced.append(&mut pending);
                pending = replaced;
                current = anchor.to_path_buf();
                continue;
            }
            if !pending.is_empty() && !metadata.is_dir() {
                return Err(Refusal::NotADirectory);
            }
            current = next;
        }
        Ok(current)
    }

    /// The canonical path a `create_workspace` may carry: a directory strictly
    /// under home.
    pub fn resolve_workspace(&self, raw: &str) -> Result<PathBuf, Refusal> {
        let real = self.resolve_dir(raw)?;
        if real == self.home {
            return Err(Refusal::HomeRoot);
        }
        Ok(real)
    }

    /// The visible subdirectories of `raw`: no files, no hidden names, and no
    /// symlink whose target leaves home or is not a directory.
    pub fn list(&self, raw: &str) -> Result<Listing, Refusal> {
        let root = self.resolve_dir(raw)?;
        let read = fs::read_dir(&root).map_err(|_| Refusal::NotFound)?;
        let mut entries = Vec::new();
        let mut truncated = false;
        for item in read {
            let Ok(item) = item else { continue };
            let name = item.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with('.') {
                continue;
            }
            let Ok(real) = item.path().canonicalize() else {
                continue;
            };
            if !real.starts_with(&self.home) || !real.is_dir() {
                continue;
            }
            if entries.len() == LIST_CAP {
                truncated = true;
                break;
            }
            entries.push(Entry {
                name: name.to_owned(),
                path: root.join(name).display().to_string(),
                is_directory: true,
            });
        }
        entries.sort_by_key(|entry| entry.name.to_lowercase());
        Ok(Listing {
            root_path: root.display().to_string(),
            entries,
            truncated,
        })
    }

    /// The children of a folder inside a registered checkout: files and
    /// directories, hidden names included, `.git` left out.
    ///
    /// This is the Explorer's line, so the policy is the Swift Explorer's
    /// (`WorkspaceOutlineView`): `.git` is the name it hides that the approved
    /// line hides too - the other names it skips are its own build directories,
    /// which the approved line does not name - and the order is its order,
    /// directories first and then the natural one `localizedStandardCompare`
    /// gives, so `file2` sorts before `file10`. The Explorer renders the order
    /// it is handed; nothing sorts these rows again on the client.
    ///
    /// A child that is neither a file nor a directory, and a symlink whose
    /// target leaves `root`, is not a row: the boundary does not describe a
    /// path outside the checkout, not even as a name in a listing.
    pub fn list_children(&self, root: &Path, raw: &str) -> Result<Listing, Refusal> {
        let dir = self.resolve_below(root, raw)?;
        let metadata = fs::metadata(&dir).map_err(|_| Refusal::NotFound)?;
        if !metadata.is_dir() {
            return Err(Refusal::NotADirectory);
        }
        // The root as the snapshot spells it may sit behind a symlink (`/var`
        // for `/private/var`) while a child's canonical path does not, so the
        // containment test uses the real spelling of both.
        let real_root = root.canonicalize().map_err(|_| Refusal::OutsideCheckout)?;
        let read = fs::read_dir(&dir).map_err(|_| Refusal::NotFound)?;
        let mut entries = Vec::new();
        let mut truncated = false;
        for item in read {
            let Ok(item) = item else { continue };
            let name = item.file_name();
            let Some(name) = name.to_str() else { continue };
            if name == GIT_DIR_NAME {
                continue;
            }
            let Some(is_directory) = child_kind(&real_root, &item.path()) else {
                continue;
            };
            if entries.len() == LIST_CAP {
                truncated = true;
                break;
            }
            entries.push(Entry {
                name: name.to_owned(),
                path: dir.join(name).display().to_string(),
                is_directory,
            });
        }
        entries.sort_by(|left, right| {
            right
                .is_directory
                .cmp(&left.is_directory)
                .then_with(|| natural_cmp(&left.name, &right.name))
        });
        Ok(Listing {
            root_path: dir.display().to_string(),
            entries,
            truncated,
        })
    }
}

/// The one name the Explorer hides: the repository's own directory. Every
/// other hidden name is a row the approved line shows.
const GIT_DIR_NAME: &str = ".git";

/// Whether a child of `root` is a directory (`true`) or a file (`false`), or
/// `None` when it is not a row the Explorer shows: a symlink whose target
/// leaves the root, one that resolves to nothing, and anything that is neither
/// a file nor a directory (a socket, a fifo, a device).
fn child_kind(root: &Path, path: &Path) -> Option<bool> {
    let real = path.canonicalize().ok()?;
    if !real.starts_with(root) {
        return None;
    }
    let metadata = fs::metadata(&real).ok()?;
    if metadata.is_dir() {
        Some(true)
    } else if metadata.is_file() {
        Some(false)
    } else {
        None
    }
}

/// The order the Swift Explorer shows names in, as far as this daemon needs it:
/// case-insensitive, with a run of digits compared as a number, so `file2`
/// sorts before `file10`. `localizedStandardCompare` is the rule the approved
/// line names and this is that rule's comparable part; a tie keeps the order the
/// directory was read in.
pub(crate) fn natural_cmp(left: &str, right: &str) -> Ordering {
    let mut left = left.chars().flat_map(char::to_lowercase).peekable();
    let mut right = right.chars().flat_map(char::to_lowercase).peekable();
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(l), Some(r)) if l.is_ascii_digit() && r.is_ascii_digit() => {
                let left_run = take_digits(&mut left);
                let right_run = take_digits(&mut right);
                let order = number_order(&left_run, &right_run);
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(l), Some(r)) => {
                left.next();
                right.next();
                let order = l.cmp(&r);
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

/// The digits at the front of an iterator, consumed.
fn take_digits(iter: &mut std::iter::Peekable<impl Iterator<Item = char>>) -> String {
    let mut run = String::new();
    while let Some(digit) = iter.peek().copied() {
        if !digit.is_ascii_digit() {
            break;
        }
        run.push(digit);
        iter.next();
    }
    run
}

/// Two digit runs by their value: the run with fewer significant digits is the
/// smaller number, and equal lengths compare as text. Leading zeros do not
/// count, so `007` and `7` are the same number.
fn number_order(left: &str, right: &str) -> Ordering {
    let left = left.trim_start_matches('0');
    let right = right.trim_start_matches('0');
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Fixture {
        _outer: tempfile::TempDir,
        home: PathBuf,
        outside: PathBuf,
        boundary: Boundary,
    }

    fn fixture() -> Fixture {
        let outer = tempfile::tempdir().unwrap();
        let home = outer.path().join("home");
        let outside = outer.path().join("outside");
        fs::create_dir_all(home.join("projects/alpha")).unwrap();
        fs::create_dir_all(home.join("projects/Beta")).unwrap();
        fs::create_dir_all(home.join("projects/.hidden")).unwrap();
        fs::create_dir_all(outside.join("secret")).unwrap();
        fs::write(home.join("projects/notes.txt"), "x").unwrap();
        symlink(&outside, home.join("projects/escape")).unwrap();
        symlink(home.join("projects/alpha"), home.join("projects/alias")).unwrap();
        symlink(
            home.join("projects/notes.txt"),
            home.join("projects/filelink"),
        )
        .unwrap();
        let boundary = Boundary::new(&home).unwrap();
        Fixture {
            _outer: outer,
            home: boundary.home().to_path_buf(),
            outside,
            boundary,
        }
    }

    fn s(path: &Path) -> String {
        path.display().to_string()
    }

    #[test]
    fn lists_only_visible_directories_inside_home() {
        let f = fixture();
        let listing = f.boundary.list(&s(&f.home.join("projects"))).unwrap();
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alias", "alpha", "Beta"]);
        assert!(!listing.truncated);
        assert_eq!(listing.root_path, s(&f.home.join("projects")));
    }

    #[test]
    fn a_checkout_listing_shows_files_and_hidden_names_but_never_git() {
        let f = fixture();
        let root = f.home.join("projects/alpha");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join(".config")).unwrap();
        fs::create_dir_all(root.join(".git/objects")).unwrap();
        fs::write(root.join("file2.txt"), "x").unwrap();
        fs::write(root.join("File10.txt"), "x").unwrap();
        fs::write(root.join(".env"), "x").unwrap();
        symlink(root.join("src"), root.join("alias")).unwrap();
        symlink(f.outside.join("secret"), root.join("escape")).unwrap();
        symlink(root.join("nope"), root.join("dangling")).unwrap();
        let listing = f.boundary.list_children(&root, &s(&root)).unwrap();
        let rows: Vec<(&str, bool)> = listing
            .entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.is_directory))
            .collect();
        assert_eq!(
            rows,
            vec![
                (".config", true),
                ("alias", true),
                ("src", true),
                (".env", false),
                ("file2.txt", false),
                ("File10.txt", false),
            ],
            "directories first and then the natural order; .git, the escaping \
             symlink and the dangling one are not rows"
        );
        assert!(!listing.truncated);
        assert_eq!(listing.root_path, s(&root));
        // A row's path keeps the root's own spelling, which is the spelling the
        // core compares the focused checkout against.
        assert_eq!(listing.entries[0].path, s(&root.join(".config")));
        // A folder under the root lists the same way one level down.
        let nested = f
            .boundary
            .list_children(&root, &s(&root.join("src")))
            .unwrap();
        assert!(nested.entries.is_empty());
        assert_eq!(nested.root_path, s(&root.join("src")));
    }

    #[test]
    fn a_checkout_listing_refuses_what_is_not_under_its_root() {
        let f = fixture();
        let root = f.home.join("projects/alpha");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("notes.txt"), "x").unwrap();
        // A sibling checkout is not under this root, and neither is home.
        for outside in [f.home.join("projects/Beta"), f.home.clone()] {
            assert_eq!(
                f.boundary.list_children(&root, &s(&outside)),
                Err(Refusal::OutsideCheckout),
                "{outside:?}"
            );
        }
        // A symlink inside the root to a directory outside it is refused
        // rather than read through.
        symlink(&f.outside, root.join("escape")).unwrap();
        assert_eq!(
            f.boundary.list_children(&root, &s(&root.join("escape"))),
            Err(Refusal::OutsideCheckout)
        );
        assert_eq!(
            f.boundary.list_children(&root, &s(&root.join("missing"))),
            Err(Refusal::NotFound)
        );
        assert_eq!(
            f.boundary.list_children(&root, &s(&root.join("notes.txt"))),
            Err(Refusal::NotADirectory)
        );
        assert_eq!(
            f.boundary
                .list_children(&root, &format!("{}/../Beta", s(&root))),
            Err(Refusal::InvalidPath)
        );
        assert!(
            f.boundary
                .list_children(&root, &s(&root.join("src")))
                .is_ok()
        );
    }

    #[test]
    fn names_sort_case_insensitively_and_by_the_value_of_a_digit_run() {
        let mut names = vec!["File10.txt", "file2.txt", "beta", "Beta2", "alpha"];
        names.sort_by(|left, right| natural_cmp(left, right));
        assert_eq!(
            names,
            vec!["alpha", "beta", "Beta2", "file2.txt", "File10.txt"]
        );
        assert_eq!(natural_cmp("007", "7"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "a "), Ordering::Less);
    }

    #[test]
    fn a_symlink_out_of_home_is_refused_even_when_named_directly() {
        let f = fixture();
        let escape = s(&f.home.join("projects/escape"));
        assert_eq!(f.boundary.list(&escape), Err(Refusal::OutsideHome));
        assert_eq!(
            f.boundary.resolve_workspace(&escape),
            Err(Refusal::OutsideHome)
        );
        let deeper = s(&f.home.join("projects/escape/secret"));
        assert_eq!(
            f.boundary.resolve_workspace(&deeper),
            Err(Refusal::OutsideHome)
        );
        // Past the escape the reason is the same whether the rest exists,
        // is a file, or is missing: the disk beyond home is not described.
        fs::write(f.outside.join("secret/marker.txt"), "x").unwrap();
        for tail in [
            "nope",
            "secret/nope",
            "secret/marker.txt",
            "secret/marker.txt/child",
        ] {
            let path = s(&f.home.join("projects/escape").join(tail));
            assert_eq!(
                f.boundary.resolve_workspace(&path),
                Err(Refusal::OutsideHome),
                "{tail}"
            );
            assert_eq!(f.boundary.list(&path), Err(Refusal::OutsideHome), "{tail}");
        }
    }

    #[test]
    fn parent_segments_and_encoded_segments_do_not_leave_home() {
        let f = fixture();
        let dotdot = format!("{}/projects/../../outside", s(&f.home));
        assert_eq!(
            f.boundary.resolve_workspace(&dotdot),
            Err(Refusal::InvalidPath)
        );
        let encoded = format!("{}/projects/%2e%2e/%2e%2e/outside", s(&f.home));
        assert_eq!(
            f.boundary.resolve_workspace(&encoded),
            Err(Refusal::NotFound)
        );
        assert_eq!(
            f.boundary.resolve_workspace(&s(&f.outside)),
            Err(Refusal::OutsideHome)
        );
        let sibling_prefix = format!("{}-extra", s(&f.home));
        fs::create_dir_all(&sibling_prefix).unwrap();
        assert_eq!(
            f.boundary.resolve_workspace(&sibling_prefix),
            Err(Refusal::OutsideHome),
            "a sibling whose name starts with the home path is outside"
        );
    }

    #[test]
    fn a_path_outside_home_gets_one_reason_whether_or_not_it_exists() {
        let f = fixture();
        let existing_dir = s(&f.outside.join("secret"));
        let missing_dir = s(&f.outside.join("nope"));
        let existing_file = s(&f.outside.join("secret/marker.txt"));
        fs::write(&existing_file, "x").unwrap();
        for path in [
            &existing_dir,
            &missing_dir,
            &existing_file,
            &"/etc/nope-zzz".to_owned(),
        ] {
            assert_eq!(
                f.boundary.resolve_workspace(path),
                Err(Refusal::OutsideHome),
                "{path}: the reason must not tell whether it exists"
            );
            assert_eq!(f.boundary.list(path), Err(Refusal::OutsideHome), "{path}");
        }
        // A symlink under home to a target outside home is refused the same
        // way whether its target exists or not, named directly or with a tail.
        symlink(&existing_dir, f.home.join("projects/out-exists")).unwrap();
        symlink(&missing_dir, f.home.join("projects/out-missing")).unwrap();
        symlink("../../outside/secret", f.home.join("projects/out-relative")).unwrap();
        symlink(
            "../../outside/nope",
            f.home.join("projects/out-relative-missing"),
        )
        .unwrap();
        for name in [
            "out-exists",
            "out-missing",
            "out-relative",
            "out-relative-missing",
        ] {
            for tail in ["", "child"] {
                let path = s(&f.home.join("projects").join(name).join(tail));
                assert_eq!(
                    f.boundary.resolve_workspace(&path),
                    Err(Refusal::OutsideHome),
                    "{name}/{tail}"
                );
            }
        }
        // A symlink into home from outside is still outside: the path as
        // written decides, not where it leads.
        let inbound = f.outside.join("inbound");
        symlink(f.home.join("projects/alpha"), &inbound).unwrap();
        assert_eq!(
            f.boundary.resolve_workspace(&s(&inbound)),
            Err(Refusal::OutsideHome)
        );
    }

    #[test]
    fn home_spelled_as_given_resolves_when_home_is_a_symlink() {
        let outer = tempfile::tempdir().unwrap();
        let real_home = outer.path().join("real-home");
        fs::create_dir_all(real_home.join("projects/alpha")).unwrap();
        let link_home = outer.path().join("link-home");
        symlink(&real_home, &link_home).unwrap();
        let boundary = Boundary::new(&link_home).unwrap();
        let canonical = real_home.canonicalize().unwrap();
        assert_eq!(boundary.home(), canonical);
        assert_eq!(
            boundary.resolve_workspace(&s(&link_home.join("projects/alpha"))),
            Ok(canonical.join("projects/alpha")),
            "the spelling HOME uses is under home"
        );
        assert_eq!(
            boundary.resolve_workspace(&s(&canonical.join("projects/alpha"))),
            Ok(canonical.join("projects/alpha"))
        );
        assert_eq!(
            boundary.list(&s(&link_home)).unwrap().root_path,
            s(&canonical)
        );
    }

    #[test]
    fn files_hidden_dirs_home_itself_and_bad_shapes() {
        let f = fixture();
        let file = s(&f.home.join("projects/notes.txt"));
        assert_eq!(
            f.boundary.resolve_workspace(&file),
            Err(Refusal::NotADirectory)
        );
        assert_eq!(f.boundary.list(&file), Err(Refusal::NotADirectory));
        let filelink = s(&f.home.join("projects/filelink"));
        assert_eq!(
            f.boundary.resolve_workspace(&filelink),
            Err(Refusal::NotADirectory)
        );
        let under_file = s(&f.home.join("projects/notes.txt/child"));
        assert!(matches!(
            f.boundary.resolve_workspace(&under_file),
            Err(Refusal::NotFound | Refusal::NotADirectory)
        ));
        let missing = s(&f.home.join("projects/nope"));
        assert_eq!(
            f.boundary.resolve_workspace(&missing),
            Err(Refusal::NotFound)
        );
        assert_eq!(
            f.boundary.resolve_workspace(&s(&f.home)),
            Err(Refusal::HomeRoot)
        );
        assert!(f.boundary.list(&s(&f.home)).is_ok(), "home itself lists");
        assert_eq!(f.boundary.list("~").unwrap().root_path, s(&f.home));
        assert_eq!(
            f.boundary.resolve_workspace("~/projects/alpha").unwrap(),
            f.home.join("projects/alpha")
        );
        assert_eq!(f.boundary.resolve_workspace("~"), Err(Refusal::HomeRoot));
        assert_eq!(
            f.boundary.resolve_workspace("~user/x"),
            Err(Refusal::InvalidPath)
        );
        assert_eq!(f.boundary.resolve_workspace(""), Err(Refusal::InvalidPath));
        assert_eq!(
            f.boundary.resolve_workspace("projects"),
            Err(Refusal::InvalidPath)
        );
        assert_eq!(
            f.boundary
                .resolve_workspace(&format!("{}\0", s(&f.home.join("projects")))),
            Err(Refusal::InvalidPath)
        );
        // A hidden directory is hidden from the listing but may be typed.
        let hidden = s(&f.home.join("projects/.hidden"));
        assert!(f.boundary.resolve_workspace(&hidden).is_ok());
    }

    #[test]
    fn symlinks_inside_home_resolve_and_loops_are_refused() {
        let f = fixture();
        symlink("alpha", f.home.join("projects/relative-alias")).unwrap();
        assert_eq!(
            f.boundary
                .resolve_workspace(&s(&f.home.join("projects/relative-alias"))),
            Ok(f.home.join("projects/alpha"))
        );
        symlink("../projects/alpha", f.home.join("projects/dotdot-alias")).unwrap();
        assert_eq!(
            f.boundary
                .resolve_workspace(&s(&f.home.join("projects/dotdot-alias"))),
            Ok(f.home.join("projects/alpha"))
        );
        symlink("loop-b", f.home.join("projects/loop-a")).unwrap();
        symlink("loop-a", f.home.join("projects/loop-b")).unwrap();
        assert_eq!(
            f.boundary
                .resolve_workspace(&s(&f.home.join("projects/loop-a"))),
            Err(Refusal::InvalidPath)
        );
        symlink("nowhere", f.home.join("projects/dangling-inside")).unwrap();
        assert_eq!(
            f.boundary
                .resolve_workspace(&s(&f.home.join("projects/dangling-inside"))),
            Err(Refusal::NotFound),
            "a dangling target under home is a missing path under home"
        );
    }

    #[test]
    fn an_accepted_path_is_the_canonical_one() {
        let f = fixture();
        let alias = s(&f.home.join("projects/alias"));
        assert_eq!(
            f.boundary.resolve_workspace(&alias).unwrap(),
            f.home.join("projects/alpha")
        );
        let trailing = format!("{}/", s(&f.home.join("projects/alpha")));
        assert_eq!(
            f.boundary.resolve_workspace(&trailing).unwrap(),
            f.home.join("projects/alpha")
        );
    }

    #[test]
    fn the_listing_is_capped() {
        let outer = tempfile::tempdir().unwrap();
        let home = outer.path().join("home");
        for i in 0..(LIST_CAP + 3) {
            fs::create_dir_all(home.join(format!("d{i:04}"))).unwrap();
        }
        let boundary = Boundary::new(&home).unwrap();
        let listing = boundary.list(&s(boundary.home())).unwrap();
        assert_eq!(listing.entries.len(), LIST_CAP);
        assert!(listing.truncated);
    }

    #[test]
    fn a_missing_home_refuses_to_boot() {
        assert!(Boundary::new(Path::new("/nonexistent/home/for/hided")).is_err());
    }

    fn root(workspace_id: &str, checkout_id: &str, path: &Path) -> Root {
        Root {
            workspace_id: workspace_id.to_owned(),
            checkout_id: checkout_id.to_owned(),
            path: path.to_path_buf(),
        }
    }

    /// The fixture with two registered checkouts: alpha, and alpha/nested
    /// inside it, so a path under the nested one has two candidate owners.
    fn rooted() -> Fixture {
        let f = fixture();
        let repo = f.home.join("projects/alpha");
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::create_dir_all(repo.join("nested/deep")).unwrap();
        fs::write(repo.join("src/main.rs"), "fn main() {}").unwrap();
        f.boundary.set_roots(vec![
            root("w1", "c1", &repo),
            root("w1", "c2", &repo.join("nested")),
        ]);
        f
    }

    #[test]
    fn a_path_under_a_root_resolves_in_the_root_spelling() {
        let f = rooted();
        let repo = f.home.join("projects/alpha");
        assert_eq!(f.boundary.resolve_target(&s(&repo)).unwrap(), repo);
        assert_eq!(
            f.boundary
                .resolve_target(&s(&repo.join("src/main.rs")))
                .unwrap(),
            repo.join("src/main.rs")
        );
        assert_eq!(
            f.boundary
                .resolve_target(&format!("{}/", s(&repo.join("src"))))
                .unwrap(),
            repo.join("src"),
            "the spelling that was checked is the one forwarded"
        );
        assert!(f.boundary.is_under_root(&s(&repo.join("src/main.rs"))));
    }

    #[test]
    fn a_file_under_a_root_resolves_with_its_size_and_a_directory_does_not() {
        let f = rooted();
        let repo = f.home.join("projects/alpha");
        let (path, size) = f
            .boundary
            .resolve_file(&s(&repo.join("src/main.rs")))
            .unwrap();
        assert_eq!(path, repo.join("src/main.rs"));
        assert_eq!(size, "fn main() {}".len() as u64);
        assert_eq!(
            f.boundary.resolve_file(&s(&repo.join("src"))),
            Err(Refusal::NotAFile),
            "a directory is not a byte source"
        );
        assert_eq!(
            f.boundary.resolve_file(&s(&repo.join("src/nope"))),
            Err(Refusal::NotFound)
        );
        assert_eq!(
            f.boundary.resolve_file(&s(&f.outside.join("secret"))),
            Err(Refusal::OutsideCheckout),
            "the file line is the checkout line, not the home one"
        );
    }

    #[test]
    fn a_path_outside_every_root_is_refused_as_outside_checkout() {
        let f = rooted();
        let repo = f.home.join("projects/alpha");
        fs::create_dir_all(f.home.join("projects/alpha-other")).unwrap();
        for path in [
            s(&f.home),
            s(&f.home.join("projects")),
            s(&f.home.join("projects/notes.txt")),
            s(&f.home.join("projects/alpha-other")),
            s(&f.outside),
            "/etc/nope-zzz".to_owned(),
        ] {
            assert_eq!(
                f.boundary.resolve_target(&path),
                Err(Refusal::OutsideCheckout),
                "{path}: a path under home that is not under a checkout is not this line's"
            );
            assert!(!f.boundary.is_under_root(&path), "{path}");
        }
        assert_eq!(
            f.boundary.resolve_target(&s(&repo.join("src/nope"))),
            Err(Refusal::NotFound),
            "inside a root, a missing name is missing rather than outside"
        );
    }

    #[test]
    fn parent_segments_never_leave_a_root() {
        let f = rooted();
        let repo = f.home.join("projects/alpha");
        for shape in [
            format!("{}/src/../nested", s(&repo)),
            format!("{}/../../outside", s(&repo)),
        ] {
            assert_eq!(
                f.boundary.resolve_target(&shape),
                Err(Refusal::InvalidPath),
                "{shape}"
            );
        }
        // A `.` component is not a way out of a root and not a reason to
        // refuse one either: the parser drops it, in this daemon and in the
        // core's own `path_inside_root`.
        assert_eq!(
            f.boundary.resolve_target(&format!("{}/./src", s(&repo))),
            Ok(repo.join("src"))
        );
    }

    #[test]
    fn a_symlink_out_of_a_root_is_refused_before_its_target_is_read() {
        let f = rooted();
        let repo = f.home.join("projects/alpha");
        symlink(&f.outside, repo.join("escape")).unwrap();
        symlink("../../outside/nope", repo.join("escape-missing")).unwrap();
        symlink("src", repo.join("alias")).unwrap();
        assert_eq!(
            f.boundary.resolve_target(&s(&repo.join("alias"))).unwrap(),
            repo.join("src"),
            "a link that stays inside the root resolves"
        );
        for name in ["escape", "escape-missing"] {
            for tail in ["", "secret", "secret/marker.txt"] {
                let path = s(&repo.join(name).join(tail));
                assert_eq!(
                    f.boundary.resolve_target(&path),
                    Err(Refusal::OutsideCheckout),
                    "{name}/{tail}: the reason is the same whether the target exists"
                );
            }
        }
        // A link that leaves the nested checkout but stays inside the outer
        // one belongs to the nested checkout, which owns the path it sits in.
        fs::create_dir_all(repo.join("other")).unwrap();
        symlink("../other", repo.join("nested/out-of-nested")).unwrap();
        assert_eq!(
            f.boundary
                .resolve_target(&s(&repo.join("nested/out-of-nested"))),
            Err(Refusal::OutsideCheckout)
        );
        f.boundary.set_roots(vec![root("w1", "c1", &repo)]);
        assert_eq!(
            f.boundary
                .resolve_target(&s(&repo.join("nested/out-of-nested")))
                .unwrap(),
            repo.join("other"),
            "with only the outer root registered, the same link resolves"
        );
    }

    #[test]
    fn a_named_root_has_to_be_one_of_the_registered_ones() {
        let f = rooted();
        let repo = f.home.join("projects/alpha");
        let nested = repo.join("nested");
        assert_eq!(f.boundary.known_root(&s(&repo)), Some(repo.clone()));
        assert_eq!(f.boundary.known_root(&s(&nested)), Some(nested.clone()));
        assert_eq!(
            f.boundary
                .resolve_in_root(&s(&repo), &s(&nested.join("deep")))
                .unwrap(),
            nested.join("deep"),
            "an outer root still owns a nested path"
        );
        assert_eq!(
            f.boundary
                .resolve_in_root(&s(&nested), &s(&nested.join("deep")))
                .unwrap(),
            nested.join("deep")
        );
        assert_eq!(
            f.boundary
                .resolve_in_root(&s(&f.home.join("projects")), &s(&nested.join("deep"))),
            Err(Refusal::OutsideCheckout),
            "a root the event named has to be registered"
        );
        assert_eq!(
            f.boundary
                .resolve_in_root(&s(&repo), &s(&f.home.join("projects/notes.txt"))),
            Err(Refusal::OutsideCheckout)
        );
    }

    #[test]
    fn the_root_set_is_replaced_wholesale() {
        let f = rooted();
        let repo = f.home.join("projects/alpha");
        f.boundary
            .set_roots(vec![root("w9", "c9", &f.home.join("projects/ghost"))]);
        assert_eq!(
            f.boundary.known_root(&s(&repo)),
            None,
            "a root that is not an existing directory is dropped"
        );
        assert_eq!(
            f.boundary.resolve_target(&s(&repo.join("src/main.rs"))),
            Err(Refusal::OutsideCheckout)
        );
        f.boundary.set_roots(Vec::new());
        assert_eq!(
            f.boundary.resolve_target(&s(&repo)),
            Err(Refusal::OutsideCheckout)
        );
    }

    #[test]
    fn a_checkout_pair_owns_the_paths_under_its_root() {
        let f = rooted();
        let repo = f.home.join("projects/alpha");
        let nested = repo.join("nested");
        assert_eq!(
            f.boundary
                .resolve_checkout("w1", "c2", &s(&nested.join("deep")))
                .unwrap(),
            nested.join("deep")
        );
        assert_eq!(
            f.boundary
                .resolve_checkout("w1", "c2", &s(&repo.join("src/main.rs"))),
            Err(Refusal::OutsideCheckout),
            "the named checkout does not own a path outside its root"
        );
        assert_eq!(
            f.boundary
                .resolve_checkout("w1", "c1", &s(&nested.join("deep")))
                .unwrap(),
            nested.join("deep"),
            "the outer checkout owns the nested path too"
        );
        assert_eq!(
            f.boundary
                .resolve_checkout("w9", "c9", &s(&repo.join("src/main.rs")))
                .unwrap(),
            repo.join("src/main.rs"),
            "a pair with no root falls back to any root; the core names the unknown checkout"
        );
        assert_eq!(
            f.boundary
                .resolve_checkout("w9", "c9", &s(&f.home.join("projects/notes.txt"))),
            Err(Refusal::OutsideCheckout)
        );
    }

    #[test]
    fn a_save_is_checked_without_being_respelled() {
        let f = rooted();
        let repo = f.home.join("projects/alpha");
        assert!(f.boundary.is_under_root(&s(&repo)));
        assert!(f.boundary.is_under_root(&s(&repo.join("src/main.rs"))));
        assert!(
            !f.boundary
                .is_under_root(&s(&f.home.join("projects/notes.txt")))
        );
        assert!(
            !f.boundary.is_under_root(&s(&repo.join("src/../main.rs"))),
            "a path that names its way around is not a path this line reads"
        );
        assert!(!f.boundary.is_under_root("/etc/passwd"));
        assert!(!f.boundary.is_under_root("relative"));
        assert!(!f.boundary.is_under_root(""));
    }

    #[test]
    fn a_home_relative_path_is_the_home_directory_on_both_lines() {
        let f = rooted();
        let repo = f.home.join("projects/alpha");
        assert_eq!(f.boundary.resolve_target("~/projects/alpha").unwrap(), repo);
        assert_eq!(
            f.boundary
                .resolve_target("~/projects/alpha/src/main.rs")
                .unwrap(),
            repo.join("src/main.rs")
        );
        assert_eq!(
            f.boundary.resolve_target("~"),
            Err(Refusal::OutsideCheckout),
            "home is not a checkout unless one is registered at it"
        );
        assert_eq!(f.boundary.resolve_dir("~").unwrap(), f.home);
        assert_eq!(
            f.boundary.resolve_target("~user/projects/alpha"),
            Err(Refusal::InvalidPath)
        );
    }

    #[test]
    fn names_are_gated_by_shape() {
        for name in ["main.rs", "a b", ".hidden", "한글.txt"] {
            assert!(valid_name(name), "{name}");
        }
        for name in ["", ".", "..", "a/b", "\0"] {
            assert!(!valid_name(name), "{name:?}");
        }
    }
}
