//! The filesystem watch behind the Explorer's live refresh (PRD B2, D-04,
//! D-08).
//!
//! One watch set per daemon, driven by the core's own state: the focused
//! checkout's root and the folders it reports as expanded, most recently
//! expanded last. A change in a watched folder is announced to every client as
//! a `directory_changed` frame, which makes the client re-read that one folder;
//! a folder past the cap is not watched, and the client draws the refresh badge
//! on its row instead.
//!
//! The resource is bounded on purpose (engineering 14 and 15): at most
//! `WATCH_CAP` folders are watched at once, the least recently expanded are
//! released first, and the opened handles are owned by one task that ends with the
//! daemon. The same cap is why the web's own listing cache holds the same
//! number: the two sides agree on which folders are live without a second
//! protocol.
//! Directory metadata is polled from the retained handle, never from a path
//! that may have been replaced after registration.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cap_std::fs::Dir;
use cap_std::time::SystemTime;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{broadcast, mpsc};

/// Folders watched at once, the checkout root included. The web's listing
/// cache uses the same number (`web/src/watch.ts`).
pub const WATCH_CAP: usize = 64;

/// One folder's changes within this window collapse into a single frame, so a
/// burst (a git checkout, an editor's atomic save) is one client re-read.
const COALESCE: Duration = Duration::from_millis(200);
const POLL: Duration = Duration::from_millis(200);

/// The folders to watch for a checkout: its root, then the most recently
/// expanded folders under it, at most `WATCH_CAP` in total. A path under a
/// sibling checkout is not this checkout's, and the root is always first so a
/// deeply expanded tree never drops the tree's own base.
pub fn watched_folders(root: &str, expanded: &[String]) -> Vec<PathBuf> {
    let prefix = format!("{root}/");
    let under: Vec<&String> = expanded
        .iter()
        .filter(|path| path.as_str() == root || path.starts_with(&prefix))
        .collect();
    let room = WATCH_CAP.saturating_sub(1);
    let recent = if under.len() > room {
        &under[under.len() - room..]
    } else {
        &under[..]
    };
    let mut folders = vec![PathBuf::from(root)];
    for path in recent {
        if path.as_str() != root {
            folders.push(PathBuf::from(path));
        }
    }
    folders
}

enum Command {
    Reconcile {
        root: Option<PathBuf>,
        desired: Vec<PathBuf>,
        opened: Vec<(PathBuf, File)>,
    },
}

#[cfg(unix)]
type DirectoryIdentity = (u64, u64);
#[cfg(windows)]
type DirectoryIdentity = (u32, u64);
#[cfg(not(any(unix, windows)))]
type DirectoryIdentity = ();

struct Watched {
    directory: Dir,
    identity: DirectoryIdentity,
    modified: SystemTime,
    dirty: Option<Instant>,
}

fn directory_identity(metadata: &cap_std::fs::Metadata) -> Option<DirectoryIdentity> {
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        Some((metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        metadata.volume_serial_number().zip(metadata.file_index())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        None
    }
}

impl Watched {
    fn from_file(file: File) -> std::io::Result<Self> {
        let directory = Dir::from_std_file(file);
        let metadata = directory.dir_metadata()?;
        let identity = directory_identity(&metadata).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "directory identity unavailable",
            )
        })?;
        Ok(Self {
            directory,
            identity,
            modified: metadata.modified()?,
            dirty: None,
        })
    }
}

/// The daemon's one watch service: a command channel into the owning task and
/// the broadcast every client subscribes to.
pub struct WatchService {
    commands: UnboundedSender<Command>,
    frames: broadcast::Sender<String>,
    requested: Arc<Mutex<Vec<PathBuf>>>,
}

