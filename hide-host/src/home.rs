//! Hide's Home folder on one machine: `~/hide`, one symlink per registered
//! project of that machine, plus a Hide-owned `AGENTS.md` and a `CLAUDE.md`
//! that links to it.
//!
//! [`sync`] converges the folder on the projects it is given and is safe to
//! repeat: a rerun with the same projects changes nothing but `AGENTS.md`'s
//! own bytes. Hide only ever removes what it made. The marker file records
//! every link Hide made (`name -> target`); an entry is removed only while it
//! is still a symlink to the recorded target, and a folder, file or link the
//! operator put there is never touched, only forgotten. The core calls this in
//! process for this Mac and `Call::HomeSync` on a device's helper.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, HostError, HostResult};

/// The Home folder's name inside the account's home directory.
pub const HOME_DIR_NAME: &str = "hide";
/// Present in a Home folder Hide made: which links are Hide's.
pub const MARKER_FILE: &str = ".hide-home.json";
/// The most project links one Home holds.
pub const MAX_LINKS: usize = 256;

const AGENTS_FILE: &str = "AGENTS.md";
const CLAUDE_FILE: &str = "CLAUDE.md";
const MARKER_VERSION: u32 = 1;

/// The Home folder after a sync.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HomeSynced {
    /// Absolute, canonical path of `<user home>/hide`.
    pub home: String,
    /// This call made the Home folder.
    pub created: bool,
    /// Every link Hide manages after the sync, sorted by name.
    pub links: Vec<HomeLink>,
    /// Managed links this sync removed.
    pub dropped: Vec<HomeDrop>,
    /// Requested projects that got no link.
    pub skipped: Vec<HomeSkip>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HomeLink {
    pub name: String,
    pub target: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HomeDrop {
    pub name: String,
    pub target: String,
    /// `unregistered` (the project left the list) or `dangling` (its folder
    /// is gone).
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HomeSkip {
    pub target: String,
    /// `missing` (not a folder now), `name_taken` (something of the
    /// operator's holds the link's name), `not_absolute`, or `no_name` (the
    /// path has no final component).
    pub reason: String,
}

/// The links Hide made, as the marker file stores them.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Marker {
    version: u32,
    links: BTreeMap<String, String>,
}

impl Marker {
    fn empty() -> Self {
        Self {
            version: MARKER_VERSION,
            links: BTreeMap::new(),
        }
    }
}

fn not_home() -> HostError {
    HostError::new(
        ErrorCode::HomeConflict,
        "~/hide already exists and is not Hide's Home. Rename or move it, then open Home again.",
    )
}

fn unreadable_marker() -> HostError {
    HostError::new(
        ErrorCode::HomeConflict,
        "Hide's Home marker (~/hide/.hide-home.json) is unreadable, so Hide cannot tell which links it made. Rename or move ~/hide, then open Home again.",
    )
}

fn io_error(error: &std::io::Error, what: impl std::fmt::Display) -> HostError {
    HostError::io(error, format!("{what}: {error}"))
}

fn reserved(name: &str) -> bool {
    matches!(name, AGENTS_FILE | CLAUDE_FILE | MARKER_FILE)
}

/// Makes `<user_home>/hide` hold one link per project in `projects` (see the
/// module documentation). Nothing is written when the answer is an error from
/// the checks before the first write: a folder that is not Hide's Home, an
/// unreadable marker, or more than [`MAX_LINKS`] projects.
pub fn sync(user_home: &Path, projects: &[String]) -> HostResult<HomeSynced> {
    let home_path = user_home.join(HOME_DIR_NAME);
    let existing = read_marker(&home_path)?;

    let mut skipped = Vec::new();
    let candidates = candidates(projects, &mut skipped)?;
    let desired = name_projects(candidates, &mut skipped);

    let (created, on_disk) = match existing {
        Some(marker) => (false, marker),
        None => {
            create_home(user_home, &home_path)?;
            (true, Marker::empty())
        }
    };

    let desired_names: BTreeSet<&str> = desired.iter().map(|(name, _)| name.as_str()).collect();
    let mut dropped = Vec::new();
    let mut next = BTreeMap::new();

    // Links Hide made that no desired project uses now.
    for (name, recorded) in &on_disk.links {
        if desired_names.contains(name.as_str()) {
            continue;
        }
        let entry = home_path.join(name);
        if is_link_to(&entry, recorded) {
            std::fs::remove_file(&entry).map_err(|error| {
                io_error(&error, format!("The link {name} could not be removed"))
            })?;
            let reason = if is_directory(Path::new(recorded)) {
                "unregistered"
            } else {
                "dangling"
            };
            dropped.push(HomeDrop {
                name: name.clone(),
                target: recorded.clone(),
                reason: reason.to_owned(),
            });
        }
        // Anything else at the name is the operator's now: forgotten, kept.
    }

    // Decide every desired link before the first write, so the marker can
    // record the links about to be made: a crash between making a link and
    // recording it would otherwise leave a link nobody owns.
    let mut plan = Vec::new();
    for (name, target) in &desired {
        let entry = home_path.join(name);
        let target_text = target.to_string_lossy().into_owned();
        let recorded = on_disk.links.get(name);
        let action = match std::fs::symlink_metadata(&entry) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Action::Create,
            Err(error) => {
                return Err(io_error(&error, format!("{name} could not be read")));
            }
            Ok(_) if is_link_to(&entry, &target_text) && recorded.is_some() => Action::Keep,
            Ok(_) if recorded.is_some_and(|recorded| is_link_to(&entry, recorded)) => {
                Action::Replace
            }
            Ok(_) => Action::Taken,
        };
        plan.push((name, target, target_text, action));
    }

    let mut before_writes = on_disk.links.clone();
    for (name, _, target_text, action) in &plan {
        if matches!(action, Action::Create) {
            before_writes.insert((*name).clone(), target_text.clone());
        }
    }
    let mut written = on_disk;
    if before_writes != written.links {
        written.links = before_writes;
        write_marker(&home_path, &written)?;
    }

    for (name, target, target_text, action) in plan {
        let entry = home_path.join(name);
        match action {
            Action::Keep => {}
            Action::Create => make_link(target, &entry, name)?,
            Action::Replace => {
                std::fs::remove_file(&entry).map_err(|error| {
                    io_error(&error, format!("The link {name} could not be replaced"))
                })?;
                make_link(target, &entry, name)?;
            }
            Action::Taken => {
                skipped.push(HomeSkip {
                    target: target_text,
                    reason: "name_taken".to_owned(),
                });
                continue;
            }
        }
        next.insert(name.clone(), target_text);
    }

    if next != written.links {
        written.links = next.clone();
        write_marker(&home_path, &written)?;
    }

    let home = home_path
        .canonicalize()
        .map_err(|error| io_error(&error, "Hide's Home cannot be read"))?;
    let links = next
        .into_iter()
        .map(|(name, target)| HomeLink { name, target })
        .collect::<Vec<_>>();
    write_atomic(&home, AGENTS_FILE, agents_text(&links).as_bytes())?;
    link_claude_file(&home)?;

    Ok(HomeSynced {
        home: home.to_string_lossy().into_owned(),
        created,
        links,
        dropped,
        skipped,
    })
}

