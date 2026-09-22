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
//! released first, and the watcher is owned by one task that ends with the
//! daemon. The same cap is why the web's own listing cache holds the same
//! number: the two sides agree on which folders are live without a second
//! protocol.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{broadcast, mpsc};

/// Folders watched at once, the checkout root included. The web's listing
/// cache uses the same number (`web/src/watch.ts`).
pub const WATCH_CAP: usize = 64;

/// One folder's changes within this window collapse into a single frame, so a
/// burst (a git checkout, an editor's atomic save) is one client re-read.
const COALESCE: Duration = Duration::from_millis(200);

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
        root: Option<String>,
        expanded: Vec<String>,
    },
}

/// The daemon's one watch service: a command channel into the owning task and
/// the broadcast every client subscribes to.
pub struct WatchService {
    commands: UnboundedSender<Command>,
    frames: broadcast::Sender<String>,
}

impl WatchService {
    pub fn new() -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (frames, _) = broadcast::channel(64);
        tokio::spawn(run(receiver, frames.clone()));
        Self { commands, frames }
    }

    /// Announces `root`'s folders as the ones to watch; the task diffs them
    /// against what it already watches, so a repeat reconcile is cheap.
    pub fn reconcile(&self, root: Option<String>, expanded: Vec<String>) {
        let _ = self.commands.send(Command::Reconcile { root, expanded });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.frames.subscribe()
    }
}

impl Default for WatchService {
    fn default() -> Self {
        Self::new()
    }
}

fn frame(path: &std::path::Path) -> String {
    serde_json::json!({
        "type": "directory_changed",
        "payload": {"path": path.display().to_string()},
    })
    .to_string()
}

async fn run(mut commands: mpsc::UnboundedReceiver<Command>, frames: broadcast::Sender<String>) {
    let (raw_tx, mut raw_rx) = mpsc::unbounded_channel();
    let mut watcher = match notify::recommended_watcher(move |result| {
        let _ = raw_tx.send(result);
    }) {
        Ok(watcher) => watcher,
        Err(error) => {
            eprintln!(
                "{}",
                serde_json::json!({
                    "component": "hided",
                    "kind": "watch.unavailable",
                    "message": error.to_string(),
                })
            );
            return;
        }
    };
    let mut watched: HashMap<PathBuf, ()> = HashMap::new();
    // A folder with an event but no frame yet, and the time of its newest
    // event. One task per folder waits for the burst to go quiet and then
    // publishes once, so a git checkout is one client re-read (B16).
    let dirty: Arc<Mutex<HashMap<PathBuf, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { return };
                match command {
                    Command::Reconcile { root, expanded } => {
                        reconcile(&mut watcher, &mut watched, root.as_deref(), &expanded);
                    }
                }
            }
            event = raw_rx.recv() => {
                let Some(Ok(event)) = event else { continue };
                for path in event.paths {
                    let folder = if watched.contains_key(&path) {
                        Some(path.clone())
                    } else {
                        path.parent().map(PathBuf::from)
                    };
                    let Some(folder) = folder.filter(|folder| watched.contains_key(folder)) else {
                        continue;
                    };
                    let first = dirty
                        .lock()
                        .expect("watch dirty set")
                        .insert(folder.clone(), Instant::now())
                        .is_none();
                    if first {
                        tokio::spawn(settle(folder, Arc::clone(&dirty), frames.clone()));
                    }
                }
            }
        }
    }
}

/// Waits until `folder`'s events stop, then publishes one frame for it and
/// clears its dirty entry. A later event that arrives during the wait extends
/// it rather than starting a second waiter, so the burst still sends once.
async fn settle(
    folder: PathBuf,
    dirty: Arc<Mutex<HashMap<PathBuf, Instant>>>,
    frames: broadcast::Sender<String>,
) {
    loop {
        let remaining = {
            let guard = dirty.lock().expect("watch dirty set");
            match guard.get(&folder) {
                Some(since) => COALESCE.saturating_sub(since.elapsed()),
                None => return,
            }
        };
        if remaining.is_zero() {
            let removed = dirty.lock().expect("watch dirty set").remove(&folder);
            if removed.is_some() {
                let _ = frames.send(frame(&folder));
            }
            return;
        }
        tokio::time::sleep(remaining).await;
    }
}

/// Diffs the desired watch set against the current one: released folders are
/// unwatched, new ones are watched, and one that no longer exists is skipped
/// rather than retried forever.
fn reconcile(
    watcher: &mut RecommendedWatcher,
    watched: &mut HashMap<PathBuf, ()>,
    root: Option<&str>,
    expanded: &[String],
) {
    let desired: Vec<PathBuf> = match root {
        Some(root) => watched_folders(root, expanded),
        None => Vec::new(),
    };
    let desired: Vec<PathBuf> = desired.into_iter().filter(|path| path.is_dir()).collect();
    let keep: std::collections::HashSet<&PathBuf> = desired.iter().collect();
    let stale: Vec<PathBuf> = watched
        .keys()
        .filter(|path| !keep.contains(path))
        .cloned()
        .collect();
    for path in stale {
        let _ = watcher.unwatch(&path);
        watched.remove(&path);
    }
    for path in desired {
        if watched.contains_key(&path) {
            continue;
        }
        if watcher.watch(&path, RecursiveMode::NonRecursive).is_ok() {
            watched.insert(path, ());
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
}
