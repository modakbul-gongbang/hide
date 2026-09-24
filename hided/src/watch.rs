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
use std::time::{Duration, Instant, SystemTime};

use cap_std::fs::Dir;
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
        opened: Vec<(PathBuf, File)>,
    },
}

struct Watched {
    directory: Dir,
    modified: SystemTime,
    dirty: Option<Instant>,
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
                    })
                    .collect()
            })
            .unwrap_or_default();
        let actual = opened.iter().map(|(path, _)| path.clone()).collect();
        if self
            .commands
            .send(Command::Reconcile {
                root: root.map(PathBuf::from),
                opened,
            })
            .is_ok()
        {
            *self.requested.lock().expect("watch request set") = actual;
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
                    Command::Reconcile { root: selected, opened } => {
                        root = selected;
                        reconcile(&mut watched, opened);
                    }
                }
            }
            _ = interval.tick() => {
                if root.as_ref().is_some_and(|root| boundary.known_root(&root.to_string_lossy()).is_none()) {
                    watched.clear();
                    requested.lock().expect("watch request set").clear();
                    continue;
                }
                for path in poll(&mut watched, Instant::now()) {
                    if boundary.resolve_target(&path.to_string_lossy()).is_ok() {
                        let _ = frames.send(frame(&path));
                    } else {
                        watched.remove(&path);
                    }
                }
                requested.lock().expect("watch request set").retain(|path| watched.contains_key(path));
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
fn reconcile(watched: &mut HashMap<PathBuf, Watched>, opened: Vec<(PathBuf, File)>) {
    let keep: HashSet<PathBuf> = opened
        .iter()
        .take(WATCH_CAP)
        .map(|(path, _)| path.clone())
        .collect();
    watched.retain(|path, _| keep.contains(path));
    for (path, file) in opened.into_iter().take(WATCH_CAP) {
        if watched.contains_key(&path) {
            continue;
        }
        let directory = Dir::from_std_file(file);
        if let Ok(modified) = directory
            .dir_metadata()
            .and_then(|metadata| metadata.modified())
        {
            watched.insert(
                path,
                Watched {
                    directory,
                    modified,
                    dirty: None,
                },
            );
        }
    }
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
        reconcile(
            &mut watched,
            vec![(root.clone(), File::open(&root).unwrap())],
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
}