enum Action {
    /// Already Hide's link to the project.
    Keep,
    Create,
    /// Hide's link, still as Hide made it, but the project's folder changed.
    Replace,
    /// The name holds something Hide did not make, or a link the operator
    /// changed.
    Taken,
}

/// The marker of an existing Home, or `None` when there is no `~/hide` yet.
fn read_marker(home_path: &Path) -> HostResult<Option<Marker>> {
    let metadata = match std::fs::symlink_metadata(home_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(&error, "~/hide could not be read")),
        Ok(metadata) => metadata,
    };
    if !metadata.is_dir() {
        return Err(not_home());
    }
    let marker_path = home_path.join(MARKER_FILE);
    match std::fs::symlink_metadata(&marker_path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(not_home()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(not_home()),
        Err(error) => return Err(io_error(&error, "~/hide could not be read")),
    }
    let bytes = std::fs::read(&marker_path).map_err(|_| unreadable_marker())?;
    let marker: Marker = serde_json::from_slice(&bytes).map_err(|_| unreadable_marker())?;
    // A recorded name is joined onto the Home folder to remove it; one that
    // could leave the folder or name Hide's own files is not a marker of ours.
    let unsafe_name = |name: &String| {
        name.is_empty()
            || name == "."
            || name == ".."
            || name.contains('/')
            || name.contains('\0')
            || reserved(name)
    };
    if marker.version != MARKER_VERSION || marker.links.keys().any(unsafe_name) {
        return Err(unreadable_marker());
    }
    Ok(Some(marker))
}

/// The absolute, normalized, deduplicated projects worth a look, in path
/// order. A path that cannot be linked is answered as skipped.
fn candidates(projects: &[String], skipped: &mut Vec<HomeSkip>) -> HostResult<BTreeSet<PathBuf>> {
    let requested: BTreeSet<&String> = projects.iter().collect();
    let mut found = BTreeSet::new();
    for project in requested {
        let path = Path::new(project);
        if !path.is_absolute() {
            skipped.push(HomeSkip {
                target: project.clone(),
                reason: "not_absolute".to_owned(),
            });
            continue;
        }
        found.insert(path.components().collect::<PathBuf>());
    }
    if found.len() > MAX_LINKS {
        return Err(HostError::new(
            ErrorCode::TooLarge,
            format!(
                "Home holds at most {MAX_LINKS} project links, and {} projects were asked for",
                found.len()
            ),
        ));
    }
    Ok(found)
}