impl WatchService {
    pub fn new(boundary: Arc<crate::boundary::Boundary>) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (frames, _) = broadcast::channel(64);
        let requested = Arc::new(Mutex::new(Vec::new()));
        tokio::spawn(run(
            receiver,
            frames.clone(),
            Arc::clone(&requested),
            boundary,
        ));
        Self {
            commands,
            frames,
            requested,
        }
    }

    /// Announces `root`'s folders as the ones to watch; the task diffs them
    /// against what it already watches, so a repeat reconcile is cheap.
    pub fn reconcile(
        &self,
        boundary: &crate::boundary::Boundary,
        root: Option<String>,
        expanded: Vec<String>,
    ) {
        let desired = root
            .as_deref()
            .map(|root| watched_folders(root, &expanded))
            .unwrap_or_default();
        if *self.requested.lock().expect("watch request set") == desired {
            return;
        }
        let opened: Vec<_> = root
            .as_deref()
            .map(|root| {
                desired
                    .iter()
                    .filter_map(|path| {
                        boundary
                            .open_directory(std::path::Path::new(root), &path.to_string_lossy())
                            .ok()
                            .map(|(_, file)| (path.clone(), file))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if self
            .commands
            .send(Command::Reconcile {
                root: root.map(PathBuf::from),
                desired: desired.clone(),
                opened,
            })
            .is_ok()
        {
            *self.requested.lock().expect("watch request set") = desired;
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.frames.subscribe()
    }
}

fn frame(path: &std::path::Path) -> String {
    serde_json::json!({
        "type": "directory_changed",
        "payload": {"path": path.display().to_string()},
    })
    .to_string()
}

async fn run(
    mut commands: mpsc::UnboundedReceiver<Command>,
    frames: broadcast::Sender<String>,
    requested: Arc<Mutex<Vec<PathBuf>>>,
    boundary: Arc<crate::boundary::Boundary>,
) {
    let mut watched: HashMap<PathBuf, Watched> = HashMap::new();
    let mut root: Option<PathBuf> = None;
    let mut interval = tokio::time::interval(POLL);
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { return };
                match command {
                    Command::Reconcile { root: selected, desired, opened } => {
                        root = selected;
                        *requested.lock().expect("watch request set") = desired;
                        for path in reconcile(&mut watched, opened) {
                            let _ = frames.send(frame(&path));
                        }
                    }
                }
            }
            _ = interval.tick() => {
                if root.as_ref().is_some_and(|root| boundary.known_root(&root.to_string_lossy()).is_none()) {
                    watched.clear();
                    requested.lock().expect("watch request set").clear();
                    continue;
                }
                let changed = poll(&mut watched, Instant::now());
                let desired = requested.lock().expect("watch request set").clone();
                let selected = root.as_ref();
                let mut emitted = HashSet::new();
                for path in changed {
                    if boundary.resolve_target(&path.to_string_lossy()).is_ok() {
                        if emitted.insert(path.clone()) {
                            let _ = frames.send(frame(&path));
                        }
                        if let Some(root) = selected {
                            for rebound in refresh_on_change(&boundary, root, &desired, &mut watched, &path) {
                                if emitted.insert(rebound.clone()) {
                                    let _ = frames.send(frame(&rebound));
                                }
                            }
                        }
                    } else {
                        watched.remove(&path);
                    }
                }
            }
        }
    }
}

fn poll(watched: &mut HashMap<PathBuf, Watched>, now: Instant) -> Vec<PathBuf> {
    let mut changed = Vec::new();
    let mut failed = Vec::new();
    for (path, watch) in watched.iter_mut() {
        match watch
            .directory
            .dir_metadata()
            .and_then(|metadata| metadata.modified())
        {
            Ok(modified) if modified != watch.modified => {
                watch.modified = modified;
                watch.dirty = Some(now);
            }
            Err(error) => {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "hided", "kind": "watch.read_failed",
                        "path": path, "message": error.to_string()
                    })
                );
                failed.push(path.clone());
                changed.push(path.clone());
                continue;
            }
            _ => {}
        }
        if watch
            .dirty
            .is_some_and(|since| now.duration_since(since) >= COALESCE)
        {
            changed.push(path.clone());
            watch.dirty = None;
        }
    }
    for path in failed {
        watched.remove(&path);
    }
    changed
}

/// Retain existing handles for unchanged folders, and own each new handle
/// until that folder leaves the capped set or the daemon ends.
fn reconcile(
    watched: &mut HashMap<PathBuf, Watched>,
    opened: Vec<(PathBuf, File)>,
) -> Vec<PathBuf> {
    let keep: HashSet<PathBuf> = opened
        .iter()
        .take(WATCH_CAP)
        .map(|(path, _)| path.clone())
        .collect();
    watched.retain(|path, _| keep.contains(path));
    let mut replaced = Vec::new();
    for (path, file) in opened.into_iter().take(WATCH_CAP) {
        if let Ok(next) = Watched::from_file(file) {
            match watched.get(&path) {
                Some(current) if current.identity == next.identity => continue,
                Some(_) => replaced.push(path.clone()),
                None => {}
            }
            watched.insert(path, next);
        }
    }
    replaced
}