/// A link name per existing project: the folder's name, or with its parent's
/// name when several projects share it, and a number when that still repeats.
fn name_projects(
    candidates: BTreeSet<PathBuf>,
    skipped: &mut Vec<HomeSkip>,
) -> Vec<(String, PathBuf)> {
    let mut named = Vec::new();
    let mut shared: BTreeMap<String, usize> = BTreeMap::new();
    for path in candidates {
        let target = path.to_string_lossy().into_owned();
        let Some(base) = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            skipped.push(HomeSkip {
                target,
                reason: "no_name".to_owned(),
            });
            continue;
        };
        if !is_directory(&path) {
            skipped.push(HomeSkip {
                target,
                reason: "missing".to_owned(),
            });
            continue;
        }
        *shared.entry(base.clone()).or_default() += 1;
        named.push((base, path));
    }

    let mut used: BTreeSet<String> = [AGENTS_FILE, CLAUDE_FILE, MARKER_FILE]
        .into_iter()
        .map(str::to_owned)
        .collect();
    named
        .into_iter()
        .map(|(base, path)| {
            let wanted = match path
                .parent()
                .and_then(Path::file_name)
                .filter(|_| shared[&base] > 1)
            {
                Some(parent) => format!("{base}-{}", parent.to_string_lossy()),
                None => base,
            };
            let mut name = wanted.clone();
            let mut number = 2;
            while !used.insert(name.clone()) {
                name = format!("{wanted}-{number}");
                number += 1;
            }
            (name, path)
        })
        .collect()
}

/// Makes `~/hide` holding its marker in one rename, so a folder Hide made is
/// never seen without the marker that says it is Hide's.
fn create_home(user_home: &Path, home_path: &Path) -> HostResult<()> {
    let staging = tempfile::Builder::new()
        .prefix(".hide-home-")
        .tempdir_in(user_home)
        .map_err(|error| io_error(&error, "~/hide could not be created"))?;
    set_readable(staging.path(), 0o755)?;
    write_marker(staging.path(), &Marker::empty())?;
    // A rename onto an empty folder would replace it; check the name is free
    // as late as possible.
    if std::fs::symlink_metadata(home_path).is_ok() {
        return Err(not_home());
    }
    let staged = staging.keep();
    std::fs::rename(&staged, home_path).map_err(|error| {
        let _ = std::fs::remove_dir_all(&staged);
        io_error(&error, "~/hide could not be created")
    })
}

fn write_marker(home: &Path, marker: &Marker) -> HostResult<()> {
    let bytes = serde_json::to_vec_pretty(marker).map_err(|error| {
        HostError::new(
            ErrorCode::Io,
            format!("Hide's Home marker could not be encoded: {error}"),
        )
    })?;
    write_atomic(home, MARKER_FILE, &bytes)
}

/// Writes `name` in `dir` whole or not at all.
fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> HostResult<()> {
    let what = format!("~/hide/{name} could not be written");
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|error| io_error(&error, &what))?;
    file.write_all(bytes)
        .and_then(|()| file.flush())
        .map_err(|error| io_error(&error, &what))?;
    set_readable(file.path(), 0o644)?;
    file.persist(dir.join(name))
        .map_err(|error| io_error(&error.error, &what))?;
    Ok(())
}

fn set_readable(path: &Path, mode: u32) -> HostResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|error| io_error(&error, "~/hide could not be prepared"))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// `CLAUDE.md` links to `AGENTS.md` unless something is there already, which
/// is left as it is.
fn link_claude_file(home: &Path) -> HostResult<()> {
    let entry = home.join(CLAUDE_FILE);
    match std::fs::symlink_metadata(&entry) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            make_link(Path::new(AGENTS_FILE), &entry, CLAUDE_FILE)
        }
        _ => Ok(()),
    }
}

fn make_link(target: &Path, entry: &Path, name: &str) -> HostResult<()> {
    symlink(target, entry)
        .map_err(|error| io_error(&error, format!("The link {name} could not be made")))
}

#[cfg(unix)]
fn symlink(target: &Path, entry: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, entry)
}

#[cfg(not(unix))]
fn symlink(_target: &Path, _entry: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "links are not supported on this platform",
    ))
}

/// Whether `entry` is itself a symlink to exactly `target`.
fn is_link_to(entry: &Path, target: &str) -> bool {
    std::fs::symlink_metadata(entry).is_ok_and(|metadata| metadata.file_type().is_symlink())
        && std::fs::read_link(entry).is_ok_and(|link| link == Path::new(target))
}

/// A directory now, following links.
fn is_directory(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

fn agents_text(links: &[HomeLink]) -> String {
    let mut text = String::from(
        "# Hide Home\n\
         \n\
         This folder is Hide's Home on this device.\n\
         Each entry is a link to a project registered in Hide.\n\
         \n",
    );
    if links.is_empty() {
        text.push_str("No projects are registered on this device yet.\n");
    }
    for link in links {
        text.push_str(&format!("- {} -> {}\n", link.name, link.target));
    }
    text.push_str(
        "\n\
         Work inside a project follows that project's own AGENTS.md or CLAUDE.md.\n\
         Work that belongs to no project can live here.\n\
         Hide rewrites this file when projects change, so edits here are not kept.\n",
    );
    text
}