/// A changed parent can replace a watched child's pathname without changing
/// the old child's opened handle. Re-open only its capped descendants through
/// the registered boundary and notify the client when identity changes.
fn refresh_on_change(
    boundary: &crate::boundary::Boundary,
    root: &std::path::Path,
    desired: &[PathBuf],
    watched: &mut HashMap<PathBuf, Watched>,
    changed: &std::path::Path,
) -> Vec<PathBuf> {
    let mut rebound = Vec::new();
    for path in desired
        .iter()
        .take(WATCH_CAP)
        .filter(|path| path.starts_with(changed))
    {
        let opened = boundary.open_directory(root, &path.to_string_lossy());
        let Ok((_, file)) = opened else {
            if watched.remove(path).is_some() {
                rebound.push(path.clone());
            }
            continue;
        };
        let Ok(next) = Watched::from_file(file) else {
            continue;
        };
        if watched
            .get(path)
            .is_some_and(|current| current.identity == next.identity)
        {
            continue;
        }
        watched.insert(path.clone(), next);
        rebound.push(path.clone());
    }
    rebound
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn the_root_comes_first_and_is_kept_past_the_cap() {
        let root = "/repo";
        let expanded = paths(&["/repo/a", "/repo/b", "/elsewhere/c"]);
        assert_eq!(
            watched_folders(root, &expanded),
            vec![
                PathBuf::from("/repo"),
                PathBuf::from("/repo/a"),
                PathBuf::from("/repo/b")
            ],
            "another checkout's folder is not this checkout's to watch"
        );
    }

    #[test]
    fn the_cap_releases_the_least_recently_expanded_folders() {
        let root = "/repo";
        let mut expanded: Vec<String> = (0..(WATCH_CAP + 10))
            .map(|index| format!("/repo/f{index:03}"))
            .collect();
        expanded.push("/repo/recent".to_owned());
        let folders = watched_folders(root, &expanded);
        assert_eq!(folders.len(), WATCH_CAP);
        assert_eq!(folders.first(), Some(&PathBuf::from("/repo")));
        assert_eq!(folders.last(), Some(&PathBuf::from("/repo/recent")));
        assert!(
            !folders.contains(&PathBuf::from("/repo/f000")),
            "the oldest expanded folder is released first"
        );
    }

    #[test]
    fn a_repeated_root_is_listed_once() {
        let root = "/repo";
        let expanded = paths(&["/repo", "/repo/a"]);
        assert_eq!(
            watched_folders(root, &expanded),
            vec![PathBuf::from("/repo"), PathBuf::from("/repo/a")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn polling_stays_on_the_opened_directory_after_its_path_is_replaced() {
        use std::os::unix::fs::symlink;
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("checkout");
        let outside = sandbox.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let mut watched = HashMap::new();
        assert!(
            reconcile(
                &mut watched,
                vec![(root.clone(), File::open(&root).unwrap())],
            )
            .is_empty()
        );
        std::fs::rename(&root, sandbox.path().join("moved")).unwrap();
        symlink(&outside, &root).unwrap();
        std::fs::write(outside.join("other.txt"), "outside").unwrap();
        assert!(poll(&mut watched, Instant::now()).is_empty());
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(sandbox.path().join("moved/inside.txt"), "inside").unwrap();
        let now = Instant::now();
        assert!(poll(&mut watched, now).is_empty());
        assert_eq!(poll(&mut watched, now + COALESCE), vec![root]);
    }

    #[cfg(unix)]
    #[test]
    fn replacement_of_an_expanded_folder_rebinds_and_reports_its_new_child() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("checkout");
        let expanded = root.join("expanded");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&expanded).unwrap();
        let boundary = crate::boundary::Boundary::new(sandbox.path()).unwrap();
        boundary.set_roots(vec![crate::boundary::Root {
            workspace_id: "w".to_owned(),
            checkout_id: "c".to_owned(),
            path: root.clone(),
        }]);
        let desired = vec![root.clone(), expanded.clone()];
        let opened = desired
            .iter()
            .map(|path| {
                boundary
                    .open_directory(&root, &path.to_string_lossy())
                    .unwrap()
            })
            .collect();
        let mut watched = HashMap::new();
        assert!(reconcile(&mut watched, opened).is_empty());
        assert!(
            refresh_on_change(&boundary, &root, &desired, &mut watched, &root).is_empty(),
            "unchanged handles do not send another refresh"
        );

        std::fs::rename(&expanded, root.join("old-expanded")).unwrap();
        std::fs::create_dir(&expanded).unwrap();
        assert_eq!(
            refresh_on_change(&boundary, &root, &desired, &mut watched, &root),
            vec![expanded.clone()],
            "the replacement invalidates its cached listing"
        );
        std::fs::write(expanded.join("new-child.txt"), "new").unwrap();
        let now = Instant::now();
        assert!(poll(&mut watched, now).is_empty());
        assert!(
            poll(&mut watched, now + COALESCE).contains(&expanded),
            "subsequent writes in the replacement folder stay live"
        );
    }
}
